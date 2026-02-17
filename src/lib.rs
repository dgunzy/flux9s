//! flux9s — Flux GitOps resource library
//!
//! This crate provides two usage modes:
//!
//! - **Full (default):** `flux9s = "0.7"` — includes the TUI application
//!   (ratatui + crossterm) and all headless APIs.
//!
//! - **Headless:** `flux9s = { version = "0.7", default-features = false }` —
//!   excludes all TUI dependencies. Use [`ClusterSession`] as the entry point
//!   for programmatic cluster monitoring, scripting, and multi-cluster tooling.
//!
//! # Headless quick start
//!
//! ```rust,no_run
//! use flux9s::services::ClusterSession;
//! use flux9s::config::schema::Config;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let config = Config::default();
//! let mut session = ClusterSession::connect_default(&config).await?;
//!
//! // Drain watch events into state
//! session.drain_events();
//!
//! // Query the current resource snapshot
//! for r in session.snapshot() {
//!     println!("{} {} ({})", r.resource_type, r.name, r.namespace);
//! }
//! # Ok(())
//! # }
//! ```

pub mod config;
pub mod constants;
pub mod kube;
pub mod models;
pub mod operations;
pub mod services;
pub mod trace;
#[cfg(feature = "tui")]
pub mod tui;
pub mod watcher;

// Re-export headless entry point
pub use services::ClusterSession;

// Re-export operations
pub use operations::{FluxOperation, OperationRegistry};

// Re-export kube utilities
pub use kube::{fetch_resource_yaml, get_api_resource_with_fallback, get_gvk_for_resource_type};

// Re-export trace types
pub use trace::{
    GraphEdge, GraphNode, NodeType, RelationshipType, ResourceGraph, SourceRef, TraceNode,
    TraceResult, TraceSpec, TraceStatus, trace_object,
};

// Re-export watcher types
pub use watcher::{
    ResourceInfo, ResourceKey, ResourceState, ResourceWatcher, WatchEvent, WatchableResource,
    extract_status_fields, get_all_commands, resource_key,
};

// Re-export resource field functions
pub use models::resource_fields::{extract_resource_specific_fields, get_resource_type_columns};
