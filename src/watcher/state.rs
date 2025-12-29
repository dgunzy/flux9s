//! Resource state management
//!
//! Tracks the current state of watched resources for display in the TUI.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, RwLock};

/// A type-safe resource key that identifies a Flux resource.
///
/// Format: `resource_type:namespace:name`
///
/// This type provides safe parsing and construction of resource keys,
/// avoiding fragile string splitting operations.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResourceKey {
    pub resource_type: String,
    pub namespace: String,
    pub name: String,
}

impl ResourceKey {
    /// Create a new ResourceKey from its components
    pub fn new(
        resource_type: impl Into<String>,
        namespace: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        Self {
            resource_type: resource_type.into(),
            namespace: namespace.into(),
            name: name.into(),
        }
    }

    /// Parse a resource key string in the format defined by RESOURCE_KEY_FORMAT
    ///
    /// Returns None if the string doesn't have exactly 3 colon-separated parts.
    pub fn parse(key: &str) -> Option<Self> {
        let parts: Vec<&str> = key.split(':').collect();
        if parts.len() == 3 {
            Some(Self {
                resource_type: parts[0].to_string(),
                namespace: parts[1].to_string(),
                name: parts[2].to_string(),
            })
        } else {
            tracing::warn!(
                "Failed to parse resource key '{}': expected format '{}'",
                key,
                crate::tui::constants::RESOURCE_KEY_FORMAT
            );
            None
        }
    }

    /// Convert the ResourceKey back to its string representation (unused so far)
    #[allow(dead_code)] // Used in tests
    pub fn to_key_string(&self) -> String {
        format!("{}:{}:{}", self.resource_type, self.namespace, self.name)
    }
}

impl fmt::Display for ResourceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}/{} ({})",
            self.namespace, self.name, self.resource_type
        )
    }
}

/// Reconciliation event tracking
#[derive(Debug, Clone)]
pub struct ReconciliationEvent {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    #[allow(dead_code)] // Reserved for future use
    pub revision: Option<String>,
    #[allow(dead_code)] // Reserved for future use
    pub status: String, // "Success", "Failed", "Unknown"
    #[allow(dead_code)] // Reserved for future use
    pub message: Option<String>,
}

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
    // Cached metadata for filtering
    pub labels: HashMap<String, String>,
    pub annotations: HashMap<String, String>,
    // Reconciliation tracking
    pub last_reconciled: Option<chrono::DateTime<chrono::Utc>>,
    pub reconciliation_history: Vec<ReconciliationEvent>, // Limited to last N events
}

/// Extract labels from a Kubernetes resource JSON object
pub fn extract_labels(obj: &serde_json::Value) -> HashMap<String, String> {
    obj.get("metadata")
        .and_then(|m| m.get("labels"))
        .and_then(|l| l.as_object())
        .map(|labels| {
            labels
                .iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

/// Extract annotations from a Kubernetes resource JSON object
pub fn extract_annotations(obj: &serde_json::Value) -> HashMap<String, String> {
    obj.get("metadata")
        .and_then(|m| m.get("annotations"))
        .and_then(|a| a.as_object())
        .map(|annotations| {
            annotations
                .iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

/// Thread-safe resource state store
#[derive(Clone, Debug)]
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
    fn test_resource_key_new() {
        let key = ResourceKey::new("Kustomization", "default", "my-resource");
        assert_eq!(key.resource_type, "Kustomization");
        assert_eq!(key.namespace, "default");
        assert_eq!(key.name, "my-resource");
    }

    #[test]
    fn test_resource_key_parse_valid() {
        let key = ResourceKey::parse("Kustomization:default:my-resource").unwrap();
        assert_eq!(key.resource_type, "Kustomization");
        assert_eq!(key.namespace, "default");
        assert_eq!(key.name, "my-resource");
    }

    #[test]
    fn test_resource_key_parse_invalid() {
        assert!(ResourceKey::parse("invalid").is_none());
        assert!(ResourceKey::parse("only:two").is_none());
        assert!(ResourceKey::parse("too:many:parts:here").is_none());
        assert!(ResourceKey::parse("").is_none());
    }

    #[test]
    fn test_resource_key_to_string() {
        let key = ResourceKey::new("GitRepository", "flux-system", "flux-system");
        assert_eq!(key.to_key_string(), "GitRepository:flux-system:flux-system");
    }

    #[test]
    fn test_resource_key_display() {
        let key = ResourceKey::new("Kustomization", "default", "my-app");
        assert_eq!(format!("{}", key), "default/my-app (Kustomization)");
    }

    #[test]
    fn test_resource_key_roundtrip() {
        let original = ResourceKey::new("HelmRelease", "production", "nginx");
        let key_string = original.to_key_string();
        let parsed = ResourceKey::parse(&key_string).unwrap();
        assert_eq!(original, parsed);
    }

    #[test]
    fn test_resource_key_generation() {
        use crate::models::FluxResourceKind;
        let key = resource_key(
            "default",
            "my-resource",
            FluxResourceKind::Kustomization.as_str(),
        );
        assert_eq!(key, "Kustomization:default:my-resource");

        let key2 = resource_key(
            "flux-system",
            "flux-system",
            FluxResourceKind::GitRepository.as_str(),
        );
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
            resource_type: FluxResourceKind::Kustomization.as_str().to_string(),
            age: None,
            suspended: Some(false),
            ready: Some(true),
            message: Some("Ready".to_string()),
            revision: None,
            labels: HashMap::new(),
            annotations: HashMap::new(),
            last_reconciled: None,
            reconciliation_history: Vec::new(),
        };

        use crate::models::FluxResourceKind;
        let key = resource_key(
            "default",
            "test-resource",
            FluxResourceKind::Kustomization.as_str(),
        );
        state.upsert(key.clone(), info);

        assert_eq!(state.all().len(), 1);
        let retrieved = state.get(&key).unwrap();
        assert_eq!(retrieved.name, "test-resource");
        assert_eq!(retrieved.namespace, "default");
        assert_eq!(
            retrieved.resource_type,
            FluxResourceKind::Kustomization.as_str()
        );
    }

    #[test]
    fn test_resource_state_remove() {
        use crate::models::FluxResourceKind;
        let state = ResourceState::new();
        let info = ResourceInfo {
            name: "test-resource".to_string(),
            namespace: "default".to_string(),
            resource_type: FluxResourceKind::Kustomization.as_str().to_string(),
            age: None,
            suspended: None,
            ready: None,
            message: None,
            revision: None,
            labels: HashMap::new(),
            annotations: HashMap::new(),
            last_reconciled: None,
            reconciliation_history: Vec::new(),
        };
        let key = resource_key(
            "default",
            "test-resource",
            FluxResourceKind::Kustomization.as_str(),
        );
        state.upsert(key.clone(), info);
        assert_eq!(state.all().len(), 1);

        state.remove(&key);
        assert_eq!(state.all().len(), 0);
        assert!(state.get(&key).is_none());
    }

    #[test]
    fn test_resource_state_by_type() {
        use crate::models::FluxResourceKind;
        let state = ResourceState::new();

        // Add multiple resources of different types
        let kustomization = ResourceInfo {
            name: "ks1".to_string(),
            namespace: "default".to_string(),
            resource_type: FluxResourceKind::Kustomization.as_str().to_string(),
            age: None,
            suspended: None,
            ready: None,
            message: None,
            revision: None,
            labels: HashMap::new(),
            annotations: HashMap::new(),
            last_reconciled: None,
            reconciliation_history: Vec::new(),
        };

        let gitrepo = ResourceInfo {
            name: "repo1".to_string(),
            namespace: "default".to_string(),
            resource_type: FluxResourceKind::GitRepository.as_str().to_string(),
            age: None,
            suspended: None,
            ready: None,
            message: None,
            revision: None,
            labels: HashMap::new(),
            annotations: HashMap::new(),
            last_reconciled: None,
            reconciliation_history: Vec::new(),
        };

        let kustomization2 = ResourceInfo {
            name: "ks2".to_string(),
            namespace: "default".to_string(),
            resource_type: FluxResourceKind::Kustomization.as_str().to_string(),
            age: None,
            suspended: None,
            ready: None,
            message: None,
            revision: None,
            labels: HashMap::new(),
            annotations: HashMap::new(),
            last_reconciled: None,
            reconciliation_history: Vec::new(),
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

        let kustomizations = state.by_type(FluxResourceKind::Kustomization.as_str());
        assert_eq!(kustomizations.len(), 2);

        let gitrepos = state.by_type(FluxResourceKind::GitRepository.as_str());
        assert_eq!(gitrepos.len(), 1);

        let nonexistent = state.by_type(FluxResourceKind::HelmRelease.as_str());
        assert_eq!(nonexistent.len(), 0);
    }

    #[test]
    fn test_resource_state_count_by_type() {
        let state = ResourceState::new();

        use crate::models::FluxResourceKind;
        // Add resources of different types
        state.upsert(
            resource_key("default", "ks1", FluxResourceKind::Kustomization.as_str()),
            ResourceInfo {
                name: "ks1".to_string(),
                namespace: "default".to_string(),
                resource_type: FluxResourceKind::Kustomization.as_str().to_string(),
                age: None,
                suspended: None,
                ready: None,
                message: None,
                revision: None,
                labels: HashMap::new(),
                annotations: HashMap::new(),
                last_reconciled: None,
                reconciliation_history: Vec::new(),
            },
        );

        state.upsert(
            resource_key("default", "ks2", FluxResourceKind::Kustomization.as_str()),
            ResourceInfo {
                name: "ks2".to_string(),
                namespace: "default".to_string(),
                resource_type: FluxResourceKind::Kustomization.as_str().to_string(),
                age: None,
                suspended: None,
                ready: None,
                message: None,
                revision: None,
                labels: HashMap::new(),
                annotations: HashMap::new(),
                last_reconciled: None,
                reconciliation_history: Vec::new(),
            },
        );

        state.upsert(
            resource_key("default", "repo1", FluxResourceKind::GitRepository.as_str()),
            ResourceInfo {
                name: "repo1".to_string(),
                namespace: "default".to_string(),
                resource_type: FluxResourceKind::GitRepository.as_str().to_string(),
                age: None,
                suspended: None,
                ready: None,
                message: None,
                revision: None,
                labels: HashMap::new(),
                annotations: HashMap::new(),
                last_reconciled: None,
                reconciliation_history: Vec::new(),
            },
        );

        let counts = state.count_by_type();
        assert_eq!(
            counts.get(FluxResourceKind::Kustomization.as_str()),
            Some(&2)
        );
        assert_eq!(
            counts.get(FluxResourceKind::GitRepository.as_str()),
            Some(&1)
        );
        assert_eq!(counts.get(FluxResourceKind::HelmRelease.as_str()), None);
    }

    #[test]
    fn test_resource_state_clear() {
        let state = ResourceState::new();

        use crate::models::FluxResourceKind;
        state.upsert(
            resource_key("default", "test", FluxResourceKind::Kustomization.as_str()),
            ResourceInfo {
                name: "test".to_string(),
                namespace: "default".to_string(),
                resource_type: FluxResourceKind::Kustomization.as_str().to_string(),
                age: None,
                suspended: None,
                ready: None,
                message: None,
                revision: None,
                labels: HashMap::new(),
                annotations: HashMap::new(),
                last_reconciled: None,
                reconciliation_history: Vec::new(),
            },
        );

        assert_eq!(state.all().len(), 1);
        state.clear();
        assert_eq!(state.all().len(), 0);
    }
}
