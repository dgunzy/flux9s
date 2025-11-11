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
    ) -> (Self, mpsc::UnboundedReceiver<WatchEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            Self {
                client,
                current_namespace: namespace,
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
                Some(ns) => Api::namespaced(client.clone(), &ns),
                None => Api::all(client.clone()),
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
                    Ok(watcher::Event::Init) | Ok(watcher::Event::InitDone) => {
                        error_count = 0; // Reset error count on successful initialization
                    }
                    Err(e) => {
                        error_count += 1;
                        // Only log errors occasionally to avoid spam
                        if error_count == 1 || error_count.is_multiple_of(10) {
                            let _ = event_tx.send(WatchEvent::Error(format!(
                                "{} watcher error ({}): {}",
                                display_name, error_count, e
                            )));
                        }
                        // Stop watcher if too many consecutive errors (likely CRD removed)
                        if error_count >= MAX_CONSECUTIVE_ERRORS {
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

    /// Start watching all registered Flux resources
    ///
    /// This function watches all Flux CRD types. To add a new resource type:
    /// 1. Add the impl_watchable! macro in src/watcher/resource.rs
    /// 2. Add the watch call here
    /// 3. Add command mapping in src/tui/app.rs execute_command()
    pub fn watch_all(&mut self) -> Result<()> {
        // Source Controller resources
        self.watch::<resource::GitRepository>()?;
        self.watch::<resource::OCIRepository>()?;
        self.watch::<resource::HelmRepository>()?;
        self.watch::<resource::Bucket>()?;
        self.watch::<resource::HelmChart>()?;
        self.watch::<resource::ExternalArtifact>()?;

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

        Ok(())
    }

    /// Abort all watcher tasks
    pub fn stop(&mut self) {
        for handle in &self.handles {
            handle.abort();
        }
        self.handles.clear();
    }
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
