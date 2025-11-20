use anyhow::Result;
use kube::core::{ApiResource, DynamicObject};
use kube::Api;

use crate::models::FluxResourceKind;
use crate::watcher::WatchableResource;
use crate::watcher::{
    Alert, Bucket, ExternalArtifact, FluxInstance, FluxReport, GitRepository, HelmChart,
    HelmRelease, HelmRepository, ImagePolicy, ImageRepository, ImageUpdateAutomation,
    Kustomization, OCIRepository, Provider, Receiver, ResourceSet, ResourceSetInputProvider,
};

/// Get GroupVersionKind for a resource type
pub fn get_gvk_for_resource_type(resource_type: &str) -> Result<(String, String, String)> {
    let (group, version, plural) = match FluxResourceKind::from_str(resource_type) {
        Some(FluxResourceKind::GitRepository) => (
            GitRepository::api_group(),
            GitRepository::api_version(),
            GitRepository::plural(),
        ),
        Some(FluxResourceKind::OCIRepository) => (
            OCIRepository::api_group(),
            OCIRepository::api_version(),
            OCIRepository::plural(),
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
            // Handle standard Kubernetes resources
            match resource_type {
                "Deployment" | "Service" | "ConfigMap" | "Secret" | "Pod" | "Namespace" => {
                    return Ok((
                        "".to_string(),
                        "v1".to_string(),
                        resource_type.to_lowercase() + "s",
                    ));
                }
                _ => return Err(anyhow::anyhow!("Unknown resource type: {}", resource_type)),
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
            let error_string = format!("{}", e);
            // If it's not a 404, return the error (resource might not exist or other issue)
            if !error_string.contains("404") && !error_string.contains("Not Found") {
                return Err(anyhow::anyhow!("Failed to fetch {}: {}", resource_type, e));
            }
            // 404 means this version doesn't exist, try fallback versions
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
                let error_string = format!("{}", e);
                // If it's not a 404, this might be the right version but resource doesn't exist
                // Continue trying other versions
                if !error_string.contains("404") && !error_string.contains("Not Found") {
                    // Non-404 error - might be the right version, return it
                    return Ok(fallback_api_resource);
                }
            }
        }
    }

    // If we get here, default version didn't work and no fallback worked
    // Return the default anyway - the error will be handled by the caller
    Ok(api_resource)
}
