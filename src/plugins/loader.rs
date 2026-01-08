//! Plugin loader
//!
//! Scans the plugins directory and loads plugin manifests with validation and conflict detection.

use super::manifest::PluginManifest;
use super::validator::PluginValidator;
use super::{PluginError, PluginResult};
use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Plugin loader
pub struct PluginLoader {
    plugins_dir: PathBuf,
}

impl PluginLoader {
    /// Create a new plugin loader with the default plugins directory
    pub fn new() -> Result<Self> {
        let plugins_dir = Self::get_plugins_dir()?;
        Ok(Self { plugins_dir })
    }

    /// Create a plugin loader with a custom plugins directory
    pub fn with_dir(plugins_dir: PathBuf) -> Self {
        Self { plugins_dir }
    }

    /// Get the default plugins directory
    pub fn get_plugins_dir() -> Result<PathBuf> {
        let config_dir = dirs::config_dir().context("Could not determine config directory")?;

        Ok(config_dir.join("flux9s").join("plugins"))
    }

    /// Load all plugins from the plugins directory
    pub fn load_all(&self) -> Result<Vec<PluginManifest>> {
        tracing::debug!("Loading plugins from: {:?}", self.plugins_dir);

        // Create plugins directory if it doesn't exist
        if !self.plugins_dir.exists() {
            tracing::info!("Plugins directory does not exist: {:?}", self.plugins_dir);
            return Ok(vec![]);
        }

        let mut plugins = Vec::new();
        let mut load_errors = Vec::new();

        // Scan directory for .yaml and .yml files
        for entry in
            std::fs::read_dir(&self.plugins_dir).context("Failed to read plugins directory")?
        {
            let entry = entry.context("Failed to read directory entry")?;
            let path = entry.path();

            // Skip non-files
            if !path.is_file() {
                continue;
            }

            // Check extension
            let extension = path.extension().and_then(|e| e.to_str());
            if extension != Some("yaml") && extension != Some("yml") {
                continue;
            }

            // Load and validate plugin
            match self.load_plugin(&path) {
                Ok(plugin) => {
                    if plugin.enabled {
                        tracing::info!("Loaded plugin: {}", plugin.name);
                        plugins.push(plugin);
                    } else {
                        tracing::info!("Plugin {} is disabled", plugin.name);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to load plugin {:?}: {}", path, e);
                    load_errors.push((path.clone(), e));
                }
            }
        }

        // Check for conflicts after loading all plugins
        if !plugins.is_empty() {
            tracing::debug!("Checking for conflicts across {} plugins", plugins.len());
            self.check_conflicts(&plugins)?;
        }

        if !load_errors.is_empty() {
            tracing::warn!(
                "Loaded {} plugins with {} errors",
                plugins.len(),
                load_errors.len()
            );
        } else if !plugins.is_empty() {
            tracing::info!(
                "Successfully loaded {} plugin(s): {}",
                plugins.len(),
                plugins
                    .iter()
                    .map(|p| p.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        } else {
            tracing::debug!("No plugins found in {:?}", self.plugins_dir);
        }

        Ok(plugins)
    }

    /// Load a single plugin from a file
    pub fn load_plugin(&self, path: &Path) -> Result<PluginManifest> {
        tracing::debug!("Loading plugin from: {:?}", path);

        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read plugin file: {:?}", path))?;

        let manifest: PluginManifest = serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse plugin YAML: {:?}", path))?;

        tracing::debug!(
            "Parsed plugin '{}' v{} from {:?}",
            manifest.name,
            manifest.version,
            path
        );

        // Validate the manifest
        PluginValidator::validate(&manifest)
            .with_context(|| format!("Plugin validation failed: {:?}", path))?;

        tracing::debug!("Plugin '{}' validated successfully", manifest.name);

        Ok(manifest)
    }

    /// Check for conflicts across all plugins
    fn check_conflicts(&self, plugins: &[PluginManifest]) -> PluginResult<()> {
        self.check_name_conflicts(plugins)?;
        self.check_column_conflicts(plugins)?;
        self.check_keybinding_conflicts(plugins)?;
        Ok(())
    }

    /// Check for duplicate plugin names
    fn check_name_conflicts(&self, plugins: &[PluginManifest]) -> PluginResult<()> {
        let mut seen_names = HashSet::new();
        let mut duplicates = Vec::new();

        for plugin in plugins {
            if !seen_names.insert(&plugin.name) {
                duplicates.push(plugin.name.clone());
            }
        }

        if !duplicates.is_empty() {
            return Err(PluginError::Conflict(format!(
                "Duplicate plugin names found: {}. Each plugin must have a unique name.",
                duplicates.join(", ")
            )));
        }

        Ok(())
    }

    /// Check for column name conflicts across plugins
    fn check_column_conflicts(&self, plugins: &[PluginManifest]) -> PluginResult<()> {
        // Track which plugin defines each column name
        let mut column_owners: HashMap<String, Vec<String>> = HashMap::new();

        for plugin in plugins {
            for column in &plugin.columns {
                column_owners
                    .entry(column.name.clone())
                    .or_default()
                    .push(plugin.name.clone());
            }
        }

        // Find conflicts
        let conflicts: Vec<_> = column_owners
            .iter()
            .filter(|(_, owners)| owners.len() > 1)
            .collect();

        if !conflicts.is_empty() {
            let mut error_msg = String::from("Column name conflicts detected:\n");
            for (column_name, owners) in &conflicts {
                tracing::error!(
                    "Column conflict: '{}' defined by plugins: {}",
                    column_name,
                    owners.join(", ")
                );
                error_msg.push_str(&format!(
                    "  - Column '{}' is defined by multiple plugins: {}\n",
                    column_name,
                    owners.join(", ")
                ));
            }
            error_msg.push_str("\nEach column name must be unique across all plugins. ");
            error_msg.push_str("Please rename conflicting columns in one of the plugins.");

            return Err(PluginError::Conflict(error_msg));
        }

        Ok(())
    }

    /// Check for keybinding conflicts across plugins
    fn check_keybinding_conflicts(&self, plugins: &[PluginManifest]) -> PluginResult<()> {
        // Track which plugin defines each keybinding
        let mut keybinding_owners: HashMap<String, Vec<String>> = HashMap::new();

        for plugin in plugins {
            for view in &plugin.views {
                keybinding_owners
                    .entry(view.keybinding.clone())
                    .or_default()
                    .push(plugin.name.clone());
            }
        }

        // Find conflicts
        let conflicts: Vec<_> = keybinding_owners
            .iter()
            .filter(|(_, owners)| owners.len() > 1)
            .collect();

        if !conflicts.is_empty() {
            let mut error_msg = String::from("Keybinding conflicts detected:\n");
            for (keybinding, owners) in conflicts {
                error_msg.push_str(&format!(
                    "  - Keybinding '{}' is defined by multiple plugins: {}\n",
                    keybinding,
                    owners.join(", ")
                ));
            }
            error_msg.push_str("\nEach keybinding must be unique across all plugins. ");
            error_msg.push_str("Please change conflicting keybindings in one of the plugins.");

            return Err(PluginError::Conflict(error_msg));
        }

        Ok(())
    }
}

impl Default for PluginLoader {
    fn default() -> Self {
        Self::new().expect("Failed to create PluginLoader")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::manifest::{
        ColumnConfig, DataSourceConfig, DataSourceType, Renderer, ViewConfig,
    };
    use tempfile::TempDir;

    fn create_test_plugin(name: &str) -> PluginManifest {
        PluginManifest {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            enabled: true,
            description: None,
            source: DataSourceConfig {
                source_type: DataSourceType::File,
                file_path: Some("/tmp/test.json".to_string()),
                service: None,
                namespace: None,
                port: None,
                path: None,
                kind: None,
                group: None,
                version: None,
                name: None,
                data_path: None,
                endpoint: None,
                auth: None,
                refresh_interval: None,
                timeout: None,
            },
            resources: vec!["Deployment".to_string()],
            columns: vec![],
            views: vec![],
            view_columns: Default::default(),
        }
    }

    #[test]
    fn test_load_from_empty_directory() {
        let temp_dir = TempDir::new().unwrap();
        let loader = PluginLoader::with_dir(temp_dir.path().to_path_buf());
        let plugins = loader.load_all().unwrap();
        assert_eq!(plugins.len(), 0);
    }

    #[test]
    fn test_load_single_plugin() {
        let temp_dir = TempDir::new().unwrap();
        let plugin_file = temp_dir.path().join("test.yaml");

        let yaml = r#"
name: test-plugin
version: 1.0.0
enabled: true
source:
  type: file
  file_path: /tmp/test.json
resources:
  - Deployment
columns: []
"#;
        std::fs::write(&plugin_file, yaml).unwrap();

        let loader = PluginLoader::with_dir(temp_dir.path().to_path_buf());
        let plugins = loader.load_all().unwrap();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "test-plugin");
    }

    #[test]
    fn test_skip_disabled_plugin() {
        let temp_dir = TempDir::new().unwrap();
        let plugin_file = temp_dir.path().join("disabled.yaml");

        let yaml = r#"
name: disabled-plugin
version: 1.0.0
enabled: false
source:
  type: file
  file_path: /tmp/test.json
resources:
  - Deployment
columns: []
"#;
        std::fs::write(&plugin_file, yaml).unwrap();

        let loader = PluginLoader::with_dir(temp_dir.path().to_path_buf());
        let plugins = loader.load_all().unwrap();
        assert_eq!(plugins.len(), 0);
    }

    #[test]
    fn test_skip_invalid_yaml() {
        let temp_dir = TempDir::new().unwrap();
        let plugin_file = temp_dir.path().join("invalid.yaml");

        let yaml = "invalid: yaml: content: [";
        std::fs::write(&plugin_file, yaml).unwrap();

        let loader = PluginLoader::with_dir(temp_dir.path().to_path_buf());
        // Should not panic, just log warning
        let plugins = loader.load_all().unwrap();
        assert_eq!(plugins.len(), 0);
    }

    #[test]
    fn test_detect_duplicate_plugin_names() {
        let plugins = vec![
            create_test_plugin("test-plugin"),
            create_test_plugin("test-plugin"), // Duplicate
        ];

        let loader = PluginLoader::with_dir(PathBuf::from("/tmp"));
        let result = loader.check_name_conflicts(&plugins);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Duplicate plugin names")
        );
    }

    #[test]
    fn test_detect_column_conflicts() {
        let mut plugin1 = create_test_plugin("plugin1");
        plugin1.columns = vec![ColumnConfig {
            name: "owner".to_string(),
            path: ".owner".to_string(),
            width: 12,
            enabled: true,
            description: None,
            renderer: Renderer::Text,
        }];

        let mut plugin2 = create_test_plugin("plugin2");
        plugin2.columns = vec![ColumnConfig {
            name: "owner".to_string(), // Conflict!
            path: ".metadata.owner".to_string(),
            width: 15,
            enabled: true,
            description: None,
            renderer: Renderer::Text,
        }];

        let plugins = vec![plugin1, plugin2];

        let loader = PluginLoader::with_dir(PathBuf::from("/tmp"));
        let result = loader.check_column_conflicts(&plugins);
        assert!(result.is_err());
        let error = result.unwrap_err().to_string();
        assert!(error.contains("Column name conflicts"));
        assert!(error.contains("owner"));
        assert!(error.contains("plugin1"));
        assert!(error.contains("plugin2"));
    }

    #[test]
    fn test_detect_keybinding_conflicts() {
        let mut plugin1 = create_test_plugin("plugin1");
        plugin1.views = vec![ViewConfig {
            name: "view1".to_string(),
            keybinding: ":agent".to_string(),
            description: None,
        }];

        let mut plugin2 = create_test_plugin("plugin2");
        plugin2.views = vec![ViewConfig {
            name: "view2".to_string(),
            keybinding: ":agent".to_string(), // Conflict!
            description: None,
        }];

        let plugins = vec![plugin1, plugin2];

        let loader = PluginLoader::with_dir(PathBuf::from("/tmp"));
        let result = loader.check_keybinding_conflicts(&plugins);
        assert!(result.is_err());
        let error = result.unwrap_err().to_string();
        assert!(error.contains("Keybinding conflicts"));
        assert!(error.contains(":agent"));
    }

    #[test]
    fn test_no_conflicts_with_different_columns() {
        let mut plugin1 = create_test_plugin("plugin1");
        plugin1.columns = vec![ColumnConfig {
            name: "owner".to_string(),
            path: ".owner".to_string(),
            width: 12,
            enabled: true,
            description: None,
            renderer: Renderer::Text,
        }];

        let mut plugin2 = create_test_plugin("plugin2");
        plugin2.columns = vec![ColumnConfig {
            name: "status".to_string(), // Different name, no conflict
            path: ".status".to_string(),
            width: 10,
            enabled: true,
            description: None,
            renderer: Renderer::Text,
        }];

        let plugins = vec![plugin1, plugin2];

        let loader = PluginLoader::with_dir(PathBuf::from("/tmp"));
        assert!(loader.check_column_conflicts(&plugins).is_ok());
    }
}
