//! Configuration system for flux9s
//!
//! This module provides a comprehensive configuration system modeled after k9s,
//! supporting multiple configuration layers, theme management, and persistent settings.

mod defaults;
#[cfg(feature = "tui")]
pub mod embedded_themes;
pub mod loader;
pub mod paths;
pub mod schema;
#[cfg(feature = "tui")]
pub mod theme_loader;

pub use loader::ConfigLoader;
#[allow(unused_imports)] // Public API exports - may be used by external code
pub use schema::Config;
#[allow(unused_imports)] // Public API exports - may be used by external code
pub use schema::UiConfig;
#[cfg(feature = "tui")]
pub use theme_loader::ThemeLoader;

/// Get a configuration value by key (dot notation)
pub fn get_config_value(config: &schema::Config, key: &str) -> anyhow::Result<String> {
    match key {
        "readOnly" => Ok(config.read_only.to_string()),
        "defaultNamespace" => Ok(config.default_namespace.clone()),
        "defaultControllerNamespace" => Ok(config.default_controller_namespace.clone()),
        "ui.enableMouse" => Ok(config.ui.enable_mouse.to_string()),
        "ui.headless" => Ok(config.ui.headless.to_string()),
        "ui.noIcons" => Ok(config.ui.no_icons.to_string()),
        "ui.skin" => Ok(config.ui.skin.clone()),
        "ui.skinReadOnly" => Ok(config.ui.skin_read_only.clone().unwrap_or_default()),
        "ui.splashless" => Ok(config.ui.splashless.to_string()),
        "namespaceHotkeys" => {
            // Return as YAML array
            serde_yaml::to_string(&config.namespace_hotkeys)
                .map_err(|e| anyhow::anyhow!("Failed to serialize namespaceHotkeys: {}", e))
        }
        "defaultResourceFilter" => Ok(config.default_resource_filter.clone().unwrap_or_default()),
        _ => Err(anyhow::anyhow!("Unknown configuration key: {}", key)),
    }
}

/// Set a configuration value by key (dot notation)
pub fn set_config_value(config: &mut schema::Config, key: &str, value: &str) -> anyhow::Result<()> {
    use anyhow::Context;
    match key {
        "readOnly" => {
            config.read_only = value
                .parse()
                .context("readOnly must be 'true' or 'false'")?;
        }
        "defaultNamespace" => {
            config.default_namespace = value.to_string();
        }
        "defaultControllerNamespace" => {
            config.default_controller_namespace = value.to_string();
        }
        "ui.enableMouse" => {
            config.ui.enable_mouse = value
                .parse()
                .context("ui.enableMouse must be 'true' or 'false'")?;
        }
        "ui.headless" => {
            config.ui.headless = value
                .parse()
                .context("ui.headless must be 'true' or 'false'")?;
        }
        "ui.noIcons" => {
            config.ui.no_icons = value
                .parse()
                .context("ui.noIcons must be 'true' or 'false'")?;
        }
        "ui.skin" => {
            config.ui.skin = value.to_string();
        }
        "ui.skinReadOnly" => {
            if value.is_empty() {
                config.ui.skin_read_only = None;
            } else {
                config.ui.skin_read_only = Some(value.to_string());
            }
        }
        "ui.splashless" => {
            config.ui.splashless = value
                .parse()
                .context("ui.splashless must be 'true' or 'false'")?;
        }
        "namespaceHotkeys" => {
            // Parse as YAML array or comma-separated list
            let hotkeys: Vec<String> = if value.trim_start().starts_with('[') {
                // YAML array format
                serde_yaml::from_str(value).context(
                    "namespaceHotkeys must be a YAML array (e.g., ['all', 'flux-system', 'ns1'])",
                )?
            } else {
                // Comma-separated list format
                value
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            };

            // Validate length (max 10)
            if hotkeys.len() > 10 {
                return Err(anyhow::anyhow!(
                    "namespaceHotkeys can have at most 10 items (0-9), got {}",
                    hotkeys.len()
                ));
            }

            config.namespace_hotkeys = hotkeys;
        }
        "defaultResourceFilter" => {
            if value.is_empty() {
                config.default_resource_filter = None;
            } else {
                let display_name =
                    crate::watcher::get_display_name_for_command(value).ok_or_else(|| {
                        let valid: Vec<_> = crate::watcher::RESOURCE_REGISTRY
                            .iter()
                            .map(|e| e.display_name)
                            .collect();
                        anyhow::anyhow!(
                            "Unknown resource type '{}'. Valid types: {}",
                            value,
                            valid.join(", ")
                        )
                    })?;
                config.default_resource_filter = Some(display_name.to_string());
            }
        }
        _ => return Err(anyhow::anyhow!("Unknown configuration key: {}", key)),
    }

    Ok(())
}
