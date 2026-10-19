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
pub use schema::Config;
pub use schema::UiConfig;
pub use schema::ViewConfig;
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
        "ui.rbacWarnings" => Ok(config.ui.rbac_warnings.to_string()),
        "namespaceHotkeys" => {
            // Return as YAML array
            crate::yaml::to_string(&config.namespace_hotkeys)
                .map_err(|e| anyhow::anyhow!("Failed to serialize namespaceHotkeys: {}", e))
        }
        "defaultResourceFilter" => Ok(config.default_resource_filter.clone().unwrap_or_default()),
        "connectTimeoutSeconds" => Ok(config.connect_timeout_seconds.to_string()),
        "discoverFluxResources" => Ok(config.discover_flux_resources.to_string()),
        "editor" => Ok(config.editor.clone().unwrap_or_default()),
        // Any field the arms above don't special-case is resolved from the
        // serialized config, so new schema fields are gettable without a new
        // arm. `skip_serializing_if` hides empty fields from the serialized
        // config — the fully-populated skeleton tells "empty" apart from
        // "not a config key at all".
        _ => {
            let resolve = |value: &serde_json::Value| -> Option<serde_json::Value> {
                key.split('.')
                    .try_fold(value.clone(), |node, part| node.get(part).cloned())
            };
            let current = serde_json::to_value(config)
                .map_err(|e| anyhow::anyhow!("Failed to serialize configuration: {}", e))?;
            if let Some(node) = resolve(&current) {
                return crate::yaml::to_string(&node)
                    .map(|s| s.trim_end().to_string())
                    .map_err(|e| anyhow::anyhow!("Failed to serialize {}: {}", key, e));
            }
            let skeleton = serde_json::to_value(schema::Config::fully_populated())
                .map_err(|e| anyhow::anyhow!("Failed to serialize configuration: {}", e))?;
            if resolve(&skeleton).is_some() {
                Ok(String::new()) // real field, currently empty/unset
            } else {
                Err(anyhow::anyhow!("Unknown configuration key: {}", key))
            }
        }
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
        "discoverFluxResources" => {
            config.discover_flux_resources = value
                .parse()
                .context("discoverFluxResources must be 'true' or 'false'")?;
        }
        "nativeResources" => {
            config.native_resources = value
                .parse()
                .context("nativeResources must be 'true' or 'false'")?;
        }
        "metricsSource" => {
            config.metrics_source = crate::kube::metrics::MetricsSourceSetting::parse(value)
                .context("metricsSource must be one of: auto, kubelet, metrics-server, none")?;
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
        "ui.rbacWarnings" => {
            config.ui.rbac_warnings = value
                .parse()
                .context("ui.rbacWarnings must be 'true' or 'false'")?;
        }
        "namespaceHotkeys" => {
            // Parse as YAML array or comma-separated list
            let hotkeys: Vec<String> = if value.trim_start().starts_with('[') {
                // YAML array format
                crate::yaml::from_str(value).context(
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
            config.default_resource_filter = if value.is_empty() {
                None
            } else if let Some(view) = config.views.keys().find(|v| v.eq_ignore_ascii_case(value)) {
                // A view name stays a view name (it follows edits to the
                // view), as configured — views match case-insensitively.
                Some(view.clone())
            } else {
                // One kind or several (`ks,hr`), stored as display names. A
                // kind only known at runtime (Flux-adjacent) is kept as given.
                let (filter, _) = crate::models::kind_filter::KindFilter::parse_allowing_unknown(
                    value,
                )
                .map_err(|e| {
                    let valid: Vec<_> = crate::watcher::RESOURCE_REGISTRY
                        .iter()
                        .map(|e| e.display_name)
                        .collect();
                    anyhow::anyhow!(
                        "{e}. Give Flux kinds (e.g. ks,hr) or a view name. Valid kinds: {}",
                        valid.join(", ")
                    )
                })?;
                Some(filter.kinds().join(","))
            };
        }
        // `views.<name>` (or `views.<name>.kinds`): the view's kinds as a
        // comma list; empty removes the view.
        _ if key.starts_with("views.") => {
            let name = key.trim_start_matches("views.").trim_end_matches(".kinds");
            if name.is_empty() || name.contains('.') {
                return Err(anyhow::anyhow!(
                    "Use views.<name> (e.g. flux9s config set views.apps ks,hr)"
                ));
            }
            if value.is_empty() {
                config.views.remove(name);
            } else {
                // Validate; store the kinds as written (aliases read better).
                // Unknown kinds may be Flux-adjacent (known at runtime); the
                // CLI warns about them through `view_problems`.
                crate::models::kind_filter::KindFilter::parse_allowing_unknown(value)
                    .map_err(|e| anyhow::anyhow!("views.{name}: {e}"))?;
                let kinds = value
                    .split(',')
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                    .map(str::to_string)
                    .collect();
                config
                    .views
                    .insert(name.to_string(), schema::ViewConfig { kinds });
            }
        }
        "connectTimeoutSeconds" => {
            let seconds = value
                .parse::<u64>()
                .context("connectTimeoutSeconds must be a positive integer")?;
            if seconds == 0 {
                return Err(anyhow::anyhow!(
                    "connectTimeoutSeconds must be a positive integer"
                ));
            }
            config.connect_timeout_seconds = seconds;
        }
        "editor" => {
            if value.is_empty() {
                config.editor = None;
            } else {
                config.editor = Some(value.to_string());
            }
        }
        _ => return Err(anyhow::anyhow!("Unknown configuration key: {}", key)),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connect_timeout_get_set_config_value() {
        let mut config = schema::Config::default();

        set_config_value(&mut config, "connectTimeoutSeconds", "15").unwrap();

        assert_eq!(config.connect_timeout_seconds, 15);
        assert_eq!(
            get_config_value(&config, "connectTimeoutSeconds").unwrap(),
            "15"
        );
    }

    #[test]
    fn views_and_kind_sets_set_from_the_cli() {
        let mut config = schema::Config::default();
        set_config_value(&mut config, "views.apps", "ks, hr").unwrap();
        assert_eq!(config.views["apps"].kinds, ["ks", "hr"]);
        assert!(
            get_config_value(&config, "views.apps")
                .unwrap()
                .contains("- ks")
        );
        set_config_value(&mut config, "views.apps.kinds", "hr").unwrap();
        assert_eq!(config.views["apps"].kinds, ["hr"]);
        let err = set_config_value(&mut config, "views.mixed", "ks,deploy").unwrap_err();
        assert!(
            err.to_string().contains("'deploy' is a Kubernetes kind"),
            "{err}"
        );

        // defaultResourceFilter: kinds stored as display names, views by name.
        set_config_value(&mut config, "defaultResourceFilter", "ks,hr").unwrap();
        assert_eq!(
            config.default_resource_filter.as_deref(),
            Some("Kustomization,HelmRelease")
        );
        set_config_value(&mut config, "defaultResourceFilter", "Apps").unwrap();
        assert_eq!(
            config.default_resource_filter.as_deref(),
            Some("apps"),
            "views match case-insensitively, stored as configured"
        );
        // A kind flux9s only learns at runtime (Flux-adjacent) is kept.
        set_config_value(&mut config, "defaultResourceFilter", "ks,RuntimeOnlyKind").unwrap();
        assert_eq!(
            config.default_resource_filter.as_deref(),
            Some("Kustomization,RuntimeOnlyKind")
        );
        set_config_value(&mut config, "views.extra", "ks,RuntimeOnlyKind").unwrap();
        assert!(set_config_value(&mut config, "defaultResourceFilter", "ks,deploy").is_err());

        set_config_value(&mut config, "views.apps", "").unwrap();
        assert!(!config.views.contains_key("apps"));
        assert!(set_config_value(&mut config, "views.", "ks").is_err());
    }

    #[test]
    fn test_connect_timeout_rejects_zero() {
        let mut config = schema::Config::default();

        let err = set_config_value(&mut config, "connectTimeoutSeconds", "0").unwrap_err();

        assert!(err.to_string().contains("positive integer"));
    }

    #[test]
    fn get_resolves_fields_without_dedicated_arms() {
        // Fields with no match arm (and any future field) resolve through the
        // serialized-config fallback instead of "Unknown configuration key".
        let mut config = schema::Config::default();
        config
            .context_skins
            .insert("prod".to_string(), "dracula".to_string());

        let skins = get_config_value(&config, "contextSkins").unwrap();
        assert!(skins.contains("prod: dracula"));

        // Empty-and-skipped fields are real keys that answer with nothing.
        assert_eq!(get_config_value(&config, "favorites").unwrap(), "");
        assert_eq!(get_config_value(&config, "cluster").unwrap(), "");

        let err = get_config_value(&config, "noSuchKey").unwrap_err();
        assert!(err.to_string().contains("Unknown configuration key"));
        let err = get_config_value(&config, "ui.noSuchKey").unwrap_err();
        assert!(err.to_string().contains("Unknown configuration key"));
    }

    #[test]
    fn test_editor_get_set_config_value() {
        let mut config = schema::Config::default();

        // Default is empty string (None)
        assert_eq!(get_config_value(&config, "editor").unwrap(), "");

        // Set a value
        set_config_value(&mut config, "editor", "nvim").unwrap();
        assert_eq!(get_config_value(&config, "editor").unwrap(), "nvim");
        assert_eq!(config.editor.as_deref(), Some("nvim"));

        // Clear by setting empty string
        set_config_value(&mut config, "editor", "").unwrap();
        assert_eq!(get_config_value(&config, "editor").unwrap(), "");
        assert!(config.editor.is_none());
    }
}
