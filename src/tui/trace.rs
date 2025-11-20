//! Flux trace functionality
//!
//! Traces the ownership chain of Kubernetes objects to find their Flux sources.
//! Similar to `flux trace` command - walks up the owner reference chain to find
//! Kustomization or HelmRelease, then resolves their sources.

use anyhow::{Context, Result};
use serde_json::Value;

use crate::tui::api::get_api_resource_with_fallback;

/// Trace result showing the ownership chain
#[derive(Debug, Clone)]
pub struct TraceResult {
    /// The original object being traced
    pub object: TraceNode,
    /// The chain of owners leading to Flux source
    pub chain: Vec<TraceNode>,
    /// The Flux source (GitRepository, OCIRepository, etc.)
    pub source: Option<TraceNode>,
}

/// A node in the trace chain
#[derive(Debug, Clone)]
pub struct TraceNode {
    pub kind: String,
    pub name: String,
    pub namespace: String,
    pub status: Option<TraceStatus>,
    pub spec: Option<TraceSpec>,
}

/// Status information from a Flux resource
#[derive(Debug, Clone)]
pub struct TraceStatus {
    pub ready: Option<bool>,
    pub message: Option<String>,
    pub last_reconciled: Option<String>,
    pub revision: Option<String>,
}

/// Spec information from a Flux resource
#[derive(Debug, Clone)]
pub struct TraceSpec {
    pub path: Option<String>,          // For Kustomization
    pub url: Option<String>,           // For GitRepository, OCIRepository
    pub branch: Option<String>,        // For GitRepository
    pub source_ref: Option<SourceRef>, // For Kustomization, HelmRelease
}

/// Source reference from Kustomization or HelmRelease
#[derive(Debug, Clone)]
pub struct SourceRef {
    pub kind: String,
    pub name: String,
    pub namespace: Option<String>,
}

/// Trace a Kubernetes object to find its Flux source
pub async fn trace_object(
    client: &kube::Client,
    resource_type: &str,
    namespace: &str,
    name: &str,
) -> Result<TraceResult> {
    use kube::core::DynamicObject;
    use kube::Api;

    // Get ApiResource with version fallback (version-agnostic)
    let api_resource =
        get_api_resource_with_fallback(client, resource_type, namespace, name).await?;
    let api: Api<DynamicObject> = Api::namespaced_with(client.clone(), namespace, &api_resource);

    // Get the initial object
    let obj = api.get(name).await.context("Failed to fetch object")?;

    // Convert DynamicObject to JSON Value
    let obj_value = serde_json::to_value(&obj).context("Failed to serialize object to JSON")?;

    let mut chain = Vec::new();
    let mut current_obj = obj;
    let current_ns = namespace.to_string();

    // Create initial node
    let initial_node = create_trace_node(&obj_value, &current_ns)?;
    chain.push(initial_node.clone());

    // Walk up the owner reference chain
    loop {
        // Convert current_obj to JSON Value for processing
        let current_obj_value =
            serde_json::to_value(&current_obj).context("Failed to serialize object to JSON")?;

        // Check for Flux labels first (kustomize.toolkit.fluxcd.io/name, etc.)
        if let Some(flux_owner) = find_flux_owner_from_labels(&current_obj_value) {
            if let Some(owner_node) = fetch_and_trace_flux_resource(
                client,
                &flux_owner.kind,
                &flux_owner.namespace.unwrap_or_else(|| current_ns.clone()),
                &flux_owner.name,
            )
            .await?
            {
                chain.push(owner_node.clone());

                // If this is a Kustomization or HelmRelease, resolve its source
                use crate::models::FluxResourceKind;
                if matches!(
                    FluxResourceKind::from_str(&flux_owner.kind),
                    Some(FluxResourceKind::Kustomization) | Some(FluxResourceKind::HelmRelease)
                ) {
                    let source = resolve_source(client, &owner_node, &current_ns).await?;
                    return Ok(TraceResult {
                        object: initial_node,
                        chain,
                        source,
                    });
                }
                break;
            }
        }

        // Check owner references - extract owner info first to avoid borrow issues
        let owner_info = current_obj_value
            .get("metadata")
            .and_then(|m| m.get("ownerReferences"))
            .and_then(|o| o.as_array())
            .and_then(|refs| {
                for owner_ref in refs {
                    if let Some(owner_kind) = owner_ref.get("kind").and_then(|k| k.as_str()) {
                        if let Some(owner_name) = owner_ref.get("name").and_then(|n| n.as_str()) {
                            return Some((owner_kind.to_string(), owner_name.to_string()));
                        }
                    }
                }
                None
            });

        if let Some((owner_kind, owner_name)) = owner_info {
            // Check if it's a Flux resource
            if is_flux_resource(&owner_kind) {
                if let Some(owner_node) =
                    fetch_and_trace_flux_resource(client, &owner_kind, &current_ns, &owner_name)
                        .await?
                {
                    chain.push(owner_node.clone());
                    current_obj =
                        fetch_object(client, &owner_kind, &current_ns, &owner_name).await?;

                    // If this is a Kustomization or HelmRelease, resolve its source
                    use crate::models::FluxResourceKind;
                    if matches!(
                        FluxResourceKind::from_str(&owner_kind),
                        Some(FluxResourceKind::Kustomization) | Some(FluxResourceKind::HelmRelease)
                    ) {
                        let source = resolve_source(client, &owner_node, &current_ns).await?;
                        return Ok(TraceResult {
                            object: initial_node,
                            chain,
                            source,
                        });
                    }
                    continue; // Continue to next iteration
                }
            } else {
                // Non-Flux owner, fetch it and continue
                if let Some(owner_node) =
                    fetch_and_trace_resource(client, &owner_kind, &current_ns, &owner_name).await?
                {
                    chain.push(owner_node.clone());
                    current_obj =
                        fetch_object(client, &owner_kind, &current_ns, &owner_name).await?;
                    continue; // Continue to next iteration
                }
            }
        }

        // No more owners found
        break;
    }

    Ok(TraceResult {
        object: initial_node,
        chain,
        source: None,
    })
}

/// Resolve the source for a Kustomization or HelmRelease
async fn resolve_source(
    client: &kube::Client,
    node: &TraceNode,
    default_ns: &str,
) -> Result<Option<TraceNode>> {
    if let Some(ref spec) = node.spec {
        if let Some(ref source_ref) = spec.source_ref {
            let source_ns = source_ref.namespace.as_deref().unwrap_or(default_ns);
            return fetch_and_trace_flux_resource(
                client,
                &source_ref.kind,
                source_ns,
                &source_ref.name,
            )
            .await;
        }
    }
    Ok(None)
}

/// Check if a resource kind is a Flux resource
fn is_flux_resource(kind: &str) -> bool {
    use crate::models::FluxResourceKind;
    FluxResourceKind::from_str(kind).is_some()
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
            use crate::models::FluxResourceKind;
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
            use crate::models::FluxResourceKind;
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
    let obj = fetch_object(client, kind, namespace, name).await?;
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
    use kube::core::DynamicObject;
    use kube::Api;

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

    // Extract status
    let status = obj
        .get("status")
        .and_then(|s| s.as_object())
        .map(|s| TraceStatus {
            ready: s
                .get("conditions")
                .and_then(|c| c.as_array())
                .and_then(|c| {
                    c.iter()
                        .find(|cond| {
                            cond.get("type")
                                .and_then(|t| t.as_str())
                                .map(|t| t == "Ready")
                                .unwrap_or(false)
                        })
                        .and_then(|cond| cond.get("status").and_then(|st| st.as_str()))
                        .map(|st| st == "True")
                }),
            message: s
                .get("conditions")
                .and_then(|c| c.as_array())
                .and_then(|c| {
                    c.iter()
                        .find(|cond| {
                            cond.get("type")
                                .and_then(|t| t.as_str())
                                .map(|t| t == "Ready")
                                .unwrap_or(false)
                        })
                        .and_then(|cond| cond.get("message").and_then(|m| m.as_str()))
                        .map(|s| s.to_string())
                }),
            last_reconciled: s
                .get("lastHandledReconcileAt")
                .and_then(|t| t.as_str())
                .map(|s| s.to_string()),
            revision: s
                .get("artifact")
                .and_then(|a| a.get("revision"))
                .and_then(|r| r.as_str())
                .map(|s| s.to_string()),
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

        // Extract sourceRef (for Kustomization, HelmRelease)
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

/// Format trace result as a string (similar to flux trace output)
pub fn format_trace_result(result: &TraceResult) -> String {
    let mut output = Vec::new();

    // Main Object header - highlighted as the primary resource
    output.push("═══════════════════════════════════════════════════════════".to_string());
    output.push(format!(
        "  Object:          {}/{}",
        result.object.kind, result.object.name
    ));
    output.push(format!("  Namespace:       {}", result.object.namespace));
    output.push("  Status:          Managed by Flux".to_string());
    output.push("═══════════════════════════════════════════════════════════".to_string());

    // Chain (Kustomization/HelmRelease)
    for node in &result.chain {
        use crate::models::FluxResourceKind;
        if matches!(
            FluxResourceKind::from_str(&node.kind),
            Some(FluxResourceKind::Kustomization) | Some(FluxResourceKind::HelmRelease)
        ) {
            // Skip if this is the same as the main object
            if node.kind == result.object.kind
                && node.name == result.object.name
                && node.namespace == result.object.namespace
            {
                continue;
            }

            // Add flow arrow with ASCII art
            output.push("".to_string());
            output.push("                    |".to_string());
            output.push("                    v".to_string());
            output.push("            managed by".to_string());
            output.push("".to_string());

            output.push("───────────────────────────────────────────────────".to_string());
            output.push(format!("{}:   {}", node.kind, node.name));
            output.push(format!("Namespace:       {}", node.namespace));

            if let Some(ref spec) = node.spec {
                if let Some(ref path) = spec.path {
                    output.push(format!("Path:            {}", path));
                }
            }

            if let Some(ref status) = node.status {
                if let Some(ref revision) = status.revision {
                    output.push(format!("Revision:        {}", revision));
                }
                if let Some(ref last_reconciled) = status.last_reconciled {
                    output.push(format!(
                        "Status:          Last reconciled at {}",
                        last_reconciled
                    ));
                }
                if let Some(ref message) = status.message {
                    output.push(format!("Message:         {}", message));
                }
            }
            output.push("───────────────────────────────────────────────────".to_string());
        }
    }

    // Source (GitRepository, OCIRepository, etc.)
    if let Some(ref source) = result.source {
        // Add flow arrow with ASCII art
        output.push("".to_string());
        output.push("                    |".to_string());
        output.push("                    v".to_string());
        output.push("            sourced from".to_string());
        output.push("".to_string());

        output.push("───────────────────────────────────────────────────".to_string());
        output.push(format!("{}:   {}", source.kind, source.name));
        output.push(format!("Namespace:       {}", source.namespace));

        if let Some(ref spec) = source.spec {
            if let Some(ref url) = spec.url {
                output.push(format!("URL:             {}", url));
            }
            if let Some(ref branch) = spec.branch {
                output.push(format!("Branch:          {}", branch));
            }
        }

        if let Some(ref status) = source.status {
            if let Some(ref revision) = status.revision {
                output.push(format!("Revision:        {}", revision));
            }
            if let Some(ref last_reconciled) = status.last_reconciled {
                output.push(format!(
                    "Status:          Last reconciled at {}",
                    last_reconciled
                ));
            }
            if let Some(ref message) = status.message {
                output.push(format!("Message:         {}", message));
            }
        }
        output.push("───────────────────────────────────────────────────".to_string());
    }

    output.join("\n")
}
