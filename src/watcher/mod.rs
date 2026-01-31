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
use kube::core::{ApiResource, DynamicObject};
use kube::runtime::watcher;
use kube::{Api, Client, ResourceExt};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Event emitted by resource watchers
#[derive(Debug, Clone)]
pub enum WatchEvent {
    /// Resource was added or updated
    Applied(String, String, String, serde_json::Value), // resource_type, namespace, name, object
    /// Resource was deleted
    Deleted(String, String, String), // resource_type, namespace, name
    /// Watch error occurred
    Error(String),
    /// Controller pod was added or updated
    PodApplied(String, serde_json::Value), // pod_name, pod_json
    /// Controller pod was deleted
    PodDeleted(String), // pod_name
    /// Flux controller deployment was added or updated (for bundle version tracking)
    DeploymentApplied(serde_json::Value), // deployment_json
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

        // Stop existing watchers
        self.stop();

        // Update namespace
        self.current_namespace = namespace;

        // Restart all watchers with new namespace
        self.watch_all()
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
            // We no longer need to manually list resources - the watcher does this automatically
            let mut w = Box::pin(watcher(api, watcher::Config::default()));
            let mut error_count = 0u32;
            const MAX_CONSECUTIVE_ERRORS: u32 = 5;

            while let Some(event) = w.next().await {
                match event {
                    // Initial apply events (for existing resources when watcher starts)
                    Ok(watcher::Event::InitApply(obj)) => {
                        error_count = 0; // Reset error count on success
                        let name = obj.name_any();
                        let ns = obj.namespace().unwrap_or_default();
                        // Send initial resources - namespace filtering happens in TUI
                        let obj_json = serde_json::to_value(&obj).unwrap_or_default();
                        let _ = event_tx.send(WatchEvent::Applied(
                            resource_type.clone(),
                            ns,
                            name,
                            obj_json,
                        ));
                    }
                    // Regular apply events (for updates and new resources)
                    Ok(watcher::Event::Apply(obj)) => {
                        error_count = 0; // Reset error count on success
                        let name = obj.name_any();
                        let ns = obj.namespace().unwrap_or_default();
                        // Send all resources - namespace filtering happens in TUI
                        let obj_json = serde_json::to_value(&obj).unwrap_or_default();
                        let _ = event_tx.send(WatchEvent::Applied(
                            resource_type.clone(),
                            ns,
                            name,
                            obj_json,
                        ));
                    }
                    // Delete events
                    Ok(watcher::Event::Delete(obj)) => {
                        error_count = 0; // Reset error count on success
                        let name = obj.name_any();
                        let ns = obj.namespace().unwrap_or_default();
                        // Send all deletions - namespace filtering happens in TUI
                        let _ = event_tx.send(WatchEvent::Deleted(resource_type.clone(), ns, name));
                    }
                    // Init and InitDone events - watcher lifecycle events, no action needed
                    Ok(watcher::Event::Init) => {
                        error_count = 0; // Reset error count on successful initialization
                        tracing::debug!("{} watcher initialized", display_name);
                    }
                    Ok(watcher::Event::InitDone) => {
                        error_count = 0; // Reset error count on successful initialization
                        tracing::debug!("{} watcher initialization complete", display_name);
                    }
                    Err(e) => {
                        // Check if this is a 404 error (CRD doesn't exist)
                        // watcher::Error can be converted to kube::Error to check the underlying error
                        let error_string = format!("{}", e);
                        let is_404 = error_string.contains("404")
                            || error_string.contains("Not Found")
                            || error_string.contains("page not found");

                        if is_404 {
                            // 404 means the CRD doesn't exist - stop immediately, don't retry
                            tracing::info!(
                                "{} CRD not found (404), stopping watcher",
                                display_name
                            );
                            let _ = event_tx.send(WatchEvent::Error(format!(
                                "{} CRD not available in cluster",
                                display_name
                            )));
                            break;
                        }

                        error_count += 1;
                        // Only log errors occasionally to avoid spam
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
                        // Stop watcher if too many consecutive errors (likely CRD removed)
                        if error_count >= MAX_CONSECUTIVE_ERRORS {
                            tracing::error!(
                                "{} watcher stopped after {} consecutive errors",
                                display_name,
                                error_count
                            );
                            let _ = event_tx.send(WatchEvent::Error(format!(
                                "{} watcher stopped after {} consecutive errors",
                                display_name, error_count
                            )));
                            break;
                        }
                        // Add small delay before retrying to avoid spam
                        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
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

            let mut w = Box::pin(watcher(api, config));
            let mut error_count = 0u32;
            const MAX_CONSECUTIVE_ERRORS: u32 = 5;

            tracing::debug!(
                "Starting Flux controller pod watcher for namespace: {}",
                namespace
            );

            while let Some(event) = w.next().await {
                match event {
                    Ok(watcher::Event::InitApply(pod)) | Ok(watcher::Event::Apply(pod)) => {
                        error_count = 0;
                        let name = pod.name_any();
                        let pod_json = serde_json::to_value(&pod).unwrap_or_default();
                        let _ = event_tx.send(WatchEvent::PodApplied(name, pod_json));
                    }
                    Ok(watcher::Event::Delete(pod)) => {
                        error_count = 0;
                        let name = pod.name_any();
                        let _ = event_tx.send(WatchEvent::PodDeleted(name));
                    }
                    Ok(watcher::Event::Init) | Ok(watcher::Event::InitDone) => {
                        error_count = 0;
                        tracing::debug!("Flux controller pod watcher initialized");
                    }
                    Err(e) => {
                        error_count += 1;
                        if error_count == 1 || error_count.is_multiple_of(10) {
                            tracing::warn!("Pod watcher error ({}): {}", error_count, e);
                        }
                        if error_count >= MAX_CONSECUTIVE_ERRORS {
                            tracing::error!(
                                "Pod watcher stopped after {} consecutive errors",
                                error_count
                            );
                            break;
                        }
                        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
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

            let mut w = Box::pin(watcher(api, config));
            let mut error_count = 0u32;
            const MAX_CONSECUTIVE_ERRORS: u32 = 5;

            tracing::debug!(
                "Starting Flux controller deployment watcher for namespace: {}",
                namespace
            );

            while let Some(event) = w.next().await {
                match event {
                    Ok(watcher::Event::InitApply(deployment))
                    | Ok(watcher::Event::Apply(deployment)) => {
                        error_count = 0;
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
                    Ok(watcher::Event::Delete(_)) => {
                        error_count = 0;
                        // We don't need to track deletion - version will just become unavailable
                    }
                    Ok(watcher::Event::Init) | Ok(watcher::Event::InitDone) => {
                        error_count = 0;
                        tracing::debug!("Flux controller deployment watcher initialized");
                    }
                    Err(e) => {
                        error_count += 1;
                        if error_count == 1 || error_count.is_multiple_of(10) {
                            tracing::warn!("Deployment watcher error ({}): {}", error_count, e);
                        }
                        if error_count >= MAX_CONSECUTIVE_ERRORS {
                            tracing::error!(
                                "Deployment watcher stopped after {} consecutive errors",
                                error_count
                            );
                            break;
                        }
                        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    }
                }
            }
        });

        self.handles.push(handle);
        Ok(())
    }

    /// Watch OCIRepository with version-agnostic support (v1 and v1beta2)
    ///
    /// This uses DynamicObject to watch OCIRepository resources regardless of their API version.
    /// It tries v1beta2 first (older version that user has), then v1 if v1beta2 doesn't exist.
    fn watch_oci_repository(&mut self) -> Result<()> {
        use crate::models::FluxResourceKind;
        let client = self.client.clone();
        let namespace = self.current_namespace.clone();
        let event_tx = self.event_tx.clone();
        let resource_type = FluxResourceKind::OCIRepository.as_str().to_string();

        let handle = tokio::spawn(async move {
            // Try v1beta2 first (since user has resources in this version), then v1
            let versions = vec!["v1beta2", "v1"];

            for version in versions {
                use crate::models::FluxResourceKind;
                let api_resource = ApiResource {
                    group: "source.toolkit.fluxcd.io".to_string(),
                    version: version.to_string(),
                    api_version: format!("source.toolkit.fluxcd.io/{}", version),
                    kind: FluxResourceKind::OCIRepository.as_str().to_string(),
                    plural: "ocirepositories".to_string(),
                };

                let api: Api<DynamicObject> = match namespace {
                    Some(ref ns) => {
                        tracing::debug!(
                            "Starting OCIRepository watcher (version {}) for namespace: {}",
                            version,
                            ns
                        );
                        Api::namespaced_with(client.clone(), ns, &api_resource)
                    }
                    None => {
                        tracing::debug!(
                            "Starting OCIRepository watcher (version {}) for all namespaces",
                            version
                        );
                        Api::all_with(client.clone(), &api_resource)
                    }
                };

                let mut w = Box::pin(watcher(api, watcher::Config::default()));
                let mut error_count = 0u32;
                const MAX_CONSECUTIVE_ERRORS: u32 = 5;
                let mut version_working = false;

                // Watch this version
                loop {
                    match w.next().await {
                        Some(Ok(watcher::Event::InitApply(obj))) => {
                            error_count = 0;
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
                        Some(Ok(watcher::Event::Apply(obj))) => {
                            error_count = 0;
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
                        Some(Ok(watcher::Event::Delete(obj))) => {
                            error_count = 0;
                            version_working = true;
                            let name = obj.name_any();
                            let ns = obj.namespace().unwrap_or_default();
                            let _ =
                                event_tx.send(WatchEvent::Deleted(resource_type.clone(), ns, name));
                        }
                        Some(Ok(watcher::Event::Init)) => {
                            error_count = 0;
                            version_working = true;
                            tracing::debug!(
                                "OCIRepository watcher (version {}) initialized",
                                version
                            );
                        }
                        Some(Ok(watcher::Event::InitDone)) => {
                            error_count = 0;
                            version_working = true;
                            tracing::debug!(
                                "OCIRepository watcher (version {}) initialization complete",
                                version
                            );
                        }
                        Some(Err(e)) => {
                            error_count += 1;
                            let error_string = format!("{}", e);
                            let is_404 = error_string.contains("404")
                                || error_string.contains("Not Found")
                                || error_string.contains("page not found");

                            if is_404 && !version_working {
                                // 404 means this version doesn't exist - try next version
                                tracing::debug!(
                                    "OCIRepository version {} not found (404), trying next version",
                                    version
                                );
                                break; // Try next version
                            }

                            if error_count >= MAX_CONSECUTIVE_ERRORS {
                                tracing::error!(
                                    "OCIRepository watcher (version {}) stopped after {} consecutive errors",
                                    version,
                                    error_count
                                );
                                let _ = event_tx.send(WatchEvent::Error(format!(
                                    "OCIRepository watcher (version {}) stopped after {} consecutive errors",
                                    version, error_count
                                )));
                                return; // Give up
                            }

                            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                        }
                        None => {
                            // Stream ended
                            tracing::debug!(
                                "OCIRepository watcher (version {}) stream ended",
                                version
                            );
                            break;
                        }
                    }
                }

                // If this version worked, use it and stop trying others
                if version_working {
                    tracing::info!("OCIRepository watcher using version {}", version);
                    return;
                }
            }

            // If no version worked, report error
            let _ = event_tx.send(WatchEvent::Error(
                "OCIRepository watcher failed: no supported version found".to_string(),
            ));
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
        self.watch::<resource::GitRepository>()?;
        // OCIRepository uses version-agnostic watch to support both v1 and v1beta2
        self.watch_oci_repository()?;
        self.watch::<resource::HelmRepository>()?;
        self.watch::<resource::Bucket>()?;
        self.watch::<resource::HelmChart>()?;
        self.watch::<resource::ExternalArtifact>()?;
        self.watch::<resource::ArtifactGenerator>()?;

        // Kustomize Controller resources
        self.watch::<resource::Kustomization>()?;

        // Helm Controller resources
        self.watch::<resource::HelmRelease>()?;

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

    /// Abort all watcher tasks
    pub fn stop(&mut self) {
        tracing::debug!("Stopping {} watchers", self.handles.len());
        for handle in &self.handles {
            handle.abort();
        }
        self.handles.clear();
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

    // Extract suspended from spec.suspend (Flux uses "suspend" not "suspended")
    // Default to false if not present (most resources are not suspended)
    let suspended = if let Some(spec) = obj.get("spec") {
        if let Some(suspend_val) = spec.get("suspend") {
            suspend_val.as_bool()
        } else {
            // If suspend field doesn't exist, default to false (not suspended)
            Some(false)
        }
    } else {
        // If spec doesn't exist, default to false
        Some(false)
    };

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
