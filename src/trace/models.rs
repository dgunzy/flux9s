//! Data structures for trace results

/// Trace result showing the ownership chain
#[derive(Debug, Clone)]
pub struct TraceResult {
    /// The original object being traced
    pub object: TraceNode,
    /// The chain of owners leading to Flux source (includes Kustomization, HelmRelease, HelmChart, etc.)
    pub chain: Vec<TraceNode>,
    /// The Flux source (GitRepository, OCIRepository, HelmRepository, etc.)
    pub source: Option<TraceNode>,
    /// Where the trace ended (#266).
    pub outcome: TraceOutcome,
}

/// Where a trace ended: at Flux, at another manager, or nowhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TraceOutcome {
    /// The object is a Flux resource or Flux manages it.
    Flux,
    /// Something other than Flux manages it (Argo CD, Helm, a
    /// `managed-by` claim); the trace stops there.
    Other(crate::kube::ownership::Manager),
    /// Nothing manages it: the chain ends at its top owner (or the object).
    Unmanaged,
    /// The chain stopped at an owner that couldn't be read (RBAC, deleted);
    /// carries its `Kind/name`.
    OwnerUnreadable(String),
}

/// Whether `kind` + `api_version` name a Flux kind: the kind name, and the
/// API group when it is known — a Crossplane `Bucket` is not a Flux
/// `Bucket`. Without an `apiVersion` the name decides.
pub fn is_flux_kind(kind: &str, api_version: Option<&str>) -> bool {
    if crate::models::FluxResourceKind::parse_optional(kind).is_none() {
        return false;
    }
    match api_version {
        None => true,
        Some(api_version) => {
            let group = api_version.split_once('/').map_or("", |(group, _)| group);
            crate::kube::api::api_resource_for_kind(kind).is_ok_and(|flux| flux.group == group)
        }
    }
}

impl TraceResult {
    /// The object followed by every distinct node of its chain, bottom-up.
    pub fn path(&self) -> Vec<&TraceNode> {
        std::iter::once(&self.object)
            .chain(self.chain.iter().filter(|n| !n.same_object(&self.object)))
            .collect()
    }

    /// The first Flux manager above the object (Kustomization, HelmRelease,
    /// ResourceSet, FluxInstance) — what `g` and `m` open from a trace.
    pub fn flux_manager(&self) -> Option<&TraceNode> {
        self.path()
            .into_iter()
            .skip(1)
            .find(|n| n.is_flux_manager())
    }
}

impl TraceNode {
    /// Same kind, namespace, and name.
    pub fn same_object(&self, other: &TraceNode) -> bool {
        self.kind == other.kind && self.name == other.name && self.namespace == other.namespace
    }

    /// A Flux object (kind and, when known, API group).
    pub fn is_flux(&self) -> bool {
        is_flux_kind(&self.kind, self.api_version.as_deref())
    }

    /// A Flux kind that manages other objects through ownership labels.
    pub fn is_flux_manager(&self) -> bool {
        use crate::models::FluxResourceKind as K;
        self.is_flux()
            && matches!(
                K::parse_optional(&self.kind),
                Some(K::Kustomization | K::HelmRelease | K::ResourceSet | K::FluxInstance)
            )
    }

    /// The node as a resource key.
    pub fn key(&self) -> crate::watcher::ResourceKey {
        crate::watcher::ResourceKey::new(&self.kind, &self.namespace, &self.name)
    }
}

/// A node in the trace chain
#[derive(Debug, Clone)]
pub struct TraceNode {
    pub kind: String,
    /// The object's `apiVersion`, when it was read from the cluster — tells
    /// a Flux kind from another API group's kind of the same name.
    pub api_version: Option<String>,
    pub name: String,
    /// Empty for cluster-scoped objects.
    pub namespace: String,
    pub status: Option<TraceStatus>,
    pub spec: Option<TraceSpec>,
}

/// Status information from a Flux resource
#[derive(Debug, Clone)]
pub struct TraceStatus {
    pub ready: Option<bool>,
    pub message: Option<String>,
    pub last_reconciled: Option<String>,
    pub revision: Option<String>,
}

/// Spec information from a Flux resource
#[derive(Debug, Clone)]
pub struct TraceSpec {
    pub path: Option<String>,          // For Kustomization
    pub url: Option<String>,           // For GitRepository, OCIRepository
    pub branch: Option<String>,        // For GitRepository
    pub source_ref: Option<SourceRef>, // For Kustomization, HelmRelease
}

/// Source reference from Kustomization or HelmRelease
#[derive(Debug, Clone)]
pub struct SourceRef {
    pub kind: String,
    pub name: String,
    pub namespace: Option<String>,
}
