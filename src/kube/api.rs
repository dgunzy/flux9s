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
                "Ingress" | "NetworkPolicy" => {
                    return Ok((
                        "networking.k8s.io".to_string(),
                        "v1".to_string(),
                        resource_type.to_lowercase() + "es",
                    ));
                }
                // For all other resources (including CRDs), return an error
                // The caller should handle this by using DynamicObject with proper discovery
                _ => {
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

/// Generate fallback API versions based on the default version
///
/// This generates common fallback versions without hardcoding specific resource types.
/// For example, if default is "v1", it will try "v1beta2", "v1beta1", "v1alpha1".
/// If default is "v2", it will try "v2beta2", "v2beta1", "v1", "v1beta2", etc.
fn generate_fallback_versions(default_version: &str) -> Vec<String> {
    let mut fallbacks = Vec::new();

    // Parse version (e.g., "v1", "v2beta1", "v1beta2")
    if let Some(version_num) = default_version.strip_prefix('v') {
        // Extract major version and suffix
        let parts: Vec<&str> = version_num.splitn(2, |c: char| c.is_alphabetic()).collect();
        let major = parts[0].parse::<u32>().unwrap_or(1);
        let suffix = if parts.len() > 1 { parts[1] } else { "" };

        // If it's a stable version (v1, v2, etc.), generate beta/alpha fallbacks
        if suffix.is_empty() {
            // For v1: try v1beta2, v1beta1, v1alpha1
            fallbacks.push(format!("v{}beta2", major));
            fallbacks.push(format!("v{}beta1", major));
            fallbacks.push(format!("v{}alpha1", major));

            // Also try previous major version's stable and betas
            if major > 1 {
                let prev_major = major - 1;
                fallbacks.push(format!("v{}", prev_major));
                fallbacks.push(format!("v{}beta2", prev_major));
                fallbacks.push(format!("v{}beta1", prev_major));
            }
        } else if suffix.starts_with("beta") {
            // If it's a beta version, try other beta versions and alpha
            let beta_num = suffix
                .strip_prefix("beta")
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(1);
            if beta_num > 1 {
                fallbacks.push(format!("v{}beta{}", major, beta_num - 1));
            }
            fallbacks.push(format!("v{}beta1", major));
            fallbacks.push(format!("v{}alpha1", major));

            // Also try previous major version
            if major > 1 {
                fallbacks.push(format!("v{}", major - 1));
            }
        } else if suffix.starts_with("alpha") {
            // If it's an alpha version, try other alpha versions
            let alpha_num = suffix
                .strip_prefix("alpha")
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(1);
            if alpha_num > 1 {
                fallbacks.push(format!("v{}alpha{}", major, alpha_num - 1));
            }
            fallbacks.push(format!("v{}alpha1", major));
        }
    }

    fallbacks
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

    for fallback in generate_fallback_versions(&default_version) {
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
        Err(e) => {
            if !is_version_missing_error(&format!("{}", e)) {
                return Err(anyhow::anyhow!("Failed to fetch {}: {}", resource_type, e));
            }
            // Version doesn't exist on this cluster, try fallback versions
        }
    }

    // Generate fallback versions dynamically based on the default version
    // This avoids hardcoding specific resource types and versions
    let fallback_versions = generate_fallback_versions(&default_version);

    // Try fallback versions
    for version in fallback_versions {
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
            Err(e) => {
                if !is_version_missing_error(&format!("{}", e)) {
                    // Non-"not found" error means this version exists but the resource doesn't
                    return Ok(fallback_api_resource);
                }
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_fallback_versions_stable_v1() {
        let fallbacks = generate_fallback_versions("v1");
        assert!(
            fallbacks.contains(&"v1beta2".to_string()),
            "v1 should fall back to v1beta2 for older clusters"
        );
        assert!(fallbacks.contains(&"v1beta1".to_string()));
    }

    #[test]
    fn test_generate_fallback_versions_stable_v2() {
        let fallbacks = generate_fallback_versions("v2");
        assert!(
            fallbacks.contains(&"v2beta2".to_string()),
            "v2 should fall back to v2beta2 for older clusters (e.g. HelmRelease)"
        );
        assert!(fallbacks.contains(&"v1".to_string()));
    }

    #[test]
    fn test_generate_fallback_versions_beta_does_not_include_stable_next() {
        // v2beta2 should NOT generate v2 as a fallback — that's a promotion, not a fallback.
        // This was the HelmRelease bug: impl_watchable! said v2beta2 so individual-resource ops
        // tried v2beta2 first and the fallback list never reached v2.
        let fallbacks = generate_fallback_versions("v2beta2");
        assert!(
            !fallbacks.contains(&"v2".to_string()),
            "beta fallbacks should not include the stable promoted version"
        );
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
}
