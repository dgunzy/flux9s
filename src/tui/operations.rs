//! Flux operations module
//!
//! Provides extensible system for performing Flux operations on resources.
//! Operations are implemented as a trait-based system for easy extension.

use crate::watcher::ResourceInfo;
use anyhow::Result;
use kube::Api;
use serde_json::json;

use crate::tui::api::get_api_resource_with_fallback;

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
        use kube::api::{Patch, PatchParams};
        use kube::core::DynamicObject;
        use kube::Api;

        // Get ApiResource with version fallback (version-agnostic)
        let api_resource =
            get_api_resource_with_fallback(client, resource_type, namespace, name).await?;
        let api: Api<DynamicObject> =
            Api::namespaced_with(client.clone(), namespace, &api_resource);

        // Patch spec.suspend to true
        let patch = json!({
            "spec": {
                "suspend": true
            }
        });

        // Use Merge patch without force (force only works with Patch::Apply)
        let patch_params = PatchParams::default();
        api.patch(name, &patch_params, &Patch::Merge(patch)).await?;

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
        use kube::api::{Patch, PatchParams};
        use kube::core::DynamicObject;
        use kube::Api;

        // Get ApiResource with version fallback (version-agnostic)
        let api_resource =
            get_api_resource_with_fallback(client, resource_type, namespace, name).await?;
        let api: Api<DynamicObject> =
            Api::namespaced_with(client.clone(), namespace, &api_resource);

        // Patch spec.suspend to false
        let patch = json!({
            "spec": {
                "suspend": false
            }
        });

        // Use Merge patch without force (force only works with Patch::Apply)
        let patch_params = PatchParams::default();
        api.patch(name, &patch_params, &Patch::Merge(patch)).await?;

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
        use kube::core::DynamicObject;
        use kube::Api;

        // Get ApiResource with version fallback (version-agnostic)
        let api_resource =
            get_api_resource_with_fallback(client, resource_type, namespace, name).await?;
        let api: Api<DynamicObject> =
            Api::namespaced_with(client.clone(), namespace, &api_resource);

        // First, verify the resource exists (like Flux does)
        let _obj = api
            .get(name)
            .await
            .map_err(|e| anyhow::anyhow!("Resource not found: {}", e))?;

        // Then delete it
        api.delete(name, &DeleteParams::default()).await?;

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
        use kube::api::{Patch, PatchParams};
        use kube::core::DynamicObject;
        use kube::Api;

        // Get ApiResource with version fallback (version-agnostic)
        let api_resource =
            get_api_resource_with_fallback(client, resource_type, namespace, name).await?;
        let api: Api<DynamicObject> =
            Api::namespaced_with(client.clone(), namespace, &api_resource);

        // First, get the resource to verify it exists and get current state
        let obj = api
            .get(name)
            .await
            .map_err(|e| anyhow::anyhow!("Resource not found: {}", e))?;

        // Check if resource is suspended (like Flux does)
        if let Some(spec) = obj.data.get("spec").and_then(|s| s.as_object()) {
            if let Some(suspended) = spec.get("suspend").and_then(|s| s.as_bool()) {
                if suspended {
                    return Err(anyhow::anyhow!("Resource is suspended"));
                }
            }
        }

        // Get current annotations or create empty map
        let mut annotations = obj
            .data
            .get("metadata")
            .and_then(|m| m.get("annotations"))
            .and_then(|a| a.as_object())
            .cloned()
            .unwrap_or_else(serde_json::Map::new);

        // Add reconcile annotation with timestamp (RFC3339Nano format like Flux)
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
        annotations.insert("reconcile.fluxcd.io/requestedAt".to_string(), json!(now));

        // Create merge patch for annotations
        let patch = json!({
            "metadata": {
                "annotations": annotations
            }
        });

        // Use Merge patch without force (force only works with Patch::Apply)
        let patch_params = PatchParams::default();
        api.patch(name, &patch_params, &Patch::Merge(patch)).await?;

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
        use kube::api::{Patch, PatchParams};
        use kube::core::DynamicObject;

        use crate::models::FluxResourceKind;
        // Only works for Kustomization and HelmRelease
        let kind = FluxResourceKind::parse_optional(resource_type);
        if !matches!(
            kind,
            Some(FluxResourceKind::Kustomization) | Some(FluxResourceKind::HelmRelease)
        ) {
            return Err(anyhow::anyhow!(
                "Reconcile with source only works for Kustomization and HelmRelease"
            ));
        }

        // Get ApiResource with version fallback (version-agnostic)
        let api_resource =
            get_api_resource_with_fallback(client, resource_type, namespace, name).await?;
        let api: Api<DynamicObject> =
            Api::namespaced_with(client.clone(), namespace, &api_resource);

        // Get the resource to check if it exists and get sourceRef
        let obj = api
            .get(name)
            .await
            .map_err(|e| anyhow::anyhow!("Resource not found: {}", e))?;

        // Check if resource is suspended
        if let Some(spec) = obj.data.get("spec").and_then(|s| s.as_object()) {
            if let Some(suspended) = spec.get("suspend").and_then(|s| s.as_bool()) {
                if suspended {
                    return Err(anyhow::anyhow!("Resource is suspended"));
                }
            }
        }

        // Extract sourceRef
        let source_ref = obj
            .data
            .get("spec")
            .and_then(|s| s.get("sourceRef"))
            .and_then(|sr| sr.as_object())
            .ok_or_else(|| anyhow::anyhow!("Resource has no sourceRef"))?;

        let source_kind = source_ref
            .get("kind")
            .and_then(|k| k.as_str())
            .ok_or_else(|| anyhow::anyhow!("sourceRef missing kind"))?;
        let source_name = source_ref
            .get("name")
            .and_then(|n| n.as_str())
            .ok_or_else(|| anyhow::anyhow!("sourceRef missing name"))?;
        let source_namespace = source_ref
            .get("namespace")
            .and_then(|n| n.as_str())
            .unwrap_or(namespace);

        // Step 1: Reconcile the source first
        // Get ApiResource with version fallback (version-agnostic)
        let source_api_resource =
            get_api_resource_with_fallback(client, source_kind, source_namespace, source_name)
                .await?;
        let source_api: Api<DynamicObject> =
            Api::namespaced_with(client.clone(), source_namespace, &source_api_resource);

        // Get source object
        let source_obj = source_api
            .get(source_name)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch source {}: {}", source_kind, e))?;

        // Check if source is suspended
        if let Some(spec) = source_obj.data.get("spec").and_then(|s| s.as_object()) {
            if let Some(suspended) = spec.get("suspend").and_then(|s| s.as_bool()) {
                if suspended {
                    return Err(anyhow::anyhow!("Source {} is suspended", source_kind));
                }
            }
        }

        // Get current annotations or create empty map
        let mut source_annotations = source_obj
            .data
            .get("metadata")
            .and_then(|m| m.get("annotations"))
            .and_then(|a| a.as_object())
            .cloned()
            .unwrap_or_else(serde_json::Map::new);

        // Add reconcile annotation to source
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
        source_annotations.insert("reconcile.fluxcd.io/requestedAt".to_string(), json!(now));

        // Patch source with reconcile annotation
        let source_patch = json!({
            "metadata": {
                "annotations": source_annotations
            }
        });
        let patch_params = PatchParams::default();
        source_api
            .patch(source_name, &patch_params, &Patch::Merge(source_patch))
            .await?;

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
        let current_obj = api.get(name).await?;
        let mut annotations = current_obj
            .data
            .get("metadata")
            .and_then(|m| m.get("annotations"))
            .and_then(|a| a.as_object())
            .cloned()
            .unwrap_or_else(serde_json::Map::new);

        // Use a new timestamp for the resource reconciliation
        let resource_now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
        annotations.insert(
            "reconcile.fluxcd.io/requestedAt".to_string(),
            json!(resource_now),
        );

        let resource_patch = json!({
            "metadata": {
                "annotations": annotations
            }
        });

        api.patch(name, &patch_params, &Patch::Merge(resource_patch))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to reconcile {}: {}", resource_type, e))?;

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
