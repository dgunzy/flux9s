//! Kubernetes Service data source connector

use super::connector::DataSourceConnector;
use crate::plugins::manifest::DataSourceType;
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::time::Duration;

/// Default HTTP timeout for Kubernetes service requests (10 seconds)
const DEFAULT_HTTP_TIMEOUT_SECS: u64 = 10;

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
    dns_suffix: String,
}

impl KubernetesServiceDataSource {
    /// Create a new Kubernetes Service data source
    ///
    /// # Arguments
    /// * `_client` - Kubernetes client (not used directly, but required for consistency)
    /// * `service` - Service name
    /// * `namespace` - Namespace name
    /// * `port` - Service port
    /// * `path` - HTTP path
    /// * `dns_suffix` - Kubernetes DNS suffix (e.g., ".svc.cluster.local")
    pub fn new(
        _client: kube::Client,
        service: String,
        namespace: String,
        port: u16,
        path: String,
        dns_suffix: String,
    ) -> Result<Self> {
        tracing::debug!(
            "Created Kubernetes Service data source: {}.{}{}:{}{}",
            service,
            namespace,
            dns_suffix,
            port,
            path
        );

        // Create HTTP client for service requests
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(DEFAULT_HTTP_TIMEOUT_SECS))
            .build()
            .context("Failed to create HTTP client for Kubernetes service")?;

        Ok(Self {
            http_client,
            service,
            namespace,
            port,
            path,
            dns_suffix,
        })
    }

    /// Build service URL
    ///
    /// Uses Kubernetes service DNS format: http://{service}.{namespace}{dns_suffix}:{port}{path}
    /// The DNS suffix is configurable via the plugin configuration.
    fn service_url(&self) -> String {
        format!(
            "http://{}.{}{}:{}{}",
            self.service, self.namespace, self.dns_suffix, self.port, self.path
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
        DataSourceType::KubernetesService.as_str()
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
