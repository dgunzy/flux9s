//! Resource fetching utilities
//!
//! Provides functions for fetching Flux resource YAML from the Kubernetes API.

use crate::models::FluxResourceKind;
use crate::watcher::{
    Alert, ArtifactGenerator, Bucket, ExternalArtifact, FluxInstance, FluxReport, GitRepository,
    HelmChart, HelmRelease, HelmRepository, ImagePolicy, ImageRepository, ImageUpdateAutomation,
    Kustomization, OCIRepository, Provider, Receiver, ResourceSet, ResourceSetInputProvider,
};
use kube::Api;

/// Fetch resource YAML from the Kubernetes API
pub async fn fetch_resource_yaml(
    client: &kube::Client,
    resource_type: &str,
    namespace: &str,
    name: &str,
) -> anyhow::Result<serde_json::Value> {
    // Match resource type and fetch using appropriate API
    macro_rules! fetch_resource {
        ($type:ty) => {{
            let api: Api<$type> = Api::namespaced(client.clone(), namespace);
            match api.get(name).await {
                Ok(obj) => {
                    return Ok(serde_json::to_value(&obj)?);
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("Failed to fetch {}: {}", resource_type, e));
                }
            }
        }};
    }

    match FluxResourceKind::parse_optional(resource_type) {
        Some(FluxResourceKind::GitRepository) => fetch_resource!(GitRepository),
        Some(FluxResourceKind::OCIRepository) => fetch_resource!(OCIRepository),
        Some(FluxResourceKind::HelmRepository) => fetch_resource!(HelmRepository),
        Some(FluxResourceKind::Bucket) => fetch_resource!(Bucket),
        Some(FluxResourceKind::HelmChart) => fetch_resource!(HelmChart),
        Some(FluxResourceKind::ExternalArtifact) => fetch_resource!(ExternalArtifact),
        Some(FluxResourceKind::ArtifactGenerator) => fetch_resource!(ArtifactGenerator),
        Some(FluxResourceKind::Kustomization) => fetch_resource!(Kustomization),
        Some(FluxResourceKind::HelmRelease) => fetch_resource!(HelmRelease),
        Some(FluxResourceKind::ImageRepository) => fetch_resource!(ImageRepository),
        Some(FluxResourceKind::ImagePolicy) => fetch_resource!(ImagePolicy),
        Some(FluxResourceKind::ImageUpdateAutomation) => fetch_resource!(ImageUpdateAutomation),
        Some(FluxResourceKind::Alert) => fetch_resource!(Alert),
        Some(FluxResourceKind::Provider) => fetch_resource!(Provider),
        Some(FluxResourceKind::Receiver) => fetch_resource!(Receiver),
        Some(FluxResourceKind::ResourceSet) => fetch_resource!(ResourceSet),
        Some(FluxResourceKind::ResourceSetInputProvider) => {
            fetch_resource!(ResourceSetInputProvider)
        }
        Some(FluxResourceKind::FluxReport) => fetch_resource!(FluxReport),
        Some(FluxResourceKind::FluxInstance) => fetch_resource!(FluxInstance),
        None => Err(anyhow::anyhow!("Unknown resource type: {}", resource_type)),
    }
}
