//! Resource fetching utilities
//!
//! Provides functions for fetching full Kubernetes resources from the API.

use anyhow::Context;
use kube::Api;
use kube::core::DynamicObject;

use crate::kube::get_api_resource_with_fallback;

/// Fetch a full resource object from the Kubernetes API using discovery-backed version fallback.
pub async fn fetch_resource(
    client: &kube::Client,
    resource_type: &str,
    namespace: &str,
    name: &str,
) -> anyhow::Result<serde_json::Value> {
    let api_resource = get_api_resource_with_fallback(client, resource_type, namespace, name)
        .await
        .with_context(|| {
            format!(
                "Failed to discover API resource for {}/{} in namespace {}",
                resource_type, name, namespace
            )
        })?;
    let api: Api<DynamicObject> = Api::namespaced_with(client.clone(), namespace, &api_resource);
    let obj = api.get(name).await.with_context(|| {
        format!(
            "Failed to fetch {}/{} in namespace {}",
            resource_type, name, namespace
        )
    })?;

    serde_json::to_value(&obj).context("Failed to serialize fetched resource")
}

/// Fetch resource data for the YAML view.
///
/// This remains as a compatibility wrapper around the generic resource fetch path.
pub async fn fetch_resource_yaml(
    client: &kube::Client,
    resource_type: &str,
    namespace: &str,
    name: &str,
) -> anyhow::Result<serde_json::Value> {
    fetch_resource(client, resource_type, namespace, name).await
}
