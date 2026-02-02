//! Data source connector trait and factory

use crate::plugins::manifest::{DataSourceConfig, DataSourceType};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

/// Data source connector trait
#[async_trait]
pub trait DataSourceConnector: Send + Sync {
    /// Fetch data from the source
    async fn fetch(&self) -> Result<Value>;

    /// Get connector type name
    fn connector_type(&self) -> &str;

    /// Health check (optional, returns Ok if source is reachable)
    async fn health_check(&self) -> Result<()> {
        // Default implementation: try to fetch
        self.fetch().await?;
        Ok(())
    }
}

/// Create a data source connector from configuration
///
/// # Arguments
/// * `config` - Data source configuration from plugin manifest
/// * `kube_client` - Optional Kubernetes client (required for kubernetes_service type)
/// * `dns_suffix` - Kubernetes DNS suffix (e.g., ".svc.cluster.local") from config
pub fn create_connector(
    config: &DataSourceConfig,
    kube_client: Option<kube::Client>,
    dns_suffix: &str,
) -> Result<Box<dyn DataSourceConnector>> {
    tracing::debug!("Creating data source connector: {:?}", config.source_type);

    match config.source_type {
        DataSourceType::KubernetesService => {
            let client = kube_client.ok_or_else(|| {
                anyhow::anyhow!("Kubernetes client required for kubernetes_service data source")
            })?;

            Ok(Box::new(
                super::k8s_service::KubernetesServiceDataSource::new(
                    client,
                    config
                        .service
                        .clone()
                        .ok_or_else(|| anyhow::anyhow!("service field required"))?,
                    config
                        .namespace
                        .clone()
                        .ok_or_else(|| anyhow::anyhow!("namespace field required"))?,
                    config
                        .port
                        .ok_or_else(|| anyhow::anyhow!("port field required"))?,
                    config
                        .path
                        .clone()
                        .ok_or_else(|| anyhow::anyhow!("path field required"))?,
                    dns_suffix.to_string(),
                )?,
            ))
        }
        DataSourceType::Http => Ok(Box::new(super::http::HttpDataSource::new(config)?)),
        DataSourceType::File => Ok(Box::new(super::file::FileDataSource::new(
            config
                .file_path
                .clone()
                .ok_or_else(|| anyhow::anyhow!("file_path field required"))?,
        )?)),
        DataSourceType::KubernetesCrd => {
            // TODO: Implement CRD connector in future phase
            anyhow::bail!("kubernetes_crd data source not yet implemented")
        }
    }
}
