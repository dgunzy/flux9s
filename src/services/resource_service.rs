//! Resource service for Kubernetes operations
//!
//! This service abstracts all Kubernetes API interactions away from the TUI layer.
//! It handles fetching resources, executing operations, and tracing ownership chains.

use crate::trace::{ResourceGraph, TraceResult};
use crate::tui::operations::FluxOperation;
use crate::watcher::ResourceKey;
use anyhow::{Context, Result};

/// Service for managing resource operations
pub struct ResourceService {
    client: kube::Client,
}

impl ResourceService {
    pub fn new(client: kube::Client) -> Self {
        Self { client }
    }

    /// Fetch YAML for a resource
    pub async fn fetch_yaml(
        &self,
        resource_type: &str,
        namespace: &str,
        name: &str,
    ) -> Result<serde_json::Value> {
        crate::tui::fetch_resource_yaml(&self.client, resource_type, namespace, name)
            .await
            .context("Failed to fetch resource YAML")
    }

    /// Trace ownership chain for a resource
    pub async fn trace_resource(&self, resource_key: &ResourceKey) -> Result<TraceResult> {
        crate::trace::trace_object(
            &self.client,
            &resource_key.resource_type,
            &resource_key.namespace,
            &resource_key.name,
        )
        .await
        .context("Failed to trace resource")
    }

    /// Build dependency graph for a resource
    pub async fn build_graph(&self, resource_key: &ResourceKey) -> Result<ResourceGraph> {
        crate::trace::build_resource_graph(
            &self.client,
            &resource_key.resource_type,
            &resource_key.namespace,
            &resource_key.name,
        )
        .await
        .context("Failed to build resource graph")
    }

    /// Execute an operation on a resource
    pub async fn execute_operation(
        &self,
        operation: &dyn FluxOperation,
        resource_type: &str,
        namespace: &str,
        name: &str,
    ) -> Result<()> {
        operation
            .execute(&self.client, resource_type, namespace, name)
            .await
            .context(format!(
                "Failed to execute operation {} on {}/{}/{}",
                operation.name(),
                resource_type,
                namespace,
                name
            ))
    }

    /// Get a reference to the underlying Kubernetes client
    pub fn client(&self) -> &kube::Client {
        &self.client
    }
}
