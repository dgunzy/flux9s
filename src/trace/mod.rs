//! Flux trace functionality
//!
//! Traces the ownership chain of Kubernetes objects to find their Flux sources.
//! Similar to `flux trace` command - walks up the owner reference chain to find
//! Kustomization or HelmRelease, then resolves their sources.

mod core;
mod graph;
mod graph_builder;
mod models;

pub use core::{trace_object, trace_target};
pub use graph_builder::{build_resource_graph, is_resource_type_with_graph, locate_in_graph};
// These types are exported for library consumers (tests, etc.)
pub use models::{SourceRef, TraceNode, TraceOutcome, TraceResult, TraceSpec, TraceStatus};
// Graph types
pub use graph::{GraphEdge, GraphNode, NodeType, RelationshipType, ResourceGraph};
