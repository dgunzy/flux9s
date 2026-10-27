//! Whether Flux still manages an object, and what does if it doesn't (#274).
//!
//! Builds on [`crate::kube::ownership`], which reads *who claims* an object,
//! to answer the questions only a Flux tool can:
//!
//! - **Orphaned** — Flux labels name a Kustomization / HelmRelease /
//!   ResourceSet / FluxInstance that is gone, or that no longer lists the
//!   object in its `status.inventory` (removed from source with pruning
//!   off). Flux left it behind.
//! - **Unmanaged** — nothing claims it: no Flux, Argo CD, or Helm marks and
//!   no owner reference. Its `managedFields` usually say how it got there
//!   (`kubectl`, `Terraform`, …).
//! - **System** — Kubernetes made it (`kube-root-ca.crt`, the `default`
//!   ServiceAccount, …), so it is never reported as unmanaged.
//!
//! The rule throughout is *unknown over wrong*: an object is only called
//! orphaned when the owner was read and settled. An owner that can't be read
//! (RBAC, timeout, another cluster), hasn't finished reconciling, or keeps no
//! inventory leaves the object as plain Flux-managed.
//!
//! Patterns follow the tools that answer the same question elsewhere:
//! gitops-zombies (Flux: labels + inventory, skip owned objects, Helm storage
//! and ServiceAccount token Secrets) and Argo CD's orphaned resources (top
//! level only, built-in exclusions, `group`/`kind`/`name` glob ignore rules).

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use kube::Api;
use kube::core::DynamicObject;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::kube::conditions;
use crate::kube::ownership::{Manager, Ownership, ownership};
use crate::models::FluxResourceKind;
use crate::models::flux_resource_kind::labels;
use crate::watcher::ResourceKey;

/// Objects younger than this are never judged "not in inventory": Flux
/// applies an object before it records it, so a brand-new object would
/// otherwise flash as orphaned.
pub const ORPHAN_GRACE: chrono::Duration = chrono::Duration::minutes(2);
/// How long a read owner is trusted before it's read again.
pub const OWNER_TTL: Duration = Duration::from_secs(30);
/// Per-owner read cap.
const OWNER_TIMEOUT: Duration = Duration::from_secs(5);
/// Owner reads in flight at once.
const OWNER_CONCURRENCY: usize = 8;

/// Namespaces left out of unmanaged sweeps unless `ownership.ignoreNamespaces`
/// says otherwise: Kubernetes' own, where almost nothing is GitOps-managed.
pub const DEFAULT_IGNORED_NAMESPACES: &[&str] = &["kube-system", "kube-public", "kube-node-lease"];

/// The id Flux inventories record an object under:
/// `<namespace>_<name>_<group>_<kind>` (empty namespace for cluster-scoped
/// objects, empty group for the core API).
pub fn inventory_id(namespace: &str, name: &str, group: &str, kind: &str) -> String {
    format!("{namespace}_{name}_{group}_{kind}")
}

/// The API group of an `apiVersion` (`apps/v1` → `apps`, `v1` → ``).
pub fn group_of(api_version: &str) -> &str {
    api_version.split_once('/').map_or("", |(group, _)| group)
}

/// The source `HelmRelease.status.helmChart` names: `<ns>/<name>` for a
/// generated HelmChart, or `<kind>/<ns>/<name>` (Flux 2.10+, for a
/// generated OCIRepository).
pub fn generated_chart_source(reference: &str, default_ns: &str) -> ResourceKey {
    let helm_chart = FluxResourceKind::HelmChart.as_str();
    match reference.splitn(3, '/').collect::<Vec<_>>().as_slice() {
        [kind, ns, name] => ResourceKey::new(*kind, *ns, *name),
        [ns, name] => ResourceKey::new(helm_chart, *ns, *name),
        _ => ResourceKey::new(helm_chart, default_ns, reference),
    }
}

// ---------------------------------------------------------------------------
// Owners
// ---------------------------------------------------------------------------

/// What a Flux owner says about the objects it manages.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OwnerSnapshot {
    /// `status.inventory` ids; `None` when the owner keeps none (a
    /// helm-controller before inventories, or never reconciled).
    pub inventory: Option<HashSet<String>>,
    /// Ready, not reconciling, and its current generation observed — only
    /// then is its inventory complete.
    pub settled: bool,
    /// `spec.prune` (Kustomization).
    pub prune: Option<bool>,
    /// The source a HelmRelease generated for its chart (`status.helmChart`):
    /// Flux labels it, but no inventory lists it.
    pub generated: Option<ResourceKey>,
}

impl OwnerSnapshot {
    /// Read from the owner object.
    pub fn from_object(owner: &Value) -> Self {
        let inventory = owner.pointer("/status/inventory").map(|inventory| {
            inventory
                .get("entries")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|entry| entry.get("id").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        });
        let generation = owner
            .pointer("/metadata/generation")
            .and_then(Value::as_i64);
        // The Flux toolkit records observedGeneration in status; the Flux
        // Operator (ResourceSet, FluxInstance) on its conditions. Both must
        // be current where present.
        let observed = [
            owner.pointer("/status/observedGeneration"),
            conditions::find(owner, conditions::READY).and_then(|c| c.get("observedGeneration")),
        ];
        let current = match generation {
            Some(generation) => observed
                .iter()
                .flatten()
                .filter_map(|v| v.as_i64())
                .all(|observed| observed >= generation),
            None => true,
        };
        let settled = conditions::is_true(owner, conditions::READY)
            && !conditions::is_true(owner, conditions::RECONCILING)
            && current;
        let namespace = owner
            .pointer("/metadata/namespace")
            .and_then(Value::as_str)
            .unwrap_or_default();
        Self {
            inventory,
            settled,
            prune: owner.pointer("/spec/prune").and_then(Value::as_bool),
            generated: owner
                .pointer("/status/helmChart")
                .and_then(Value::as_str)
                .map(|reference| generated_chart_source(reference, namespace)),
        }
    }
}

/// A Flux owner as last read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnerState {
    Present(OwnerSnapshot),
    /// Read, and it doesn't exist.
    Gone,
    /// Couldn't be read (RBAC, timeout, unknown kind) — no verdict.
    Unknown,
}

/// Why Flux no longer manages an object it labelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrphanReason {
    /// The Flux object its labels name doesn't exist.
    OwnerGone,
    /// The owner exists but its inventory doesn't list it.
    /// `prune_disabled`: pruning was off, which is why it was left behind.
    NotInInventory { prune_disabled: bool },
}

impl OrphanReason {
    pub fn describe(self, manager: &Manager) -> String {
        match self {
            OrphanReason::OwnerGone => format!("{} no longer exists", manager.describe()),
            OrphanReason::NotInInventory { prune_disabled } => {
                let why = if prune_disabled {
                    " (pruning disabled, so Flux left it behind)"
                } else {
                    ""
                };
                format!("not in the inventory of {}{why}", manager.describe())
            }
        }
    }
}

fn string_at<'a>(obj: &'a Value, pointer: &str) -> Option<&'a str> {
    obj.pointer(pointer).and_then(Value::as_str)
}

fn annotation<'a>(obj: &'a Value, key: &str) -> Option<&'a str> {
    obj.pointer("/metadata/annotations")
        .and_then(|a| a.get(key))
        .and_then(Value::as_str)
}

fn label<'a>(obj: &'a Value, key: &str) -> Option<&'a str> {
    obj.pointer("/metadata/labels")
        .and_then(|l| l.get(key))
        .and_then(Value::as_str)
}

/// Whether an object opted out of Flux garbage collection.
fn prune_disabled(obj: &Value) -> bool {
    annotation(obj, labels::PRUNE) == Some("disabled")
        || label(obj, labels::PRUNE) == Some("disabled")
}

/// Whether `obj` is younger than [`ORPHAN_GRACE`] at `now`.
fn is_new(obj: &Value, now: chrono::DateTime<chrono::Utc>) -> bool {
    string_at(obj, "/metadata/creationTimestamp")
        .and_then(|t| t.parse::<chrono::DateTime<chrono::Utc>>().ok())
        .is_some_and(|created| now - created < ORPHAN_GRACE)
}

/// Why a Flux-labelled object is orphaned, if it is. `None` means managed,
/// or that the owner can't tell us (unknown over wrong).
pub fn orphan_reason(
    obj: &Value,
    group: &str,
    kind: &str,
    owner: &OwnerState,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<OrphanReason> {
    // Never for a brand-new object (Flux may not have recorded it, or its
    // owner may be appearing too) or a Helm hook (never in a release
    // inventory) — whatever state the owner is in.
    if is_new(obj, now) || annotation(obj, "helm.sh/hook").is_some() {
        return None;
    }
    let snapshot = match owner {
        OwnerState::Gone => return Some(OrphanReason::OwnerGone),
        OwnerState::Unknown => return None,
        OwnerState::Present(snapshot) => snapshot,
    };
    // Only a settled owner's inventory is complete. An inventory that is
    // recorded but empty is complete too: Flux writes it whether or not it
    // prunes, so everything still labelled for it was left behind.
    let inventory = snapshot.inventory.as_ref()?;
    if !snapshot.settled {
        return None;
    }
    let namespace = string_at(obj, "/metadata/namespace").unwrap_or_default();
    let name = string_at(obj, "/metadata/name")?;
    if snapshot
        .generated
        .as_ref()
        .is_some_and(|g| g.resource_type == kind && g.namespace == namespace && g.name == name)
    {
        return None;
    }
    // Some charts render namespaced objects without a namespace; accept the
    // namespace-less id too rather than call them orphaned.
    let listed = inventory.contains(&inventory_id(namespace, name, group, kind))
        || inventory.contains(&inventory_id("", name, group, kind));
    (!listed).then_some(OrphanReason::NotInInventory {
        prune_disabled: prune_disabled(obj) || snapshot.prune == Some(false),
    })
}

/// Read one Flux owner.
pub async fn fetch_owner(client: &kube::Client, key: &ResourceKey) -> OwnerState {
    let read = async {
        let resource = crate::kube::api::get_api_resource_with_fallback(
            client,
            &key.resource_type,
            &key.namespace,
            &key.name,
        )
        .await
        .ok()?;
        let api: Api<DynamicObject> =
            Api::namespaced_with(client.clone(), &key.namespace, &resource);
        let fetched = api
            .get(&key.name)
            .await
            .map(|object| serde_json::to_value(&object).unwrap_or_default());
        Some(owner_state_of(fetched, key))
    };
    tokio::time::timeout(OWNER_TIMEOUT, read)
        .await
        .ok()
        .flatten()
        .unwrap_or(OwnerState::Unknown)
}

/// An owner read's outcome. Only "this object doesn't exist" means gone: a
/// 404 because the owner's API isn't served (its CRD isn't installed here —
/// a Kustomization applied from another cluster) says nothing about it.
fn owner_state_of(fetched: Result<Value, kube::Error>, key: &ResourceKey) -> OwnerState {
    match fetched {
        Ok(value) => OwnerState::Present(OwnerSnapshot::from_object(&value)),
        Err(e) if crate::kube::api::is_object_missing(&e) => OwnerState::Gone,
        Err(e) => {
            tracing::debug!("Cannot read Flux owner {key}: {e}");
            OwnerState::Unknown
        }
    }
}

/// Whether a Flux-labelled object is orphaned, reading its owner — for one
/// object (describe). Bounded by [`crate::kube::ownership::MANAGER_TIMEOUT`];
/// `None` for objects Flux doesn't label, still manages, or can't judge.
pub async fn check_orphan(client: &kube::Client, obj: &Value) -> Option<(Manager, OrphanReason)> {
    let (manager, key) = flux_owner(obj)?;
    let owner = tokio::time::timeout(
        crate::kube::ownership::MANAGER_TIMEOUT,
        fetch_owner(client, &key),
    )
    .await
    .unwrap_or(OwnerState::Unknown);
    let group = group_of(string_at(obj, "/apiVersion")?);
    let kind = string_at(obj, "/kind")?;
    let reason = orphan_reason(obj, group, kind, &owner, chrono::Utc::now())?;
    Some((manager, reason))
}

/// Flux owners read for one list or sweep, each re-read after [`OWNER_TTL`].
/// One read per owner, never per object.
#[derive(Debug, Default)]
pub struct FluxOwners {
    states: HashMap<ResourceKey, (OwnerState, Instant)>,
}

impl FluxOwners {
    pub fn get(&self, key: &ResourceKey) -> Option<&OwnerState> {
        self.states.get(key).map(|(state, _)| state)
    }

    /// Record an owner directly (tests).
    #[cfg(test)]
    pub(crate) fn seed(&mut self, key: ResourceKey, state: OwnerState) {
        self.states.insert(key, (state, Instant::now()));
    }

    /// Forget owners nothing references any more.
    pub fn retain(&mut self, keep: &HashSet<ResourceKey>) {
        self.states.retain(|key, _| keep.contains(key));
    }

    /// Read every owner in `keys` that is unread or older than [`OWNER_TTL`].
    /// Returns whether any owner's state changed.
    pub async fn refresh(&mut self, client: &kube::Client, keys: &HashSet<ResourceKey>) -> bool {
        use futures::StreamExt;
        let due: Vec<ResourceKey> = keys
            .iter()
            .filter(|key| {
                self.states
                    .get(*key)
                    .is_none_or(|(_, read)| read.elapsed() >= OWNER_TTL)
            })
            .cloned()
            .collect();
        if due.is_empty() {
            return false;
        }
        let read: Vec<(ResourceKey, OwnerState)> = futures::stream::iter(due)
            .map(|key| async move {
                let state = fetch_owner(client, &key).await;
                (key, state)
            })
            .buffer_unordered(OWNER_CONCURRENCY)
            .collect()
            .await;
        let mut changed = false;
        let now = Instant::now();
        for (key, state) in read {
            changed |= self.get(&key) != Some(&state);
            self.states.insert(key, (state, now));
        }
        changed
    }
}

/// The Flux owner an object's own labels name, if its inventory can be
/// checked. The Flux Operator marks its own objects (the entitlement
/// Secret) `managed-by: flux-operator` too, so a FluxInstance member counts
/// only when it names its instance.
pub fn flux_owner(obj: &Value) -> Option<(Manager, ResourceKey)> {
    let Ownership::Managed(manager) = ownership(obj) else {
        return None;
    };
    if let Manager::Flux { kind, .. } = &manager
        && *kind == FluxResourceKind::FluxInstance
        && label(obj, labels::OPERATOR_NAME).is_none()
    {
        return None;
    }
    let namespace = string_at(obj, "/metadata/namespace").unwrap_or_default();
    let key = manager.flux_key(namespace)?;
    Some((manager, key))
}

// ---------------------------------------------------------------------------
// Kubernetes' own objects, and other recognisable creators
// ---------------------------------------------------------------------------

/// Why an object is Kubernetes' own (never reported as unmanaged), if it is.
/// The same built-ins Argo CD and gitops-zombies exclude.
pub fn system_reason(obj: &Value, group: &str, kind: &str) -> Option<&'static str> {
    let name = string_at(obj, "/metadata/name").unwrap_or_default();
    let namespace = string_at(obj, "/metadata/namespace").unwrap_or_default();
    let core = group.is_empty();
    if core && kind == "ConfigMap" && name == "kube-root-ca.crt" {
        return Some("cluster CA bundle Kubernetes puts in every namespace");
    }
    if core && kind == "ServiceAccount" && name == "default" {
        return Some("the namespace's default ServiceAccount");
    }
    if core && kind == "Secret" && annotation(obj, "kubernetes.io/service-account.name").is_some() {
        return Some("a ServiceAccount token");
    }
    if core && kind == "Service" && namespace == "default" && name == "kubernetes" {
        return Some("the API server's Service");
    }
    if core
        && kind == "Namespace"
        && matches!(
            name,
            "default" | "kube-system" | "kube-public" | "kube-node-lease"
        )
    {
        return Some("a built-in namespace");
    }
    if label(obj, "kubernetes.io/bootstrapping") == Some("rbac-defaults") {
        return Some("default RBAC Kubernetes creates");
    }
    if matches!(
        label(obj, "kube-aggregator.kubernetes.io/automanaged"),
        Some("true" | "onstart")
    ) {
        return Some("a built-in APIService");
    }
    if annotation(obj, "apf.kubernetes.io/autoupdate-spec").is_some() {
        return Some("built-in API priority and fairness config");
    }
    if group == "scheduling.k8s.io" && kind == "PriorityClass" && name.starts_with("system-") {
        return Some("a built-in PriorityClass");
    }
    if core
        && kind == "PersistentVolume"
        && annotation(obj, "pv.kubernetes.io/provisioned-by").is_some()
    {
        return Some("provisioned for a PersistentVolumeClaim");
    }
    match created_by(obj).as_deref() {
        Some("kubeadm") => Some("created when kubeadm set up the cluster"),
        Some(creator) if is_kubernetes_component(creator) => Some("created by Kubernetes itself"),
        _ => None,
    }
}

/// Field managers that are Kubernetes itself.
fn is_kubernetes_component(manager: &str) -> bool {
    matches!(
        manager,
        "kube-apiserver" | "kube-controller-manager" | "kube-scheduler" | "kubelet"
    ) || manager.starts_with("kubernetes-")
}

/// The Helm release a Helm storage object belongs to: Helm keeps each
/// release revision in a Secret (or ConfigMap) labelled `owner=helm`.
pub fn helm_storage_release(obj: &Value, group: &str, kind: &str) -> Option<Manager> {
    if !group.is_empty()
        || !matches!(kind, "Secret" | "ConfigMap")
        || label(obj, "owner") != Some("helm")
    {
        return None;
    }
    Some(Manager::Helm {
        namespace: string_at(obj, "/metadata/namespace")
            .unwrap_or_default()
            .to_string(),
        release: label(obj, "name")?.to_string(),
    })
}

/// How an unmanaged object got there: the field manager that wrote it
/// first (`kubectl`, `Terraform`, `pulumi-kubernetes`, …), from its
/// `managedFields`.
///
/// Status writes don't count, and on a tie (timestamps are per second) a
/// client like `kubectl` wins over a controller, which only reacts to what a
/// client created.
pub fn created_by(obj: &Value) -> Option<String> {
    let first = obj
        .pointer("/metadata/managedFields")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|entry| entry.get("subresource").is_none())
        .filter_map(|entry| {
            let manager = entry.get("manager").and_then(Value::as_str)?;
            let time = entry
                .get("time")
                .and_then(Value::as_str)
                .unwrap_or_default();
            Some((time, !manager.starts_with("kubectl"), manager))
        })
        .min()
        .map(|(_, _, manager)| manager);
    let manager = match first {
        Some(manager) => manager,
        None => {
            return annotation(obj, "kubectl.kubernetes.io/last-applied-configuration")
                .map(|_| "kubectl".to_string());
        }
    };
    if manager.starts_with("kubectl") {
        Some("kubectl".to_string())
    } else {
        Some(manager.chars().take(48).collect())
    }
}

fn has_owner_references(obj: &Value) -> bool {
    obj.pointer("/metadata/ownerReferences")
        .and_then(Value::as_array)
        .is_some_and(|refs| !refs.is_empty())
}

// ---------------------------------------------------------------------------
// Standing
// ---------------------------------------------------------------------------

/// Where a top-level object stands with Flux.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Standing {
    /// Flux manages it — and, where its owner keeps an inventory and could
    /// be read, the inventory lists it.
    Flux(Manager),
    /// Flux labelled it but no longer manages it.
    Orphaned(Manager, OrphanReason),
    /// Another tool manages it (Argo CD, Helm, `app.kubernetes.io/managed-by`).
    Other(Manager),
    /// Kubernetes created it.
    System(&'static str),
    /// Nothing claims it; `created_by` is its first field manager.
    Unmanaged { created_by: Option<String> },
}

impl Standing {
    /// The STANDING column.
    pub fn label(&self) -> &'static str {
        match self {
            Standing::Flux(_) => "Flux",
            Standing::Orphaned(..) => "Orphaned",
            Standing::Other(_) => "Other",
            Standing::System(_) => "System",
            Standing::Unmanaged { .. } => "Unmanaged",
        }
    }

    /// The DETAIL column: who manages it, or why nothing does.
    pub fn detail(&self) -> String {
        match self {
            Standing::Flux(manager) | Standing::Other(manager) => manager.describe(),
            Standing::Orphaned(manager, reason) => reason.describe(manager),
            Standing::System(reason) => (*reason).to_string(),
            Standing::Unmanaged {
                created_by: Some(creator),
            } => format!("created by {creator}"),
            Standing::Unmanaged { created_by: None } => "no creator recorded".to_string(),
        }
    }

    /// The Flux object this standing names (managed or orphaned), whose
    /// key `m` / `t` / `g` act on.
    pub fn flux_manager(&self, namespace: &str) -> Option<ResourceKey> {
        match self {
            Standing::Flux(manager) | Standing::Orphaned(manager, _) => manager.flux_key(namespace),
            _ => None,
        }
    }
}

/// Where a **top-level** object stands; `None` for objects something owns
/// through `ownerReferences` (a pod, a ReplicaSet, an operator's Secret) —
/// their owner is what's reported. `owner_of` returns the read state of a
/// Flux owner (see [`FluxOwners`]); without one, a Flux-labelled object
/// stays [`Standing::Flux`].
pub fn assess(
    obj: &Value,
    group: &str,
    kind: &str,
    owner_of: impl Fn(&ResourceKey) -> Option<OwnerState>,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<Standing> {
    if let Some(reason) = system_reason(obj, group, kind) {
        return Some(Standing::System(reason));
    }
    let namespace = string_at(obj, "/metadata/namespace").unwrap_or_default();
    match ownership(obj) {
        Ownership::Managed(manager) if manager.flux_key(namespace).is_none() => {
            Some(Standing::Other(manager))
        }
        Ownership::Managed(manager) => {
            let reason = flux_owner(obj)
                .and_then(|(_, key)| owner_of(&key))
                .and_then(|owner| orphan_reason(obj, group, kind, &owner, now));
            Some(match reason {
                Some(reason) => Standing::Orphaned(manager, reason),
                None => Standing::Flux(manager),
            })
        }
        Ownership::Owned(_) => None,
        Ownership::Unmanaged if has_owner_references(obj) => None,
        Ownership::Unmanaged => Some(match helm_storage_release(obj, group, kind) {
            Some(release) => Standing::Other(release),
            None => Standing::Unmanaged {
                created_by: created_by(obj),
            },
        }),
    }
}

// ---------------------------------------------------------------------------
// Ignore rules (config: `ownership`)
// ---------------------------------------------------------------------------

/// One ignore rule: every field given must match (`*` / `?` globs); a
/// field left out matches anything. `group: ""` is the core API group.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IgnoreRule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl IgnoreRule {
    pub fn matches(&self, group: &str, kind: &str, namespace: &str, name: &str) -> bool {
        let field = |pattern: &Option<String>, value: &str, fold: bool| {
            pattern.as_deref().is_none_or(|pattern| {
                if fold {
                    glob_match(&pattern.to_lowercase(), &value.to_lowercase())
                } else {
                    glob_match(pattern, value)
                }
            })
        };
        field(&self.group, group, false)
            && field(&self.kind, kind, true)
            && field(&self.namespace, namespace, false)
            && field(&self.name, name, false)
    }
}

/// What the unmanaged / orphaned sweeps leave out (config: `ownership`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnershipConfig {
    /// Objects never reported.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignore: Vec<IgnoreRule>,
    /// Namespaces never swept; unset means [`DEFAULT_IGNORED_NAMESPACES`],
    /// `[]` sweeps every namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ignore_namespaces: Option<Vec<String>>,
}

impl OwnershipConfig {
    pub fn ignores_namespace(&self, namespace: &str) -> bool {
        match &self.ignore_namespaces {
            Some(namespaces) => namespaces.iter().any(|ns| glob_match(ns, namespace)),
            None => DEFAULT_IGNORED_NAMESPACES.contains(&namespace),
        }
    }

    pub fn ignores(&self, group: &str, kind: &str, namespace: &str, name: &str) -> bool {
        (!namespace.is_empty() && self.ignores_namespace(namespace))
            || self
                .ignore
                .iter()
                .any(|rule| rule.matches(group, kind, namespace, name))
    }
}

/// `*` (any run) and `?` (any one character) glob match.
pub fn glob_match(pattern: &str, value: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let value: Vec<char> = value.chars().collect();
    let (mut p, mut v) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while v < value.len() {
        match pattern.get(p) {
            Some('*') => {
                star = Some((p, v));
                p += 1;
            }
            Some(&c) if c == '?' || c == value[v] => {
                p += 1;
                v += 1;
            }
            _ => match star {
                Some((star_p, star_v)) => {
                    p = star_p + 1;
                    v = star_v + 1;
                    star = Some((star_p, star_v + 1));
                }
                None => return false,
            },
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn now() -> chrono::DateTime<chrono::Utc> {
        "2026-10-25T12:00:00Z".parse().unwrap()
    }

    fn ks_labelled(name: &str, extra: Value) -> Value {
        let mut obj = json!({"metadata": {
            "name": name, "namespace": "apps", "creationTimestamp": "2026-10-01T00:00:00Z",
            "labels": {"kustomize.toolkit.fluxcd.io/name": "apps", "kustomize.toolkit.fluxcd.io/namespace": "flux-system"}
        }});
        if let (Some(meta), Some(extra)) = (obj["metadata"].as_object_mut(), extra.as_object()) {
            for (k, v) in extra {
                meta.insert(k.clone(), v.clone());
            }
        }
        obj
    }

    fn owner(inventory: &[&str], ready: bool) -> OwnerSnapshot {
        OwnerSnapshot::from_object(&json!({
            "metadata": {"name": "apps", "namespace": "flux-system", "generation": 3},
            "spec": {"prune": true},
            "status": {
                "observedGeneration": 3,
                "conditions": [{"type": "Ready", "status": if ready { "True" } else { "False" }}],
                "inventory": {"entries": inventory.iter().map(|id| json!({"id": id, "v": "v1"})).collect::<Vec<_>>()}
            }
        }))
    }

    #[test]
    fn inventory_ids_match_what_flux_records() {
        assert_eq!(
            inventory_id("apps", "web", "apps", "Deployment"),
            "apps_web_apps_Deployment"
        );
        assert_eq!(
            inventory_id("", "podinfo", "", "Namespace"),
            "_podinfo__Namespace"
        );
        assert_eq!(group_of("apps/v1"), "apps");
        assert_eq!(group_of("v1"), "");
    }

    #[test]
    fn owner_snapshot_reads_inventory_readiness_and_generated_chart() {
        let snapshot = owner(&["apps_web__ConfigMap"], true);
        assert!(snapshot.settled);
        assert_eq!(snapshot.prune, Some(true));
        assert!(snapshot.inventory.unwrap().contains("apps_web__ConfigMap"));

        let reconciling = OwnerSnapshot::from_object(&json!({
            "metadata": {"generation": 4},
            "status": {"observedGeneration": 3, "conditions": [{"type": "Ready", "status": "True"}]}
        }));
        assert!(!reconciling.settled, "a new generation isn't observed yet");
        assert_eq!(reconciling.inventory, None, "no inventory recorded");

        let hr = OwnerSnapshot::from_object(&json!({
            "metadata": {"namespace": "apps"},
            "status": {"helmChart": "OCIRepository/apps/podinfo", "inventory": {}}
        }));
        assert_eq!(
            hr.generated,
            Some(ResourceKey::new("OCIRepository", "apps", "podinfo"))
        );
        assert_eq!(
            hr.inventory,
            Some(HashSet::new()),
            "an empty inventory is still one"
        );
    }

    #[test]
    fn generated_chart_source_names_its_kind_since_flux_2_10() {
        let owned = |k: &str, ns: &str, n: &str| ResourceKey::new(k, ns, n);
        assert_eq!(
            generated_chart_source("flux-system/apps-podinfo", "default"),
            owned("HelmChart", "flux-system", "apps-podinfo")
        );
        assert_eq!(
            generated_chart_source("OCIRepository/apps/podinfo", "default"),
            owned("OCIRepository", "apps", "podinfo")
        );
        assert_eq!(
            generated_chart_source("podinfo", "default"),
            owned("HelmChart", "default", "podinfo")
        );
    }

    #[test]
    fn orphaned_when_the_owner_is_gone_or_drops_it_from_inventory() {
        let obj = ks_labelled("web", json!({}));
        assert_eq!(
            orphan_reason(&obj, "", "ConfigMap", &OwnerState::Gone, now()),
            Some(OrphanReason::OwnerGone)
        );
        let listed = OwnerState::Present(owner(&["apps_web__ConfigMap"], true));
        assert_eq!(orphan_reason(&obj, "", "ConfigMap", &listed, now()), None);
        let dropped = OwnerState::Present(owner(&["apps_other__ConfigMap"], true));
        assert_eq!(
            orphan_reason(&obj, "", "ConfigMap", &dropped, now()),
            Some(OrphanReason::NotInInventory {
                prune_disabled: false
            })
        );
        let kept = ks_labelled(
            "web",
            json!({"annotations": {"kustomize.toolkit.fluxcd.io/prune": "disabled"}}),
        );
        assert_eq!(
            orphan_reason(&kept, "", "ConfigMap", &dropped, now()),
            Some(OrphanReason::NotInInventory {
                prune_disabled: true
            })
        );
    }

    #[test]
    fn no_verdict_without_a_complete_inventory() {
        let obj = ks_labelled("web", json!({}));
        let unsettled = OwnerState::Present(owner(&["apps_other__ConfigMap"], false));
        assert_eq!(
            orphan_reason(&obj, "", "ConfigMap", &unsettled, now()),
            None
        );
        assert_eq!(
            orphan_reason(&obj, "", "ConfigMap", &OwnerState::Unknown, now()),
            None
        );
        let no_inventory = OwnerState::Present(OwnerSnapshot {
            settled: true,
            ..OwnerSnapshot::default()
        });
        assert_eq!(
            orphan_reason(&obj, "", "ConfigMap", &no_inventory, now()),
            None
        );
        let unpruned_empty = OwnerState::Present(OwnerSnapshot {
            settled: true,
            prune: Some(false),
            inventory: Some(HashSet::new()),
            generated: None,
        });
        assert_eq!(
            orphan_reason(&obj, "", "ConfigMap", &unpruned_empty, now()),
            Some(OrphanReason::NotInInventory {
                prune_disabled: true
            }),
            "everything removed from source with pruning off: a recorded empty inventory is complete"
        );
    }

    #[test]
    fn grace_and_hooks_apply_when_the_owner_is_gone_too() {
        let new = ks_labelled("web", json!({"creationTimestamp": "2026-10-25T11:59:30Z"}));
        assert_eq!(
            orphan_reason(&new, "", "ConfigMap", &OwnerState::Gone, now()),
            None
        );
        let hook = ks_labelled(
            "migrate",
            json!({"annotations": {"helm.sh/hook": "pre-install"}}),
        );
        assert_eq!(
            orphan_reason(&hook, "batch", "Job", &OwnerState::Gone, now()),
            None
        );
    }

    #[test]
    fn operator_owners_need_their_ready_condition_current() {
        // ResourceSet / FluxInstance report observedGeneration on conditions.
        let owner = |observed: i64| {
            OwnerSnapshot::from_object(&json!({
                "metadata": {"generation": 5},
                "status": {"conditions": [{"type": "Ready", "status": "True", "observedGeneration": observed}],
                           "inventory": {"entries": []}}
            }))
        };
        assert!(!owner(4).settled, "Ready from an older generation");
        assert!(owner(5).settled);
    }

    #[test]
    fn only_a_missing_object_means_its_owner_is_gone() {
        let key = ResourceKey::new("Kustomization", "flux-system", "apps");
        let error = |message: &str| {
            Err(kube::Error::Api(Box::new(
                kube::core::Status::failure(message, "NotFound").with_code(404),
            )))
        };
        assert_eq!(
            owner_state_of(
                error(r#"kustomizations.kustomize.toolkit.fluxcd.io "apps" not found"#),
                &key
            ),
            OwnerState::Gone
        );
        assert_eq!(
            owner_state_of(
                error("the server could not find the requested resource"),
                &key
            ),
            OwnerState::Unknown,
            "the API isn't served here (CRD not installed): no verdict"
        );
        assert!(matches!(
            owner_state_of(Ok(json!({"metadata": {}})), &key),
            OwnerState::Present(_)
        ));
    }

    #[test]
    fn new_objects_hooks_and_generated_charts_are_never_orphans() {
        let dropped = OwnerState::Present(owner(&["apps_other__ConfigMap"], true));
        let new = ks_labelled("web", json!({"creationTimestamp": "2026-10-25T11:59:30Z"}));
        assert_eq!(orphan_reason(&new, "", "ConfigMap", &dropped, now()), None);
        let hook = ks_labelled(
            "migrate",
            json!({"annotations": {"helm.sh/hook": "pre-upgrade"}}),
        );
        assert_eq!(orphan_reason(&hook, "batch", "Job", &dropped, now()), None);

        let mut hr = owner(&["apps_web_apps_Deployment"], true);
        hr.generated = Some(ResourceKey::new("HelmChart", "apps", "apps-web"));
        let chart = ks_labelled("apps-web", json!({}));
        assert_eq!(
            orphan_reason(
                &chart,
                "source.toolkit.fluxcd.io",
                "HelmChart",
                &OwnerState::Present(hr),
                now()
            ),
            None,
            "helm-controller labels the HelmChart it generates but never inventories it"
        );
    }

    #[test]
    fn namespace_less_inventory_ids_count_as_listed() {
        let obj = ks_labelled("web", json!({}));
        let owner = OwnerState::Present(owner(&["_web__ConfigMap"], true));
        assert_eq!(orphan_reason(&obj, "", "ConfigMap", &owner, now()), None);
    }

    #[test]
    fn kubernetes_builtins_are_system() {
        let obj = |kind_name: &str, ns: &str, extra: Value| {
            let mut o = json!({"metadata": {"name": kind_name, "namespace": ns}});
            if let (Some(meta), Some(extra)) = (o["metadata"].as_object_mut(), extra.as_object()) {
                for (k, v) in extra {
                    meta.insert(k.clone(), v.clone());
                }
            }
            o
        };
        assert!(
            system_reason(&obj("kube-root-ca.crt", "apps", json!({})), "", "ConfigMap").is_some()
        );
        assert!(system_reason(&obj("default", "apps", json!({})), "", "ServiceAccount").is_some());
        assert!(
            system_reason(
                &obj(
                    "tok",
                    "apps",
                    json!({"annotations": {"kubernetes.io/service-account.name": "x"}})
                ),
                "",
                "Secret"
            )
            .is_some()
        );
        assert!(system_reason(&obj("kubernetes", "default", json!({})), "", "Service").is_some());
        assert!(
            system_reason(
                &obj(
                    "admin",
                    "",
                    json!({"labels": {"kubernetes.io/bootstrapping": "rbac-defaults"}})
                ),
                "rbac.authorization.k8s.io",
                "ClusterRole"
            )
            .is_some()
        );
        assert!(system_reason(&obj("web", "apps", json!({})), "", "ConfigMap").is_none());
        assert!(system_reason(&obj("default", "apps", json!({})), "", "ConfigMap").is_none());
    }

    #[test]
    fn operator_objects_without_an_instance_label_are_not_judged() {
        // The Flux Operator's entitlement Secret: managed-by flux-operator,
        // but not part of any FluxInstance inventory.
        let entitlement = json!({"metadata": {"name": "flux-operator-entitlement", "namespace": "flux-system",
            "creationTimestamp": "2026-07-24T18:28:25Z",
            "labels": {"app.kubernetes.io/managed-by": "flux-operator"}}});
        assert!(flux_owner(&entitlement).is_none());
        let gone = |_: &ResourceKey| Some(OwnerState::Gone);
        assert!(matches!(
            assess(&entitlement, "", "Secret", gone, now()),
            Some(Standing::Flux(_))
        ));
        let member = json!({"metadata": {"name": "source-controller", "namespace": "flux-system",
            "labels": {"app.kubernetes.io/managed-by": "flux-operator",
                       "fluxcd.controlplane.io/name": "flux", "fluxcd.controlplane.io/namespace": "flux-system"}}});
        assert_eq!(
            flux_owner(&member).map(|(_, key)| key),
            Some(ResourceKey::new("FluxInstance", "flux-system", "flux"))
        );
    }

    #[test]
    fn kubernetes_components_and_kubeadm_are_system_creators() {
        let made_by = |manager: &str| json!({"metadata": {"name": "x", "managedFields": [{"manager": manager, "time": "2026-10-01T00:00:00Z"}]}});
        assert!(
            system_reason(&made_by("kube-apiserver"), "networking.k8s.io", "IPAddress").is_some()
        );
        assert!(
            system_reason(
                &made_by("kubernetes-service-cidr-controller"),
                "networking.k8s.io",
                "ServiceCIDR"
            )
            .is_some()
        );
        assert!(
            system_reason(
                &made_by("kubeadm"),
                "rbac.authorization.k8s.io",
                "ClusterRole"
            )
            .is_some()
        );
        assert!(system_reason(&made_by("kubectl-create"), "", "ConfigMap").is_none());
    }

    #[test]
    fn creators_come_from_managed_fields() {
        let obj = json!({"metadata": {"managedFields": [
            {"manager": "kube-controller-manager", "time": "2026-10-02T00:00:00Z"},
            {"manager": "kubectl-client-side-apply", "time": "2026-10-01T00:00:00Z"}
        ]}});
        assert_eq!(created_by(&obj).as_deref(), Some("kubectl"));
        // Same second: the client created it, the controller reacted.
        let tie = json!({"metadata": {"managedFields": [
            {"manager": "flux-operator", "time": "2026-07-24T18:28:25Z"},
            {"manager": "kubectl-client-side-apply", "time": "2026-07-24T18:28:25Z"},
            {"manager": "early-status", "time": "2026-07-01T00:00:00Z", "subresource": "status"}
        ]}});
        assert_eq!(created_by(&tie).as_deref(), Some("kubectl"));
        let tf = json!({"metadata": {"managedFields": [{"manager": "Terraform", "time": "2026-10-01T00:00:00Z"}]}});
        assert_eq!(created_by(&tf).as_deref(), Some("Terraform"));
        let applied = json!({"metadata": {"annotations": {"kubectl.kubernetes.io/last-applied-configuration": "{}"}}});
        assert_eq!(created_by(&applied).as_deref(), Some("kubectl"));
        assert_eq!(created_by(&json!({"metadata": {}})), None);
    }

    #[test]
    fn assess_covers_every_standing() {
        let gone = |_: &ResourceKey| Some(OwnerState::Gone);
        let unread = |_: &ResourceKey| None;

        let labelled = ks_labelled("web", json!({}));
        assert!(matches!(
            assess(&labelled, "", "ConfigMap", gone, now()),
            Some(Standing::Orphaned(_, OrphanReason::OwnerGone))
        ));
        assert!(matches!(
            assess(&labelled, "", "ConfigMap", unread, now()),
            Some(Standing::Flux(_))
        ));

        let argo = json!({"metadata": {"name": "ui", "namespace": "apps",
            "annotations": {"argocd.argoproj.io/tracking-id": "guestbook:apps/Deployment:apps/ui"}}});
        assert_eq!(
            assess(&argo, "apps", "Deployment", unread, now()).map(|s| s.label()),
            Some("Other")
        );

        let helm_storage = json!({"metadata": {"name": "sh.helm.release.v1.web.v1", "namespace": "apps",
            "labels": {"owner": "helm", "name": "web"}}});
        assert_eq!(
            assess(&helm_storage, "", "Secret", unread, now()).map(|s| s.detail()),
            Some("Helm release apps/web".to_string())
        );

        let owned = json!({"metadata": {"name": "web-abc", "namespace": "apps", "ownerReferences": [
            {"apiVersion": "apps/v1", "kind": "ReplicaSet", "name": "web", "uid": "u"}]}});
        assert_eq!(
            assess(&owned, "", "Pod", unread, now()),
            None,
            "owned objects aren't top-level"
        );

        let hand_made = json!({"metadata": {"name": "notes", "namespace": "apps",
            "managedFields": [{"manager": "kubectl-create", "time": "2026-10-01T00:00:00Z"}]}});
        let standing = assess(&hand_made, "", "ConfigMap", unread, now()).unwrap();
        assert_eq!(standing.label(), "Unmanaged");
        assert_eq!(standing.detail(), "created by kubectl");

        let root_ca = json!({"metadata": {"name": "kube-root-ca.crt", "namespace": "apps"}});
        assert_eq!(
            assess(&root_ca, "", "ConfigMap", unread, now()).map(|s| s.label()),
            Some("System")
        );
    }

    #[test]
    fn ignore_rules_glob_and_default_namespaces() {
        let config = OwnershipConfig {
            ignore: vec![IgnoreRule {
                group: Some("velero.io".into()),
                kind: Some("backup".into()),
                name: Some("capi-*".into()),
                namespace: None,
            }],
            ignore_namespaces: None,
        };
        assert!(config.ignores("velero.io", "Backup", "velero", "capi-daily"));
        assert!(!config.ignores("velero.io", "Backup", "velero", "manual"));
        assert!(
            config.ignores("", "ConfigMap", "kube-system", "x"),
            "default ignored namespace"
        );
        assert!(
            !config.ignores("", "Namespace", "", "kube-system"),
            "cluster-scoped objects aren't namespaced"
        );

        let everywhere = OwnershipConfig {
            ignore_namespaces: Some(vec![]),
            ..OwnershipConfig::default()
        };
        assert!(!everywhere.ignores("", "ConfigMap", "kube-system", "x"));
        let core_only = IgnoreRule {
            group: Some(String::new()),
            ..IgnoreRule::default()
        };
        assert!(core_only.matches("", "Secret", "a", "b"));
        assert!(!core_only.matches("apps", "Deployment", "a", "b"));
    }

    #[test]
    fn glob_matching() {
        assert!(glob_match("*", ""));
        assert!(glob_match("sh.helm.release.*", "sh.helm.release.v1.web.v3"));
        assert!(glob_match("team-?", "team-a"));
        assert!(!glob_match("team-?", "team-ab"));
        assert!(glob_match("*-cache-*", "web-cache-warm"));
        assert!(!glob_match("web", "webapp"));
    }
}
