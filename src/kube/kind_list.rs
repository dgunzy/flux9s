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
use kube::discovery::ApiResource;
use kube::runtime::{WatchStreamExt, watcher};
use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;

use crate::kube::object_status::{ObjectHealth, compute_object_status};
use crate::kube::ownership::{OwnerResolver, Ownership, managed_by_text, ownership};
use crate::models::kinds::{BuiltinColumn, Column, KindSpec};

/// Let a burst of watch events settle before rebuilding rows.
const DEBOUNCE: Duration = Duration::from_millis(200);

/// Most rows sent to the UI per snapshot. Larger lists are truncated (in
/// sorted order) with the total reported, so `:po` on a huge cluster stays
/// responsive — narrow with `:ns` or `/`.
pub const MAX_ROWS: usize = 5_000;

/// Cap on the one-off CRD read for printer columns.
const CRD_LOOKUP_TIMEOUT: Duration = Duration::from_secs(5);

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

/// Compact age (`45s`, `12m`, `3h`, `20d`).
fn format_age(elapsed: chrono::Duration) -> String {
    let secs = elapsed.num_seconds().max(0);
    match secs {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m", s / 60),
        s if s < 86_400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86_400),
    }
}

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
    let cells = columns
        .iter()
        .map(|column| match column {
            Column::Builtin(BuiltinColumn::Status | BuiltinColumn::Health) => {
                status.health.label().to_string()
            }
            Column::Builtin(BuiltinColumn::Message) => {
                sanitize_cell(status.message.lines().next().unwrap_or_default())
            }
            Column::Builtin(BuiltinColumn::Age) => {
                created.map(|t| format_age(now - t)).unwrap_or_default()
            }
            Column::Builtin(BuiltinColumn::ManagedBy) => {
                sanitize_cell(&managed_by_text(&owned, None))
            }
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

fn api_resource(spec: &KindSpec) -> ApiResource {
    ApiResource {
        group: spec.gvk.group.clone(),
        version: spec.gvk.version.clone(),
        api_version: spec.gvk.api_version(),
        kind: spec.gvk.kind.clone(),
        plural: spec.names.plural.clone(),
    }
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

    /// Up to [`MAX_ROWS`] rows, with AGE cells recomputed for `now` and
    /// MANAGED-BY cells filled from resolved owner chains.
    fn snapshot(
        &self,
        columns: &[Column],
        now: chrono::DateTime<chrono::Utc>,
        owners: &OwnerResolver,
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
                            *cell = format_age(now - created);
                        }
                    }
                }
                if let Ownership::Owned(owner) = &row.ownership
                    && let Some(resolved) = owners.get(&owner.uid)
                {
                    let text = sanitize_cell(&managed_by_text(&row.ownership, Some(resolved)));
                    for &i in &managed_cells {
                        if let Some(cell) = row.cells.get_mut(i) {
                            cell.clone_from(&text);
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
    /// Distinct owners still to resolve, with the namespace to look in.
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
    let spec = &request.spec;
    let columns = crd_columns(&client, spec)
        .await
        .unwrap_or_else(|| spec.columns.clone());
    let resource = api_resource(spec);
    let api: Api<DynamicObject> = match (&request.namespace, spec.is_namespaced()) {
        (Some(ns), true) => Api::namespaced_with(client.clone(), ns, &resource),
        _ => Api::all_with(client.clone(), &resource),
    };

    let mut events = watcher(api, watcher::Config::default())
        .backoff(crate::watcher::CappedBackoff::new())
        .boxed();
    let mut store = KindStore::default();
    let mut owners = OwnerResolver::default();
    let mut last_error: Option<String> = None;

    while let Some(first) = events.next().await {
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
                .send(Ok(store.snapshot(&columns, chrono::Utc::now(), &owners)))
                .is_err()
            {
                return; // List closed
            }
            // Resolve owner chains in chunks (cached, capped), publishing
            // progress and folding in any watch events that arrived.
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
                    .send(Ok(store.snapshot(&columns, chrono::Utc::now(), &owners)))
                    .is_err()
                {
                    return;
                }
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

    #[test]
    fn ages_are_compact() {
        assert_eq!(format_age(chrono::Duration::seconds(45)), "45s");
        assert_eq!(format_age(chrono::Duration::seconds(125)), "2m");
        assert_eq!(format_age(chrono::Duration::hours(5)), "5h");
        assert_eq!(format_age(chrono::Duration::days(20)), "20d");
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
        let snapshot = store.snapshot(&s.columns, now, &OwnerResolver::default());
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
                },
            );
        }
        store.apply(&s, &s.columns, watcher::Event::InitDone);
        let snapshot = store.snapshot(&s.columns, chrono::Utc::now(), &OwnerResolver::default());
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
        let before = store.snapshot(&s.columns, now, &owners);
        assert_eq!(
            before.rows[0].cells[1], "ReplicaSet",
            "direct owner meanwhile"
        );

        owners.seed(
            "rs-1",
            Resolved::Manager(Manager::Flux {
                kind: "Kustomization".into(),
                namespace: "flux-system".into(),
                name: "apps".into(),
            }),
        );
        let after = store.snapshot(&s.columns, now, &owners);
        assert_eq!(after.rows[0].cells[1], "Flux ks");
        assert!(store.pending_owners(&owners).is_empty());
    }
}
