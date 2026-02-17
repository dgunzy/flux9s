//! Headless cluster session for library consumers
//!
//! `ClusterSession` wraps a Kubernetes client, resource watcher, and resource state
//! into a single abstraction that can be used without any TUI dependencies.
//! This is the primary entry point for using flux9s as a library.

use anyhow::{Context, Result};
use std::path::Path;
use tokio::sync::mpsc;

use crate::config::schema::Config;
use crate::constants::MAX_RECONCILIATION_HISTORY;
use crate::models::FluxResourceKind;
use crate::watcher::{
    ResourceInfo, ResourceState, ResourceWatcher, WatchEvent, extract_annotations, extract_labels,
    extract_reconciliation_info, extract_status_fields, resource_key,
};

/// A headless session connected to a single Kubernetes cluster.
///
/// Manages a Kubernetes client, resource watcher, and resource state
/// without any TUI dependencies. Use this when embedding flux9s as a library.
///
/// # Example
///
/// ```rust,no_run
/// use flux9s::services::ClusterSession;
/// use flux9s::config::schema::Config;
///
/// # async fn example() -> anyhow::Result<()> {
/// let config = Config::default();
/// let mut session = ClusterSession::connect_default(&config).await?;
///
/// // Process events and build up state
/// session.drain_events();
///
/// // Read current state
/// for resource in session.snapshot() {
///     println!("{}: {} ({})", resource.resource_type, resource.name, resource.namespace);
/// }
/// # Ok(())
/// # }
/// ```
pub struct ClusterSession {
    client: kube::Client,
    context: String,
    namespace: Option<String>,
    controller_namespace: String,
    state: ResourceState,
    watcher: ResourceWatcher,
    event_rx: mpsc::UnboundedReceiver<WatchEvent>,
}

impl ClusterSession {
    /// Connect to a cluster using a specific context name.
    pub async fn connect(
        context: &str,
        namespace: Option<String>,
        controller_namespace: &str,
    ) -> Result<Self> {
        let client = crate::kube::create_client_for_context(context)
            .await
            .with_context(|| format!("Failed to connect to context '{}'", context))?;

        let state = ResourceState::new();
        let (mut watcher, event_rx) = ResourceWatcher::new(
            client.clone(),
            namespace.clone(),
            controller_namespace.to_string(),
        );

        watcher
            .watch_all()
            .context("Failed to start resource watchers")?;

        Ok(Self {
            client,
            context: context.to_string(),
            namespace,
            controller_namespace: controller_namespace.to_string(),
            state,
            watcher,
            event_rx,
        })
    }

    /// Connect using the default kubeconfig and configuration.
    pub async fn connect_default(config: &Config) -> Result<Self> {
        let client = crate::kube::create_client()
            .await
            .context("Failed to create Kubernetes client")?;

        let context = crate::kube::get_context()
            .await
            .context("Failed to get current context")?;

        let namespace = if config.default_namespace.is_empty()
            || config.default_namespace == "all"
            || config.default_namespace == "-A"
        {
            crate::kube::get_default_namespace().await
        } else {
            Some(config.default_namespace.clone())
        };

        let state = ResourceState::new();
        let (mut watcher, event_rx) = ResourceWatcher::new(
            client.clone(),
            namespace.clone(),
            config.default_controller_namespace.clone(),
        );

        watcher
            .watch_all()
            .context("Failed to start resource watchers")?;

        Ok(Self {
            client,
            context,
            namespace,
            controller_namespace: config.default_controller_namespace.clone(),
            state,
            watcher,
            event_rx,
        })
    }

    /// Connect using a specific kubeconfig file path.
    pub async fn connect_from_kubeconfig(path: &Path, config: &Config) -> Result<Self> {
        let client = crate::kube::create_client_from_kubeconfig_path(path)
            .await
            .with_context(|| {
                format!(
                    "Failed to create client from kubeconfig: {}",
                    path.display()
                )
            })?;

        let context = crate::kube::get_context_from_kubeconfig_path(path)?;

        let namespace = if config.default_namespace.is_empty()
            || config.default_namespace == "all"
            || config.default_namespace == "-A"
        {
            crate::kube::get_default_namespace().await
        } else {
            Some(config.default_namespace.clone())
        };

        let state = ResourceState::new();
        let (mut watcher, event_rx) = ResourceWatcher::new(
            client.clone(),
            namespace.clone(),
            config.default_controller_namespace.clone(),
        );

        watcher
            .watch_all()
            .context("Failed to start resource watchers")?;

        Ok(Self {
            client,
            context,
            namespace,
            controller_namespace: config.default_controller_namespace.clone(),
            state,
            watcher,
            event_rx,
        })
    }

    /// Returns a reference to the underlying Kubernetes client.
    pub fn client(&self) -> &kube::Client {
        &self.client
    }

    /// Returns the current context name.
    pub fn context(&self) -> &str {
        &self.context
    }

    /// Returns the current namespace filter, if any.
    pub fn namespace(&self) -> Option<&str> {
        self.namespace.as_deref()
    }

    /// Returns a reference to the resource state.
    pub fn state(&self) -> &ResourceState {
        &self.state
    }

    /// Returns a snapshot of all currently known resources.
    pub fn snapshot(&self) -> Vec<ResourceInfo> {
        self.state.all()
    }

    /// Wait for and return the next watch event.
    ///
    /// Returns `None` if the watcher channel is closed.
    pub async fn recv_event(&mut self) -> Option<WatchEvent> {
        self.event_rx.recv().await
    }

    /// Try to receive a watch event without blocking.
    ///
    /// Returns `None` if no event is available.
    pub fn try_recv_event(&mut self) -> Option<WatchEvent> {
        self.event_rx.try_recv().ok()
    }

    /// Apply a watch event to the internal state.
    ///
    /// This processes `Applied` and `Deleted` events, updating the resource
    /// state accordingly. Pod and Deployment events are ignored (they are
    /// TUI-specific for controller status display).
    pub fn apply_event(&self, event: WatchEvent) {
        match event {
            WatchEvent::Applied(resource_type, ns, name, obj_json) => {
                let key = resource_key(&ns, &name, &resource_type);

                let reconciliation_event = extract_reconciliation_info(&obj_json);

                let existing_info = self.state.get(&key);

                let should_add_history = if let (Some(event), Some(existing)) =
                    (&reconciliation_event, &existing_info)
                {
                    existing.last_reconciled != Some(event.timestamp)
                } else {
                    reconciliation_event.is_some()
                };

                let (suspended, ready, message, revision) = extract_status_fields(&obj_json);

                // Stateless resources have no status.conditions — mark as ready
                let ready = if ready.is_none() {
                    if let Some(kind) = FluxResourceKind::parse_optional(&resource_type) {
                        if kind.is_stateless() {
                            Some(true)
                        } else {
                            ready
                        }
                    } else {
                        ready
                    }
                } else {
                    ready
                };

                let labels = extract_labels(&obj_json);
                let annotations = extract_annotations(&obj_json);

                let mut history = if let Some(existing) = existing_info {
                    existing.reconciliation_history.clone()
                } else {
                    Vec::new()
                };

                if should_add_history {
                    if let Some(event) = reconciliation_event.clone() {
                        history.push(event);
                        if history.len() > MAX_RECONCILIATION_HISTORY {
                            history.remove(0);
                        }
                    }
                }

                self.state.upsert(
                    key,
                    ResourceInfo {
                        name,
                        namespace: ns,
                        resource_type,
                        age: Some(chrono::Utc::now()),
                        suspended,
                        ready,
                        message,
                        revision,
                        labels,
                        annotations,
                        last_reconciled: reconciliation_event.as_ref().map(|e| e.timestamp),
                        reconciliation_history: history,
                    },
                );
            }
            WatchEvent::Deleted(resource_type, ns, name) => {
                let key = resource_key(&ns, &name, &resource_type);
                self.state.remove(&key);
            }
            // Pod/Deployment events are TUI-specific (controller status bar)
            WatchEvent::Error(msg) => {
                tracing::warn!("Watch event error: {}", msg);
            }
            WatchEvent::PodApplied(_, _)
            | WatchEvent::PodDeleted(_)
            | WatchEvent::DeploymentApplied(_) => {}
        }
    }

    /// Drain all pending events and apply them to state.
    ///
    /// Returns the number of events processed.
    pub fn drain_events(&mut self) -> usize {
        let mut count = 0;
        while let Ok(event) = self.event_rx.try_recv() {
            self.apply_event(event);
            count += 1;
        }
        count
    }

    /// Switch to a different cluster context.
    ///
    /// Creates a new client and restarts watchers. State is cleared.
    pub async fn switch_context(
        &mut self,
        context: &str,
        controller_namespace: &str,
    ) -> Result<()> {
        let new_client = crate::kube::create_client_for_context(context)
            .await
            .with_context(|| format!("Failed to switch to context '{}'", context))?;

        self.watcher.stop();
        self.state.clear();

        let (mut new_watcher, new_event_rx) = ResourceWatcher::new(
            new_client.clone(),
            self.namespace.clone(),
            controller_namespace.to_string(),
        );

        new_watcher
            .watch_all()
            .context("Failed to start watchers after context switch")?;

        self.client = new_client;
        self.context = context.to_string();
        self.controller_namespace = controller_namespace.to_string();
        self.watcher = new_watcher;
        self.event_rx = new_event_rx;

        Ok(())
    }

    /// Change the namespace filter and restart watchers.
    pub fn set_namespace(&mut self, namespace: Option<String>) -> Result<()> {
        self.state.clear();
        self.watcher.set_namespace(namespace.clone())?;
        self.namespace = namespace;
        Ok(())
    }
}
