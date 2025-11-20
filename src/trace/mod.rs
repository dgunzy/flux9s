//! Flux trace functionality
//!
//! Traces the ownership chain of Kubernetes objects to find their Flux sources.
//! Similar to `flux trace` command - walks up the owner reference chain to find
//! Kustomization or HelmRelease, then resolves their sources.

mod core;
mod models;

pub use core::trace_object;
// These types are exported for library consumers (tests, etc.)
#[allow(unused_imports)] // Exported for external use
pub use models::{SourceRef, TraceNode, TraceResult, TraceSpec, TraceStatus};
