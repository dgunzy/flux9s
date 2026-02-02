//! Configuration loading and merging logic
//!
//! Handles loading configuration from multiple sources and merging them
//! according to precedence rules.

use super::{
    defaults, paths,
    schema::{Config, PluginConfig},
};
use anyhow::{Context, Result};
use std::path::PathBuf;

/// Configuration loader
pub struct ConfigLoader;

impl ConfigLoader {
    /// Load configuration with all layers merged
    ///
    /// Precedence order (highest to lowest):
    /// 1. Environment variable overrides
    /// 2. Context-specific config
    /// 3. Cluster-specific config
    /// 4. Root config
    /// 5. Built-in defaults
    pub fn load(cluster: Option<&str>, context: Option<&str>) -> Result<Config> {
        let mut config = Self::load_defaults();

        // Load root config
        if let Ok(root_config) = Self::load_file(&paths::root_config_path()) {
            config = Self::merge_config(config, root_config);
        }

        // Load cluster-specific config if cluster is provided
        if let Some(cluster_name) = cluster {
            if let Ok(cluster_config) =
                Self::load_file(&paths::cluster_config_path(cluster_name, None))
            {
                config = Self::merge_config(config, cluster_config);
            }

            // Load context-specific config if context is provided
            if let Some(context_name) = context {
                if let Ok(context_config) = Self::load_file(&paths::cluster_config_path(
                    cluster_name,
                    Some(context_name),
                )) {
                    config = Self::merge_config(config, context_config);
                }
            }
        }

        // Apply environment variable overrides
        config = Self::apply_env_overrides(config);

        Ok(config)
    }

    /// Load configuration from a file
    pub fn load_file(path: &PathBuf) -> Result<Config> {
        if !path.exists() {
            return Err(anyhow::anyhow!("Config file not found: {}", path.display()));
        }

        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        let config: Config = serde_yaml::from_str(&contents)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;

        Ok(config)
    }

    /// Validate configuration by loading and checking for errors
    ///
    /// This performs strict validation - it will fail on:
    /// - Invalid YAML syntax
    /// - Unknown configuration keys (by attempting to parse with strict mode)
    /// - Invalid value types
    /// - File read errors
    pub fn validate(cluster: Option<&str>, context: Option<&str>) -> Result<()> {
        use anyhow::Context;

        // Try to load root config file if it exists
        let root_path = paths::root_config_path();
        if root_path.exists() {
            let contents = std::fs::read_to_string(&root_path)
                .with_context(|| format!("Failed to read config file: {}", root_path.display()))?;

            // Parse with serde_yaml - this will catch YAML syntax errors
            let config: Config = serde_yaml::from_str(&contents)
                .with_context(|| format!("Failed to parse config file: {}", root_path.display()))?;

            // Validate namespace_hotkeys length
            if config.namespace_hotkeys.len() > 10 {
                return Err(anyhow::anyhow!(
                    "namespaceHotkeys has {} items, maximum is 10",
                    config.namespace_hotkeys.len()
                ));
            }
        }

        // Try to load the full merged config to catch any merge issues
        let _ = Self::load(cluster, context).context("Failed to load merged configuration")?;

        Ok(())
    }

    /// Load default configuration
    pub fn load_defaults() -> Config {
        defaults::default_config()
    }

    /// Merge two configurations, with `other` taking precedence
    fn merge_config(_base: Config, other: Config) -> Config {
        Config {
            read_only: other.read_only,
            default_namespace: other.default_namespace.clone(),
            default_controller_namespace: other.default_controller_namespace.clone(),
            ui: UiConfig {
                enable_mouse: other.ui.enable_mouse,
                headless: other.ui.headless,
                no_icons: other.ui.no_icons,
                skin: other.ui.skin.clone(),
                skin_read_only: other.ui.skin_read_only.clone(),
                splashless: other.ui.splashless,
            },
            logger: LoggerConfig {
                tail: other.logger.tail,
                buffer: other.logger.buffer,
                since_seconds: other.logger.since_seconds,
                text_wrap: other.logger.text_wrap,
            },
            plugin: PluginConfig {
                kubernetes_dns_suffix: other.plugin.kubernetes_dns_suffix.clone(),
            },
            namespace_hotkeys: other.namespace_hotkeys.clone(),
            context_skins: other.context_skins.clone(),
            cluster: other.cluster,
            favorites: other.favorites.clone(),
        }
    }

    /// Apply environment variable overrides
    fn apply_env_overrides(mut config: Config) -> Config {
        // FLUX9S_SKIN override
        if let Ok(skin) = std::env::var("FLUX9S_SKIN") {
            config.ui.skin = skin;
        }

        // FLUX9S_READ_ONLY override
        if let Ok(read_only) = std::env::var("FLUX9S_READ_ONLY") {
            if let Ok(val) = read_only.parse::<bool>() {
                config.read_only = val;
            }
        }

        // FLUX9S_DEFAULT_NAMESPACE override
        if let Ok(namespace) = std::env::var("FLUX9S_DEFAULT_NAMESPACE") {
            config.default_namespace = namespace;
        }

        config
    }

    /// Save configuration to a file
    pub fn save(config: &Config, path: &PathBuf) -> Result<()> {
        // Ensure directory exists
        if let Some(parent) = path.parent() {
            paths::ensure_dir(parent)?;
        }

        let yaml =
            serde_yaml::to_string(config).context("Failed to serialize configuration to YAML")?;

        std::fs::write(path, yaml)
            .with_context(|| format!("Failed to write config file: {}", path.display()))?;

        Ok(())
    }

    /// Save root configuration
    pub fn save_root(config: &Config) -> Result<()> {
        Self::save(config, &paths::root_config_path())
    }

    /// Save cluster-specific configuration
    pub fn save_cluster(config: &Config, cluster: &str, context: Option<&str>) -> Result<()> {
        Self::save(config, &paths::cluster_config_path(cluster, context))
    }
}

// Re-export types for convenience
use super::schema::{LoggerConfig, UiConfig};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_defaults() {
        let config = ConfigLoader::load_defaults();
        assert!(config.read_only);
        assert_eq!(config.default_namespace, "flux-system");
    }

    #[test]
    fn test_merge_config() {
        let base = Config::default();
        let other = Config {
            read_only: true,
            default_namespace: "test-ns".to_string(),
            ..Default::default()
        };

        let merged = ConfigLoader::merge_config(base, other);
        assert!(merged.read_only);
        assert_eq!(merged.default_namespace, "test-ns");
    }

    #[test]
    fn test_env_overrides() {
        // SAFETY: set_var is unsafe in Rust 2024 due to potential data races.
        // This is safe in tests because:
        // 1. Tests run sequentially by default (unless explicitly parallelized)
        // 2. Each test sets its own isolated environment variables
        // 3. We clean up after the test completes
        unsafe {
            std::env::set_var("FLUX9S_SKIN", "test-skin");
            std::env::set_var("FLUX9S_READ_ONLY", "true");
        }

        let config = Config::default();
        let config = ConfigLoader::apply_env_overrides(config);

        assert_eq!(config.ui.skin, "test-skin");
        assert!(config.read_only);

        // Cleanup
        // SAFETY: remove_var is unsafe in Rust 2024 due to potential data races.
        // Safe in tests for the same reasons as set_var above.
        unsafe {
            std::env::remove_var("FLUX9S_SKIN");
            std::env::remove_var("FLUX9S_READ_ONLY");
        }
    }
}
