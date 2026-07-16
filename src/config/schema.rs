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

    /// Flux Controllers namespace
    #[serde(default = "default_namespace")]
    pub default_controller_namespace: String,

    /// Opt-in dynamic discovery of Flux-adjacent CRDs (#197). When true,
    /// flux9s watches CustomResourceDefinitions labeled
    /// `app.kubernetes.io/part-of=<flux instance>` — the same label the Flux
    /// Operator's FluxReport uses — and shows their resources with generic
    /// columns (view-only). Default false: no CRD watch, no extra API calls.
    #[serde(default)]
    pub discover_flux_resources: bool,

    /// UI configuration
    #[serde(default)]
    pub ui: UiConfig,

    /// Namespace hotkeys configuration (0-9)
    /// Array of namespace names, where index corresponds to hotkey (0=all, 1=flux-system, etc.)
    /// Maximum 10 items (0-9)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub namespace_hotkeys: Vec<String>,

    /// Context-specific skin configuration
    /// Map of context name to skin name
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub context_skins: HashMap<String, String>,

    /// Cluster-specific settings (merged with cluster configs)
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub cluster: HashMap<String, serde_yaml::Value>,

    /// Favorite resources (resource keys: "resource_type:namespace:name")
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub favorites: Vec<String>,

    /// Default resource type filter applied at startup (None = show all types)
    /// Accepts display names (e.g., "Kustomization") or aliases (e.g., "ks") — stored as display name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_resource_filter: Option<String>,

    /// Timeout (in seconds) for the initial connectivity/health check to the
    /// Kubernetes API server at startup. Overridable at runtime with the
    /// `FLUX9S_CONNECT_TIMEOUT` environment variable.
    #[serde(default = "default_connect_timeout_seconds")]
    pub connect_timeout_seconds: u64,
}

impl Config {
    /// A config with every field populated — the field-enumeration source for
    /// `config list` and the completeness tests, since `skip_serializing_if`
    /// hides empty fields from a serialized real config.
    ///
    /// Deliberately constructed without `..Default::default()`: adding a
    /// field to the schema fails compilation here, forcing the reference
    /// docs, `config get`/`set`, and the docs site to be updated with it.
    pub fn fully_populated() -> Self {
        Self {
            read_only: true,
            default_namespace: "flux-system".to_string(),
            default_controller_namespace: "flux-system".to_string(),
            discover_flux_resources: true,
            ui: UiConfig {
                enable_mouse: true,
                headless: true,
                no_icons: true,
                skin: "default".to_string(),
                skin_read_only: Some("default".to_string()),
                splashless: true,
            },
            namespace_hotkeys: vec!["flux-system".to_string()],
            context_skins: HashMap::from([("my-context".to_string(), "default".to_string())]),
            cluster: HashMap::from([(
                "my-cluster".to_string(),
                serde_yaml::Value::String("value".to_string()),
            )]),
            favorites: vec!["Kustomization:flux-system:my-app".to_string()],
            default_resource_filter: Some("Kustomization".to_string()),
            connect_timeout_seconds: default_connect_timeout_seconds(),
        }
    }

    /// Resolve which skin to use for the given context.
    ///
    /// Priority order:
    /// 1. `FLUX9S_SKIN` environment variable (highest)
    /// 2. Context-specific skin from `contextSkins`
    /// 3. Readonly-specific skin (`ui.skinReadOnly`) when `readOnly` is true
    /// 4. Default skin (`ui.skin`)
    pub fn resolve_skin_name(&self, context_name: Option<&str>) -> String {
        if let Ok(env_skin) = std::env::var("FLUX9S_SKIN") {
            tracing::debug!(
                "Using skin from FLUX9S_SKIN environment variable: {}",
                env_skin
            );
            return env_skin;
        }
        if let Some(context) = context_name {
            if let Some(context_skin) = self.context_skins.get(context) {
                tracing::debug!(
                    "Using context-specific skin for '{}': {}",
                    context,
                    context_skin
                );
                return context_skin.clone();
            }
        }
        if self.read_only {
            if let Some(ref skin) = self.ui.skin_read_only {
                tracing::debug!("Using readonly-specific skin: {}", skin);
                return skin.clone();
            }
        }
        tracing::debug!("Using default skin: {}", self.ui.skin);
        self.ui.skin.clone()
    }
}

/// UI configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UiConfig {
    /// Enable mouse support
    #[serde(default = "default_false")]
    pub enable_mouse: bool,

    /// Hide header
    #[serde(default = "default_false")]
    pub headless: bool,

    /// Disable Unicode icons for compatibility
    #[serde(default = "default_false")]
    pub no_icons: bool,

    /// Default skin name
    #[serde(default = "default_skin")]
    pub skin: String,

    /// Skin name for readonly mode (overrides skin when readOnly=true)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skin_read_only: Option<String>,

    /// Skip startup splash screen
    #[serde(default = "default_false")]
    pub splashless: bool,
}

// Default value functions
fn default_read_only() -> bool {
    true
}

fn default_namespace() -> String {
    "flux-system".to_string()
}

fn default_false() -> bool {
    false
}

fn default_skin() -> String {
    "default".to_string()
}

/// Default connection/health-check timeout in seconds.
fn default_connect_timeout_seconds() -> u64 {
    crate::kube::health::DEFAULT_CONNECT_TIMEOUT_SECS
}

impl Default for Config {
    fn default() -> Self {
        Self {
            read_only: default_read_only(),
            default_namespace: default_namespace(),
            default_controller_namespace: default_namespace(),
            discover_flux_resources: false,
            ui: UiConfig::default(),
            namespace_hotkeys: Vec::new(), // Empty means use auto-discovered defaults
            context_skins: HashMap::new(),
            cluster: HashMap::new(),
            favorites: Vec::new(), // Empty by default
            default_resource_filter: None,
            connect_timeout_seconds: default_connect_timeout_seconds(),
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
            skin_read_only: None,
            splashless: default_false(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = Config::default();
        assert!(config.read_only);
        assert_eq!(config.default_namespace, "flux-system");
        assert_eq!(config.default_controller_namespace, "flux-system");
        assert_eq!(config.ui.skin, "default");
        assert_eq!(
            config.connect_timeout_seconds,
            crate::kube::health::DEFAULT_CONNECT_TIMEOUT_SECS
        );
    }

    #[test]
    fn test_config_connect_timeout_defaults_when_absent() {
        // Older config files without the field should still deserialize.
        let yaml = "readOnly: false\n";
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            config.connect_timeout_seconds,
            crate::kube::health::DEFAULT_CONNECT_TIMEOUT_SECS
        );
    }

    #[test]
    fn test_config_serialization() {
        let config = Config::default();
        let yaml = serde_yaml::to_string(&config).unwrap();
        assert!(yaml.contains("readOnly"));
        assert!(yaml.contains("defaultNamespace"));
        assert!(yaml.contains("connectTimeoutSeconds"));
    }

    #[test]
    fn test_config_deserialization() {
        let yaml = r#"
readOnly: true
defaultNamespace: my-ns
connectTimeoutSeconds: 15
ui:
  skin: dracula
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert!(config.read_only);
        assert_eq!(config.default_namespace, "my-ns");
        assert_eq!(config.connect_timeout_seconds, 15);
        assert_eq!(config.ui.skin, "dracula");
    }
}
