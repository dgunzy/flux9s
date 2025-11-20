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
}

/// A node in the trace chain
#[derive(Debug, Clone)]
pub struct TraceNode {
    pub kind: String,
    pub name: String,
    pub namespace: String,
    pub status: Option<TraceStatus>,
    pub spec: Option<TraceSpec>,
}

/// Status information from a Flux resource
#[derive(Debug, Clone)]
pub struct TraceStatus {
    #[allow(dead_code)] // Used in format_trace_result which may be used for debugging
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
