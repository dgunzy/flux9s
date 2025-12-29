//! Flux trace functionality
//!
//! Traces the ownership chain of Kubernetes objects to find their Flux sources.
//! Similar to `flux trace` command - walks up the owner reference chain to find
//! Kustomization or HelmRelease, then resolves their sources.

mod core;
mod graph;
mod graph_builder;
mod models;

pub use core::trace_object;
pub use graph_builder::{build_resource_graph, is_resource_type_with_graph};
// These types are exported for library consumers (tests, etc.)
#[allow(unused_imports)] // Exported for external use
pub use models::{SourceRef, TraceNode, TraceResult, TraceSpec, TraceStatus};
// Graph types
#[allow(unused_imports)] // Re-exported for external use via lib.rs
pub use graph::{GraphEdge, GraphNode, NodeType, RelationshipType, ResourceGraph};
