//! Plugin manifest validation
//!
//! Validates plugin YAML files and provides helpful error messages for common issues.

use super::manifest::{DataSourceType, PluginManifest};
use super::{PluginError, PluginResult};
use std::collections::HashSet;

/// Plugin manifest validator
pub struct PluginValidator;

impl PluginValidator {
    /// Validate a plugin manifest
    pub fn validate(manifest: &PluginManifest) -> PluginResult<()> {
        Self::validate_name(&manifest.name)?;
        Self::validate_version(&manifest.version)?;
        Self::validate_source(&manifest.source)?;
        Self::validate_resources(&manifest.resources)?;
        Self::validate_columns(&manifest.columns)?;
        Self::validate_views(&manifest.views)?;
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
        match source.source_type {
            DataSourceType::KubernetesService => {
                Self::require_field(&source.service, "source.service", "kubernetes_service")?;
                Self::require_field(&source.namespace, "source.namespace", "kubernetes_service")?;
                Self::require_field(&source.port, "source.port", "kubernetes_service")?;
                Self::require_field(&source.path, "source.path", "kubernetes_service")?;
            }
            DataSourceType::KubernetesCrd => {
                Self::require_field(&source.kind, "source.kind", "kubernetes_crd")?;
                Self::require_field(&source.group, "source.group", "kubernetes_crd")?;
                Self::require_field(&source.version, "source.version", "kubernetes_crd")?;
            }
            DataSourceType::Http => {
                Self::require_field(&source.endpoint, "source.endpoint", "http")?;
            }
            DataSourceType::File => {
                Self::require_field(&source.file_path, "source.file_path", "file")?;
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

    /// Validate resources list
    fn validate_resources(resources: &[String]) -> PluginResult<()> {
        if resources.is_empty() {
            return Err(PluginError::ValidationError(
                "Plugin must specify at least one resource type".to_string(),
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
        let num_part = if duration.ends_with("ms") {
            &duration[..duration.len() - 2]
        } else {
            &duration[..duration.len() - 1]
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
    use crate::plugins::manifest::{ColumnConfig, DataSourceConfig, Renderer, ViewConfig};

    fn create_valid_manifest() -> PluginManifest {
        PluginManifest {
            name: "test-plugin".to_string(),
            version: "1.0.0".to_string(),
            enabled: true,
            description: None,
            source: DataSourceConfig {
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
            },
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
        }
    }

    #[test]
    fn test_valid_manifest() {
        let manifest = create_valid_manifest();
        assert!(PluginValidator::validate(&manifest).is_ok());
    }

    #[test]
    fn test_empty_name() {
        let mut manifest = create_valid_manifest();
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
        let mut manifest = create_valid_manifest();
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
        let mut manifest = create_valid_manifest();
        manifest.source.service = None;
        let result = PluginValidator::validate(&manifest);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("source.service"));
    }

    #[test]
    fn test_invalid_duration() {
        let mut manifest = create_valid_manifest();
        manifest.source.refresh_interval = Some("invalid".to_string());
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
    fn test_empty_resources() {
        let mut manifest = create_valid_manifest();
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
        let mut manifest = create_valid_manifest();
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
        let mut manifest = create_valid_manifest();
        manifest.columns[0].width = 0;
        let result = PluginValidator::validate(&manifest);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("invalid width"));
    }

    #[test]
    fn test_invalid_keybinding() {
        let mut manifest = create_valid_manifest();
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
        let mut manifest = create_valid_manifest();
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
}
