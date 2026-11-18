//! Graph builder for discovering resource relationships
//!
//! This module provides functionality to build a complete resource graph,
//! including upstream sources and downstream child resources (workloads, etc.).
//! Based on Flux Operator Web UI graph implementation patterns.

use crate::kube::get_api_resource_with_fallback;
use crate::kube::inventory::{extract_inventory, group_inventory};
use crate::models::FluxResourceKind;
use crate::trace::graph::{GraphEdge, GraphNode, NodeType, RelationshipType, ResourceGraph};
use anyhow::{Context, Result};
use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, StatefulSet};
use k8s_openapi::api::batch::v1::{CronJob, Job};
use kube::{Api, core::DynamicObject};
use serde_json::Value;

use crate::watcher::ResourceKey;

/// Check if a resource type supports the graph view
///
/// Uses FluxResourceKind enum to avoid hardcoding resource types.
pub fn is_resource_type_with_graph(resource_type: &str) -> bool {
    FluxResourceKind::parse_optional(resource_type)
        .map(|kind| kind.supports_graph())
        .unwrap_or(false)
}

/// Where `target` shows up in `graph` (#266): the object itself, else the
/// first of its owners the graph lists — a Pod's Deployment sits in the
/// Kustomization's workload group. `None` when neither appears.
pub async fn locate_in_graph(
    client: &kube::Client,
    graph: &ResourceGraph,
    target: &crate::kube::objects::ObjectRef,
) -> Option<crate::watcher::ResourceKey> {
    let own = target.to_resource_key();
    if graph.node_containing(&own).is_some() {
        return Some(own);
    }
    let trace = match crate::trace::trace_target(client, target).await {
        Ok(trace) => trace,
        Err(e) => {
            tracing::debug!(
                "Couldn't trace {} to place it in the graph: {:#}",
                target,
                e
            );
            return None;
        }
    };
    trace
        .path()
        .into_iter()
        .map(crate::trace::TraceNode::key)
        .find(|key| graph.node_containing(key).is_some())
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

/// Hops of source lineage the graph follows above the object's source
/// (ExternalArtifact → ArtifactGenerator → GitRepository → Git origin is 3);
/// bounds a reference cycle.
const MAX_LINEAGE_DEPTH: usize = 8;

/// Discover upstream resources: the object's own source (through any
/// intermediates and that source's own lineage) and the Flux object that
/// manages it.
async fn discover_upstream_resources(
    client: &kube::Client,
    graph: &mut ResourceGraph,
    obj_value: &Value,
    namespace: &str,
    from_node_id: &str,
) -> Result<()> {
    let kind = obj_value.get("kind").and_then(|k| k.as_str()).unwrap_or("");
    let name = obj_value
        .pointer("/metadata/name")
        .and_then(Value::as_str)
        .unwrap_or("");

    // Its own source — not its manager's (see `own_source`): source → any
    // intermediates (HelmChart) → object.
    let mut lineage: Vec<(ResourceKey, String)> = Vec::new();
    if FluxResourceKind::parse_optional(kind).is_some_and(|kind| kind.has_source_ref()) {
        match crate::trace::own_source(client, kind, namespace, name).await {
            Ok((chain, source)) => {
                let mut child = from_node_id.to_string();
                for intermediate in &chain {
                    let key = intermediate.key();
                    let id = graph_id(&key);
                    if let Some(node) = fetch_graph_node(client, &key, NodeType::Chain)
                        .await
                        .map(|(n, _)| n)
                    {
                        add_node_once(graph, node);
                        graph.add_edge(GraphEdge {
                            from: id.clone(),
                            to: child,
                            relationship: RelationshipType::SourcedFrom,
                        });
                        child = id;
                    }
                }
                if let Some(source) = source {
                    lineage.push((source.key(), child));
                }
            }
            Err(e) => tracing::warn!("Couldn't resolve the source of {kind}/{name}: {e:#}"),
        }
    } else {
        // A source-like object (ArtifactGenerator, ExternalArtifact): what it
        // is built from sits above it.
        lineage.extend(
            lineage_refs(obj_value)
                .into_iter()
                .map(|key| (key, from_node_id.to_string())),
        );
        add_origin(graph, obj_value, from_node_id);
    }
    add_lineage(client, graph, lineage).await;

    // The Flux object that applied this one (its labels), beside the source.
    if let Some(crate::kube::ownership::Manager::Flux {
        kind: manager_kind,
        namespace: manager_ns,
        name: manager_name,
    }) = crate::kube::ownership::detect_manager(obj_value)
    {
        let manager_ns = if manager_ns.is_empty() {
            namespace.to_string()
        } else {
            manager_ns
        };
        let key = ResourceKey::new(manager_kind.as_str(), &manager_ns, &manager_name);
        let id = graph_id(&key);
        if id != from_node_id
            && let Some((node, _)) = fetch_graph_node(client, &key, NodeType::Chain).await
        {
            add_node_once(graph, node);
            graph.add_edge(GraphEdge {
                from: id,
                to: from_node_id.to_string(),
                relationship: RelationshipType::ManagedBy,
            });
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
            ) && is_flux_resource(owner_kind)
            {
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
    // Inventory-carrying kinds (Kustomization, ResourceSet, FluxInstance)
    // share the status.inventory discovery path; HelmRelease has its own
    // (inventory lives in Helm storage Secrets).
    let flux_kind = FluxResourceKind::parse_optional(resource_type);

    match flux_kind {
        Some(kind) if kind.has_inventory_downstream() => {
            discover_inventory_resources(
                client,
                graph,
                resource_type,
                namespace,
                name,
                from_node_id,
            )
            .await?;
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

/// Discover managed resources from `status.inventory.entries` — the shared
/// downstream path for Kustomization, ResourceSet, and FluxInstance (see
/// [`FluxResourceKind::has_inventory_downstream`]). Arbitrary inventory kinds
/// (Namespaces, CRDs, custom resources) land in the aggregated resource
/// group; Flux resources become individual navigable nodes.
async fn discover_inventory_resources(
    client: &kube::Client,
    graph: &mut ResourceGraph,
    resource_type: &str,
    namespace: &str,
    name: &str,
    from_node_id: &str,
) -> Result<()> {
    // Fetch the resource to extract its inventory
    let api_resource =
        get_api_resource_with_fallback(client, resource_type, namespace, name).await?;
    let api: Api<DynamicObject> = Api::namespaced_with(client.clone(), namespace, &api_resource);
    let obj = api
        .get(name)
        .await
        .with_context(|| format!("Failed to fetch {}", resource_type))?;
    let obj_value = serde_json::to_value(&obj).context("Failed to serialize resource")?;

    // Extract and group inventory
    if let Ok(entries) = extract_inventory(&obj_value) {
        tracing::debug!(
            "Extracted {} inventory entries from {} {}",
            entries.len(),
            resource_type,
            name
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

                    // Encoded via WorkloadRef — the single source of truth for
                    // the line format the renderer and workload list decode.
                    let status_indicator = match ready {
                        Some(true) => "●",
                        Some(false) => "○",
                        None => "?",
                    };
                    let workload_info = crate::kube::workloads::WorkloadRef {
                        kind: workload_entry.kind.clone(),
                        name: workload_entry.name.clone(),
                        namespace: workload_entry.namespace.clone(),
                        indicator: status_indicator.to_string(),
                        status: status_desc.unwrap_or_else(|| "Unknown".to_string()),
                        api_version: workload_entry.api_version.clone(),
                    }
                    .to_graph_line();
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
            let total_count = groups.resources.len();
            let resource_group_id = format!("resourcegroup:{}", namespace);

            if !graph.node_index.contains_key(&resource_group_id) {
                // One encoded InventoryEntry per line: the node itself renders
                // per-kind counts, and Enter drills into the full breakdown.
                let resource_list: Vec<String> = groups
                    .resources
                    .iter()
                    .map(crate::kube::inventory::InventoryEntry::to_graph_line)
                    .collect();

                let resource_group_node = GraphNode {
                    id: resource_group_id.clone(),
                    kind: "Resources".to_string(),
                    name: format!("Resources ({})", total_count),
                    namespace: namespace.to_string(),
                    node_type: NodeType::ResourceGroup,
                    ready: None,
                    position: None,
                    description: Some(resource_list.join("\n")),
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
            "No inventory found for {} {} in namespace {}",
            resource_type,
            name,
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
            crate::kube::inventory::helm_release_inventory(client, &obj_value, namespace).await?
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
        let total_count = groups.resources.len();
        let resource_group_id = format!("resourcegroup:{}", namespace);

        if !graph.node_index.contains_key(&resource_group_id) {
            // One encoded InventoryEntry per line: the node itself renders
            // per-kind counts, and Enter drills into the full breakdown.
            let resource_list: Vec<String> = groups
                .resources
                .iter()
                .map(crate::kube::inventory::InventoryEntry::to_graph_line)
                .collect();

            let resource_group_node = GraphNode {
                id: resource_group_id.clone(),
                kind: "Resources".to_string(),
                name: format!("Resources ({})", total_count),
                namespace: namespace.to_string(),
                node_type: NodeType::ResourceGroup,
                ready: None,
                position: None,
                description: Some(resource_list.join("\n")),
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

    let ready = crate::kube::conditions::is_ready(obj_value);
    let description = node_description(&kind, obj_value);

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

/// The one-line summary under a node: a repository's URL, a
/// Kustomization's path, or a HelmRelease's chart (else version) — the same
/// extraction as the list's columns.
fn node_description(kind: &str, obj: &Value) -> Option<String> {
    use crate::models::flux_resource_kind::field_names::{CHART, VERSION};
    let spec_str = |field: &str| {
        obj.pointer(&format!("/spec/{field}"))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    match FluxResourceKind::parse_optional(kind)? {
        FluxResourceKind::GitRepository
        | FluxResourceKind::OCIRepository
        | FluxResourceKind::HelmRepository => spec_str("url"),
        FluxResourceKind::Kustomization => spec_str("path"),
        FluxResourceKind::HelmRelease => {
            let fields = FluxResourceKind::HelmRelease.extract_fields(obj);
            fields
                .get(CHART)
                .map(|chart| format!("chart: {chart}"))
                .or_else(|| {
                    fields
                        .get(VERSION)
                        .map(|version| format!("version: {version}"))
                })
        }
        _ => None,
    }
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

/// The graph id of a resource (`kind:namespace:name`).
fn graph_id(key: &ResourceKey) -> String {
    key.to_key_string()
}

/// Add `node` unless the graph already has it.
fn add_node_once(graph: &mut ResourceGraph, node: GraphNode) {
    if !graph.node_index.contains_key(&node.id) {
        graph.add_node(node);
    }
}

/// Fetch a Flux resource as a graph node of `node_type`, with its JSON;
/// `None` (logged) when it can't be read.
async fn fetch_graph_node(
    client: &kube::Client,
    key: &ResourceKey,
    node_type: NodeType,
) -> Option<(GraphNode, Value)> {
    let fetched = async {
        let api_resource =
            get_api_resource_with_fallback(client, &key.resource_type, &key.namespace, &key.name)
                .await?;
        let api: Api<DynamicObject> =
            Api::namespaced_with(client.clone(), &key.namespace, &api_resource);
        let obj = api.get(&key.name).await?;
        let value = serde_json::to_value(&obj)?;
        let node = create_graph_node_from_value(&value, &key.namespace, node_type)?;
        anyhow::Ok((node, value))
    }
    .await;
    match fetched {
        Ok(fetched) => Some(fetched),
        Err(e) => {
            tracing::warn!("Graph: couldn't read {}: {:#}", key.to_key_string(), e);
            None
        }
    }
}

/// What a source-like object comes from ([`crate::kube::sources`]): an
/// ExternalArtifact's producer, a producer's `spec.sources`.
fn lineage_refs(obj: &Value) -> Vec<ResourceKey> {
    use crate::kube::sources;
    let Some(kind) = obj
        .get("kind")
        .and_then(Value::as_str)
        .and_then(FluxResourceKind::parse_optional)
    else {
        return Vec::new();
    };
    let namespace = obj
        .pointer("/metadata/namespace")
        .and_then(Value::as_str)
        .unwrap_or_default();
    sources::producer_ref(kind, obj, namespace)
        .into_iter()
        .chain(sources::built_from(kind, obj, namespace))
        .collect()
}

/// Add the source lineage above each `(source, child id)`: the source as a
/// node feeding the child, then (breadth-first, bounded by
/// [`MAX_LINEAGE_DEPTH`]) whatever that source is built from, and the Git
/// origin of each.
async fn add_lineage(
    client: &kube::Client,
    graph: &mut ResourceGraph,
    start: Vec<(ResourceKey, String)>,
) {
    let mut queue: std::collections::VecDeque<(ResourceKey, String, usize)> = start
        .into_iter()
        .map(|(key, child)| (key, child, 0))
        .collect();
    while let Some((key, child, depth)) = queue.pop_front() {
        let id = graph_id(&key);
        let known = graph.node_index.contains_key(&id);
        if !known {
            let Some((node, value)) = fetch_graph_node(client, &key, NodeType::Source).await else {
                continue;
            };
            graph.add_node(node);
            add_origin(graph, &value, &id);
            if depth < MAX_LINEAGE_DEPTH {
                queue.extend(
                    lineage_refs(&value)
                        .into_iter()
                        .map(|parent| (parent, id.clone(), depth + 1)),
                );
            }
        }
        graph.add_edge(GraphEdge {
            from: id,
            to: child,
            relationship: RelationshipType::SourcedFrom,
        });
    }
}

/// The external origin an artifact records
/// ([`crate::kube::sources::origin_url`]) as an upstream node above it —
/// named by the URL's last path segment, as the Flux Operator Web UI does.
fn add_origin(graph: &mut ResourceGraph, value: &Value, node_id: &str) {
    let Some(url) = crate::kube::sources::origin_url(value) else {
        return;
    };
    let name = url
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .rsplit('/')
        .next()
        .filter(|segment| !segment.is_empty())
        .unwrap_or(&url)
        .to_string();
    let upstream_id = format!("upstream:{url}");
    if !graph.node_index.contains_key(&upstream_id) {
        graph.add_node(GraphNode {
            id: upstream_id.clone(),
            kind: "Upstream".to_string(),
            name,
            namespace: String::new(),
            node_type: NodeType::Upstream,
            ready: None,
            position: None,
            description: Some(url),
        });
    }
    graph.add_edge(GraphEdge {
        from: upstream_id,
        to: node_id.to_string(),
        relationship: RelationshipType::SourcedFrom,
    });
}

/// Check if a resource kind is a Flux resource
fn is_flux_resource(kind: &str) -> bool {
    FluxResourceKind::parse_optional(kind).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use crate::kube::fake_api::{FakeCluster, fixtures, flux};

    /// Every edge above the object (inventory edges left out), sorted.
    fn upstream_edges(graph: &ResourceGraph) -> Vec<String> {
        let mut edges: Vec<String> = graph
            .edges
            .iter()
            .filter(|e| e.relationship != RelationshipType::Owns)
            .map(|e| format!("{} -> {} ({:?})", e.from, e.to, e.relationship))
            .collect();
        edges.sort();
        edges
    }

    /// The user-reported graph: the child's own lineage — artifact ←
    /// generator ← repository — with the parent beside it as manager, and
    /// no edge from the parent's `fleet` repository.
    #[tokio::test]
    async fn a_managed_kustomizations_graph_shows_its_own_lineage_and_its_manager() {
        let cluster = FakeCluster::new(fixtures::external_artifact_pipeline());
        let graph = build_resource_graph(&cluster.client(), "Kustomization", "team-a", "apps")
            .await
            .unwrap();
        assert_eq!(
            upstream_edges(&graph),
            [
                "ArtifactGenerator:flux-system:gen -> ExternalArtifact:flux-system:apps (SourcedFrom)",
                "ExternalArtifact:flux-system:apps -> Kustomization:team-a:apps (SourcedFrom)",
                "GitRepository:flux-system:repo -> ArtifactGenerator:flux-system:gen (SourcedFrom)",
                "Kustomization:flux-system:flux-system -> Kustomization:team-a:apps (ManagedBy)",
            ]
        );
        assert!(
            !graph
                .node_index
                .contains_key("GitRepository:flux-system:fleet")
        );
    }

    /// A HelmRelease's generated HelmChart sits between it and the
    /// repository (not beside them), and a repository keeps its URL.
    #[tokio::test]
    async fn a_helm_releases_graph_runs_through_its_helm_chart() {
        let cluster = FakeCluster::new(fixtures::helm_pipeline());
        let graph = build_resource_graph(&cluster.client(), "HelmRelease", "apps", "web")
            .await
            .unwrap();
        assert_eq!(
            upstream_edges(&graph),
            [
                "HelmChart:flux-system:apps-web -> HelmRelease:apps:web (SourcedFrom)",
                "HelmRepository:flux-system:charts -> HelmChart:flux-system:apps-web (SourcedFrom)",
                "Kustomization:flux-system:flux-system -> HelmRelease:apps:web (ManagedBy)",
            ]
        );
        let repo = &graph.nodes[graph.node_index["HelmRepository:flux-system:charts"]];
        assert_eq!(
            repo.description.as_deref(),
            Some("https://charts.example.com")
        );
    }

    /// The origin an OCI artifact records becomes an Upstream node above
    /// its repository, named by the URL's last segment.
    #[tokio::test]
    async fn an_artifacts_recorded_origin_is_an_upstream_node() {
        let cluster = FakeCluster::new([
            flux(
                "OCIRepository",
                "apps",
                "podinfo",
                json!({
                "spec": {"url": "oci://ghcr.io/stefanprodan/charts/podinfo"},
                "status": {"artifact": {"metadata": {
                    "org.opencontainers.image.source": "https://github.com/stefanprodan/podinfo"}}}}),
            ),
            flux(
                "Kustomization",
                "apps",
                "podinfo",
                json!({"spec":
                {"sourceRef": {"kind": "OCIRepository", "name": "podinfo"}}}),
            ),
        ]);
        let graph = build_resource_graph(&cluster.client(), "Kustomization", "apps", "podinfo")
            .await
            .unwrap();
        let origin = "upstream:https://github.com/stefanprodan/podinfo";
        assert_eq!(
            upstream_edges(&graph),
            [
                "OCIRepository:apps:podinfo -> Kustomization:apps:podinfo (SourcedFrom)"
                    .to_string(),
                format!("{origin} -> OCIRepository:apps:podinfo (SourcedFrom)"),
            ]
        );
        let node = &graph.nodes[graph.node_index[origin]];
        assert_eq!(
            (node.kind.as_str(), node.name.as_str()),
            ("Upstream", "podinfo")
        );
    }

    /// An ArtifactGenerator's own graph shows what it is built from above it.
    #[tokio::test]
    async fn a_generators_graph_shows_its_sources_above_it() {
        let cluster = FakeCluster::new(fixtures::external_artifact_pipeline());
        let graph =
            build_resource_graph(&cluster.client(), "ArtifactGenerator", "flux-system", "gen")
                .await
                .unwrap();
        assert_eq!(
            upstream_edges(&graph),
            ["GitRepository:flux-system:repo -> ArtifactGenerator:flux-system:gen (SourcedFrom)"]
        );
    }
}
