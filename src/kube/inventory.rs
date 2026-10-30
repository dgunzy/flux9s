//! Inventory extraction and grouping for Flux resources
//!
//! Handles extraction of managed resources from Flux resource status fields
//! and groups them into categories for graph visualization.

use crate::models::FluxResourceKind;
use anyhow::Context;
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
pub const WORKLOAD_KINDS: &[&str] = &["Deployment", "StatefulSet", "DaemonSet"];

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
        // ArtifactGenerator entries carry no apiVersion: a Flux kind's is
        // known; anything else is assumed core.
        let api_version = entry
            .get("apiVersion")
            .and_then(|av| av.as_str())
            .map(str::to_string)
            .or_else(|| {
                FluxResourceKind::parse_optional(kind).map(|k| k.api_resource().api_version)
            })
            .unwrap_or_else(|| "v1".to_string());

        return Some(InventoryEntry {
            kind: kind.to_string(),
            name: name.to_string(),
            namespace,
            api_version,
        });
    }

    None
}

/// Parse a Flux inventory `id` (cli-utils `ObjMetadata` string).
///
/// The format is always four `_`-separated fields,
/// `<namespace>_<name>_<group>_<Kind>`, where the namespace is empty for
/// cluster-scoped objects and the group is empty for core kinds:
/// - `_cabot-book__Namespace` — cluster-scoped, core
/// - `cabot-book_web__Service` — namespaced, core
/// - `cabot-book_web_apps_Deployment` — namespaced, grouped
/// - `_system__auth_rbac.authorization.k8s.io_ClusterRole` — a `:` in a name
///   (`system:auth`) is transcoded to `__`
///
/// Namespace, kind, and group never contain `_`, so they are split off from
/// the ends and whatever remains is the name — the same algorithm as
/// cli-utils' `ParseObjMetadata`. The `v` field only holds the version, so the
/// full `apiVersion` is rebuilt from the group.
fn parse_id_field(id: &str, v: &Value) -> Option<InventoryEntry> {
    let version = v.as_str().unwrap_or("v1");
    let parsed = id.split_once('_').and_then(|(namespace, rest)| {
        let (rest, kind) = rest.rsplit_once('_')?;
        let (name, group) = rest.rsplit_once('_')?;
        Some((namespace, name, group, kind))
    });
    let Some((namespace, name, group, kind)) = parsed else {
        tracing::warn!("Failed to parse inventory ID format: {}", id);
        return None;
    };
    if name.is_empty() || kind.is_empty() {
        tracing::warn!("Failed to parse inventory ID format: {}", id);
        return None;
    }
    let api_version = if group.is_empty() {
        version.to_string()
    } else {
        format!("{group}/{version}")
    };
    Some(InventoryEntry {
        kind: kind.to_string(),
        name: name.replace("__", ":"),
        namespace: namespace.to_string(),
        api_version,
    })
}

/// What a Flux object applied: its `status.inventory`, or for a
/// HelmRelease without one, its latest release's manifest from Helm storage
/// ([`helm_release_inventory`]).
pub async fn applied_objects(
    client: &kube::Client,
    kind: FluxResourceKind,
    obj: &Value,
    namespace: &str,
) -> anyhow::Result<Vec<InventoryEntry>> {
    let entries = extract_inventory(obj)?;
    if entries.is_empty() && kind == FluxResourceKind::HelmRelease {
        return helm_release_inventory(client, obj, namespace).await;
    }
    Ok(entries)
}

/// Extract HelmRelease inventory from Helm storage Secrets
///
/// Follows the web UI pattern exactly:
/// 1. Check prerequisites (skip remote clusters, require storageNamespace and history)
/// 2. Extract latest release info from status.history[0]
/// 3. Fetch Helm storage Secret from status.storageNamespace
/// 4. Decode: base64 → gzip detection → decompress → JSON parse → extract manifest
/// 5. Parse multi-document YAML manifest
/// 6. Handle CRDs if spec.install.crds or spec.upgrade.crds is true
pub async fn helm_release_inventory(
    client: &kube::Client,
    helmrelease_obj: &serde_json::Value,
    helmrelease_namespace: &str,
) -> anyhow::Result<Vec<InventoryEntry>> {
    use k8s_openapi::api::core::v1::Secret;
    use kube::api::Api;

    // 1. Check prerequisites
    let spec = helmrelease_obj
        .get("spec")
        .ok_or_else(|| anyhow::anyhow!("HelmRelease missing spec"))?;

    // Skip remote cluster releases (spec.kubeConfig exists)
    if spec.get("kubeConfig").is_some() {
        tracing::debug!("Skipping HelmRelease with kubeConfig (remote cluster)");
        return Ok(Vec::new());
    }

    // No status, storage namespace, or history yet: nothing was released,
    // so nothing was applied.
    let (Some(storage_namespace), Some(history)) = (
        helmrelease_obj
            .pointer("/status/storageNamespace")
            .and_then(|s| s.as_str()),
        helmrelease_obj
            .pointer("/status/history")
            .and_then(|h| h.as_array()),
    ) else {
        tracing::debug!("HelmRelease has no release yet");
        return Ok(Vec::new());
    };

    if history.is_empty() {
        tracing::debug!("HelmRelease has no release history");
        return Ok(Vec::new());
    }

    // 2. Extract latest release info from status.history[0]
    let latest_release = &history[0];
    let release_name = latest_release
        .get("name")
        .and_then(|n| n.as_str())
        .ok_or_else(|| anyhow::anyhow!("Latest release missing name"))?;
    let release_version = latest_release
        .get("version")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| anyhow::anyhow!("Latest release missing version"))?;
    let release_namespace = latest_release
        .get("namespace")
        .and_then(|n| n.as_str())
        .unwrap_or(helmrelease_namespace);

    tracing::debug!(
        "Latest release: name={}, version={}, namespace={}, storageNamespace={}",
        release_name,
        release_version,
        release_namespace,
        storage_namespace
    );

    // 3. Fetch Helm storage Secret
    // Secret name format: sh.helm.release.v1.{releaseName}.v{version}
    // Secret namespace: status.storageNamespace (NOT HelmRelease namespace!)
    let secret_name = format!("sh.helm.release.v1.{}.v{}", release_name, release_version);
    let secrets_api: Api<Secret> = Api::namespaced(client.clone(), storage_namespace);

    let secret = secrets_api.get(&secret_name).await.with_context(|| {
        format!(
            "Failed to fetch Helm storage Secret {}/{}",
            storage_namespace, secret_name
        )
    })?;

    // 4. Decode release data
    let release_data = secret
        .data
        .as_ref()
        .and_then(|data| data.get("release"))
        .ok_or_else(|| anyhow::anyhow!("Secret missing 'release' key"))?;

    // Base64 decode
    use base64::Engine;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(&release_data.0)
        .context("Failed to decode base64 release data")?;

    // Check for gzip magic bytes (0x1f, 0x8b, 0x08)
    let is_gzipped =
        decoded.len() >= 3 && decoded[0] == 0x1f && decoded[1] == 0x8b && decoded[2] == 0x08;

    // Decompress if gzipped
    let decompressed = if is_gzipped {
        use std::io::Read;
        let mut decoder = flate2::read::GzDecoder::new(&decoded[..]);
        let mut buf = Vec::new();
        decoder
            .read_to_end(&mut buf)
            .context("Failed to decompress gzip release data")?;
        buf
    } else {
        decoded
    };

    // JSON unmarshal
    let release_json: serde_json::Value =
        serde_json::from_slice(&decompressed).context("Failed to parse release JSON")?;

    // Extract manifest from release JSON
    // Helm release JSON structure: { "manifest": "---\nkind: Deployment\n..." }
    let manifest = release_json
        .get("manifest")
        .and_then(|m| m.as_str())
        .ok_or_else(|| anyhow::anyhow!("Release JSON missing 'manifest' field"))?;

    tracing::debug!(
        "Extracted manifest from Helm release Secret ({} bytes, {} objects)",
        manifest.len(),
        manifest.matches("---\n").count() + 1
    );

    // 5. Parse multi-document YAML manifest
    let mut entries = Vec::new();

    // Split by "---\n" to get individual YAML documents
    let documents: Vec<&str> = manifest.split("---\n").collect();

    for doc in documents {
        let doc = doc.trim();
        if doc.is_empty() {
            continue;
        }

        // Parse YAML document
        let resource: serde_json::Value = match crate::yaml::from_str(doc) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("Failed to parse YAML document: {}", e);
                continue;
            }
        };

        // Extract resource metadata
        let kind = resource
            .get("kind")
            .and_then(|k| k.as_str())
            .ok_or_else(|| anyhow::anyhow!("Resource missing kind"))?;

        let metadata = resource
            .get("metadata")
            .ok_or_else(|| anyhow::anyhow!("Resource missing metadata"))?;

        let name = metadata
            .get("name")
            .and_then(|n| n.as_str())
            .ok_or_else(|| anyhow::anyhow!("Resource missing name"))?;

        // Namespace handling: use from manifest, fall back to release namespace
        let resource_namespace = metadata
            .get("namespace")
            .and_then(|n| n.as_str())
            .unwrap_or(release_namespace)
            .to_string();

        let api_version = resource
            .get("apiVersion")
            .and_then(|av| av.as_str())
            .unwrap_or("v1")
            .to_string();

        entries.push(InventoryEntry {
            kind: kind.to_string(),
            name: name.to_string(),
            namespace: resource_namespace,
            api_version,
        });
    }

    // 6. Handle CRDs if spec.install.crds or spec.upgrade.crds is true
    let install_crds = spec
        .get("install")
        .and_then(|i| i.get("crds"))
        .and_then(|c| c.as_bool())
        .unwrap_or(false);
    let upgrade_crds = spec
        .get("upgrade")
        .and_then(|u| u.get("crds"))
        .and_then(|c| c.as_bool())
        .unwrap_or(false);

    if install_crds || upgrade_crds {
        tracing::debug!(
            "HelmRelease has CRD install/upgrade enabled, fetching CRDs with Helm labels"
        );

        // List CRDs with Helm labels: helm.toolkit.fluxcd.io/name and helm.toolkit.fluxcd.io/namespace
        // This is a simplified approach - full implementation would use RESTMapper
        // For now, we'll skip CRD extraction as it requires additional API calls and discovery
        tracing::debug!("CRD extraction not yet implemented (requires RESTMapper)");
    }

    tracing::debug!(
        "Extracted {} resources from Helm release manifest",
        entries.len()
    );

    Ok(entries)
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
        // The group from the id is folded into the apiVersion (#262).
        assert_eq!(parsed.api_version, "apps/v1");
    }

    #[test]
    fn test_parse_inventory_id_cluster_scoped_grouped_and_colon_names() {
        let parsed = parse_inventory_entry(&json!({
            "id": "_system__auth-delegator_rbac.authorization.k8s.io_ClusterRole",
            "v": "v1"
        }))
        .unwrap();
        assert_eq!(parsed.kind, "ClusterRole");
        assert_eq!(parsed.namespace, "");
        // `__` in a name decodes back to `:`
        assert_eq!(parsed.name, "system:auth-delegator");
        assert_eq!(parsed.api_version, "rbac.authorization.k8s.io/v1");

        let crd = parse_inventory_entry(&json!({
            "id": "_widgets.example.com_apiextensions.k8s.io_CustomResourceDefinition",
            "v": "v1"
        }))
        .unwrap();
        assert_eq!(crd.name, "widgets.example.com");
        assert_eq!(crd.api_version, "apiextensions.k8s.io/v1");

        assert!(parse_inventory_entry(&json!({"id": "garbage", "v": "v1"})).is_none());
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
    fn artifact_generator_inventory_keeps_namespaces_and_flux_api_versions() {
        // Flux 2.10 can generate artifacts in other namespaces.
        let ag = json!({"status": {"inventory": [
            {"kind": "ExternalArtifact", "name": "apps-base", "namespace": "flux-resources",
             "digest": "sha256:1", "filename": "1.tar.gz"},
            {"kind": "ExternalArtifact", "name": "apps-shared", "namespace": "flux-artifacts",
             "digest": "sha256:2", "filename": "2.tar.gz"}
        ]}});
        let entries = extract_inventory(&ag).unwrap();
        let found: Vec<_> = entries
            .iter()
            .map(|e| {
                (
                    e.namespace.as_str(),
                    e.name.as_str(),
                    e.api_version.as_str(),
                )
            })
            .collect();
        assert_eq!(
            found,
            [
                ("flux-resources", "apps-base", "source.toolkit.fluxcd.io/v1"),
                (
                    "flux-artifacts",
                    "apps-shared",
                    "source.toolkit.fluxcd.io/v1"
                ),
            ]
        );
        assert_eq!(
            group_inventory(entries).flux.len(),
            2,
            "navigable Flux nodes"
        );
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
