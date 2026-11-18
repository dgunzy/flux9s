//! Core trace implementation

use anyhow::{Context, Result};
use serde_json::Value;

use crate::kube::conditions;
use crate::kube::objects::ObjectRef;
use crate::kube::ownership::Manager;
use crate::kube::sources;
use crate::models::FluxResourceKind;
use crate::trace::models::{
    SourceRef, TraceNode, TraceOutcome, TraceResult, TraceSource, TraceSpec, TraceStatus,
    is_flux_kind,
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

/// Trace any object up its delivery pipeline, as the Flux Operator's trace
/// does (#266): its controller owners (Pod → ReplicaSet → Deployment) to
/// the object Flux labels claim, then every Flux object managing it, up to
/// the top. The source is the one pulled by the **nearest applier** — the
/// object itself when it has a source (a Kustomization applied by another
/// still shows its own `sourceRef`), else its nearest manager with one —
/// followed to the ExternalArtifact's producer and what that is built from.
/// Native objects resolve by `apiVersion`, so CRDs and cluster-scoped
/// kinds work too. Argo CD, Helm, and unmanaged chains end the trace at
/// that manager ([`TraceOutcome`]).
pub async fn trace_target(client: &kube::Client, target: &ObjectRef) -> Result<TraceResult> {
    let obj_value = fetch_value(client, target)
        .await
        .context("Failed to fetch object")?;
    let object = create_trace_node(&obj_value)?;

    let (above, outcome) =
        walk_owner_chain(client, &object, obj_value.clone(), target.namespace.clone()).await;
    let outcome = if object.is_flux() {
        TraceOutcome::Flux
    } else {
        outcome
    };

    // An unreadable source ends the search, as in the Operator's trace: a
    // manager's source isn't this applier's.
    let source = match nearest_applier_source(&(object.clone(), obj_value), &above) {
        Some((applier, source_key)) => match resolve_through(client, source_key).await {
            Ok((_, (source_node, source_value))) => {
                Some(trace_source(client, applier.key(), source_node, &source_value).await)
            }
            Err(e) => {
                tracing::warn!("Couldn't read the source of {}: {:#}", applier.key(), e);
                None
            }
        },
        None => None,
    };

    Ok(TraceResult {
        object,
        chain: above.into_iter().map(|(node, _)| node).collect(),
        source,
        outcome,
    })
}

/// The nearest applier with a source and the source it names: the object
/// itself when it has one, else the nearest Flux object above it that does
/// (the walk may top out at a ResourceSet or FluxInstance, which have none).
fn nearest_applier_source<'a>(
    object: &'a (TraceNode, Value),
    above: &'a [(TraceNode, Value)],
) -> Option<(&'a TraceNode, ResourceKey)> {
    std::iter::once(object)
        .chain(above)
        .find_map(|(node, value)| {
            let kind = FluxResourceKind::parse_optional(&node.kind).filter(|_| node.is_flux())?;
            sources::source_ref(kind, value, &node.namespace).map(|key| (node, key))
        })
}

/// A resolved source with what it came from: an ExternalArtifact's
/// producer (and the sources that combines), or a producer source's own
/// `spec.sources`.
async fn trace_source(
    client: &kube::Client,
    resolved_for: ResourceKey,
    node: TraceNode,
    value: &Value,
) -> TraceSource {
    let mut source = TraceSource {
        resolved_for,
        node,
        produced_by: None,
        built_from: Vec::new(),
    };
    let Some(kind) = FluxResourceKind::parse_optional(&source.node.kind) else {
        return source;
    };
    let mut builder = (kind, value.clone(), source.node.namespace.clone());
    if let Some(producer_key) = sources::producer_ref(kind, value, &source.node.namespace) {
        let Some((producer, producer_value)) =
            fetch_node(client, &ObjectRef::from(producer_key)).await
        else {
            return source;
        };
        let Some(producer_kind) = FluxResourceKind::parse_optional(&producer.kind) else {
            return source;
        };
        builder = (producer_kind, producer_value, producer.namespace.clone());
        source.produced_by = Some(producer);
    }
    let (kind, value, namespace) = builder;
    for key in sources::built_from(kind, &value, &namespace) {
        if let Some((upstream, _)) = fetch_node(client, &ObjectRef::from(key)).await {
            source.built_from.push(upstream);
        }
    }
    source
}

/// Fetch the source `key` names, resolving through intermediates (a
/// HelmRelease's HelmChart) to the source that holds the artifact:
/// `(intermediates, source)`, each with its JSON.
async fn resolve_through(
    client: &kube::Client,
    mut key: ResourceKey,
) -> Result<(Vec<(TraceNode, Value)>, (TraceNode, Value))> {
    let mut intermediates: Vec<(TraceNode, Value)> = Vec::new();
    loop {
        let value = fetch_value(client, &ObjectRef::from(key.clone()))
            .await
            .with_context(|| format!("Failed to fetch source {key}"))?;
        let node = create_trace_node(&value)?;
        let next = FluxResourceKind::parse_optional(&node.kind)
            .filter(|kind| sources::is_intermediate(*kind))
            .and_then(|kind| sources::source_ref(kind, &value, &node.namespace));
        let Some(next) = next.filter(|_| intermediates.len() < MAX_TRACE_DEPTH) else {
            return Ok((intermediates, (node, value)));
        };
        intermediates.push((node, value));
        key = next;
    }
}

/// An object's **own** source, through any intermediates (a HelmChart
/// between a HelmRelease and its repository): `(intermediates, source)`.
/// `None` when it pulls from no source. The graph draws these; the trace
/// resolves through them.
pub async fn own_source(
    client: &kube::Client,
    kind: &str,
    namespace: &str,
    name: &str,
) -> Result<(Vec<TraceNode>, Option<TraceNode>)> {
    let target = ObjectRef::from(ResourceKey::new(kind, namespace, name));
    let obj = fetch_value(client, &target)
        .await
        .context("Failed to fetch object")?;
    let node = create_trace_node(&obj)?;
    let Some(kind) = FluxResourceKind::parse_optional(&node.kind).filter(|_| node.is_flux()) else {
        return Ok((Vec::new(), None));
    };
    let Some(source_key) = sources::source_ref(kind, &obj, &node.namespace) else {
        return Ok((Vec::new(), None));
    };
    let (intermediates, (source, _)) = resolve_through(client, source_key).await?;
    Ok((
        intermediates.into_iter().map(|(node, _)| node).collect(),
        Some(source),
    ))
}

/// Walk up from `object` — Flux ownership labels first, then controller
/// owner references — through every manager to the top, or to another
/// manager (Argo CD, Helm) or an unreadable owner. Returns what is above
/// the object, nearest first, each with its JSON.
async fn walk_owner_chain(
    client: &kube::Client,
    object: &TraceNode,
    mut current: Value,
    mut current_ns: String,
) -> (Vec<(TraceNode, Value)>, TraceOutcome) {
    let mut chain: Vec<(TraceNode, Value)> = Vec::new();
    let seen = |chain: &[(TraceNode, Value)], node: &TraceNode| {
        node.same_object(object) || chain.iter().any(|(n, _)| n.same_object(node))
    };
    let nodes = |chain: &[(TraceNode, Value)]| -> Vec<TraceNode> {
        chain.iter().map(|(n, _)| n.clone()).collect()
    };
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
            // Flux claims the object but its manager can't be read (RBAC,
            // or deleted and the object orphaned): say so, rather than
            // walking past the claim to a misleading "not managed".
            let Some((owner_node, owner_obj)) = fetch_node(client, &owner).await else {
                let outcome = TraceOutcome::OwnerUnreadable(format!(
                    "{}/{}",
                    flux_owner.kind, flux_owner.name
                ));
                return (chain, outcome);
            };
            if seen(&chain, &owner_node) {
                break; // A label cycle: the chain is complete.
            }
            chain.push((owner_node, owner_obj.clone()));
            // Keep walking: the manager may itself be managed.
            current = owner_obj;
            current_ns = owner_ns;
            continue;
        }

        // Another definitive manager (Argo CD): the trace stops there.
        if let Some(manager) = crate::kube::ownership::detect_manager(&current)
            && !matches!(manager, Manager::Flux { .. })
        {
            return (chain, TraceOutcome::Other(manager));
        }

        // The controller owner — the same one MANAGED-BY follows.
        let Some(owner_ref) = crate::kube::ownership::controller_owner(&current) else {
            let outcome = top_of_chain_outcome(&nodes(&chain), &current);
            return (chain, outcome);
        };
        // Flux kinds (by kind *and* API group) go through the Flux fetch path
        // (version fallback); everything else by the owner's own apiVersion,
        // so cluster-scoped owners and CRDs resolve too.
        let owner = if is_flux_kind(&owner_ref.kind, Some(&owner_ref.api_version)) {
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
            return (chain, unreadable());
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
            return (chain, unreadable());
        }

        if seen(&chain, &owner_node) {
            break; // An owner reference cycle.
        }
        chain.push((owner_node, owner_obj.clone()));
        current = owner_obj;
    }
    let outcome = top_of_chain_outcome(&nodes(&chain), &current);
    (chain, outcome)
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
            kind: kind.as_str().to_string(),
            name,
            namespace: (!namespace.is_empty()).then_some(namespace),
        }),
        _ => None,
    }
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
        let ready_condition = s
            .get("conditions")
            .and_then(|c| conditions::of_type(c, conditions::READY));
        let ready = ready_condition.map(conditions::Condition::is_true);
        let message = ready_condition
            .and_then(conditions::Condition::message)
            .map(str::to_string);

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
        if let Some(source_ref) = s.get("sourceRef").and_then(|sr| sr.as_object())
            && let (Some(kind), Some(name)) = (
                source_ref.get("kind").and_then(|k| k.as_str()),
                source_ref.get("name").and_then(|n| n.as_str()),
            )
        {
            trace_spec.source_ref = Some(SourceRef {
                kind: kind.to_string(),
                name: name.to_string(),
                namespace: source_ref
                    .get("namespace")
                    .and_then(|n| n.as_str())
                    .map(|s| s.to_string()),
            });
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
        let object = create_trace_node(&labelled).unwrap();
        let (chain, outcome) =
            walk_owner_chain(&client, &object, labelled.clone(), "apps".into()).await;
        assert!(chain.is_empty());
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

    fn applier(kind: &str, namespace: &str, name: &str, spec: Value) -> (TraceNode, Value) {
        let api_version = crate::kube::api::api_resource_for_kind(kind)
            .unwrap()
            .api_version;
        let value = json!({"apiVersion": api_version, "kind": kind,
            "metadata": {"name": name, "namespace": namespace}, "spec": spec});
        (create_trace_node(&value).unwrap(), value)
    }

    /// The user-reported case (and the Flux Operator's rule): a
    /// Kustomization applied by another resolves to its **own** source,
    /// not to its parent's GitRepository.
    #[test]
    fn a_managed_kustomization_resolves_its_own_source() {
        let child = applier(
            "Kustomization",
            "team-a",
            "apps",
            json!({"sourceRef":
            {"kind": "ExternalArtifact", "name": "apps", "namespace": "flux-system"}}),
        );
        let parent = applier(
            "Kustomization",
            "flux-system",
            "flux-system",
            json!({"sourceRef": {"kind": "GitRepository", "name": "flux-system"}}),
        );
        let (resolved_for, key) =
            nearest_applier_source(&child, std::slice::from_ref(&parent)).unwrap();
        assert_eq!(resolved_for.name, "apps");
        assert_eq!(
            key,
            ResourceKey::new("ExternalArtifact", "flux-system", "apps")
        );
    }

    /// A workload has no source of its own: the nearest manager's is used,
    /// skipping managers without one (a ResourceSet).
    #[test]
    fn a_workload_resolves_its_nearest_appliers_source() {
        let deployment = (node("Deployment"), json!({"kind": "Deployment"}));
        let release = applier(
            "HelmRelease",
            "apps",
            "podinfo",
            json!({"chartRef":
            {"kind": "OCIRepository", "name": "podinfo"}}),
        );
        let rset = applier("ResourceSet", "flux-system", "apps", json!({}));
        let above = [release, rset.clone()];
        let (resolved_for, key) = nearest_applier_source(&deployment, &above).unwrap();
        assert_eq!(resolved_for.kind, "HelmRelease");
        assert_eq!(key, ResourceKey::new("OCIRepository", "apps", "podinfo"));

        let top = applier(
            "Kustomization",
            "flux-system",
            "tenants",
            json!({"sourceRef": {"kind": "GitRepository", "name": "tenants"}}),
        );
        let above = [rset.clone(), top];
        let (resolved_for, _) = nearest_applier_source(&deployment, &above).unwrap();
        assert_eq!(resolved_for.name, "tenants", "past the ResourceSet");
        assert!(nearest_applier_source(&deployment, &[rset]).is_none());
    }

    use crate::kube::fake_api::{FakeCluster, fixtures, flux};

    fn names(nodes: &[TraceNode]) -> Vec<String> {
        nodes
            .iter()
            .map(|n| format!("{}/{}/{}", n.kind, n.namespace, n.name))
            .collect()
    }

    /// The user-reported shape, through the real walk: the parent is the
    /// manager, but the source is the child's own ExternalArtifact — with
    /// its producer and what that is built from — never the parent's
    /// `fleet` repository.
    #[tokio::test]
    async fn a_managed_kustomization_traces_to_its_own_artifact_and_producer() {
        let cluster = FakeCluster::new(fixtures::external_artifact_pipeline());
        let trace = trace_object(&cluster.client(), "Kustomization", "team-a", "apps")
            .await
            .unwrap();
        assert_eq!(
            names(&trace.chain),
            ["Kustomization/flux-system/flux-system"]
        );
        assert_eq!(trace.outcome, TraceOutcome::Flux);
        let source = trace.source.unwrap();
        assert_eq!(
            source.resolved_for,
            ResourceKey::new("Kustomization", "team-a", "apps")
        );
        assert_eq!(names(&[source.node]), ["ExternalArtifact/flux-system/apps"]);
        assert_eq!(
            names(&source.produced_by.into_iter().collect::<Vec<_>>()),
            ["ArtifactGenerator/flux-system/gen"]
        );
        assert_eq!(
            names(&source.built_from),
            ["GitRepository/flux-system/repo"]
        );
        assert!(cluster.writes().is_empty(), "a trace only reads");
    }

    /// A Pod walks its controller owners to the HelmRelease Flux labels
    /// name, keeps going to the Kustomization that applied that, and takes
    /// the nearest applier's source — the release's — resolved through the
    /// generated HelmChart to the repository.
    #[tokio::test]
    async fn a_pod_traces_through_owners_and_every_manager_to_its_releases_repository() {
        let cluster = FakeCluster::new(fixtures::helm_pipeline());
        let pod = ObjectRef::native("v1", "Pod", "apps", "web-abc");
        let trace = trace_target(&cluster.client(), &pod).await.unwrap();
        assert_eq!(
            names(&trace.chain),
            [
                "ReplicaSet/apps/web-7b5",
                "Deployment/apps/web",
                "HelmRelease/apps/web",
                "Kustomization/flux-system/flux-system",
            ]
        );
        assert_eq!(trace.outcome, TraceOutcome::Flux);
        let source = trace.source.unwrap();
        assert_eq!(
            source.resolved_for,
            ResourceKey::new("HelmRelease", "apps", "web")
        );
        assert_eq!(
            names(&[source.node]),
            ["HelmRepository/flux-system/charts"],
            "resolved through the HelmChart"
        );
        assert!(source.produced_by.is_none() && source.built_from.is_empty());
    }

    /// `spec.sourceRef` is optional on an ExternalArtifact: one naming no
    /// producer is still the source, with nothing above it.
    #[tokio::test]
    async fn an_external_artifact_without_a_producer_is_still_the_source() {
        let mut objects = fixtures::external_artifact_pipeline();
        for obj in &mut objects {
            if obj["kind"] == "ExternalArtifact" {
                obj["spec"] = json!({});
            }
        }
        let cluster = FakeCluster::new(objects);
        let trace = trace_object(&cluster.client(), "Kustomization", "team-a", "apps")
            .await
            .unwrap();
        let source = trace.source.unwrap();
        assert_eq!(names(&[source.node]), ["ExternalArtifact/flux-system/apps"]);
        assert!(source.produced_by.is_none() && source.built_from.is_empty());
    }

    /// The applier's source can't be read: no source — not a manager's,
    /// which isn't this applier's — and the chain still shows.
    #[tokio::test]
    async fn an_unreadable_source_is_not_replaced_by_a_managers() {
        let objects: Vec<Value> = fixtures::external_artifact_pipeline()
            .into_iter()
            .filter(|obj| obj["kind"] != "ExternalArtifact")
            .collect();
        let cluster = FakeCluster::new(objects);
        let trace = trace_object(&cluster.client(), "Kustomization", "team-a", "apps")
            .await
            .unwrap();
        assert_eq!(
            names(&trace.chain),
            ["Kustomization/flux-system/flux-system"]
        );
        assert!(trace.source.is_none(), "{:?}", trace.source);
    }

    /// Two Kustomizations labelled as applying each other: the walk stops
    /// instead of looping, with each listed once.
    #[tokio::test]
    async fn a_label_cycle_ends_the_walk() {
        let labelled = |name: &str, by: &str| {
            flux(
                "Kustomization",
                "apps",
                name,
                json!({
                "metadata": {"labels": {"kustomize.toolkit.fluxcd.io/name": by,
                    "kustomize.toolkit.fluxcd.io/namespace": "apps"}},
                "spec": {"sourceRef": {"kind": "GitRepository", "name": "missing"}}}),
            )
        };
        let cluster = FakeCluster::new([labelled("a", "b"), labelled("b", "a")]);
        let trace = trace_object(&cluster.client(), "Kustomization", "apps", "a")
            .await
            .unwrap();
        assert_eq!(names(&trace.chain), ["Kustomization/apps/b"]);
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
