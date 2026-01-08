//! Plugin manifest schema
//!
//! Defines the structure of plugin YAML files that configure external data sources,
//! column extensions, and custom views.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Plugin manifest - root structure of a plugin YAML file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Plugin name (must be unique)
    pub name: String,

    /// Plugin version (semver recommended)
    pub version: String,

    /// Whether this plugin is enabled
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Data source configuration
    pub source: DataSourceConfig,

    /// Resources this plugin enhances
    pub resources: Vec<String>,

    /// Column extensions for resource list view
    #[serde(default)]
    pub columns: Vec<ColumnConfig>,

    /// Custom views
    #[serde(default)]
    pub views: Vec<ViewConfig>,

    /// Column mappings for custom views that display resource lists
    #[serde(default)]
    pub view_columns: HashMap<String, Vec<ColumnConfig>>,
}

/// Data source configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSourceConfig {
    /// Data source type
    #[serde(rename = "type")]
    pub source_type: DataSourceType,

    /// Kubernetes Service configuration (when type = kubernetes_service)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,

    /// Kubernetes CRD configuration (when type = kubernetes_crd)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_path: Option<String>,

    /// HTTP configuration (when type = http)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<AuthConfig>,

    /// File configuration (when type = file)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,

    /// Refresh interval (e.g., "30s", "1m")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_interval: Option<String>,

    /// Request timeout (e.g., "5s")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<String>,
}

/// Data source type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DataSourceType {
    /// Kubernetes Service endpoint
    KubernetesService,

    /// Kubernetes CRD
    KubernetesCrd,

    /// External HTTP API
    Http,

    /// Local file
    File,
}

/// Authentication configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    /// Auth type
    #[serde(rename = "type")]
    pub auth_type: AuthType,

    /// Token from environment variable (for bearer and api_key)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_env: Option<String>,

    /// Username (for basic auth)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,

    /// Password from environment variable (for basic auth)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password_env: Option<String>,

    /// Header name (for api_key)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
}

/// Authentication type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthType {
    None,
    Bearer,
    Basic,
    ApiKey,
}

/// Column configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnConfig {
    /// Column name (must be unique within plugin)
    pub name: String,

    /// JSONPath to extract value from plugin data
    pub path: String,

    /// Column width in characters
    pub width: u16,

    /// Whether this column is enabled
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Renderer to use for formatting
    #[serde(default)]
    pub renderer: Renderer,
}

/// Built-in renderers
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Renderer {
    /// Plain text (default)
    #[default]
    Text,

    /// Issue badge (-, ⚠ 1, 🔴 2)
    IssueBadge,

    /// Percentage bar ([████░░] 80%)
    PercentageBar,

    /// Duration (2h 30m)
    Duration,
}

/// View configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewConfig {
    /// View name (must be unique within plugin)
    pub name: String,

    /// Keybinding to activate view (e.g., ":agent")
    pub keybinding: String,

    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// Default values
fn default_enabled() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_manifest() {
        let yaml = r#"
name: test-plugin
version: 1.0.0
source:
  type: kubernetes_service
  service: test-service
  namespace: default
  port: 8080
  path: /api/data
resources:
  - Deployment
columns:
  - name: status
    path: .status
    width: 10
"#;

        let manifest: PluginManifest = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(manifest.name, "test-plugin");
        assert_eq!(manifest.version, "1.0.0");
        assert!(manifest.enabled);
        assert_eq!(
            manifest.source.source_type,
            DataSourceType::KubernetesService
        );
        assert_eq!(manifest.source.service, Some("test-service".to_string()));
        assert_eq!(manifest.resources, vec!["Deployment"]);
        assert_eq!(manifest.columns.len(), 1);
        assert_eq!(manifest.columns[0].name, "status");
    }

    #[test]
    fn test_parse_full_manifest() {
        let yaml = r#"
name: confighub-agent
version: 1.0.0
enabled: true
description: "ConfigHub Agent integration"
source:
  type: kubernetes_service
  service: confighub-agent
  namespace: confighub-system
  port: 8080
  path: /api/map
  refresh_interval: 30s
  timeout: 5s
resources:
  - Kustomization
  - HelmRelease
columns:
  - name: owner
    path: .ownership.owner
    width: 12
    enabled: true
    description: "Resource owner"
    renderer: text
  - name: issues
    path: .ccve.count
    width: 8
    enabled: true
    renderer: issue_badge
views:
  - name: agent_detail
    keybinding: ":agent"
    description: "Show ConfigHub Agent details"
"#;

        let manifest: PluginManifest = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(manifest.name, "confighub-agent");
        assert_eq!(
            manifest.description,
            Some("ConfigHub Agent integration".to_string())
        );
        assert_eq!(manifest.columns.len(), 2);
        assert_eq!(manifest.views.len(), 1);
        assert_eq!(manifest.views[0].keybinding, ":agent");
    }

    #[test]
    fn test_parse_http_source() {
        let yaml = r#"
name: http-plugin
version: 1.0.0
source:
  type: http
  endpoint: https://api.example.com/data
  auth:
    type: bearer
    token_env: API_TOKEN
  refresh_interval: 60s
resources:
  - Deployment
columns: []
"#;

        let manifest: PluginManifest = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(manifest.source.source_type, DataSourceType::Http);
        assert_eq!(
            manifest.source.endpoint,
            Some("https://api.example.com/data".to_string())
        );
        assert!(manifest.source.auth.is_some());
        let auth = manifest.source.auth.unwrap();
        assert_eq!(auth.auth_type, AuthType::Bearer);
        assert_eq!(auth.token_env, Some("API_TOKEN".to_string()));
    }

    #[test]
    fn test_parse_crd_source() {
        let yaml = r#"
name: crd-plugin
version: 1.0.0
source:
  type: kubernetes_crd
  kind: ConfigHubData
  group: confighub.com
  version: v1
  namespace: confighub-system
  name: cluster-data
  data_path: .status.data
resources:
  - Kustomization
columns: []
"#;

        let manifest: PluginManifest = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(manifest.source.source_type, DataSourceType::KubernetesCrd);
        assert_eq!(manifest.source.kind, Some("ConfigHubData".to_string()));
        assert_eq!(manifest.source.group, Some("confighub.com".to_string()));
        assert_eq!(manifest.source.version, Some("v1".to_string()));
        assert_eq!(manifest.source.data_path, Some(".status.data".to_string()));
    }
}
