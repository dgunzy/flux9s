//! Kubernetes Service data source connector

use super::connector::DataSourceConnector;
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;

/// Kubernetes Service data source connector
///
/// Queries a Kubernetes service endpoint using the current kubeconfig context.
/// This is the most common use case for on-cluster plugin data sources.
pub struct KubernetesServiceDataSource {
    http_client: reqwest::Client,
    service: String,
    namespace: String,
    port: u16,
    path: String,
}

impl KubernetesServiceDataSource {
    /// Create a new Kubernetes Service data source
    pub fn new(
        _client: kube::Client,
        service: String,
        namespace: String,
        port: u16,
        path: String,
    ) -> Result<Self> {
        tracing::debug!(
            "Created Kubernetes Service data source: {}.{}.svc:{}{}",
            service,
            namespace,
            port,
            path
        );

        // Create HTTP client for service requests
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .context("Failed to create HTTP client for Kubernetes service")?;

        Ok(Self {
            http_client,
            service,
            namespace,
            port,
            path,
        })
    }

    /// Build service URL
    fn service_url(&self) -> String {
        // Use Kubernetes service DNS
        format!(
            "http://{}.{}.svc.cluster.local:{}{}",
            self.service, self.namespace, self.port, self.path
        )
    }
}

#[async_trait]
impl DataSourceConnector for KubernetesServiceDataSource {
    async fn fetch(&self) -> Result<Value> {
        let url = self.service_url();
        tracing::debug!("Fetching data from Kubernetes service: {}", url);

        let resp = self
            .http_client
            .get(&url)
            .send()
            .await
            .with_context(|| format!("Failed to fetch from service: {}", url))?;

        if !resp.status().is_success() {
            anyhow::bail!(
                "Service request failed: {} (status: {})",
                url,
                resp.status()
            );
        }

        let data = resp
            .json()
            .await
            .context("Failed to parse JSON response from service")?;

        tracing::debug!("Successfully fetched data from service: {}", url);

        Ok(data)
    }

    fn connector_type(&self) -> &str {
        "kubernetes_service"
    }

    async fn health_check(&self) -> Result<()> {
        let url = self.service_url();
        tracing::debug!("Health check for Kubernetes service: {}", url);

        let resp = self
            .http_client
            .head(&url)
            .send()
            .await
            .with_context(|| format!("Health check failed for service: {}", url))?;

        if resp.status().is_success() || resp.status() == 404 {
            Ok(())
        } else {
            anyhow::bail!("Health check failed: {}", resp.status())
        }
    }
}
