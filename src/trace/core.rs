//! Core trace implementation

use anyhow::{Context, Result};
use serde_json::Value;

use crate::kube::objects::ObjectRef;
use crate::kube::ownership::Manager;
use crate::models::FluxResourceKind;
use crate::trace::models::{
    SourceRef, TraceNode, TraceOutcome, TraceResult, TraceSpec, TraceStatus, is_flux_kind,
};
use crate::watcher::ResourceKey;

/// Owner hops a trace follows before giving up — guards against owner
/// reference cycles (Pod → ReplicaSet → Deployment is 2).
const MAX_TRACE_DEPTH: usize = 16;

/// Trace a Flux resource (or a built-in kind by name) to its Flux source.
/// See [`trace_target`] for any object.
pub async fn trace_object(
    client: &kube::Client,
    resource_type: &str,
    namespace: &str,
    name: &str,
) -> Result<TraceResult> {
    trace_target(
        client,
        &ObjectRef::from(ResourceKey::new(resource_type, namespace, name)),
    )
    .await
}

/// Trace any object up to what manages it (#266): its controller owners
/// (Pod → ReplicaSet → Deployment), the Flux object that manages the chain,
/// and that object's source. Native objects resolve by `apiVersion`, so
/// CRDs and cluster-scoped kinds work too. Argo CD, Helm, and unmanaged
/// chains end the trace at that manager ([`TraceOutcome`]).
pub async fn trace_target(client: &kube::Client, target: &ObjectRef) -> Result<TraceResult> {
    let obj_value = fetch_value(client, target)
        .await
        .context("Failed to fetch object")?;
    let current_ns = target.namespace.clone();
    let initial_node = create_trace_node(&obj_value)?;

    // A Kustomization or HelmRelease: first check whether another Flux
    // resource manages it (a Kustomization applying a Kustomization). The
    // traced object is shown as "Object", so only its managers join the
    // chain (as the Flux CLI does).
    let initial_kind =
        FluxResourceKind::parse_optional(&initial_node.kind).filter(|_| initial_node.is_flux());
    if matches!(
        initial_kind,
        Some(FluxResourceKind::Kustomization) | Some(FluxResourceKind::HelmRelease)
    ) {
        let (walked_chain, walked_source, _) =
            walk_owner_chain(client, Vec::new(), obj_value, current_ns.clone()).await?;
        if walked_source.is_some() || !walked_chain.is_empty() {
            return Ok(TraceResult {
                object: initial_node,
                chain: walked_chain,
                source: walked_source,
                outcome: TraceOutcome::Flux,
            });
        }

        // Not managed by another: resolve its own source.
        let mut chain_for_source = Vec::new();
        let source =
            resolve_source(client, &mut chain_for_source, &initial_node, &current_ns).await?;
        return Ok(TraceResult {
            object: initial_node,
            chain: chain_for_source,
            source,
            outcome: TraceOutcome::Flux,
        });
    }

    // Anything else starts the chain with itself and walks up.
    let (chain, source, outcome) =
        walk_owner_chain(client, vec![initial_node.clone()], obj_value, current_ns).await?;
    let outcome = if initial_kind.is_some() {
        TraceOutcome::Flux
    } else {
        outcome
    };
    Ok(TraceResult {
        object: initial_node,
        chain,
        source,
        outcome,
    })
}

/// Walk up from `current` — Flux ownership labels first, then controller
/// owner references — until a Kustomization / HelmRelease (whose source is
/// resolved), another manager (Argo CD, Helm), or the top of the chain.
async fn walk_owner_chain(
    client: &kube::Client,
    mut chain: Vec<TraceNode>,
    mut current: Value,
    mut current_ns: String,
) -> Result<(Vec<TraceNode>, Option<TraceNode>, TraceOutcome)> {
    for _ in 0..MAX_TRACE_DEPTH {
        // Flux labels (kustomize.toolkit.fluxcd.io/name, …) name the manager.
        if let Some(flux_owner) = find_flux_owner_from_labels(&current) {
            let owner_ns = flux_owner
                .namespace
                .as_deref()
                .unwrap_or(&current_ns)
                .to_string();
            let owner = ObjectRef::from(ResourceKey::new(
                &flux_owner.kind,
                &owner_ns,
                &flux_owner.name,
            ));
            let already_in_chain = chain.iter().any(|node| {
                node.kind == flux_owner.kind
                    && node.name == flux_owner.name
                    && node.namespace == owner_ns
            });

            if !already_in_chain {
                // Flux claims the object but its manager can't be read (RBAC,
                // or deleted and the object orphaned): say so, rather than
                // walking past the claim to a misleading "not managed".
                let Some((owner_node, owner_obj)) = fetch_node(client, &owner).await else {
                    return Ok((
                        chain,
                        None,
                        TraceOutcome::OwnerUnreadable(format!(
                            "{}/{}",
                            flux_owner.kind, flux_owner.name
                        )),
                    ));
                };
                chain.push(owner_node.clone());

                // A Kustomization or HelmRelease: resolve its source and stop.
                if matches!(
                    FluxResourceKind::parse_optional(&flux_owner.kind),
                    Some(FluxResourceKind::Kustomization) | Some(FluxResourceKind::HelmRelease)
                ) {
                    let source = resolve_source(client, &mut chain, &owner_node, &owner_ns).await?;
                    return Ok((chain, source, TraceOutcome::Flux));
                }

                // ResourceSet / FluxInstance: keep walking from it.
                current = owner_obj;
                current_ns = owner_ns;
                continue;
            }
        }

        // Another definitive manager (Argo CD): the trace stops there.
        if let Some(manager) = crate::kube::ownership::detect_manager(&current)
            && !matches!(manager, Manager::Flux { .. })
        {
            return Ok((chain, None, TraceOutcome::Other(manager)));
        }

        // The controller owner — the same one MANAGED-BY follows.
        let Some(owner_ref) = crate::kube::ownership::controller_owner(&current) else {
            let outcome = top_of_chain_outcome(&chain, &current);
            return Ok((chain, None, outcome));
        };
        // Flux kinds (by kind *and* API group) go through the Flux fetch path
        // (version fallback); everything else by the owner's own apiVersion,
        // so cluster-scoped owners and CRDs resolve too.
        let owner_is_flux = is_flux_kind(&owner_ref.kind, Some(&owner_ref.api_version));
        let owner = if owner_is_flux {
            ObjectRef::from(ResourceKey::new(
                &owner_ref.kind,
                &current_ns,
                &owner_ref.name,
            ))
        } else {
            ObjectRef::native(
                &owner_ref.api_version,
                &owner_ref.kind,
                &current_ns,
                &owner_ref.name,
            )
        };
        let unreadable =
            || TraceOutcome::OwnerUnreadable(format!("{}/{}", owner_ref.kind, owner_ref.name));
        let Some((owner_node, owner_obj)) = fetch_node(client, &owner).await else {
            return Ok((chain, None, unreadable()));
        };

        // A recreated owner with the same name isn't this owner.
        let obj_uid = owner_obj
            .pointer("/metadata/uid")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if obj_uid != owner_ref.uid {
            tracing::warn!(
                "Owner UID mismatch for {}/{}: expected {}, got {}",
                owner_ref.kind,
                owner_ref.name,
                owner_ref.uid,
                obj_uid
            );
            return Ok((chain, None, unreadable()));
        }

        if !chain.iter().any(|n| n.same_object(&owner_node)) {
            chain.push(owner_node.clone());
        }
        if owner_is_flux
            && matches!(
                FluxResourceKind::parse_optional(&owner_ref.kind),
                Some(FluxResourceKind::Kustomization) | Some(FluxResourceKind::HelmRelease)
            )
        {
            let source = resolve_source(client, &mut chain, &owner_node, &current_ns).await?;
            return Ok((chain, source, TraceOutcome::Flux));
        }
        current = owner_obj;
    }
    tracing::warn!("Trace stopped after {MAX_TRACE_DEPTH} owner hops");
    let outcome = top_of_chain_outcome(&chain, &current);
    Ok((chain, None, outcome))
}

/// How a chain that ran out of owners ended: Flux when a Flux object is in
/// it (a ResourceSet or FluxInstance nothing manages), else whatever the
/// top object's own metadata claims (Helm, `managed-by`), else unmanaged.
fn top_of_chain_outcome(chain: &[TraceNode], top: &Value) -> TraceOutcome {
    if chain.iter().any(TraceNode::is_flux) {
        return TraceOutcome::Flux;
    }
    match crate::kube::ownership::ownership(top) {
        // A Flux claim the walk didn't reach: the manager couldn't be read.
        crate::kube::ownership::Ownership::Managed(Manager::Flux { kind, name, .. }) => {
            TraceOutcome::OwnerUnreadable(format!("{kind}/{name}"))
        }
        crate::kube::ownership::Ownership::Managed(manager) => TraceOutcome::Other(manager),
        _ => TraceOutcome::Unmanaged,
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
    let ks_value = fetch_object(client, &node.kind, &node.namespace, &node.name).await?;

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
            let ea_value = fetch_object(client, source_kind, source_ns, source_name).await?;
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
/// 1. HelmRelease.status.helmChart -> HelmChart -> source, or (Flux 2.10+)
///    the generated OCIRepository
/// 2. HelmRelease.spec.chartRef -> OCIRepository/ExternalArtifact
/// 3. HelmRelease.spec.chart.spec.sourceRef -> GitRepository/HelmRepository
async fn resolve_helmrelease_source(
    client: &kube::Client,
    chain: &mut Vec<TraceNode>,
    node: &TraceNode,
    default_ns: &str,
) -> Result<Option<TraceNode>> {
    // Fetch the HelmRelease object
    let hr_value = fetch_object(client, &node.kind, &node.namespace, &node.name).await?;

    // First, check the source object the controller generated, named in status.
    // Since Flux 2.10 a `chart` template can generate an OCIRepository instead
    // of a HelmChart, and status then names its kind.
    if let Some(helm_chart_ref) = hr_value
        .get("status")
        .and_then(|s| s.get("helmChart"))
        .and_then(|hc| hc.as_str())
    {
        let (generated_kind, chart_ns, chart_name) =
            crate::kube::standing::generated_chart_source(helm_chart_ref, default_ns);
        match FluxResourceKind::parse_optional(&generated_kind) {
            Some(kind @ (FluxResourceKind::HelmChart | FluxResourceKind::OCIRepository)) => {
                if let Some(generated) =
                    fetch_and_trace_flux_resource(client, kind.as_str(), &chart_ns, &chart_name)
                        .await?
                {
                    if kind == FluxResourceKind::OCIRepository {
                        return Ok(Some(generated)); // the source itself
                    }
                    // A HelmChart sits between the release and its source.
                    chain.push(generated.clone());
                    return resolve_helmchart_source(client, &generated, &chart_ns).await;
                }
            }
            _ => tracing::warn!("Unsupported status.helmChart kind: {}", generated_kind),
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

        match FluxResourceKind::parse_optional(chart_ref_kind) {
            Some(FluxResourceKind::HelmChart) => {
                // If chartRef points to HelmChart, add it to chain and resolve its source
                if let Some(chart_node) = fetch_and_trace_flux_resource(
                    client,
                    chart_ref_kind,
                    chart_ref_ns,
                    chart_ref_name,
                )
                .await?
                {
                    // Add HelmChart to chain
                    chain.push(chart_node.clone());
                    return resolve_helmchart_source(client, &chart_node, chart_ref_ns).await;
                }
            }
            Some(FluxResourceKind::OCIRepository) => {
                // Direct source reference
                return fetch_and_trace_flux_resource(
                    client,
                    chart_ref_kind,
                    chart_ref_ns,
                    chart_ref_name,
                )
                .await;
            }
            Some(FluxResourceKind::ExternalArtifact) => {
                // Validate ExternalArtifact has sourceRef
                let ea_value =
                    fetch_object(client, chart_ref_kind, chart_ref_ns, chart_ref_name).await?;
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
    let chart_value = fetch_object(
        client,
        &chart_node.kind,
        &chart_node.namespace,
        &chart_node.name,
    )
    .await?;

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

/// The Flux object an object's labels name as its manager — the same
/// detection as the kind lists' MANAGED-BY column ([`detect_manager`]), so
/// trace and the lists agree (Kustomization, HelmRelease, ResourceSet, and
/// FluxInstance).
///
/// [`detect_manager`]: crate::kube::ownership::detect_manager
fn find_flux_owner_from_labels(obj: &Value) -> Option<SourceRef> {
    match crate::kube::ownership::detect_manager(obj)? {
        crate::kube::ownership::Manager::Flux {
            kind,
            namespace,
            name,
        } => Some(SourceRef {
            kind,
            name,
            namespace: (!namespace.is_empty()).then_some(namespace),
        }),
        _ => None,
    }
}

/// Fetch and trace a Flux resource
async fn fetch_and_trace_flux_resource(
    client: &kube::Client,
    kind: &str,
    namespace: &str,
    name: &str,
) -> Result<Option<TraceNode>> {
    let target = ObjectRef::from(ResourceKey::new(kind, namespace, name));
    Ok(fetch_node(client, &target).await.map(|(node, _)| node))
}

/// Fetch an object and its trace node; `None` (logged) when it can't be read.
async fn fetch_node(client: &kube::Client, target: &ObjectRef) -> Option<(TraceNode, Value)> {
    let obj = match fetch_value(client, target).await {
        Ok(obj) => obj,
        Err(e) => {
            tracing::warn!("Failed to fetch {}: {:#}", target, e);
            return None;
        }
    };
    match create_trace_node(&obj) {
        Ok(node) => Some((node, obj)),
        Err(e) => {
            tracing::warn!("Unreadable {}: {:#}", target, e);
            None
        }
    }
}

/// Fetch any object as JSON: Flux kinds by name (with version fallback),
/// native objects by `apiVersion` (namespaced or cluster-scoped).
async fn fetch_value(client: &kube::Client, target: &ObjectRef) -> Result<Value> {
    let obj = crate::kube::objects::object_api(client, target)
        .await?
        .get(&target.name)
        .await
        .with_context(|| format!("Failed to fetch {target}"))?;
    serde_json::to_value(&obj).context("Failed to serialize object to JSON")
}

/// Fetch a Flux resource (or built-in kind) by name.
async fn fetch_object(
    client: &kube::Client,
    kind: &str,
    namespace: &str,
    name: &str,
) -> Result<Value> {
    fetch_value(
        client,
        &ObjectRef::from(ResourceKey::new(kind, namespace, name)),
    )
    .await
}

/// Create a trace node from a JSON object. Its namespace is the object's own
/// — none for cluster-scoped objects, even when reached from a namespaced
/// dependent.
fn create_trace_node(obj: &Value) -> Result<TraceNode> {
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
        .unwrap_or_default()
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
        api_version: obj
            .get("apiVersion")
            .and_then(Value::as_str)
            .map(str::to_string),
        name,
        namespace: ns,
        status,
        spec,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn labelled(labels: Value) -> Value {
        json!({"metadata": {"name": "x", "namespace": "apps", "labels": labels}})
    }

    fn node(kind: &str) -> TraceNode {
        TraceNode {
            kind: kind.into(),
            api_version: None,
            name: "x".into(),
            namespace: "apps".into(),
            status: None,
            spec: None,
        }
    }

    #[test]
    fn nodes_keep_their_own_namespace_and_api_version() {
        // A cluster-scoped owner reached from a namespaced pod has no namespace.
        let role = json!({"apiVersion": "rbac.authorization.k8s.io/v1", "kind": "ClusterRole",
            "metadata": {"name": "view"}});
        let node = create_trace_node(&role).unwrap();
        assert_eq!(node.namespace, "");
        assert_eq!(
            node.api_version.as_deref(),
            Some("rbac.authorization.k8s.io/v1")
        );
        assert_eq!(node.key().namespace, "", "so the graph lookup matches it");

        // An owner from another API group with a Flux kind's name isn't Flux.
        let bucket = json!({"apiVersion": "s3.aws.upbound.io/v1beta1", "kind": "Bucket",
            "metadata": {"name": "data"}});
        let bucket = create_trace_node(&bucket).unwrap();
        assert!(!bucket.is_flux());
        assert_eq!(
            top_of_chain_outcome(&[bucket], &json!({"metadata": {"name": "data"}})),
            TraceOutcome::Unmanaged
        );
    }

    /// A client whose API server refuses connections: every fetch fails fast.
    async fn unreachable_client() -> kube::Client {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let config = kube::Config::new(format!("http://{addr}").parse().unwrap());
        kube::Client::try_from(config).unwrap()
    }

    /// An object Flux labels claim, whose Kustomization can't be read (RBAC,
    /// or deleted and the object orphaned), ends at that Kustomization —
    /// not at a contradictory "not managed by Flux".
    #[tokio::test]
    async fn an_unreadable_flux_manager_ends_the_trace_as_unreadable() {
        let client = unreachable_client().await;
        let labelled = json!({"apiVersion": "v1", "kind": "ConfigMap", "metadata": {
            "name": "settings", "namespace": "apps", "labels": {
                "kustomize.toolkit.fluxcd.io/name": "apps",
                "kustomize.toolkit.fluxcd.io/namespace": "flux-system"}}});
        let (chain, source, outcome) =
            walk_owner_chain(&client, Vec::new(), labelled.clone(), "apps".into())
                .await
                .unwrap();
        assert!(chain.is_empty() && source.is_none());
        assert_eq!(
            outcome,
            TraceOutcome::OwnerUnreadable("Kustomization/apps".into())
        );
        // The same claim reaching the top of a chain reads the same way.
        assert_eq!(
            top_of_chain_outcome(&[], &labelled),
            TraceOutcome::OwnerUnreadable("Kustomization/apps".into())
        );
    }

    #[test]
    fn a_chain_without_flux_ends_at_what_its_top_claims() {
        let helm = json!({"metadata": {"name": "web", "annotations": {
            "meta.helm.sh/release-name": "web", "meta.helm.sh/release-namespace": "apps"}}});
        assert_eq!(
            top_of_chain_outcome(&[node("Deployment")], &helm),
            TraceOutcome::Other(Manager::Helm {
                namespace: "apps".into(),
                release: "web".into(),
            })
        );
        let bare = json!({"metadata": {"name": "web"}});
        assert_eq!(
            top_of_chain_outcome(&[node("Deployment")], &bare),
            TraceOutcome::Unmanaged
        );
        // A ResourceSet nothing manages is still Flux.
        assert_eq!(
            top_of_chain_outcome(&[node("ConfigMap"), node("ResourceSet")], &bare),
            TraceOutcome::Flux
        );
    }

    #[test]
    fn flux_owner_detection_matches_the_managed_by_column() {
        let ks = find_flux_owner_from_labels(&labelled(json!({
            "kustomize.toolkit.fluxcd.io/name": "apps",
            "kustomize.toolkit.fluxcd.io/namespace": "flux-system"
        })))
        .unwrap();
        assert_eq!(
            (ks.kind.as_str(), ks.name.as_str()),
            ("Kustomization", "apps")
        );
        assert_eq!(ks.namespace.as_deref(), Some("flux-system"));

        // Previously missed by trace, already shown by MANAGED-BY.
        let rset = find_flux_owner_from_labels(&labelled(json!({
            "resourceset.fluxcd.controlplane.io/name": "tenants",
            "resourceset.fluxcd.controlplane.io/namespace": "flux-system"
        })))
        .unwrap();
        assert_eq!(rset.kind, FluxResourceKind::ResourceSet.as_str());
        let instance = find_flux_owner_from_labels(&labelled(json!({
            "app.kubernetes.io/managed-by": "flux-operator",
            "fluxcd.controlplane.io/name": "flux",
            "fluxcd.controlplane.io/namespace": "flux-system"
        })))
        .unwrap();
        assert_eq!(instance.kind, FluxResourceKind::FluxInstance.as_str());

        // A label without a namespace falls back to the object's namespace.
        let hr = find_flux_owner_from_labels(&labelled(json!({
            "helm.toolkit.fluxcd.io/name": "podinfo"
        })))
        .unwrap();
        assert_eq!(hr.namespace, None);

        // Non-Flux managers aren't Flux owners.
        assert!(
            find_flux_owner_from_labels(&labelled(json!({
                "argocd.argoproj.io/instance": "guestbook"
            })))
            .is_none()
        );
    }
}
