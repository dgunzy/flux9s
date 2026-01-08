//! Plugin registry
//!
//! Manages loaded plugins and provides access to plugin data.

use super::manifest::PluginManifest;
use super::{PluginError, PluginResult};
use std::collections::HashMap;

/// Plugin registry holds all loaded and validated plugins
#[derive(Debug, Default)]
pub struct PluginRegistry {
    plugins: HashMap<String, PluginManifest>,
}

impl PluginRegistry {
    /// Create a new empty plugin registry
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
        }
    }

    /// Create a plugin registry from a list of plugins
    pub fn from_plugins(plugins: Vec<PluginManifest>) -> Self {
        let mut registry = Self::new();
        for plugin in plugins {
            registry.plugins.insert(plugin.name.clone(), plugin);
        }
        registry
    }

    /// Register a plugin
    pub fn register(&mut self, plugin: PluginManifest) {
        self.plugins.insert(plugin.name.clone(), plugin);
    }

    /// Get a plugin by name
    pub fn get(&self, name: &str) -> Option<&PluginManifest> {
        self.plugins.get(name)
    }

    /// Get all plugins
    pub fn all(&self) -> Vec<&PluginManifest> {
        self.plugins.values().collect()
    }

    /// Get plugin names
    pub fn names(&self) -> Vec<String> {
        self.plugins.keys().cloned().collect()
    }

    /// Check if a plugin is registered
    pub fn contains(&self, name: &str) -> bool {
        self.plugins.contains_key(name)
    }

    /// Get the number of registered plugins
    pub fn len(&self) -> usize {
        self.plugins.len()
    }

    /// Check if the registry is empty
    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }

    /// Remove a plugin by name
    pub fn remove(&mut self, name: &str) -> PluginResult<PluginManifest> {
        self.plugins
            .remove(name)
            .ok_or_else(|| PluginError::NotFound(name.to_string()))
    }

    /// Get all column names defined by plugins
    pub fn column_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for plugin in self.plugins.values() {
            for column in &plugin.columns {
                names.push(column.name.clone());
            }
        }
        names
    }

    /// Get all keybindings defined by plugins
    pub fn keybindings(&self) -> Vec<String> {
        let mut keybindings = Vec::new();
        for plugin in self.plugins.values() {
            for view in &plugin.views {
                keybindings.push(view.keybinding.clone());
            }
        }
        keybindings
    }

    /// Get plugins that enhance a specific resource type
    pub fn plugins_for_resource(&self, resource_type: &str) -> Vec<&PluginManifest> {
        self.plugins
            .values()
            .filter(|plugin| plugin.resources.contains(&resource_type.to_string()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::manifest::{ColumnConfig, DataSourceConfig, DataSourceType, Renderer};

    fn create_test_plugin(name: &str, resources: Vec<&str>) -> PluginManifest {
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
            resources: resources.iter().map(|s| s.to_string()).collect(),
            columns: vec![],
            views: vec![],
            view_columns: Default::default(),
        }
    }

    #[test]
    fn test_empty_registry() {
        let registry = PluginRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn test_register_plugin() {
        let mut registry = PluginRegistry::new();
        let plugin = create_test_plugin("test", vec!["Deployment"]);

        registry.register(plugin);
        assert_eq!(registry.len(), 1);
        assert!(registry.contains("test"));
    }

    #[test]
    fn test_get_plugin() {
        let mut registry = PluginRegistry::new();
        let plugin = create_test_plugin("test", vec!["Deployment"]);

        registry.register(plugin);

        let retrieved = registry.get("test");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "test");

        let missing = registry.get("missing");
        assert!(missing.is_none());
    }

    #[test]
    fn test_remove_plugin() {
        let mut registry = PluginRegistry::new();
        let plugin = create_test_plugin("test", vec!["Deployment"]);

        registry.register(plugin);
        assert_eq!(registry.len(), 1);

        let removed = registry.remove("test");
        assert!(removed.is_ok());
        assert_eq!(registry.len(), 0);

        let not_found = registry.remove("missing");
        assert!(not_found.is_err());
    }

    #[test]
    fn test_from_plugins() {
        let plugins = vec![
            create_test_plugin("plugin1", vec!["Deployment"]),
            create_test_plugin("plugin2", vec!["Service"]),
        ];

        let registry = PluginRegistry::from_plugins(plugins);
        assert_eq!(registry.len(), 2);
        assert!(registry.contains("plugin1"));
        assert!(registry.contains("plugin2"));
    }

    #[test]
    fn test_plugins_for_resource() {
        let mut registry = PluginRegistry::new();
        registry.register(create_test_plugin("plugin1", vec!["Deployment", "Service"]));
        registry.register(create_test_plugin("plugin2", vec!["Deployment"]));
        registry.register(create_test_plugin("plugin3", vec!["HelmRelease"]));

        let deployment_plugins = registry.plugins_for_resource("Deployment");
        assert_eq!(deployment_plugins.len(), 2);

        let service_plugins = registry.plugins_for_resource("Service");
        assert_eq!(service_plugins.len(), 1);

        let helm_plugins = registry.plugins_for_resource("HelmRelease");
        assert_eq!(helm_plugins.len(), 1);

        let missing_plugins = registry.plugins_for_resource("Missing");
        assert_eq!(missing_plugins.len(), 0);
    }

    #[test]
    fn test_column_names() {
        let mut plugin = create_test_plugin("test", vec!["Deployment"]);
        plugin.columns = vec![
            ColumnConfig {
                name: "owner".to_string(),
                path: ".owner".to_string(),
                width: 12,
                enabled: true,
                description: None,
                renderer: Renderer::Text,
            },
            ColumnConfig {
                name: "status".to_string(),
                path: ".status".to_string(),
                width: 10,
                enabled: true,
                description: None,
                renderer: Renderer::Text,
            },
        ];

        let mut registry = PluginRegistry::new();
        registry.register(plugin);

        let column_names = registry.column_names();
        assert_eq!(column_names.len(), 2);
        assert!(column_names.contains(&"owner".to_string()));
        assert!(column_names.contains(&"status".to_string()));
    }
}
