//! Async operation management
//!
//! The per-view fetches (YAML, describe, trace, graph) are plain
//! [`AsyncTask`](crate::tui::app::async_task::AsyncTask) slots on
//! [`AsyncOperationState`](super::state::AsyncOperationState); the main loop
//! dispatches and polls them directly. This module keeps only the flows with
//! extra semantics: mutating operations (registry validation, success message
//! bookkeeping) and the graph result hook (initial keyboard focus).

use super::core::App;
use crate::watcher::ResourceKey;

/// Request to save edited resource YAML via Server Side Apply
pub struct EditSaveRequest {
    /// Key of the resource being saved
    pub resource_key: ResourceKey,
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
        let rk = self.async_state.edit_pending.clone()?;
        let client = self.kube_client.clone()?;
        let yaml_to_apply = self.async_state.edit_save_pending.take()?;

        let (tx, rx) = tokio::sync::oneshot::channel();
        self.async_state.edit_save_result_rx = Some(rx);

        Some(EditSaveRequest {
            resource_key: rk,
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
                // Clear all edit state
                self.async_state.edit_pending = None;
                self.async_state.edit_full_yaml = None;
                self.async_state.edit_save_pending = None;
                self.async_state.edit_error_message = None;
                self.async_state.edit_editor_launched = false;
                // Return to origin view (wherever 'e' was pressed from)
                self.view_state.current_view = self.async_state.edit_return_view;
                self.async_state.edit_return_view = super::state::View::ResourceList;
                self.set_status_message(("Resource saved successfully".to_string(), false));
            }
            Err(e) => {
                tracing::warn!("Resource edit SSA apply failed: {}", e);
                let msg = e.to_string();
                self.async_state.edit_error_message = Some(msg.clone());
                self.async_state.edit_pending = None;
                self.async_state.edit_full_yaml = None;
                self.async_state.edit_save_pending = None;
                self.async_state.edit_editor_launched = false;
                self.view_state.current_view = self.async_state.edit_return_view;
                self.async_state.edit_return_view = super::state::View::ResourceList;
                self.set_status_message((format!("Save failed: {}", msg), true));
            }
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
    pub fn set_graph_result(&mut self, result: crate::trace::ResourceGraph) {
        self.view_state.graph_focus_index = result.object_node_index();
        self.async_state.graph.set_result(result);
    }
}

#[cfg(test)]
mod tests {
    use super::super::state::View;
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
            },
            context_skins: HashMap::new(),
            cluster: HashMap::new(),
            favorites: vec![],
            default_resource_filter: None,
            connect_timeout_seconds: crate::kube::health::DEFAULT_CONNECT_TIMEOUT_SECS,
            discover_flux_resources: false,
            editor: None,
        };
        let theme = Theme::default();
        App::new(state, "test-context".to_string(), None, config, theme)
    }

    fn set_edit_in_progress(app: &mut App) {
        use crate::watcher::ResourceKey;
        app.async_state.edit_pending = Some(ResourceKey {
            resource_type: "Kustomization".to_string(),
            namespace: "flux-system".to_string(),
            name: "my-ks".to_string(),
        });
        app.async_state.edit_full_yaml = Some(serde_json::json!({"apiVersion": "v1"}));
        app.async_state.edit_save_pending = Some("apiVersion: v1".to_string());
        app.async_state.edit_editor_launched = true;
        app.view_state.current_view = View::ResourceEdit;
        app.async_state.edit_return_view = View::ResourceList;
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
        app.async_state.edit_return_view = View::ResourceFavorites;

        app.set_edit_save_result(Err(anyhow::anyhow!("some error")));

        assert_eq!(app.view_state.current_view, View::ResourceFavorites);
    }

    #[test]
    fn test_set_edit_save_result_success_returns_to_yaml_view() {
        let mut app = create_test_app();
        set_edit_in_progress(&mut app);
        // Simulate pressing 'e' from ResourceYAML
        app.async_state.edit_return_view = View::ResourceYAML;

        app.set_edit_save_result(Ok(()));

        assert_eq!(app.view_state.current_view, View::ResourceYAML);
    }
}
