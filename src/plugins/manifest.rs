//! Plugin manifest schema
//!
//! Defines the structure of plugin YAML files that configure external data sources,
//! column extensions, custom views, and watched CRD resources.

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

    /// Data source configuration (optional - only needed for column enrichment)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<DataSourceConfig>,

    /// Resources this plugin enhances with extra columns (optional)
    #[serde(default)]
    pub resources: Vec<String>,

    /// Column extensions for resource list view (enriches existing Flux views)
    #[serde(default)]
    pub columns: Vec<ColumnConfig>,

    /// Custom views (legacy - prefer watched_resources for new plugins)
    #[serde(default)]
    pub views: Vec<ViewConfig>,

    /// Column mappings for custom views that display resource lists
    #[serde(default)]
    pub view_columns: HashMap<String, Vec<ColumnConfig>>,

    /// Watched CRD resources - creates new resource views with full TUI support
    #[serde(default)]
    pub watched_resources: Vec<WatchedResourceConfig>,
}

/// Type of watched resource - determines how data is fetched
///
/// This enum is extensible for future data source types.
/// Currently only `kubernetes_crd` is implemented.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WatchedResourceType {
    /// Watch a Kubernetes CRD via kube-rs watcher
    /// Requires: kind, group, version, plural
    KubernetesCrd,
    // Future types (not yet implemented):
    // /// Poll an HTTP API endpoint
    // HttpApi,
    // /// Stream from a gRPC service
    // Grpc,
}

impl std::fmt::Display for WatchedResourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WatchedResourceType::KubernetesCrd => write!(f, "kubernetes_crd"),
        }
    }
}

/// Configuration for watching a resource and displaying it as a resource view
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchedResourceConfig {
    /// Resource type - determines how data is fetched
    /// Currently only "kubernetes_crd" is supported
    #[serde(rename = "type")]
    pub resource_type: WatchedResourceType,

    /// CRD kind (e.g., "Application") - required for kubernetes_crd
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,

    /// API group (e.g., "argoproj.io") - required for kubernetes_crd
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,

    /// API version (e.g., "v1alpha1") - required for kubernetes_crd
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// Plural name for API calls (e.g., "applications") - required for kubernetes_crd
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plural: Option<String>,

    /// Command/keybinding to access this view (e.g., ":argo")
    pub command: String,

    /// Display name shown in header (defaults to kind for kubernetes_crd)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,

    /// Enable YAML view ('y' key) - default true
    #[serde(default = "default_enabled")]
    pub supports_yaml: bool,

    /// Enable describe view ('d' key) - default true
    #[serde(default = "default_enabled")]
    pub supports_describe: bool,

    /// Enable logs view ('l' key) - default false (only for pod-like resources)
    #[serde(default)]
    pub supports_logs: bool,

    /// Column definitions for the list view
    pub columns: Vec<ColumnConfig>,

    /// Status field extraction configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<StatusConfig>,
}

impl WatchedResourceConfig {
    /// Get the display name, defaulting to kind if not specified
    pub fn display_name(&self) -> &str {
        self.display_name
            .as_deref()
            .or(self.kind.as_deref())
            .unwrap_or("Unknown")
    }

    /// Get the API version string (group/version) for kubernetes_crd type
    pub fn api_version(&self) -> Option<String> {
        match self.resource_type {
            WatchedResourceType::KubernetesCrd => {
                let group = self.group.as_deref().unwrap_or("");
                let version = self.version.as_deref()?;
                if group.is_empty() {
                    Some(version.to_string())
                } else {
                    Some(format!("{}/{}", group, version))
                }
            }
        }
    }

    /// Get the kind for kubernetes_crd type
    pub fn kind(&self) -> Option<&str> {
        self.kind.as_deref()
    }

    /// Get the group for kubernetes_crd type
    pub fn group(&self) -> Option<&str> {
        self.group.as_deref()
    }

    /// Get the version for kubernetes_crd type
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    /// Get the plural for kubernetes_crd type
    pub fn plural(&self) -> Option<&str> {
        self.plural.as_deref()
    }
}

/// Configuration for extracting status fields from watched resources
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusConfig {
    /// JSONPath to the ready indicator field
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ready_path: Option<String>,

    /// Value that indicates "ready" state (e.g., "Healthy", "True")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ready_value: Option<String>,

    /// JSONPath to the suspended indicator field
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suspended_path: Option<String>,

    /// If true, resource is considered suspended when suspended_path field is missing
    #[serde(default)]
    pub suspended_when_missing: bool,

    /// JSONPath to the status message field
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_path: Option<String>,

    /// JSONPath to revision/version field
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision_path: Option<String>,
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

impl DataSourceType {
    /// Get the string representation of the data source type
    /// This matches the serde serialization format (snake_case)
    pub fn as_str(&self) -> &'static str {
        match self {
            DataSourceType::KubernetesService => "kubernetes_service",
            DataSourceType::KubernetesCrd => "kubernetes_crd",
            DataSourceType::Http => "http",
            DataSourceType::File => "file",
        }
    }
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

    /// Status badge - colored based on value (Ready=green, Degraded=yellow, etc.)
    StatusBadge,

    /// Age - time since timestamp (e.g., "5m", "2h", "3d")
    Age,

    /// Boolean - shows checkmark or X
    Boolean,
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
    fn test_parse_minimal_manifest_with_source() {
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
        let source = manifest.source.unwrap();
        assert_eq!(source.source_type, DataSourceType::KubernetesService);
        assert_eq!(source.service, Some("test-service".to_string()));
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
        let source = manifest.source.unwrap();
        assert_eq!(source.source_type, DataSourceType::Http);
        assert_eq!(
            source.endpoint,
            Some("https://api.example.com/data".to_string())
        );
        assert!(source.auth.is_some());
        let auth = source.auth.unwrap();
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
        let source = manifest.source.unwrap();
        assert_eq!(source.source_type, DataSourceType::KubernetesCrd);
        assert_eq!(source.kind, Some("ConfigHubData".to_string()));
        assert_eq!(source.group, Some("confighub.com".to_string()));
        assert_eq!(source.version, Some("v1".to_string()));
        assert_eq!(source.data_path, Some(".status.data".to_string()));
    }

    #[test]
    fn test_parse_watched_resources_only() {
        let yaml = r#"
name: argocd
version: 1.0.0
enabled: true

watched_resources:
  - type: kubernetes_crd
    kind: Application
    group: argoproj.io
    version: v1alpha1
    plural: applications
    command: ":argo"
    display_name: "Argo Apps"
    supports_yaml: true
    supports_describe: true
    supports_logs: false
    columns:
      - name: NAME
        path: .metadata.name
        width: 25
      - name: SYNC
        path: .status.sync.status
        width: 10
        renderer: status_badge
    status:
      ready_path: .status.health.status
      ready_value: "Healthy"
      message_path: .status.conditions[0].message
"#;

        let manifest: PluginManifest = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(manifest.name, "argocd");
        assert!(manifest.source.is_none());
        assert_eq!(manifest.watched_resources.len(), 1);

        let argo = &manifest.watched_resources[0];
        assert_eq!(argo.resource_type, WatchedResourceType::KubernetesCrd);
        assert_eq!(argo.kind, Some("Application".to_string()));
        assert_eq!(argo.group, Some("argoproj.io".to_string()));
        assert_eq!(argo.version, Some("v1alpha1".to_string()));
        assert_eq!(argo.plural, Some("applications".to_string()));
        assert_eq!(argo.command, ":argo");
        assert_eq!(argo.display_name(), "Argo Apps");
        assert!(argo.supports_yaml);
        assert!(argo.supports_describe);
        assert!(!argo.supports_logs);
        assert_eq!(argo.columns.len(), 2);
        assert_eq!(argo.columns[1].renderer, Renderer::StatusBadge);

        let status = argo.status.as_ref().unwrap();
        assert_eq!(status.ready_path, Some(".status.health.status".to_string()));
        assert_eq!(status.ready_value, Some("Healthy".to_string()));
    }

    #[test]
    fn test_parse_mixed_plugin() {
        let yaml = r#"
name: argocd-full
version: 1.0.0
enabled: true

# Watch Argo CRDs
watched_resources:
  - type: kubernetes_crd
    kind: Application
    group: argoproj.io
    version: v1alpha1
    plural: applications
    command: ":argo"
    columns:
      - name: NAME
        path: .metadata.name
        width: 25

# Also enrich Flux resources with Argo data
source:
  type: kubernetes_service
  service: argocd-server
  namespace: argocd
  port: 8080
  path: /api/v1/applications

resources:
  - Kustomization
  - HelmRelease

columns:
  - name: argo-sync
    path: .status.sync.status
    width: 12
"#;

        let manifest: PluginManifest = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(manifest.name, "argocd-full");

        // Has watched_resources
        assert_eq!(manifest.watched_resources.len(), 1);
        assert_eq!(manifest.watched_resources[0].command, ":argo");
        assert_eq!(
            manifest.watched_resources[0].resource_type,
            WatchedResourceType::KubernetesCrd
        );

        // Also has source for enrichment
        assert!(manifest.source.is_some());
        assert_eq!(manifest.resources.len(), 2);
        assert_eq!(manifest.columns.len(), 1);
    }

    #[test]
    fn test_watched_resource_api_version() {
        let config = WatchedResourceConfig {
            resource_type: WatchedResourceType::KubernetesCrd,
            kind: Some("Application".to_string()),
            group: Some("argoproj.io".to_string()),
            version: Some("v1alpha1".to_string()),
            plural: Some("applications".to_string()),
            command: ":argo".to_string(),
            display_name: None,
            supports_yaml: true,
            supports_describe: true,
            supports_logs: false,
            columns: vec![],
            status: None,
        };

        assert_eq!(
            config.api_version(),
            Some("argoproj.io/v1alpha1".to_string())
        );
        assert_eq!(config.display_name(), "Application");
    }

    #[test]
    fn test_watched_resource_core_api() {
        // Core API resources have empty group
        let config = WatchedResourceConfig {
            resource_type: WatchedResourceType::KubernetesCrd,
            kind: Some("ConfigMap".to_string()),
            group: Some("".to_string()),
            version: Some("v1".to_string()),
            plural: Some("configmaps".to_string()),
            command: ":cm".to_string(),
            display_name: Some("ConfigMaps".to_string()),
            supports_yaml: true,
            supports_describe: true,
            supports_logs: false,
            columns: vec![],
            status: None,
        };

        assert_eq!(config.api_version(), Some("v1".to_string()));
        assert_eq!(config.display_name(), "ConfigMaps");
    }

    #[test]
    fn test_status_config_defaults() {
        let yaml = r#"
name: minimal-watched
version: 1.0.0

watched_resources:
  - type: kubernetes_crd
    kind: MyResource
    group: example.com
    version: v1
    plural: myresources
    command: ":my"
    columns:
      - name: NAME
        path: .metadata.name
        width: 20
"#;

        let manifest: PluginManifest = serde_yaml::from_str(yaml).unwrap();
        let watched = &manifest.watched_resources[0];

        // Defaults
        assert_eq!(watched.resource_type, WatchedResourceType::KubernetesCrd);
        assert!(watched.supports_yaml);
        assert!(watched.supports_describe);
        assert!(!watched.supports_logs);
        assert!(watched.status.is_none());
        assert!(watched.display_name.is_none());
        assert_eq!(watched.display_name(), "MyResource"); // Falls back to kind
    }
}
