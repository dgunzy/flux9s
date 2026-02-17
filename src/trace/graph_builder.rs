//! Graph builder for discovering resource relationships
//!
//! This module provides functionality to build a complete resource graph,
//! including upstream sources and downstream child resources (workloads, etc.).
//! Based on Flux Operator Web UI graph implementation patterns.

use crate::kube::get_api_resource_with_fallback;
use crate::kube::inventory::{extract_inventory, extract_upstream, group_inventory};
use crate::models::FluxResourceKind;
use crate::trace::graph::{GraphEdge, GraphNode, NodeType, RelationshipType, ResourceGraph};
use anyhow::{Context, Result};
use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, StatefulSet};
use k8s_openapi::api::batch::v1::{CronJob, Job};
use kube::{Api, core::DynamicObject};
use serde_json::Value;

/// Check if a resource type supports the graph view
///
/// Uses FluxResourceKind enum to avoid hardcoding resource types.
pub fn is_resource_type_with_graph(resource_type: &str) -> bool {
    FluxResourceKind::parse_optional(resource_type)
        .map(|kind| kind.supports_graph())
        .unwrap_or(false)
}

/// Build a complete resource graph for a Flux resource
/// This discovers both upstream (sources) and downstream (child resources) relationships
pub async fn build_resource_graph(
    client: &kube::Client,
    resource_type: &str,
    namespace: &str,
    name: &str,
) -> Result<ResourceGraph> {
    let mut graph = ResourceGraph::new();

    // Get the initial resource
    let api_resource =
        get_api_resource_with_fallback(client, resource_type, namespace, name).await?;
    let api: Api<DynamicObject> = Api::namespaced_with(client.clone(), namespace, &api_resource);
    let obj = api.get(name).await.context("Failed to fetch resource")?;
    let obj_value = serde_json::to_value(&obj).context("Failed to serialize resource")?;

    // Create the main node
    let main_node = create_graph_node_from_value(&obj_value, namespace, NodeType::Object)?;
    let main_node_id = main_node.id.clone();
    graph.add_node(main_node);

    // Discover upstream relationships (sources)
    discover_upstream_resources(client, &mut graph, &obj_value, namespace, &main_node_id).await?;

    // Discover downstream relationships (child resources)
    discover_downstream_resources(
        client,
        &mut graph,
        resource_type,
        namespace,
        name,
        &main_node_id,
    )
    .await?;

    Ok(graph)
}

/// Discover upstream resources (sources, parent Flux resources)
async fn discover_upstream_resources(
    client: &kube::Client,
    graph: &mut ResourceGraph,
    obj_value: &Value,
    namespace: &str,
    from_node_id: &str,
) -> Result<()> {
    // Use trace_object to discover upstream relationships
    // This reuses the existing trace logic
    use crate::trace::trace_object;

    // Check if this is a Kustomization or HelmRelease
    let kind = obj_value.get("kind").and_then(|k| k.as_str()).unwrap_or("");

    let flux_kind = FluxResourceKind::parse_optional(kind);

    if matches!(
        flux_kind,
        Some(FluxResourceKind::Kustomization) | Some(FluxResourceKind::HelmRelease)
    ) {
        // Use trace_object to discover upstream relationships
        // This reuses the existing trace logic
        let resource_name = obj_value
            .get("metadata")
            .and_then(|m| m.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or("");

        if let Ok(trace_result) = trace_object(client, kind, namespace, resource_name).await {
            // Add source node if present
            if let Some(source) = &trace_result.source {
                let source_id = format!("{}:{}:{}", source.kind, source.namespace, source.name);

                if !graph.node_index.contains_key(&source_id) {
                    // Extract description from source spec
                    let description = source
                        .spec
                        .as_ref()
                        .and_then(|spec| spec.url.clone().or_else(|| spec.path.clone()));

                    let source_node = GraphNode {
                        id: source_id.clone(),
                        kind: source.kind.clone(),
                        name: source.name.clone(),
                        namespace: source.namespace.clone(),
                        node_type: NodeType::Source,
                        ready: source.status.as_ref().and_then(|s| s.ready),
                        position: None,
                        description: description.clone(),
                    };
                    graph.add_node(source_node);

                    // Fetch the actual source resource to check for upstream
                    if let Ok(source_api_resource) = get_api_resource_with_fallback(
                        client,
                        &source.kind,
                        &source.namespace,
                        &source.name,
                    )
                    .await
                    {
                        let source_api: Api<DynamicObject> = Api::namespaced_with(
                            client.clone(),
                            &source.namespace,
                            &source_api_resource,
                        );
                        if let Ok(source_obj) = source_api.get(&source.name).await {
                            if let Ok(source_value) = serde_json::to_value(&source_obj) {
                                // Check for upstream node (external source like GitHub URL)
                                if let Some((upstream_name, upstream_url)) =
                                    extract_upstream(&source_value)
                                {
                                    let upstream_id = format!("upstream:{}", upstream_name);
                                    if !graph.node_index.contains_key(&upstream_id) {
                                        let upstream_node = GraphNode {
                                            id: upstream_id.clone(),
                                            kind: "GitHub".to_string(),
                                            name: upstream_name,
                                            namespace: source.namespace.clone(),
                                            node_type: NodeType::Upstream,
                                            ready: None,
                                            position: None,
                                            description: Some(upstream_url),
                                        };
                                        graph.add_node(upstream_node);

                                        // Add edge from upstream to source (top-down flow)
                                        graph.add_edge(GraphEdge {
                                            from: upstream_id,
                                            to: source_id.clone(),
                                            relationship: RelationshipType::SourcedFrom,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }

                // Add edge from source to main node (top-down flow)
                graph.add_edge(GraphEdge {
                    from: source_id,
                    to: from_node_id.to_string(),
                    relationship: RelationshipType::SourcedFrom,
                });
            }

            // Add chain nodes (intermediate resources like HelmChart)
            for chain_node in &trace_result.chain {
                let chain_id = format!(
                    "{}:{}:{}",
                    chain_node.kind, chain_node.namespace, chain_node.name
                );

                if !graph.node_index.contains_key(&chain_id) {
                    // Extract description from chain node spec
                    let description = chain_node
                        .spec
                        .as_ref()
                        .and_then(|spec| spec.url.clone().or_else(|| spec.path.clone()));

                    let graph_node = GraphNode {
                        id: chain_id.clone(),
                        kind: chain_node.kind.clone(),
                        name: chain_node.name.clone(),
                        namespace: chain_node.namespace.clone(),
                        node_type: NodeType::Chain,
                        ready: chain_node.status.as_ref().and_then(|s| s.ready),
                        position: None,
                        description,
                    };
                    graph.add_node(graph_node);
                }

                // Add edge from source/parent to chain to main node (top-down flow)
                // Chain nodes are intermediate - they connect source to object
                // For now, we'll connect chain to object (reverse this later based on trace structure)
                graph.add_edge(GraphEdge {
                    from: chain_id,
                    to: from_node_id.to_string(),
                    relationship: RelationshipType::ManagedBy,
                });
            }
        }
    }

    // Check for owner references (if this resource is managed by another Flux resource)
    if let Some(owner_refs) = obj_value
        .get("metadata")
        .and_then(|m| m.get("ownerReferences"))
        .and_then(|o| o.as_array())
    {
        for owner_ref in owner_refs {
            if let (Some(owner_kind), Some(owner_name)) = (
                owner_ref.get("kind").and_then(|k| k.as_str()),
                owner_ref.get("name").and_then(|n| n.as_str()),
            ) {
                if is_flux_resource(owner_kind) {
                    // This resource is managed by another Flux resource
                    if let Some(owner_node) =
                        fetch_flux_resource_as_graph_node(client, owner_kind, namespace, owner_name)
                            .await?
                    {
                        let owner_id = owner_node.id.clone();
                        if !graph.node_index.contains_key(&owner_id) {
                            graph.add_node(owner_node);
                        }

                        // Add edge from owner to this resource
                        graph.add_edge(GraphEdge {
                            from: owner_id,
                            to: from_node_id.to_string(),
                            relationship: RelationshipType::ManagedBy,
                        });
                    }
                }
            }
        }
    }

    Ok(())
}

/// Discover downstream resources (workloads, ConfigMaps, Services, etc. created by this resource)
async fn discover_downstream_resources(
    client: &kube::Client,
    graph: &mut ResourceGraph,
    resource_type: &str,
    namespace: &str,
    name: &str,
    from_node_id: &str,
) -> Result<()> {
    // For Kustomization and HelmRelease, discover managed resources
    let flux_kind = FluxResourceKind::parse_optional(resource_type);

    match flux_kind {
        Some(FluxResourceKind::Kustomization) => {
            discover_kustomization_resources(client, graph, namespace, name, from_node_id).await?;
        }
        Some(FluxResourceKind::HelmRelease) => {
            discover_helmrelease_resources(client, graph, namespace, name, from_node_id).await?;
        }
        _ => {
            // For other resources, check if they're managed by a Flux resource
            // This is handled in discover_upstream_resources
        }
    }

    Ok(())
}

/// Discover resources managed by a Kustomization using inventory extraction
async fn discover_kustomization_resources(
    client: &kube::Client,
    graph: &mut ResourceGraph,
    namespace: &str,
    kustomization_name: &str,
    from_node_id: &str,
) -> Result<()> {
    // Fetch the Kustomization resource to extract inventory
    let api_resource =
        get_api_resource_with_fallback(client, "Kustomization", namespace, kustomization_name)
            .await?;
    let api: Api<DynamicObject> = Api::namespaced_with(client.clone(), namespace, &api_resource);
    let obj = api
        .get(kustomization_name)
        .await
        .context("Failed to fetch Kustomization")?;
    let obj_value = serde_json::to_value(&obj).context("Failed to serialize resource")?;

    // Extract and group inventory
    if let Ok(entries) = extract_inventory(&obj_value) {
        tracing::debug!(
            "Extracted {} inventory entries from Kustomization {}",
            entries.len(),
            kustomization_name
        );
        let groups = group_inventory(entries);
        tracing::debug!(
            "Grouped inventory: {} Flux resources, {} workloads, {} resource groups",
            groups.flux.len(),
            groups.workloads.len(),
            groups.resources.len()
        );

        // Add Flux resource nodes (individual items)
        for flux_entry in &groups.flux {
            let flux_id = format!(
                "{}:{}:{}",
                flux_entry.kind, flux_entry.namespace, flux_entry.name
            );
            if !graph.node_index.contains_key(&flux_id) {
                let flux_node = GraphNode {
                    id: flux_id.clone(),
                    kind: flux_entry.kind.clone(),
                    name: flux_entry.name.clone(),
                    namespace: flux_entry.namespace.clone(),
                    node_type: NodeType::FluxResource,
                    ready: None,
                    position: None,
                    description: None,
                };
                graph.add_node(flux_node);
                graph.add_edge(GraphEdge {
                    from: from_node_id.to_string(),
                    to: flux_id,
                    relationship: RelationshipType::Owns,
                });
            }
        }

        // Add a single workload group node (aggregated with status for each workload)
        if !groups.workloads.is_empty() {
            let workload_group_id = format!("workloadgroup:{}", namespace);

            if !graph.node_index.contains_key(&workload_group_id) {
                // Fetch status for each workload and build description
                let mut workload_list = Vec::new();

                for workload_entry in &groups.workloads {
                    // Fetch workload status to get replica information
                    let (ready, status_desc) = fetch_workload_status(
                        client,
                        &workload_entry.kind,
                        &workload_entry.namespace,
                        &workload_entry.name,
                    )
                    .await
                    .unwrap_or_else(|e| {
                        tracing::warn!(
                            "Failed to fetch workload status for {}/{}: {}",
                            workload_entry.kind,
                            workload_entry.name,
                            e
                        );
                        (None, None)
                    });

                    tracing::debug!(
                        "Fetched workload {}/{} - ready: {:?}, status: {:?}",
                        workload_entry.kind,
                        workload_entry.name,
                        ready,
                        status_desc
                    );

                    // Format: "Kind|name|namespace|ready|status"
                    // We'll parse this in the rendering code
                    let status_indicator = match ready {
                        Some(true) => "●",
                        Some(false) => "○",
                        None => "?",
                    };
                    let status_text = status_desc.unwrap_or_else(|| "Unknown".to_string());
                    let workload_info = format!(
                        "{}|{}|{}|{}|{}",
                        workload_entry.kind,
                        workload_entry.name,
                        workload_entry.namespace,
                        status_indicator,
                        status_text
                    );
                    workload_list.push(workload_info);
                }

                tracing::debug!(
                    "Creating WorkloadGroup node with {} workloads: {:?}",
                    groups.workloads.len(),
                    workload_list
                );

                let workload_group_node = GraphNode {
                    id: workload_group_id.clone(),
                    kind: "Workloads".to_string(),
                    name: format!("Workloads ({})", groups.workloads.len()),
                    namespace: namespace.to_string(),
                    node_type: NodeType::WorkloadGroup,
                    ready: None,
                    position: None,
                    description: Some(workload_list.join("\n")),
                };
                graph.add_node(workload_group_node);
                graph.add_edge(GraphEdge {
                    from: from_node_id.to_string(),
                    to: workload_group_id.clone(),
                    relationship: RelationshipType::Owns,
                });

                tracing::debug!("Added WorkloadGroup node: {}", workload_group_id);
            }
        }

        // Add a single resource group node (aggregated by kind with counts)
        if !groups.resources.is_empty() {
            let total_count: usize = groups.resources.values().sum();
            let resource_group_id = format!("resourcegroup:{}", namespace);

            if !graph.node_index.contains_key(&resource_group_id) {
                // Create a description with all resource kinds and counts
                // Format: "Kind1: count1, Kind2: count2, ..."
                let mut resource_list: Vec<String> = groups
                    .resources
                    .iter()
                    .map(|(kind, count)| format!("{}: {}", kind, count))
                    .collect();
                resource_list.sort(); // Alphabetical order like Web UI

                let resource_group_node = GraphNode {
                    id: resource_group_id.clone(),
                    kind: "Resources".to_string(),
                    name: format!("Resources ({})", total_count),
                    namespace: namespace.to_string(),
                    node_type: NodeType::ResourceGroup,
                    ready: None,
                    position: None,
                    description: Some(resource_list.join(", ")),
                };
                graph.add_node(resource_group_node);
                graph.add_edge(GraphEdge {
                    from: from_node_id.to_string(),
                    to: resource_group_id,
                    relationship: RelationshipType::Owns,
                });
            }
        }
    } else {
        tracing::debug!(
            "No inventory found for Kustomization {} in namespace {}",
            kustomization_name,
            namespace
        );
    }

    Ok(())
}

/// Discover resources managed by a HelmRelease using inventory extraction
async fn discover_helmrelease_resources(
    client: &kube::Client,
    graph: &mut ResourceGraph,
    namespace: &str,
    helmrelease_name: &str,
    from_node_id: &str,
) -> Result<()> {
    // Fetch the HelmRelease resource to extract inventory
    let api_resource =
        get_api_resource_with_fallback(client, "HelmRelease", namespace, helmrelease_name).await?;
    let api: Api<DynamicObject> = Api::namespaced_with(client.clone(), namespace, &api_resource);
    let obj = api
        .get(helmrelease_name)
        .await
        .context("Failed to fetch HelmRelease")?;
    let obj_value = serde_json::to_value(&obj).context("Failed to serialize resource")?;

    // Extract inventory following web UI pattern:
    // 1. Try status.inventory first (newer Flux versions may populate this)
    // 2. Fall back to Helm storage Secret extraction (standard approach)
    tracing::debug!(
        "Attempting to extract inventory for HelmRelease {}/{}",
        namespace,
        helmrelease_name
    );

    // Try status.inventory first (may be populated in newer Flux versions)
    let entries = match extract_inventory(&obj_value) {
        Ok(entries) if !entries.is_empty() => {
            tracing::debug!(
                "Extracted {} inventory entries from HelmRelease status.inventory",
                entries.len()
            );
            entries
        }
        _ => {
            // Extract from Helm storage Secrets (standard approach per web UI)
            tracing::debug!("Extracting inventory from Helm storage Secrets");
            extract_helmrelease_inventory_from_secrets(client, &obj_value, namespace).await?
        }
    };

    if entries.is_empty() {
        tracing::warn!(
            "No inventory entries found for HelmRelease {}/{}",
            namespace,
            helmrelease_name
        );
        return Ok(());
    }

    let groups = group_inventory(entries);
    tracing::debug!(
        "Grouped inventory: {} Flux resources, {} workloads, {} resource groups",
        groups.flux.len(),
        groups.workloads.len(),
        groups.resources.len()
    );

    // Add Flux resource nodes (individual items)
    for flux_entry in &groups.flux {
        let flux_id = format!(
            "{}:{}:{}",
            flux_entry.kind, flux_entry.namespace, flux_entry.name
        );
        if !graph.node_index.contains_key(&flux_id) {
            let flux_node = GraphNode {
                id: flux_id.clone(),
                kind: flux_entry.kind.clone(),
                name: flux_entry.name.clone(),
                namespace: flux_entry.namespace.clone(),
                node_type: NodeType::FluxResource,
                ready: None,
                position: None,
                description: None,
            };
            graph.add_node(flux_node);
            graph.add_edge(GraphEdge {
                from: from_node_id.to_string(),
                to: flux_id,
                relationship: RelationshipType::Owns,
            });
        }
    }

    // Add a single workload group node (aggregated with status for each workload)
    if !groups.workloads.is_empty() {
        let workload_group_id = format!("workloadgroup:{}", namespace);

        if !graph.node_index.contains_key(&workload_group_id) {
            // Fetch status for each workload and build description
            let mut workload_list = Vec::new();

            for workload_entry in &groups.workloads {
                // Fetch workload status to get replica information
                let (ready, status_desc) = fetch_workload_status(
                    client,
                    &workload_entry.kind,
                    &workload_entry.namespace,
                    &workload_entry.name,
                )
                .await
                .unwrap_or_else(|e| {
                    tracing::warn!(
                        "Failed to fetch workload status for {}/{}: {}",
                        workload_entry.kind,
                        workload_entry.name,
                        e
                    );
                    (None, None)
                });

                tracing::debug!(
                    "Fetched workload {}/{} - ready: {:?}, status: {:?}",
                    workload_entry.kind,
                    workload_entry.name,
                    ready,
                    status_desc
                );

                // Format: "Kind|name|namespace|ready|status"
                // We'll parse this in the rendering code
                let status_indicator = match ready {
                    Some(true) => "●",
                    Some(false) => "○",
                    None => "?",
                };
                let status_text = status_desc.unwrap_or_else(|| "Unknown".to_string());
                let workload_info = format!(
                    "{}|{}|{}|{}|{}",
                    workload_entry.kind,
                    workload_entry.name,
                    workload_entry.namespace,
                    status_indicator,
                    status_text
                );
                workload_list.push(workload_info);
            }

            tracing::debug!(
                "Creating WorkloadGroup node with {} workloads: {:?}",
                groups.workloads.len(),
                workload_list
            );

            let workload_group_node = GraphNode {
                id: workload_group_id.clone(),
                kind: "Workloads".to_string(),
                name: format!("Workloads ({})", groups.workloads.len()),
                namespace: namespace.to_string(),
                node_type: NodeType::WorkloadGroup,
                ready: None,
                position: None,
                description: Some(workload_list.join("\n")),
            };
            graph.add_node(workload_group_node);
            graph.add_edge(GraphEdge {
                from: from_node_id.to_string(),
                to: workload_group_id.clone(),
                relationship: RelationshipType::Owns,
            });

            tracing::debug!("Added WorkloadGroup node: {}", workload_group_id);
        }
    }

    // Add a single resource group node (aggregated by kind with counts)
    if !groups.resources.is_empty() {
        let total_count: usize = groups.resources.values().sum();
        let resource_group_id = format!("resourcegroup:{}", namespace);

        if !graph.node_index.contains_key(&resource_group_id) {
            // Create a description with all resource kinds and counts
            // Format: "Kind1: count1, Kind2: count2, ..."
            let mut resource_list: Vec<String> = groups
                .resources
                .iter()
                .map(|(kind, count)| format!("{}: {}", kind, count))
                .collect();
            resource_list.sort(); // Alphabetical order like Web UI

            let resource_group_node = GraphNode {
                id: resource_group_id.clone(),
                kind: "Resources".to_string(),
                name: format!("Resources ({})", total_count),
                namespace: namespace.to_string(),
                node_type: NodeType::ResourceGroup,
                ready: None,
                position: None,
                description: Some(resource_list.join(", ")),
            };
            graph.add_node(resource_group_node);
            graph.add_edge(GraphEdge {
                from: from_node_id.to_string(),
                to: resource_group_id,
                relationship: RelationshipType::Owns,
            });
        }
    }

    Ok(())
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
async fn extract_helmrelease_inventory_from_secrets(
    client: &kube::Client,
    helmrelease_obj: &serde_json::Value,
    helmrelease_namespace: &str,
) -> Result<Vec<crate::kube::inventory::InventoryEntry>> {
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

    let status = helmrelease_obj
        .get("status")
        .ok_or_else(|| anyhow::anyhow!("HelmRelease missing status"))?;

    // Require status.storageNamespace
    let storage_namespace = status
        .get("storageNamespace")
        .and_then(|s| s.as_str())
        .ok_or_else(|| anyhow::anyhow!("HelmRelease missing status.storageNamespace"))?;

    // Require status.history with at least one entry
    let history = status
        .get("history")
        .and_then(|h| h.as_array())
        .ok_or_else(|| anyhow::anyhow!("HelmRelease missing status.history"))?;

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
        let resource: serde_json::Value = match serde_yaml::from_str(doc) {
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

        entries.push(crate::kube::inventory::InventoryEntry {
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

/// Create a graph node from a JSON value
fn create_graph_node_from_value(
    obj_value: &Value,
    namespace: &str,
    node_type: NodeType,
) -> Result<GraphNode> {
    let metadata = obj_value.get("metadata").context("Missing metadata")?;

    let name = metadata
        .get("name")
        .and_then(|n| n.as_str())
        .context("Missing name")?
        .to_string();

    let kind = obj_value
        .get("kind")
        .and_then(|k| k.as_str())
        .context("Missing kind")?
        .to_string();

    let ns = metadata
        .get("namespace")
        .and_then(|n| n.as_str())
        .unwrap_or(namespace)
        .to_string();

    // Extract status
    let ready = obj_value
        .get("status")
        .and_then(|s| s.get("conditions"))
        .and_then(|c| c.as_array())
        .and_then(|arr| {
            arr.iter()
                .find(|c| c.get("type").and_then(|t| t.as_str()) == Some("Ready"))
        })
        .and_then(|c| c.get("status").and_then(|s| s.as_str()))
        .map(|s| s == "True");

    // Extract description from spec based on resource type
    let description = obj_value.get("spec").and_then(|spec| {
        // For GitRepository/OCIRepository, use URL
        if kind == "GitRepository" || kind == "OCIRepository" {
            spec.get("url")
                .and_then(|u| u.as_str())
                .map(|s| s.to_string())
        }
        // For Kustomization, use path
        else if kind == "Kustomization" {
            spec.get("path")
                .and_then(|p| p.as_str())
                .map(|s| s.to_string())
        }
        // For HelmRelease, use chart name or version
        else if kind == "HelmRelease" {
            spec.get("chart")
                .and_then(|c| c.get("spec"))
                .and_then(|cs| cs.get("chart"))
                .and_then(|ch| ch.as_str())
                .map(|s| format!("chart: {}", s))
                .or_else(|| {
                    spec.get("chart")
                        .and_then(|c| c.get("spec"))
                        .and_then(|cs| cs.get("version"))
                        .and_then(|v| v.as_str())
                        .map(|s| format!("version: {}", s))
                })
        } else {
            None
        }
    });

    Ok(GraphNode {
        id: format!("{}:{}:{}", kind, ns, name),
        kind,
        name,
        namespace: ns,
        node_type,
        ready,
        position: None,
        description,
    })
}

/// Fetch a Flux resource and convert to graph node
async fn fetch_flux_resource_as_graph_node(
    client: &kube::Client,
    kind: &str,
    namespace: &str,
    name: &str,
) -> Result<Option<GraphNode>> {
    // Fetch object directly using kube API
    let api_resource = get_api_resource_with_fallback(client, kind, namespace, name).await?;
    let api: Api<DynamicObject> = Api::namespaced_with(client.clone(), namespace, &api_resource);

    match api.get(name).await {
        Ok(obj) => {
            let obj_value = serde_json::to_value(&obj)?;
            Ok(Some(create_graph_node_from_value(
                &obj_value,
                namespace,
                NodeType::Chain,
            )?))
        }
        Err(_) => Ok(None),
    }
}

/// Fetch workload status to display replica information using native kube-rs types
async fn fetch_workload_status(
    client: &kube::Client,
    kind: &str,
    namespace: &str,
    name: &str,
) -> Result<(Option<bool>, Option<String>)> {
    tracing::debug!(
        "Fetching workload status for {}/{} in namespace {}",
        kind,
        name,
        namespace
    );

    let (ready, description) = match kind {
        "Deployment" => {
            let api: Api<Deployment> = Api::namespaced(client.clone(), namespace);
            let deployment = api.get(name).await?;

            let desired_replicas = deployment
                .spec
                .as_ref()
                .and_then(|s| s.replicas)
                .unwrap_or(1);

            let ready_replicas = deployment
                .status
                .as_ref()
                .and_then(|s| s.ready_replicas)
                .unwrap_or(0);

            let available_replicas = deployment
                .status
                .as_ref()
                .and_then(|s| s.available_replicas)
                .unwrap_or(0);

            tracing::debug!(
                "Deployment {}/{} - desired: {}, ready: {}, available: {}",
                namespace,
                name,
                desired_replicas,
                ready_replicas,
                available_replicas
            );

            let is_ready =
                ready_replicas == desired_replicas && available_replicas == desired_replicas;
            let desc = format!("Replicas: {}/{}", ready_replicas, desired_replicas);

            (Some(is_ready), Some(desc))
        }
        "StatefulSet" => {
            let api: Api<StatefulSet> = Api::namespaced(client.clone(), namespace);
            let sts = api.get(name).await?;

            let desired_replicas = sts.spec.as_ref().and_then(|s| s.replicas).unwrap_or(1);

            let ready_replicas = sts
                .status
                .as_ref()
                .and_then(|s| s.ready_replicas)
                .unwrap_or(0);

            tracing::debug!(
                "StatefulSet {}/{} - desired: {}, ready: {}",
                namespace,
                name,
                desired_replicas,
                ready_replicas
            );

            let is_ready = ready_replicas == desired_replicas;
            let desc = format!("Replicas: {}/{}", ready_replicas, desired_replicas);

            (Some(is_ready), Some(desc))
        }
        "DaemonSet" => {
            let api: Api<DaemonSet> = Api::namespaced(client.clone(), namespace);
            let ds = api.get(name).await?;

            let desired_scheduled = ds
                .status
                .as_ref()
                .map(|s| s.desired_number_scheduled)
                .unwrap_or(0);

            let number_ready = ds.status.as_ref().map(|s| s.number_ready).unwrap_or(0);

            tracing::debug!(
                "DaemonSet {}/{} - desired: {}, ready: {}",
                namespace,
                name,
                desired_scheduled,
                number_ready
            );

            let is_ready = number_ready == desired_scheduled && desired_scheduled > 0;
            let desc = format!("Ready: {}/{}", number_ready, desired_scheduled);

            (Some(is_ready), Some(desc))
        }
        "Job" => {
            let api: Api<Job> = Api::namespaced(client.clone(), namespace);
            let job = api.get(name).await?;

            let succeeded = job.status.as_ref().and_then(|s| s.succeeded).unwrap_or(0);

            let failed = job.status.as_ref().and_then(|s| s.failed).unwrap_or(0);

            let is_ready = succeeded > 0;
            let desc = if failed > 0 {
                format!("Failed: {}", failed)
            } else {
                format!("Succeeded: {}", succeeded)
            };

            (Some(is_ready), Some(desc))
        }
        "CronJob" => {
            let api: Api<CronJob> = Api::namespaced(client.clone(), namespace);
            let cronjob = api.get(name).await?;

            let active = cronjob
                .status
                .as_ref()
                .and_then(|s| s.active.as_ref())
                .map(|a| a.len())
                .unwrap_or(0);

            let desc = format!("Active: {}", active);

            (None, Some(desc))
        }
        _ => {
            tracing::warn!("Unknown workload kind: {}", kind);
            (None, None)
        }
    };

    tracing::debug!(
        "Workload {}/{} - ready: {:?}, description: {:?}",
        kind,
        name,
        ready,
        description
    );

    Ok((ready, description))
}

/// Check if a resource kind is a Flux resource
fn is_flux_resource(kind: &str) -> bool {
    FluxResourceKind::parse_optional(kind).is_some()
}
