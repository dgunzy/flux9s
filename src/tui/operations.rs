//! Flux operations module
//!
//! Provides extensible system for performing Flux operations on resources.
//! Operations are implemented as a trait-based system for easy extension.

use crate::watcher::ResourceInfo;
use anyhow::Result;
use kube::Api;
use serde_json::json;

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
        use kube::core::{ApiResource, DynamicObject};

        // Get GVK for resource type - kind should be the resource type name, not plural
        let (group, version, plural) = get_gvk_for_resource_type(resource_type)?;
        // Construct ApiResource with both GVK and plural name
        let api_resource = ApiResource {
            group: group.clone(),
            version: version.clone(),
            api_version: format!("{}/{}", group, version),
            kind: resource_type.to_string(),
            plural: plural.clone(),
        };
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
        matches!(
            resource_type,
            "GitRepository"
                | "OCIRepository"
                | "HelmRepository"
                | "Kustomization"
                | "HelmRelease"
                | "ImageUpdateAutomation"
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
        use kube::core::{ApiResource, DynamicObject};

        // Get GVK for resource type - kind should be the resource type name, not plural
        let (group, version, plural) = get_gvk_for_resource_type(resource_type)?;
        // Construct ApiResource with both GVK and plural name
        let api_resource = ApiResource {
            group: group.clone(),
            version: version.clone(),
            api_version: format!("{}/{}", group, version),
            kind: resource_type.to_string(),
            plural: plural.clone(),
        };
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
        matches!(
            resource_type,
            "GitRepository"
                | "OCIRepository"
                | "HelmRepository"
                | "Kustomization"
                | "HelmRelease"
                | "ImageUpdateAutomation"
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
        use kube::core::{ApiResource, DynamicObject};

        // Get GVK for resource type - kind should be the resource type name, not plural
        let (group, version, plural) = get_gvk_for_resource_type(resource_type)?;
        // Construct ApiResource with both GVK and plural name
        let api_resource = ApiResource {
            group: group.clone(),
            version: version.clone(),
            api_version: format!("{}/{}", group, version),
            kind: resource_type.to_string(),
            plural: plural.clone(),
        };
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
        use kube::core::{ApiResource, DynamicObject};

        // Get GVK for resource type - kind should be the resource type name, not plural
        let (group, version, plural) = get_gvk_for_resource_type(resource_type)?;
        // Construct ApiResource with both GVK and plural name
        let api_resource = ApiResource {
            group: group.clone(),
            version: version.clone(),
            api_version: format!("{}/{}", group, version),
            kind: resource_type.to_string(),
            plural: plural.clone(),
        };
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

/// Get GroupVersionKind for a resource type
fn get_gvk_for_resource_type(resource_type: &str) -> Result<(String, String, String)> {
    use crate::watcher::WatchableResource;
    use crate::watcher::{
        Alert, Bucket, ExternalArtifact, GitRepository, HelmChart, HelmRelease, HelmRepository,
        ImagePolicy, ImageRepository, ImageUpdateAutomation, Kustomization, OCIRepository,
        Provider, Receiver,
    };

    let (group, version, plural) = match resource_type {
        "GitRepository" => (
            GitRepository::api_group(),
            GitRepository::api_version(),
            GitRepository::plural(),
        ),
        "OCIRepository" => (
            OCIRepository::api_group(),
            OCIRepository::api_version(),
            OCIRepository::plural(),
        ),
        "HelmRepository" => (
            HelmRepository::api_group(),
            HelmRepository::api_version(),
            HelmRepository::plural(),
        ),
        "Bucket" => (Bucket::api_group(), Bucket::api_version(), Bucket::plural()),
        "HelmChart" => (
            HelmChart::api_group(),
            HelmChart::api_version(),
            HelmChart::plural(),
        ),
        "ExternalArtifact" => (
            ExternalArtifact::api_group(),
            ExternalArtifact::api_version(),
            ExternalArtifact::plural(),
        ),
        "Kustomization" => (
            Kustomization::api_group(),
            Kustomization::api_version(),
            Kustomization::plural(),
        ),
        "HelmRelease" => (
            HelmRelease::api_group(),
            HelmRelease::api_version(),
            HelmRelease::plural(),
        ),
        "ImageRepository" => (
            ImageRepository::api_group(),
            ImageRepository::api_version(),
            ImageRepository::plural(),
        ),
        "ImagePolicy" => (
            ImagePolicy::api_group(),
            ImagePolicy::api_version(),
            ImagePolicy::plural(),
        ),
        "ImageUpdateAutomation" => (
            ImageUpdateAutomation::api_group(),
            ImageUpdateAutomation::api_version(),
            ImageUpdateAutomation::plural(),
        ),
        "Alert" => (Alert::api_group(), Alert::api_version(), Alert::plural()),
        "Provider" => (
            Provider::api_group(),
            Provider::api_version(),
            Provider::plural(),
        ),
        "Receiver" => (
            Receiver::api_group(),
            Receiver::api_version(),
            Receiver::plural(),
        ),
        _ => return Err(anyhow::anyhow!("Unknown resource type: {}", resource_type)),
    };

    Ok((group.to_string(), version.to_string(), plural.to_string()))
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

    pub fn get_all(&self) -> &[Box<dyn FluxOperation>] {
        &self.operations
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watcher::ResourceInfo;

    #[test]
    fn test_suspend_operation_properties() {
        let op = SuspendOperation;

        assert_eq!(op.keybinding(), 's');
        assert_eq!(op.name(), "Suspend");
        assert!(!op.requires_confirmation());

        let resource = ResourceInfo {
            name: "test-ks".to_string(),
            namespace: "default".to_string(),
            resource_type: "Kustomization".to_string(),
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
        let op = DeleteOperation;

        assert_eq!(op.keybinding(), 'd');
        assert_eq!(op.name(), "Delete");
        assert!(op.requires_confirmation());

        let resource = ResourceInfo {
            name: "test-resource".to_string(),
            namespace: "flux-system".to_string(),
            resource_type: "Kustomization".to_string(),
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
        assert!(suspend.is_valid_for("Kustomization"));
        assert!(suspend.is_valid_for("GitRepository"));
        assert!(suspend.is_valid_for("HelmRelease"));

        // Delete should work for all resources
        assert!(delete.is_valid_for("Kustomization"));
        assert!(delete.is_valid_for("GitRepository"));
        assert!(delete.is_valid_for("HelmRelease"));
        assert!(delete.is_valid_for("Alert"));

        // Reconcile should work for all resources
        assert!(reconcile.is_valid_for("Kustomization"));
        assert!(reconcile.is_valid_for("GitRepository"));
        assert!(reconcile.is_valid_for("HelmRelease"));
    }

    #[test]
    fn test_operation_registry() {
        let registry = OperationRegistry::new();

        // Test getting operations by keybinding
        assert!(registry.get_by_keybinding('s').is_some());
        assert!(registry.get_by_keybinding('r').is_some());
        assert!(registry.get_by_keybinding('d').is_some());
        assert!(registry.get_by_keybinding('R').is_some());

        // Test invalid keybinding
        assert!(registry.get_by_keybinding('x').is_none());

        // Test that we get the right operation
        let suspend = registry.get_by_keybinding('s').unwrap();
        assert_eq!(suspend.name(), "Suspend");

        let delete = registry.get_by_keybinding('d').unwrap();
        assert_eq!(delete.name(), "Delete");
        assert!(delete.requires_confirmation());
    }
}

impl Default for OperationRegistry {
    fn default() -> Self {
        Self::new()
    }
}
