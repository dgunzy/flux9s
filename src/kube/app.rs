//! What Flux apps deploy (the app view): for each Flux object a view names
//! — a Kustomization, HelmRelease, ResourceSet, or FluxInstance — the
//! object itself, its source, everything its inventory lists (nested Flux
//! objects expanded the same way), and the pods its workloads run.
//!
//! Nothing is listed by hand: the tree comes from Flux's own records
//! (`status.inventory`, or a HelmRelease's release manifest), so it follows
//! the app as Git changes. [`resolve`] builds it once; the live task
//! (`kube::live::watch_app`) keeps it current and rebuilds it when the
//! records change shape ([`AppShape`]).

use std::collections::{HashMap, HashSet};

use futures::StreamExt;
use futures::future::BoxFuture;
use k8s_openapi::api::core::v1::Pod;
use kube::Api;
use kube::api::ListParams;
use serde_json::{Value, json};

use crate::kube::inventory::{InventoryEntry, WORKLOAD_KINDS, applied_objects};
use crate::kube::object_status::{ObjectHealth, ObjectStatus, compute_object_status};
use crate::kube::objects::{ObjectRef, fetch_object, fetch_object_statuses, status_for_error};
use crate::models::FluxResourceKind;
use crate::watcher::ResourceKey;

/// Levels of nested Flux objects expanded below an app; deeper ones are
/// listed without their contents.
pub const MAX_DEPTH: usize = 4;
/// Rows an app view holds (reported as truncated).
pub const MAX_ROWS: usize = 2_000;

/// The Flux objects an app view shows, in the order given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppRequest {
    pub roots: Vec<ResourceKey>,
}

/// A Flux object an app view shows: `<kind>/<name>` or
/// `<kind>/<namespace>/<name>`, as typed or configured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppTarget {
    pub kind: String,
    pub namespace: Option<String>,
    pub name: String,
}

impl AppTarget {
    pub fn parse(token: &str) -> Result<Self, String> {
        let parts: Vec<&str> = token.trim().split('/').collect();
        let target = match parts.as_slice() {
            [kind, name] => Self {
                kind: kind.to_string(),
                namespace: None,
                name: name.to_string(),
            },
            [kind, namespace, name] if !namespace.is_empty() => Self {
                kind: kind.to_string(),
                namespace: Some(namespace.to_string()),
                name: name.to_string(),
            },
            _ => {
                return Err(format!(
                    "'{token}' isn't <kind>/<name> or <kind>/<namespace>/<name>"
                ));
            }
        };
        if target.kind.is_empty() || target.name.is_empty() {
            return Err(format!(
                "'{token}' isn't <kind>/<name> or <kind>/<namespace>/<name>"
            ));
        }
        Ok(target)
    }

    /// `ks/podinfo`, as given.
    pub fn label(&self) -> String {
        match &self.namespace {
            Some(namespace) => format!("{}/{namespace}/{}", self.kind, self.name),
            None => format!("{}/{}", self.kind, self.name),
        }
    }
}

/// Whether a kind can head an app view: the Flux kinds that apply objects
/// and keep an inventory of them.
pub fn is_app_kind(kind: FluxResourceKind) -> bool {
    kind.can_orphan()
}

/// Why a row is in the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppRole {
    /// One of the Flux objects the view was asked for.
    App,
    /// Where the Flux object above gets its artifact.
    Source,
    /// Listed in the inventory of the Flux object above.
    Applied,
    /// A pod the workload above runs.
    Pod,
}

impl AppRole {
    pub fn label(self) -> &'static str {
        match self {
            AppRole::App => "app",
            AppRole::Source => "source",
            AppRole::Applied => "applied",
            AppRole::Pod => "pod",
        }
    }
}

/// One object of the tree; `depth` places it under the row above it with a
/// smaller depth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppRow {
    pub object: InventoryEntry,
    pub depth: usize,
    pub role: AppRole,
    pub status: ObjectStatus,
}

impl AppRow {
    /// The object, for describe and YAML. A Flux object is addressed by
    /// kind, so the version fallback for older Flux applies.
    pub fn object_ref(&self) -> ObjectRef {
        let o = &self.object;
        match self.flux_kind() {
            Some(kind) => ObjectRef::from(ResourceKey::new(kind.as_str(), &o.namespace, &o.name)),
            None => ObjectRef::native(&o.api_version, &o.kind, &o.namespace, &o.name),
        }
    }

    /// The row's Flux kind, if it is a Flux object ([`flux_kind_of`]).
    pub fn flux_kind(&self) -> Option<FluxResourceKind> {
        flux_kind_of(&self.object)
    }

    /// The row's Flux object when it can head an app view of its own.
    pub fn flux_app(&self) -> Option<ResourceKey> {
        let kind = self.flux_kind()?;
        is_app_kind(kind)
            .then(|| ResourceKey::new(kind.as_str(), &self.object.namespace, &self.object.name))
    }
}

/// The tree, and what couldn't be read while building it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppSnapshot {
    pub rows: Vec<AppRow>,
    /// Inventories or pods that couldn't be read (RBAC, Helm storage).
    pub notes: Vec<String>,
    /// [`MAX_ROWS`] cut the tree short.
    pub truncated: bool,
}

/// What a snapshot was built from: the rows whose object shaped the tree
/// below them — expanded Flux objects and workloads — with the fields that
/// did ([`shape_of`], or [`MISSING`] when it couldn't be read), and the pods
/// each workload selects. When either changes, the tree is rebuilt; any
/// other change only updates a row's status.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppShape {
    pub shaped: HashMap<usize, String>,
    pub pod_sets: Vec<PodSet>,
}

/// The pods one workload selects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PodSet {
    pub namespace: String,
    pub selector: String,
    /// The pods listed when the tree was built: one missing from a later
    /// listing was deleted in between, and the tree is rebuilt.
    pub pods: Vec<String>,
}

/// The shape recorded for an object that couldn't be read: one that later
/// appears has another ([`shape_of`] is never empty); one still missing
/// keeps it ([`same_shape`]).
pub const MISSING: &str = "";

/// Whether `obj` (`None`: missing) still has the shape the tree was built
/// from — else its part of the tree is rebuilt.
pub fn same_shape(recorded: &str, obj: Option<&Value>) -> bool {
    match obj {
        Some(obj) => shape_of(obj) == recorded,
        None => recorded == MISSING,
    }
}

/// The fields of an object that decide the tree below it: a Flux object's
/// source references and what it applied, a workload's pod selector.
/// Status-only changes (a new reconcile time, a condition, ready replicas)
/// leave it the same.
pub fn shape_of(obj: &Value) -> String {
    let at = |pointer: &str| obj.pointer(pointer).cloned().unwrap_or(Value::Null);
    json!([
        at("/spec/selector"),
        at("/spec/sourceRef"),
        at("/spec/chartRef"),
        at("/spec/chart/spec/sourceRef"),
        at("/status/helmChart"),
        at("/status/inventory"),
        at("/status/history/0/name"),
        at("/status/history/0/namespace"),
        at("/status/history/0/version"),
    ])
    .to_string()
}

/// A Flux object's sources, as its spec and status name them: a
/// Kustomization's `sourceRef`; a HelmRelease's generated HelmChart (and the
/// chart's source) or OCIRepository, else its `chartRef`.
pub fn source_refs(kind: FluxResourceKind, obj: &Value, namespace: &str) -> Vec<InventoryEntry> {
    let reference = |pointer: &str| -> Option<InventoryEntry> {
        let r = obj.pointer(pointer)?;
        let kind = r.get("kind").and_then(Value::as_str)?;
        let name = r.get("name").and_then(Value::as_str)?;
        let namespace = r
            .get("namespace")
            .and_then(Value::as_str)
            .unwrap_or(namespace);
        flux_entry(kind, namespace, name)
    };
    match kind {
        FluxResourceKind::Kustomization => reference("/spec/sourceRef").into_iter().collect(),
        FluxResourceKind::HelmRelease => {
            if let Some(generated) = obj.pointer("/status/helmChart").and_then(Value::as_str) {
                let key = crate::kube::standing::generated_chart_source(generated, namespace);
                let generated = flux_entry(&key.resource_type, &key.namespace, &key.name);
                let chart_source = (key.resource_type == FluxResourceKind::HelmChart.as_str())
                    .then(|| reference("/spec/chart/spec/sourceRef"))
                    .flatten();
                return generated.into_iter().chain(chart_source).collect();
            }
            reference("/spec/chartRef")
                .or_else(|| reference("/spec/chart/spec/sourceRef"))
                .into_iter()
                .collect()
        }
        _ => Vec::new(),
    }
}

/// An entry for a Flux kind, at the API version flux9s reads it at.
fn flux_entry(kind: &str, namespace: &str, name: &str) -> Option<InventoryEntry> {
    let kind = FluxResourceKind::parse_optional(kind)?;
    Some(InventoryEntry {
        kind: kind.as_str().to_string(),
        namespace: namespace.to_string(),
        name: name.to_string(),
        api_version: kind.api_resource().api_version,
    })
}

/// The Flux kind an entry is — by kind *and* API group, so a same-named
/// kind of another group (Crossplane's `Bucket`) isn't taken for Flux's.
pub fn flux_kind_of(entry: &InventoryEntry) -> Option<FluxResourceKind> {
    FluxResourceKind::from_api(&entry.api_version, &entry.kind)
}

fn is_workload(entry: &InventoryEntry) -> bool {
    entry.api_version.starts_with("apps/") && WORKLOAD_KINDS.contains(&entry.kind.as_str())
}

/// Order of an inventory's objects: Flux objects (each with its own tree),
/// then workloads (with their pods), then everything else.
fn child_rank(entry: &InventoryEntry) -> u8 {
    if flux_kind_of(entry).is_some() {
        0
    } else if is_workload(entry) {
        1
    } else {
        2
    }
}

fn sort_children(children: &mut [InventoryEntry]) {
    children.sort_by(|a, b| {
        (child_rank(a), &a.kind, &a.namespace, &a.name).cmp(&(
            child_rank(b),
            &b.kind,
            &b.namespace,
            &b.name,
        ))
    });
}

/// The status a failed lookup shows.
fn status_of_error(error: &anyhow::Error) -> ObjectStatus {
    match error.downcast_ref::<kube::Error>() {
        Some(e) => status_for_error(e),
        None => ObjectStatus::new(ObjectHealth::Unknown, format!("{error:#}")),
    }
}

/// Builds the tree depth-first, so each object's contents follow it.
struct Builder<'a> {
    client: &'a kube::Client,
    snapshot: AppSnapshot,
    shape: AppShape,
    /// Flux objects already expanded: shown again elsewhere, not repeated.
    expanded: HashSet<ResourceKey>,
    /// Rows whose status is read in one batch at the end.
    unread: Vec<usize>,
}

impl Builder<'_> {
    fn push(
        &mut self,
        object: InventoryEntry,
        depth: usize,
        role: AppRole,
        status: Option<ObjectStatus>,
    ) -> Option<usize> {
        if self.snapshot.rows.len() >= MAX_ROWS {
            self.snapshot.truncated = true;
            return None;
        }
        let index = self.snapshot.rows.len();
        if status.is_none() {
            self.unread.push(index);
        }
        self.snapshot.rows.push(AppRow {
            object,
            depth,
            role,
            status: status.unwrap_or_else(|| ObjectStatus::new(ObjectHealth::Unknown, "")),
        });
        Some(index)
    }

    /// A Flux object, then its sources and what it applied.
    fn expand(&mut self, key: ResourceKey, depth: usize, role: AppRole) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            let Some(kind) = FluxResourceKind::parse_optional(&key.resource_type) else {
                return;
            };
            let mut entry = InventoryEntry {
                kind: kind.as_str().to_string(),
                namespace: key.namespace.clone(),
                name: key.name.clone(),
                api_version: kind.api_resource().api_version,
            };
            let obj = match fetch_object(self.client, &ObjectRef::from(key.clone())).await {
                Ok(obj) => obj,
                Err(e) => {
                    // Not there (yet), or unreadable: when it appears, its
                    // shape differs from "missing" and the tree is rebuilt
                    // to expand it.
                    if let Some(index) = self.push(entry, depth, role, Some(status_of_error(&e)))
                        && is_app_kind(kind)
                    {
                        self.shape.shaped.insert(index, MISSING.to_string());
                    }
                    return;
                }
            };
            // The version the cluster served (older Flux falls back).
            if let Some(api_version) = obj.get("apiVersion").and_then(Value::as_str) {
                entry.api_version = api_version.to_string();
            }
            let Some(index) = self.push(entry, depth, role, Some(compute_object_status(&obj)))
            else {
                return;
            };
            if !is_app_kind(kind) || !self.expanded.insert(key.clone()) {
                return;
            }
            self.shape.shaped.insert(index, shape_of(&obj));
            for source in source_refs(kind, &obj, &key.namespace) {
                self.push(source, depth + 1, AppRole::Source, None);
            }
            match applied_objects(self.client, kind, &obj, &key.namespace).await {
                Ok(mut children) => {
                    sort_children(&mut children);
                    for child in children {
                        if self.snapshot.truncated {
                            break;
                        }
                        self.child(child, depth + 1).await;
                    }
                }
                Err(e) => self.snapshot.notes.push(format!(
                    "{} {}/{}: {e:#}",
                    kind.as_str(),
                    key.namespace,
                    key.name
                )),
            }
        })
    }

    /// One inventory object: a nested Flux object is expanded, a workload
    /// gets its pods.
    async fn child(&mut self, entry: InventoryEntry, depth: usize) {
        if let Some(kind) = flux_kind_of(&entry)
            && is_app_kind(kind)
            && depth < MAX_DEPTH
        {
            let key = ResourceKey::new(kind.as_str(), &entry.namespace, &entry.name);
            self.expand(key, depth, AppRole::Applied).await;
        } else if is_workload(&entry) {
            self.workload(entry, depth).await;
        } else {
            self.push(entry, depth, AppRole::Applied, None);
        }
    }

    async fn workload(&mut self, entry: InventoryEntry, depth: usize) {
        let target = ObjectRef::native(
            &entry.api_version,
            &entry.kind,
            &entry.namespace,
            &entry.name,
        );
        let namespace = entry.namespace.clone();
        let obj = match fetch_object(self.client, &target).await {
            Ok(obj) => obj,
            Err(e) => {
                // When it appears, its pods are listed.
                if let Some(index) =
                    self.push(entry, depth, AppRole::Applied, Some(status_of_error(&e)))
                {
                    self.shape.shaped.insert(index, MISSING.to_string());
                }
                return;
            }
        };
        let status = compute_object_status(&obj);
        let Some(index) = self.push(entry, depth, AppRole::Applied, Some(status)) else {
            return;
        };
        // A new selector selects other pods.
        self.shape.shaped.insert(index, shape_of(&obj));
        let Some(selector) = crate::kube::workloads::extract_selector(&obj) else {
            return;
        };
        let api: Api<Pod> = Api::namespaced(self.client.clone(), &namespace);
        match api.list(&ListParams::default().labels(&selector)).await {
            Ok(list) => {
                let mut pods: Vec<Value> = list
                    .items
                    .iter()
                    .filter_map(|pod| serde_json::to_value(pod).ok())
                    .collect();
                pods.sort_by(|a, b| pod_name(a).cmp(pod_name(b)));
                let names: Vec<String> = pods.iter().map(|pod| pod_name(pod).to_string()).collect();
                for pod in pods {
                    let entry = InventoryEntry {
                        kind: "Pod".to_string(),
                        namespace: namespace.clone(),
                        name: pod_name(&pod).to_string(),
                        api_version: "v1".to_string(),
                    };
                    let status = compute_object_status(&pod);
                    self.push(entry, depth + 1, AppRole::Pod, Some(status));
                }
                self.shape.pod_sets.push(PodSet {
                    namespace,
                    selector,
                    pods: names,
                });
            }
            Err(e) => self.snapshot.notes.push(format!("pods of {target}: {e}")),
        }
    }
}

fn pod_name(pod: &Value) -> &str {
    pod.pointer("/metadata/name")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

/// Build the tree for `request`: each root, its sources, what it applied
/// (nested Flux objects expanded up to [`MAX_DEPTH`]), and its workloads'
/// pods. Never fails as a whole: an unreadable object shows its error as
/// its status, an unreadable inventory or pod list becomes a note.
pub async fn resolve(client: &kube::Client, request: &AppRequest) -> (AppSnapshot, AppShape) {
    let mut builder = Builder {
        client,
        snapshot: AppSnapshot::default(),
        shape: AppShape::default(),
        expanded: HashSet::new(),
        unread: Vec::new(),
    };
    for root in &request.roots {
        builder.expand(root.clone(), 0, AppRole::App).await;
    }
    let Builder {
        mut snapshot,
        shape,
        unread,
        ..
    } = builder;
    // Flux objects (sources) are read by kind, with the version fallback;
    // everything else by its inventory apiVersion, in one batch.
    let (flux, other): (Vec<usize>, Vec<usize>) = unread
        .into_iter()
        .partition(|&index| snapshot.rows[index].flux_kind().is_some());
    let targets: Vec<ObjectRef> = flux
        .iter()
        .map(|&index| snapshot.rows[index].object_ref())
        .collect();
    let fetched: Vec<anyhow::Result<Value>> = futures::stream::iter(targets)
        .map(|target| {
            let client = client.clone();
            async move { fetch_object(&client, &target).await }
        })
        .buffered(8)
        .collect()
        .await;
    for (index, obj) in flux.into_iter().zip(fetched) {
        let row = &mut snapshot.rows[index];
        row.status = match obj {
            Ok(obj) => {
                if let Some(api_version) = obj.get("apiVersion").and_then(Value::as_str) {
                    row.object.api_version = api_version.to_string();
                }
                compute_object_status(&obj)
            }
            Err(e) => status_of_error(&e),
        };
    }
    let entries: Vec<InventoryEntry> = other
        .iter()
        .map(|&index| snapshot.rows[index].object.clone())
        .collect();
    let statuses = fetch_object_statuses(client, &entries).await;
    for (index, status) in other.into_iter().zip(statuses) {
        snapshot.rows[index].status = status;
    }
    (snapshot, shape)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kustomization_sources_follow_the_source_ref() {
        let ks = json!({"spec": {"sourceRef": {"kind": "GitRepository", "name": "repo"}}});
        let sources = source_refs(FluxResourceKind::Kustomization, &ks, "flux-system");
        assert_eq!(sources.len(), 1);
        assert_eq!(
            (
                sources[0].kind.as_str(),
                sources[0].namespace.as_str(),
                sources[0].name.as_str()
            ),
            ("GitRepository", "flux-system", "repo"),
            "the namespace defaults to the Kustomization's"
        );
        assert!(
            sources[0]
                .api_version
                .starts_with("source.toolkit.fluxcd.io/")
        );
    }

    #[test]
    fn helm_release_sources_are_the_generated_chart_and_its_source() {
        let hr = json!({
            "spec": {"chart": {"spec": {"sourceRef": {"kind": "HelmRepository", "name": "podinfo", "namespace": "flux-system"}}}},
            "status": {"helmChart": "flux-system/apps-podinfo"}
        });
        let kinds: Vec<(String, String)> = source_refs(FluxResourceKind::HelmRelease, &hr, "apps")
            .into_iter()
            .map(|e| (e.kind, format!("{}/{}", e.namespace, e.name)))
            .collect();
        assert_eq!(
            kinds,
            [
                ("HelmChart".into(), "flux-system/apps-podinfo".into()),
                ("HelmRepository".into(), "flux-system/podinfo".into()),
            ]
        );

        // A chartRef (OCIRepository) with nothing generated yet.
        let oci = json!({"spec": {"chartRef": {"kind": "OCIRepository", "name": "podinfo"}}});
        let sources = source_refs(FluxResourceKind::HelmRelease, &oci, "apps");
        assert_eq!(sources[0].kind, "OCIRepository");
        assert_eq!(sources[0].namespace, "apps");
    }

    #[test]
    fn the_shape_ignores_status_only_changes() {
        let base = json!({
            "spec": {"sourceRef": {"kind": "GitRepository", "name": "repo"}},
            "status": {"inventory": {"entries": [{"id": "apps_web_apps_Deployment", "v": "v1"}]},
                       "lastHandledReconcileAt": "1"}
        });
        let mut reconciled = base.clone();
        reconciled["status"]["lastHandledReconcileAt"] = json!("2");
        assert_eq!(shape_of(&base), shape_of(&reconciled));
        let mut pruned = base.clone();
        pruned["status"]["inventory"]["entries"] = json!([]);
        assert_ne!(shape_of(&base), shape_of(&pruned));
    }

    #[test]
    fn children_list_flux_objects_then_workloads_then_the_rest() {
        let entry = |api_version: &str, kind: &str| InventoryEntry {
            kind: kind.into(),
            namespace: "apps".into(),
            name: "x".into(),
            api_version: api_version.into(),
        };
        let mut children = vec![
            entry("v1", "Service"),
            entry("apps/v1", "Deployment"),
            entry("helm.toolkit.fluxcd.io/v2", "HelmRelease"),
            entry("example.com/v1", "Deployment"),
        ];
        sort_children(&mut children);
        let order: Vec<_> = children
            .iter()
            .map(|e| format!("{}/{}", e.api_version, e.kind))
            .collect();
        assert_eq!(
            order,
            [
                "helm.toolkit.fluxcd.io/v2/HelmRelease",
                "apps/v1/Deployment",
                // A same-named CRD isn't a workload.
                "example.com/v1/Deployment",
                "v1/Service",
            ]
        );
    }

    #[test]
    fn flux_objects_are_told_apart_by_api_group() {
        let entry = |api_version: &str, kind: &str| InventoryEntry {
            kind: kind.into(),
            namespace: "apps".into(),
            name: "data".into(),
            api_version: api_version.into(),
        };
        let flux = entry("source.toolkit.fluxcd.io/v1", "Bucket");
        let crossplane = entry("s3.aws.upbound.io/v1beta1", "Bucket");
        assert_eq!(flux_kind_of(&flux), Some(FluxResourceKind::Bucket));
        assert_eq!(flux_kind_of(&crossplane), None);
        // An older Flux version is still Flux.
        assert!(
            flux_kind_of(&entry(
                "kustomize.toolkit.fluxcd.io/v1beta2",
                "Kustomization"
            ))
            .is_some()
        );

        let row = AppRow {
            object: crossplane.clone(),
            depth: 1,
            role: AppRole::Applied,
            status: ObjectStatus::new(ObjectHealth::Current, ""),
        };
        assert_eq!(
            row.object_ref().api_version.as_deref(),
            Some("s3.aws.upbound.io/v1beta1"),
            "described at its own API, not Flux's"
        );
        assert_eq!(row.flux_app(), None);
        let mut children = vec![crossplane, entry("v1", "ConfigMap"), flux];
        sort_children(&mut children);
        assert_eq!(
            children[0].api_version, "source.toolkit.fluxcd.io/v1",
            "only the Flux Bucket sorts with the Flux objects"
        );
    }

    #[test]
    fn rows_name_their_object_and_flux_app() {
        let row = AppRow {
            object: InventoryEntry {
                kind: "HelmRelease".into(),
                namespace: "apps".into(),
                name: "web".into(),
                api_version: "helm.toolkit.fluxcd.io/v2".into(),
            },
            depth: 1,
            role: AppRole::Applied,
            status: ObjectStatus::new(ObjectHealth::Current, ""),
        };
        assert_eq!(
            row.flux_app(),
            Some(ResourceKey::new("HelmRelease", "apps", "web"))
        );
        let mut source = row.clone();
        source.object.kind = "GitRepository".into();
        assert_eq!(source.flux_app(), None, "sources don't apply objects");
    }
}
