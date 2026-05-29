//! Configuration loading and merging logic
//!
//! Handles loading configuration from multiple sources and merging them
//! according to precedence rules.

use super::{defaults, paths, schema::Config};
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
        let mut merged_yaml = match serde_yaml::to_value(Self::load_defaults()) {
            Ok(v) => v,
            Err(_) => serde_yaml::Value::Mapping(serde_yaml::Mapping::new()),
        };

        // Load root config
        if let Ok(root_yaml) = Self::load_yaml_file(&paths::root_config_path()) {
            Self::merge_yaml(&mut merged_yaml, root_yaml);
        }

        // Load cluster-specific config if cluster is provided
        if let Some(cluster_name) = cluster {
            if let Ok(cluster_yaml) =
                Self::load_yaml_file(&paths::cluster_config_path(cluster_name, None))
            {
                Self::merge_yaml(&mut merged_yaml, cluster_yaml);
            }

            // Load context-specific config if context is provided
            if let Some(context_name) = context {
                if let Ok(context_yaml) = Self::load_yaml_file(&paths::cluster_config_path(
                    cluster_name,
                    Some(context_name),
                )) {
                    Self::merge_yaml(&mut merged_yaml, context_yaml);
                }
            }
        }

        // Deserialize the fully merged YAML value into Config
        let mut config: Config =
            serde_yaml::from_value(merged_yaml).context("Failed to parse merged configuration")?;

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

    /// Load a YAML value from a file
    fn load_yaml_file(path: &PathBuf) -> Result<serde_yaml::Value> {
        if !path.exists() {
            return Err(anyhow::anyhow!("Config file not found: {}", path.display()));
        }

        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        if contents.trim().is_empty() {
            return Ok(serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));
        }

        let val: serde_yaml::Value = serde_yaml::from_str(&contents)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;

        Ok(val)
    }

    /// Recursively merge two YAML values, with `other` taking precedence over `base`.
    fn merge_yaml(base: &mut serde_yaml::Value, other: serde_yaml::Value) {
        match (base, other) {
            (serde_yaml::Value::Mapping(base_map), serde_yaml::Value::Mapping(other_map)) => {
                for (k, v) in other_map {
                    if v.is_null() {
                        continue;
                    }
                    // Special case for sequence fields: if empty, inherit from base
                    if let serde_yaml::Value::Sequence(ref seq) = v {
                        if seq.is_empty() {
                            continue;
                        }
                    }
                    // Special case for options: skip if null
                    if v.is_null() {
                        continue;
                    }
                    if base_map.contains_key(&k) {
                        if let Some(base_val) = base_map.get_mut(&k) {
                            Self::merge_yaml(base_val, v);
                        }
                    } else {
                        base_map.insert(k, v);
                    }
                }
            }
            (base, other) => {
                if !other.is_null() {
                    *base = other;
                }
            }
        }
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

        // FLUX9S_DEFAULT_RESOURCE_FILTER override
        if let Ok(filter) = std::env::var("FLUX9S_DEFAULT_RESOURCE_FILTER") {
            if !filter.is_empty() {
                config.default_resource_filter = Some(filter);
            }
        }

        // FLUX9S_CONNECT_TIMEOUT override
        if let Ok(timeout) = std::env::var(crate::kube::health::CONNECT_TIMEOUT_ENV) {
            if let Ok(seconds) = timeout.parse::<u64>() {
                if seconds > 0 {
                    config.connect_timeout_seconds = seconds;
                }
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn merge_config(base: Config, other: Config) -> Config {
        let mut base_val = serde_yaml::to_value(base).unwrap();
        let other_val = serde_yaml::to_value(other).unwrap();
        ConfigLoader::merge_yaml(&mut base_val, other_val);
        serde_yaml::from_value(base_val).unwrap()
    }

    #[test]
    fn test_load_defaults() {
        let config = ConfigLoader::load_defaults();
        assert!(config.read_only);
        assert_eq!(config.default_namespace, "flux-system");
    }

    #[test]
    fn test_merge_config_scalar_fields() {
        let base = Config::default();
        let other = Config {
            read_only: true,
            default_namespace: "test-ns".to_string(),
            ..Default::default()
        };

        let merged = merge_config(base, other);
        assert!(merged.read_only);
        assert_eq!(merged.default_namespace, "test-ns");
    }

    #[test]
    fn test_merge_config_preserves_base_options() {
        // Base has skin_read_only and default_resource_filter set; other does not.
        // Merge should preserve the base values rather than wiping them.
        let base = Config {
            ui: crate::config::schema::UiConfig {
                skin_read_only: Some("rose-pine".to_string()),
                ..Default::default()
            },
            default_resource_filter: Some("Kustomization".to_string()),
            ..Default::default()
        };
        let other = Config::default(); // other has no Option values set

        let merged = merge_config(base, other);
        assert_eq!(merged.ui.skin_read_only, Some("rose-pine".to_string()));
        assert_eq!(
            merged.default_resource_filter,
            Some("Kustomization".to_string())
        );
    }

    #[test]
    fn test_merge_config_other_option_wins() {
        // When other explicitly sets an Option field it should override base.
        let base = Config {
            ui: crate::config::schema::UiConfig {
                skin_read_only: Some("base-skin".to_string()),
                ..Default::default()
            },
            default_resource_filter: Some("HelmRelease".to_string()),
            ..Default::default()
        };
        let other = Config {
            ui: crate::config::schema::UiConfig {
                skin_read_only: Some("other-skin".to_string()),
                ..Default::default()
            },
            default_resource_filter: Some("Kustomization".to_string()),
            ..Default::default()
        };

        let merged = merge_config(base, other);
        assert_eq!(merged.ui.skin_read_only, Some("other-skin".to_string()));
        assert_eq!(
            merged.default_resource_filter,
            Some("Kustomization".to_string())
        );
    }

    #[test]
    fn test_merge_config_preserves_base_vec_fields() {
        // Non-empty base hotkeys and favorites are kept when other leaves them empty.
        let base = Config {
            namespace_hotkeys: vec!["all".to_string(), "flux-system".to_string()],
            favorites: vec!["Kustomization:flux-system:app".to_string()],
            ..Default::default()
        };
        let other = Config::default(); // empty vecs

        let merged = merge_config(base, other);
        assert_eq!(merged.namespace_hotkeys.len(), 2);
        assert_eq!(merged.favorites.len(), 1);
    }

    #[test]
    fn test_merge_config_merges_context_skins() {
        // context_skins maps should be merged, with other taking precedence per key.
        let mut base_skins = std::collections::HashMap::new();
        base_skins.insert("prod".to_string(), "rose-pine".to_string());
        base_skins.insert("staging".to_string(), "dracula".to_string());

        let mut other_skins = std::collections::HashMap::new();
        other_skins.insert("prod".to_string(), "nord".to_string()); // override
        other_skins.insert("dev".to_string(), "monokai".to_string()); // new key

        let base = Config {
            context_skins: base_skins,
            ..Default::default()
        };
        let other = Config {
            context_skins: other_skins,
            ..Default::default()
        };

        let merged = merge_config(base, other);
        assert_eq!(
            merged.context_skins.get("prod").map(String::as_str),
            Some("nord")
        ); // other wins
        assert_eq!(
            merged.context_skins.get("staging").map(String::as_str),
            Some("dracula")
        ); // base kept
        assert_eq!(
            merged.context_skins.get("dev").map(String::as_str),
            Some("monokai")
        ); // new key
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
            std::env::set_var(crate::kube::health::CONNECT_TIMEOUT_ENV, "12");
        }

        let config = Config::default();
        let config = ConfigLoader::apply_env_overrides(config);

        assert_eq!(config.ui.skin, "test-skin");
        assert!(config.read_only);
        assert_eq!(config.connect_timeout_seconds, 12);

        // Cleanup
        // SAFETY: remove_var is unsafe in Rust 2024 due to potential data races.
        // Safe in tests for the same reasons as set_var above.
        unsafe {
            std::env::remove_var("FLUX9S_SKIN");
            std::env::remove_var("FLUX9S_READ_ONLY");
            std::env::remove_var(crate::kube::health::CONNECT_TIMEOUT_ENV);
        }
    }

    #[test]
    fn test_merge_yaml_preserves_root_timeout_when_cluster_omits_it() {
        let base_yaml_str = "connectTimeoutSeconds: 30\nreadOnly: true";
        let other_yaml_str = "defaultNamespace: my-ns"; // omits connectTimeoutSeconds and readOnly

        let mut base_val: serde_yaml::Value = serde_yaml::from_str(base_yaml_str).unwrap();
        let other_val: serde_yaml::Value = serde_yaml::from_str(other_yaml_str).unwrap();

        ConfigLoader::merge_yaml(&mut base_val, other_val);

        let merged_config: Config = serde_yaml::from_value(base_val).unwrap();
        assert_eq!(merged_config.connect_timeout_seconds, 30);
        assert!(merged_config.read_only);
        assert_eq!(merged_config.default_namespace, "my-ns");
    }
}
