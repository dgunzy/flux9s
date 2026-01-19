//! Plugin manifest validation
//!
//! Validates plugin YAML files and provides helpful error messages for common issues.

use super::manifest::{DataSourceType, PluginManifest, WatchedResourceConfig};
use super::{PluginError, PluginResult};
use std::collections::HashSet;

/// Plugin manifest validator
pub struct PluginValidator;

impl PluginValidator {
    /// Validate a plugin manifest
    pub fn validate(manifest: &PluginManifest) -> PluginResult<()> {
        Self::validate_name(&manifest.name)?;
        Self::validate_version(&manifest.version)?;

        // A plugin must have either a source (for column enrichment) or watched_resources (for new views)
        let has_source = manifest.source.is_some();
        let has_watched = !manifest.watched_resources.is_empty();

        if !has_source && !has_watched {
            return Err(PluginError::ValidationError(
                "Plugin must have either 'source' (for column enrichment) or 'watched_resources' (for new views)".to_string(),
            ));
        }

        // Validate source if present
        if let Some(ref source) = manifest.source {
            Self::validate_source(source)?;
            // Resources are required when source is present
            Self::validate_resources(&manifest.resources)?;
        }

        Self::validate_columns(&manifest.columns)?;
        Self::validate_views(&manifest.views)?;
        Self::validate_watched_resources(&manifest.watched_resources)?;

        Ok(())
    }

    /// Validate plugin name
    fn validate_name(name: &str) -> PluginResult<()> {
        if name.is_empty() {
            return Err(PluginError::ValidationError(
                "Plugin name cannot be empty".to_string(),
            ));
        }

        // Name should be alphanumeric with hyphens/underscores
        if !name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            return Err(PluginError::ValidationError(format!(
                "Plugin name '{}' contains invalid characters. Use only alphanumeric, hyphens, and underscores",
                name
            )));
        }

        Ok(())
    }

    /// Validate version string
    fn validate_version(version: &str) -> PluginResult<()> {
        if version.is_empty() {
            return Err(PluginError::ValidationError(
                "Plugin version cannot be empty".to_string(),
            ));
        }
        Ok(())
    }

    /// Validate data source configuration
    fn validate_source(source: &super::manifest::DataSourceConfig) -> PluginResult<()> {
        let source_type_str = source.source_type.as_str();
        match source.source_type {
            DataSourceType::KubernetesService => {
                Self::require_field(&source.service, "source.service", source_type_str)?;
                Self::require_field(&source.namespace, "source.namespace", source_type_str)?;
                Self::require_field(&source.port, "source.port", source_type_str)?;
                Self::require_field(&source.path, "source.path", source_type_str)?;
            }
            DataSourceType::KubernetesCrd => {
                Self::require_field(&source.kind, "source.kind", source_type_str)?;
                Self::require_field(&source.group, "source.group", source_type_str)?;
                Self::require_field(&source.version, "source.version", source_type_str)?;
            }
            DataSourceType::Http => {
                Self::require_field(&source.endpoint, "source.endpoint", source_type_str)?;
            }
            DataSourceType::File => {
                Self::require_field(&source.file_path, "source.file_path", source_type_str)?;
            }
        }

        // Validate refresh_interval if present
        if let Some(interval) = &source.refresh_interval {
            Self::validate_duration(interval, "refresh_interval")?;
        }

        // Validate timeout if present
        if let Some(timeout) = &source.timeout {
            Self::validate_duration(timeout, "timeout")?;
        }

        Ok(())
    }

    /// Validate resources list (required when source is present)
    fn validate_resources(resources: &[String]) -> PluginResult<()> {
        if resources.is_empty() {
            return Err(PluginError::ValidationError(
                "Plugin with 'source' must specify at least one resource type in 'resources'"
                    .to_string(),
            ));
        }

        for resource in resources {
            if resource.is_empty() {
                return Err(PluginError::ValidationError(
                    "Resource type cannot be empty".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Validate watched_resources configuration
    fn validate_watched_resources(watched: &[WatchedResourceConfig]) -> PluginResult<()> {
        use super::manifest::WatchedResourceType;

        let mut seen_commands = HashSet::new();
        let mut seen_kinds = HashSet::new();

        for resource in watched {
            // Validate type-specific required fields
            match resource.resource_type {
                WatchedResourceType::KubernetesCrd => {
                    Self::validate_kubernetes_crd_fields(resource)?;
                }
            }

            // Validate command (required for all types)
            if resource.command.is_empty() {
                return Err(PluginError::ValidationError(
                    "watched_resources: 'command' cannot be empty".to_string(),
                ));
            }

            // Command should start with ':'
            if !resource.command.starts_with(':') {
                return Err(PluginError::ValidationError(format!(
                    "watched_resources: command '{}' should start with ':' (e.g., ':argo')",
                    resource.command
                )));
            }

            // Check for duplicate commands
            if !seen_commands.insert(&resource.command) {
                return Err(PluginError::ValidationError(format!(
                    "watched_resources: duplicate command '{}' in plugin",
                    resource.command
                )));
            }

            // Check for duplicate resource definitions (type-specific)
            match resource.resource_type {
                WatchedResourceType::KubernetesCrd => {
                    let kind_key = format!(
                        "{}/{}/{}",
                        resource.group.as_deref().unwrap_or(""),
                        resource.version.as_deref().unwrap_or(""),
                        resource.kind.as_deref().unwrap_or("")
                    );
                    if !seen_kinds.insert(kind_key.clone()) {
                        return Err(PluginError::ValidationError(format!(
                            "watched_resources: duplicate resource definition '{}'",
                            kind_key
                        )));
                    }
                } // Future types would handle duplicate checking here
            }

            // Validate columns (required for watched resources)
            let display = resource.display_name();
            if resource.columns.is_empty() {
                return Err(PluginError::ValidationError(format!(
                    "watched_resources: '{}' must have at least one column",
                    display
                )));
            }

            // Validate columns
            Self::validate_columns(&resource.columns)?;
        }

        Ok(())
    }

    /// Validate kubernetes_crd specific fields
    fn validate_kubernetes_crd_fields(resource: &WatchedResourceConfig) -> PluginResult<()> {
        // kind is required
        match &resource.kind {
            None => {
                return Err(PluginError::ValidationError(
                    "watched_resources: 'kind' is required for type 'kubernetes_crd'".to_string(),
                ));
            }
            Some(kind) if kind.is_empty() => {
                return Err(PluginError::ValidationError(
                    "watched_resources: 'kind' cannot be empty for type 'kubernetes_crd'"
                        .to_string(),
                ));
            }
            _ => {}
        }

        // group can be empty for core API resources, but must be present
        if resource.group.is_none() {
            return Err(PluginError::ValidationError(
                "watched_resources: 'group' is required for type 'kubernetes_crd' (use empty string for core API)".to_string(),
            ));
        }
        if resource.group.as_deref() == Some("") {
            tracing::debug!(
                "watched_resources: '{}' has empty group (core API resource)",
                resource.kind.as_deref().unwrap_or("unknown")
            );
        }

        // version is required
        match &resource.version {
            None => {
                return Err(PluginError::ValidationError(
                    "watched_resources: 'version' is required for type 'kubernetes_crd'"
                        .to_string(),
                ));
            }
            Some(version) if version.is_empty() => {
                return Err(PluginError::ValidationError(
                    "watched_resources: 'version' cannot be empty for type 'kubernetes_crd'"
                        .to_string(),
                ));
            }
            _ => {}
        }

        // plural is required
        match &resource.plural {
            None => {
                return Err(PluginError::ValidationError(
                    "watched_resources: 'plural' is required for type 'kubernetes_crd'".to_string(),
                ));
            }
            Some(plural) if plural.is_empty() => {
                return Err(PluginError::ValidationError(
                    "watched_resources: 'plural' cannot be empty for type 'kubernetes_crd'"
                        .to_string(),
                ));
            }
            _ => {}
        }

        Ok(())
    }

    /// Validate columns configuration
    fn validate_columns(columns: &[super::manifest::ColumnConfig]) -> PluginResult<()> {
        let mut seen_names = HashSet::new();

        for column in columns {
            // Check for duplicate column names
            if !seen_names.insert(&column.name) {
                return Err(PluginError::ValidationError(format!(
                    "Duplicate column name '{}' in plugin",
                    column.name
                )));
            }

            // Validate column name
            if column.name.is_empty() {
                return Err(PluginError::ValidationError(
                    "Column name cannot be empty".to_string(),
                ));
            }

            // Validate path
            if column.path.is_empty() {
                return Err(PluginError::ValidationError(format!(
                    "Column '{}' has empty path",
                    column.name
                )));
            }

            // Validate width
            if column.width == 0 {
                return Err(PluginError::ValidationError(format!(
                    "Column '{}' has invalid width: 0",
                    column.name
                )));
            }
        }

        Ok(())
    }

    /// Validate views configuration
    fn validate_views(views: &[super::manifest::ViewConfig]) -> PluginResult<()> {
        let mut seen_names = HashSet::new();
        let mut seen_keybindings = HashSet::new();

        for view in views {
            // Check for duplicate view names
            if !seen_names.insert(&view.name) {
                return Err(PluginError::ValidationError(format!(
                    "Duplicate view name '{}' in plugin",
                    view.name
                )));
            }

            // Check for duplicate keybindings
            if !seen_keybindings.insert(&view.keybinding) {
                return Err(PluginError::ValidationError(format!(
                    "Duplicate keybinding '{}' in plugin",
                    view.keybinding
                )));
            }

            // Validate view name
            if view.name.is_empty() {
                return Err(PluginError::ValidationError(
                    "View name cannot be empty".to_string(),
                ));
            }

            // Validate keybinding
            if view.keybinding.is_empty() {
                return Err(PluginError::ValidationError(format!(
                    "View '{}' has empty keybinding",
                    view.name
                )));
            }

            // Keybinding should start with ':'
            if !view.keybinding.starts_with(':') {
                return Err(PluginError::ValidationError(format!(
                    "View '{}' keybinding '{}' should start with ':' (e.g., ':agent')",
                    view.name, view.keybinding
                )));
            }
        }

        Ok(())
    }

    /// Validate duration string (e.g., "30s", "1m", "1h")
    fn validate_duration(duration: &str, field_name: &str) -> PluginResult<()> {
        // Simple validation - check format
        let valid = duration.ends_with('s')
            || duration.ends_with('m')
            || duration.ends_with('h')
            || duration.ends_with("ms");

        if !valid {
            return Err(PluginError::ValidationError(format!(
                "Invalid {} '{}'. Expected format: '30s', '1m', '1h'",
                field_name, duration
            )));
        }

        // Check that everything before the suffix is a number
        let num_part = if let Some(stripped) = duration.strip_suffix("ms") {
            stripped
        } else if let Some(stripped) = duration.strip_suffix("s") {
            stripped
        } else if let Some(stripped) = duration.strip_suffix("m") {
            stripped
        } else if let Some(stripped) = duration.strip_suffix("h") {
            stripped
        } else {
            return Err(PluginError::ValidationError(format!(
                "Invalid {} '{}'. Expected format: '30s', '1m', '1h'",
                field_name, duration
            )));
        };

        if num_part.parse::<u64>().is_err() {
            return Err(PluginError::ValidationError(format!(
                "Invalid {} '{}'. Duration value must be a number",
                field_name, duration
            )));
        }

        Ok(())
    }

    /// Require a field to be present
    fn require_field<T>(
        field: &Option<T>,
        field_name: &str,
        source_type: &str,
    ) -> PluginResult<()> {
        if field.is_none() {
            return Err(PluginError::ValidationError(format!(
                "Missing required field '{}' for source type '{}'",
                field_name, source_type
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::manifest::{
        ColumnConfig, DataSourceConfig, Renderer, ViewConfig, WatchedResourceConfig,
    };

    fn create_valid_manifest_with_source() -> PluginManifest {
        PluginManifest {
            name: "test-plugin".to_string(),
            version: "1.0.0".to_string(),
            enabled: true,
            description: None,
            source: Some(DataSourceConfig {
                source_type: DataSourceType::KubernetesService,
                service: Some("test-service".to_string()),
                namespace: Some("default".to_string()),
                port: Some(8080),
                path: Some("/api/data".to_string()),
                kind: None,
                group: None,
                version: None,
                name: None,
                data_path: None,
                endpoint: None,
                auth: None,
                file_path: None,
                refresh_interval: Some("30s".to_string()),
                timeout: Some("5s".to_string()),
            }),
            resources: vec!["Deployment".to_string()],
            columns: vec![ColumnConfig {
                name: "status".to_string(),
                path: ".status".to_string(),
                width: 10,
                enabled: true,
                description: None,
                renderer: Renderer::Text,
            }],
            views: vec![],
            view_columns: Default::default(),
            watched_resources: vec![],
        }
    }

    fn create_valid_manifest_with_watched() -> PluginManifest {
        use crate::plugins::manifest::WatchedResourceType;
        PluginManifest {
            name: "watched-plugin".to_string(),
            version: "1.0.0".to_string(),
            enabled: true,
            description: None,
            source: None,
            resources: vec![],
            columns: vec![],
            views: vec![],
            view_columns: Default::default(),
            watched_resources: vec![WatchedResourceConfig {
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
                columns: vec![ColumnConfig {
                    name: "NAME".to_string(),
                    path: ".metadata.name".to_string(),
                    width: 20,
                    enabled: true,
                    description: None,
                    renderer: Renderer::Text,
                }],
                status: None,
            }],
        }
    }

    #[test]
    fn test_valid_manifest_with_source() {
        let manifest = create_valid_manifest_with_source();
        assert!(PluginValidator::validate(&manifest).is_ok());
    }

    #[test]
    fn test_valid_manifest_with_watched() {
        let manifest = create_valid_manifest_with_watched();
        assert!(PluginValidator::validate(&manifest).is_ok());
    }

    #[test]
    fn test_manifest_requires_source_or_watched() {
        let manifest = PluginManifest {
            name: "empty-plugin".to_string(),
            version: "1.0.0".to_string(),
            enabled: true,
            description: None,
            source: None,
            resources: vec![],
            columns: vec![],
            views: vec![],
            view_columns: Default::default(),
            watched_resources: vec![],
        };
        let result = PluginValidator::validate(&manifest);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("must have either"));
    }

    #[test]
    fn test_empty_name() {
        let mut manifest = create_valid_manifest_with_source();
        manifest.name = "".to_string();
        let result = PluginValidator::validate(&manifest);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("name cannot be empty")
        );
    }

    #[test]
    fn test_invalid_name() {
        let mut manifest = create_valid_manifest_with_source();
        manifest.name = "test@plugin!".to_string();
        let result = PluginValidator::validate(&manifest);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("invalid characters")
        );
    }

    #[test]
    fn test_missing_service_field() {
        let mut manifest = create_valid_manifest_with_source();
        if let Some(ref mut source) = manifest.source {
            source.service = None;
        }
        let result = PluginValidator::validate(&manifest);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("source.service"));
    }

    #[test]
    fn test_invalid_duration() {
        let mut manifest = create_valid_manifest_with_source();
        if let Some(ref mut source) = manifest.source {
            source.refresh_interval = Some("invalid".to_string());
        }
        let result = PluginValidator::validate(&manifest);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid refresh_interval")
        );
    }

    #[test]
    fn test_empty_resources_with_source() {
        let mut manifest = create_valid_manifest_with_source();
        manifest.resources = vec![];
        let result = PluginValidator::validate(&manifest);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("at least one resource")
        );
    }

    #[test]
    fn test_duplicate_column_names() {
        let mut manifest = create_valid_manifest_with_source();
        manifest.columns = vec![
            ColumnConfig {
                name: "status".to_string(),
                path: ".status".to_string(),
                width: 10,
                enabled: true,
                description: None,
                renderer: Renderer::Text,
            },
            ColumnConfig {
                name: "status".to_string(),
                path: ".status2".to_string(),
                width: 10,
                enabled: true,
                description: None,
                renderer: Renderer::Text,
            },
        ];
        let result = PluginValidator::validate(&manifest);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Duplicate column name")
        );
    }

    #[test]
    fn test_zero_width_column() {
        let mut manifest = create_valid_manifest_with_source();
        manifest.columns[0].width = 0;
        let result = PluginValidator::validate(&manifest);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("invalid width"));
    }

    #[test]
    fn test_invalid_keybinding() {
        let mut manifest = create_valid_manifest_with_source();
        manifest.views = vec![ViewConfig {
            name: "test_view".to_string(),
            keybinding: "agent".to_string(), // Missing ':'
            description: None,
        }];
        let result = PluginValidator::validate(&manifest);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("should start with ':'")
        );
    }

    #[test]
    fn test_duplicate_view_keybindings() {
        let mut manifest = create_valid_manifest_with_source();
        manifest.views = vec![
            ViewConfig {
                name: "view1".to_string(),
                keybinding: ":agent".to_string(),
                description: None,
            },
            ViewConfig {
                name: "view2".to_string(),
                keybinding: ":agent".to_string(),
                description: None,
            },
        ];
        let result = PluginValidator::validate(&manifest);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Duplicate keybinding")
        );
    }

    #[test]
    fn test_valid_durations() {
        let durations = vec!["30s", "1m", "1h", "500ms", "60s"];
        for duration in durations {
            assert!(PluginValidator::validate_duration(duration, "test").is_ok());
        }
    }

    #[test]
    fn test_invalid_durations() {
        let durations = vec!["30", "1x", "invalid", "s30", ""];
        for duration in durations {
            assert!(PluginValidator::validate_duration(duration, "test").is_err());
        }
    }

    #[test]
    fn test_watched_resource_missing_command() {
        let mut manifest = create_valid_manifest_with_watched();
        manifest.watched_resources[0].command = "".to_string();
        let result = PluginValidator::validate(&manifest);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("'command' cannot be empty")
        );
    }

    #[test]
    fn test_watched_resource_invalid_command() {
        let mut manifest = create_valid_manifest_with_watched();
        manifest.watched_resources[0].command = "argo".to_string(); // Missing ':'
        let result = PluginValidator::validate(&manifest);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("should start with ':'")
        );
    }

    #[test]
    fn test_watched_resource_duplicate_commands() {
        use crate::plugins::manifest::WatchedResourceType;
        let mut manifest = create_valid_manifest_with_watched();
        manifest.watched_resources.push(WatchedResourceConfig {
            resource_type: WatchedResourceType::KubernetesCrd,
            kind: Some("AppProject".to_string()),
            group: Some("argoproj.io".to_string()),
            version: Some("v1alpha1".to_string()),
            plural: Some("appprojects".to_string()),
            command: ":argo".to_string(), // Duplicate!
            display_name: None,
            supports_yaml: true,
            supports_describe: true,
            supports_logs: false,
            columns: vec![ColumnConfig {
                name: "NAME".to_string(),
                path: ".metadata.name".to_string(),
                width: 20,
                enabled: true,
                description: None,
                renderer: Renderer::Text,
            }],
            status: None,
        });
        let result = PluginValidator::validate(&manifest);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("duplicate command")
        );
    }

    #[test]
    fn test_watched_resource_empty_columns() {
        let mut manifest = create_valid_manifest_with_watched();
        manifest.watched_resources[0].columns = vec![];
        let result = PluginValidator::validate(&manifest);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("at least one column")
        );
    }
}
