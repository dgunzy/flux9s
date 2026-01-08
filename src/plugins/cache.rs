//! Plugin data cache with TTL-based refresh
//!
//! Manages fetching and caching data from plugin data sources.
//! Each plugin has its own refresh interval and cache entry.

use super::datasource::{create_connector, DataSourceConnector};
use super::manifest::PluginManifest;
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Cached plugin data entry
#[derive(Debug, Clone)]
struct CacheEntry {
    /// The cached data
    data: Value,
    /// When this entry was last refreshed
    last_refresh: Instant,
    /// TTL for this entry
    ttl: Duration,
}

impl CacheEntry {
    /// Check if this entry is expired
    fn is_expired(&self) -> bool {
        self.last_refresh.elapsed() >= self.ttl
    }
}

/// Plugin data cache
///
/// Manages data fetching and caching for all loaded plugins.
/// Each plugin has its own refresh interval and cache entry.
pub struct PluginCache {
    /// Data source connectors for each plugin
    connectors: HashMap<String, Arc<Box<dyn DataSourceConnector>>>,
    /// Cached data entries
    cache: Arc<RwLock<HashMap<String, CacheEntry>>>,
    /// Kubernetes client for service connectors
    kube_client: Option<kube::Client>,
}

impl PluginCache {
    /// Create a new plugin cache
    pub fn new(kube_client: Option<kube::Client>) -> Self {
        Self {
            connectors: HashMap::new(),
            cache: Arc::new(RwLock::new(HashMap::new())),
            kube_client,
        }
    }

    /// Load plugins and create connectors
    pub fn load_plugins(&mut self, plugins: Vec<PluginManifest>) -> Result<()> {
        tracing::info!("Loading {} plugin(s) into cache", plugins.len());

        for plugin in plugins {
            tracing::debug!("Creating connector for plugin: {}", plugin.name);

            // Create connector for this plugin's data source
            let connector = create_connector(&plugin.source, self.kube_client.clone())
                .with_context(|| format!("Failed to create connector for plugin '{}'", plugin.name))?;

            // Store connector
            self.connectors
                .insert(plugin.name.clone(), Arc::new(connector));

            tracing::debug!("Successfully loaded plugin connector: {}", plugin.name);
        }

        tracing::info!(
            "Successfully loaded connectors for {} plugin(s)",
            self.connectors.len()
        );

        Ok(())
    }

    /// Get cached data for a plugin
    ///
    /// Returns None if the plugin is not loaded or has no cached data.
    pub async fn get(&self, plugin_name: &str) -> Option<Value> {
        let cache = self.cache.read().await;
        cache.get(plugin_name).map(|entry| entry.data.clone())
    }

    /// Refresh data for a specific plugin
    ///
    /// Fetches fresh data from the data source and updates the cache.
    pub async fn refresh(&self, plugin_name: &str, ttl: Duration) -> Result<()> {
        // Get connector
        let connector = self
            .connectors
            .get(plugin_name)
            .with_context(|| format!("Plugin '{}' not loaded", plugin_name))?;

        tracing::debug!("Refreshing data for plugin: {}", plugin_name);

        // Fetch data
        let data = connector
            .fetch()
            .await
            .with_context(|| format!("Failed to fetch data for plugin '{}'", plugin_name))?;

        // Update cache
        let entry = CacheEntry {
            data,
            last_refresh: Instant::now(),
            ttl,
        };

        let mut cache = self.cache.write().await;
        cache.insert(plugin_name.to_string(), entry);

        tracing::debug!("Successfully refreshed data for plugin: {}", plugin_name);

        Ok(())
    }

    /// Refresh all plugins that are expired
    ///
    /// This should be called periodically from a background task.
    pub async fn refresh_expired(&self, plugins: &[PluginManifest]) -> Result<()> {
        let mut refresh_count = 0;

        for plugin in plugins {
            // Check if entry is expired
            let should_refresh = {
                let cache = self.cache.read().await;
                match cache.get(&plugin.name) {
                    None => true, // No entry, need initial fetch
                    Some(entry) => entry.is_expired(),
                }
            };

            if should_refresh {
                // Use refresh_interval from config, or default to 30s
                let interval_str = plugin
                    .source
                    .refresh_interval
                    .as_deref()
                    .unwrap_or("30s");
                let ttl = parse_duration(interval_str)?;

                if let Err(e) = self.refresh(&plugin.name, ttl).await {
                    tracing::warn!("Failed to refresh plugin '{}': {}", plugin.name, e);
                    // Continue with other plugins even if one fails
                } else {
                    refresh_count += 1;
                }
            }
        }

        if refresh_count > 0 {
            tracing::debug!("Refreshed {} plugin(s)", refresh_count);
        }

        Ok(())
    }

    /// Perform health check on all plugins
    ///
    /// Returns a map of plugin name to health check result.
    pub async fn health_check_all(&self) -> HashMap<String, Result<()>> {
        let mut results = HashMap::new();

        for (name, connector) in &self.connectors {
            tracing::debug!("Health check for plugin: {}", name);
            let result = connector.health_check().await;

            if let Err(ref e) = result {
                tracing::warn!("Health check failed for plugin '{}': {}", name, e);
            }

            results.insert(name.clone(), result);
        }

        results
    }

    /// Get cache statistics
    pub async fn stats(&self) -> CacheStats {
        let cache = self.cache.read().await;
        let total_entries = cache.len();
        let expired_entries = cache.values().filter(|e| e.is_expired()).count();

        CacheStats {
            total_entries,
            expired_entries,
            fresh_entries: total_entries - expired_entries,
        }
    }
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// Total number of cached entries
    pub total_entries: usize,
    /// Number of expired entries
    pub expired_entries: usize,
    /// Number of fresh entries
    pub fresh_entries: usize,
}

/// Parse duration string (e.g., "30s", "1m", "5s")
fn parse_duration(s: &str) -> Result<Duration> {
    if let Some(secs) = s.strip_suffix("ms") {
        let ms: u64 = secs
            .parse()
            .context("Invalid milliseconds in duration")?;
        Ok(Duration::from_millis(ms))
    } else if let Some(secs) = s.strip_suffix('s') {
        let secs: u64 = secs.parse().context("Invalid seconds in duration")?;
        Ok(Duration::from_secs(secs))
    } else if let Some(mins) = s.strip_suffix('m') {
        let mins: u64 = mins.parse().context("Invalid minutes in duration")?;
        Ok(Duration::from_secs(mins * 60))
    } else if let Some(hours) = s.strip_suffix('h') {
        let hours: u64 = hours.parse().context("Invalid hours in duration")?;
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
        assert_eq!(
            parse_duration("500ms").unwrap(),
            Duration::from_millis(500)
        );
        assert!(parse_duration("invalid").is_err());
    }

    #[test]
    fn test_cache_entry_expiry() {
        let entry = CacheEntry {
            data: serde_json::json!({"test": "value"}),
            last_refresh: Instant::now() - Duration::from_secs(60),
            ttl: Duration::from_secs(30),
        };

        assert!(entry.is_expired());

        let fresh_entry = CacheEntry {
            data: serde_json::json!({"test": "value"}),
            last_refresh: Instant::now(),
            ttl: Duration::from_secs(30),
        };

        assert!(!fresh_entry.is_expired());
    }
}
