//! Resource state management
//!
//! Tracks the current state of watched resources for display in the TUI.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Resource metadata for display
#[derive(Debug, Clone)]
pub struct ResourceInfo {
    pub name: String,
    pub namespace: String,
    pub resource_type: String,
    #[allow(dead_code)] // Set but not yet displayed - reserved for future age display feature
    pub age: Option<chrono::DateTime<chrono::Utc>>,
    // Common status fields across Flux CRDs
    pub suspended: Option<bool>,
    pub ready: Option<bool>,
    pub message: Option<String>,
    pub revision: Option<String>,
}

/// Thread-safe resource state store
#[derive(Clone)]
pub struct ResourceState {
    inner: Arc<RwLock<HashMap<String, ResourceInfo>>>,
}

impl ResourceState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add or update a resource
    pub fn upsert(&self, key: String, info: ResourceInfo) {
        let mut state = self.inner.write().unwrap();
        state.insert(key, info);
    }

    /// Remove a resource
    pub fn remove(&self, key: &str) {
        let mut state = self.inner.write().unwrap();
        state.remove(key);
    }

    /// Get all resources
    pub fn all(&self) -> Vec<ResourceInfo> {
        let state = self.inner.read().unwrap();
        state.values().cloned().collect()
    }

    /// Get resources by type
    pub fn by_type(&self, resource_type: &str) -> Vec<ResourceInfo> {
        let state = self.inner.read().unwrap();
        state
            .values()
            .filter(|info| info.resource_type == resource_type)
            .cloned()
            .collect()
    }

    /// Get a specific resource
    pub fn get(&self, key: &str) -> Option<ResourceInfo> {
        let state = self.inner.read().unwrap();
        state.get(key).cloned()
    }

    /// Count resources by type
    pub fn count_by_type(&self) -> HashMap<String, usize> {
        let state = self.inner.read().unwrap();
        let mut counts = HashMap::new();
        for info in state.values() {
            *counts.entry(info.resource_type.clone()).or_insert(0) += 1;
        }
        counts
    }

    /// Clear all resources (useful when switching namespaces)
    pub fn clear(&self) {
        let mut state = self.inner.write().unwrap();
        state.clear();
    }
}

impl Default for ResourceState {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate a unique key for a resource
pub fn resource_key(namespace: &str, name: &str, resource_type: &str) -> String {
    format!("{}:{}:{}", resource_type, namespace, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_key_generation() {
        let key = resource_key("default", "my-resource", "Kustomization");
        assert_eq!(key, "Kustomization:default:my-resource");

        let key2 = resource_key("flux-system", "flux-system", "GitRepository");
        assert_eq!(key2, "GitRepository:flux-system:flux-system");
    }

    #[test]
    fn test_resource_state_new() {
        let state = ResourceState::new();
        assert_eq!(state.all().len(), 0);
    }

    #[test]
    fn test_resource_state_upsert() {
        let state = ResourceState::new();
        let info = ResourceInfo {
            name: "test-resource".to_string(),
            namespace: "default".to_string(),
            resource_type: "Kustomization".to_string(),
            age: None,
            suspended: Some(false),
            ready: Some(true),
            message: Some("Ready".to_string()),
            revision: None,
        };

        let key = resource_key("default", "test-resource", "Kustomization");
        state.upsert(key.clone(), info);

        assert_eq!(state.all().len(), 1);
        let retrieved = state.get(&key).unwrap();
        assert_eq!(retrieved.name, "test-resource");
        assert_eq!(retrieved.namespace, "default");
        assert_eq!(retrieved.resource_type, "Kustomization");
    }

    #[test]
    fn test_resource_state_remove() {
        let state = ResourceState::new();
        let info = ResourceInfo {
            name: "test-resource".to_string(),
            namespace: "default".to_string(),
            resource_type: "Kustomization".to_string(),
            age: None,
            suspended: None,
            ready: None,
            message: None,
            revision: None,
        };

        let key = resource_key("default", "test-resource", "Kustomization");
        state.upsert(key.clone(), info);
        assert_eq!(state.all().len(), 1);

        state.remove(&key);
        assert_eq!(state.all().len(), 0);
        assert!(state.get(&key).is_none());
    }

    #[test]
    fn test_resource_state_by_type() {
        let state = ResourceState::new();

        // Add multiple resources of different types
        let kustomization = ResourceInfo {
            name: "ks1".to_string(),
            namespace: "default".to_string(),
            resource_type: "Kustomization".to_string(),
            age: None,
            suspended: None,
            ready: None,
            message: None,
            revision: None,
        };

        let gitrepo = ResourceInfo {
            name: "repo1".to_string(),
            namespace: "default".to_string(),
            resource_type: "GitRepository".to_string(),
            age: None,
            suspended: None,
            ready: None,
            message: None,
            revision: None,
        };

        let kustomization2 = ResourceInfo {
            name: "ks2".to_string(),
            namespace: "default".to_string(),
            resource_type: "Kustomization".to_string(),
            age: None,
            suspended: None,
            ready: None,
            message: None,
            revision: None,
        };

        state.upsert(
            resource_key("default", "ks1", "Kustomization"),
            kustomization,
        );
        state.upsert(resource_key("default", "repo1", "GitRepository"), gitrepo);
        state.upsert(
            resource_key("default", "ks2", "Kustomization"),
            kustomization2,
        );

        let kustomizations = state.by_type("Kustomization");
        assert_eq!(kustomizations.len(), 2);

        let gitrepos = state.by_type("GitRepository");
        assert_eq!(gitrepos.len(), 1);

        let nonexistent = state.by_type("HelmRelease");
        assert_eq!(nonexistent.len(), 0);
    }

    #[test]
    fn test_resource_state_count_by_type() {
        let state = ResourceState::new();

        // Add resources of different types
        state.upsert(
            resource_key("default", "ks1", "Kustomization"),
            ResourceInfo {
                name: "ks1".to_string(),
                namespace: "default".to_string(),
                resource_type: "Kustomization".to_string(),
                age: None,
                suspended: None,
                ready: None,
                message: None,
                revision: None,
            },
        );

        state.upsert(
            resource_key("default", "ks2", "Kustomization"),
            ResourceInfo {
                name: "ks2".to_string(),
                namespace: "default".to_string(),
                resource_type: "Kustomization".to_string(),
                age: None,
                suspended: None,
                ready: None,
                message: None,
                revision: None,
            },
        );

        state.upsert(
            resource_key("default", "repo1", "GitRepository"),
            ResourceInfo {
                name: "repo1".to_string(),
                namespace: "default".to_string(),
                resource_type: "GitRepository".to_string(),
                age: None,
                suspended: None,
                ready: None,
                message: None,
                revision: None,
            },
        );

        let counts = state.count_by_type();
        assert_eq!(counts.get("Kustomization"), Some(&2));
        assert_eq!(counts.get("GitRepository"), Some(&1));
        assert_eq!(counts.get("HelmRelease"), None);
    }

    #[test]
    fn test_resource_state_clear() {
        let state = ResourceState::new();

        state.upsert(
            resource_key("default", "test", "Kustomization"),
            ResourceInfo {
                name: "test".to_string(),
                namespace: "default".to_string(),
                resource_type: "Kustomization".to_string(),
                age: None,
                suspended: None,
                ready: None,
                message: None,
                revision: None,
            },
        );

        assert_eq!(state.all().len(), 1);
        state.clear();
        assert_eq!(state.all().len(), 0);
    }
}
