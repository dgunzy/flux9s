//! Inventory extraction and grouping for Flux resources
//!
//! Handles extraction of managed resources from Flux resource status fields
//! and groups them into categories for graph visualization.

use crate::models::FluxResourceKind;
use serde_json::Value;
use std::collections::HashMap;

/// A resource entry from inventory
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InventoryEntry {
    pub kind: String,
    pub name: String,
    pub namespace: String,
    pub api_version: String,
}

/// Grouped inventory for graph visualization
#[derive(Debug, Clone, Default)]
pub struct InventoryGroups {
    /// Flux resources (individual items)
    pub flux: Vec<InventoryEntry>,
    /// Workload resources (Deployment, StatefulSet, DaemonSet)
    pub workloads: Vec<InventoryEntry>,
    /// Other resources grouped by kind with counts
    pub resources: HashMap<String, usize>,
}

/// Workload kinds that get special treatment in the graph
const WORKLOAD_KINDS: &[&str] = &["Deployment", "StatefulSet", "DaemonSet"];

/// Extract inventory from a resource's status field
///
/// Handles different inventory formats:
/// - Standard: `status.inventory.entries[]` with `id` and `v` fields
/// - HelmRelease: Extracted from Helm storage (not implemented here, handled separately)
/// - ArtifactGenerator: `status.inventory[]` array
pub fn extract_inventory(obj: &Value) -> Result<Vec<InventoryEntry>, anyhow::Error> {
    let mut entries = Vec::new();

    // Try status.inventory.entries first (Kustomization, ResourceSet, FluxInstance)
    if let Some(inventory_entries) = obj
        .get("status")
        .and_then(|s| s.get("inventory"))
        .and_then(|i| i.get("entries"))
        .and_then(|e| e.as_array())
    {
        tracing::debug!(
            "Found status.inventory.entries with {} items",
            inventory_entries.len()
        );
        for (idx, entry) in inventory_entries.iter().enumerate() {
            tracing::debug!("Parsing inventory entry {}: {:?}", idx, entry);
            if let Some(parsed) = parse_inventory_entry(entry) {
                entries.push(parsed);
            } else {
                tracing::warn!("Failed to parse inventory entry {}: {:?}", idx, entry);
            }
        }
        return Ok(entries);
    }

    // Try status.inventory[] array (ArtifactGenerator)
    if let Some(inventory_array) = obj
        .get("status")
        .and_then(|s| s.get("inventory"))
        .and_then(|i| i.as_array())
    {
        tracing::debug!(
            "Found status.inventory array with {} items",
            inventory_array.len()
        );
        for entry in inventory_array {
            if let Some(parsed) = parse_inventory_entry(entry) {
                entries.push(parsed);
            }
        }
        return Ok(entries);
    }

    tracing::debug!("No inventory field found in resource status");
    Ok(entries)
}

/// Parse a single inventory entry
///
/// Format from backend:
/// - `id`: Object metadata string (e.g., "_/ConfigMap/default/my-config")
/// - `v`: API version
fn parse_inventory_entry(entry: &Value) -> Option<InventoryEntry> {
    // Standard format with id and v fields
    if let (Some(id), Some(v)) = (entry.get("id").and_then(|i| i.as_str()), entry.get("v")) {
        return parse_id_field(id, v);
    }

    // Alternative format with direct fields (for ArtifactGenerator ExternalArtifact refs)
    if let (Some(kind), Some(name)) = (
        entry.get("kind").and_then(|k| k.as_str()),
        entry.get("name").and_then(|n| n.as_str()),
    ) {
        let namespace = entry
            .get("namespace")
            .and_then(|ns| ns.as_str())
            .unwrap_or("")
            .to_string();
        let api_version = entry
            .get("apiVersion")
            .and_then(|av| av.as_str())
            .unwrap_or("v1")
            .to_string();

        return Some(InventoryEntry {
            kind: kind.to_string(),
            name: name.to_string(),
            namespace,
            api_version,
        });
    }

    None
}

/// Parse the id field format from Flux inventory
///
/// Formats (discovered from actual Flux inventory data):
/// - Cluster-scoped resources use `__`: `_<name>__<Kind>` (e.g., "_cabot-book__Namespace")
/// - Namespaced resources WITHOUT API group use `__`: `<namespace>_<name>__<Kind>` (e.g., "cabot-book_cabot-book-service__Service")
/// - Namespaced resources WITH API group use `_`: `<namespace>_<name>_<group>_<Kind>` (e.g., "cabot-book_cabot-book_apps_Deployment")
///
/// The key insight: `__` (double underscore) appears ONLY when there's NO API group.
fn parse_id_field(id: &str, v: &Value) -> Option<InventoryEntry> {
    let api_version = v.as_str().unwrap_or("v1").to_string();

    // Check if this uses double underscore (no API group) or single underscores (has API group)
    if id.contains("__") {
        // Format with double underscore (no API group)
        let parts: Vec<&str> = id.split("__").collect();
        if parts.len() != 2 {
            tracing::warn!("Invalid __ format: {}", id);
            return None;
        }

        let kind = parts[1].to_string();
        let before_kind = parts[0];

        // Split the part before __ by single underscore
        let name_parts: Vec<&str> = before_kind.split('_').collect();

        if name_parts.len() == 2 && name_parts[0].is_empty() {
            // Cluster-scoped resource: "_<name>__<Kind>"
            return Some(InventoryEntry {
                kind,
                name: name_parts[1].to_string(),
                namespace: "".to_string(),
                api_version,
            });
        } else if name_parts.len() >= 2 {
            // Namespaced resource without API group: "<namespace>_<name>__<Kind>"
            let namespace = name_parts[0].to_string();
            let name = name_parts[1..].join("_"); // Handle names with underscores
            return Some(InventoryEntry {
                kind,
                name,
                namespace,
                api_version,
            });
        }
    } else {
        // Format with single underscores (has API group)
        // Format: "<namespace>_<name>_<group>_<Kind>"
        let parts: Vec<&str> = id.split('_').collect();

        if parts.len() >= 4 {
            // The last part is the Kind
            let kind = parts[parts.len() - 1].to_string();
            // The second-to-last part is the API group (might contain dots)
            let _api_group = parts[parts.len() - 2];
            // The first part is the namespace
            let namespace = parts[0].to_string();
            // Everything between namespace and API group is the name
            let name = parts[1..parts.len() - 2].join("_");

            return Some(InventoryEntry {
                kind,
                name,
                namespace,
                api_version,
            });
        }
    }

    tracing::warn!("Failed to parse inventory ID format: {}", id);
    None
}

/// Group inventory entries by category for graph display
pub fn group_inventory(entries: Vec<InventoryEntry>) -> InventoryGroups {
    let mut groups = InventoryGroups::default();

    for entry in entries {
        // Check if it's a Flux resource
        if FluxResourceKind::parse_optional(&entry.kind).is_some() {
            groups.flux.push(entry);
        }
        // Check if it's a workload
        else if WORKLOAD_KINDS.contains(&entry.kind.as_str()) {
            groups.workloads.push(entry);
        }
        // All other resources are grouped by kind with counts
        else {
            *groups.resources.entry(entry.kind.clone()).or_insert(0) += 1;
        }
    }

    groups
}

/// Extract upstream node information from sourceRef
///
/// Returns (name, url) if available
pub fn extract_upstream(obj: &Value) -> Option<(String, String)> {
    let source_ref = obj.get("status")?.get("sourceRef")?;

    let origin_url = source_ref
        .get("originURL")
        .and_then(|u| u.as_str())?
        .to_string();

    // Extract name from URL (last path segment)
    let name = origin_url
        .rsplit('/')
        .next()
        .unwrap_or(&origin_url)
        .trim_end_matches(".git")
        .to_string();

    Some((name, origin_url))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_inventory_entry_standard_format() {
        // Test namespace resource (cluster-scoped)
        let entry = json!({
            "id": "_cabot-book__Namespace",
            "v": "v1"
        });

        let parsed = parse_inventory_entry(&entry).unwrap();
        assert_eq!(parsed.kind, "Namespace");
        assert_eq!(parsed.name, "cabot-book");
        assert_eq!(parsed.namespace, "");
        assert_eq!(parsed.api_version, "v1");

        // Test service resource (namespaced, no API group)
        let entry = json!({
            "id": "cabot-book_cabot-book-service__Service",
            "v": "v1"
        });

        let parsed = parse_inventory_entry(&entry).unwrap();
        assert_eq!(parsed.kind, "Service");
        assert_eq!(parsed.name, "cabot-book-service");
        assert_eq!(parsed.namespace, "cabot-book");
        assert_eq!(parsed.api_version, "v1");

        // Test deployment resource (namespaced, with API group)
        let entry = json!({
            "id": "cabot-book_cabot-book_apps_Deployment",
            "v": "v1"
        });

        let parsed = parse_inventory_entry(&entry).unwrap();
        assert_eq!(parsed.kind, "Deployment");
        assert_eq!(parsed.name, "cabot-book");
        assert_eq!(parsed.namespace, "cabot-book");
        assert_eq!(parsed.api_version, "v1");
    }

    #[test]
    fn test_parse_inventory_entry_alternative_format() {
        let entry = json!({
            "kind": "Deployment",
            "name": "my-app",
            "namespace": "prod",
            "apiVersion": "apps/v1"
        });

        let parsed = parse_inventory_entry(&entry).unwrap();
        assert_eq!(parsed.kind, "Deployment");
        assert_eq!(parsed.name, "my-app");
        assert_eq!(parsed.namespace, "prod");
        assert_eq!(parsed.api_version, "apps/v1");
    }

    #[test]
    fn test_group_inventory_flux_resources() {
        let entries = vec![
            InventoryEntry {
                kind: "Kustomization".to_string(),
                name: "app".to_string(),
                namespace: "flux-system".to_string(),
                api_version: "kustomize.toolkit.fluxcd.io/v1".to_string(),
            },
            InventoryEntry {
                kind: "ConfigMap".to_string(),
                name: "config".to_string(),
                namespace: "default".to_string(),
                api_version: "v1".to_string(),
            },
        ];

        let groups = group_inventory(entries);
        assert_eq!(groups.flux.len(), 1);
        assert_eq!(groups.flux[0].kind, "Kustomization");
        assert_eq!(groups.resources.get("ConfigMap"), Some(&1));
    }

    #[test]
    fn test_group_inventory_workloads() {
        let entries = vec![
            InventoryEntry {
                kind: "Deployment".to_string(),
                name: "app".to_string(),
                namespace: "default".to_string(),
                api_version: "apps/v1".to_string(),
            },
            InventoryEntry {
                kind: "StatefulSet".to_string(),
                name: "db".to_string(),
                namespace: "default".to_string(),
                api_version: "apps/v1".to_string(),
            },
        ];

        let groups = group_inventory(entries);
        assert_eq!(groups.workloads.len(), 2);
        assert_eq!(groups.workloads[0].kind, "Deployment");
        assert_eq!(groups.workloads[1].kind, "StatefulSet");
    }

    #[test]
    fn test_extract_upstream() {
        let obj = json!({
            "status": {
                "sourceRef": {
                    "originURL": "https://github.com/user/repo.git"
                }
            }
        });

        let (name, url) = extract_upstream(&obj).unwrap();
        assert_eq!(name, "repo");
        assert_eq!(url, "https://github.com/user/repo.git");
    }
}
