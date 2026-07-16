//! Dynamically discovered extra resource kinds (#197).
//!
//! When `discoverFluxResources` is enabled, flux9s watches
//! CustomResourceDefinitions labeled `app.kubernetes.io/part-of=<instance>` —
//! the same label the Flux Operator's FluxReport uses to enumerate
//! reconcilers — and registers their kinds here. Discovered kinds get the
//! generic treatment (watched, listed, filterable, `y`/`d`) and are
//! **view-only**: every richer capability (operations, graph, trace,
//! history) answers through [`crate::models::FluxResourceKind`], which
//! discovered kinds are deliberately not part of, so privileges stay
//! deny-by-default.

use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

use crate::models::FluxResourceKind;

/// The CRD label that marks a kind as part of the Flux instance — the Flux
/// Operator's own convention (`FluxReport` reconciler discovery uses it).
pub const PART_OF_LABEL: &str = "app.kubernetes.io/part-of";

/// The conventional FluxInstance name the label value must match. The
/// operator matches its instance's name; "flux" is the documented default
/// everywhere in the Flux Operator ecosystem.
pub const PART_OF_VALUE: &str = "flux";

/// A discovered kind, reduced to what the dynamic watcher and the kind→GVK
/// resolution need. Plural and short names come from the CRD itself, so
/// `:` command aliases match what `kubectl` accepts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtraKind {
    pub kind: String,
    pub group: String,
    /// The CRD's storage version (last stored version, like the operator).
    pub version: String,
    pub plural: String,
    /// CRD short names (e.g. `tf` for Terraform) — become command aliases.
    pub short_names: Vec<String>,
}

impl ExtraKind {
    /// Build from a CRD, applying the guard rails:
    /// - built-in [`FluxResourceKind`]s are excluded (the FluxInstance labels
    ///   its own CRDs `part-of=<instance>` too — never double-watch them)
    /// - cluster-scoped CRDs are skipped in v1 (the watch and key formats
    ///   assume namespaces)
    /// - a CRD without a stored/storage version is skipped
    pub fn from_crd(crd: &CustomResourceDefinition) -> Option<Self> {
        let spec = &crd.spec;
        let kind = spec.names.kind.clone();

        if FluxResourceKind::parse_optional(&kind).is_some() {
            tracing::debug!("Skipping discovered CRD for built-in kind {}", kind);
            return None;
        }
        if spec.scope != "Namespaced" {
            tracing::info!(
                "Skipping discovered cluster-scoped CRD {} (only namespaced kinds are supported)",
                kind
            );
            return None;
        }

        let version = crd
            .status
            .as_ref()
            .and_then(|s| s.stored_versions.as_ref())
            .and_then(|v| v.last().cloned())
            .or_else(|| {
                spec.versions
                    .iter()
                    .find(|v| v.storage)
                    .map(|v| v.name.clone())
            })?;

        Some(Self {
            kind,
            group: spec.group.clone(),
            version,
            plural: spec.names.plural.clone(),
            short_names: spec.names.short_names.clone().unwrap_or_default(),
        })
    }

    /// `(group, version, plural)` in the shape
    /// [`crate::kube::get_gvk_for_resource_type`] resolves to.
    pub fn gvk(&self) -> (String, String, String) {
        (
            self.group.clone(),
            self.version.clone(),
            self.plural.clone(),
        )
    }
}

/// Shared, dynamic registry of discovered kinds.
///
/// `Arc<RwLock>` is earned here: the CRD watcher task inserts/removes, fetch
/// tasks resolve kinds, and the UI thread lists/filters — all concurrently,
/// and the contents change at runtime as CRDs are labeled or deleted.
#[derive(Debug, Clone, Default)]
pub struct ExtraKindRegistry {
    kinds: Arc<RwLock<HashMap<String, ExtraKind>>>,
}

impl ExtraKindRegistry {
    /// Register a discovered kind. Returns false when it was already present.
    pub fn insert(&self, extra: ExtraKind) -> bool {
        self.kinds
            .write()
            .expect("extra kind registry poisoned")
            .insert(extra.kind.clone(), extra)
            .is_none()
    }

    /// Remove a kind (its CRD was deleted or unlabeled). Returns the removed
    /// entry so the caller can stop its watcher.
    pub fn remove(&self, kind: &str) -> Option<ExtraKind> {
        self.kinds
            .write()
            .expect("extra kind registry poisoned")
            .remove(kind)
    }

    /// Look up a kind by its exact name.
    pub fn get(&self, kind: &str) -> Option<ExtraKind> {
        self.kinds
            .read()
            .expect("extra kind registry poisoned")
            .get(kind)
            .cloned()
    }

    /// Resolve a `:` command token (kind, plural, or short name — case
    /// insensitive) to the kind name.
    pub fn resolve_command(&self, token: &str) -> Option<String> {
        let token = token.to_lowercase();
        self.kinds
            .read()
            .expect("extra kind registry poisoned")
            .values()
            .find(|extra| {
                extra.kind.to_lowercase() == token
                    || extra.plural.to_lowercase() == token
                    || extra
                        .short_names
                        .iter()
                        .any(|short| short.to_lowercase() == token)
            })
            .map(|extra| extra.kind.clone())
    }

    /// All registered kind names, sorted.
    pub fn kind_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .kinds
            .read()
            .expect("extra kind registry poisoned")
            .keys()
            .cloned()
            .collect();
        names.sort();
        names
    }

    pub fn is_empty(&self) -> bool {
        self.kinds
            .read()
            .expect("extra kind registry poisoned")
            .is_empty()
    }

    /// Drop every discovered kind. Called when the discovery watcher stops
    /// (context switch, namespace restart) so kinds from one cluster never
    /// leak into another; re-discovery repopulates the registry.
    pub fn clear(&self) {
        self.kinds
            .write()
            .expect("extra kind registry poisoned")
            .clear();
    }
}

/// The process-wide registry. Always present but empty unless discovery is
/// enabled and CRDs are found — an empty registry means every lookup misses,
/// which is exactly the disabled behavior.
pub fn global() -> &'static ExtraKindRegistry {
    static REGISTRY: OnceLock<ExtraKindRegistry> = OnceLock::new();
    REGISTRY.get_or_init(ExtraKindRegistry::default)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crd(kind: &str, scope: &str, stored: Option<&str>) -> CustomResourceDefinition {
        let crd_json = serde_json::json!({
            "apiVersion": "apiextensions.k8s.io/v1",
            "kind": "CustomResourceDefinition",
            "metadata": {"name": format!("{}s.example.com", kind.to_lowercase())},
            "spec": {
                "group": "example.com",
                "scope": scope,
                "names": {
                    "kind": kind,
                    "plural": format!("{}s", kind.to_lowercase()),
                    "shortNames": ["wd"]
                },
                "versions": [
                    {"name": "v1alpha1", "served": true, "storage": false, "schema": {"openAPIV3Schema": {"type": "object"}}},
                    {"name": "v1", "served": true, "storage": true, "schema": {"openAPIV3Schema": {"type": "object"}}}
                ]
            },
            "status": stored.map(|v| serde_json::json!({
                "storedVersions": ["v1alpha1", v],
                "acceptedNames": {"kind": kind, "plural": format!("{}s", kind.to_lowercase())},
                "conditions": []
            })).unwrap_or(serde_json::json!(null))
        });
        serde_json::from_value(crd_json).expect("test CRD should deserialize")
    }

    #[test]
    fn from_crd_extracts_kind_plural_shortnames_and_version() {
        let extra = ExtraKind::from_crd(&crd("Widget", "Namespaced", Some("v1"))).unwrap();
        assert_eq!(extra.kind, "Widget");
        assert_eq!(extra.group, "example.com");
        assert_eq!(extra.plural, "widgets");
        assert_eq!(extra.short_names, ["wd"]);
        assert_eq!(extra.version, "v1", "last stored version wins");
        assert_eq!(
            extra.gvk(),
            (
                "example.com".to_string(),
                "v1".to_string(),
                "widgets".to_string()
            )
        );
    }

    #[test]
    fn from_crd_falls_back_to_storage_version_without_status() {
        let extra = ExtraKind::from_crd(&crd("Widget", "Namespaced", None)).unwrap();
        assert_eq!(extra.version, "v1", "spec storage version fallback");
    }

    #[test]
    fn from_crd_guards_built_ins_and_cluster_scope() {
        // The FluxInstance labels its own CRDs part-of=<instance>, so the
        // built-in exclusion is what prevents double-watching Kustomizations.
        assert!(ExtraKind::from_crd(&crd("Kustomization", "Namespaced", Some("v1"))).is_none());
        assert!(ExtraKind::from_crd(&crd("Widget", "Cluster", Some("v1"))).is_none());
    }

    #[test]
    fn registry_insert_remove_and_command_resolution() {
        let registry = ExtraKindRegistry::default();
        assert!(registry.is_empty());

        let extra = ExtraKind::from_crd(&crd("Widget", "Namespaced", Some("v1"))).unwrap();
        assert!(registry.insert(extra.clone()));
        assert!(!registry.insert(extra), "re-insert reports already present");

        assert!(registry.get("Widget").is_some());
        assert_eq!(registry.kind_names(), ["Widget"]);
        // Kind, plural, and CRD short name all resolve, case-insensitively
        for token in ["widget", "WIDGETS", "wd"] {
            assert_eq!(
                registry.resolve_command(token).as_deref(),
                Some("Widget"),
                "{token} should resolve"
            );
        }
        assert!(registry.resolve_command("ks").is_none());

        let removed = registry.remove("Widget").unwrap();
        assert_eq!(removed.kind, "Widget");
        assert!(registry.is_empty());
    }

    #[test]
    fn clear_forgets_all_kinds() {
        // Context switches clear the registry via ResourceWatcher::stop() so
        // one cluster's aliases never resolve against another cluster.
        let registry = ExtraKindRegistry::default();
        registry.insert(ExtraKind::from_crd(&crd("Widget", "Namespaced", Some("v1"))).unwrap());
        assert!(!registry.is_empty());

        registry.clear();
        assert!(registry.is_empty());
        assert!(registry.resolve_command("wd").is_none());
    }
}
