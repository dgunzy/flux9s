//! Core trace implementation

use anyhow::{Context, Result};
use serde_json::Value;

use crate::models::FluxResourceKind;
use crate::trace::models::{SourceRef, TraceNode, TraceResult, TraceSpec, TraceStatus};
use crate::tui::get_api_resource_with_fallback;

/// Trace a Kubernetes object to find its Flux source
pub async fn trace_object(
    client: &kube::Client,
    resource_type: &str,
    namespace: &str,
    name: &str,
) -> Result<TraceResult> {
    use kube::Api;
    use kube::core::DynamicObject;

    // Get ApiResource with version fallback (version-agnostic)
    let api_resource =
        get_api_resource_with_fallback(client, resource_type, namespace, name).await?;
    let api: Api<DynamicObject> = Api::namespaced_with(client.clone(), namespace, &api_resource);

    // Get the initial object
    let obj = api.get(name).await.context("Failed to fetch object")?;

    // Convert DynamicObject to JSON Value
    let obj_value = serde_json::to_value(&obj).context("Failed to serialize object to JSON")?;

    let current_ns = namespace.to_string();

    // Create initial node
    let initial_node = create_trace_node(&obj_value, &current_ns)?;

    // Check if the initial object itself is a Kustomization or HelmRelease
    use crate::models::FluxResourceKind;
    let initial_kind = FluxResourceKind::parse_optional(resource_type);
    if matches!(
        initial_kind,
        Some(FluxResourceKind::Kustomization) | Some(FluxResourceKind::HelmRelease)
    ) {
        // For Kustomization/HelmRelease, first check if it's managed by another Flux resource
        // (e.g., a Kustomization managed by another Kustomization via labels)
        // Don't add the traced object to chain - it's already shown as "Object"
        // Only add managing resources to the chain (matches Flux CLI behavior)
        let chain = Vec::new();

        // Clone values before moving
        let obj_clone = obj.clone();
        let current_ns_clone = current_ns.clone();

        // Walk up the owner reference chain to find any managing Flux resources
        let (walked_chain, walked_source) =
            walk_owner_chain(client, chain, obj_clone, current_ns_clone).await?;

        // If we found a managing Flux resource in the chain, use that chain and source
        if walked_source.is_some() || !walked_chain.is_empty() {
            return Ok(TraceResult {
                object: initial_node,
                chain: walked_chain,
                source: walked_source,
            });
        }

        // Otherwise, resolve the source directly (this Kustomization/HelmRelease is not managed by another)
        // Don't add the object to chain - it's already shown as "Object"
        let mut chain_for_source = Vec::new();
        let source =
            resolve_source(client, &mut chain_for_source, &initial_node, &current_ns).await?;
        return Ok(TraceResult {
            object: initial_node,
            chain: chain_for_source,
            source,
        });
    }

    // For non-Kustomization/HelmRelease objects, start with the object in the chain
    let chain = vec![initial_node.clone()];

    // Walk up the owner reference chain iteratively
    let (final_chain, final_source) = walk_owner_chain(client, chain, obj, current_ns).await?;

    Ok(TraceResult {
        object: initial_node,
        chain: final_chain,
        source: final_source,
    })
}

/// Iteratively walk the owner reference chain to find Flux resources
async fn walk_owner_chain(
    client: &kube::Client,
    mut chain: Vec<TraceNode>,
    mut current_obj: kube::core::DynamicObject,
    mut current_ns: String,
) -> Result<(Vec<TraceNode>, Option<TraceNode>)> {
    // Use iterative approach instead of recursion to avoid boxing issues
    loop {
        // Convert current_obj to JSON Value for processing
        let current_obj_value =
            serde_json::to_value(&current_obj).context("Failed to serialize object to JSON")?;

        // Check for Flux labels first (kustomize.toolkit.fluxcd.io/name, etc.)
        if let Some(flux_owner) = find_flux_owner_from_labels(&current_obj_value) {
            let owner_ns = flux_owner
                .namespace
                .as_deref()
                .unwrap_or(&current_ns)
                .to_string();

            // Check if we're already tracking this owner in the chain (avoid duplicates)
            let already_in_chain = chain.iter().any(|node| {
                node.kind == flux_owner.kind
                    && node.name == flux_owner.name
                    && node.namespace == owner_ns
            });

            if !already_in_chain {
                if let Some(owner_node) = fetch_and_trace_flux_resource(
                    client,
                    &flux_owner.kind,
                    &owner_ns,
                    &flux_owner.name,
                )
                .await?
                {
                    chain.push(owner_node.clone());

                    // If this is a Kustomization or HelmRelease, resolve its source
                    if matches!(
                        FluxResourceKind::parse_optional(&flux_owner.kind),
                        Some(FluxResourceKind::Kustomization) | Some(FluxResourceKind::HelmRelease)
                    ) {
                        let source =
                            resolve_source(client, &mut chain, &owner_node, &owner_ns).await?;
                        return Ok((chain, source));
                    }

                    // Continue walking from this Flux owner
                    current_obj =
                        fetch_object(client, &flux_owner.kind, &owner_ns, &flux_owner.name).await?;
                    current_ns = owner_ns;
                    continue;
                }
            }
        }

        // Check owner references
        let owner_refs = current_obj_value
            .get("metadata")
            .and_then(|m| m.get("ownerReferences"))
            .and_then(|o| o.as_array())
            .cloned()
            .unwrap_or_default();

        let mut found_owner = false;
        for owner_ref in owner_refs {
            let owner_kind = owner_ref
                .get("kind")
                .and_then(|k| k.as_str())
                .context("Owner reference missing kind")?;
            let owner_name = owner_ref
                .get("name")
                .and_then(|n| n.as_str())
                .context("Owner reference missing name")?;
            let owner_uid = owner_ref.get("uid").and_then(|u| u.as_str());

            // Fetch the owner object
            let owner_obj = match fetch_object(client, owner_kind, &current_ns, owner_name).await {
                Ok(obj) => obj,
                Err(e) => {
                    tracing::warn!("Failed to fetch owner {}/{}: {}", owner_kind, owner_name, e);
                    continue;
                }
            };

            // Verify UID matches if provided
            if let Some(uid) = owner_uid {
                let obj_uid = owner_obj.metadata.uid.as_deref().unwrap_or("");
                if obj_uid != uid {
                    tracing::warn!(
                        "Owner UID mismatch for {}/{}: expected {}, got {}",
                        owner_kind,
                        owner_name,
                        uid,
                        obj_uid
                    );
                    continue;
                }
            }

            // Check if it's a Flux resource
            if is_flux_resource(owner_kind) {
                let owner_node = match fetch_and_trace_flux_resource(
                    client,
                    owner_kind,
                    &current_ns,
                    owner_name,
                )
                .await?
                {
                    Some(node) => node,
                    None => continue,
                };

                // Check if we're already tracking this owner in the chain (avoid duplicates)
                let already_in_chain = chain.iter().any(|n| {
                    n.kind == owner_node.kind
                        && n.name == owner_node.name
                        && n.namespace == owner_node.namespace
                });

                if !already_in_chain {
                    chain.push(owner_node.clone());
                }

                // If this is a Kustomization or HelmRelease, resolve its source
                if matches!(
                    FluxResourceKind::parse_optional(owner_kind),
                    Some(FluxResourceKind::Kustomization) | Some(FluxResourceKind::HelmRelease)
                ) {
                    let source =
                        resolve_source(client, &mut chain, &owner_node, &current_ns).await?;
                    return Ok((chain, source));
                }

                // Continue walking from this Flux owner
                current_obj = owner_obj;
                found_owner = true;
                break;
            } else {
                // Non-Flux owner, add to chain and continue walking
                let owner_node =
                    match fetch_and_trace_resource(client, owner_kind, &current_ns, owner_name)
                        .await?
                    {
                        Some(node) => node,
                        None => continue,
                    };

                // Check if we're already tracking this owner in the chain (avoid duplicates)
                let already_in_chain = chain.iter().any(|n| {
                    n.kind == owner_node.kind
                        && n.name == owner_node.name
                        && n.namespace == owner_node.namespace
                });

                if !already_in_chain {
                    chain.push(owner_node.clone());
                }

                // Continue walking
                current_obj = owner_obj;
                found_owner = true;
                break;
            }
        }

        // No more owners found
        if !found_owner {
            return Ok((chain, None));
        }
    }
}

/// Resolve the source for a Kustomization or HelmRelease
/// Adds intermediate resources (like HelmChart) to the chain
async fn resolve_source(
    client: &kube::Client,
    chain: &mut Vec<TraceNode>,
    node: &TraceNode,
    default_ns: &str,
) -> Result<Option<TraceNode>> {
    use crate::models::FluxResourceKind;

    match FluxResourceKind::parse_optional(&node.kind) {
        Some(FluxResourceKind::Kustomization) => {
            resolve_kustomization_source(client, node, default_ns).await
        }
        Some(FluxResourceKind::HelmRelease) => {
            resolve_helmrelease_source(client, chain, node, default_ns).await
        }
        _ => Ok(None),
    }
}

/// Resolve source for a Kustomization
async fn resolve_kustomization_source(
    client: &kube::Client,
    node: &TraceNode,
    default_ns: &str,
) -> Result<Option<TraceNode>> {
    // Fetch the Kustomization object to get its spec
    let ks_obj = fetch_object(client, &node.kind, &node.namespace, &node.name).await?;
    let ks_value = serde_json::to_value(&ks_obj)?;

    // Extract sourceRef from spec
    let source_ref = ks_value
        .get("spec")
        .and_then(|s| s.get("sourceRef"))
        .and_then(|sr| sr.as_object());

    if let Some(sr) = source_ref {
        let source_kind = sr
            .get("kind")
            .and_then(|k| k.as_str())
            .context("sourceRef missing kind")?;
        let source_name = sr
            .get("name")
            .and_then(|n| n.as_str())
            .context("sourceRef missing name")?;
        let source_ns = sr
            .get("namespace")
            .and_then(|n| n.as_str())
            .unwrap_or(default_ns);

        // Handle ExternalArtifact - validate it has sourceRef
        if source_kind == "ExternalArtifact" {
            let ea_obj = fetch_object(client, source_kind, source_ns, source_name).await?;
            let ea_value = serde_json::to_value(&ea_obj)?;
            if ea_value
                .get("spec")
                .and_then(|s| s.get("sourceRef"))
                .is_none()
            {
                return Err(anyhow::anyhow!(
                    "ExternalArtifact {}/{} is missing spec.sourceRef",
                    source_ns,
                    source_name
                ));
            }
        }

        return fetch_and_trace_flux_resource(client, source_kind, source_ns, source_name).await;
    }

    Ok(None)
}

/// Resolve source for a HelmRelease
/// Handles:
/// 1. HelmRelease.status.helmChart -> HelmChart -> source
/// 2. HelmRelease.spec.chartRef -> OCIRepository/ExternalArtifact
/// 3. HelmRelease.spec.chart.spec.sourceRef -> GitRepository/HelmRepository
async fn resolve_helmrelease_source(
    client: &kube::Client,
    chain: &mut Vec<TraceNode>,
    node: &TraceNode,
    default_ns: &str,
) -> Result<Option<TraceNode>> {
    // Fetch the HelmRelease object
    let hr_obj = fetch_object(client, &node.kind, &node.namespace, &node.name).await?;
    let hr_value = serde_json::to_value(&hr_obj)?;

    // First, check if there's a HelmChart referenced in status
    if let Some(helm_chart_ref) = hr_value
        .get("status")
        .and_then(|s| s.get("helmChart"))
        .and_then(|hc| hc.as_str())
    {
        // Parse the HelmChart reference (format: namespace/name)
        let (chart_ns, chart_name) = parse_namespaced_name(helm_chart_ref, default_ns);

        // Fetch the HelmChart and add it to the chain
        if let Some(chart_node) =
            fetch_and_trace_flux_resource(client, "HelmChart", &chart_ns, &chart_name).await?
        {
            // Add HelmChart to chain
            chain.push(chart_node.clone());
            // Resolve the HelmChart's source
            return resolve_helmchart_source(client, &chart_node, &chart_ns).await;
        }
    }

    // Check spec.chartRef (for OCIRepository, ExternalArtifact, or HelmChart)
    if let Some(chart_ref) = hr_value
        .get("spec")
        .and_then(|s| s.get("chartRef"))
        .and_then(|cr| cr.as_object())
    {
        let chart_ref_kind = chart_ref
            .get("kind")
            .and_then(|k| k.as_str())
            .context("chartRef missing kind")?;
        let chart_ref_name = chart_ref
            .get("name")
            .and_then(|n| n.as_str())
            .context("chartRef missing name")?;
        let chart_ref_ns = chart_ref
            .get("namespace")
            .and_then(|n| n.as_str())
            .unwrap_or(default_ns);

        match chart_ref_kind {
            "HelmChart" => {
                // If chartRef points to HelmChart, add it to chain and resolve its source
                if let Some(chart_node) =
                    fetch_and_trace_flux_resource(client, "HelmChart", chart_ref_ns, chart_ref_name)
                        .await?
                {
                    // Add HelmChart to chain
                    chain.push(chart_node.clone());
                    return resolve_helmchart_source(client, &chart_node, chart_ref_ns).await;
                }
            }
            "OCIRepository" => {
                // Direct source reference
                return fetch_and_trace_flux_resource(
                    client,
                    chart_ref_kind,
                    chart_ref_ns,
                    chart_ref_name,
                )
                .await;
            }
            "ExternalArtifact" => {
                // Validate ExternalArtifact has sourceRef
                let ea_obj =
                    fetch_object(client, chart_ref_kind, chart_ref_ns, chart_ref_name).await?;
                let ea_value = serde_json::to_value(&ea_obj)?;
                if ea_value
                    .get("spec")
                    .and_then(|s| s.get("sourceRef"))
                    .is_none()
                {
                    return Err(anyhow::anyhow!(
                        "ExternalArtifact {}/{} is missing spec.sourceRef",
                        chart_ref_ns,
                        chart_ref_name
                    ));
                }
                // Direct source reference
                return fetch_and_trace_flux_resource(
                    client,
                    chart_ref_kind,
                    chart_ref_ns,
                    chart_ref_name,
                )
                .await;
            }
            _ => {
                tracing::warn!("Unsupported chartRef kind: {}", chart_ref_kind);
            }
        }
    }

    // Check spec.chart.spec.sourceRef (for GitRepository or HelmRepository)
    if let Some(chart_spec) = hr_value
        .get("spec")
        .and_then(|s| s.get("chart"))
        .and_then(|c| c.get("spec"))
        .and_then(|cs| cs.as_object())
    {
        if let Some(source_ref) = chart_spec.get("sourceRef").and_then(|sr| sr.as_object()) {
            let source_kind = source_ref
                .get("kind")
                .and_then(|k| k.as_str())
                .context("chart.spec.sourceRef missing kind")?;
            let source_name = source_ref
                .get("name")
                .and_then(|n| n.as_str())
                .context("chart.spec.sourceRef missing name")?;
            let source_ns = source_ref
                .get("namespace")
                .and_then(|n| n.as_str())
                .unwrap_or(default_ns);

            return fetch_and_trace_flux_resource(client, source_kind, source_ns, source_name)
                .await;
        }
    }

    Ok(None)
}

/// Resolve source for a HelmChart
async fn resolve_helmchart_source(
    client: &kube::Client,
    chart_node: &TraceNode,
    default_ns: &str,
) -> Result<Option<TraceNode>> {
    // Fetch the HelmChart object to get its spec
    let chart_obj = fetch_object(
        client,
        &chart_node.kind,
        &chart_node.namespace,
        &chart_node.name,
    )
    .await?;
    let chart_value = serde_json::to_value(&chart_obj)?;

    // Extract sourceRef from spec
    let source_ref = chart_value
        .get("spec")
        .and_then(|s| s.get("sourceRef"))
        .and_then(|sr| sr.as_object());

    if let Some(sr) = source_ref {
        let source_kind = sr
            .get("kind")
            .and_then(|k| k.as_str())
            .context("HelmChart sourceRef missing kind")?;
        let source_name = sr
            .get("name")
            .and_then(|n| n.as_str())
            .context("HelmChart sourceRef missing name")?;
        let source_ns = sr
            .get("namespace")
            .and_then(|n| n.as_str())
            .unwrap_or(default_ns);

        return fetch_and_trace_flux_resource(client, source_kind, source_ns, source_name).await;
    }

    Ok(None)
}

/// Parse a namespaced name reference (format: "namespace/name" or just "name")
fn parse_namespaced_name(ref_str: &str, default_ns: &str) -> (String, String) {
    if let Some((ns, name)) = ref_str.split_once('/') {
        (ns.to_string(), name.to_string())
    } else {
        (default_ns.to_string(), ref_str.to_string())
    }
}

/// Check if a resource kind is a Flux resource
fn is_flux_resource(kind: &str) -> bool {
    FluxResourceKind::parse_optional(kind).is_some()
}

/// Find Flux owner from labels
fn find_flux_owner_from_labels(obj: &Value) -> Option<SourceRef> {
    if let Some(labels) = obj
        .get("metadata")
        .and_then(|m| m.get("labels"))
        .and_then(|l| l.as_object())
    {
        // Check for kustomize.toolkit.fluxcd.io/name
        if let Some(name) = labels
            .get("kustomize.toolkit.fluxcd.io/name")
            .and_then(|n| n.as_str())
        {
            return Some(SourceRef {
                kind: FluxResourceKind::Kustomization.as_str().to_string(),
                name: name.to_string(),
                namespace: labels
                    .get("kustomize.toolkit.fluxcd.io/namespace")
                    .and_then(|n| n.as_str())
                    .map(|s| s.to_string()),
            });
        }
        // Check for helm.toolkit.fluxcd.io/name
        if let Some(name) = labels
            .get("helm.toolkit.fluxcd.io/name")
            .and_then(|n| n.as_str())
        {
            return Some(SourceRef {
                kind: FluxResourceKind::HelmRelease.as_str().to_string(),
                name: name.to_string(),
                namespace: labels
                    .get("helm.toolkit.fluxcd.io/namespace")
                    .and_then(|n| n.as_str())
                    .map(|s| s.to_string()),
            });
        }
    }
    None
}

/// Fetch and trace a Flux resource
async fn fetch_and_trace_flux_resource(
    client: &kube::Client,
    kind: &str,
    namespace: &str,
    name: &str,
) -> Result<Option<TraceNode>> {
    fetch_and_trace_resource(client, kind, namespace, name).await
}

/// Fetch and trace any resource
async fn fetch_and_trace_resource(
    client: &kube::Client,
    kind: &str,
    namespace: &str,
    name: &str,
) -> Result<Option<TraceNode>> {
    let obj = match fetch_object(client, kind, namespace, name).await {
        Ok(obj) => obj,
        Err(e) => {
            tracing::warn!("Failed to fetch {}/{}: {}", kind, name, e);
            return Ok(None);
        }
    };
    let obj_value = serde_json::to_value(&obj).context("Failed to serialize object to JSON")?;
    create_trace_node(&obj_value, namespace).map(Some)
}

/// Fetch a Kubernetes object
async fn fetch_object(
    client: &kube::Client,
    kind: &str,
    namespace: &str,
    name: &str,
) -> Result<kube::core::DynamicObject> {
    use kube::Api;
    use kube::core::DynamicObject;

    // Get ApiResource with version fallback (version-agnostic)
    let api_resource = get_api_resource_with_fallback(client, kind, namespace, name).await?;
    let api: Api<DynamicObject> = Api::namespaced_with(client.clone(), namespace, &api_resource);

    api.get(name)
        .await
        .with_context(|| format!("Failed to fetch {}/{}", kind, name))
}

/// Create a trace node from a JSON object
fn create_trace_node(obj: &Value, namespace: &str) -> Result<TraceNode> {
    let metadata = obj
        .get("metadata")
        .and_then(|m| m.as_object())
        .context("Missing metadata")?;

    let kind = obj
        .get("kind")
        .and_then(|k| k.as_str())
        .context("Missing kind")?
        .to_string();
    let name = metadata
        .get("name")
        .and_then(|n| n.as_str())
        .context("Missing name")?
        .to_string();
    let ns = metadata
        .get("namespace")
        .and_then(|n| n.as_str())
        .unwrap_or(namespace)
        .to_string();

    // Extract status - properly find Ready condition
    let status = obj.get("status").and_then(|s| s.as_object()).map(|s| {
        let mut ready = None;
        let mut message = None;

        // Find Ready condition
        if let Some(conditions) = s.get("conditions").and_then(|c| c.as_array()) {
            for cond in conditions {
                if let Some(cond_type) = cond.get("type").and_then(|t| t.as_str()) {
                    if cond_type == "Ready" {
                        ready = cond
                            .get("status")
                            .and_then(|st| st.as_str())
                            .map(|st| st == "True");
                        message = cond
                            .get("message")
                            .and_then(|m| m.as_str())
                            .map(|s| s.to_string());
                        break;
                    }
                }
            }
        }

        TraceStatus {
            ready,
            message,
            last_reconciled: s
                .get("lastHandledReconcileAt")
                .and_then(|t| t.as_str())
                .map(|s| s.to_string()),
            revision: s
                .get("artifact")
                .and_then(|a| a.get("revision"))
                .and_then(|r| r.as_str())
                .map(|s| s.to_string()),
        }
    });

    // Extract spec
    let spec = obj.get("spec").and_then(|s| s.as_object()).map(|s| {
        let mut trace_spec = TraceSpec {
            path: None,
            url: None,
            branch: None,
            source_ref: None,
        };

        // Extract path (for Kustomization)
        if let Some(path) = s.get("path").and_then(|p| p.as_str()) {
            trace_spec.path = Some(path.to_string());
        }

        // Extract URL (for GitRepository, OCIRepository)
        if let Some(url) = s.get("url").and_then(|u| u.as_str()) {
            trace_spec.url = Some(url.to_string());
        }

        // Extract branch (for GitRepository)
        if let Some(branch) = s.get("branch").and_then(|b| b.as_str()) {
            trace_spec.branch = Some(branch.to_string());
        }

        // Extract sourceRef (for Kustomization, HelmRelease, HelmChart, ExternalArtifact)
        if let Some(source_ref) = s.get("sourceRef").and_then(|sr| sr.as_object()) {
            if let (Some(kind), Some(name)) = (
                source_ref.get("kind").and_then(|k| k.as_str()),
                source_ref.get("name").and_then(|n| n.as_str()),
            ) {
                trace_spec.source_ref = Some(SourceRef {
                    kind: kind.to_string(),
                    name: name.to_string(),
                    namespace: source_ref
                        .get("namespace")
                        .and_then(|n| n.as_str())
                        .map(|s| s.to_string()),
                });
            }
        }

        trace_spec
    });

    Ok(TraceNode {
        kind,
        name,
        namespace: ns,
        status,
        spec,
    })
}
