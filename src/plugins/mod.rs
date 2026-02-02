// Plugin system for flux9s
//
// Enables external data sources to enhance resource views with additional columns
// and custom detail views through YAML configuration.

pub mod cache;
pub mod column_extraction;
pub mod datasource;
pub mod loader;
pub mod manifest;
pub mod registry;
pub mod validator;

pub use cache::PluginCache;
pub use column_extraction::{extract_plugin_columns, render_column_value};
pub use loader::PluginLoader;
pub use manifest::{WatchedResourceConfig, WatchedResourceType};
pub use registry::PluginRegistry;
pub use validator::PluginValidator;

use anyhow::Result;

/// Plugin errors
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("Invalid plugin manifest: {0}")]
    InvalidManifest(String),

    #[error("Plugin conflict: {0}")]
    Conflict(String),

    #[error("Plugin not found: {0}")]
    NotFound(String),

    #[error("Failed to load plugin: {0}")]
    LoadError(String),

    #[error("Validation error: {0}")]
    ValidationError(String),
}

/// Result type for plugin operations
pub type PluginResult<T> = Result<T, PluginError>;
