//! Async operation management
//!
//! The per-view fetches (YAML, describe, trace, graph) are plain
//! [`AsyncTask`](crate::tui::app::async_task::AsyncTask) slots on
//! [`AsyncOperationState`](super::state::AsyncOperationState); the main loop
//! dispatches and polls them directly. This module keeps only the flows with
//! extra semantics: mutating operations (registry validation, success message
//! bookkeeping) and the graph result hook (initial keyboard focus).

use super::core::App;

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
