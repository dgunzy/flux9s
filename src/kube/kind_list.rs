//! Live lists of any kind (#267).
//!
//! Backs the `:<kind>` browser: one watch for the kind on screen (namespace
//! scoped when a namespace is selected), turned into generic rows by the
//! kind's [`KindSpec`] columns. CRDs get their `additionalPrinterColumns` —
//! the same columns `kubectl get` shows — read once when the list opens.
//! The watch runs only while the list is shown.
//!
//! Hardening (reviewed for #267):
//! - Objects are reduced to display rows **on arrival**; full objects are
//!   never retained. Listing Secrets keeps no secret data in memory, and
//!   large objects (Helm release Secrets, big ConfigMaps) cost one row each.
//! - Every displayed string is cluster data, so it's sanitised: control
//!   characters (terminal escapes) removed, newlines flattened, length capped.
//! - Snapshots are capped at [`MAX_ROWS`]; the UI says when it truncates.

use std::collections::BTreeMap;
use std::time::Duration;

use futures::{FutureExt, StreamExt};
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::Api;
use kube::core::DynamicObject;
use kube::runtime::{WatchStreamExt, watcher};
use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;

use crate::kube::object_status::{ObjectHealth, compute_object_status};
use crate::kube::ownership::{
    Manager, OwnerResolver, Ownership, Resolved, managed_by_text, ownership,
};
use crate::kube::standing::{self, FluxOwners, OrphanReason};
use crate::models::kinds::{BuiltinColumn, Column, KindSpec};
use crate::watcher::ResourceKey;

/// Let a burst of watch events settle before rebuilding rows.
const DEBOUNCE: Duration = Duration::from_millis(200);

/// Most rows sent to the UI per snapshot. Larger lists are truncated (in
/// sorted order) with the total reported, so `:po` on a huge cluster stays
/// responsive — narrow with `:ns` or `/`.
pub const MAX_ROWS: usize = 5_000;

/// Cap on the one-off CRD read for printer columns.
const CRD_LOOKUP_TIMEOUT: Duration = Duration::from_secs(5);

/// How long the first listing may take before the list says so and starts
/// a fresh watch (a request stalled on a dead connection otherwise waits
/// out the client's ~5 minute read timeout). Doubles per retry, capped.
const FIRST_LIST_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_FIRST_LIST_TIMEOUT: Duration = Duration::from_secs(60);

/// Longest displayed cell, in characters.
const MAX_CELL_CHARS: usize = 256;

/// What the list shows: a kind, optionally narrowed to one namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KindListRequest {
    pub spec: KindSpec,
    /// `None` = all namespaces (and always for cluster-scoped kinds).
    pub namespace: Option<String>,
}

/// One row: identity, health, and the spec's column values in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KindRow {
    /// Empty for cluster-scoped objects.
    pub namespace: String,
    pub name: String,
    pub health: ObjectHealth,
    pub cells: Vec<String>,
    /// Creation time, so AGE cells can be refreshed without the object.
    pub created: Option<chrono::DateTime<chrono::Utc>>,
    /// Who manages it, per its own metadata — owner chains are resolved
    /// separately and applied at snapshot time.
    pub ownership: Ownership,
    /// Where the owner chain led, once resolved (set at snapshot time).
    pub resolved: Option<Resolved>,
    /// For rows Flux labels: what the orphan check needs (#274).
    pub flux_probe: Option<FluxProbe>,
    /// Why Flux no longer manages it, once its owner was read (set at
    /// snapshot time).
    pub orphan: Option<OrphanReason>,
}

/// What the orphan check needs from a Flux-labelled object — its owner and
/// a few metadata fields, never the whole object (see the module docs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FluxProbe {
    pub manager: Manager,
    pub owner: ResourceKey,
    pub group: String,
    pub kind: String,
    /// `metadata` with only identity, creation time, and the opt-out marks.
    pub meta: Value,
}

impl FluxProbe {
    fn of(obj: &Value) -> Option<Self> {
        let (manager, owner) = standing::flux_owner(obj)?;
        let namespace = obj
            .pointer("/metadata/namespace")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let pick = |field: &str, keys: &[&str]| -> Value {
            let source = obj.pointer(&format!("/metadata/{field}"));
            keys.iter()
                .filter_map(|key| Some((key.to_string(), source?.get(*key)?.clone())))
                .collect::<serde_json::Map<_, _>>()
                .into()
        };
        let prune = crate::models::flux_resource_kind::labels::PRUNE;
        Some(Self {
            manager,
            owner,
            group: standing::group_of(obj.get("apiVersion")?.as_str()?).to_string(),
            kind: obj.get("kind")?.as_str()?.to_string(),
            meta: serde_json::json!({"metadata": {
                "name": obj.pointer("/metadata/name"),
                "namespace": namespace,
                "creationTimestamp": obj.pointer("/metadata/creationTimestamp"),
                "annotations": pick("annotations", &["helm.sh/hook", prune]),
                "labels": pick("labels", &[prune]),
            }}),
        })
    }
}

/// MANAGED-BY text for an orphan: what it was managed by (`orphaned (ks)`).
fn orphan_label(manager: &Manager) -> String {
    let short = manager.short_label();
    format!(
        "orphaned ({})",
        short.strip_prefix("Flux ").unwrap_or(&short)
    )
}

impl KindRow {
    /// The Flux object managing this row, directly or through its owner
    /// chain — what `t`, `g`, and `m` act on (#266).
    pub fn flux_manager(&self) -> Option<ResourceKey> {
        if self.orphan == Some(OrphanReason::OwnerGone) {
            return None; // nothing to jump to
        }
        let manager = match &self.ownership {
            Ownership::Managed(manager) => Some(manager),
            Ownership::Owned(_) => self.resolved.as_ref().and_then(Resolved::manager),
            Ownership::Unmanaged => None,
        }?;
        manager.flux_key(&self.namespace)
    }
}

/// A full list snapshot. `columns` can differ from the request's spec when
/// the CRD's printer columns were found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KindListSnapshot {
    pub columns: Vec<Column>,
    pub rows: Vec<KindRow>,
    /// Objects in the list before the [`MAX_ROWS`] cap.
    pub total: usize,
}

impl KindListSnapshot {
    /// Whether rows were dropped by the [`MAX_ROWS`] cap.
    pub fn truncated(&self) -> bool {
        self.total > self.rows.len()
    }
}

/// Make cluster-supplied text safe to draw: drop control characters (a
/// value containing `\x1b[...` must not reach the terminal as an escape
/// sequence), flatten line breaks and tabs to spaces, and cap the length.
pub fn sanitize_cell(text: &str) -> String {
    let mut out = String::with_capacity(text.len().min(MAX_CELL_CHARS));
    for (count, c) in text
        .chars()
        .map(|c| {
            if matches!(c, '\n' | '\r' | '\t') {
                ' '
            } else {
                c
            }
        })
        .filter(|c| !c.is_control())
        .enumerate()
    {
        if count == MAX_CELL_CHARS {
            out.push('…');
            break;
        }
        out.push(c);
    }
    out
}

// ---------------------------------------------------------------------------
// JSONPath (the subset printer columns use)
// ---------------------------------------------------------------------------

/// One step of a JSONPath.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Field(String),
    Index(usize),
    /// `[?(@.key=="value")]` — first array element whose `key` equals `value`.
    Filter {
        key: String,
        value: String,
    },
    /// `[*]` — every element.
    All,
}

fn parse_path(path: &str) -> Option<Vec<Step>> {
    let path = path.trim().trim_start_matches('{').trim_end_matches('}');
    let mut steps = Vec::new();
    let mut rest = path.strip_prefix('$').unwrap_or(path);
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix('.') {
            let end = r.find(['.', '[']).unwrap_or(r.len());
            if end > 0 {
                steps.push(Step::Field(r[..end].to_string()));
            }
            rest = &r[end..];
        } else if let Some(r) = rest.strip_prefix('[') {
            let end = r.find(']')?;
            let inner = r[..end].trim();
            rest = &r[end + 1..];
            if inner == "*" {
                steps.push(Step::All);
            } else if let Some(filter) =
                inner.strip_prefix("?(@.").and_then(|f| f.strip_suffix(')'))
            {
                let (key, value) = filter.split_once("==")?;
                steps.push(Step::Filter {
                    key: key.trim().to_string(),
                    value: value.trim().trim_matches(['"', '\'']).to_string(),
                });
            } else if let Ok(index) = inner.parse() {
                steps.push(Step::Index(index));
            } else {
                // ['quoted.key']
                steps.push(Step::Field(inner.trim_matches(['"', '\'']).to_string()));
            }
        } else {
            // Bare leading field without a dot.
            let end = rest.find(['.', '[']).unwrap_or(rest.len());
            steps.push(Step::Field(rest[..end].to_string()));
            rest = &rest[end..];
        }
    }
    Some(steps)
}

fn walk<'a>(values: Vec<&'a Value>, step: &Step) -> Vec<&'a Value> {
    values
        .into_iter()
        .flat_map(|value| -> Vec<&'a Value> {
            match step {
                Step::Field(name) => value.get(name).into_iter().collect(),
                Step::Index(i) => value.get(*i).into_iter().collect(),
                Step::All => value
                    .as_array()
                    .map(|a| a.iter().collect())
                    .unwrap_or_default(),
                Step::Filter { key, value: want } => value
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|item| {
                        item.get(key).is_some_and(|v| match v {
                            Value::String(s) => s == want,
                            // Numbers / bools: compare as JSON (`[?(@.n==2)]`).
                            other => {
                                serde_json::from_str::<Value>(want).ok().as_ref() == Some(other)
                            }
                        })
                    })
                    .take(1)
                    .collect(),
            }
        })
        .collect()
}

fn scalar_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Array(items) => items.iter().map(scalar_text).collect::<Vec<_>>().join(","),
        other => other.to_string(),
    }
}

/// Evaluate a printer-column JSONPath against an object. Unsupported syntax
/// or missing values give an empty string, as `kubectl get` shows `<none>`.
pub fn eval_path(obj: &Value, path: &str) -> String {
    let Some(steps) = parse_path(path) else {
        return String::new();
    };
    let found = steps.iter().fold(vec![obj], walk);
    found
        .iter()
        .map(|v| scalar_text(v))
        .collect::<Vec<_>>()
        .join(",")
}

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

/// Build a row. `obj` must carry `apiVersion`/`kind` — watched objects don't,
/// so [`watch_kind`] stamps them first (kind-specific health keys on it).
pub fn kind_row(
    columns: &[Column],
    obj: &Value,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<KindRow> {
    let name = sanitize_cell(obj.pointer("/metadata/name")?.as_str()?);
    let namespace = sanitize_cell(
        obj.pointer("/metadata/namespace")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    let created = obj
        .pointer("/metadata/creationTimestamp")
        .and_then(Value::as_str)
        .and_then(|c| c.parse::<chrono::DateTime<chrono::Utc>>().ok());
    let status = compute_object_status(obj);
    let owned = ownership(obj);
    let group = standing::group_of(
        obj.get("apiVersion")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    let kind = obj.get("kind").and_then(Value::as_str).unwrap_or_default();
    // Unclaimed objects may still be Kubernetes' own or Helm's storage (#274).
    let managed_text = match &owned {
        Ownership::Unmanaged => standing::system_reason(obj, group, kind)
            .map(|_| "system".to_string())
            .or_else(|| standing::helm_storage_release(obj, group, kind).map(|m| m.short_label()))
            .unwrap_or_else(|| managed_by_text(&owned, None)),
        _ => managed_by_text(&owned, None),
    };
    let flux_probe = FluxProbe::of(obj);
    let cells = columns
        .iter()
        .map(|column| match column {
            Column::Builtin(BuiltinColumn::Status | BuiltinColumn::Health) => {
                status.health.label().to_string()
            }
            Column::Builtin(BuiltinColumn::Message) => {
                sanitize_cell(status.message.lines().next().unwrap_or_default())
            }
            Column::Builtin(BuiltinColumn::Age) => created
                .map(|t| crate::age::compact(now - t))
                .unwrap_or_default(),
            Column::Builtin(BuiltinColumn::ManagedBy) => sanitize_cell(&managed_text),
            Column::JsonPath { path, .. } => sanitize_cell(&eval_path(obj, path)),
        })
        .collect();
    Some(KindRow {
        namespace,
        name,
        health: status.health,
        cells,
        created,
        ownership: owned,
        resolved: None,
        flux_probe,
        orphan: None,
    })
}

/// Columns from a CRD's `additionalPrinterColumns` for `version`: STATUS
/// (kstatus) first, then the CRD's priority-0 columns, then AGE. `None`
/// when the CRD declares none.
pub fn printer_columns(crd: &CustomResourceDefinition, version: &str) -> Option<Vec<Column>> {
    let declared = crd
        .spec
        .versions
        .iter()
        .find(|v| v.name == version)?
        .additional_printer_columns
        .as_ref()?;
    let own_status = declared
        .iter()
        .any(|c| c.priority.unwrap_or(0) == 0 && c.name.eq_ignore_ascii_case("status"));
    let mut columns = vec![Column::Builtin(if own_status {
        BuiltinColumn::Health
    } else {
        BuiltinColumn::Status
    })];
    columns.extend(
        declared
            .iter()
            .filter(|c| c.priority.unwrap_or(0) == 0)
            .filter(|c| !c.name.eq_ignore_ascii_case("age"))
            .map(|c| Column::JsonPath {
                name: c.name.clone(),
                path: c.json_path.clone(),
            }),
    );
    columns.push(Column::Builtin(BuiltinColumn::ManagedBy));
    columns.push(Column::Builtin(BuiltinColumn::Age));
    (columns.len() > 3).then_some(columns)
}

/// Printer columns for a CRD-backed kind, if it has any and we can read the
/// CRD. Built-in kinds (no CRD) keep their spec columns.
async fn crd_columns(client: &kube::Client, spec: &KindSpec) -> Option<Vec<Column>> {
    if spec.gvk.group.is_empty() || !spec.gvk.group.contains('.') {
        return None; // core / built-in groups are never CRDs
    }
    let name = format!("{}.{}", spec.names.plural, spec.gvk.group);
    // Bounded: the list can't start until this returns, and printer columns
    // are a nicety — a slow API server falls back to the generic columns.
    let crds = Api::<CustomResourceDefinition>::all(client.clone());
    match tokio::time::timeout(CRD_LOOKUP_TIMEOUT, crds.get(&name)).await {
        Ok(Ok(crd)) => printer_columns(&crd, &spec.gvk.version),
        Ok(Err(e)) => {
            tracing::debug!("No printer columns for {name}: {e}");
            None
        }
        Err(_) => {
            tracing::debug!("CRD lookup for {name} timed out; using generic columns");
            None
        }
    }
}

/// Stamp apiVersion/kind onto a watched object and build its row.
fn row_for(spec: &KindSpec, columns: &[Column], obj: &DynamicObject) -> Option<KindRow> {
    let mut value = serde_json::to_value(obj).ok()?;
    if let Some(map) = value.as_object_mut() {
        map.insert("apiVersion".into(), spec.gvk.api_version().into());
        map.insert("kind".into(), spec.gvk.kind.clone().into());
    }
    kind_row(columns, &value, chrono::Utc::now())
}

/// The watched objects of one list, **as rows**. Pure, so the watch event
/// handling is unit-testable without a cluster.
#[derive(Debug, Default)]
struct KindStore {
    /// Keyed by (namespace, name) so rows come out sorted.
    rows: BTreeMap<(String, String), KindRow>,
    /// Rows of an in-progress (re)list, swapped in at `InitDone`.
    relist: BTreeMap<(String, String), KindRow>,
    /// Whether the first listing has completed.
    listed: bool,
}

impl KindStore {
    fn key(obj: &DynamicObject) -> (String, String) {
        (
            obj.metadata.namespace.clone().unwrap_or_default(),
            obj.metadata.name.clone().unwrap_or_default(),
        )
    }

    /// Apply one watch event; returns whether the visible list changed.
    fn apply(
        &mut self,
        spec: &KindSpec,
        columns: &[Column],
        event: watcher::Event<DynamicObject>,
    ) -> bool {
        match event {
            watcher::Event::Init => {
                self.relist.clear();
                false
            }
            watcher::Event::InitApply(obj) => {
                if let Some(row) = row_for(spec, columns, &obj) {
                    self.relist.insert(Self::key(&obj), row);
                }
                false
            }
            watcher::Event::InitDone => {
                self.rows = std::mem::take(&mut self.relist);
                self.listed = true;
                true
            }
            watcher::Event::Apply(obj) => {
                match row_for(spec, columns, &obj) {
                    Some(row) => self.rows.insert(Self::key(&obj), row),
                    None => self.rows.remove(&Self::key(&obj)),
                };
                true
            }
            watcher::Event::Delete(obj) => self.rows.remove(&Self::key(&obj)).is_some(),
        }
    }

    /// Up to [`MAX_ROWS`] rows, with AGE cells recomputed for `now`,
    /// MANAGED-BY cells filled from resolved owner chains, and Flux-labelled
    /// rows checked against their owners' inventories (#274).
    fn snapshot(
        &self,
        columns: &[Column],
        now: chrono::DateTime<chrono::Utc>,
        owners: &OwnerResolver,
        flux_owners: &FluxOwners,
    ) -> KindListSnapshot {
        let cells_of = |wanted: BuiltinColumn| -> Vec<usize> {
            columns
                .iter()
                .enumerate()
                .filter(|(_, c)| **c == Column::Builtin(wanted))
                .map(|(i, _)| i)
                .collect()
        };
        let age_cells = cells_of(BuiltinColumn::Age);
        let managed_cells = cells_of(BuiltinColumn::ManagedBy);
        let rows = self
            .rows
            .values()
            .take(MAX_ROWS)
            .map(|row| {
                let mut row = row.clone();
                if let Some(created) = row.created {
                    for &i in &age_cells {
                        if let Some(cell) = row.cells.get_mut(i) {
                            *cell = crate::age::compact(now - created);
                        }
                    }
                }
                if let Ownership::Owned(owner) = &row.ownership
                    && let Some(resolved) = owners.get(&owner.uid)
                {
                    row.resolved = Some(resolved.clone());
                    let text = sanitize_cell(&managed_by_text(&row.ownership, Some(resolved)));
                    for &i in &managed_cells {
                        if let Some(cell) = row.cells.get_mut(i) {
                            cell.clone_from(&text);
                        }
                    }
                }
                if let Some(probe) = &row.flux_probe {
                    row.orphan = flux_owners.get(&probe.owner).and_then(|owner| {
                        standing::orphan_reason(&probe.meta, &probe.group, &probe.kind, owner, now)
                    });
                    if row.orphan.is_some() {
                        let text = orphan_label(&probe.manager);
                        for &i in &managed_cells {
                            if let Some(cell) = row.cells.get_mut(i) {
                                cell.clone_from(&text);
                            }
                        }
                    }
                }
                row
            })
            .collect();
        KindListSnapshot {
            columns: columns.to_vec(),
            rows,
            total: self.rows.len(),
        }
    }
}

impl KindStore {
    /// The Flux owners the listed rows' labels name.
    fn flux_owner_keys(&self) -> std::collections::HashSet<ResourceKey> {
        self.rows
            .values()
            .filter_map(|row| row.flux_probe.as_ref().map(|p| p.owner.clone()))
            .collect()
    }

    /// Distinct owners still to resolve, with the namespace to look in.
    /// UIDs of the direct owners of the rows currently listed.
    fn owner_uids(&self) -> std::collections::HashSet<String> {
        self.rows
            .values()
            .filter_map(|row| match &row.ownership {
                Ownership::Owned(owner) => Some(owner.uid.clone()),
                _ => None,
            })
            .collect()
    }

    fn pending_owners(
        &self,
        owners: &OwnerResolver,
    ) -> Vec<(crate::kube::ownership::OwnerRef, String)> {
        let mut seen = std::collections::HashSet::new();
        self.rows
            .values()
            .filter_map(|row| match &row.ownership {
                Ownership::Owned(owner)
                    if owners.wants(&owner.uid) && seen.insert(owner.uid.clone()) =>
                {
                    Some((owner.clone(), row.namespace.clone()))
                }
                _ => None,
            })
            .collect()
    }
}

/// Owners resolved between snapshots, so a big list shows progress.
const OWNER_CHUNK: usize = 50;

/// The message shown when the list can't be read.
fn list_error_message(spec: &KindSpec, namespace: &Option<String>, error: &str) -> String {
    let forbidden = crate::kube::api::is_forbidden_error(error);
    match (forbidden, namespace, spec.is_namespaced()) {
        (true, None, true) => format!(
            "Forbidden: cannot list {} in all namespaces — try :ns <namespace>",
            spec.names.plural
        ),
        (true, _, _) => format!("Forbidden: cannot list {}", spec.names.plural),
        (false, _, _) => format!("Cannot list {} (retrying): {error}", spec.names.plural),
    }
}

/// Keep a kind's list live: send a snapshot after the initial listing and
/// after every settled burst of changes. Errors before the first listing
/// are reported (once per distinct message) while the watch keeps retrying;
/// a forbidden list is reported and ends the task. Ends when the receiver is
/// dropped (list closed).
pub async fn watch_kind(
    client: kube::Client,
    request: KindListRequest,
    tx: UnboundedSender<anyhow::Result<KindListSnapshot>>,
) {
    watch_kind_with(client, request, tx, FIRST_LIST_TIMEOUT).await;
}

async fn watch_kind_with(
    client: kube::Client,
    request: KindListRequest,
    tx: UnboundedSender<anyhow::Result<KindListSnapshot>>,
    first_list_timeout: Duration,
) {
    let spec = &request.spec;
    let columns = crd_columns(&client, spec)
        .await
        .unwrap_or_else(|| spec.columns.clone());
    let resource = spec.api_resource();
    let api: Api<DynamicObject> = match (&request.namespace, spec.is_namespaced()) {
        (Some(ns), true) => Api::namespaced_with(client.clone(), ns, &resource),
        _ => Api::all_with(client.clone(), &resource),
    };

    let start = || {
        watcher(api.clone(), watcher::Config::default())
            .backoff(crate::watcher::CappedBackoff::new())
            .boxed()
    };
    let mut events = start();
    let mut store = KindStore::default();
    let mut owners = OwnerResolver::default();
    let mut flux_owners = FluxOwners::default();
    // Owners are re-read on a timer too: an object turns orphaned when its
    // owner's inventory changes, with no event for the object itself.
    let mut owner_tick = tokio::time::interval(standing::OWNER_TTL);
    owner_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_error: Option<String> = None;
    let mut first_list_wait = first_list_timeout;

    loop {
        let first = if store.listed {
            tokio::select! {
                event = events.next() => match event {
                    Some(event) => event,
                    None => break,
                },
                _ = owner_tick.tick() => {
                    let keys = store.flux_owner_keys();
                    flux_owners.retain(&keys);
                    if flux_owners.refresh(&client, &keys).await
                        && tx
                            .send(Ok(store.snapshot(&columns, chrono::Utc::now(), &owners, &flux_owners)))
                            .is_err()
                    {
                        return; // List closed
                    }
                    continue;
                }
            }
        } else {
            // Until the first listing lands, a stall is visible and retried
            // rather than an endless "Listing…".
            match tokio::time::timeout(first_list_wait, events.next()).await {
                Ok(Some(event)) => event,
                Ok(None) => break,
                Err(_) => {
                    let message = format!(
                        "Still listing {} after {}s — retrying (the API server is slow or unreachable)",
                        spec.names.plural,
                        first_list_wait.as_secs()
                    );
                    tracing::warn!("{message}");
                    if tx.send(Err(anyhow::anyhow!(message))).is_err() {
                        return; // List closed
                    }
                    events = start();
                    first_list_wait = (first_list_wait * 2).min(MAX_FIRST_LIST_TIMEOUT);
                    continue;
                }
            }
        };
        let mut batch = vec![first];
        tokio::time::sleep(DEBOUNCE).await;
        while let Some(Some(more)) = events.next().now_or_never() {
            batch.push(more);
        }
        let mut changed = false;
        for event in batch {
            match event {
                Ok(event) => changed |= store.apply(spec, &columns, event),
                Err(e) => {
                    let text = e.to_string();
                    let forbidden = crate::kube::api::is_forbidden_error(&text);
                    let message = list_error_message(spec, &request.namespace, &text);
                    tracing::debug!("{message}");
                    // Surface errors the user would otherwise never see:
                    // before the first listing the view is still loading.
                    if (forbidden || !store.listed) && last_error.as_deref() != Some(&message) {
                        if tx.send(Err(anyhow::anyhow!(message.clone()))).is_err() {
                            return;
                        }
                        last_error = Some(message);
                    }
                    if forbidden {
                        return; // Retrying can't fix RBAC.
                    }
                }
            }
        }
        if store.listed && changed {
            last_error = None;
            if tx
                .send(Ok(store.snapshot(
                    &columns,
                    chrono::Utc::now(),
                    &owners,
                    &flux_owners,
                )))
                .is_err()
            {
                return; // List closed
            }
            // Resolve owner chains in chunks (cached, budgeted per pass),
            // publishing progress and folding in any watch events that
            // arrived. Owners over this pass's budget resolve on the next.
            owners.retain_owners(&store.owner_uids());
            owners.begin_pass();
            loop {
                let pending = store.pending_owners(&owners);
                if pending.is_empty() {
                    break;
                }
                owners
                    .resolve_many(&client, pending.into_iter().take(OWNER_CHUNK).collect())
                    .await;
                while let Some(Some(event)) = events.next().now_or_never() {
                    if let Ok(event) = event {
                        store.apply(spec, &columns, event);
                    }
                }
                if tx
                    .send(Ok(store.snapshot(
                        &columns,
                        chrono::Utc::now(),
                        &owners,
                        &flux_owners,
                    )))
                    .is_err()
                {
                    return;
                }
            }
            // Then the Flux owners the rows name (one read per owner).
            let keys = store.flux_owner_keys();
            flux_owners.retain(&keys);
            if flux_owners.refresh(&client, &keys).await
                && tx
                    .send(Ok(store.snapshot(
                        &columns,
                        chrono::Utc::now(),
                        &owners,
                        &flux_owners,
                    )))
                    .is_err()
            {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::kinds::{Gvk, KindScope};
    use serde_json::json;

    #[test]
    fn eval_path_handles_printer_column_forms() {
        let obj = json!({
            "spec": {"replicas": 3, "tags": ["a", "b"], "ports": [{"port": 80, "name": "http"}, {"port": 443, "name": "https"}]},
            "status": {"conditions": [
                {"type": "Progressing", "status": "True"},
                {"type": "Ready", "status": "False", "message": "waiting"}
            ]},
            "metadata": {"labels": {"app.kubernetes.io/name": "web"}}
        });
        assert_eq!(eval_path(&obj, ".spec.replicas"), "3");
        assert_eq!(eval_path(&obj, ".spec.tags"), "a,b");
        assert_eq!(eval_path(&obj, ".spec.tags[1]"), "b");
        assert_eq!(
            eval_path(&obj, r#".status.conditions[?(@.type=="Ready")].status"#),
            "False"
        );
        assert_eq!(
            eval_path(&obj, ".status.conditions[*].type"),
            "Progressing,Ready"
        );
        assert_eq!(
            eval_path(&obj, ".metadata.labels['app.kubernetes.io/name']"),
            "web"
        );
        assert_eq!(eval_path(&obj, ".spec.ports[?(@.port==443)].name"), "https");
        assert_eq!(eval_path(&obj, ".status.missing"), "");
        assert_eq!(eval_path(&obj, "{.spec.replicas}"), "3");
    }

    fn spec() -> KindSpec {
        KindSpec::generic(
            Gvk {
                group: "apps".into(),
                version: "v1".into(),
                kind: "Deployment".into(),
            },
            KindScope::Namespaced,
            "deployments".into(),
            vec!["deploy".into()],
        )
    }

    #[test]
    fn rows_use_kind_specific_health_and_columns() {
        let now: chrono::DateTime<chrono::Utc> = "2026-09-29T12:00:00Z".parse().unwrap();
        let obj = json!({
            "apiVersion": "apps/v1",
            "kind": "Deployment",
            "metadata": {"name": "web", "namespace": "apps", "creationTimestamp": "2026-09-29T09:00:00Z"},
            "spec": {"replicas": 2},
            "status": {"replicas": 2, "updatedReplicas": 2, "readyReplicas": 1, "availableReplicas": 1}
        });
        let row = kind_row(&spec().columns, &obj, now).unwrap();
        assert_eq!(row.namespace, "apps");
        assert_eq!(row.health, ObjectHealth::InProgress);
        assert_eq!(row.cells, ["InProgress", "-", "3h", "Ready: 1/2"]);
        assert!(row.created.is_some());
    }

    #[test]
    fn watched_objects_get_kind_stamped_before_health() {
        // Watch items omit apiVersion/kind; without stamping, Deployment
        // rules would be skipped and this would read Current.
        let obj: DynamicObject = serde_json::from_value(json!({
            "metadata": {"name": "web", "namespace": "apps"},
            "spec": {"replicas": 2},
            "status": {"replicas": 2, "updatedReplicas": 2, "readyReplicas": 0, "availableReplicas": 0}
        }))
        .unwrap();
        let s = spec();
        let row = row_for(&s, &s.columns, &obj).unwrap();
        assert_eq!(row.health, ObjectHealth::InProgress);
    }

    #[test]
    fn printer_columns_come_from_the_served_version() {
        let crd: CustomResourceDefinition = serde_json::from_value(json!({
            "metadata": {"name": "clusters.postgresql.cnpg.io"},
            "spec": {
                "group": "postgresql.cnpg.io",
                "names": {"kind": "Cluster", "plural": "clusters"},
                "scope": "Namespaced",
                "versions": [{
                    "name": "v1", "served": true, "storage": true,
                    "additionalPrinterColumns": [
                        {"name": "Age", "type": "date", "jsonPath": ".metadata.creationTimestamp"},
                        {"name": "Instances", "type": "integer", "jsonPath": ".status.instances"},
                        {"name": "Status", "type": "string", "jsonPath": ".status.phase"},
                        {"name": "Primary", "type": "string", "jsonPath": ".status.currentPrimary", "priority": 1}
                    ]
                }]
            }
        }))
        .unwrap();
        let columns = printer_columns(&crd, "v1").unwrap();
        let headers: Vec<_> = columns.iter().map(Column::header).collect();
        assert_eq!(
            headers,
            ["HEALTH", "INSTANCES", "STATUS", "MANAGED-BY", "AGE"],
            "the CRD's own Status column keeps its name; kstatus becomes HEALTH"
        );
        assert!(printer_columns(&crd, "v2").is_none());
    }

    fn dynamic(ns: &str, name: &str, ready: i64) -> DynamicObject {
        serde_json::from_value(json!({
            "metadata": {"name": name, "namespace": ns, "creationTimestamp": "2026-09-29T09:00:00Z"},
            "spec": {"replicas": 1},
            "status": {"replicas": 1, "updatedReplicas": 1, "readyReplicas": ready, "availableReplicas": ready},
            "data": {"secret": "c2VjcmV0"}
        }))
        .unwrap()
    }

    #[test]
    fn store_publishes_only_after_the_first_listing_and_tracks_changes() {
        let s = spec();
        let mut store = KindStore::default();
        assert!(!store.apply(&s, &s.columns, watcher::Event::Init));
        assert!(!store.apply(
            &s,
            &s.columns,
            watcher::Event::InitApply(dynamic("a", "web", 1))
        ));
        assert!(!store.listed, "nothing to show mid-listing");
        assert!(store.apply(&s, &s.columns, watcher::Event::InitDone));
        assert_eq!(store.rows.len(), 1);

        assert!(store.apply(
            &s,
            &s.columns,
            watcher::Event::Apply(dynamic("a", "api", 0))
        ));
        let now: chrono::DateTime<chrono::Utc> = "2026-09-29T12:00:00Z".parse().unwrap();
        let snapshot = store.snapshot(
            &s.columns,
            now,
            &OwnerResolver::default(),
            &FluxOwners::default(),
        );
        let names: Vec<_> = snapshot.rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["api", "web"], "sorted by namespace/name");
        assert_eq!(snapshot.rows[0].health, ObjectHealth::InProgress);
        assert_eq!(
            snapshot.rows[0].cells[2], "3h",
            "age computed at snapshot time"
        );

        assert!(store.apply(
            &s,
            &s.columns,
            watcher::Event::Delete(dynamic("a", "api", 0))
        ));
        assert!(!store.apply(
            &s,
            &s.columns,
            watcher::Event::Delete(dynamic("a", "gone", 0))
        ));
        assert_eq!(store.rows.len(), 1);
    }

    #[test]
    fn relist_replaces_the_store_so_missed_deletes_disappear() {
        let s = spec();
        let mut store = KindStore::default();
        for event in [
            watcher::Event::Init,
            watcher::Event::InitApply(dynamic("a", "old", 1)),
            watcher::Event::InitDone,
            // Reconnect: `old` was deleted while disconnected.
            watcher::Event::Init,
            watcher::Event::InitApply(dynamic("a", "new", 1)),
        ] {
            store.apply(&s, &s.columns, event);
        }
        assert_eq!(
            store.rows.len(),
            1,
            "old list shown until the relist completes"
        );
        store.apply(&s, &s.columns, watcher::Event::InitDone);
        let names: Vec<_> = store.rows.values().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["new"]);
    }

    #[test]
    fn store_keeps_rows_not_objects() {
        // Rows carry only displayed text — secret data never lands in memory.
        let s = spec();
        let mut store = KindStore::default();
        store.apply(
            &s,
            &s.columns,
            watcher::Event::Apply(dynamic("a", "web", 1)),
        );
        let debug = format!("{:?}", store.rows);
        assert!(!debug.contains("c2VjcmV0"));
    }

    #[test]
    fn snapshots_cap_rows_and_report_the_total() {
        let s = spec();
        let mut store = KindStore::default();
        for i in 0..(MAX_ROWS + 3) {
            store.relist.insert(
                (String::new(), format!("obj-{i:05}")),
                KindRow {
                    namespace: String::new(),
                    name: format!("obj-{i:05}"),
                    health: ObjectHealth::Current,
                    cells: vec![],
                    created: None,
                    ownership: Ownership::Unmanaged,
                    resolved: None,
                    flux_probe: None,
                    orphan: None,
                },
            );
        }
        store.apply(&s, &s.columns, watcher::Event::InitDone);
        let snapshot = store.snapshot(
            &s.columns,
            chrono::Utc::now(),
            &OwnerResolver::default(),
            &FluxOwners::default(),
        );
        assert_eq!(snapshot.rows.len(), MAX_ROWS);
        assert_eq!(snapshot.total, MAX_ROWS + 3);
        assert!(snapshot.truncated());
    }

    #[test]
    fn cells_are_sanitised_for_the_terminal() {
        assert_eq!(sanitize_cell("ok\u{1b}[31mred\u{7}"), "ok[31mred");
        assert_eq!(sanitize_cell("line1\nline2\ttab"), "line1 line2 tab");
        let long = "x".repeat(MAX_CELL_CHARS + 50);
        let capped = sanitize_cell(&long);
        assert_eq!(capped.chars().count(), MAX_CELL_CHARS + 1);
        assert!(capped.ends_with('…'));

        // Printer-column values from the cluster go through it.
        let now = chrono::Utc::now();
        let obj = json!({"metadata": {"name": "x"}, "spec": {"v": "a\u{1b}]0;pwned\u{7}b"}});
        let columns = vec![Column::JsonPath {
            name: "V".into(),
            path: ".spec.v".into(),
        }];
        let row = kind_row(&columns, &obj, now).unwrap();
        assert_eq!(row.cells, ["a]0;pwnedb"]);
    }

    #[test]
    fn error_messages_point_at_the_fix() {
        let s = spec();
        let all = list_error_message(&s, &None, "ApiError: forbidden (403)");
        assert!(all.contains("in all namespaces — try :ns"));
        let one = list_error_message(&s, &Some("apps".into()), "ApiError: forbidden (403)");
        assert_eq!(one, "Forbidden: cannot list deployments");
        let other = list_error_message(
            &s,
            &None,
            "the server could not find the requested resource",
        );
        assert!(other.starts_with("Cannot list deployments (retrying)"));
    }

    #[test]
    fn snapshot_fills_managed_by_from_resolved_owner_chains() {
        use crate::kube::ownership::{Manager, Resolved};
        let s = spec();
        let mut store = KindStore::default();
        let pod: DynamicObject = serde_json::from_value(json!({
            "metadata": {"name": "web-abc", "namespace": "apps", "ownerReferences": [
                {"apiVersion": "apps/v1", "kind": "ReplicaSet", "name": "web-7b5", "uid": "rs-1", "controller": true}
            ]}
        }))
        .unwrap();
        store.apply(&s, &s.columns, watcher::Event::Apply(pod));
        store.listed = true;
        let now = chrono::Utc::now();

        let mut owners = OwnerResolver::default();
        assert_eq!(store.pending_owners(&owners).len(), 1);
        let before = store.snapshot(&s.columns, now, &owners, &FluxOwners::default());
        assert_eq!(
            before.rows[0].cells[1], "ReplicaSet",
            "direct owner meanwhile"
        );
        assert_eq!(before.rows[0].flux_manager(), None, "not known yet");

        owners.seed(
            "rs-1",
            Resolved::Manager(Manager::Flux {
                kind: crate::models::FluxResourceKind::Kustomization,
                namespace: "flux-system".into(),
                name: "apps".into(),
            }),
        );
        let after = store.snapshot(&s.columns, now, &owners, &FluxOwners::default());
        assert_eq!(after.rows[0].cells[1], "Flux ks");
        assert_eq!(
            after.rows[0].flux_manager(),
            Some(ResourceKey::new("Kustomization", "flux-system", "apps")),
            "t / g / m act on the manager the chain reached (#266)"
        );
        assert!(store.pending_owners(&owners).is_empty());
    }

    #[test]
    fn flux_labelled_rows_turn_orphaned_once_their_owner_is_read() {
        use crate::kube::standing::{OwnerSnapshot, OwnerState};
        let s = spec();
        let mut store = KindStore::default();
        let labelled = |name: &str| -> DynamicObject {
            serde_json::from_value(json!({"metadata": {
                "name": name, "namespace": "apps", "creationTimestamp": "2026-09-01T00:00:00Z",
                "labels": {"kustomize.toolkit.fluxcd.io/name": "apps", "kustomize.toolkit.fluxcd.io/namespace": "flux-system"}
            }}))
            .unwrap()
        };
        store.apply(&s, &s.columns, watcher::Event::Apply(labelled("kept")));
        store.apply(&s, &s.columns, watcher::Event::Apply(labelled("listed")));
        store.listed = true;
        let owner = ResourceKey::new("Kustomization", "flux-system", "apps");
        assert_eq!(store.flux_owner_keys(), [owner.clone()].into());

        let now = chrono::Utc::now();
        let unread = store.snapshot(
            &s.columns,
            now,
            &OwnerResolver::default(),
            &FluxOwners::default(),
        );
        assert!(
            unread
                .rows
                .iter()
                .all(|r| r.orphan.is_none() && r.cells[1] == "Flux ks")
        );

        let mut owners = FluxOwners::default();
        owners.seed(
            owner,
            OwnerState::Present(OwnerSnapshot {
                inventory: Some(["apps_listed_apps_Deployment".to_string()].into()),
                settled: true,
                prune: Some(false),
                generated: None,
            }),
        );
        let read = store.snapshot(&s.columns, now, &OwnerResolver::default(), &owners);
        let kept = &read.rows[0];
        assert_eq!(kept.name, "kept");
        assert_eq!(kept.cells[1], "orphaned (ks)");
        assert!(
            kept.flux_manager().is_some(),
            "the owner exists: m goes to it"
        );
        assert_eq!(read.rows[1].cells[1], "Flux ks", "listed in the inventory");
    }

    #[test]
    fn kubernetes_builtins_and_helm_storage_say_so() {
        let now = chrono::Utc::now();
        let columns = [Column::Builtin(BuiltinColumn::ManagedBy)];
        let row = |obj: Value| kind_row(&columns, &obj, now).unwrap().cells[0].clone();
        assert_eq!(
            row(
                json!({"apiVersion": "v1", "kind": "ConfigMap", "metadata": {"name": "kube-root-ca.crt", "namespace": "apps"}})
            ),
            "system"
        );
        assert_eq!(
            row(
                json!({"apiVersion": "v1", "kind": "Secret", "metadata": {"name": "sh.helm.release.v1.web.v1", "namespace": "apps",
                "labels": {"owner": "helm", "name": "web"}}})
            ),
            "Helm"
        );
        assert_eq!(
            row(
                json!({"apiVersion": "v1", "kind": "ConfigMap", "metadata": {"name": "notes", "namespace": "apps"}})
            ),
            "-"
        );
    }

    #[test]
    fn owners_over_budget_resolve_on_a_later_pass_and_departed_owners_are_pruned() {
        use crate::kube::ownership::Resolved;
        let s = spec();
        let mut store = KindStore::default();
        let pod = |name: &str, uid: &str| -> DynamicObject {
            serde_json::from_value(json!({
                "metadata": {"name": name, "namespace": "apps", "ownerReferences": [
                    {"apiVersion": "apps/v1", "kind": "ReplicaSet", "name": format!("rs-{uid}"), "uid": uid, "controller": true}
                ]}
            }))
            .unwrap()
        };
        store.apply(&s, &s.columns, watcher::Event::Apply(pod("a", "rs-a")));
        store.listed = true;
        let mut owners = OwnerResolver::default();
        owners.seed(
            "rs-a",
            Resolved::Owner {
                kind: "Deployment".into(),
                name: "a".into(),
            },
        );

        // A long-open list whose budget ran out: a new owner appears...
        owners.exhaust_budget();
        store.apply(&s, &s.columns, watcher::Event::Apply(pod("b", "rs-b")));
        assert!(
            store.pending_owners(&owners).is_empty(),
            "no budget this pass"
        );
        // ...and the next batch's pass picks it up rather than giving up.
        owners.retain_owners(&store.owner_uids());
        owners.begin_pass();
        let pending = store.pending_owners(&owners);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].0.uid, "rs-b");

        // Rolled-out owners are forgotten once no row references them.
        store.apply(&s, &s.columns, watcher::Event::Delete(pod("a", "rs-a")));
        owners.retain_owners(&store.owner_uids());
        assert!(owners.get("rs-a").is_none());
    }

    /// A server that accepts connections and never answers.
    async fn black_hole_client() -> kube::Client {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((socket, _)) = listener.accept().await {
                held.push(socket);
            }
        });
        let config = kube::Config::new(format!("http://{addr}").parse().unwrap());
        kube::Client::try_from(config).unwrap()
    }

    #[tokio::test]
    async fn a_stalled_first_listing_is_reported_and_retried() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let request = KindListRequest {
            spec: spec(),
            namespace: Some("apps".into()),
        };
        let task = tokio::spawn(watch_kind_with(
            black_hole_client().await,
            request,
            tx,
            Duration::from_millis(200),
        ));
        let first = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("no report of the stall")
            .unwrap();
        let message = format!("{:#}", first.unwrap_err());
        assert!(message.contains("Still listing"), "{message}");
        assert!(
            message.contains("0s") || message.contains("retrying"),
            "{message}"
        );
        // It keeps retrying (with a longer wait) instead of giving up.
        let second = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("no retry")
            .unwrap();
        assert!(second.is_err());
        task.abort();
    }
}
