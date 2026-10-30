//! Live regression tests against the flux9s dev kind clusters.
//!
//! Every test here is `#[ignore]`d: `just ci` never runs them (no cluster in
//! unit CI). They run:
//!   - locally:  `./scripts/dev-clusters.sh ci` then `just test-live`
//!   - in CI:    .github/workflows/live-tests.yml (weekly + manual dispatch)
//!
//! The dev-cluster script plants deterministic fixtures (the staged-rollout
//! ResourceSets, broken-path-demo Kustomization, legacy v1beta2 sources), so
//! these tests make exact assertions against a real API server — the layer
//! unit tests can't reach: discovery, watch/list/log wire formats, and the
//! operator's actual condition-message formats.
//!
//! `clippy.toml`'s `allow-panic-in-tests` only covers `#[test]` function bodies,
//! not the shared helpers below, so this test crate opts out explicitly: a
//! missing dev cluster should fail loudly with setup instructions.
#![allow(clippy::panic)]
//!
//! Discipline: only assert on state `dev-clusters.sh` creates
//! deterministically. Never assert on public-internet sources (e.g. the
//! flux2-branches RSIP) or exact timing — eventual state with a bounded poll.

use std::time::{Duration, Instant};

/// Context of the primary dev cluster (Flux 2.9.x + source-watcher).
fn simple_context() -> String {
    std::env::var("FLUX9S_LIVE_SIMPLE_CONTEXT").unwrap_or_else(|_| "kind-flux9s-simple".to_string())
}

/// Context of the legacy dev cluster (Flux 2.2.x, sources at v1beta2).
fn legacy_context() -> String {
    std::env::var("FLUX9S_LIVE_LEGACY_CONTEXT").unwrap_or_else(|_| "kind-flux9s-legacy".to_string())
}

async fn client_for(context: &str) -> kube::Client {
    flux9s::kube::create_client_for_context(context)
        .await
        .unwrap_or_else(|e| {
            panic!("context '{context}' unavailable (run ./scripts/dev-clusters.sh ci): {e}")
        })
}

/// Poll `check` every 5s until it yields a value or `timeout_secs` elapses.
/// Freshly built clusters need time for first reconciliations, so live tests
/// assert eventual state with a bounded deadline instead of exact timing.
async fn eventually<T, F, Fut>(what: &str, timeout_secs: u64, mut check: F) -> T
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<T>>,
{
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        if let Some(value) = check().await {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "timed out after {timeout_secs}s waiting for: {what}"
        );
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

/// #204: a step-based ResourceSet's graph includes its downstream inventory —
/// the staged-rollout ConfigMaps and Jobs aggregate into a resource group.
#[tokio::test]
#[ignore = "requires the kind-flux9s-simple dev cluster"]
async fn resource_set_graph_shows_inventory() {
    use flux9s::trace::{NodeType, build_resource_graph};

    let client = client_for(&simple_context()).await;

    let description = eventually("staged-rollout graph resource group", 180, || {
        let client = client.clone();
        async move {
            let graph =
                build_resource_graph(&client, "ResourceSet", "flux-resources", "staged-rollout")
                    .await
                    .ok()?;
            graph
                .nodes
                .iter()
                .find(|n| n.node_type == NodeType::ResourceGroup)
                .and_then(|n| n.description.clone())
        }
    })
    .await;

    assert!(
        description.contains("ConfigMap") && description.contains("Job"),
        "resource group should aggregate the staged-rollout inventory kinds: {description}"
    );
}

/// #191/#193 coupling: the operator's step-failure condition message must
/// keep the `step "<name>"` format the detail-view phase parser matches.
/// Catches upstream flux-operator message-format drift.
#[tokio::test]
#[ignore = "requires the kind-flux9s-simple dev cluster"]
async fn step_failure_condition_matches_parser_format() {
    let client = client_for(&simple_context()).await;

    let message = eventually("staged-rollout-broken step failure", 300, || {
        let client = client.clone();
        async move {
            let obj = flux9s::kube::fetch_resource(
                &client,
                "ResourceSet",
                "flux-resources",
                "staged-rollout-broken",
            )
            .await
            .ok()?;
            let conditions = obj.pointer("/status/conditions")?.as_array()?.clone();
            let ready = conditions
                .iter()
                .find(|c| c["type"].as_str() == Some("Ready"))?;
            if ready["status"].as_str() == Some("False") {
                ready["message"].as_str().map(String::from)
            } else {
                None
            }
        }
    })
    .await;

    assert!(
        message.contains("step \"verify\""),
        "the steps parser matches `step \"<name>\"` in failure messages; \
         the operator now says: {message}"
    );
}

/// #191: describe data for the deliberately broken Kustomization includes
/// its Warning events (the field-selector fetch path, live).
#[tokio::test]
#[ignore = "requires the kind-flux9s-simple dev cluster"]
async fn broken_kustomization_describe_includes_warning_events() {
    let client = client_for(&simple_context()).await;

    let events = eventually("broken-path-demo Warning events", 300, || {
        let client = client.clone();
        async move {
            let describe = flux9s::kube::fetch::fetch_describe_data(
                &client,
                "Kustomization",
                "flux-resources",
                "broken-path-demo",
            )
            .await
            .ok()?;
            assert!(
                describe.events_error.is_none(),
                "events lookup should not fail: {:?}",
                describe.events_error
            );
            if describe.events.iter().any(|e| e.is_warning()) {
                Some(describe.events)
            } else {
                None
            }
        }
    })
    .await;

    let warning = events.iter().find(|e| e.is_warning()).unwrap();
    assert_eq!(warning.involved_name, "broken-path-demo");
    assert!(
        !warning.message.is_empty(),
        "warning events carry the reconcile error detail"
    );
}

/// #192: the pod log API path used by the `:logs` stream returns lines for a
/// controller pod (snapshot mode — bounded, no follow).
#[tokio::test]
#[ignore = "requires the kind-flux9s-simple dev cluster"]
async fn controller_log_snapshot_returns_lines() {
    use futures::{AsyncBufReadExt, TryStreamExt};
    use k8s_openapi::api::core::v1::Pod;
    use kube::api::{ListParams, LogParams};

    let client = client_for(&simple_context()).await;
    let pods: kube::Api<Pod> = kube::Api::namespaced(client, "flux-system");

    let pod_name = eventually("a running source-controller pod", 120, || {
        let pods = pods.clone();
        async move {
            let list = pods
                .list(&ListParams::default().labels("app=source-controller"))
                .await
                .ok()?;
            list.items.first().and_then(|p| p.metadata.name.clone())
        }
    })
    .await;

    let params = LogParams {
        tail_lines: Some(20),
        ..Default::default()
    };
    let stream = pods
        .log_stream(&pod_name, &params)
        .await
        .expect("log stream should open for a controller pod");
    let mut lines = stream.lines();
    let first = lines
        .try_next()
        .await
        .expect("log stream should be readable");
    assert!(
        first.is_some_and(|line| !line.is_empty()),
        "controller pod should have log output"
    );
}

/// Version-fallback discovery against the legacy (Flux 2.2.x) cluster: an
/// OCIRepository is only served at v1beta2 there, so discovery must fall back
/// from v1. This is the failure mode that once left all five fallback kinds
/// silently unwatched — asserting it live keeps the fallback path honest.
#[tokio::test]
#[ignore = "requires the kind-flux9s-legacy dev cluster"]
async fn legacy_sources_resolve_via_version_fallback() {
    let client = client_for(&legacy_context()).await;

    let api_resource = eventually("OCIRepository v1beta2 fallback discovery", 120, || {
        let client = client.clone();
        async move {
            flux9s::kube::get_api_resource_with_fallback(
                &client,
                "OCIRepository",
                "flux-resources",
                "podinfo-oci",
            )
            .await
            .ok()
        }
    })
    .await;

    assert_eq!(
        api_resource.version, "v1beta2",
        "Flux 2.2.x serves OCIRepository at v1beta2 — discovery must fall back from v1"
    );
}

/// #194: the workload drill-down fetch returns rollout data, containers, and
/// pods for a real controller Deployment.
#[tokio::test]
#[ignore = "requires the kind-flux9s-simple dev cluster"]
async fn workload_drilldown_fetches_pods_and_containers() {
    let client = client_for(&simple_context()).await;

    let workload = eventually("source-controller workload data", 120, || {
        let client = client.clone();
        async move {
            let data = flux9s::kube::workloads::fetch_workload_data(
                &client,
                "Deployment",
                "flux-system",
                "source-controller",
            )
            .await
            .ok()?;
            // A healthy controller has a ready pod behind its selector
            if data.ready == Some(true) && !data.pods.is_empty() {
                Some(data)
            } else {
                None
            }
        }
    })
    .await;

    assert!(
        workload.summary.iter().any(|(k, _)| k == "Replicas"),
        "Deployment summary includes replica rollout"
    );
    assert!(
        workload
            .containers
            .iter()
            .any(|c| c.image.contains("source-controller")),
        "containers carry their images: {:?}",
        workload.containers
    );
    let pod = &workload.pods[0];
    assert!(pod.name.starts_with("source-controller-"));
    assert_eq!(pod.phase, "Running");
    assert!(
        workload.events_error.is_none(),
        "events lookup should succeed"
    );
}

/// #197: a part-of-labeled CRD on the cluster parses through the real
/// discovery path, and its instances are listable via the derived
/// ApiResource — the exact plumbing the dynamic watcher uses. The fixture
/// (widgets.example.com + two Widget CRs) is planted by dev-clusters.sh.
#[tokio::test]
#[ignore = "requires the kind-flux9s-simple dev cluster"]
async fn labeled_crd_discovers_and_lists_instances() {
    use flux9s::models::extra_kinds::{ExtraKind, PART_OF_LABEL, PART_OF_VALUE};
    use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
    use kube::core::DynamicObject;

    let client = client_for(&simple_context()).await;

    let crds: kube::Api<CustomResourceDefinition> = kube::Api::all(client.clone());
    let crd = eventually("the labeled widgets CRD", 60, || {
        let crds = crds.clone();
        async move { crds.get("widgets.example.com").await.ok() }
    })
    .await;

    // The fixture carries the operator's part-of label — the discovery
    // watcher's selector would match it
    assert_eq!(
        crd.metadata
            .labels
            .as_ref()
            .and_then(|l| l.get(PART_OF_LABEL))
            .map(String::as_str),
        Some(PART_OF_VALUE),
    );

    // The real discovery parse (guard rails included)
    let extra = ExtraKind::from_crd(&crd).expect("labeled namespaced CRD should register");
    assert_eq!(extra.kind, "Widget");
    assert_eq!(extra.plural, "widgets");
    assert_eq!(extra.short_names, ["wd"]);

    // Instances list through the derived ApiResource — the same construction
    // the dynamic watcher and y/d fetches use
    let api_resource = extra.api_resource();
    let widgets: kube::Api<DynamicObject> =
        kube::Api::namespaced_with(client, "flux-resources", &api_resource);
    let list = eventually("widget fixtures", 60, || {
        let widgets = widgets.clone();
        async move {
            let list = widgets.list(&Default::default()).await.ok()?;
            (list.items.len() >= 2).then_some(list)
        }
    })
    .await;

    // Readiness extraction works on the fixtures' standard conditions
    let mut ready_states = std::collections::HashMap::new();
    for item in &list.items {
        let json = serde_json::to_value(item).unwrap();
        let (_, ready, _, _) = flux9s::watcher::extract_status_fields(&json);
        ready_states.insert(item.name_any(), ready);
    }
    use kube::ResourceExt;
    assert_eq!(ready_states.get("widget-healthy"), Some(&Some(true)));
    assert_eq!(ready_states.get("widget-broken"), Some(&Some(false)));
}

/// Kind registry + `:<kind>` browser (#267) against a real API server:
/// discovery wire format, served-version handling, and the live list watch.
#[tokio::test]
#[ignore = "requires the kind-flux9s-simple dev cluster"]
async fn kind_discovery_and_live_kind_list() {
    use flux9s::kube::kind_list::{KindListRequest, watch_kind};
    use flux9s::kube::object_status::ObjectHealth;
    use flux9s::models::kinds::KindScope;

    let client = client_for(&simple_context()).await;
    let kinds = flux9s::kube::discovery::discover_kinds(&client)
        .await
        .unwrap_or_else(|e| panic!("discovery failed: {e:#}"));

    let find = |kind: &str, group: &str| {
        kinds
            .iter()
            .find(|k| k.gvk.kind == kind && k.gvk.group == group)
            .cloned()
            .unwrap_or_else(|| panic!("{kind} ({group}) not discovered"))
    };
    let deployments = find("Deployment", "apps");
    assert_eq!(deployments.gvk.api_version(), "apps/v1");
    assert_eq!(deployments.scope, KindScope::Namespaced);
    assert!(deployments.names.short_names.iter().any(|s| s == "deploy"));
    assert_eq!(find("Node", "").scope, KindScope::Cluster);
    // Flux kinds are discovered too; the registry keeps them on the Flux provider.
    find("Kustomization", "kustomize.toolkit.fluxcd.io");

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let task = tokio::spawn(watch_kind(
        client,
        KindListRequest {
            spec: deployments,
            namespace: Some("flux-system".to_string()),
        },
        tx,
    ));
    // Snapshots keep arriving as the list changes. Read them until the
    // controller is healthy: a freshly started cluster reports it
    // InProgress first, so this also proves rows update live.
    let deadline = Instant::now() + Duration::from_secs(240);
    let mut seen_states = Vec::new();
    let snapshot = loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let snapshot = match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Some(Ok(snapshot))) => snapshot,
            Ok(Some(Err(e))) => panic!("kind list failed: {e:#}"),
            Ok(None) => panic!("kind list task ended early"),
            Err(_) => panic!("source-controller never became Current; saw {seen_states:?}"),
        };
        let controller = snapshot
            .rows
            .iter()
            .find(|row| row.name == "source-controller")
            .unwrap_or_else(|| panic!("source-controller missing: {:?}", snapshot.rows));
        assert_eq!(controller.namespace, "flux-system");
        seen_states.push(controller.health);
        if controller.health == ObjectHealth::Current {
            break snapshot;
        }
    };
    task.abort();

    assert!(!snapshot.truncated());

    // The controllers are installed by the Flux Operator: MANAGED-BY reads
    // the FluxInstance (flux-operator labels, same rules as its web UI).
    let managed = snapshot
        .columns
        .iter()
        .position(|c| c.header() == "MANAGED-BY")
        .expect("MANAGED-BY column");
    let controller = snapshot
        .rows
        .iter()
        .find(|row| row.name == "source-controller")
        .expect("source-controller row");
    assert_eq!(controller.cells[managed], "Flux instance");
}

/// Both discovery paths must see the same kinds at the same versions —
/// aggregated discovery on current servers, the legacy walk on older ones.
/// Checked on both dev clusters (different Flux / CRD sets).
#[tokio::test]
#[ignore = "requires the kind-flux9s-simple and kind-flux9s-legacy dev clusters"]
async fn aggregated_and_legacy_discovery_agree() {
    for context in [simple_context(), legacy_context()] {
        let client = client_for(&context).await;
        let summarize = |kinds: Vec<flux9s::models::kinds::KindSpec>| {
            let mut out: Vec<String> = kinds
                .iter()
                .map(|k| format!("{}|{}|{:?}", k.gvk.api_version(), k.gvk.kind, k.scope))
                .collect();
            out.sort();
            out
        };
        let aggregated = summarize(
            flux9s::kube::discovery::discover_kinds(&client)
                .await
                .unwrap_or_else(|e| panic!("{context}: discovery failed: {e:#}")),
        );
        let legacy = summarize(
            flux9s::kube::discovery::discover_legacy(&client)
                .await
                .unwrap_or_else(|e| panic!("{context}: legacy discovery failed: {e:#}")),
        );
        assert!(aggregated.len() > 50, "{context}: suspiciously few kinds");
        assert_eq!(aggregated, legacy, "{context}: discovery paths disagree");
    }
}

/// Describe's Managed By (#278 review): the object renders without waiting
/// for the owner chain, and the chain (Pod → ReplicaSet → Deployment) then
/// resolves within the overall bound, reusing cached kind lookups.
#[tokio::test]
#[ignore = "requires the kind-flux9s-simple dev cluster"]
async fn describe_resolves_managed_by_separately_and_within_bound() {
    use k8s_openapi::api::core::v1::Pod;
    use kube::api::ListParams;

    let client = client_for(&simple_context()).await;
    let pods: kube::Api<Pod> = kube::Api::namespaced(client.clone(), "flux-system");
    let pod = pods
        .list(&ListParams::default().labels("app=source-controller"))
        .await
        .unwrap_or_else(|e| panic!("listing pods failed: {e:#}"))
        .items
        .into_iter()
        .next()
        .expect("a source-controller pod");
    let name = pod.metadata.name.clone().unwrap_or_default();
    let target = flux9s::kube::objects::ObjectRef::native("v1", "Pod", "flux-system", &name);

    let describe = flux9s::kube::fetch::fetch_object_describe_data(&client, &target)
        .await
        .unwrap_or_else(|e| panic!("describe failed: {e:#}"));
    assert_eq!(
        describe.managed_by, None,
        "an owned pod leaves Managed By to the background resolution"
    );

    flux9s::kube::ownership::clear_api_cache();
    for _ in 0..2 {
        let started = Instant::now();
        let managed_by = flux9s::kube::ownership::describe_manager(&client, &describe.object).await;
        let text = &managed_by.text;
        assert!(
            started.elapsed() <= flux9s::kube::ownership::MANAGER_TIMEOUT + Duration::from_secs(1),
            "took {:?}",
            started.elapsed()
        );
        assert!(text.starts_with("Flux"), "{name}: {text}");
        assert!(text.contains("via ReplicaSet/"), "{name}: {text}");
        assert!(
            managed_by.flux.is_some(),
            "{name}: the Flux manager is known, so t / g / m apply"
        );
    }
}

/// Trace follows the same manager detection as MANAGED-BY: a Flux
/// controller Deployment (labelled by the Flux Operator) traces to its
/// FluxInstance.
#[tokio::test]
#[ignore = "requires the kind-flux9s-simple dev cluster"]
async fn trace_reaches_the_flux_instance_of_a_controller_deployment() {
    let client = client_for(&simple_context()).await;
    let result =
        flux9s::trace::trace_object(&client, "Deployment", "flux-system", "source-controller")
            .await
            .unwrap_or_else(|e| panic!("trace failed: {e:#}"));
    let kinds: Vec<&str> = result.chain.iter().map(|n| n.kind.as_str()).collect();
    assert!(kinds.contains(&"FluxInstance"), "chain: {kinds:?}");
}

/// The running source-controller pod (deterministic: the FluxInstance
/// installs it).
async fn source_controller_pod(client: &kube::Client) -> String {
    use k8s_openapi::api::core::v1::Pod;
    let pods: kube::Api<Pod> = kube::Api::namespaced(client.clone(), "flux-system");
    let list = pods
        .list(&kube::api::ListParams::default().labels("app=source-controller"))
        .await
        .unwrap_or_else(|e| panic!("listing source-controller pods: {e}"));
    list.items
        .into_iter()
        .find_map(|pod| pod.metadata.name)
        .unwrap_or_else(|| panic!("no source-controller pod (run ./scripts/dev-clusters.sh ci)"))
}

/// #266: a native object traces by apiVersion up its owner chain to the
/// Flux object managing it — Pod → ReplicaSet → Deployment → FluxInstance.
#[tokio::test]
#[ignore = "requires the kind-flux9s-simple dev cluster"]
async fn trace_walks_a_pod_up_its_owner_chain_to_flux() {
    use flux9s::kube::objects::ObjectRef;
    let client = client_for(&simple_context()).await;
    let pod = source_controller_pod(&client).await;
    let result = flux9s::trace::trace_target(
        &client,
        &ObjectRef::native("v1", "Pod", "flux-system", &pod),
    )
    .await
    .unwrap_or_else(|e| panic!("trace failed: {e:#}"));
    let kinds: Vec<&str> = result.path().iter().map(|n| n.kind.as_str()).collect();
    assert_eq!(
        kinds,
        ["Pod", "ReplicaSet", "Deployment", "FluxInstance"],
        "path: {kinds:?}"
    );
    assert_eq!(result.outcome, flux9s::trace::TraceOutcome::Flux);
    assert_eq!(
        result
            .flux_manager()
            .map(|n| (n.kind.as_str(), n.name.as_str())),
        Some(("FluxInstance", "flux"))
    );
}

/// #266: cluster-scoped objects trace too (they used to be fetched as
/// namespaced), and a Helm-installed chain ends at Helm.
#[tokio::test]
#[ignore = "requires the kind-flux9s-simple dev cluster"]
async fn trace_reaches_cluster_scoped_objects_and_stops_at_helm() {
    use flux9s::kube::objects::ObjectRef;
    use flux9s::trace::TraceOutcome;
    let client = client_for(&simple_context()).await;

    let role = flux9s::trace::trace_target(
        &client,
        &ObjectRef::native(
            "rbac.authorization.k8s.io/v1",
            "ClusterRole",
            "",
            "flux-edit-flux-system",
        ),
    )
    .await
    .unwrap_or_else(|e| panic!("trace failed: {e:#}"));
    assert_eq!(role.outcome, TraceOutcome::Flux);
    assert_eq!(role.object.namespace, "", "cluster-scoped: no namespace");
    assert_eq!(
        role.flux_manager().map(|n| n.kind.as_str()),
        Some("FluxInstance")
    );

    // dev-clusters.sh installs the Flux Operator with Helm.
    let operator = flux9s::trace::trace_target(
        &client,
        &ObjectRef::native("apps/v1", "Deployment", "flux-system", "flux-operator"),
    )
    .await
    .unwrap_or_else(|e| panic!("trace failed: {e:#}"));
    assert!(
        matches!(
            &operator.outcome,
            TraceOutcome::Other(flux9s::kube::ownership::Manager::Helm { release, .. })
                if release == "flux-operator"
        ),
        "{:?}",
        operator.outcome
    );
    assert!(operator.flux_manager().is_none());
}

/// #266: `:trace` without `-n` while watching all namespaces finds the
/// object wherever it lives.
#[tokio::test]
#[ignore = "requires the kind-flux9s-simple dev cluster"]
async fn find_by_name_searches_every_namespace() {
    let client = client_for(&simple_context()).await;
    let found = flux9s::kube::objects::find_by_name(
        &client,
        "Deployment",
        Some("apps/v1"),
        "source-controller",
    )
    .await
    .unwrap_or_else(|e| panic!("search failed: {e:#}"));
    let namespaces: Vec<&str> = found.iter().map(|o| o.namespace.as_str()).collect();
    assert_eq!(namespaces, ["flux-system"]);
    // Built-in kinds by name work without discovery.
    let by_name = flux9s::kube::objects::find_by_name(&client, "Deployment", None, "flux-operator")
        .await
        .unwrap_or_else(|e| panic!("search failed: {e:#}"));
    assert_eq!(by_name.len(), 1);
    assert!(
        flux9s::kube::objects::find_by_name(&client, "Deployment", Some("apps/v1"), "no-such")
            .await
            .unwrap()
            .is_empty()
    );
}

/// #266: `g` on a pod opens its manager's graph focused on the Deployment
/// the manager applied, listed in the workload group.
#[tokio::test]
#[ignore = "requires the kind-flux9s-simple dev cluster"]
async fn graph_locates_a_pod_through_its_deployment() {
    use flux9s::kube::objects::ObjectRef;
    let client = client_for(&simple_context()).await;
    let graph = flux9s::trace::build_resource_graph(&client, "FluxInstance", "flux-system", "flux")
        .await
        .unwrap_or_else(|e| panic!("graph failed: {e:#}"));
    let pod = source_controller_pod(&client).await;
    let found = flux9s::trace::locate_in_graph(
        &client,
        &graph,
        &ObjectRef::native("v1", "Pod", "flux-system", &pod),
    )
    .await;
    assert_eq!(
        found,
        Some(flux9s::watcher::ResourceKey::new(
            "Deployment",
            "flux-system",
            "source-controller"
        ))
    );
    let index = graph.node_containing(found.as_ref().unwrap()).unwrap();
    assert_eq!(
        graph.nodes[index].node_type,
        flux9s::trace::NodeType::WorkloadGroup
    );
}

/// The ownership fixtures (`scripts/dev-manifests/simple/ownership.yaml`)
/// classify as #274 describes: a kubectl-made ConfigMap is unmanaged, one
/// naming a deleted Kustomization is orphaned, and one its Kustomization
/// doesn't list is left behind — while Flux-managed objects and Kubernetes'
/// own never show.
#[tokio::test]
#[ignore = "requires the kind-flux9s-simple dev cluster"]
async fn sweeps_find_unmanaged_and_orphaned_objects() {
    use flux9s::kube::standing::{OrphanReason, Standing};
    use flux9s::kube::sweep::{SweepPhase, SweepRequest, SweepScope, sweep};
    use flux9s::watcher::ResourceKey;

    let client = client_for(&simple_context()).await;
    let kinds = flux9s::kube::discovery::discover_kinds(&client)
        .await
        .expect("discovery");
    let request = |scope| SweepRequest {
        scope,
        namespace: Some("flux-resources".to_string()),
        config: Default::default(),
    };

    // "Not in inventory" waits out the grace period for new objects.
    let unmanaged = eventually("ownership fixtures classified", 240, || {
        let (client, kinds) = (client.clone(), kinds.clone());
        async move {
            let (progress, phase) = tokio::sync::watch::channel(SweepPhase::Discovering);
            let result = sweep(&client, request(SweepScope::NotFlux), kinds, &progress).await;
            // Every kind was listed before the owners were read.
            assert!(matches!(*phase.borrow(), SweepPhase::ReadingOwners { .. }));
            let left = result
                .rows
                .iter()
                .find(|r| r.name == "ownership-left-behind")?;
            matches!(left.standing, Standing::Orphaned(..)).then_some(result)
        }
    })
    .await;
    let standing = |name: &str| {
        unmanaged
            .rows
            .iter()
            .find(|r| r.name == name)
            .map(|r| r.standing.clone())
    };
    assert_eq!(
        standing("ownership-hand-made"),
        Some(Standing::Unmanaged {
            created_by: Some("kubectl".to_string())
        })
    );
    assert!(matches!(
        standing("ownership-owner-gone"),
        Some(Standing::Orphaned(_, OrphanReason::OwnerGone))
    ));
    assert!(matches!(
        standing("ownership-left-behind"),
        Some(Standing::Orphaned(
            _,
            OrphanReason::NotInInventory {
                prune_disabled: true
            }
        ))
    ));
    // Orphans come first, whatever their namespace or kind.
    let first_other = unmanaged
        .rows
        .iter()
        .position(|r| !matches!(r.standing, Standing::Orphaned(..)))
        .unwrap_or(unmanaged.rows.len());
    assert!(
        unmanaged.rows[first_other..]
            .iter()
            .all(|r| !matches!(r.standing, Standing::Orphaned(..))),
        "orphans sort first"
    );
    // Kubernetes' own (kube-root-ca.crt, the default ServiceAccount) and
    // Flux-managed (the HelmRelease's podinfo Deployment and Service). The
    // hand-applied `podinfo` HelmChart fixture is labelled
    // `managed-by: flux9s-dev`, so it rightly shows as another tool's.
    for (kind, hidden) in [
        ("ConfigMap", "kube-root-ca.crt"),
        ("ServiceAccount", "default"),
        ("Deployment", "podinfo"),
        ("Service", "podinfo"),
    ] {
        let found = unmanaged
            .rows
            .iter()
            .find(|r| r.kind == kind && r.name == hidden);
        assert!(
            found.is_none(),
            "{kind} {hidden} is Kubernetes' own or Flux-managed: {found:?}"
        );
    }
    assert!(
        unmanaged
            .rows
            .iter()
            .any(|r| r.kind == "HelmChart" && matches!(r.standing, Standing::Other(_))),
        "other tools' objects are in the default view"
    );
    assert!(
        unmanaged
            .rows
            .iter()
            .all(|r| !r.name.starts_with("sh.helm.release")),
        "Helm storage belongs to its release"
    );

    // What `apps` left behind: only the object it no longer lists.
    let of_apps = sweep(
        &client,
        request(SweepScope::OrphansOf(ResourceKey::new(
            "Kustomization",
            "flux-resources",
            "apps",
        ))),
        kinds.clone(),
        &tokio::sync::watch::channel(SweepPhase::Discovering).0,
    )
    .await;
    let names: Vec<_> = of_apps.rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["ownership-left-behind"]);
}

/// Flux 2.10 serves Alert / Provider at `notification.toolkit.fluxcd.io/v1`:
/// flux9s's first-choice version is served (no fallback to the deprecated
/// `v1beta3`), and the generated v1 models read the dev cluster's objects.
#[tokio::test]
#[ignore = "requires the kind-flux9s-simple dev cluster (Flux 2.10)"]
async fn notification_kinds_are_read_at_v1_on_flux_2_10() {
    use flux9s::models::FluxResourceKind;
    use flux9s::watcher::{Alert, Provider, Receiver};
    use kube::Api;

    let client = client_for(&simple_context()).await;
    for (kind, name) in [
        (FluxResourceKind::Alert, "on-error"),
        (FluxResourceKind::Provider, "slack"),
        (FluxResourceKind::Receiver, "github-push"),
    ] {
        let resource = flux9s::kube::get_api_resource_with_fallback(
            &client,
            kind.as_str(),
            "flux-resources",
            name,
        )
        .await
        .unwrap_or_else(|e| panic!("{kind:?}: {e:#}"));
        assert_eq!(resource.version, "v1", "{kind:?} should be served at v1");
    }

    let ns = "flux-resources";
    let alerts = Api::<Alert>::namespaced(client.clone(), ns)
        .list(&Default::default())
        .await
        .expect("v1 Alerts deserialize into the generated model");
    assert!(alerts.items.len() >= 2, "{} alerts", alerts.items.len());
    let providers = Api::<Provider>::namespaced(client.clone(), ns)
        .list(&Default::default())
        .await
        .expect("v1 Providers deserialize into the generated model");
    assert!(
        providers
            .items
            .iter()
            .any(|p| p.metadata.name.as_deref() == Some("slack"))
    );
    Api::<Receiver>::namespaced(client, ns)
        .list(&Default::default())
        .await
        .expect("v1 Receivers deserialize into the generated model");
}

/// A Flux 2.10 HelmRelease with an OCIRepository chart template
/// (`podinfo-oci-chart`): its workload traces through the HelmRelease to the
/// generated OCIRepository (`status.helmChart` = `OCIRepository/<ns>/<name>`),
/// the list shows the chart URL and ref, and the generated source — labelled
/// for the HelmRelease but never in its inventory — isn't called an orphan.
#[tokio::test]
#[ignore = "requires the kind-flux9s-simple dev cluster (Flux 2.10)"]
async fn oci_chart_template_traces_to_its_generated_ocirepository() {
    use flux9s::kube::objects::ObjectRef;
    use flux9s::kube::sweep::{SweepRequest, SweepScope, sweep};
    use flux9s::models::FluxResourceKind;
    use flux9s::watcher::ResourceKey;

    let client = client_for(&simple_context()).await;
    let ns = "flux-resources";
    let release = eventually("podinfo-oci-chart installed", 300, || {
        let client = client.clone();
        async move {
            let hr = flux9s::kube::fetch_resource(&client, "HelmRelease", ns, "podinfo-oci-chart")
                .await
                .ok()?;
            hr.pointer("/status/helmChart")
                .and_then(|v| v.as_str())
                .is_some_and(|c| c.starts_with("OCIRepository/"))
                .then_some(hr)
        }
    })
    .await;

    let fields = FluxResourceKind::HelmRelease.extract_fields(&release);
    assert_eq!(
        fields.get("CHART").map(String::as_str),
        Some("oci://ghcr.io/stefanprodan/charts/podinfo"),
        "a reconciled release still shows the chart URL, not the generated source"
    );
    assert_eq!(
        fields.get("VERSION").map(String::as_str),
        Some(">=6.0.0"),
        "the OCI ref stands in for the chart version"
    );

    let trace = eventually("podinfo-oci-chart deployment traced", 120, || {
        let client = client.clone();
        async move {
            flux9s::trace::trace_target(
                &client,
                &ObjectRef::native("apps/v1", "Deployment", ns, "podinfo-oci-chart"),
            )
            .await
            .ok()
        }
    })
    .await;
    assert_eq!(
        trace
            .flux_manager()
            .map(|n| (n.kind.as_str(), n.name.as_str())),
        Some(("HelmRelease", "podinfo-oci-chart"))
    );
    let source = trace.source.as_ref().expect("a source");
    assert_eq!(
        (source.kind.as_str(), source.name.as_str()),
        ("OCIRepository", "flux-resources-podinfo-oci-chart"),
        "the generated OCIRepository, from status.helmChart"
    );

    let kinds = flux9s::kube::discovery::discover_kinds(&client)
        .await
        .expect("discovery");
    let left = sweep(
        &client,
        SweepRequest {
            scope: SweepScope::OrphansOf(ResourceKey::new("HelmRelease", ns, "podinfo-oci-chart")),
            namespace: None,
            config: Default::default(),
        },
        kinds,
        &tokio::sync::watch::channel(flux9s::kube::sweep::SweepPhase::Discovering).0,
    )
    .await;
    assert!(
        left.rows.is_empty(),
        "the generated OCIRepository is not an orphan: {:?}",
        left.rows
    );
}

/// Looking a Flux object up by name in every namespace (`:trace <kind>/<name>`
/// while watching all namespaces) and namespace discovery use the version the
/// cluster serves — on Flux 2.2, OCIRepository only at `v1beta2`.
#[tokio::test]
#[ignore = "requires the kind-flux9s-legacy dev cluster"]
async fn lookups_by_name_and_namespace_discovery_fall_back_on_older_flux() {
    let client = client_for(&legacy_context()).await;
    let found = flux9s::kube::objects::find_by_name(&client, "OCIRepository", None, "podinfo-oci")
        .await
        .unwrap_or_else(|e| panic!("lookup failed: {e:#}"));
    let namespaces: Vec<&str> = found.iter().map(|o| o.namespace.as_str()).collect();
    assert_eq!(namespaces, ["flux-resources"]);

    let served = flux9s::kube::api::served_flux_api_resource(
        &client,
        flux9s::models::FluxResourceKind::OCIRepository,
    )
    .await
    .expect("OCIRepository is served");
    assert_eq!(served.version, "v1beta2");

    let discovered = flux9s::kube::discover_namespaces_with_flux_resources(&client)
        .await
        .expect("namespace discovery");
    assert!(
        discovered.iter().any(|ns| ns == "flux-resources"),
        "{discovered:?}"
    );
}

/// The kinds `ResourceWatcher::watch_all` reports on a cluster within the
/// deadline — every kind is watched as `DynamicObject`s at the first
/// version the cluster serves.
async fn watched_kinds(context: &str, expected: &[&str]) -> std::collections::BTreeSet<String> {
    use flux9s::watcher::{ResourceWatcher, WatchEvent};
    let client = client_for(context).await;
    let (mut watcher, mut events) =
        ResourceWatcher::new(client, None, "flux-system".to_string(), false);
    watcher
        .watch_all()
        .unwrap_or_else(|e| panic!("watchers start: {e:#}"));
    let mut seen = std::collections::BTreeSet::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    while !expected.iter().all(|kind| seen.contains(*kind)) {
        match tokio::time::timeout_at(deadline, events.recv()).await {
            Ok(Some(WatchEvent::Applied(key, obj))) => {
                assert_eq!(
                    obj["kind"],
                    key.resource_type.as_str(),
                    "the event carries the object"
                );
                seen.insert(key.resource_type);
            }
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => break,
        }
    }
    watcher.stop();
    seen
}

/// Flux 2.10: every kind is read at its current version.
#[tokio::test]
#[ignore = "requires the kind-flux9s-simple dev cluster (Flux 2.10)"]
async fn watch_all_reports_every_kind_on_flux_2_10() {
    let expected = [
        "GitRepository",
        "OCIRepository",
        "HelmRepository",
        "HelmChart",
        "Kustomization",
        "HelmRelease",
        "ImagePolicy",
        "Alert",
        "Provider",
        "Receiver",
    ];
    let seen = watched_kinds(&simple_context(), &expected).await;
    for kind in expected {
        assert!(seen.contains(kind), "{kind} not watched; saw {seen:?}");
    }
}

/// Flux 2.2: the promoted kinds are read at their older versions
/// (HelmRelease v2beta2, OCIRepository / HelmRepository / HelmChart v1beta2,
/// Alert / Provider v1beta3).
#[tokio::test]
#[ignore = "requires the kind-flux9s-legacy dev cluster (Flux 2.2)"]
async fn watch_all_falls_back_on_older_flux() {
    let expected = [
        "GitRepository",
        "OCIRepository",
        "HelmRepository",
        "HelmChart",
        "Kustomization",
        "HelmRelease",
        "Alert",
        "Provider",
    ];
    let seen = watched_kinds(&legacy_context(), &expected).await;
    for kind in expected {
        assert!(seen.contains(kind), "{kind} not watched; saw {seen:?}");
    }
}

/// Flux 2.10 cross-namespace artifacts: an ArtifactGenerator's graph shows
/// the ExternalArtifact it generated in another namespace, in that
/// namespace (its inventory entry names it).
#[tokio::test]
#[ignore = "requires the kind-flux9s-simple dev cluster (Flux 2.10)"]
async fn artifact_generator_graph_follows_cross_namespace_artifacts() {
    use flux9s::trace::build_resource_graph;

    let client = client_for(&simple_context()).await;
    let nodes = eventually("cross-namespace ExternalArtifact in the graph", 120, || {
        let client = client.clone();
        async move {
            let graph = build_resource_graph(
                &client,
                "ArtifactGenerator",
                "flux-resources",
                "cross-namespace",
            )
            .await
            .ok()?;
            let nodes: Vec<_> = graph
                .nodes
                .iter()
                .map(|n| (n.kind.clone(), n.namespace.clone(), n.name.clone()))
                .collect();
            nodes
                .iter()
                .any(|(kind, _, name)| kind == "ExternalArtifact" && name == "apps-shared")
                .then_some(nodes)
        }
    })
    .await;
    assert!(
        nodes.contains(&(
            "ExternalArtifact".to_string(),
            "flux-artifacts".to_string(),
            "apps-shared".to_string()
        )),
        "the artifact is in its own namespace: {nodes:?}"
    );
}

/// ArtifactGenerators suspend through `source.extensions.fluxcd.io/reconcile`
/// (Flux 2.10 `flux suspend artifact generator`): flux9s reads it and its
/// suspend / resume operations write it.
#[tokio::test]
#[ignore = "requires the kind-flux9s-simple dev cluster (Flux 2.10)"]
async fn artifact_generators_suspend_and_resume_through_their_annotation() {
    use flux9s::models::FluxResourceKind;
    use flux9s::operations::{FluxOperation, ResumeOperation, SuspendOperation};

    let client = client_for(&simple_context()).await;
    let (kind, ns, name) = ("ArtifactGenerator", "flux-resources", "cross-namespace");
    let suspended = || {
        let client = client.clone();
        async move {
            let obj = flux9s::kube::fetch::fetch_resource(&client, kind, ns, name)
                .await
                .unwrap_or_else(|e| panic!("fetch {kind}: {e:#}"));
            FluxResourceKind::ArtifactGenerator
                .extract_suspended(&obj)
                .unwrap_or(false)
        }
    };

    SuspendOperation
        .execute(&client, kind, ns, name)
        .await
        .unwrap_or_else(|e| panic!("suspend: {e:#}"));
    assert!(suspended().await, "suspended after s");
    ResumeOperation
        .execute(&client, kind, ns, name)
        .await
        .unwrap_or_else(|e| panic!("resume: {e:#}"));
    assert!(!suspended().await, "resumed after r");
}

/// Flux 2.2–2.9 serve Alert / Provider at `v1beta3` only (v1 is Flux 2.10):
/// lookups by name and by discovery both fall back to it.
#[tokio::test]
#[ignore = "requires the kind-flux9s-legacy dev cluster (Flux 2.2)"]
async fn notification_kinds_fall_back_to_v1beta3_on_older_flux() {
    use flux9s::models::FluxResourceKind;

    let client = client_for(&legacy_context()).await;
    for (kind, name) in [
        (FluxResourceKind::Alert, "on-error"),
        (FluxResourceKind::Provider, "generic"),
    ] {
        let by_name = flux9s::kube::get_api_resource_with_fallback(
            &client,
            kind.as_str(),
            "flux-resources",
            name,
        )
        .await
        .unwrap_or_else(|e| panic!("{kind}: {e:#}"));
        assert_eq!(by_name.version, "v1beta3", "{kind} by name");

        let served = flux9s::kube::api::served_flux_api_resource(&client, kind)
            .await
            .unwrap_or_else(|| panic!("{kind}: no served version"));
        assert_eq!(served.version, "v1beta3", "{kind} via discovery");

        let object =
            flux9s::kube::fetch::fetch_resource(&client, kind.as_str(), "flux-resources", name)
                .await
                .unwrap_or_else(|e| panic!("{kind}: {e:#}"));
        assert_eq!(
            object["apiVersion"],
            "notification.toolkit.fluxcd.io/v1beta3"
        );
    }
}

/// A mixed Flux list's Kubernetes kinds (#304): one combined watch lists
/// each kind in the namespace and keeps the kinds apart, in order.
#[tokio::test]
#[ignore = "requires the kind-flux9s-simple dev cluster"]
async fn mixed_watch_lists_each_kubernetes_kind() {
    use flux9s::kube::kind_list::{KindListRequest, MixedRequest, watch_kinds};

    let client = client_for(&simple_context()).await;
    let kinds = flux9s::kube::discovery::discover_kinds(&client)
        .await
        .unwrap_or_else(|e| panic!("discovery failed: {e:#}"));
    let find = |kind: &str, group: &str| {
        kinds
            .iter()
            .find(|k| k.gvk.kind == kind && k.gvk.group == group)
            .cloned()
            .unwrap_or_else(|| panic!("{kind} not discovered"))
    };
    let request = |spec| KindListRequest {
        spec,
        namespace: Some("flux-resources".to_string()),
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let task = tokio::spawn(watch_kinds(
        client,
        MixedRequest {
            kinds: vec![
                request(find("Deployment", "apps")),
                request(find("Service", "")),
            ],
        },
        tx,
    ));
    let deadline = Instant::now() + Duration::from_secs(60);
    let snapshot = loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let message = tokio::time::timeout(remaining, rx.recv())
            .await
            .expect("both kinds listed in time")
            .expect("watch still running");
        let snapshot = message.unwrap_or_else(|e| panic!("watch failed: {e:#}"));
        if snapshot.kinds.iter().all(|k| k.snapshot.is_some()) {
            break snapshot;
        }
    };
    task.abort();

    let names = |index: usize| -> Vec<String> {
        snapshot.kinds[index]
            .snapshot
            .as_ref()
            .unwrap()
            .rows
            .iter()
            .map(|r| r.name.clone())
            .collect()
    };
    assert_eq!(
        snapshot.kinds[0].spec.gvk.kind, "Deployment",
        "in the order given"
    );
    assert_eq!(snapshot.kinds[1].spec.gvk.kind, "Service");
    assert!(names(0).contains(&"podinfo".to_string()), "{:?}", names(0));
    assert!(names(1).contains(&"podinfo".to_string()), "{:?}", names(1));
    assert!(
        snapshot.kinds.iter().all(|k| k.error.is_none()),
        "no errors: {:?}",
        snapshot.kinds.iter().map(|k| &k.error).collect::<Vec<_>>()
    );
}

/// The app view's tree is read from Flux's own records: a Kustomization's
/// source and inventory (its HelmRelease expanded in turn), and a
/// HelmRelease's generated source, release manifest, and workload pods.
#[tokio::test]
#[ignore = "requires the kind-flux9s-simple dev cluster (Flux 2.10)"]
async fn app_trees_follow_inventories_sources_and_pods() {
    use flux9s::kube::app::{AppRequest, AppRole, resolve};
    use flux9s::watcher::ResourceKey;

    let client = client_for(&simple_context()).await;
    let ns = "flux-resources";
    let request = AppRequest {
        roots: vec![
            ResourceKey::new("Kustomization", ns, "apps"),
            ResourceKey::new("HelmRelease", ns, "podinfo-oci-chart"),
            ResourceKey::new("Kustomization", ns, "not-created-yet"),
        ],
    };
    let (snapshot, shape) = resolve(&client, &request).await;
    // A root that doesn't exist yet is shown, and watched to expand later.
    let missing = snapshot
        .rows
        .iter()
        .position(|r| r.object.name == "not-created-yet")
        .expect("the missing root has a row");
    assert_eq!(
        snapshot.rows[missing].status.health,
        flux9s::kube::object_status::ObjectHealth::NotFound
    );
    assert!(shape.shaped.contains_key(&missing));
    let rows: Vec<(usize, AppRole, &str, &str)> = snapshot
        .rows
        .iter()
        .map(|r| {
            (
                r.depth,
                r.role,
                r.object.kind.as_str(),
                r.object.name.as_str(),
            )
        })
        .collect();

    let roots: Vec<_> = rows.iter().filter(|r| r.1 == AppRole::App).collect();
    assert_eq!(roots.len(), 3, "{rows:?}");
    assert!(
        rows.contains(&(1, AppRole::Source, "GitRepository", "flux-system")),
        "the Kustomization's source: {rows:?}"
    );
    assert!(
        rows.iter()
            .any(|r| r.0 == 1 && r.1 == AppRole::Applied && r.2 == "HelmRelease"),
        "a HelmRelease the Kustomization applied, expanded: {rows:?}"
    );
    assert!(
        rows.contains(&(
            1,
            AppRole::Source,
            "OCIRepository",
            "flux-resources-podinfo-oci-chart"
        )),
        "the OCIRepository the chart template generated: {rows:?}"
    );
    assert!(
        rows.contains(&(1, AppRole::Applied, "Deployment", "podinfo-oci-chart")),
        "from the Helm release manifest: {rows:?}"
    );
    assert!(
        rows.iter().any(|r| r.0 == 2 && r.1 == AppRole::Pod),
        "the Deployment's pods: {rows:?}"
    );
    assert!(!shape.pod_sets.is_empty() && !shape.shaped.is_empty());
}
