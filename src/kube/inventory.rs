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

impl InventoryEntry {
    /// Encode for a ResourceGroup node description line (#245). Mirrors
    /// [`crate::kube::workloads::WorkloadRef::to_graph_line`]: this impl is the
    /// single source of truth for the format the graph renderer and the
    /// inventory list view decode.
    pub fn to_graph_line(&self) -> String {
        format!(
            "{}|{}|{}|{}",
            self.kind, self.namespace, self.name, self.api_version
        )
    }

    /// Decode a ResourceGroup node description line.
    pub fn parse_graph_line(line: &str) -> Option<Self> {
        let mut parts = line.splitn(4, '|');
        Some(Self {
            kind: parts.next()?.to_string(),
            namespace: parts.next()?.to_string(),
            name: parts.next()?.to_string(),
            api_version: parts.next().unwrap_or("v1").to_string(),
        })
    }
}

/// Grouped inventory for graph visualization
#[derive(Debug, Clone, Default)]
pub struct InventoryGroups {
    /// Flux resources (individual items)
    pub flux: Vec<InventoryEntry>,
    /// Workload resources (Deployment, StatefulSet, DaemonSet)
    pub workloads: Vec<InventoryEntry>,
    /// Everything else. Kept as individual entries (not just per-kind counts)
    /// so the graph's Resources node can be drilled into for the namespace and
    /// name of each item; the node itself still displays per-kind counts via
    /// [`kind_counts`].
    pub resources: Vec<InventoryEntry>,
}

/// Per-kind counts of `entries`, in alphabetical kind order. Drives the
/// "Kind: count" summary lines a ResourceGroup node displays (and, through
/// them, its rendered height).
pub fn kind_counts(entries: &[InventoryEntry]) -> Vec<(String, usize)> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for entry in entries {
        *counts.entry(entry.kind.as_str()).or_insert(0) += 1;
    }
    let mut counts: Vec<(String, usize)> = counts
        .into_iter()
        .map(|(kind, count)| (kind.to_string(), count))
        .collect();
    counts.sort();
    counts
}

/// Per-kind counts for an encoded ResourceGroup node description (one
/// [`InventoryEntry::to_graph_line`] per line). Unparseable lines are skipped.
pub fn kind_counts_from_description(description: &str) -> Vec<(String, usize)> {
    let entries: Vec<InventoryEntry> = description
        .lines()
        .filter_map(InventoryEntry::parse_graph_line)
        .collect();
    kind_counts(&entries)
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
        // Everything else keeps its identity; the graph aggregates by kind
        // for display only.
        else {
            groups.resources.push(entry);
        }
    }

    // Stable, readable order for both the node summary and the drill-down list.
    groups
        .resources
        .sort_by(|a, b| (&a.kind, &a.namespace, &a.name).cmp(&(&b.kind, &b.namespace, &b.name)));

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
        assert_eq!(
            kind_counts(&groups.resources),
            vec![("ConfigMap".into(), 1)]
        );
        // The entry keeps its namespace/name for the drill-down view.
        assert_eq!(groups.resources[0].namespace, "default");
        assert_eq!(groups.resources[0].name, "config");
    }

    #[test]
    fn graph_line_round_trips() {
        let entry = InventoryEntry {
            kind: "Service".to_string(),
            name: "cabot-book-service".to_string(),
            namespace: "cabot-book".to_string(),
            api_version: "v1".to_string(),
        };
        let line = entry.to_graph_line();
        assert_eq!(line, "Service|cabot-book|cabot-book-service|v1");
        assert_eq!(InventoryEntry::parse_graph_line(&line), Some(entry));

        // Cluster-scoped entries have an empty namespace field.
        let cluster = InventoryEntry {
            kind: "Namespace".to_string(),
            name: "demo".to_string(),
            namespace: String::new(),
            api_version: "v1".to_string(),
        };
        assert_eq!(
            InventoryEntry::parse_graph_line(&cluster.to_graph_line()),
            Some(cluster)
        );

        // Garbage lines are skipped rather than panicking.
        assert_eq!(InventoryEntry::parse_graph_line("nonsense"), None);
    }

    #[test]
    fn kind_counts_aggregate_alphabetically() {
        let description = [
            "Service|ns-b|svc-2|v1",
            "ConfigMap|ns-a|cm-1|v1",
            "Service|ns-a|svc-1|v1",
            "not a line",
        ]
        .join("\n");

        assert_eq!(
            kind_counts_from_description(&description),
            vec![("ConfigMap".to_string(), 1), ("Service".to_string(), 2)]
        );
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

    /// A step-based ResourceSet's inventory (same `status.inventory.entries`
    /// format as Kustomization) covering the #204 cases: cluster-scoped
    /// resources (Namespace, CRD), a workload, plain resources, and a
    /// produced Flux resource that must become an individual graph node.
    #[test]
    fn test_resource_set_inventory_extraction_and_grouping() {
        let obj = json!({
            "apiVersion": "fluxcd.controlplane.io/v1",
            "kind": "ResourceSet",
            "metadata": {"name": "staged-rollout", "namespace": "flux-resources"},
            "status": {
                "inventory": {
                    "entries": [
                        // Cluster-scoped, no group
                        {"id": "_demo__Namespace", "v": "v1"},
                        // Cluster-scoped CRD (has an API group)
                        {"id": "_widgets.example.com_apiextensions.k8s.io_CustomResourceDefinition", "v": "v1"},
                        // Workload
                        {"id": "flux-resources_staged-app_apps_Deployment", "v": "v1"},
                        // Plain namespaced resources (real ids from the dev cluster)
                        {"id": "flux-resources_staged-app-config__ConfigMap", "v": "v1"},
                        {"id": "flux-resources_staged-db-migration_batch_Job", "v": "v1"},
                        // A Flux resource produced by the ResourceSet
                        {"id": "flux-resources_podinfo_kustomize.toolkit.fluxcd.io_Kustomization", "v": "v1"}
                    ]
                }
            }
        });

        let entries = extract_inventory(&obj).unwrap();
        assert_eq!(entries.len(), 6);

        let crd = entries
            .iter()
            .find(|e| e.kind == "CustomResourceDefinition")
            .expect("CRD entry parses");
        assert_eq!(crd.name, "widgets.example.com");
        assert_eq!(crd.namespace, "", "cluster-scoped CRD has no namespace");

        let groups = group_inventory(entries);
        // The produced Kustomization is an individual, navigable node
        assert_eq!(groups.flux.len(), 1);
        assert_eq!(groups.flux[0].kind, "Kustomization");
        assert_eq!(groups.flux[0].name, "podinfo");
        // The Deployment gets workload treatment (status fetch + group node)
        assert_eq!(groups.workloads.len(), 1);
        assert_eq!(groups.workloads[0].name, "staged-app");
        // Everything else aggregates by kind for display — including
        // arbitrary/CRD kinds — while keeping the individual entries.
        assert_eq!(
            kind_counts(&groups.resources),
            vec![
                ("ConfigMap".to_string(), 1),
                ("CustomResourceDefinition".to_string(), 1),
                ("Job".to_string(), 1),
                ("Namespace".to_string(), 1),
            ]
        );
        assert!(groups.resources.iter().any(|e| e.kind == "Job"
            && e.namespace == "flux-resources"
            && e.name == "staged-db-migration"));
    }
}
