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
