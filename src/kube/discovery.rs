//! API discovery for the kind registry (#267).
//!
//! Lists every kind the API server serves so `:<kind>` can browse native
//! resources and third-party CRDs. Tries **aggregated discovery** first (one
//! request per root, GA in Kubernetes 1.30) and falls back to the legacy
//! walk (one request per group/version) on older servers.
//!
//! Only listable + watchable top-level resources become kinds: subresources
//! (`pods/log`) and kinds without `list`/`watch` can't back a live list.

use anyhow::Context;
use futures::StreamExt;
use serde_json::Value;

use crate::models::kinds::{Gvk, KindScope, KindSpec};

/// Accept header asking for the aggregated discovery document.
const AGGREGATED_ACCEPT: &str =
    "application/json;g=apidiscovery.k8s.io;v=v2;as=APIGroupDiscoveryList,application/json";

/// Parallel requests during the legacy walk.
const LEGACY_CONCURRENCY: usize = 8;

/// Per-request cap: one hung aggregated API (a broken metrics-server, say)
/// must not stall discovery — the client itself has no request timeout.
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

async fn get_json(client: &kube::Client, path: &str, accept: &str) -> anyhow::Result<Value> {
    let request = http::Request::get(path)
        .header(http::header::ACCEPT, accept)
        .body(Vec::new())
        .context("Failed to build discovery request")?;
    let text = tokio::time::timeout(REQUEST_TIMEOUT, client.request_text(request))
        .await
        .map_err(|_| anyhow::anyhow!("Discovery request {path} timed out"))?
        .with_context(|| format!("Discovery request {path} failed"))?;
    serde_json::from_str(&text).with_context(|| format!("Invalid discovery document at {path}"))
}

fn has_verbs(resource: &Value) -> bool {
    let verbs: Vec<&str> = resource
        .get("verbs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    verbs.contains(&"list") && verbs.contains(&"watch")
}

fn short_names(resource: &Value) -> Vec<String> {
    resource
        .get("shortNames")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

/// Kinds from an aggregated `APIGroupDiscoveryList`. Every served version
/// is read, in preference order, and each kind keeps the most preferred
/// version that serves it — a group's preferred version often serves only
/// some of its kinds (metallb.io/v1beta2 has only BGPPeer; IPAddressPool is
/// v1beta1-only). `None` if the document isn't an aggregated list.
pub fn parse_aggregated(doc: &Value) -> Option<Vec<KindSpec>> {
    if doc.get("kind").and_then(Value::as_str) != Some("APIGroupDiscoveryList") {
        return None;
    }
    let mut kinds: Vec<KindSpec> = Vec::new();
    for group in doc.get("items")?.as_array()? {
        let group_name = group
            .pointer("/metadata/name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        // Versions are listed in preference order.
        for version in group
            .get("versions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let version_name = version
                .get("version")
                .and_then(Value::as_str)
                .unwrap_or_default();
            for resource in version
                .get("resources")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let (Some(plural), Some(kind)) = (
                    resource.get("resource").and_then(Value::as_str),
                    resource
                        .pointer("/responseKind/kind")
                        .and_then(Value::as_str),
                ) else {
                    continue;
                };
                if !has_verbs(resource) || already_have(&kinds, group_name, kind) {
                    continue;
                }
                let scope = if resource.get("scope").and_then(Value::as_str) == Some("Cluster") {
                    KindScope::Cluster
                } else {
                    KindScope::Namespaced
                };
                kinds.push(KindSpec::generic(
                    Gvk {
                        group: group_name.to_string(),
                        version: version_name.to_string(),
                        kind: kind.to_string(),
                    },
                    scope,
                    plural.to_string(),
                    short_names(resource),
                ));
            }
        }
    }
    Some(kinds)
}

/// Whether a more preferred version already supplied this kind.
fn already_have(kinds: &[KindSpec], group: &str, kind: &str) -> bool {
    kinds
        .iter()
        .any(|k| k.gvk.group == group && k.gvk.kind == kind)
}

/// Kinds from one legacy `APIResourceList` (`/api/v1`, `/apis/g/v`).
pub fn parse_resource_list(doc: &Value) -> Vec<KindSpec> {
    let group_version = doc
        .get("groupVersion")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let (group, version) = group_version.split_once('/').unwrap_or(("", group_version));
    doc.get("resources")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|r| {
            r.get("name")
                .and_then(Value::as_str)
                .is_some_and(|n| !n.contains('/'))
                && has_verbs(r)
        })
        .filter_map(|r| {
            let scope = if r.get("namespaced").and_then(Value::as_bool) == Some(false) {
                KindScope::Cluster
            } else {
                KindScope::Namespaced
            };
            Some(KindSpec::generic(
                Gvk {
                    group: group.to_string(),
                    version: version.to_string(),
                    kind: r.get("kind")?.as_str()?.to_string(),
                },
                scope,
                r.get("name")?.as_str()?.to_string(),
                short_names(r),
            ))
        })
        .collect()
}

async fn discover_aggregated(client: &kube::Client) -> anyhow::Result<Option<Vec<KindSpec>>> {
    let core = get_json(client, "/api", AGGREGATED_ACCEPT).await?;
    let groups = get_json(client, "/apis", AGGREGATED_ACCEPT).await?;
    Ok(match (parse_aggregated(&core), parse_aggregated(&groups)) {
        (Some(mut core), Some(groups)) => {
            core.extend(groups);
            Some(core)
        }
        _ => None,
    })
}

/// Resource-list paths for the legacy walk: the core group, then every
/// served version of every group, preferred first (see `parse_aggregated`).
fn legacy_paths(groups: &Value) -> Vec<String> {
    let mut paths = vec!["/api/v1".to_string()];
    for group in groups
        .get("groups")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let preferred = group
            .pointer("/preferredVersion/groupVersion")
            .and_then(Value::as_str);
        let mut versions: Vec<&str> = preferred.into_iter().collect();
        versions.extend(
            group
                .get("versions")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|v| v.get("groupVersion")?.as_str())
                .filter(|gv| Some(*gv) != preferred),
        );
        paths.extend(versions.into_iter().map(|gv| format!("/apis/{gv}")));
    }
    paths
}

/// The legacy (per group/version) walk on its own. `discover_kinds` only
/// uses it when aggregated discovery isn't served; public so live tests can
/// check both paths agree against a real server.
pub async fn discover_legacy(client: &kube::Client) -> anyhow::Result<Vec<KindSpec>> {
    let groups = get_json(client, "/apis", "application/json").await?;
    let paths = legacy_paths(&groups);
    let lists: Vec<Vec<KindSpec>> = futures::stream::iter(paths)
        .map(|path| async move {
            match get_json(client, &path, "application/json").await {
                Ok(doc) => parse_resource_list(&doc),
                Err(e) => {
                    // An unavailable aggregated API (e.g. a broken
                    // metrics-server) must not sink the whole catalog.
                    tracing::debug!("Skipping {path}: {e:#}");
                    Vec::new()
                }
            }
        })
        .buffered(LEGACY_CONCURRENCY)
        .collect()
        .await;
    let mut kinds: Vec<KindSpec> = Vec::new();
    for spec in lists.into_iter().flatten() {
        if !already_have(&kinds, &spec.gvk.group, &spec.gvk.kind) {
            kinds.push(spec);
        }
    }
    Ok(kinds)
}

/// Every served, listable kind, core group first. Flux kinds are included —
/// the registry's precedence keeps them on the Flux provider.
pub async fn discover_kinds(client: &kube::Client) -> anyhow::Result<Vec<KindSpec>> {
    match discover_aggregated(client).await {
        Ok(Some(kinds)) => return Ok(kinds),
        Ok(None) => tracing::debug!("Aggregated discovery not served; using the legacy walk"),
        Err(e) => tracing::debug!("Aggregated discovery failed ({e:#}); using the legacy walk"),
    }
    discover_legacy(client).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_aggregated_documents() {
        let doc = json!({
            "kind": "APIGroupDiscoveryList",
            "items": [{
                "metadata": {"name": "apps"},
                "versions": [
                    {"version": "v1", "resources": [
                        {"resource": "deployments", "responseKind": {"kind": "Deployment"},
                         "scope": "Namespaced", "shortNames": ["deploy"],
                         "verbs": ["get", "list", "watch", "create"]},
                        {"resource": "controllerrevisions", "responseKind": {"kind": "ControllerRevision"},
                         "scope": "Namespaced", "verbs": ["get"]}
                    ]},
                    {"version": "v1beta1", "resources": [
                        {"resource": "old", "responseKind": {"kind": "Old"}, "verbs": ["list", "watch"]}
                    ]}
                ]
            }, {
                "metadata": {"name": ""},
                "versions": [{"version": "v1", "resources": [
                    {"resource": "nodes", "responseKind": {"kind": "Node"}, "scope": "Cluster",
                     "shortNames": ["no"], "verbs": ["list", "watch"]}
                ]}]
            }]
        });
        let kinds = parse_aggregated(&doc).unwrap();
        let names: Vec<_> = kinds.iter().map(|k| k.gvk.kind.as_str()).collect();
        assert_eq!(names, ["Deployment", "Old", "Node"], "unwatchable skipped");
        assert_eq!(
            kinds[1].gvk.version, "v1beta1",
            "only served in the older version"
        );
        assert_eq!(kinds[0].gvk.api_version(), "apps/v1");
        assert_eq!(kinds[0].names.short_names, ["deploy"]);
        assert_eq!(kinds[2].gvk.api_version(), "v1");
        assert_eq!(kinds[2].scope, KindScope::Cluster);
    }

    #[test]
    fn kinds_only_in_non_preferred_versions_are_kept_at_their_best_version() {
        // metallb.io: v1beta2 (preferred) serves only BGPPeer; IPAddressPool
        // and friends are v1beta1-only.
        let resource = |plural: &str, kind: &str| json!({"resource": plural, "responseKind": {"kind": kind}, "scope": "Namespaced", "verbs": ["list", "watch"]});
        let doc = json!({
            "kind": "APIGroupDiscoveryList",
            "items": [{
                "metadata": {"name": "metallb.io"},
                "versions": [
                    {"version": "v1beta2", "resources": [resource("bgppeers", "BGPPeer")]},
                    {"version": "v1beta1", "resources": [
                        resource("bgppeers", "BGPPeer"),
                        resource("ipaddresspools", "IPAddressPool")
                    ]}
                ]
            }]
        });
        let kinds = parse_aggregated(&doc).unwrap();
        let found: Vec<_> = kinds
            .iter()
            .map(|k| (k.gvk.kind.as_str(), k.gvk.version.as_str()))
            .collect();
        assert_eq!(
            found,
            [("BGPPeer", "v1beta2"), ("IPAddressPool", "v1beta1")]
        );
    }

    #[test]
    fn non_aggregated_documents_are_rejected() {
        assert!(parse_aggregated(&json!({"kind": "APIGroupList", "groups": []})).is_none());
    }

    #[test]
    fn parses_legacy_resource_lists_skipping_subresources() {
        let doc = json!({
            "groupVersion": "v1",
            "resources": [
                {"name": "pods", "kind": "Pod", "namespaced": true, "shortNames": ["po"], "verbs": ["list", "watch"]},
                {"name": "pods/log", "kind": "Pod", "namespaced": true, "verbs": ["get"]},
                {"name": "namespaces", "kind": "Namespace", "namespaced": false, "shortNames": ["ns"], "verbs": ["list", "watch"]},
                {"name": "bindings", "kind": "Binding", "namespaced": true, "verbs": ["create"]}
            ]
        });
        let kinds = parse_resource_list(&doc);
        let names: Vec<_> = kinds.iter().map(|k| k.gvk.kind.as_str()).collect();
        assert_eq!(names, ["Pod", "Namespace"]);
        assert_eq!(kinds[0].gvk.group, "");
        assert_eq!(kinds[1].scope, KindScope::Cluster);

        let grouped = parse_resource_list(&json!({
            "groupVersion": "networking.k8s.io/v1",
            "resources": [{"name": "ingresses", "kind": "Ingress", "namespaced": true, "verbs": ["list", "watch"]}]
        }));
        assert_eq!(grouped[0].gvk.api_version(), "networking.k8s.io/v1");
    }

    #[test]
    fn legacy_walk_visits_every_version_preferred_first() {
        let groups = json!({"kind": "APIGroupList", "groups": [{
            "name": "metallb.io",
            "versions": [
                {"groupVersion": "metallb.io/v1beta1"},
                {"groupVersion": "metallb.io/v1beta2"}
            ],
            "preferredVersion": {"groupVersion": "metallb.io/v1beta2"}
        }]});
        assert_eq!(
            legacy_paths(&groups),
            [
                "/api/v1",
                "/apis/metallb.io/v1beta2",
                "/apis/metallb.io/v1beta1"
            ]
        );
    }
}
