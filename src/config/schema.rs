//! Configuration schema definitions
//!
//! Defines the structure of configuration files using serde for serialization.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Root configuration structure
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    /// Disable modification operations globally
    #[serde(default = "default_read_only")]
    pub read_only: bool,

    /// Starting namespace
    #[serde(default = "default_namespace")]
    pub default_namespace: String,

    /// UI configuration
    #[serde(default)]
    pub ui: UiConfig,

    /// Logger configuration
    #[serde(default)]
    pub logger: LoggerConfig,

    /// Cluster-specific settings (merged with cluster configs)
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub cluster: HashMap<String, serde_yaml::Value>,
}

/// UI configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UiConfig {
    /// Enable mouse support
    #[serde(default = "default_true")]
    pub enable_mouse: bool,

    /// Hide header
    #[serde(default = "default_false")]
    pub headless: bool,

    /// Disable Unicode icons for compatibility
    #[serde(default = "default_false")]
    pub no_icons: bool,

    /// Default theme name
    #[serde(default = "default_skin")]
    pub skin: String,

    /// Skip startup splash screen
    #[serde(default = "default_false")]
    pub splashless: bool,
}

/// Logger configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LoggerConfig {
    /// Default log line count
    #[serde(default = "default_log_tail")]
    pub tail: u32,

    /// Max log lines in view
    #[serde(default = "default_log_buffer")]
    pub buffer: u32,

    /// Historical log timeframe in seconds
    #[serde(default = "default_log_since_seconds")]
    pub since_seconds: u64,

    /// Enable/disable line wrapping
    #[serde(default = "default_false")]
    pub text_wrap: bool,
}

// Default value functions
fn default_read_only() -> bool {
    false
}

fn default_namespace() -> String {
    "flux-system".to_string()
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

fn default_skin() -> String {
    "default".to_string()
}

fn default_log_tail() -> u32 {
    100
}

fn default_log_buffer() -> u32 {
    5000
}

fn default_log_since_seconds() -> u64 {
    300
}

impl Default for Config {
    fn default() -> Self {
        Self {
            read_only: default_read_only(),
            default_namespace: default_namespace(),
            ui: UiConfig::default(),
            logger: LoggerConfig::default(),
            cluster: HashMap::new(),
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            enable_mouse: default_false(),
            headless: default_false(),
            no_icons: default_false(),
            skin: default_skin(),
            splashless: default_false(),
        }
    }
}

impl Default for LoggerConfig {
    fn default() -> Self {
        Self {
            tail: default_log_tail(),
            buffer: default_log_buffer(),
            since_seconds: default_log_since_seconds(),
            text_wrap: default_false(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = Config::default();
        assert!(!config.read_only);
        assert_eq!(config.default_namespace, "flux-system");
        assert_eq!(config.ui.skin, "default");
    }

    #[test]
    fn test_config_serialization() {
        let config = Config::default();
        let yaml = serde_yaml::to_string(&config).unwrap();
        assert!(yaml.contains("readOnly"));
        assert!(yaml.contains("defaultNamespace"));
    }

    #[test]
    fn test_config_deserialization() {
        let yaml = r#"
readOnly: true
defaultNamespace: my-ns
ui:
  skin: dracula
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert!(config.read_only);
        assert_eq!(config.default_namespace, "my-ns");
        assert_eq!(config.ui.skin, "dracula");
    }
}
