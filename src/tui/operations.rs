//! Flux operations module
//!
//! Provides extensible system for performing Flux operations on resources.
//! Operations are implemented as a trait-based system for easy extension.

use crate::watcher::ResourceInfo;
use anyhow::{Context, Result};
use kube::api::{Patch, PatchParams};
use kube::core::DynamicObject;
use kube::Api;
use serde_json::json;
#[cfg(test)]
use std::collections::HashMap;

use crate::tui::api::get_api_resource_with_fallback;

/// Helper to get a namespaced API for a resource with version fallback
async fn get_resource_api(
    client: &kube::Client,
    resource_type: &str,
    namespace: &str,
    name: &str,
) -> Result<Api<DynamicObject>> {
    let api_resource = get_api_resource_with_fallback(client, resource_type, namespace, name)
        .await
        .with_context(|| {
            format!(
                "Failed to get API resource for {}/{} in namespace {}",
                resource_type, name, namespace
            )
        })?;
    Ok(Api::namespaced_with(
        client.clone(),
        namespace,
        &api_resource,
    ))
}

/// Helper to check if a resource is suspended
fn is_resource_suspended(obj: &DynamicObject) -> bool {
    obj.data
        .get("spec")
        .and_then(|s| s.as_object())
        .and_then(|spec| spec.get("suspend"))
        .and_then(|s| s.as_bool())
        .unwrap_or(false)
}

/// Helper to get or create annotations map from a resource
fn get_annotations(obj: &DynamicObject) -> serde_json::Map<String, serde_json::Value> {
    obj.data
        .get("metadata")
        .and_then(|m| m.get("annotations"))
        .and_then(|a| a.as_object())
        .cloned()
        .unwrap_or_else(serde_json::Map::new)
}

/// Helper to add reconcile annotation to a resource
fn add_reconcile_annotation(annotations: &mut serde_json::Map<String, serde_json::Value>) {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
    annotations.insert("reconcile.fluxcd.io/requestedAt".to_string(), json!(now));
}

/// Trait for Flux operations
#[async_trait::async_trait]
pub trait FluxOperation: Send + Sync {
    /// Execute the operation on the given resource
    async fn execute(
        &self,
        client: &kube::Client,
        resource_type: &str,
        namespace: &str,
        name: &str,
    ) -> Result<()>;

    /// Keybinding character for this operation
    fn keybinding(&self) -> char;

    /// Whether this operation requires user confirmation
    fn requires_confirmation(&self) -> bool;

    /// Confirmation message to show to user
    fn confirmation_message(&self, resource: &ResourceInfo) -> String;

    /// Human-readable name for this operation
    fn name(&self) -> &'static str;

    /// Whether this operation is valid for the given resource type
    fn is_valid_for(&self, resource_type: &str) -> bool;
}

/// Suspend operation - suspends reconciliation
pub struct SuspendOperation;

#[async_trait::async_trait]
impl FluxOperation for SuspendOperation {
    async fn execute(
        &self,
        client: &kube::Client,
        resource_type: &str,
        namespace: &str,
        name: &str,
    ) -> Result<()> {
        tracing::debug!(
            "Suspending {}/{} in namespace {}",
            resource_type,
            name,
            namespace
        );

        let api = get_resource_api(client, resource_type, namespace, name).await?;

        // Patch spec.suspend to true
        let patch = json!({
            "spec": {
                "suspend": true
            }
        });

        api.patch(name, &PatchParams::default(), &Patch::Merge(patch))
            .await
            .with_context(|| {
                format!(
                    "Failed to suspend {}/{} in namespace {}",
                    resource_type, name, namespace
                )
            })?;

        tracing::info!(
            "Successfully suspended {}/{} in namespace {}",
            resource_type,
            name,
            namespace
        );
        Ok(())
    }

    fn keybinding(&self) -> char {
        's'
    }

    fn requires_confirmation(&self) -> bool {
        false
    }

    fn confirmation_message(&self, resource: &ResourceInfo) -> String {
        format!(
            "Suspend {} {} in {}?",
            resource.resource_type, resource.name, resource.namespace
        )
    }

    fn name(&self) -> &'static str {
        "Suspend"
    }

    fn is_valid_for(&self, resource_type: &str) -> bool {
        use crate::models::FluxResourceKind;
        matches!(
            FluxResourceKind::parse_optional(resource_type),
            Some(FluxResourceKind::GitRepository)
                | Some(FluxResourceKind::OCIRepository)
                | Some(FluxResourceKind::HelmRepository)
                | Some(FluxResourceKind::Kustomization)
                | Some(FluxResourceKind::HelmRelease)
                | Some(FluxResourceKind::ImageUpdateAutomation)
        )
    }
}

/// Resume operation - resumes reconciliation
pub struct ResumeOperation;

#[async_trait::async_trait]
impl FluxOperation for ResumeOperation {
    async fn execute(
        &self,
        client: &kube::Client,
        resource_type: &str,
        namespace: &str,
        name: &str,
    ) -> Result<()> {
        tracing::debug!(
            "Resuming {}/{} in namespace {}",
            resource_type,
            name,
            namespace
        );

        let api = get_resource_api(client, resource_type, namespace, name).await?;

        // Patch spec.suspend to false
        let patch = json!({
            "spec": {
                "suspend": false
            }
        });

        api.patch(name, &PatchParams::default(), &Patch::Merge(patch))
            .await
            .with_context(|| {
                format!(
                    "Failed to resume {}/{} in namespace {}",
                    resource_type, name, namespace
                )
            })?;

        tracing::info!(
            "Successfully resumed {}/{} in namespace {}",
            resource_type,
            name,
            namespace
        );
        Ok(())
    }

    fn keybinding(&self) -> char {
        'r'
    }

    fn requires_confirmation(&self) -> bool {
        false
    }

    fn confirmation_message(&self, resource: &ResourceInfo) -> String {
        format!(
            "Resume {} {} in {}?",
            resource.resource_type, resource.name, resource.namespace
        )
    }

    fn name(&self) -> &'static str {
        "Resume"
    }

    fn is_valid_for(&self, resource_type: &str) -> bool {
        use crate::models::FluxResourceKind;
        matches!(
            FluxResourceKind::parse_optional(resource_type),
            Some(FluxResourceKind::GitRepository)
                | Some(FluxResourceKind::OCIRepository)
                | Some(FluxResourceKind::HelmRepository)
                | Some(FluxResourceKind::Kustomization)
                | Some(FluxResourceKind::HelmRelease)
                | Some(FluxResourceKind::ImageUpdateAutomation)
        )
    }
}

/// Delete operation - deletes a resource
pub struct DeleteOperation;

#[async_trait::async_trait]
impl FluxOperation for DeleteOperation {
    async fn execute(
        &self,
        client: &kube::Client,
        resource_type: &str,
        namespace: &str,
        name: &str,
    ) -> Result<()> {
        use kube::api::DeleteParams;

        tracing::debug!(
            "Deleting {}/{} in namespace {}",
            resource_type,
            name,
            namespace
        );

        let api = get_resource_api(client, resource_type, namespace, name).await?;

        // First, verify the resource exists (like Flux does)
        api.get(name).await.with_context(|| {
            format!(
                "Resource {}/{} not found in namespace {}",
                resource_type, name, namespace
            )
        })?;

        // Then delete it
        api.delete(name, &DeleteParams::default())
            .await
            .with_context(|| {
                format!(
                    "Failed to delete {}/{} in namespace {}",
                    resource_type, name, namespace
                )
            })?;

        tracing::info!(
            "Successfully deleted {}/{} in namespace {}",
            resource_type,
            name,
            namespace
        );
        Ok(())
    }

    fn keybinding(&self) -> char {
        'd'
    }

    fn requires_confirmation(&self) -> bool {
        true
    }

    fn confirmation_message(&self, resource: &ResourceInfo) -> String {
        format!(
            "Delete {} {} in {}? (y/N)",
            resource.resource_type, resource.name, resource.namespace
        )
    }

    fn name(&self) -> &'static str {
        "Delete"
    }

    fn is_valid_for(&self, _resource_type: &str) -> bool {
        true // Delete works for all resources
    }
}

/// Reconcile operation - forces reconciliation
pub struct ReconcileOperation;

/// Reconcile with source operation - reconciles source first, then the resource
pub struct ReconcileWithSourceOperation;

#[async_trait::async_trait]
impl FluxOperation for ReconcileOperation {
    async fn execute(
        &self,
        client: &kube::Client,
        resource_type: &str,
        namespace: &str,
        name: &str,
    ) -> Result<()> {
        tracing::debug!(
            "Reconciling {}/{} in namespace {}",
            resource_type,
            name,
            namespace
        );

        let api = get_resource_api(client, resource_type, namespace, name).await?;

        // First, get the resource to verify it exists and get current state
        let obj = api.get(name).await.with_context(|| {
            format!(
                "Resource {}/{} not found in namespace {}",
                resource_type, name, namespace
            )
        })?;

        // Check if resource is suspended (like Flux does)
        if is_resource_suspended(&obj) {
            tracing::warn!(
                "Cannot reconcile suspended resource {}/{} in namespace {}",
                resource_type,
                name,
                namespace
            );
            return Err(anyhow::anyhow!(
                "Cannot reconcile {}/{} in namespace {}: resource is suspended",
                resource_type,
                name,
                namespace
            ));
        }

        // Get current annotations and add reconcile annotation
        let mut annotations = get_annotations(&obj);
        add_reconcile_annotation(&mut annotations);

        // Create merge patch for annotations
        let patch = json!({
            "metadata": {
                "annotations": annotations
            }
        });

        api.patch(name, &PatchParams::default(), &Patch::Merge(patch))
            .await
            .with_context(|| {
                format!(
                    "Failed to reconcile {}/{} in namespace {}",
                    resource_type, name, namespace
                )
            })?;

        tracing::info!(
            "Successfully triggered reconciliation for {}/{} in namespace {}",
            resource_type,
            name,
            namespace
        );
        Ok(())
    }

    fn keybinding(&self) -> char {
        'R'
    }

    fn requires_confirmation(&self) -> bool {
        false
    }

    fn confirmation_message(&self, resource: &ResourceInfo) -> String {
        format!(
            "Reconcile {} {} in {}?",
            resource.resource_type, resource.name, resource.namespace
        )
    }

    fn name(&self) -> &'static str {
        "Reconcile"
    }

    fn is_valid_for(&self, _resource_type: &str) -> bool {
        true // Reconcile works for all Flux resources
    }
}

#[async_trait::async_trait]
impl FluxOperation for ReconcileWithSourceOperation {
    async fn execute(
        &self,
        client: &kube::Client,
        resource_type: &str,
        namespace: &str,
        name: &str,
    ) -> Result<()> {
        use crate::models::FluxResourceKind;

        tracing::debug!(
            "Reconciling {}/{} with source in namespace {}",
            resource_type,
            name,
            namespace
        );

        // Only works for Kustomization and HelmRelease
        let kind = FluxResourceKind::parse_optional(resource_type);
        if !matches!(
            kind,
            Some(FluxResourceKind::Kustomization) | Some(FluxResourceKind::HelmRelease)
        ) {
            return Err(anyhow::anyhow!(
                "Reconcile with source only works for Kustomization and HelmRelease, not {}",
                resource_type
            ));
        }

        let api = get_resource_api(client, resource_type, namespace, name).await?;

        // Get the resource to check if it exists and get sourceRef
        let obj = api.get(name).await.with_context(|| {
            format!(
                "Resource {}/{} not found in namespace {}",
                resource_type, name, namespace
            )
        })?;

        // Check if resource is suspended
        if is_resource_suspended(&obj) {
            tracing::warn!(
                "Cannot reconcile suspended resource {}/{} in namespace {}",
                resource_type,
                name,
                namespace
            );
            return Err(anyhow::anyhow!(
                "Cannot reconcile {}/{} in namespace {}: resource is suspended",
                resource_type,
                name,
                namespace
            ));
        }

        // Extract sourceRef
        let source_ref = obj
            .data
            .get("spec")
            .and_then(|s| s.get("sourceRef"))
            .and_then(|sr| sr.as_object())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "{}/{} in namespace {} has no sourceRef",
                    resource_type,
                    name,
                    namespace
                )
            })?;

        let source_kind = source_ref
            .get("kind")
            .and_then(|k| k.as_str())
            .ok_or_else(|| anyhow::anyhow!("{}/{} sourceRef missing kind", resource_type, name))?;
        let source_name = source_ref
            .get("name")
            .and_then(|n| n.as_str())
            .ok_or_else(|| anyhow::anyhow!("{}/{} sourceRef missing name", resource_type, name))?;
        let source_namespace = source_ref
            .get("namespace")
            .and_then(|n| n.as_str())
            .unwrap_or(namespace);

        tracing::debug!(
            "Found source reference: {}/{} in namespace {}",
            source_kind,
            source_name,
            source_namespace
        );

        // Step 1: Reconcile the source first
        let source_api =
            get_resource_api(client, source_kind, source_namespace, source_name).await?;

        // Get source object
        let source_obj = source_api.get(source_name).await.with_context(|| {
            format!(
                "Failed to fetch source {}/{} in namespace {}",
                source_kind, source_name, source_namespace
            )
        })?;

        // Check if source is suspended
        if is_resource_suspended(&source_obj) {
            tracing::warn!(
                "Cannot reconcile: source {}/{} in namespace {} is suspended",
                source_kind,
                source_name,
                source_namespace
            );
            return Err(anyhow::anyhow!(
                "Cannot reconcile: source {}/{} in namespace {} is suspended",
                source_kind,
                source_name,
                source_namespace
            ));
        }

        // Get current annotations and add reconcile annotation to source
        let mut source_annotations = get_annotations(&source_obj);
        add_reconcile_annotation(&mut source_annotations);

        // Patch source with reconcile annotation
        let source_patch = json!({
            "metadata": {
                "annotations": source_annotations
            }
        });
        source_api
            .patch(
                source_name,
                &PatchParams::default(),
                &Patch::Merge(source_patch),
            )
            .await
            .with_context(|| {
                format!(
                    "Failed to trigger reconciliation on source {}/{} in namespace {}",
                    source_kind, source_name, source_namespace
                )
            })?;

        tracing::info!(
            "Triggered reconciliation on source {}/{} in namespace {}",
            source_kind,
            source_name,
            source_namespace
        );

        // Step 2: Wait for source reconciliation to complete
        // Poll until lastHandledReconcileAt matches our requestedAt
        // Note: We wait a short time to allow the source to start reconciling,
        // but we don't wait for completion - we proceed after a brief delay
        let mut attempts = 0;
        let max_attempts = 10; // 10 seconds max wait (reduced from 60)
        let mut source_reconciled = false;

        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            attempts += 1;

            let current_source = match source_api.get(source_name).await {
                Ok(obj) => obj,
                Err(e) => {
                    // If we can't fetch the source, log but continue
                    tracing::warn!("Failed to fetch source during polling: {}", e);
                    break;
                }
            };

            let current_requested_at = current_source
                .data
                .get("metadata")
                .and_then(|m| m.get("annotations"))
                .and_then(|a| a.as_object())
                .and_then(|a| a.get("reconcile.fluxcd.io/requestedAt"))
                .and_then(|t| t.as_str());

            let last_handled = current_source
                .data
                .get("status")
                .and_then(|s| s.get("lastHandledReconcileAt"))
                .and_then(|t| t.as_str());

            // Check if source is ready
            let is_ready = current_source
                .data
                .get("status")
                .and_then(|s| s.get("conditions"))
                .and_then(|c| c.as_array())
                .and_then(|c| {
                    c.iter()
                        .find(|cond| {
                            cond.get("type")
                                .and_then(|t| t.as_str())
                                .map(|t| t == "Ready")
                                .unwrap_or(false)
                        })
                        .and_then(|cond| cond.get("status").and_then(|st| st.as_str()))
                        .map(|st| st == "True")
                })
                .unwrap_or(false);

            // Check if lastHandledReconcileAt matches requestedAt
            // We check if the requestedAt annotation exists and if lastHandled matches it
            if let Some(requested_at) = current_requested_at {
                if let Some(handled_at) = last_handled {
                    // Compare timestamps - they should match if reconciliation completed
                    if handled_at == requested_at {
                        source_reconciled = true;
                        // Also check if ready, but don't require it if reconciliation completed
                        if is_ready {
                            break;
                        } else {
                            // Source reconciled but not ready - wait a bit more
                            if attempts >= 5 {
                                // Give up waiting for ready state after 5 seconds
                                tracing::info!(
                                    "Source {} reconciled but not ready, proceeding anyway",
                                    source_kind
                                );
                                break;
                            }
                        }
                    }
                }
            }

            // If we've waited long enough, proceed anyway
            // The source reconciliation might take longer, but we'll reconcile the resource
            if attempts >= max_attempts {
                if source_reconciled {
                    tracing::info!(
                        "Source {} reconciliation in progress, proceeding with resource reconciliation",
                        source_kind
                    );
                } else {
                    tracing::warn!(
                        "Timeout waiting for source {} reconciliation, proceeding anyway",
                        source_kind
                    );
                }
                break;
            }
        }

        // Step 3: Reconcile the Kustomization/HelmRelease
        // Get fresh copy of the resource to ensure we have latest annotations
        let current_obj = api.get(name).await.with_context(|| {
            format!(
                "Failed to refetch {}/{} in namespace {}",
                resource_type, name, namespace
            )
        })?;

        // Get current annotations and add reconcile annotation
        let mut annotations = get_annotations(&current_obj);
        add_reconcile_annotation(&mut annotations);

        let resource_patch = json!({
            "metadata": {
                "annotations": annotations
            }
        });

        api.patch(name, &PatchParams::default(), &Patch::Merge(resource_patch))
            .await
            .with_context(|| {
                format!(
                    "Failed to reconcile {}/{} in namespace {}",
                    resource_type, name, namespace
                )
            })?;

        tracing::info!(
            "Successfully triggered reconciliation for {}/{} with source in namespace {}",
            resource_type,
            name,
            namespace
        );
        Ok(())
    }

    fn keybinding(&self) -> char {
        'W' // Use 'W' for reconcile With source
    }

    fn requires_confirmation(&self) -> bool {
        false
    }

    fn confirmation_message(&self, resource: &ResourceInfo) -> String {
        format!(
            "Reconcile {} {} with source in {}?",
            resource.resource_type, resource.name, resource.namespace
        )
    }

    fn name(&self) -> &'static str {
        "Reconcile with Source"
    }

    fn is_valid_for(&self, resource_type: &str) -> bool {
        use crate::models::FluxResourceKind;
        matches!(
            FluxResourceKind::parse_optional(resource_type),
            Some(FluxResourceKind::Kustomization) | Some(FluxResourceKind::HelmRelease)
        )
    }
}

/// Operation registry - holds all available operations
pub struct OperationRegistry {
    operations: Vec<Box<dyn FluxOperation>>,
}

impl OperationRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            operations: Vec::new(),
        };

        // Register all operations
        registry.register(Box::new(SuspendOperation));
        registry.register(Box::new(ResumeOperation));
        registry.register(Box::new(DeleteOperation));
        registry.register(Box::new(ReconcileOperation));
        registry.register(Box::new(ReconcileWithSourceOperation));

        registry
    }

    pub fn register(&mut self, operation: Box<dyn FluxOperation>) {
        self.operations.push(operation);
    }

    pub fn get_by_keybinding(&self, key: char) -> Option<&dyn FluxOperation> {
        self.operations
            .iter()
            .find(|op| op.keybinding() == key)
            .map(|op| op.as_ref())
    }

    /// Get all registered operations
    /// Currently only used in tests
    #[allow(dead_code)] // Used in tests
    pub fn get_all(&self) -> &[Box<dyn FluxOperation>] {
        &self.operations
    }
}

impl Default for OperationRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watcher::ResourceInfo;

    #[test]
    fn test_suspend_operation_properties() {
        use crate::models::FluxResourceKind;
        let op = SuspendOperation;

        assert_eq!(op.keybinding(), 's');
        assert_eq!(op.name(), "Suspend");
        assert!(!op.requires_confirmation());

        let resource = ResourceInfo {
            name: "test-ks".to_string(),
            namespace: "default".to_string(),
            resource_type: FluxResourceKind::Kustomization.as_str().to_string(),
            age: None,
            suspended: None,
            ready: None,
            message: None,
            revision: None,
            labels: HashMap::new(),
            annotations: HashMap::new(),
        };

        let msg = op.confirmation_message(&resource);
        assert!(msg.contains("Suspend"));
        assert!(msg.contains("test-ks"));
        assert!(msg.contains("default"));
    }

    #[test]
    fn test_resume_operation_properties() {
        let op = ResumeOperation;

        assert_eq!(op.keybinding(), 'r');
        assert_eq!(op.name(), "Resume");
        assert!(!op.requires_confirmation());
    }

    #[test]
    fn test_delete_operation_properties() {
        use crate::models::FluxResourceKind;
        let op = DeleteOperation;

        assert_eq!(op.keybinding(), 'd');
        assert_eq!(op.name(), "Delete");
        assert!(op.requires_confirmation());

        let resource = ResourceInfo {
            name: "test-resource".to_string(),
            namespace: "flux-system".to_string(),
            resource_type: FluxResourceKind::Kustomization.as_str().to_string(),
            age: None,
            suspended: None,
            ready: None,
            message: None,
            revision: None,
            labels: HashMap::new(),
            annotations: HashMap::new(),
        };

        let msg = op.confirmation_message(&resource);
        assert!(msg.contains("Delete"));
        assert!(msg.contains("test-resource"));
        assert!(msg.contains("flux-system"));
    }

    #[test]
    fn test_reconcile_operation_properties() {
        let op = ReconcileOperation;

        assert_eq!(op.keybinding(), 'R');
        assert_eq!(op.name(), "Reconcile");
        assert!(!op.requires_confirmation());
    }

    #[test]
    fn test_operation_is_valid_for() {
        let suspend = SuspendOperation;
        let delete = DeleteOperation;
        let reconcile = ReconcileOperation;

        // Suspend should work for suspendable resources
        use crate::models::FluxResourceKind;
        assert!(suspend.is_valid_for(FluxResourceKind::Kustomization.as_str()));
        assert!(suspend.is_valid_for(FluxResourceKind::GitRepository.as_str()));
        assert!(suspend.is_valid_for(FluxResourceKind::HelmRelease.as_str()));

        // Delete should work for all resources
        assert!(delete.is_valid_for(FluxResourceKind::Kustomization.as_str()));
        assert!(delete.is_valid_for(FluxResourceKind::GitRepository.as_str()));
        assert!(delete.is_valid_for(FluxResourceKind::HelmRelease.as_str()));
        assert!(delete.is_valid_for(FluxResourceKind::Alert.as_str()));

        // Reconcile should work for all resources
        assert!(reconcile.is_valid_for(FluxResourceKind::Kustomization.as_str()));
        assert!(reconcile.is_valid_for(FluxResourceKind::GitRepository.as_str()));
        assert!(reconcile.is_valid_for(FluxResourceKind::HelmRelease.as_str()));
    }

    #[test]
    fn test_reconcile_with_source_operation_properties() {
        let op = ReconcileWithSourceOperation;

        assert_eq!(op.keybinding(), 'W');
        assert_eq!(op.name(), "Reconcile with Source");
        assert!(!op.requires_confirmation());
    }

    #[test]
    fn test_reconcile_with_source_is_valid_for() {
        let op = ReconcileWithSourceOperation;

        // Should only work for Kustomization and HelmRelease
        assert!(op.is_valid_for(FluxResourceKind::Kustomization.as_str()));
        assert!(op.is_valid_for(FluxResourceKind::HelmRelease.as_str()));

        // Should not work for other resources
        use crate::models::FluxResourceKind;
        assert!(!op.is_valid_for(FluxResourceKind::GitRepository.as_str()));
        assert!(!op.is_valid_for(FluxResourceKind::HelmChart.as_str()));
        assert!(!op.is_valid_for(FluxResourceKind::HelmRepository.as_str()));
        assert!(!op.is_valid_for(FluxResourceKind::OCIRepository.as_str()));
    }

    #[test]
    fn test_reconcile_with_source_confirmation_message() {
        use crate::models::FluxResourceKind;
        let op = ReconcileWithSourceOperation;

        let resource = ResourceInfo {
            name: "test-kustomization".to_string(),
            namespace: "flux-system".to_string(),
            resource_type: FluxResourceKind::Kustomization.as_str().to_string(),
            age: None,
            suspended: None,
            ready: None,
            message: None,
            revision: None,
            labels: HashMap::new(),
            annotations: HashMap::new(),
        };

        let msg = op.confirmation_message(&resource);
        assert!(msg.contains("Reconcile"));
        assert!(msg.contains("test-kustomization"));
        assert!(msg.contains("flux-system"));
        assert!(msg.contains("source"));
    }

    #[test]
    fn test_operation_registry() {
        let registry = OperationRegistry::new();

        // Test getting operations by keybinding
        assert!(registry.get_by_keybinding('s').is_some());
        assert!(registry.get_by_keybinding('r').is_some());
        assert!(registry.get_by_keybinding('d').is_some());
        assert!(registry.get_by_keybinding('R').is_some());
        assert!(registry.get_by_keybinding('W').is_some());

        // Test invalid keybinding
        assert!(registry.get_by_keybinding('x').is_none());

        // Test that we get the right operation
        let suspend = registry.get_by_keybinding('s').unwrap();
        assert_eq!(suspend.name(), "Suspend");

        let delete = registry.get_by_keybinding('d').unwrap();
        assert_eq!(delete.name(), "Delete");
        assert!(delete.requires_confirmation());

        let reconcile_with_source = registry.get_by_keybinding('W').unwrap();
        assert_eq!(reconcile_with_source.name(), "Reconcile with Source");
        assert!(!reconcile_with_source.requires_confirmation());
    }
}
