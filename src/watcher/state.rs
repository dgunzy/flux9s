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

    /// Parse a key in the format defined by RESOURCE_KEY_FORMAT. Kinds and
    /// namespaces never contain `:`, but names can (`system:controller:…`
    /// ClusterRoles), so everything after the second `:` is the name. The
    /// namespace is empty for cluster-scoped objects; kind and name aren't.
    pub fn parse(key: &str) -> Option<Self> {
        let mut parts = key.splitn(3, ':');
        let resource_type = parts.next().filter(|k| !k.is_empty())?;
        let namespace = parts.next()?;
        let name = parts.next().filter(|n| !n.is_empty())?;
        Some(Self::new(resource_type, namespace, name))
    }

    /// Convert the ResourceKey back to its string representation — the format
    /// used by the resource state map and `selected_resource_key`.
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
    pub revision: Option<String>,
    pub status: String, // "Success", "Failed", "Unknown"
    pub message: Option<String>,
}

/// Resource metadata for display
#[derive(Debug, Clone)]
pub struct ResourceInfo {
    pub name: String,
    pub namespace: String,
    pub resource_type: String,
    /// Creation timestamp (from `metadata.creationTimestamp`), shown as the AGE column
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
    /// The API group of a Kubernetes row in a mixed Flux list (#304);
    /// `None` for watched Flux resources. With kind, namespace, and name it
    /// identifies the row — TYPE is display text only.
    pub api_group: Option<String>,
}

impl ResourceInfo {
    /// Effective readiness: unknown (`None`) counts as ready — stateless
    /// kinds are normalized upstream, so `None` means "no signal yet".
    pub fn effective_ready(&self) -> bool {
        self.ready.unwrap_or(true)
    }

    /// Effective suspension: unknown counts as not suspended.
    pub fn effective_suspended(&self) -> bool {
        self.suspended.unwrap_or(false)
    }

    /// The single health classification used by the health filters and the
    /// pulse dashboard: healthy = ready and not suspended.
    pub fn is_healthy(&self) -> bool {
        self.effective_ready() && !self.effective_suspended()
    }
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

/// Extract the creation timestamp from a Kubernetes resource JSON object.
///
/// Reads `metadata.creationTimestamp` (RFC 3339); used for the AGE column.
pub fn extract_creation_timestamp(
    obj: &serde_json::Value,
) -> Option<chrono::DateTime<chrono::Utc>> {
    obj.get("metadata")
        .and_then(|m| m.get("creationTimestamp"))
        .and_then(|v| v.as_str())
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc))
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

    /// Acquire the write lock, recovering from poisoning.
    ///
    /// A poisoned lock means another thread panicked while holding it; the map
    /// itself is still valid (worst case one stale entry), so recovering keeps
    /// the TUI alive instead of cascading the panic.
    fn write_lock(&self) -> std::sync::RwLockWriteGuard<'_, HashMap<String, ResourceInfo>> {
        self.inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Acquire the read lock, recovering from poisoning (see [`Self::write_lock`]).
    fn read_lock(&self) -> std::sync::RwLockReadGuard<'_, HashMap<String, ResourceInfo>> {
        self.inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Add or update a resource
    pub fn upsert(&self, key: String, info: ResourceInfo) {
        self.write_lock().insert(key, info);
    }

    /// Remove a resource
    pub fn remove(&self, key: &str) {
        self.write_lock().remove(key);
    }

    /// Get all resources
    pub fn all(&self) -> Vec<ResourceInfo> {
        self.read_lock().values().cloned().collect()
    }

    /// Get resources by type
    pub fn by_type(&self, resource_type: &str) -> Vec<ResourceInfo> {
        self.read_lock()
            .values()
            .filter(|info| info.resource_type == resource_type)
            .cloned()
            .collect()
    }

    /// Get a specific resource
    pub fn get(&self, key: &str) -> Option<ResourceInfo> {
        self.read_lock().get(key).cloned()
    }

    /// Count resources by type
    pub fn count_by_type(&self) -> HashMap<String, usize> {
        let state = self.read_lock();
        let mut counts = HashMap::new();
        for info in state.values() {
            *counts.entry(info.resource_type.clone()).or_insert(0) += 1;
        }
        counts
    }

    /// Clear all resources (useful when switching namespaces)
    pub fn clear(&self) {
        self.write_lock().clear();
    }
}

impl Default for ResourceState {
    fn default() -> Self {
        Self::new()
    }
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
        assert!(ResourceKey::parse(":ns:name").is_none());
        assert!(ResourceKey::parse("Kind:ns:").is_none());
        assert!(ResourceKey::parse("").is_none());
    }

    #[test]
    fn names_may_contain_colons() {
        let key = ResourceKey::parse("ClusterRole::system:controller:job-controller").unwrap();
        assert_eq!(key.resource_type, "ClusterRole");
        assert_eq!(key.namespace, "");
        assert_eq!(key.name, "system:controller:job-controller");
        assert_eq!(
            ResourceKey::parse(&key.to_key_string()),
            Some(key),
            "round-trips"
        );
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
    fn test_resource_state_new() {
        let state = ResourceState::new();
        assert_eq!(state.all().len(), 0);
    }

    #[test]
    fn test_resource_state_upsert() {
        let state = ResourceState::new();
        let info = ResourceInfo {
            api_group: None,
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
        let key = crate::watcher::ResourceKey::new(
            FluxResourceKind::Kustomization.as_str(),
            "default",
            "test-resource",
        )
        .to_key_string();
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
            api_group: None,
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
        let key = crate::watcher::ResourceKey::new(
            FluxResourceKind::Kustomization.as_str(),
            "default",
            "test-resource",
        )
        .to_key_string();
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
            api_group: None,
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
            api_group: None,
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
            api_group: None,
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
            crate::watcher::ResourceKey::new("Kustomization", "default", "ks1").to_key_string(),
            kustomization,
        );
        state.upsert(
            crate::watcher::ResourceKey::new("GitRepository", "default", "repo1").to_key_string(),
            gitrepo,
        );
        state.upsert(
            crate::watcher::ResourceKey::new("Kustomization", "default", "ks2").to_key_string(),
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
            crate::watcher::ResourceKey::new(
                FluxResourceKind::Kustomization.as_str(),
                "default",
                "ks1",
            )
            .to_key_string(),
            ResourceInfo {
                api_group: None,
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
            crate::watcher::ResourceKey::new(
                FluxResourceKind::Kustomization.as_str(),
                "default",
                "ks2",
            )
            .to_key_string(),
            ResourceInfo {
                api_group: None,
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
            crate::watcher::ResourceKey::new(
                FluxResourceKind::GitRepository.as_str(),
                "default",
                "repo1",
            )
            .to_key_string(),
            ResourceInfo {
                api_group: None,
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
            crate::watcher::ResourceKey::new(
                FluxResourceKind::Kustomization.as_str(),
                "default",
                "test",
            )
            .to_key_string(),
            ResourceInfo {
                api_group: None,
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
