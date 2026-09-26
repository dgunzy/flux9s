//! Arbitrary (non-Flux) Kubernetes objects (#262).
//!
//! Flux resources are resolved from [`crate::models::FluxResourceKind`]
//! metadata; everything a Flux object *manages* — ConfigMaps, Services, CRDs,
//! other operators' custom resources — is resolved here through API discovery
//! from the `apiVersion` + `kind` its inventory entry already records.

use anyhow::Context;
use futures::StreamExt;
use kube::Api;
use kube::core::{DynamicObject, GroupVersionKind};
use kube::discovery::{ApiResource, Scope};
use serde_json::Value;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fmt;

use crate::kube::inventory::InventoryEntry;
use crate::kube::object_status::{ObjectHealth, ObjectStatus, compute_object_status};
use crate::watcher::ResourceKey;

/// Placeholder written over every Secret value before it reaches a view.
pub const REDACTED: &str = "<redacted>";

/// Concurrent GETs issued while computing inventory statuses.
const STATUS_FETCH_CONCURRENCY: usize = 8;

/// A reference to any Kubernetes object.
///
/// `api_version` is `None` for Flux resources, whose group/version come from
/// the Flux model metadata; it is set for native objects, where it is the only
/// way to resolve the kind unambiguously.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ObjectRef {
    pub kind: String,
    /// Empty for cluster-scoped objects.
    pub namespace: String,
    pub name: String,
    pub api_version: Option<String>,
}

impl ObjectRef {
    /// A native object identified by `apiVersion` + `kind`.
    pub fn native(
        api_version: impl Into<String>,
        kind: impl Into<String>,
        namespace: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        Self {
            kind: kind.into(),
            namespace: namespace.into(),
            name: name.into(),
            api_version: Some(api_version.into()),
        }
    }

    /// The `kind:namespace:name` key used for view titles and selection.
    pub fn to_resource_key(&self) -> ResourceKey {
        ResourceKey::new(self.kind.clone(), self.namespace.clone(), self.name.clone())
    }
}

impl From<ResourceKey> for ObjectRef {
    fn from(rk: ResourceKey) -> Self {
        Self {
            kind: rk.resource_type,
            namespace: rk.namespace,
            name: rk.name,
            api_version: None,
        }
    }
}

impl From<&InventoryEntry> for ObjectRef {
    fn from(entry: &InventoryEntry) -> Self {
        Self::native(
            entry.api_version.clone(),
            entry.kind.clone(),
            entry.namespace.clone(),
            entry.name.clone(),
        )
    }
}

impl fmt::Display for ObjectRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.namespace.is_empty() {
            write!(f, "{}/{}", self.kind, self.name)
        } else {
            write!(f, "{}/{}/{}", self.kind, self.namespace, self.name)
        }
    }
}

/// Split an `apiVersion` into `(group, version)`; core kinds have no group.
fn split_api_version(api_version: &str) -> (&str, &str) {
    api_version.split_once('/').unwrap_or(("", api_version))
}

/// Resolve a native kind's API resource and scope through discovery.
async fn discover(
    client: &kube::Client,
    api_version: &str,
    kind: &str,
) -> anyhow::Result<(ApiResource, Scope)> {
    let (group, version) = split_api_version(api_version);
    let gvk = GroupVersionKind::gvk(group, version, kind);
    let (resource, caps) = kube::discovery::pinned_kind(client, &gvk)
        .await
        .with_context(|| format!("Failed to discover {kind} in {api_version}"))?;
    Ok((resource, caps.scope))
}

fn api_for(
    client: &kube::Client,
    resource: &ApiResource,
    scope: &Scope,
    namespace: &str,
) -> Api<DynamicObject> {
    match scope {
        Scope::Namespaced => Api::namespaced_with(client.clone(), namespace, resource),
        Scope::Cluster => Api::all_with(client.clone(), resource),
    }
}

/// Fetch any object as JSON. Flux resources (no `api_version`) go through the
/// Flux fetch path; native objects through discovery. Secret values are
/// redacted before they leave this function.
pub async fn fetch_object(client: &kube::Client, target: &ObjectRef) -> anyhow::Result<Value> {
    let Some(api_version) = target.api_version.as_deref() else {
        return crate::kube::fetch::fetch_resource(
            client,
            &target.kind,
            &target.namespace,
            &target.name,
        )
        .await;
    };
    let (resource, scope) = discover(client, api_version, &target.kind).await?;
    let obj = api_for(client, &resource, &scope, &target.namespace)
        .get(&target.name)
        .await
        .with_context(|| format!("Failed to fetch {target}"))?;
    let mut value = serde_json::to_value(&obj).context("Failed to serialize fetched object")?;
    redact_secret(&mut value);
    Ok(value)
}

/// Replace every value of a Secret with [`REDACTED`], keeping the keys so the
/// shape is still visible. Also drops `last-applied-configuration`, which
/// embeds the values verbatim. No-op for any other kind.
pub fn redact_secret(obj: &mut Value) {
    if obj.get("kind").and_then(Value::as_str) != Some("Secret") {
        return;
    }
    for field in ["data", "stringData"] {
        if let Some(map) = obj.get_mut(field).and_then(Value::as_object_mut) {
            for value in map.values_mut() {
                *value = Value::String(REDACTED.to_string());
            }
        }
    }
    if let Some(annotations) = obj
        .pointer_mut("/metadata/annotations")
        .and_then(Value::as_object_mut)
    {
        annotations.remove("kubectl.kubernetes.io/last-applied-configuration");
    }
}

/// Map a lookup failure to the status an inventory row should show.
fn status_for_error(error: &kube::Error) -> ObjectStatus {
    match error {
        kube::Error::Api(resp) if resp.code == 404 => {
            ObjectStatus::new(ObjectHealth::NotFound, "Not found in cluster")
        }
        kube::Error::Api(resp) if resp.code == 403 => {
            ObjectStatus::new(ObjectHealth::Forbidden, resp.message.clone())
        }
        other => ObjectStatus::new(ObjectHealth::Unknown, other.to_string()),
    }
}

/// Compute the health of every inventory entry, in input order.
///
/// Never fails as a whole: each kind is discovered once, objects are fetched
/// concurrently, and a failed discovery or GET degrades only its own rows.
pub async fn fetch_object_statuses(
    client: &kube::Client,
    entries: &[InventoryEntry],
) -> Vec<ObjectStatus> {
    // Owned keys and per-future clones keep the stream's futures 'static-free
    // of borrows, which `tokio::spawn` (via the caller) requires.
    let mut resolved: HashMap<(String, String), Result<(ApiResource, Scope), String>> =
        HashMap::new();
    for entry in entries {
        let key = (entry.api_version.clone(), entry.kind.clone());
        if let Entry::Vacant(slot) = resolved.entry(key) {
            let (api_version, kind) = slot.key();
            let result = discover(client, api_version, kind)
                .await
                .map_err(|e| format!("{e:#}"));
            slot.insert(result);
        }
    }

    futures::stream::iter(entries.iter().cloned())
        .map(|entry| {
            let resolution = resolved
                .get(&(entry.api_version.clone(), entry.kind.clone()))
                .cloned();
            let client = client.clone();
            async move {
                let (resource, scope) = match resolution {
                    Some(Ok(found)) => found,
                    Some(Err(e)) => return ObjectStatus::new(ObjectHealth::Unknown, e),
                    None => return ObjectStatus::new(ObjectHealth::Unknown, "Kind not resolved"),
                };
                match api_for(&client, &resource, &scope, &entry.namespace)
                    .get(&entry.name)
                    .await
                {
                    Ok(obj) => match serde_json::to_value(&obj) {
                        Ok(value) => compute_object_status(&value),
                        Err(e) => ObjectStatus::new(ObjectHealth::Unknown, e.to_string()),
                    },
                    Err(e) => status_for_error(&e),
                }
            }
        })
        .buffered(STATUS_FETCH_CONCURRENCY)
        .collect()
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn splits_core_and_grouped_api_versions() {
        assert_eq!(split_api_version("v1"), ("", "v1"));
        assert_eq!(split_api_version("apps/v1"), ("apps", "v1"));
        assert_eq!(
            split_api_version("networking.k8s.io/v1"),
            ("networking.k8s.io", "v1")
        );
    }

    #[test]
    fn inventory_entry_becomes_native_ref() {
        let entry = InventoryEntry {
            kind: "Service".into(),
            name: "web".into(),
            namespace: "apps".into(),
            api_version: "v1".into(),
        };
        let target = ObjectRef::from(&entry);
        assert_eq!(target.api_version.as_deref(), Some("v1"));
        assert_eq!(target.to_resource_key().to_key_string(), "Service:apps:web");
        assert_eq!(target.to_string(), "Service/apps/web");
    }

    #[test]
    fn resource_key_becomes_flux_ref() {
        let target = ObjectRef::from(ResourceKey::new("Kustomization", "flux-system", "apps"));
        assert_eq!(target.api_version, None);
    }

    #[test]
    fn cluster_scoped_display_omits_namespace() {
        let target = ObjectRef::native("v1", "Namespace", "", "apps");
        assert_eq!(target.to_string(), "Namespace/apps");
    }

    #[test]
    fn secret_values_are_redacted() {
        let mut secret = json!({
            "kind": "Secret",
            "metadata": {"annotations": {
                "kubectl.kubernetes.io/last-applied-configuration": "{\"data\":{\"password\":\"aHVudGVyMg==\"}}",
                "keep": "me"
            }},
            "data": {"password": "aHVudGVyMg=="},
            "stringData": {"token": "plain"}
        });
        redact_secret(&mut secret);
        assert_eq!(secret["data"]["password"], REDACTED);
        assert_eq!(secret["stringData"]["token"], REDACTED);
        assert!(
            secret["metadata"]["annotations"]
                .get("kubectl.kubernetes.io/last-applied-configuration")
                .is_none()
        );
        assert_eq!(secret["metadata"]["annotations"]["keep"], "me");
    }

    #[test]
    fn non_secrets_are_untouched() {
        let mut cm = json!({"kind": "ConfigMap", "data": {"k": "v"}});
        redact_secret(&mut cm);
        assert_eq!(cm["data"]["k"], "v");
    }
}
