//! Async operation management
//!
//! This module handles all asynchronous operations including YAML fetching,
//! tracing, graph building, and resource operations with their result channels.

use super::core::App;
use crate::watcher::ResourceKey;

/// Request to trace a resource's ownership chain
pub struct TraceRequest {
    /// The type of resource to trace (e.g., "Kustomization", "HelmRelease")
    pub resource_type: String,
    /// The namespace of the resource
    pub namespace: String,
    /// The name of the resource
    pub name: String,
    /// Kubernetes client to use for API calls
    pub client: kube::Client,
    /// Channel to send the trace result back
    pub tx: tokio::sync::oneshot::Sender<anyhow::Result<crate::tui::trace::TraceResult>>,
}

/// Request to save edited spec for a resource
pub struct EditSaveRequest {
    /// The resource being edited
    pub resource_key: ResourceKey,
    /// Parsed spec to apply
    pub spec: serde_json::Value,
    /// Kubernetes client to use for API calls
    pub client: kube::Client,
    /// Channel to send the save result back
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
    /// Trigger YAML fetch if pending
    ///
    /// Returns (resource_key, client, result_channel) if fetch should be triggered
    pub fn trigger_yaml_fetch(
        &mut self,
    ) -> Option<(
        String,
        kube::Client,
        tokio::sync::oneshot::Sender<anyhow::Result<serde_json::Value>>,
    )> {
        if let Some(ref key) = self.async_state.yaml_fetch_pending {
            if let Some(ref client) = self.kube_client {
                let (tx, rx) = tokio::sync::oneshot::channel();
                let key_clone = key.clone();
                let client_clone = client.clone();
                self.async_state.yaml_fetch_pending = None;
                self.async_state.yaml_fetch_rx = Some(rx);
                return Some((key_clone, client_clone, tx));
            }
        }
        None
    }

    /// Trigger describe fetch if pending
    pub fn trigger_describe_fetch(
        &mut self,
    ) -> Option<(
        String,
        kube::Client,
        tokio::sync::oneshot::Sender<anyhow::Result<serde_json::Value>>,
    )> {
        if let Some(ref key) = self.async_state.describe_fetch_pending {
            if let Some(ref client) = self.kube_client {
                let (tx, rx) = tokio::sync::oneshot::channel();
                let key_clone = key.clone();
                let client_clone = client.clone();
                self.async_state.describe_fetch_pending = None;
                self.async_state.describe_fetch_rx = Some(rx);
                return Some((key_clone, client_clone, tx));
            }
        }
        None
    }

    /// Set YAML fetch result
    pub fn set_yaml_fetched(&mut self, yaml: serde_json::Value) {
        self.async_state.yaml_fetched = Some(yaml.clone());

        if self.view_state.current_view == crate::tui::app::state::View::ResourceEdit
            && self.async_state.editor_state.is_none()
        {
            let spec_value = yaml
                .get("spec")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            match serde_yaml::to_string(&spec_value) {
                Ok(spec_yaml) => {
                    self.async_state.edit_yaml = Some(spec_yaml.clone());
                    self.async_state.editor_state =
                        Some(crate::tui::app::state::EditorState::new(&spec_yaml));
                }
                Err(e) => {
                    self.async_state.edit_error_message =
                        Some(format!("Failed to initialize editor: {}", e));
                }
            }
        }
    }

    /// Set YAML fetch error
    pub fn set_yaml_fetch_error(&mut self) {
        self.async_state.yaml_fetched = None;
        self.async_state.yaml_fetch_pending = None;
    }

    /// Set describe fetch result
    pub fn set_describe_fetched(&mut self, describe: serde_json::Value) {
        self.async_state.describe_fetched = Some(describe);
    }

    /// Set describe fetch error
    pub fn set_describe_fetch_error(&mut self) {
        self.async_state.describe_fetched = None;
        self.async_state.describe_fetch_pending = None;
    }

    /// Try to get YAML fetch result
    pub fn try_get_yaml_result(&mut self) -> Option<anyhow::Result<serde_json::Value>> {
        if let Some(ref mut rx) = self.async_state.yaml_fetch_rx {
            match rx.try_recv() {
                Ok(result) => {
                    self.async_state.yaml_fetch_rx = None;
                    return Some(result);
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                    return None;
                }
                Err(_) => {
                    self.async_state.yaml_fetch_rx = None;
                    return Some(Err(anyhow::anyhow!("YAML fetch failed")));
                }
            }
        }
        None
    }

    /// Try to get describe fetch result
    pub fn try_get_describe_result(&mut self) -> Option<anyhow::Result<serde_json::Value>> {
        if let Some(ref mut rx) = self.async_state.describe_fetch_rx {
            match rx.try_recv() {
                Ok(result) => {
                    self.async_state.describe_fetch_rx = None;
                    return Some(result);
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                    return None;
                }
                Err(_) => {
                    self.async_state.describe_fetch_rx = None;
                    return Some(Err(anyhow::anyhow!("Describe fetch failed")));
                }
            }
        }
        None
    }

    /// Trigger trace if pending
    pub fn trigger_trace(&mut self) -> Option<TraceRequest> {
        if let Some(ref rk) = self.async_state.trace_pending {
            if let Some(ref client) = self.kube_client {
                let (tx, rx) = tokio::sync::oneshot::channel();
                let request = TraceRequest {
                    resource_type: rk.resource_type.clone(),
                    namespace: rk.namespace.clone(),
                    name: rk.name.clone(),
                    client: client.clone(),
                    tx,
                };
                self.async_state.trace_pending = None;
                self.async_state.trace_result_rx = Some(rx);
                return Some(request);
            }
        }
        None
    }

    /// Set trace result
    pub fn set_trace_result(&mut self, result: crate::tui::trace::TraceResult) {
        self.async_state.trace_result = Some(result);
    }

    /// Set trace error
    pub fn set_trace_error(&mut self) {
        self.async_state.trace_result = None;
        self.async_state.trace_pending = None;
    }

    /// Try to get trace result
    pub fn try_get_trace_result(
        &mut self,
    ) -> Option<anyhow::Result<crate::tui::trace::TraceResult>> {
        if let Some(ref mut rx) = self.async_state.trace_result_rx {
            match rx.try_recv() {
                Ok(result) => {
                    self.async_state.trace_result_rx = None;
                    return Some(result);
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                    return None;
                }
                Err(_) => {
                    self.async_state.trace_result_rx = None;
                    return Some(Err(anyhow::anyhow!("Trace failed")));
                }
            }
        }
        None
    }

    /// Trigger graph building if pending
    pub fn trigger_graph(
        &mut self,
    ) -> Option<(
        ResourceKey,
        kube::Client,
        tokio::sync::oneshot::Sender<anyhow::Result<crate::trace::ResourceGraph>>,
    )> {
        if let Some(ref rk) = self.async_state.graph_pending {
            if let Some(ref client) = self.kube_client {
                let (tx, rx) = tokio::sync::oneshot::channel();
                let request = (rk.clone(), client.clone(), tx);
                self.async_state.graph_pending = None;
                self.async_state.graph_result_rx = Some(rx);
                return Some(request);
            }
        }
        None
    }

    /// Trigger edit save if pending
    pub fn trigger_edit_save(&mut self) -> Option<EditSaveRequest> {
        if let (Some(spec), Some(resource_key), Some(client)) = (
            self.async_state.edit_save_pending.clone(),
            self.async_state.edit_pending.clone(),
            self.kube_client.clone(),
        ) {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let request = EditSaveRequest {
                resource_key,
                spec,
                client,
                tx,
            };
            self.async_state.edit_save_pending = None;
            self.async_state.edit_save_result_rx = Some(rx);
            return Some(request);
        }
        None
    }

    /// Try to get edit save result
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

    /// Set edit save result and update state
    pub fn set_edit_save_result(&mut self, result: anyhow::Result<()>) {
        self.async_state.edit_save_pending = None;
        self.async_state.edit_save_result_rx = None;
        match result {
            Ok(_) => {
                self.async_state.edit_pending = None;
                self.async_state.edit_yaml = None;
                self.async_state.editor_state = None;
                self.async_state.edit_error_message = None;
                self.view_state.current_view = self.view_state.previous_list_view;
                self.set_status_message(("Saved edits successfully".to_string(), false));
            }
            Err(e) => {
                let message = format!("Save failed: {}", e);
                self.async_state.edit_error_message = Some(message.clone());
                self.set_status_message((message, true));
            }
        }
    }

    /// Try to get graph result
    pub fn try_get_graph_result(&mut self) -> Option<anyhow::Result<crate::trace::ResourceGraph>> {
        if let Some(ref mut rx) = self.async_state.graph_result_rx {
            match rx.try_recv() {
                Ok(result) => {
                    self.async_state.graph_result_rx = None;
                    return Some(result);
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                    return None;
                }
                Err(_) => {
                    self.async_state.graph_result_rx = None;
                    return Some(Err(anyhow::anyhow!("Graph building failed")));
                }
            }
        }
        None
    }

    /// Set graph result
    pub fn set_graph_result(&mut self, result: crate::trace::ResourceGraph) {
        self.async_state.graph_result = Some(result);
    }

    /// Set graph error
    pub fn set_graph_error(&mut self) {
        self.async_state.graph_result = None;
        self.async_state.graph_pending = None;
    }

    /// Trigger operation execution if pending
    pub fn trigger_operation_execution(&mut self) -> Option<OperationRequest> {
        if let Some(ref pending) = self.async_state.pending_operation {
            if let Some(ref client) = self.kube_client {
                if self
                    .operation_registry
                    .get_by_keybinding(pending.operation_key)
                    .is_some()
                {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    let request = OperationRequest {
                        resource_type: pending.resource_type.clone(),
                        namespace: pending.namespace.clone(),
                        name: pending.name.clone(),
                        operation_key: pending.operation_key,
                        client: client.clone(),
                        tx,
                    };

                    self.async_state.last_operation_key = Some(pending.operation_key); // Store operation key for success message
                    self.async_state.pending_operation = None;
                    self.async_state.operation_result_rx = Some(rx);

                    return Some(request);
                }
            }
        }
        None
    }

    /// Try to get operation result
    pub fn try_get_operation_result(&mut self) -> Option<anyhow::Result<()>> {
        if let Some(ref mut rx) = self.async_state.operation_result_rx {
            match rx.try_recv() {
                Ok(result) => {
                    self.async_state.operation_result_rx = None;
                    return Some(result);
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                    return None;
                }
                Err(_) => {
                    self.async_state.operation_result_rx = None;
                    return Some(Err(anyhow::anyhow!("Operation failed")));
                }
            }
        }
        None
    }

    /// Set operation result and update status message
    pub fn set_operation_result(&mut self, result: anyhow::Result<()>) {
        match result {
            Ok(_) => {
                if let Some(op_key) = self.async_state.last_operation_key.take() {
                    if let Some(operation) = self.operation_registry.get_by_keybinding(op_key) {
                        self.set_status_message((
                            format!("{} completed successfully", operation.name()),
                            false,
                        ));
                    } else {
                        self.set_status_message((
                            "Operation completed successfully".to_string(),
                            false,
                        ));
                    }
                } else {
                    self.set_status_message((
                        "Operation completed successfully".to_string(),
                        false,
                    ));
                }
            }
            Err(e) => {
                self.async_state.last_operation_key = None;
                self.set_status_message((format!("Operation failed: {}", e), true));
            }
        }
    }
}
