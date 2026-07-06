//! Watcher module
//!
//! Provides watch functionality for Flux CRD resources.
//! Designed to be extensible - new resource types can be easily added.

mod registry;
mod resource;
mod state;

pub use registry::*;
pub use resource::*;
pub use state::*;

use anyhow::Result;
use futures::StreamExt;
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::Pod;
use kube::core::DynamicObject;
use kube::runtime::utils::Backoff;
use kube::runtime::{WatchStreamExt, watcher};
use kube::{Api, Client, ResourceExt};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::kube::api::{
    get_flux_api_resources_with_fallback, is_forbidden_error, is_version_missing_error,
};
use crate::models::FluxResourceKind;

/// Maximum interval between watch-reconnect attempts.
///
/// kube's `default_backoff()` caps at 30s, which makes the "watch degraded"
/// banner drain slowly: each watcher reconnects independently, and once pinned
/// at the cap it only retries that often, so the degraded count ticks down one
/// step per cap interval. A short 3s cap keeps the count draining quickly once
/// the API server is reachable again. The extra reconnect traffic is negligible
/// (a few dozen watchers retrying at most every 3s).
const WATCH_BACKOFF_MAX: Duration = Duration::from_secs(3);
const WATCH_BACKOFF_MIN: Duration = Duration::from_millis(800);

/// Exponential backoff for watch retries, capped at [`WATCH_BACKOFF_MAX`].
///
/// Retries forever — `next()` never returns `None` — so a watcher is never
/// permanently abandoned during a prolonged outage. `reset()` is called by
/// `StreamBackoff` on every successful watch event, returning to the minimum.
struct CappedBackoff {
    current: Duration,
}

impl CappedBackoff {
    fn new() -> Self {
        Self {
            current: WATCH_BACKOFF_MIN,
        }
    }
}

impl Iterator for CappedBackoff {
    type Item = Duration;

    fn next(&mut self) -> Option<Duration> {
        let delay = self.current;
        self.current = (self.current * 2).min(WATCH_BACKOFF_MAX);
        Some(delay)
    }
}

impl Backoff for CappedBackoff {
    fn reset(&mut self) {
        self.current = WATCH_BACKOFF_MIN;
    }
}

/// Event emitted by resource watchers
#[derive(Debug, Clone)]
pub enum WatchEvent {
    /// Resource was added or updated
    Applied(String, String, String, serde_json::Value), // resource_type, namespace, name, object
    /// Resource was deleted
    Deleted(String, String, String), // resource_type, namespace, name
    /// Watch error occurred
    Error(String),
    /// A watcher started erroring and is retrying with backoff.
    /// Drives the "watch degraded" banner in the UI.
    WatcherDegraded(String), // watcher display name
    /// A previously degraded watcher received a successful event again.
    WatcherRecovered(String), // watcher display name
    /// Controller pod was added or updated
    PodApplied(String, serde_json::Value), // pod_name, pod_json
    /// Controller pod was deleted
    PodDeleted(String), // pod_name
    /// Flux controller deployment was added or updated (for bundle version tracking)
    DeploymentApplied(serde_json::Value), // deployment_json
    /// A Kubernetes Event (core/v1) was added or updated (events feed)
    KubeEventApplied(serde_json::Value), // event_json
    /// A Kubernetes Event was deleted (TTL expiry)
    KubeEventDeleted(String), // event uid
}

/// Trait for watchable Flux resources
pub trait WatchableResource:
    kube::Resource + Clone + Send + std::fmt::Debug + serde::Serialize + 'static
where
    <Self as kube::Resource>::DynamicType: Default,
    Self: for<'de> serde::Deserialize<'de>,
{
    /// Get the API group for this resource
    fn api_group() -> &'static str;

    /// Get the API version for this resource
    fn api_version() -> &'static str;

    /// Get the plural name for this resource
    fn plural() -> &'static str;

    /// Get a display name for this resource type
    fn display_name() -> &'static str;
}

/// Manages multiple resource watchers
///
/// Watchers are namespace-aware and can be restarted when namespace changes.
/// This allows efficient watching: Api::namespaced for specific namespace,
/// Api::all for all namespaces.
pub struct ResourceWatcher {
    client: Client,
    current_namespace: Option<String>,
    controller_namespace: String,
    event_tx: mpsc::UnboundedSender<WatchEvent>,
    handles: Vec<JoinHandle<()>>,
    /// The Kubernetes Events watcher has its own lifecycle: it is started
    /// lazily (first time the events view opens) and can be stopped without
    /// tearing down the resource watchers, since Events are the churniest
    /// resource in a cluster and shouldn't be paid for while unused.
    kube_events_handle: Option<JoinHandle<()>>,
}

impl ResourceWatcher {
    /// Create a new ResourceWatcher
    ///
    /// Starts watching with the specified namespace filter.
    /// Use `set_namespace()` to change namespace (restarts watchers).
    pub fn new(
        client: Client,
        namespace: Option<String>,
        controller_namespace: String,
    ) -> (Self, mpsc::UnboundedReceiver<WatchEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            Self {
                client,
                current_namespace: namespace,
                controller_namespace,
                event_tx: tx,
                handles: Vec::new(),
                kube_events_handle: None,
            },
            rx,
        )
    }

    /// Change the namespace filter and restart all watchers
    ///
    /// This is more efficient than watching all namespaces and filtering,
    /// especially for large clusters. Watchers are restarted with the new namespace.
    pub fn set_namespace(&mut self, namespace: Option<String>) -> Result<()> {
        if self.current_namespace == namespace {
            return Ok(()); // No change needed
        }

        tracing::debug!(
            "Changing namespace filter: {:?} -> {:?}",
            self.current_namespace,
            namespace
        );

        // Stop existing watchers (remembering whether the lazily started
        // events watcher was running so it survives the namespace switch)
        let events_active = self.is_watching_kube_events();
        self.stop();

        // Update namespace
        self.current_namespace = namespace;

        // Restart all watchers with new namespace
        self.watch_all()?;
        if events_active {
            self.watch_kube_events()?;
        }
        Ok(())
    }

    /// Start watching a specific resource type
    ///
    /// Uses Api::namespaced if namespace is set, Api::all otherwise.
    /// This is more efficient than always watching all namespaces.
    ///
    /// All Flux resources are namespaced, so we require NamespaceResourceScope.
    pub fn watch<R>(&mut self) -> Result<()>
    where
        R: WatchableResource + kube::Resource<Scope = kube::core::NamespaceResourceScope>,
        R::DynamicType: Default,
    {
        let client = self.client.clone();
        let namespace = self.current_namespace.clone();
        let event_tx = self.event_tx.clone();
        let display_name = R::display_name().to_string();
        let resource_type = display_name.clone();

        let handle = tokio::spawn(async move {
            // Use namespaced API if namespace is specified (more efficient)
            // Otherwise use Api::all for watching all namespaces
            // All Flux resources are namespaced, so both work
            let api: Api<R> = match namespace {
                Some(ref ns) => {
                    tracing::debug!("Starting {} watcher for namespace: {}", display_name, ns);
                    Api::namespaced(client.clone(), ns)
                }
                None => {
                    tracing::debug!("Starting {} watcher for all namespaces", display_name);
                    Api::all(client.clone())
                }
            };

            // In kube 2.0, watcher handles initial resource loading via InitApply events
            // We no longer need to manually list resources - the watcher does this automatically.
            // CappedBackoff paces retries exponentially (resets on success), so a network blip
            // or API-server restart recovers automatically without a tight error loop.
            let mut w =
                Box::pin(watcher(api, watcher::Config::default()).backoff(CappedBackoff::new()));
            let mut error_count = 0u32;

            while let Some(event) = w.next().await {
                let ev = match event {
                    Ok(ev) => ev,
                    Err(e) => {
                        let err_str = format!("{}", e);
                        if is_version_missing_error(&err_str) {
                            // CRD not installed or version not served — stop, don't retry.
                            // Clear any degraded state: this watcher is intentionally
                            // stopping, not reconnecting.
                            if error_count > 0 {
                                let _ = event_tx
                                    .send(WatchEvent::WatcherRecovered(display_name.clone()));
                            }
                            tracing::info!(
                                "{} CRD not available in cluster, stopping watcher",
                                display_name
                            );
                            let _ = event_tx.send(WatchEvent::Error(format!(
                                "{} CRD not available in cluster",
                                display_name
                            )));
                            break;
                        }

                        if is_forbidden_error(&err_str) {
                            // RBAC denies access to this resource — retrying won't help
                            // within the session, so stop rather than flag the watch
                            // as degraded. Clear any degraded state from earlier errors.
                            if error_count > 0 {
                                let _ = event_tx
                                    .send(WatchEvent::WatcherRecovered(display_name.clone()));
                            }
                            tracing::info!(
                                "{} watch forbidden by RBAC, stopping watcher: {}",
                                display_name,
                                e
                            );
                            let _ = event_tx.send(WatchEvent::Error(format!(
                                "{} watch forbidden by RBAC: {}",
                                display_name, e
                            )));
                            break;
                        }

                        error_count += 1;
                        if error_count == 1 {
                            // First error after healthy operation: flag as degraded
                            let _ =
                                event_tx.send(WatchEvent::WatcherDegraded(display_name.clone()));
                        }
                        // Keep watching: backoff paces the retries, so transient
                        // outages (laptop sleep, VPN reconnect, API-server restart)
                        // recover automatically. Only log occasionally to avoid spam.
                        if error_count == 1 || error_count.is_multiple_of(10) {
                            tracing::warn!(
                                "{} watcher error ({}): {}",
                                display_name,
                                error_count,
                                e
                            );
                            let _ = event_tx.send(WatchEvent::Error(format!(
                                "{} watcher error ({}): {}",
                                display_name, error_count, e
                            )));
                        } else {
                            tracing::debug!(
                                "{} watcher error ({}): {}",
                                display_name,
                                error_count,
                                e
                            );
                        }
                        continue;
                    }
                };

                // Any successful event after errors means the watch recovered.
                // Init is excluded: it fires before the HTTP request, so counting it
                // would flap the degraded state on every retry cycle.
                if error_count > 0 && !matches!(ev, watcher::Event::Init) {
                    error_count = 0;
                    let _ = event_tx.send(WatchEvent::WatcherRecovered(display_name.clone()));
                }

                match ev {
                    // Apply events (initial sync and updates) — namespace filtering happens in TUI
                    watcher::Event::InitApply(obj) | watcher::Event::Apply(obj) => {
                        let name = obj.name_any();
                        let ns = obj.namespace().unwrap_or_default();
                        let obj_json = serde_json::to_value(&obj).unwrap_or_default();
                        let _ = event_tx.send(WatchEvent::Applied(
                            resource_type.clone(),
                            ns,
                            name,
                            obj_json,
                        ));
                    }
                    watcher::Event::Delete(obj) => {
                        let name = obj.name_any();
                        let ns = obj.namespace().unwrap_or_default();
                        let _ = event_tx.send(WatchEvent::Deleted(resource_type.clone(), ns, name));
                    }
                    // Watcher lifecycle events, no action needed
                    watcher::Event::Init | watcher::Event::InitDone => {
                        tracing::debug!("{} watcher init event", display_name);
                    }
                }
            }
        });

        self.handles.push(handle);
        Ok(())
    }

    /// Watch Flux controller pods for status monitoring
    pub fn watch_flux_pods(&mut self) -> Result<()> {
        let client = self.client.clone();
        let namespace = self.controller_namespace.clone();
        let event_tx = self.event_tx.clone();

        let handle = tokio::spawn(async move {
            let api: Api<Pod> = Api::namespaced(client.clone(), &namespace);
            // Watch all pods in flux-system to catch flux-operator and other controllers
            // that may use different labels
            let config = watcher::Config::default();

            let mut w = Box::pin(watcher(api, config).backoff(CappedBackoff::new()));
            let mut error_count = 0u32;

            tracing::debug!(
                "Starting Flux controller pod watcher for namespace: {}",
                namespace
            );

            const WATCHER_NAME: &str = "Controller pods";
            while let Some(event) = w.next().await {
                let ev = match event {
                    Ok(ev) => ev,
                    Err(e) => {
                        if is_forbidden_error(&format!("{}", e)) {
                            // RBAC denies access — retrying won't help, so stop rather
                            // than flag the watch as degraded. Clear earlier degraded state.
                            if error_count > 0 {
                                let _ = event_tx
                                    .send(WatchEvent::WatcherRecovered(WATCHER_NAME.to_string()));
                            }
                            tracing::info!("Pod watcher forbidden by RBAC, stopping: {}", e);
                            break;
                        }
                        error_count += 1;
                        if error_count == 1 {
                            let _ = event_tx
                                .send(WatchEvent::WatcherDegraded(WATCHER_NAME.to_string()));
                        }
                        // Keep watching: backoff paces the retries.
                        if error_count == 1 || error_count.is_multiple_of(10) {
                            tracing::warn!("Pod watcher error ({}): {}", error_count, e);
                        }
                        continue;
                    }
                };

                // Init is excluded: it fires before the HTTP request succeeds
                if error_count > 0 && !matches!(ev, watcher::Event::Init) {
                    error_count = 0;
                    let _ = event_tx.send(WatchEvent::WatcherRecovered(WATCHER_NAME.to_string()));
                }

                match ev {
                    watcher::Event::InitApply(pod) | watcher::Event::Apply(pod) => {
                        let name = pod.name_any();
                        let pod_json = serde_json::to_value(&pod).unwrap_or_default();
                        let _ = event_tx.send(WatchEvent::PodApplied(name, pod_json));
                    }
                    watcher::Event::Delete(pod) => {
                        let name = pod.name_any();
                        let _ = event_tx.send(WatchEvent::PodDeleted(name));
                    }
                    watcher::Event::Init | watcher::Event::InitDone => {
                        tracing::debug!("Flux controller pod watcher initialized");
                    }
                }
            }
        });

        self.handles.push(handle);
        Ok(())
    }

    /// Watch Flux controller deployments for bundle version tracking
    pub fn watch_flux_deployments(&mut self) -> Result<()> {
        let client = self.client.clone();
        let namespace = self.controller_namespace.clone();
        let event_tx = self.event_tx.clone();

        let handle = tokio::spawn(async move {
            let api: Api<Deployment> = Api::namespaced(client.clone(), &namespace);
            let config = watcher::Config::default();

            let mut w = Box::pin(watcher(api, config).backoff(CappedBackoff::new()));
            let mut error_count = 0u32;

            tracing::debug!(
                "Starting Flux controller deployment watcher for namespace: {}",
                namespace
            );

            const WATCHER_NAME: &str = "Controller deployments";
            while let Some(event) = w.next().await {
                let ev = match event {
                    Ok(ev) => ev,
                    Err(e) => {
                        if is_forbidden_error(&format!("{}", e)) {
                            // RBAC denies access — retrying won't help, so stop rather
                            // than flag the watch as degraded. Clear earlier degraded state.
                            if error_count > 0 {
                                let _ = event_tx
                                    .send(WatchEvent::WatcherRecovered(WATCHER_NAME.to_string()));
                            }
                            tracing::info!("Deployment watcher forbidden by RBAC, stopping: {}", e);
                            break;
                        }
                        error_count += 1;
                        if error_count == 1 {
                            let _ = event_tx
                                .send(WatchEvent::WatcherDegraded(WATCHER_NAME.to_string()));
                        }
                        // Keep watching: backoff paces the retries.
                        if error_count == 1 || error_count.is_multiple_of(10) {
                            tracing::warn!("Deployment watcher error ({}): {}", error_count, e);
                        }
                        continue;
                    }
                };

                // Init is excluded: it fires before the HTTP request succeeds
                if error_count > 0 && !matches!(ev, watcher::Event::Init) {
                    error_count = 0;
                    let _ = event_tx.send(WatchEvent::WatcherRecovered(WATCHER_NAME.to_string()));
                }

                match ev {
                    watcher::Event::InitApply(deployment) | watcher::Event::Apply(deployment) => {
                        // Only track Flux deployments (with app.kubernetes.io/part-of: flux label)
                        if let Some(labels) = &deployment.metadata.labels {
                            if labels.get("app.kubernetes.io/part-of") == Some(&"flux".to_string())
                            {
                                let deployment_json =
                                    serde_json::to_value(&deployment).unwrap_or_default();
                                let _ =
                                    event_tx.send(WatchEvent::DeploymentApplied(deployment_json));
                            }
                        }
                    }
                    watcher::Event::Delete(_) => {
                        // We don't need to track deletion - version will just become unavailable
                    }
                    watcher::Event::Init | watcher::Event::InitDone => {
                        tracing::debug!("Flux controller deployment watcher initialized");
                    }
                }
            }
        });

        self.handles.push(handle);
        Ok(())
    }

    /// Watch a resource type that may be served at different API versions across Flux releases.
    ///
    /// Uses DynamicObject to try each version in order, stopping at the first that responds.
    /// Handles all five resource types that graduated from beta to stable in Flux 2.3.0:
    ///   - OCIRepository / Bucket / HelmRepository / HelmChart  (v1beta2 → v1)
    ///   - HelmRelease                                           (v2beta2 → v2)
    fn watch_with_version_fallback(&mut self, resource_kind: FluxResourceKind) -> Result<()> {
        let api_resources = get_flux_api_resources_with_fallback(resource_kind)?;
        let client = self.client.clone();
        let namespace = self.current_namespace.clone();
        let event_tx = self.event_tx.clone();
        let display_name = resource_kind.as_str();
        let resource_type = display_name.to_string();

        let handle = tokio::spawn(async move {
            // Tracks the degraded banner across version attempts so a recovery
            // (or giving up) on a later version still clears it.
            let mut degraded_sent = false;
            for api_resource in api_resources {
                let version = api_resource.version.clone();
                let api: Api<DynamicObject> = match namespace {
                    Some(ref ns) => {
                        tracing::debug!(
                            "Starting {} watcher (version {}) for namespace: {}",
                            display_name,
                            version,
                            ns
                        );
                        Api::namespaced_with(client.clone(), ns, &api_resource)
                    }
                    None => {
                        tracing::debug!(
                            "Starting {} watcher (version {}) for all namespaces",
                            display_name,
                            version
                        );
                        Api::all_with(client.clone(), &api_resource)
                    }
                };

                let mut w = Box::pin(
                    watcher(api, watcher::Config::default()).backoff(CappedBackoff::new()),
                );
                let mut error_count = 0u32;
                let mut version_working = false;

                loop {
                    let ev = match w.next().await {
                        Some(Ok(ev)) => ev,
                        Some(Err(e)) => {
                            let err_str = format!("{}", e);
                            if is_version_missing_error(&err_str) && !version_working {
                                tracing::debug!(
                                    "{} version {} not available, trying next version",
                                    display_name,
                                    version
                                );
                                break; // Try next version
                            }

                            if is_forbidden_error(&err_str) {
                                // RBAC denies access to this resource — every version
                                // would be forbidden too, so stop rather than flag the
                                // watch as degraded. Clear any degraded state first.
                                if degraded_sent {
                                    let _ = event_tx
                                        .send(WatchEvent::WatcherRecovered(resource_type.clone()));
                                }
                                tracing::info!(
                                    "{} watch forbidden by RBAC, stopping watcher: {}",
                                    display_name,
                                    e
                                );
                                let _ = event_tx.send(WatchEvent::Error(format!(
                                    "{} watch forbidden by RBAC: {}",
                                    display_name, e
                                )));
                                return;
                            }

                            error_count += 1;
                            if !degraded_sent {
                                degraded_sent = true;
                                let _ = event_tx
                                    .send(WatchEvent::WatcherDegraded(resource_type.clone()));
                            }
                            // Keep watching: backoff paces the retries, so transient
                            // outages recover automatically instead of killing the watcher.
                            if error_count == 1 || error_count.is_multiple_of(10) {
                                tracing::warn!(
                                    "{} watcher (version {}) error ({}): {}",
                                    display_name,
                                    version,
                                    error_count,
                                    e
                                );
                                let _ = event_tx.send(WatchEvent::Error(format!(
                                    "{} watcher error ({}): {}",
                                    display_name, error_count, e
                                )));
                            } else {
                                tracing::debug!(
                                    "{} watcher (version {}) error ({}): {}",
                                    display_name,
                                    version,
                                    error_count,
                                    e
                                );
                            }
                            continue;
                        }
                        None => {
                            tracing::debug!(
                                "{} watcher (version {}) stream ended",
                                display_name,
                                version
                            );
                            break;
                        }
                    };

                    // Any successful event after errors means the watch recovered.
                    // Init is excluded: it fires before the HTTP request succeeds.
                    if !matches!(ev, watcher::Event::Init) {
                        error_count = 0;
                        if degraded_sent {
                            degraded_sent = false;
                            let _ =
                                event_tx.send(WatchEvent::WatcherRecovered(resource_type.clone()));
                        }
                    }

                    match ev {
                        watcher::Event::InitApply(obj) | watcher::Event::Apply(obj) => {
                            version_working = true;
                            let name = obj.name_any();
                            let ns = obj.namespace().unwrap_or_default();
                            let obj_json = serde_json::to_value(&obj).unwrap_or_default();
                            let _ = event_tx.send(WatchEvent::Applied(
                                resource_type.clone(),
                                ns,
                                name,
                                obj_json,
                            ));
                        }
                        watcher::Event::Delete(obj) => {
                            version_working = true;
                            let name = obj.name_any();
                            let ns = obj.namespace().unwrap_or_default();
                            let _ =
                                event_tx.send(WatchEvent::Deleted(resource_type.clone(), ns, name));
                        }
                        watcher::Event::Init => {
                            // Init fires before the HTTP request — does NOT confirm the
                            // version exists on this cluster.
                            tracing::debug!(
                                "{} watcher (version {}) starting",
                                display_name,
                                version
                            );
                        }
                        watcher::Event::InitDone => {
                            // InitDone fires after a successful initial list — confirms the version
                            // exists on this cluster (even if there are no resources yet).
                            version_working = true;
                            tracing::debug!(
                                "{} watcher (version {}) confirmed available",
                                display_name,
                                version
                            );
                        }
                    }
                }

                if version_working {
                    tracing::info!("{} watcher using version {}", display_name, version);
                    return;
                }
            }

            // Clear any degraded state before giving up: the watcher is stopping
            // for good (CRD absent), not reconnecting.
            if degraded_sent {
                let _ = event_tx.send(WatchEvent::WatcherRecovered(resource_type.clone()));
            }
            let _ = event_tx.send(WatchEvent::Error(format!(
                "{} watcher: no supported API version found on this cluster",
                display_name
            )));
        });

        self.handles.push(handle);
        Ok(())
    }

    /// Start watching all registered Flux resources
    ///
    /// This function watches all Flux CRD types. To add a new resource type:
    /// 1. Add the impl_watchable! macro in src/watcher/resource.rs
    /// 2. Add the watch call here
    /// 3. Add command mapping in src/tui/app.rs execute_command()
    pub fn watch_all(&mut self) -> Result<()> {
        tracing::debug!("Starting watchers for all Flux resources");

        // Source Controller resources
        // GitRepository has been at v1 since Flux 2.0 — stable, no fallback needed.
        self.watch::<resource::GitRepository>()?;
        // The five resources below graduated from beta to stable in Flux 2.3.0.
        // Use version-fallback watches so older clusters (Flux 2.2.x) still work.
        self.watch_with_version_fallback(FluxResourceKind::OCIRepository)?;
        self.watch_with_version_fallback(FluxResourceKind::HelmRepository)?;
        self.watch_with_version_fallback(FluxResourceKind::Bucket)?;
        self.watch_with_version_fallback(FluxResourceKind::HelmChart)?;
        self.watch::<resource::ExternalArtifact>()?;
        self.watch::<resource::ArtifactGenerator>()?;

        // Kustomize Controller resources
        // Kustomization has been at v1 since Flux 2.0 — stable.
        self.watch::<resource::Kustomization>()?;

        // Helm Controller resources
        // HelmRelease graduated from v2beta2 to v2 in Flux 2.3.0.
        self.watch_with_version_fallback(FluxResourceKind::HelmRelease)?;

        // Image Reflector Controller resources
        self.watch::<resource::ImageRepository>()?;
        self.watch::<resource::ImagePolicy>()?;

        // Image Automation Controller resources
        self.watch::<resource::ImageUpdateAutomation>()?;

        // Notification Controller resources
        self.watch::<resource::Alert>()?;
        self.watch::<resource::Provider>()?;
        self.watch::<resource::Receiver>()?;

        // Flux Operator resources
        self.watch::<resource::ResourceSet>()?;
        self.watch::<resource::ResourceSetInputProvider>()?;
        self.watch::<resource::FluxReport>()?;
        self.watch::<resource::FluxInstance>()?;

        // Flux Controller Pods (for status monitoring)
        self.watch_flux_pods()?;

        // Flux Controller Deployments (for bundle version tracking)
        self.watch_flux_deployments()?;

        tracing::debug!("All watchers started ({} total)", self.handles.len());
        Ok(())
    }

    /// Whether the Kubernetes Events watcher is currently running.
    pub fn is_watching_kube_events(&self) -> bool {
        self.kube_events_handle
            .as_ref()
            .is_some_and(|handle| !handle.is_finished())
    }

    /// Start watching Kubernetes Events (core/v1) in the current namespace
    /// scope, feeding the events view. Started lazily — the first time the
    /// events view opens — rather than in `watch_all`, because Events churn
    /// far more than any other resource. A no-op if already running.
    pub fn watch_kube_events(&mut self) -> Result<()> {
        if self.is_watching_kube_events() {
            return Ok(());
        }

        let client = self.client.clone();
        let namespace = self.current_namespace.clone();
        let event_tx = self.event_tx.clone();

        let handle = tokio::spawn(async move {
            use k8s_openapi::api::core::v1::Event as CoreEvent;

            let api: Api<CoreEvent> = match namespace {
                Some(ref ns) => Api::namespaced(client, ns),
                None => Api::all(client),
            };
            let mut w =
                Box::pin(watcher(api, watcher::Config::default()).backoff(CappedBackoff::new()));
            let mut error_count = 0u32;

            const WATCHER_NAME: &str = "Kubernetes events";
            tracing::debug!("Starting Kubernetes events watcher");

            while let Some(event) = w.next().await {
                let ev = match event {
                    Ok(ev) => ev,
                    Err(e) => {
                        if is_forbidden_error(&format!("{}", e)) {
                            // RBAC denies access — retrying won't help, so stop rather
                            // than flag the watch as degraded. Clear earlier degraded state.
                            if error_count > 0 {
                                let _ = event_tx
                                    .send(WatchEvent::WatcherRecovered(WATCHER_NAME.to_string()));
                            }
                            let _ = event_tx.send(WatchEvent::Error(
                                "Events watcher forbidden by RBAC".to_string(),
                            ));
                            tracing::info!("Events watcher forbidden by RBAC, stopping: {}", e);
                            break;
                        }
                        error_count += 1;
                        if error_count == 1 {
                            let _ = event_tx
                                .send(WatchEvent::WatcherDegraded(WATCHER_NAME.to_string()));
                        }
                        // Keep watching: backoff paces the retries.
                        if error_count == 1 || error_count.is_multiple_of(10) {
                            tracing::warn!("Events watcher error ({}): {}", error_count, e);
                        }
                        continue;
                    }
                };

                // Init is excluded: it fires before the HTTP request succeeds
                if error_count > 0 && !matches!(ev, watcher::Event::Init) {
                    error_count = 0;
                    let _ = event_tx.send(WatchEvent::WatcherRecovered(WATCHER_NAME.to_string()));
                }

                match ev {
                    watcher::Event::InitApply(kube_event) | watcher::Event::Apply(kube_event) => {
                        let event_json = serde_json::to_value(&kube_event).unwrap_or_default();
                        let _ = event_tx.send(WatchEvent::KubeEventApplied(event_json));
                    }
                    watcher::Event::Delete(kube_event) => {
                        if let Some(uid) = kube_event.metadata.uid {
                            let _ = event_tx.send(WatchEvent::KubeEventDeleted(uid));
                        }
                    }
                    watcher::Event::Init | watcher::Event::InitDone => {
                        tracing::debug!("Kubernetes events watcher initialized");
                    }
                }
            }
        });

        self.kube_events_handle = Some(handle);
        Ok(())
    }

    /// Stop the Kubernetes Events watcher without touching the resource
    /// watchers. A no-op if it isn't running.
    pub fn stop_kube_events(&mut self) {
        if let Some(handle) = self.kube_events_handle.take() {
            tracing::debug!("Stopping Kubernetes events watcher");
            handle.abort();
        }
    }

    /// Abort all watcher tasks
    pub fn stop(&mut self) {
        tracing::debug!("Stopping {} watchers", self.handles.len());
        for handle in &self.handles {
            handle.abort();
        }
        self.handles.clear();
        self.stop_kube_events();
    }
}

/// Dropping a `JoinHandle` only detaches the task, so without this the watch
/// streams would keep running against the old cluster after a context switch
/// replaces the watcher.
impl Drop for ResourceWatcher {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Extract reconciliation information from resource status
pub fn extract_reconciliation_info(
    obj: &serde_json::Value,
) -> Option<crate::watcher::state::ReconciliationEvent> {
    let status = obj.get("status")?;

    // Extract lastReconciledAt or lastReconciled timestamp
    let last_reconciled_str = status
        .get("lastReconciledAt")
        .or_else(|| status.get("lastReconciled"))
        .and_then(|v| v.as_str())?;

    let timestamp = chrono::DateTime::parse_from_rfc3339(last_reconciled_str)
        .ok()?
        .with_timezone(&chrono::Utc);

    // Extract revision
    let revision = status
        .get("lastAppliedRevision")
        .or_else(|| status.get("observedRevision"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Extract status from conditions
    let ready_condition = status
        .get("conditions")
        .and_then(|c| c.as_array())
        .and_then(|arr| {
            arr.iter().find(|c| {
                c.get("type")
                    .and_then(|t| t.as_str())
                    .map(|s| s == "Ready")
                    .unwrap_or(false)
            })
        });

    let status_str = ready_condition
        .and_then(|c| c.get("status").and_then(|s| s.as_str()))
        .map(|s| if s == "True" { "Success" } else { "Failed" })
        .unwrap_or_else(|| "Unknown");

    // Extract message
    let message = status
        .get("message")
        .and_then(|m| m.as_str())
        .or_else(|| {
            ready_condition
                .and_then(|c| c.get("message"))
                .and_then(|m| m.as_str())
        })
        .map(|s| s.to_string());

    Some(crate::watcher::state::ReconciliationEvent {
        timestamp,
        revision,
        status: status_str.to_string(),
        message,
    })
}

/// Extract common status fields from a Flux CRD object JSON
pub fn extract_status_fields(
    obj: &serde_json::Value,
) -> (Option<bool>, Option<bool>, Option<String>, Option<String>) {
    let mut ready = None;
    let mut message = None;
    let mut revision = None;

    // Suspension is resource-specific: most Flux resources use spec.suspend,
    // while Flux Operator resources use a reconcile annotation.
    let suspended = obj
        .get("kind")
        .and_then(|k| k.as_str())
        .and_then(crate::models::FluxResourceKind::parse_optional)
        .and_then(|kind| kind.extract_suspended(obj))
        .or_else(|| {
            Some(
                obj.get("spec")
                    .and_then(|spec| spec.get("suspend"))
                    .and_then(|s| s.as_bool())
                    .unwrap_or(false),
            )
        });

    // Extract ready and message from status.conditions
    if let Some(status) = obj.get("status") {
        // Look for Ready condition
        if let Some(conditions) = status.get("conditions").and_then(|c| c.as_array()) {
            for condition in conditions {
                if let Some(type_val) = condition.get("type").and_then(|t| t.as_str()) {
                    if type_val == "Ready" {
                        if let Some(status_val) = condition.get("status").and_then(|s| s.as_str()) {
                            ready = Some(status_val == "True");
                        }
                        if let Some(msg) = condition.get("message").and_then(|m| m.as_str()) {
                            message = Some(msg.to_string());
                        }
                    }
                }
            }
        }

        // Extract revision from status.observedGeneration or status.lastAppliedRevision
        if let Some(rev) = status.get("lastAppliedRevision").and_then(|r| r.as_str()) {
            revision = Some(rev.to_string());
        } else if let Some(rev) = status
            .get("lastHandledReconcileAt")
            .and_then(|r| r.as_str())
        {
            // Some resources use different fields
            revision = Some(rev.to_string());
        }
    }

    (suspended, ready, message, revision)
}

#[cfg(test)]
impl std::fmt::Debug for ResourceWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResourceWatcher")
            .field("current_namespace", &self.current_namespace)
            .field("handles", &format!("<{} handles>", self.handles.len()))
            .field("client", &"<kube::Client>")
            .field("event_tx", &"<mpsc::UnboundedSender>")
            .finish()
    }
}

#[cfg(test)]
mod backoff_tests {
    use super::*;

    #[test]
    fn test_capped_backoff_grows_then_caps() {
        let mut b = CappedBackoff::new();
        assert_eq!(b.next(), Some(WATCH_BACKOFF_MIN)); // 800ms
        assert_eq!(b.next(), Some(Duration::from_millis(1600)));
        // Next step would be 3.2s but is capped at the 3s max
        assert_eq!(b.next(), Some(WATCH_BACKOFF_MAX));
        assert_eq!(b.next(), Some(WATCH_BACKOFF_MAX));
    }

    #[test]
    fn test_capped_backoff_never_gives_up() {
        // A watcher must never be permanently abandoned: next() is always Some.
        let mut b = CappedBackoff::new();
        for _ in 0..1000 {
            assert!(b.next().is_some());
        }
    }

    #[test]
    fn test_capped_backoff_reset_returns_to_min() {
        let mut b = CappedBackoff::new();
        for _ in 0..10 {
            b.next();
        }
        b.reset();
        assert_eq!(b.next(), Some(WATCH_BACKOFF_MIN));
    }
}
