//! File data source connector (for testing)

use super::connector::DataSourceConnector;
use crate::plugins::manifest::DataSourceType;
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;

/// File data source connector
///
/// Reads plugin data from a local JSON file.
/// Primarily used for testing and development.
pub struct FileDataSource {
    file_path: PathBuf,
}

impl FileDataSource {
    /// Create a new File data source
    pub fn new(file_path: String) -> Result<Self> {
        let path = PathBuf::from(file_path);

        tracing::debug!("Created File data source: {:?}", path);

        Ok(Self { file_path: path })
    }
}

#[async_trait]
impl DataSourceConnector for FileDataSource {
    async fn fetch(&self) -> Result<Value> {
        tracing::debug!("Reading data from file: {:?}", self.file_path);

        let content = tokio::fs::read_to_string(&self.file_path)
            .await
            .with_context(|| format!("Failed to read file: {:?}", self.file_path))?;

        let data: Value = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse JSON from file: {:?}", self.file_path))?;

        tracing::debug!("Successfully loaded data from file: {:?}", self.file_path);

        Ok(data)
    }

    fn connector_type(&self) -> &str {
        DataSourceType::File.as_str()
    }

    async fn health_check(&self) -> Result<()> {
        tracing::debug!("Health check for file: {:?}", self.file_path);

        if !self.file_path.exists() {
            anyhow::bail!("File does not exist: {:?}", self.file_path);
        }

        Ok(())
    }
}
