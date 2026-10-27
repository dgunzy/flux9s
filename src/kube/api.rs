use anyhow::Result;
use kube::Api;
use kube::core::{ApiResource, DynamicObject};

use crate::models::FluxResourceKind;

/// How to read `resource_type` as dynamic objects: a Flux kind's current
/// version from its generated model ([`FluxResourceKind::api_resource`]);
/// common built-ins without discovery; anything else from the discovered
/// kinds. For a Flux kind on an older cluster, use
/// [`get_api_resource_with_fallback`] or [`served_flux_api_resource`].
pub fn api_resource_for_kind(resource_type: &str) -> Result<ApiResource> {
    if let Some(kind) = FluxResourceKind::parse_optional(resource_type) {
        return Ok(kind.api_resource());
    }
    let built_in = |group: &str, plural: String| {
        Ok(crate::models::kinds::Gvk {
            group: group.to_string(),
            version: "v1".to_string(),
            kind: resource_type.to_string(),
        }
        .api_resource(plural))
    };
    match resource_type {
        "Deployment" | "StatefulSet" | "DaemonSet" | "ReplicaSet" => {
            built_in("apps", resource_type.to_lowercase() + "s")
        }
        "Service" | "ConfigMap" | "Secret" | "Pod" | "Namespace" | "ServiceAccount" => {
            built_in("", resource_type.to_lowercase() + "s")
        }
        "Ingress" => built_in("networking.k8s.io", "ingresses".to_string()),
        "NetworkPolicy" => built_in("networking.k8s.io", "networkpolicies".to_string()),
        _ => {
            // Flux-adjacent kinds (#197; empty unless discoverFluxResources
            // is on), then any kind the cluster serves (#267; empty while
            // nativeResources is off).
            if let Some(extra) = crate::models::extra_kinds::global().get(resource_type) {
                return Ok(extra.api_resource());
            }
            if let Some(spec) = crate::models::kinds::catalog().find_by_kind(resource_type) {
                return Ok(spec.api_resource());
            }
            Err(anyhow::anyhow!(
                "Resource type '{}' is not a Flux or Kubernetes built-in resource. \
                 For CRDs and custom resources, use DynamicObject with API discovery.",
                resource_type
            ))
        }
    }
}

/// The API version of a Flux kind this cluster serves: the first of
/// [`FluxResourceKind::api_resources`] whose version lists the kind in
/// API discovery. Discovery is readable by every authenticated user, so the
/// answer doesn't depend on RBAC (a 403 says nothing about whether a version
/// exists, and a user limited to some names can't list unfiltered). `None`
/// when no version is served, or discovery can't be read.
pub async fn served_flux_api_resource(
    client: &kube::Client,
    kind: FluxResourceKind,
) -> Option<ApiResource> {
    let candidates = kind.api_resources();
    let group = candidates.first()?.group.clone();
    let discovered = match kube::discovery::oneshot::group(client, &group).await {
        Ok(discovered) => discovered,
        Err(e) => {
            tracing::debug!("Discovery of {group} failed: {e}");
            return None;
        }
    };
    first_served(candidates, |version, plural| {
        discovered
            .versioned_resources(version)
            .iter()
            .any(|(resource, _)| resource.plural == plural)
    })
}

/// The first candidate `serves(version, plural)` accepts. A group can serve
/// a version without every kind (notification `v1` had Receiver long before
/// Alert), so the kind is checked, not just the version.
fn first_served(
    candidates: Vec<ApiResource>,
    serves: impl Fn(&str, &str) -> bool,
) -> Option<ApiResource> {
    candidates
        .into_iter()
        .find(|candidate| serves(&candidate.version, &candidate.plural))
}

/// Classification of a failed GET used while probing API versions.
#[derive(Debug, PartialEq, Eq)]
enum NotFoundKind {
    /// The API version itself is not served on this cluster (404 for the API path).
    VersionMissing,
    /// The version is served but the named resource does not exist.
    ResourceMissing,
    /// Any other failure (auth, network, server error, ...).
    Other,
}

/// Distinguish "this API version is not served" from "the named resource does
/// not exist". Both surface as HTTP 404, but the API server uses a generic
/// "could not find the requested resource" (or "404 page not found") message
/// for unserved versions, and names the object (`<plural>.<group> "<name>"
/// not found`) when only the object is missing — in which case the version IS
/// served and there is no point probing fallback versions.
fn classify_not_found(error: &kube::Error) -> NotFoundKind {
    if let kube::Error::Api(api_err) = error
        && api_err.code == 404
    {
        let msg = api_err.message.to_lowercase();
        if msg.contains("could not find the requested resource") || msg.contains("page not found") {
            return NotFoundKind::VersionMissing;
        }
        return NotFoundKind::ResourceMissing;
    }
    NotFoundKind::Other
}

/// Whether a failed GET means the named object doesn't exist — as opposed
/// to its API not being served at all (a CRD that isn't installed), which
/// says nothing about the object.
pub fn is_object_missing(error: &kube::Error) -> bool {
    classify_not_found(error) == NotFoundKind::ResourceMissing
}

/// The ApiResource to read `namespace/name` of `resource_type` with: for a
/// Flux kind, the first of [`FluxResourceKind::api_resources`] the cluster
/// answers for (kube-rs needs an explicit version, where kubectl discovers
/// one). A GET that names the object as missing still proves its version is
/// served, so probing stops there; only an unserved version moves on.
pub async fn get_api_resource_with_fallback(
    client: &kube::Client,
    resource_type: &str,
    namespace: &str,
    name: &str,
) -> Result<ApiResource> {
    let candidates = match FluxResourceKind::parse_optional(resource_type) {
        Some(kind) => kind.api_resources(),
        None => vec![api_resource_for_kind(resource_type)?],
    };
    let Some(newest) = candidates.first().cloned() else {
        anyhow::bail!("No API version known for {resource_type}");
    };
    // Core resources are always served at v1.
    if newest.group.is_empty() {
        return Ok(newest);
    }

    for candidate in candidates {
        let api: Api<DynamicObject> = Api::namespaced_with(client.clone(), namespace, &candidate);
        match api.get(name).await {
            Ok(_) => return Ok(candidate),
            Err(e) => match classify_not_found(&e) {
                NotFoundKind::VersionMissing => {} // Try the next (older) version
                // The version is served; the object just doesn't exist —
                // the caller's own GET reports that.
                NotFoundKind::ResourceMissing => return Ok(candidate),
                NotFoundKind::Other => {
                    return Err(anyhow::anyhow!("Failed to fetch {}: {}", resource_type, e));
                }
            },
        }
    }
    // No version answered: return the newest; the caller's GET errors.
    Ok(newest)
}

/// Returns true if an error string indicates the API version doesn't exist on this cluster.
///
/// kube-rs formats missing resource types as "the server could not find the requested resource"
/// (HTTP 404), so we check case-insensitively to catch all variants.
pub fn is_version_missing_error(error: &str) -> bool {
    let lower = error.to_lowercase();
    lower.contains("404")
        || lower.contains("not found")
        || lower.contains("could not find")
        || lower.contains("page not found")
}

/// Returns true if an error string indicates the user is forbidden (RBAC 403)
/// from watching this resource type.
///
/// A 403 means the credentials are valid but lack permission to list/watch the
/// resource — e.g. RBAC denies access to a particular CRD. Unlike a transient
/// outage, this won't recover by retrying within the session, so watchers treat
/// it like a missing CRD: log it once and stop, rather than flagging the
/// "watch degraded" banner. kube-rs surfaces these as
/// "... is forbidden: User \"...\" cannot list resource ..." (HTTP 403).
pub fn is_forbidden_error(error: &str) -> bool {
    let lower = error.to_lowercase();
    lower.contains("forbidden") || lower.contains("403")
}

#[cfg(test)]
mod tests {
    #[test]
    fn built_in_networking_kinds_have_real_plurals() {
        let resource = api_resource_for_kind("NetworkPolicy").unwrap();
        assert_eq!(resource.plural, "networkpolicies");
        assert_eq!(resource.api_version, "networking.k8s.io/v1");
        let resource = api_resource_for_kind("ConfigMap").unwrap();
        assert_eq!(
            (resource.api_version.as_str(), resource.plural.as_str()),
            ("v1", "configmaps")
        );
    }

    use super::*;

    fn versions(kind: FluxResourceKind) -> Vec<String> {
        kind.api_resources()
            .into_iter()
            .map(|r| r.version)
            .collect()
    }

    #[test]
    fn fallbacks_are_each_kinds_real_version_history() {
        assert_eq!(
            versions(FluxResourceKind::HelmRelease),
            ["v2", "v2beta2", "v2beta1"]
        );
        assert_eq!(versions(FluxResourceKind::OCIRepository), ["v1", "v1beta2"]);
        // Alert / Provider: v1 in Flux 2.10, v1beta3 in 2.2–2.9, v1beta2 before.
        assert_eq!(
            versions(FluxResourceKind::Alert),
            ["v1", "v1beta3", "v1beta2"]
        );
        assert_eq!(
            versions(FluxResourceKind::Provider),
            ["v1", "v1beta3", "v1beta2"]
        );
        // Receiver went v1beta2 → v1 (Flux 2.0); it never had v1beta3.
        assert_eq!(versions(FluxResourceKind::Receiver), ["v1", "v1beta2"]);
        assert_eq!(versions(FluxResourceKind::ImagePolicy), ["v1", "v1beta2"]);
        // v1beta2 only since Flux 2.3: Flux 2.2 serves v1beta1.
        assert_eq!(
            versions(FluxResourceKind::ImageUpdateAutomation),
            ["v1", "v1beta2", "v1beta1"]
        );
        assert_eq!(versions(FluxResourceKind::Kustomization), ["v1"]);
    }

    #[test]
    fn the_served_version_is_the_first_whose_version_lists_the_kind() {
        let candidates = || FluxResourceKind::Alert.api_resources();
        // Flux 2.9: notification v1 exists (Receiver) but not for alerts.
        let flux_2_9 = |version: &str, plural: &str| match version {
            "v1" => plural == "receivers",
            "v1beta3" | "v1beta2" => plural == "alerts" || plural == "providers",
            _ => false,
        };
        assert_eq!(
            first_served(candidates(), flux_2_9).map(|r| r.version),
            Some("v1beta3".to_string())
        );
        assert_eq!(
            first_served(candidates(), |_, _| true).map(|r| r.version),
            Some("v1".to_string())
        );
        assert_eq!(first_served(candidates(), |_, _| false), None);
    }

    #[test]
    fn oci_repository_api_resources_fall_back_to_v1beta2() {
        let api_resources = FluxResourceKind::OCIRepository.api_resources();

        assert_eq!(api_resources[0].group, "source.toolkit.fluxcd.io");
        assert_eq!(api_resources[0].kind, "OCIRepository");
        assert_eq!(api_resources[0].plural, "ocirepositories");
        assert_eq!(api_resources[0].version, "v1");
        assert!(
            api_resources
                .iter()
                .any(|resource| resource.version == "v1beta2"),
            "OCIRepository should fall back to v1beta2 for older Flux clusters"
        );
    }

    #[test]
    fn helm_release_api_resources_fall_back_to_v2beta2() {
        let api_resources = FluxResourceKind::HelmRelease.api_resources();

        assert_eq!(api_resources[0].group, "helm.toolkit.fluxcd.io");
        assert_eq!(api_resources[0].kind, "HelmRelease");
        assert_eq!(api_resources[0].plural, "helmreleases");
        assert_eq!(api_resources[0].version, "v2");
        assert!(
            api_resources
                .iter()
                .any(|resource| resource.version == "v2beta2"),
            "HelmRelease should fall back to v2beta2 for older Flux clusters"
        );
    }

    #[test]
    fn test_is_version_missing_error_kubernetes_message() {
        // The actual message kube-rs surfaces for a missing resource type
        assert!(is_version_missing_error(
            "failed to perform initial watch: Api error: the server could not find the requested resource"
        ));
    }

    #[test]
    fn test_is_version_missing_error_numeric_code() {
        assert!(is_version_missing_error("error 404 from server"));
    }

    #[test]
    fn test_is_version_missing_error_not_found_mixed_case() {
        assert!(is_version_missing_error("Api error: Not Found"));
        assert!(is_version_missing_error("api error: not found"));
    }

    #[test]
    fn test_is_version_missing_error_unrelated_error() {
        assert!(!is_version_missing_error("connection refused"));
        assert!(!is_version_missing_error("timeout waiting for response"));
        assert!(!is_version_missing_error("unauthorized: token expired"));
    }

    #[test]
    fn test_is_forbidden_error_rbac_message() {
        // The message kube-rs surfaces when RBAC denies list/watch on a CRD
        assert!(is_forbidden_error(
            "failed to perform initial watch: Api error: artifactgenerators.swp.fluxcd.io is forbidden: User \"u\" cannot list resource \"artifactgenerators\" in API group \"swp.fluxcd.io\" at the cluster scope"
        ));
    }

    #[test]
    fn test_is_forbidden_error_numeric_code() {
        assert!(is_forbidden_error("error 403 from server"));
    }

    #[test]
    fn test_is_forbidden_error_unrelated_error() {
        assert!(!is_forbidden_error("connection refused"));
        assert!(!is_forbidden_error(
            "the server could not find the requested resource"
        ));
        assert!(!is_forbidden_error("timeout waiting for response"));
    }

    fn api_error(code: u16, message: &str) -> kube::Error {
        kube::Error::Api(Box::new(
            kube::core::Status::failure(message, "NotFound").with_code(code),
        ))
    }

    #[test]
    fn test_classify_not_found_version_missing() {
        // The message the API server returns when an apiVersion is not served
        assert_eq!(
            classify_not_found(&api_error(
                404,
                "the server could not find the requested resource"
            )),
            NotFoundKind::VersionMissing
        );
        // Raw 404 from a missing API group path
        assert_eq!(
            classify_not_found(&api_error(404, "404 page not found")),
            NotFoundKind::VersionMissing
        );
    }

    #[test]
    fn test_classify_not_found_resource_missing() {
        // The message when the GVR is served but the named object doesn't exist
        assert_eq!(
            classify_not_found(&api_error(
                404,
                "helmreleases.helm.toolkit.fluxcd.io \"my-release\" not found"
            )),
            NotFoundKind::ResourceMissing
        );
    }

    #[test]
    fn test_classify_not_found_other_errors() {
        assert_eq!(
            classify_not_found(&api_error(403, "forbidden")),
            NotFoundKind::Other
        );
        assert_eq!(
            classify_not_found(&api_error(500, "internal error")),
            NotFoundKind::Other
        );
    }
}
