//! Flux TUI Library
//!
//! This library provides the core functionality for the flux9s TUI application.
//! It can be used both as a binary and as a library for testing.

pub mod kube;
pub mod models;
pub mod tui;
pub mod watcher;

// Re-export commonly used types for convenience
pub use watcher::{
    extract_status_fields, get_all_commands, resource_key, ResourceInfo, ResourceState,
    ResourceWatcher, WatchEvent, WatchableResource,
};

// Re-export TUI functions for testing
pub use tui::views::resource_fields::{
    extract_resource_specific_fields, get_resource_type_columns,
};
