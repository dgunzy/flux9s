//! Helper functions for extracting resource-specific fields from JSON objects
//!
//! These are thin wrappers that delegate to FluxResourceKind methods.

use crate::models::FluxResourceKind;
use crate::models::flux_resource_kind::field_names;
use crate::plugins::{PluginRegistry, extract_plugin_columns};
use serde_json::Value;
use std::collections::HashMap;

/// Extract resource-specific display fields from a JSON object
pub fn extract_resource_specific_fields(
    resource_type: &str,
    obj: &Value,
    plugin_registry: Option<&PluginRegistry>,
) -> HashMap<String, String> {
    match FluxResourceKind::parse_optional(resource_type) {
        Some(kind) => kind.extract_fields(obj),
        None => {
            // Check if this is a plugin resource
            if let Some(registry) = plugin_registry {
                if let Some((_, watched)) =
                    registry.get_watched_resource_for_display_name(resource_type)
                {
                    // Extract plugin columns using JSONPath
                    return extract_plugin_columns(obj, &watched.columns);
                }
            }
            HashMap::new()
        }
    }
}

/// Get column headers for a resource type
pub fn get_resource_type_columns(
    resource_type: &str,
    plugin_registry: Option<&PluginRegistry>,
) -> Vec<String> {
    match FluxResourceKind::parse_optional(resource_type) {
        Some(kind) => kind.columns(),
        None => {
            // Check if this is a plugin resource
            if let Some(registry) = plugin_registry {
                if let Some((_, watched)) =
                    registry.get_watched_resource_for_display_name(resource_type)
                {
                    // Return plugin-defined columns
                    let mut columns = vec!["STATUS".to_string()];
                    for col in &watched.columns {
                        if col.enabled {
                            columns.push(col.name.clone());
                        }
                    }
                    return columns;
                }
            }
            // Default columns for unknown resource types
            vec![
                field_names::STATUS,
                field_names::NAMESPACE,
                field_names::NAME,
                field_names::TYPE,
                field_names::SUSPENDED,
                field_names::READY,
                field_names::MESSAGE,
            ]
        }
    }
    .into_iter()
    .map(|s| s.to_string())
    .collect()
}
