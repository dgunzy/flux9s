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

/// Payload backing the describe view: the full object plus its Kubernetes
/// Events, fetched together so the view renders in one pass.
#[derive(Debug, Clone)]
pub struct DescribeData {
    /// The full resource object.
    pub object: serde_json::Value,
    /// The resource's events, newest first.
    pub events: Vec<crate::kube::events::KubeEventInfo>,
    /// Set when the events lookup failed (e.g. RBAC) — the describe view
    /// degrades to a notice instead of failing entirely.
    pub events_error: Option<String>,
}

/// Fetch a resource and its Events for the describe view.
///
/// The object fetch failing fails the describe; the events fetch failing
/// only degrades the Events section.
pub async fn fetch_describe_data(
    client: &kube::Client,
    resource_type: &str,
    namespace: &str,
    name: &str,
) -> anyhow::Result<DescribeData> {
    let object = fetch_resource(client, resource_type, namespace, name).await?;
    let (events, events_error) = match crate::kube::events::fetch_events_for_resource(
        client,
        resource_type,
        namespace,
        name,
    )
    .await
    {
        Ok(events) => (events, None),
        Err(e) => {
            tracing::warn!(
                "Events lookup failed for {}/{} in namespace {}: {}",
                resource_type,
                name,
                namespace,
                e
            );
            (Vec::new(), Some(format!("{}", e)))
        }
    };
    Ok(DescribeData {
        object,
        events,
        events_error,
    })
}
