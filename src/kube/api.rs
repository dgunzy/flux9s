use anyhow::Result;
use kube::Api;
use kube::core::{ApiResource, DynamicObject};

use crate::models::FluxResourceKind;
use crate::watcher::WatchableResource;
use crate::watcher::{
    Alert, ArtifactGenerator, Bucket, ExternalArtifact, FluxInstance, FluxReport, GitRepository,
    HelmChart, HelmRelease, HelmRepository, ImagePolicy, ImageRepository, ImageUpdateAutomation,
    Kustomization, OciRepository, Provider, Receiver, ResourceSet, ResourceSetInputProvider,
};

/// Get GroupVersionKind for a resource type
pub fn get_gvk_for_resource_type(resource_type: &str) -> Result<(String, String, String)> {
    let (group, version, plural) = match FluxResourceKind::parse_optional(resource_type) {
        Some(FluxResourceKind::GitRepository) => (
            GitRepository::api_group(),
            GitRepository::api_version(),
            GitRepository::plural(),
        ),
        Some(FluxResourceKind::OCIRepository) => (
            OciRepository::api_group(),
            OciRepository::api_version(),
            OciRepository::plural(),
        ),
        Some(FluxResourceKind::HelmRepository) => (
            HelmRepository::api_group(),
            HelmRepository::api_version(),
            HelmRepository::plural(),
        ),
        Some(FluxResourceKind::Bucket) => {
            (Bucket::api_group(), Bucket::api_version(), Bucket::plural())
        }
        Some(FluxResourceKind::HelmChart) => (
            HelmChart::api_group(),
            HelmChart::api_version(),
            HelmChart::plural(),
        ),
        Some(FluxResourceKind::ExternalArtifact) => (
            ExternalArtifact::api_group(),
            ExternalArtifact::api_version(),
            ExternalArtifact::plural(),
        ),
        Some(FluxResourceKind::ArtifactGenerator) => (
            ArtifactGenerator::api_group(),
            ArtifactGenerator::api_version(),
            ArtifactGenerator::plural(),
        ),
        Some(FluxResourceKind::Kustomization) => (
            Kustomization::api_group(),
            Kustomization::api_version(),
            Kustomization::plural(),
        ),
        Some(FluxResourceKind::HelmRelease) => (
            HelmRelease::api_group(),
            HelmRelease::api_version(),
            HelmRelease::plural(),
        ),
        Some(FluxResourceKind::ImageRepository) => (
            ImageRepository::api_group(),
            ImageRepository::api_version(),
            ImageRepository::plural(),
        ),
        Some(FluxResourceKind::ImagePolicy) => (
            ImagePolicy::api_group(),
            ImagePolicy::api_version(),
            ImagePolicy::plural(),
        ),
        Some(FluxResourceKind::ImageUpdateAutomation) => (
            ImageUpdateAutomation::api_group(),
            ImageUpdateAutomation::api_version(),
            ImageUpdateAutomation::plural(),
        ),
        Some(FluxResourceKind::Alert) => {
            (Alert::api_group(), Alert::api_version(), Alert::plural())
        }
        Some(FluxResourceKind::Provider) => (
            Provider::api_group(),
            Provider::api_version(),
            Provider::plural(),
        ),
        Some(FluxResourceKind::Receiver) => (
            Receiver::api_group(),
            Receiver::api_version(),
            Receiver::plural(),
        ),
        Some(FluxResourceKind::ResourceSet) => (
            ResourceSet::api_group(),
            ResourceSet::api_version(),
            ResourceSet::plural(),
        ),
        Some(FluxResourceKind::ResourceSetInputProvider) => (
            ResourceSetInputProvider::api_group(),
            ResourceSetInputProvider::api_version(),
            ResourceSetInputProvider::plural(),
        ),
        Some(FluxResourceKind::FluxReport) => (
            FluxReport::api_group(),
            FluxReport::api_version(),
            FluxReport::plural(),
        ),
        Some(FluxResourceKind::FluxInstance) => (
            FluxInstance::api_group(),
            FluxInstance::api_version(),
            FluxInstance::plural(),
        ),
        None => {
            // For non-Flux resources, we should use DynamicObject and let the discovery API handle it
            // However, for common Kubernetes built-in resources, we can provide defaults to avoid discovery overhead
            match resource_type {
                // Workload resources (apps/v1 API group) - Kubernetes built-ins only
                "Deployment" | "StatefulSet" | "DaemonSet" | "ReplicaSet" => {
                    return Ok((
                        "apps".to_string(),
                        "v1".to_string(),
                        resource_type.to_lowercase() + "s",
                    ));
                }
                // Core resources (v1 API group - no group prefix) - Kubernetes built-ins only
                "Service" | "ConfigMap" | "Secret" | "Pod" | "Namespace" | "ServiceAccount" => {
                    return Ok((
                        "".to_string(),
                        "v1".to_string(),
                        resource_type.to_lowercase() + "s",
                    ));
                }
                // Networking resources - Kubernetes built-ins only
                "Ingress" => {
                    return Ok((
                        "networking.k8s.io".to_string(),
                        "v1".to_string(),
                        "ingresses".to_string(),
                    ));
                }
                "NetworkPolicy" => {
                    return Ok((
                        "networking.k8s.io".to_string(),
                        "v1".to_string(),
                        "networkpolicies".to_string(),
                    ));
                }
                // For all other resources: consult the dynamically
                // discovered kinds (#197) before giving up. The registry is
                // empty unless discoverFluxResources is enabled.
                _ => {
                    if let Some(extra) = crate::models::extra_kinds::global().get(resource_type) {
                        return Ok(extra.gvk());
                    }
                    // Then any kind the cluster serves (#267 discovery; empty
                    // while nativeResources is off).
                    if let Some(spec) = crate::models::kinds::catalog().find_by_kind(resource_type)
                    {
                        return Ok((spec.gvk.group, spec.gvk.version, spec.names.plural));
                    }
                    return Err(anyhow::anyhow!(
                        "Resource type '{}' is not a Flux or Kubernetes built-in resource. \
                    For CRDs and custom resources, use DynamicObject with API discovery.",
                        resource_type
                    ));
                }
            }
        }
    };

    Ok((group.to_string(), version.to_string(), plural.to_string()))
}

/// Older API versions to try for `resource_type` when its current one
/// isn't served: the Flux kind's real version history
/// ([`FluxResourceKind::older_api_versions`]); none for anything else.
fn fallback_versions(resource_type: &str) -> Vec<String> {
    FluxResourceKind::parse_optional(resource_type)
        .map(|kind| {
            kind.older_api_versions()
                .iter()
                .map(|v| v.to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// Get all Flux ApiResources to try for a resource kind, ordered from newest to oldest.
///
/// This keeps the watcher fallback logic aligned with the central Flux resource metadata
/// instead of repeating API groups, versions, and plurals at each callsite.
pub fn get_flux_api_resources_with_fallback(
    resource_kind: FluxResourceKind,
) -> Result<Vec<ApiResource>> {
    let resource_type = resource_kind.as_str();
    let (group, default_version, plural) = get_gvk_for_resource_type(resource_type)?;
    let mut versions = vec![default_version.clone()];

    for fallback in fallback_versions(resource_type) {
        if !versions.contains(&fallback) {
            versions.push(fallback);
        }
    }

    Ok(versions
        .into_iter()
        .map(|version| ApiResource {
            group: group.clone(),
            version: version.clone(),
            api_version: format!("{}/{}", group, version),
            kind: resource_type.to_string(),
            plural: plural.clone(),
        })
        .collect())
}

/// The API version of a Flux kind this cluster serves: the first of
/// [`get_flux_api_resources_with_fallback`] whose version lists the kind in
/// API discovery. Discovery is readable by every authenticated user, so the
/// answer doesn't depend on RBAC (a 403 says nothing about whether a version
/// exists, and a user limited to some names can't list unfiltered). `None`
/// when no version is served, or discovery can't be read.
pub async fn served_flux_api_resource(
    client: &kube::Client,
    kind: FluxResourceKind,
) -> Option<ApiResource> {
    let candidates = get_flux_api_resources_with_fallback(kind).ok()?;
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
    if let kube::Error::Api(api_err) = error {
        if api_err.code == 404 {
            let msg = api_err.message.to_lowercase();
            if msg.contains("could not find the requested resource")
                || msg.contains("page not found")
            {
                return NotFoundKind::VersionMissing;
            }
            return NotFoundKind::ResourceMissing;
        }
    }
    NotFoundKind::Other
}

/// Whether a failed GET means the named object doesn't exist — as opposed
/// to its API not being served at all (a CRD that isn't installed), which
/// says nothing about the object.
pub fn is_object_missing(error: &kube::Error) -> bool {
    classify_not_found(error) == NotFoundKind::ResourceMissing
}

/// Get ApiResource for a resource type with version fallback
///
/// **Why kubectl works without versions but kube-rs doesn't:**
/// - kubectl uses Kubernetes API discovery (`/apis` endpoint) to find all available versions
///   and automatically selects the preferred version or converts between versions
/// - kube-rs requires explicit ApiResource specification, but we can discover versions
///   by trying them (which is what this function does)
///
/// This function tries the default version first, then falls back to older versions if needed.
/// Fallback versions are generated dynamically based on the default version, avoiding hardcoded
/// version lists for specific resource types.
pub async fn get_api_resource_with_fallback(
    client: &kube::Client,
    resource_type: &str,
    namespace: &str,
    name: &str,
) -> Result<ApiResource> {
    // Get default group, version, and plural
    let (group, default_version, plural) = get_gvk_for_resource_type(resource_type)?;

    // For standard Kubernetes resources, use default version
    if group.is_empty() {
        return Ok(ApiResource {
            group: group.clone(),
            version: default_version.clone(),
            api_version: format!("{}/{}", group, default_version),
            kind: resource_type.to_string(),
            plural: plural.clone(),
        });
    }

    // Try default version first (usually v1, the newest)
    let api_resource = ApiResource {
        group: group.clone(),
        version: default_version.clone(),
        api_version: format!("{}/{}", group, default_version),
        kind: resource_type.to_string(),
        plural: plural.clone(),
    };

    let api: Api<DynamicObject> = Api::namespaced_with(client.clone(), namespace, &api_resource);

    // Try to get the resource with default version
    match api.get(name).await {
        Ok(_) => {
            // Default version works!
            return Ok(api_resource);
        }
        Err(e) => match classify_not_found(&e) {
            // Version doesn't exist on this cluster, try fallback versions below
            NotFoundKind::VersionMissing => {}
            // The API answered for this GVR, so the version is served — the named
            // resource just doesn't exist. Return immediately instead of probing
            // fallback versions; the caller's own GET produces the proper error.
            NotFoundKind::ResourceMissing => return Ok(api_resource),
            NotFoundKind::Other => {
                return Err(anyhow::anyhow!("Failed to fetch {}: {}", resource_type, e));
            }
        },
    }

    // Then the kind's older versions, newest first.
    for version in fallback_versions(resource_type) {
        let fallback_api_resource = ApiResource {
            group: group.clone(),
            version: version.clone(),
            api_version: format!("{}/{}", group, version),
            kind: resource_type.to_string(),
            plural: plural.clone(),
        };

        let fallback_api: Api<DynamicObject> =
            Api::namespaced_with(client.clone(), namespace, &fallback_api_resource);

        match fallback_api.get(name).await {
            Ok(_) => {
                // This version works!
                tracing::debug!(
                    "Using fallback version {} for {} (default was {})",
                    version,
                    resource_type,
                    default_version
                );
                return Ok(fallback_api_resource);
            }
            Err(e) => match classify_not_found(&e) {
                NotFoundKind::VersionMissing => {} // Try the next version
                // Version is served; resource doesn't exist at any version.
                NotFoundKind::ResourceMissing => return Ok(fallback_api_resource),
                NotFoundKind::Other => {
                    return Err(anyhow::anyhow!("Failed to fetch {}: {}", resource_type, e));
                }
            },
        }
    }

    // If we get here, default version didn't work and no fallback worked
    // Return the default anyway - the error will be handled by the caller
    Ok(api_resource)
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
        let (_, _, plural) = get_gvk_for_resource_type("NetworkPolicy").unwrap();
        assert_eq!(plural, "networkpolicies");
        let (_, _, plural) = get_gvk_for_resource_type("Ingress").unwrap();
        assert_eq!(plural, "ingresses");
    }

    use super::*;

    fn versions(kind: FluxResourceKind) -> Vec<String> {
        get_flux_api_resources_with_fallback(kind)
            .unwrap()
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
        assert!(
            fallback_versions("Deployment").is_empty(),
            "built-ins have none"
        );
    }

    #[test]
    fn the_served_version_is_the_first_whose_version_lists_the_kind() {
        let candidates = || get_flux_api_resources_with_fallback(FluxResourceKind::Alert).unwrap();
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
    fn test_get_flux_api_resources_with_fallback_for_oci_repository() {
        let api_resources =
            get_flux_api_resources_with_fallback(FluxResourceKind::OCIRepository).unwrap();

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
    fn test_get_flux_api_resources_with_fallback_for_helm_release() {
        let api_resources =
            get_flux_api_resources_with_fallback(FluxResourceKind::HelmRelease).unwrap();

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
