//! Sweeps for objects Flux doesn't manage (#274): `:unmanaged`,
//! `:orphaned`, and the orphans of one Flux object (`O`).
//!
//! A sweep lists every discovered kind once, **metadata only** (no Secret
//! data, no specs — ownership lives in metadata), keeps the top-level
//! objects (anything with `ownerReferences` is its owner's concern, as in
//! Argo CD and gitops-zombies), reads each Flux owner the objects name once,
//! and judges each object with [`standing::assess`]. It runs on demand —
//! never as a watch — and is bounded per kind and overall.

use std::collections::HashSet;
use std::time::Duration;

use futures::StreamExt;
use kube::Api;
use kube::api::ListParams;
use kube::core::DynamicObject;
use serde_json::Value;

use crate::kube::standing::{self, FluxOwners, OwnershipConfig, Standing};
use crate::models::FluxResourceKind;
use crate::models::flux_resource_kind::labels;
use crate::models::kinds::KindSpec;
use crate::watcher::ResourceKey;

/// Objects per list page.
const PAGE_SIZE: u32 = 500;
/// Objects read per kind before the sweep moves on (reported as truncated).
const MAX_OBJECTS_PER_KIND: usize = 10_000;
/// Rows a sweep returns.
pub const MAX_ROWS: usize = 5_000;
/// Per-kind listing cap.
const LIST_TIMEOUT: Duration = Duration::from_secs(15);
/// Kinds listed at once.
const LIST_CONCURRENCY: usize = 6;

/// Kinds a sweep never lists: churn Kubernetes generates (events, leases,
/// endpoints, revisions, Service IPs), cluster infrastructure (nodes, CSR),
/// metrics, and the Flux Operator's report.
/// `*` matches every kind of the group.
const SKIPPED_KINDS: &[(&str, &str)] = &[
    ("", "Event"),
    ("events.k8s.io", "Event"),
    ("coordination.k8s.io", "Lease"),
    ("", "Endpoints"),
    ("discovery.k8s.io", "EndpointSlice"),
    ("apps", "ControllerRevision"),
    ("", "Node"),
    ("", "ComponentStatus"),
    ("certificates.k8s.io", "CertificateSigningRequest"),
    ("storage.k8s.io", "CSINode"),
    ("storage.k8s.io", "VolumeAttachment"),
    ("networking.k8s.io", "IPAddress"),
    ("networking.k8s.io", "ServiceCIDR"),
    ("metrics.k8s.io", "*"),
    // The Flux Operator's generated status report.
    ("fluxcd.controlplane.io", "FluxReport"),
];

/// What a sweep looks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SweepScope {
    /// Every object Flux doesn't manage: orphaned, unmanaged, and managed
    /// by another tool. Which of them the list shows is a [`SweepShow`],
    /// switched without rescanning.
    NotFlux,
    /// What one Flux object (Kustomization, HelmRelease, ResourceSet,
    /// FluxInstance) left behind, across all namespaces.
    OrphansOf(ResourceKey),
}

impl SweepScope {
    /// Whether a sweep keeps a row of this standing.
    fn keeps(&self, standing: &Standing) -> bool {
        match self {
            SweepScope::NotFlux => SweepShow::All.shows(standing),
            SweepScope::OrphansOf(_) => SweepShow::Orphaned.shows(standing),
        }
    }

    /// The label selector that finds a Flux object's labelled objects
    /// server-side, so a per-owner sweep reads only those.
    fn label_selector(&self) -> Option<String> {
        let SweepScope::OrphansOf(owner) = self else {
            return None;
        };
        let kind = FluxResourceKind::parse_optional(&owner.resource_type)?;
        if kind == FluxResourceKind::FluxInstance {
            return Some(format!(
                "{}={},{}={}",
                labels::MANAGED_BY,
                labels::FLUX_OPERATOR,
                labels::OPERATOR_NAME,
                owner.name
            ));
        }
        let prefix = kind.owner_label_prefix()?;
        Some(format!(
            "{prefix}/name={},{prefix}/namespace={}",
            owner.name, owner.namespace
        ))
    }
}

/// Which rows of a [`SweepScope::NotFlux`] sweep the list shows — what
/// `:orphaned` and `:unmanaged` start with, and `o` /
/// `u` / `a` switch between.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SweepShow {
    /// Objects Flux labelled but left behind.
    Orphaned,
    /// Objects nothing manages.
    Unmanaged,
    /// Everything Flux doesn't manage: orphaned, unmanaged, and objects
    /// another tool (Argo CD, Helm, …) manages. What `:unmanaged` opens on.
    #[default]
    All,
}

impl SweepShow {
    pub fn shows(self, standing: &Standing) -> bool {
        match standing {
            Standing::Orphaned(..) => self != SweepShow::Unmanaged,
            Standing::Unmanaged { .. } => self != SweepShow::Orphaned,
            Standing::Other(_) => self == SweepShow::All,
            Standing::Flux(_) | Standing::System(_) => false,
        }
    }
}

/// Rows of a sweep by standing (orphaned / unmanaged / other).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StandingCounts {
    pub orphaned: usize,
    pub unmanaged: usize,
    pub other: usize,
}

/// Orphans first: they're what needs attention.
fn rank(standing: &Standing) -> u8 {
    match standing {
        Standing::Orphaned(..) => 0,
        Standing::Unmanaged { .. } => 1,
        Standing::Other(_) => 2,
        Standing::Flux(_) | Standing::System(_) => 3,
    }
}

/// Whether a kind can leave orphans ([`FluxResourceKind::can_orphan`]).
pub fn can_orphan(kind: &str) -> bool {
    FluxResourceKind::parse_optional(kind).is_some_and(|kind| kind.can_orphan())
}

/// A sweep to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepRequest {
    pub scope: SweepScope,
    /// `None` = every namespace, plus cluster-scoped kinds. Ignored by
    /// [`SweepScope::OrphansOf`], which always looks everywhere.
    pub namespace: Option<String>,
    pub config: OwnershipConfig,
}

/// One object a sweep reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepRow {
    pub api_version: String,
    pub kind: String,
    /// Empty for cluster-scoped objects.
    pub namespace: String,
    pub name: String,
    pub standing: Standing,
    pub created: Option<chrono::DateTime<chrono::Utc>>,
}

impl SweepRow {
    /// The Flux object a managed or orphaned row names — `None` when it's
    /// gone, so there's nothing to go to.
    pub fn flux_manager(&self) -> Option<ResourceKey> {
        match &self.standing {
            Standing::Orphaned(_, standing::OrphanReason::OwnerGone) => None,
            standing => standing.flux_manager(&self.namespace),
        }
    }
}

/// A one-line description of what a sweep shows, for its title.
pub fn describe_request(request: &SweepRequest, show: SweepShow) -> String {
    let what = match &request.scope {
        SweepScope::NotFlux => match show {
            SweepShow::Orphaned => "Left behind by Flux",
            SweepShow::Unmanaged => "Managed by nothing",
            SweepShow::All => "Not managed by Flux",
        }
        .to_string(),
        SweepScope::OrphansOf(owner) => {
            return format!(
                "Left behind by {} {}/{}",
                owner.resource_type, owner.namespace, owner.name
            );
        }
    };
    match &request.namespace {
        Some(namespace) => format!("{what} in {namespace}"),
        None => format!("{what} in all namespaces"),
    }
}

/// What a sweep found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SweepResult {
    pub rows: Vec<SweepRow>,
    /// Kinds listed.
    pub kinds_scanned: usize,
    /// Kinds that couldn't be listed (RBAC, timeout), by plural.
    pub unreadable: Vec<String>,
    /// Whether a cap cut the sweep short.
    pub truncated: bool,
}

impl SweepResult {
    /// Rows by standing, whatever the list shows.
    pub fn counts(&self) -> StandingCounts {
        let mut counts = StandingCounts::default();
        for row in &self.rows {
            match row.standing {
                Standing::Orphaned(..) => counts.orphaned += 1,
                Standing::Unmanaged { .. } => counts.unmanaged += 1,
                Standing::Other(_) => counts.other += 1,
                Standing::Flux(_) | Standing::System(_) => {}
            }
        }
        counts
    }
}

fn skipped(spec: &KindSpec) -> bool {
    SKIPPED_KINDS
        .iter()
        .any(|(group, kind)| spec.gvk.group == *group && (*kind == "*" || spec.gvk.kind == *kind))
}

/// The kinds a sweep lists: every discovered kind but the skipped ones;
/// namespaced kinds only when sweeping one namespace.
pub fn sweep_kinds(kinds: &[KindSpec], namespace: Option<&str>) -> Vec<KindSpec> {
    kinds
        .iter()
        .filter(|spec| !skipped(spec))
        .filter(|spec| namespace.is_none() || spec.is_namespaced())
        .cloned()
        .collect()
}

/// One listed object, metadata only.
struct Listed {
    spec: KindSpec,
    meta: Value,
}

/// One kind's objects (metadata only).
struct KindObjects {
    items: Vec<Value>,
    /// More than [`MAX_OBJECTS_PER_KIND`]: the rest weren't read.
    truncated: bool,
}

/// A kind that couldn't be listed, by plural (for the report).
type Unreadable = String;

/// List one kind's objects (metadata only), page by page, up to
/// [`MAX_OBJECTS_PER_KIND`].
async fn list_kind(
    client: &kube::Client,
    spec: &KindSpec,
    namespace: Option<&str>,
    selector: Option<&str>,
) -> Result<KindObjects, Unreadable> {
    let resource = spec.api_resource();
    let api: Api<DynamicObject> = match (namespace, spec.is_namespaced()) {
        (Some(ns), true) => Api::namespaced_with(client.clone(), ns, &resource),
        _ => Api::all_with(client.clone(), &resource),
    };
    let list = async {
        let mut objects = Vec::new();
        let mut params = ListParams::default().limit(PAGE_SIZE);
        if let Some(selector) = selector {
            params = params.labels(selector);
        }
        loop {
            let page = api.list_metadata(&params).await.map_err(|e| {
                tracing::debug!("Sweep cannot list {}: {e}", spec.names.plural);
                spec.names.plural.clone()
            })?;
            objects.extend(
                page.items
                    .iter()
                    .filter_map(|item| serde_json::to_value(&item.metadata).ok())
                    .map(|meta| serde_json::json!({ "metadata": meta })),
            );
            match page.metadata.continue_ {
                Some(token) if !token.is_empty() && objects.len() < MAX_OBJECTS_PER_KIND => {
                    params.continue_token = Some(token);
                }
                Some(token) if !token.is_empty() => {
                    return Ok(KindObjects {
                        items: objects,
                        truncated: true,
                    });
                }
                _ => {
                    return Ok(KindObjects {
                        items: objects,
                        truncated: false,
                    });
                }
            }
        }
    };
    tokio::time::timeout(LIST_TIMEOUT, list)
        .await
        .unwrap_or_else(|_| Err(spec.names.plural.clone()))
}

/// Run a sweep over `kinds` (the discovered catalog).
pub async fn sweep(
    client: &kube::Client,
    request: SweepRequest,
    kinds: Vec<KindSpec>,
) -> SweepResult {
    let per_owner = matches!(request.scope, SweepScope::OrphansOf(_));
    let namespace = if per_owner {
        None
    } else {
        request.namespace.clone()
    };
    let kinds = sweep_kinds(&kinds, namespace.as_deref());
    let selector = request.scope.label_selector();
    let mut result = SweepResult {
        kinds_scanned: kinds.len(),
        ..SweepResult::default()
    };

    let listed: Vec<(KindSpec, Result<KindObjects, Unreadable>)> = futures::stream::iter(kinds)
        .map(|spec| {
            let namespace = namespace.clone();
            let selector = selector.clone();
            async move {
                let objects =
                    list_kind(client, &spec, namespace.as_deref(), selector.as_deref()).await;
                (spec, objects)
            }
        })
        .buffer_unordered(LIST_CONCURRENCY)
        .collect()
        .await;

    let mut objects = Vec::new();
    for (spec, outcome) in listed {
        match outcome {
            Ok(KindObjects { items, truncated }) => {
                result.truncated |= truncated;
                objects.extend(items.into_iter().map(|meta| Listed {
                    spec: spec.clone(),
                    meta,
                }));
            }
            Err(plural) => result.unreadable.push(plural),
        }
    }
    result.unreadable.sort();

    // Ignore rules first, so ignored objects cost no owner reads.
    objects.retain(|object| {
        let meta = &object.meta;
        let at = |field: &str| {
            meta.pointer(&format!("/metadata/{field}"))
                .and_then(Value::as_str)
                .unwrap_or_default()
        };
        let group = &object.spec.gvk.group;
        let kind = &object.spec.gvk.kind;
        if per_owner {
            !request
                .config
                .ignore
                .iter()
                .any(|rule| rule.matches(group, kind, at("namespace"), at("name")))
        } else {
            !request
                .config
                .ignores(group, kind, at("namespace"), at("name"))
        }
    });

    // One read per Flux owner the objects name.
    let owner_keys: HashSet<ResourceKey> = objects
        .iter()
        .filter_map(|object| standing::flux_owner(&object.meta).map(|(_, key)| key))
        .collect();
    let mut owners = FluxOwners::default();
    owners.refresh(client, &owner_keys).await;

    let now = chrono::Utc::now();
    for object in objects {
        let spec = &object.spec;
        // Helm's release storage (revision Secrets) belongs to its release,
        // which is what's listed; the kind lists still label it Helm.
        if standing::helm_storage_release(&object.meta, &spec.gvk.group, &spec.gvk.kind).is_some() {
            continue;
        }
        let Some(standing) = standing::assess(
            &object.meta,
            &spec.gvk.group,
            &spec.gvk.kind,
            |key| owners.get(key).cloned(),
            now,
        ) else {
            continue;
        };
        if !request.scope.keeps(&standing) {
            continue;
        }
        let meta = &object.meta;
        let text = |field: &str| {
            meta.pointer(&format!("/metadata/{field}"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        result.rows.push(SweepRow {
            api_version: spec.gvk.api_version(),
            kind: spec.gvk.kind.clone(),
            namespace: text("namespace"),
            name: text("name"),
            created: text("creationTimestamp").parse().ok(),
            standing,
        });
    }
    // Orphans first; the cap drops other tools' objects before orphans.
    result.rows.sort_by(|a, b| {
        (rank(&a.standing), &a.namespace, &a.kind, &a.name).cmp(&(
            rank(&b.standing),
            &b.namespace,
            &b.kind,
            &b.name,
        ))
    });
    if result.rows.len() > MAX_ROWS {
        result.rows.truncate(MAX_ROWS);
        result.truncated = true;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kube::ownership::Manager;
    use crate::kube::standing::OrphanReason;
    use crate::models::kinds::{Gvk, KindScope};

    fn kind(group: &str, kind: &str, scope: KindScope) -> KindSpec {
        KindSpec::generic(
            Gvk {
                group: group.into(),
                version: "v1".into(),
                kind: kind.into(),
            },
            scope,
            format!("{}s", kind.to_lowercase()),
            vec![],
        )
    }

    #[test]
    fn sweeps_skip_churn_and_cluster_kinds_in_a_namespace() {
        let kinds = vec![
            kind("", "ConfigMap", KindScope::Namespaced),
            kind("", "Event", KindScope::Namespaced),
            kind("coordination.k8s.io", "Lease", KindScope::Namespaced),
            kind("metrics.k8s.io", "PodMetrics", KindScope::Namespaced),
            kind(
                "rbac.authorization.k8s.io",
                "ClusterRole",
                KindScope::Cluster,
            ),
            kind("", "Node", KindScope::Cluster),
        ];
        let names =
            |found: Vec<KindSpec>| found.into_iter().map(|k| k.gvk.kind).collect::<Vec<_>>();
        assert_eq!(names(sweep_kinds(&kinds, Some("apps"))), ["ConfigMap"]);
        assert_eq!(
            names(sweep_kinds(&kinds, None)),
            ["ConfigMap", "ClusterRole"]
        );
    }

    #[test]
    fn scopes_choose_what_to_show() {
        let flux = Manager::Flux {
            kind: crate::models::FluxResourceKind::Kustomization,
            namespace: "flux-system".into(),
            name: "apps".into(),
        };
        let orphan = Standing::Orphaned(flux.clone(), OrphanReason::OwnerGone);
        let unmanaged = Standing::Unmanaged { created_by: None };
        let other = Standing::Other(Manager::ArgoCd { app: "x".into() });
        let managed = Standing::Flux(flux);
        let plain = SweepShow::Unmanaged;
        let all = SweepShow::All;
        // Each key shows one standing; `a` everything Flux doesn't manage.
        assert!(plain.shows(&unmanaged) && !plain.shows(&orphan) && !plain.shows(&other));
        assert!(all.shows(&other) && all.shows(&orphan) && all.shows(&unmanaged));
        assert!(!all.shows(&managed));
        assert!(SweepShow::Orphaned.shows(&orphan) && !SweepShow::Orphaned.shows(&unmanaged));
        assert!(!all.shows(&Standing::System("x")));
        // A sweep keeps everything any show needs; per-owner ones only orphans.
        assert!(SweepScope::NotFlux.keeps(&other) && !SweepScope::NotFlux.keeps(&managed));
        let per_owner =
            SweepScope::OrphansOf(ResourceKey::new("Kustomization", "flux-system", "apps"));
        assert!(per_owner.keeps(&orphan) && !per_owner.keeps(&unmanaged));
        // Orphans sort first.
        assert!(rank(&orphan) < rank(&unmanaged) && rank(&unmanaged) < rank(&other));
    }

    #[test]
    fn per_owner_sweeps_select_the_owners_labels() {
        let scope =
            |kind: &str| SweepScope::OrphansOf(ResourceKey::new(kind, "flux-system", "apps"));
        assert_eq!(
            scope("Kustomization").label_selector().as_deref(),
            Some(
                "kustomize.toolkit.fluxcd.io/name=apps,kustomize.toolkit.fluxcd.io/namespace=flux-system"
            )
        );
        assert_eq!(
            scope("HelmRelease").label_selector().as_deref(),
            Some("helm.toolkit.fluxcd.io/name=apps,helm.toolkit.fluxcd.io/namespace=flux-system")
        );
        assert!(
            scope("FluxInstance")
                .label_selector()
                .unwrap()
                .contains("managed-by=flux-operator")
        );
        assert_eq!(scope("GitRepository").label_selector(), None);
        assert!(
            can_orphan("Kustomization")
                && can_orphan("HelmRelease")
                && !can_orphan("GitRepository")
        );
    }
}
