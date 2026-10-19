//! Async operation management
//!
//! The per-view fetches (YAML, describe, trace, graph) are plain
//! [`AsyncTask`](crate::tui::app::async_task::AsyncTask) slots on
//! [`AsyncOperationState`](super::state::AsyncOperationState); the main loop
//! dispatches and polls them directly. This module keeps only the flows with
//! extra semantics: mutating operations (registry validation, success message
//! bookkeeping) and the graph result hook (initial keyboard focus).

use super::core::App;

/// Request to save edited resource YAML via Server Side Apply
pub struct EditSaveRequest {
    /// The object being saved (Flux or native)
    pub target: crate::kube::objects::ObjectRef,
    /// Full YAML string from the editor (must include resourceVersion for conflict detection)
    pub yaml_to_apply: String,
    /// Kubernetes client to use for the SSA patch
    pub client: kube::Client,
    /// Channel to send the apply result back
    pub tx: tokio::sync::oneshot::Sender<anyhow::Result<()>>,
}

/// Request to execute an operation on a resource
pub struct OperationRequest {
    /// The type of resource to operate on
    pub resource_type: String,
    /// The namespace of the resource
    pub namespace: String,
    /// The name of the resource
    pub name: String,
    /// The operation keybinding character (e.g., 's' for suspend, 'r' for resume)
    pub operation_key: char,
    /// Kubernetes client to use for API calls
    pub client: kube::Client,
    /// Channel to send the operation result back
    pub tx: tokio::sync::oneshot::Sender<anyhow::Result<()>>,
}

impl App {
    /// Trigger operation execution if pending
    pub fn trigger_operation_execution(&mut self) -> Option<OperationRequest> {
        let pending = self.async_state.operation.pending()?;
        let client = self.kube_client.as_ref()?;
        self.operation_registry
            .get_by_keybinding(pending.operation_key)?;

        let client = client.clone();
        let (pending, tx) = self.async_state.operation.dispatch()?;
        // Store operation key for the success message
        self.async_state.last_operation_key = Some(pending.operation_key);
        Some(OperationRequest {
            resource_type: pending.resource_type,
            namespace: pending.namespace,
            name: pending.name,
            operation_key: pending.operation_key,
            client,
            tx,
        })
    }

    /// Try to get operation result
    pub fn try_get_operation_result(&mut self) -> Option<anyhow::Result<()>> {
        self.async_state.operation.try_recv()
    }

    /// Trigger the SSA apply if edited YAML is pending.
    ///
    /// Returns an [`EditSaveRequest`] when all conditions are met (a kube client
    /// is present, `edit_save_pending` is set, and no save is already in flight).
    pub fn trigger_edit_save(&mut self) -> Option<EditSaveRequest> {
        self.async_state.edit_save_pending.as_ref()?;
        if self.async_state.edit_save_result_rx.is_some() {
            return None;
        }
        let target = self.async_state.edit_pending.clone()?;
        let client = self.kube_client.clone()?;
        let yaml_to_apply = self.async_state.edit_save_pending.take()?;

        let (tx, rx) = tokio::sync::oneshot::channel();
        self.async_state.edit_save_result_rx = Some(rx);

        Some(EditSaveRequest {
            target,
            yaml_to_apply,
            client,
            tx,
        })
    }

    /// Try to get the SSA apply result (non-blocking).
    pub fn try_get_edit_save_result(&mut self) -> Option<anyhow::Result<()>> {
        if let Some(ref mut rx) = self.async_state.edit_save_result_rx {
            match rx.try_recv() {
                Ok(result) => {
                    self.async_state.edit_save_result_rx = None;
                    return Some(result);
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                    return None;
                }
                Err(_) => {
                    self.async_state.edit_save_result_rx = None;
                    return Some(Err(anyhow::anyhow!("Edit save failed")));
                }
            }
        }
        None
    }

    /// Handle the SSA apply result, updating status and returning to list view on success.
    pub fn set_edit_save_result(&mut self, result: anyhow::Result<()>) {
        match result {
            Ok(_) => {
                tracing::info!("Resource edit applied successfully via SSA");
                // Back to wherever `e` was pressed (clears the edit state).
                self.leave_edit();
                self.set_status_message(("Resource saved successfully".to_string(), false));
            }
            Err(e) => {
                tracing::warn!("Resource edit SSA apply failed: {}", e);
                let msg = e.to_string();
                self.leave_edit();
                self.async_state.edit_error_message = Some(msg.clone());
                self.set_status_message((format!("Save failed: {}", msg), true));
            }
        }
    }

    /// Question for the pending confirmation dialog, if one is open.
    pub fn confirmation_message(&self) -> Option<String> {
        use super::state::Confirmation;
        Some(match self.async_state.confirmation_pending.as_ref()? {
            Confirmation::Flux(pending) => self
                .operation_registry
                .get_by_keybinding(pending.operation_key)
                .and_then(|operation| {
                    self.state
                        .get(&crate::watcher::resource_key(
                            &pending.namespace,
                            &pending.name,
                            &pending.resource_type,
                        ))
                        .map(|resource| operation.confirmation_message(&resource))
                })
                .unwrap_or_else(|| "Resource not found".to_string()),
            Confirmation::Object(action) => action.confirmation_message(),
            Confirmation::Edit(message) => message.clone(),
        })
    }

    /// Record a finished workload action (#263). The live workload watch
    /// picks up the resulting rollout / replacement pod on its own.
    pub fn set_object_action_result(
        &mut self,
        result: anyhow::Result<crate::kube::workloads::ObjectAction>,
    ) {
        match result {
            Ok(action) => {
                self.set_status_message((action.success_message(), false));
            }
            Err(e) => self.set_status_message((format!("Operation failed: {:#}", e), true)),
        }
    }

    /// Set operation result and update status message
    pub fn set_operation_result(&mut self, result: anyhow::Result<()>) {
        match result {
            Ok(_) => {
                let name = self
                    .async_state
                    .last_operation_key
                    .take()
                    .and_then(|op_key| self.operation_registry.get_by_keybinding(op_key))
                    .map(|operation| operation.name().to_string());
                match name {
                    Some(name) => {
                        self.set_status_message((format!("{} completed successfully", name), false))
                    }
                    None => self.set_status_message((
                        "Operation completed successfully".to_string(),
                        false,
                    )),
                }
            }
            Err(e) => {
                self.async_state.last_operation_key = None;
                self.set_status_message((format!("Operation failed: {}", e), true));
            }
        }
    }

    /// Store the graph result and start keyboard focus on the resource being
    /// viewed (the object node) so the graph is immediately navigable with
    /// j/k and Enter.
    /// A graph opened with `g` on a native object (#266) focuses the node
    /// listing that object (or its owner) instead, and marks it there.
    pub fn set_graph_result(&mut self, result: impl Into<super::state::GraphResult>) {
        let result = result.into();
        self.view_state.graph_focus_index = result
            .highlight
            .as_ref()
            .and_then(|key| result.graph.node_containing(key))
            .or_else(|| result.graph.object_node_index());
        self.view_state.graph_highlight = result.highlight.clone();
        self.async_state.graph.set_result(result);
    }

    /// The graph on screen, if built.
    pub(crate) fn graph(&self) -> Option<&crate::trace::ResourceGraph> {
        self.async_state.graph.result().map(|r| &r.graph)
    }

    /// Store a YAML result. A native object whose manager needs its owner
    /// chain walked gets that resolved in the background, so `t` / `g` /
    /// `m` (#266) can offer its Flux manager.
    pub fn set_yaml_result(&mut self, yaml: serde_json::Value) {
        self.view_state.yaml_manager = None;
        if self.selection_state.native_object.is_some()
            && crate::kube::ownership::immediate_manager(&yaml).is_none()
        {
            self.async_state.describe_manager.request(yaml.clone());
        }
        self.async_state.yaml.set_result(yaml);
    }

    /// Store a describe result. When Managed By needs an owner chain walked,
    /// queue that separately so the object and events render right away.
    pub fn set_describe_result(&mut self, describe: crate::kube::fetch::DescribeData) {
        if describe.managed_by.is_none() {
            self.async_state
                .describe_manager
                .request(describe.object.clone());
        } else {
            self.async_state.describe_manager.clear();
        }
        self.async_state.describe.set_result(describe);
    }

    /// Fill the describe (or YAML) view's Managed By — only if it is still
    /// showing the object that was resolved (a late answer for an earlier
    /// object is dropped).
    pub fn set_manager_resolution(&mut self, resolution: super::state::ManagerResolution) {
        let uid_of = |object: &serde_json::Value| {
            object
                .pointer("/metadata/uid")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        };
        if self
            .async_state
            .yaml
            .result()
            .and_then(uid_of)
            .is_some_and(|uid| uid == resolution.uid)
        {
            self.view_state.yaml_manager = Some(resolution.clone());
        }
        if let Some(describe) = self.async_state.describe.result_mut()
            && describe
                .object
                .pointer("/metadata/uid")
                .and_then(serde_json::Value::as_str)
                == Some(resolution.uid.as_str())
        {
            describe.managed_by = Some(resolution.managed_by);
        }
    }

    /// Whether the describe view's Managed By is still being resolved.
    pub fn describe_manager_pending(&self) -> bool {
        self.async_state.describe_manager.is_loading()
            && self
                .async_state
                .describe
                .result()
                .is_some_and(|d| d.managed_by.is_none())
    }
}

#[cfg(test)]
mod tests {
    use super::super::state::{NavEntry, View};
    use super::*;
    use crate::config::{Config, UiConfig};
    use crate::tui::Theme;
    use crate::watcher::ResourceState;
    use std::collections::HashMap;

    fn create_test_app() -> App {
        let state = ResourceState::new();
        let config = Config {
            read_only: false,
            default_namespace: "".to_string(),
            default_controller_namespace: "".to_string(),
            namespace_hotkeys: vec![],
            ui: UiConfig {
                enable_mouse: false,
                headless: false,
                no_icons: false,
                skin: "default".to_string(),
                skin_read_only: None,
                splashless: true,
                rbac_warnings: true,
            },
            context_skins: HashMap::new(),
            cluster: HashMap::new(),
            favorites: vec![],
            default_resource_filter: None,
            views: Default::default(),
            connect_timeout_seconds: crate::kube::health::DEFAULT_CONNECT_TIMEOUT_SECS,
            discover_flux_resources: false,
            native_resources: true,
            metrics_source: crate::kube::metrics::MetricsSourceSetting::Auto,
            editor: None,
        };
        let theme = Theme::default();
        App::new(state, "test-context".to_string(), None, config, theme)
    }

    fn set_edit_in_progress(app: &mut App) {
        use crate::watcher::ResourceKey;
        app.async_state.edit_pending = Some(
            ResourceKey {
                resource_type: "Kustomization".to_string(),
                namespace: "flux-system".to_string(),
                name: "my-ks".to_string(),
            }
            .into(),
        );
        app.async_state.edit_full_yaml = Some(serde_json::json!({"apiVersion": "v1"}));
        app.async_state.edit_save_pending = Some("apiVersion: v1".to_string());
        app.async_state.edit_editor_launched = true;
        app.view_state.current_view = View::ResourceEdit;
        app.view_state.nav_stack = vec![NavEntry::View(View::ResourceList)];
    }

    #[test]
    fn test_set_edit_save_result_success_clears_state_and_returns_to_list() {
        let mut app = create_test_app();
        set_edit_in_progress(&mut app);

        app.set_edit_save_result(Ok(()));

        // All edit state cleared
        assert!(app.async_state.edit_pending.is_none());
        assert!(app.async_state.edit_full_yaml.is_none());
        assert!(app.async_state.edit_save_pending.is_none());
        assert!(app.async_state.edit_error_message.is_none());
        assert!(!app.async_state.edit_editor_launched);
        // Returns to list view
        assert_eq!(app.view_state.current_view, View::ResourceList);
        // Shows success status
        let (msg, is_error) = app.ui_state.status_message.as_ref().unwrap();
        assert!(!is_error, "success result should not be an error message");
        assert!(
            msg.contains("saved"),
            "success message should mention 'saved'"
        );
    }

    #[test]
    fn test_set_edit_save_result_error_clears_state_and_returns_to_list() {
        let mut app = create_test_app();
        set_edit_in_progress(&mut app);

        app.set_edit_save_result(Err(anyhow::anyhow!("conflict: resource was modified")));

        // All edit state cleared even on error
        assert!(app.async_state.edit_pending.is_none());
        assert!(app.async_state.edit_full_yaml.is_none());
        assert!(app.async_state.edit_save_pending.is_none());
        assert!(!app.async_state.edit_editor_launched);
        // Error message stored
        assert!(app.async_state.edit_error_message.is_some());
        // Returns to list view (not stuck on ResourceEdit)
        assert_eq!(app.view_state.current_view, View::ResourceList);
        // Shows error status
        let (msg, is_error) = app.ui_state.status_message.as_ref().unwrap();
        assert!(is_error, "error result should set is_error=true");
        assert!(
            msg.contains("conflict"),
            "error message should propagate the error text"
        );
    }

    #[test]
    fn test_set_edit_save_result_error_returns_to_origin_view() {
        let mut app = create_test_app();
        set_edit_in_progress(&mut app);
        // Simulate pressing 'e' from ResourceFavorites
        app.view_state.nav_stack = vec![
            NavEntry::View(View::ResourceList),
            NavEntry::View(View::ResourceFavorites),
        ];

        app.set_edit_save_result(Err(anyhow::anyhow!("some error")));

        assert_eq!(app.view_state.current_view, View::ResourceFavorites);
    }

    #[test]
    fn test_set_edit_save_result_success_returns_to_yaml_view() {
        let mut app = create_test_app();
        set_edit_in_progress(&mut app);
        // Simulate pressing 'e' from ResourceYAML
        app.view_state.nav_stack = vec![
            NavEntry::View(View::ResourceList),
            NavEntry::View(View::ResourceYAML),
        ];

        app.set_edit_save_result(Ok(()));

        assert_eq!(app.view_state.current_view, View::ResourceYAML);
    }

    #[test]
    fn workload_action_success_reports() {
        use crate::kube::workloads::{ObjectAction, WorkloadData};
        let mut app = create_test_app();
        app.view_state.current_view = super::super::state::View::WorkloadDetail;
        app.async_state.workload.set_result(WorkloadData {
            kind: "Deployment".into(),
            name: "web".into(),
            namespace: "apps".into(),
            ready: Some(true),
            summary: Vec::new(),
            containers: Vec::new(),
            pods: Vec::new(),
            events: Vec::new(),
            events_error: None,
            pod_selector: None,
            managed_by: None,
            api_version: Some("apps/v1".to_string()),
        });

        app.set_object_action_result(Ok(ObjectAction::Patch {
            target: crate::kube::objects::ObjectRef::native("apps/v1", "Deployment", "apps", "web"),
            verb: "Restart".into(),
            done: "Restarted".into(),
            detail: String::new(),
            patch: serde_json::json!({}),
        }));

        assert_eq!(
            app.ui_state.status_message,
            Some(("Restarted Deployment web".to_string(), false))
        );
    }

    #[test]
    fn workload_action_failure_reports_error() {
        let mut app = create_test_app();
        app.set_object_action_result(Err(anyhow::anyhow!("forbidden")));
        assert!(
            app.ui_state
                .status_message
                .as_ref()
                .is_some_and(|(msg, is_error)| *is_error && msg.contains("forbidden"))
        );
    }

    fn describe_of(uid: &str, managed_by: Option<&str>) -> crate::kube::fetch::DescribeData {
        crate::kube::fetch::DescribeData {
            object: serde_json::json!({"metadata": {"name": "web-abc", "uid": uid}}),
            events: Vec::new(),
            events_error: None,
            managed_by: managed_by.map(|text| crate::kube::ownership::ManagedBy {
                text: text.to_string(),
                flux: None,
            }),
        }
    }

    #[test]
    fn describe_shows_first_and_fills_managed_by_later() {
        use super::super::state::ManagerResolution;
        let mut app = create_test_app();
        app.set_describe_result(describe_of("pod-1", None));
        assert!(
            app.async_state.describe.result().is_some(),
            "the object renders without waiting for the owner chain"
        );
        assert!(app.describe_manager_pending());
        let (object, _tx) = app.async_state.describe_manager.dispatch().unwrap();
        assert_eq!(object["metadata"]["uid"], "pod-1");

        app.set_manager_resolution(ManagerResolution {
            uid: "pod-1".into(),
            managed_by: crate::kube::ownership::ManagedBy {
                text: "Flux Kustomization flux-system/apps (via ReplicaSet/web-7b5)".into(),
                flux: None,
            },
        });
        let describe = app.async_state.describe.result().unwrap();
        assert_eq!(
            describe.managed_by.as_ref().map(|m| m.text.as_str()),
            Some("Flux Kustomization flux-system/apps (via ReplicaSet/web-7b5)")
        );
        assert!(!app.describe_manager_pending());
    }

    #[test]
    fn late_manager_answers_for_another_object_are_dropped() {
        use super::super::state::ManagerResolution;
        let mut app = create_test_app();
        app.set_describe_result(describe_of("pod-2", None));
        app.set_manager_resolution(ManagerResolution {
            uid: "pod-1".into(),
            managed_by: crate::kube::ownership::ManagedBy {
                text: "stale".into(),
                flux: None,
            },
        });
        assert_eq!(app.async_state.describe.result().unwrap().managed_by, None);
    }

    #[test]
    fn describe_with_immediate_manager_queues_no_lookup() {
        let mut app = create_test_app();
        app.set_describe_result(describe_of("pod-3", Some("Helm release apps/web")));
        assert!(app.async_state.describe_manager.dispatch().is_none());
        assert!(!app.describe_manager_pending());
    }
}
