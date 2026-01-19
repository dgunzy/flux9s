//! HTTP data source connector

use super::connector::DataSourceConnector;
use crate::plugins::manifest::{AuthConfig, AuthType, DataSourceConfig, DataSourceType};
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::time::Duration;

/// HTTP data source connector
pub struct HttpDataSource {
    client: reqwest::Client,
    endpoint: String,
    auth: Option<AuthConfig>,
}

impl HttpDataSource {
    /// Create a new HTTP data source
    pub fn new(config: &DataSourceConfig) -> Result<Self> {
        let endpoint = config
            .endpoint
            .clone()
            .context("endpoint required for HTTP data source")?;

        let timeout = if let Some(timeout_str) = &config.timeout {
            parse_duration(timeout_str)?
        } else {
            Duration::from_secs(5)
        };

        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .context("Failed to create HTTP client")?;

        tracing::debug!("Created HTTP data source for: {}", endpoint);

        Ok(Self {
            client,
            endpoint,
            auth: config.auth.clone(),
        })
    }

    /// Apply authentication to a request
    fn apply_auth(&self, mut req: reqwest::RequestBuilder) -> Result<reqwest::RequestBuilder> {
        if let Some(auth) = &self.auth {
            match auth.auth_type {
                AuthType::None => {}
                AuthType::Bearer => {
                    let token = self.get_env_var(&auth.token_env, "token_env")?;
                    req = req.bearer_auth(token);
                }
                AuthType::Basic => {
                    let username = auth
                        .username
                        .clone()
                        .context("username required for basic auth")?;
                    let password = self.get_env_var(&auth.password_env, "password_env")?;
                    req = req.basic_auth(username, Some(password));
                }
                AuthType::ApiKey => {
                    let header = auth
                        .header
                        .clone()
                        .context("header required for api_key auth")?;
                    let token = self.get_env_var(&auth.token_env, "token_env")?;
                    req = req.header(header, token);
                }
            }
        }

        Ok(req)
    }

    /// Get environment variable value
    fn get_env_var(&self, env_var: &Option<String>, field_name: &str) -> Result<String> {
        let var_name = env_var
            .as_ref()
            .with_context(|| format!("{} required for this auth type", field_name))?;

        std::env::var(var_name).with_context(|| {
            format!(
                "Environment variable {} not set (required for auth)",
                var_name
            )
        })
    }
}

#[async_trait]
impl DataSourceConnector for HttpDataSource {
    async fn fetch(&self) -> Result<Value> {
        tracing::debug!("Fetching data from: {}", self.endpoint);

        let req = self.client.get(&self.endpoint);
        let req = self.apply_auth(req)?;

        let resp = req
            .send()
            .await
            .with_context(|| format!("Failed to fetch from: {}", self.endpoint))?;

        if !resp.status().is_success() {
            anyhow::bail!(
                "HTTP request failed: {} (status: {})",
                self.endpoint,
                resp.status()
            );
        }

        let data = resp.json().await.context("Failed to parse JSON response")?;

        tracing::debug!("Successfully fetched data from: {}", self.endpoint);

        Ok(data)
    }

    fn connector_type(&self) -> &str {
        DataSourceType::Http.as_str()
    }

    async fn health_check(&self) -> Result<()> {
        tracing::debug!("Health check for: {}", self.endpoint);

        let req = self.client.head(&self.endpoint);
        let req = self.apply_auth(req)?;

        let resp = req
            .send()
            .await
            .with_context(|| format!("Health check failed for: {}", self.endpoint))?;

        if resp.status().is_success() || resp.status() == 404 {
            Ok(())
        } else {
            anyhow::bail!("Health check failed: {}", resp.status())
        }
    }
}

/// Parse duration string (e.g., "30s", "1m", "5s")
fn parse_duration(s: &str) -> Result<Duration> {
    if let Some(secs) = s.strip_suffix("ms") {
        let ms: u64 = secs.parse().context("Invalid duration")?;
        Ok(Duration::from_millis(ms))
    } else if let Some(secs) = s.strip_suffix('s') {
        let secs: u64 = secs.parse().context("Invalid duration")?;
        Ok(Duration::from_secs(secs))
    } else if let Some(mins) = s.strip_suffix('m') {
        let mins: u64 = mins.parse().context("Invalid duration")?;
        Ok(Duration::from_secs(mins * 60))
    } else if let Some(hours) = s.strip_suffix('h') {
        let hours: u64 = hours.parse().context("Invalid duration")?;
        Ok(Duration::from_secs(hours * 3600))
    } else {
        anyhow::bail!("Invalid duration format: {}", s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("30s").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_duration("1m").unwrap(), Duration::from_secs(60));
        assert_eq!(parse_duration("2h").unwrap(), Duration::from_secs(7200));
        assert_eq!(parse_duration("500ms").unwrap(), Duration::from_millis(500));
        assert!(parse_duration("invalid").is_err());
    }
}
