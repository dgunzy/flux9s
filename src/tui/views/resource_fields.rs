//! Helper functions for extracting resource-specific fields from JSON objects
//!
//! These are thin wrappers that delegate to FluxResourceKind methods.

use crate::models::FluxResourceKind;
use crate::models::flux_resource_kind::field_names;
use serde_json::Value;
use std::collections::HashMap;

/// Extract resource-specific display fields from a JSON object
pub fn extract_resource_specific_fields(
    resource_type: &str,
    obj: &Value,
) -> HashMap<String, String> {
    match FluxResourceKind::parse_optional(resource_type) {
        Some(kind) => kind.extract_fields(obj),
        None => HashMap::new(),
    }
}

/// Get column headers for a resource type
pub fn get_resource_type_columns(resource_type: &str) -> Vec<&'static str> {
    match FluxResourceKind::parse_optional(resource_type) {
        Some(kind) => kind.columns(),
        None => vec![
            field_names::STATUS,
            field_names::NAMESPACE,
            field_names::NAME,
            field_names::TYPE,
            field_names::SUSPENDED,
            field_names::READY,
            field_names::MESSAGE,
        ],
    }
}
