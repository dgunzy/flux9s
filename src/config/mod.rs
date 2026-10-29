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
                    let valid: Vec<_> = crate::models::FluxResourceKind::ALL
                        .iter()
                        .map(|kind| kind.as_str())
                        .collect();
                    anyhow::anyhow!(
                        "{e}. Give Flux kinds (e.g. ks,hr) or a view name. Valid kinds: {}",
                        valid.join(", ")
                    )
                })?;
                Some(filter.all_kinds().collect::<Vec<_>>().join(","))
            };
        }
        "defaultSort" => {
            config.default_sort = if value.is_empty() {
                None
            } else {
                crate::models::list_sort::ListSort::parse(value)
                    .map_err(|e| anyhow::anyhow!("defaultSort: {e}"))?;
                Some(value.trim().to_string())
            };
        }
        // `views.<name>` / `views.<name>.kinds`: the view's kinds as a comma
        // list (empty removes the view); `views.<name>.namespace` and
        // `views.<name>.sort` set its options (empty unsets them).
        _ if key.starts_with("views.") => {
            let rest = &key["views.".len()..];
            let (name, field) = rest.split_once('.').unwrap_or((rest, "kinds"));
            if name.is_empty() || field.contains('.') {
                return Err(anyhow::anyhow!(
                    "Use views.<name>[.kinds|.namespace|.sort] (e.g. flux9s config set views.apps ks,hr)"
                ));
            }
            let set = |value: &str| (!value.is_empty()).then(|| value.trim().to_string());
            match field {
                "kinds" if value.is_empty() => {
                    config.views.remove(name);
                }
                "kinds" => {
                    // Validate; store the kinds as written (aliases read
                    // better). Unknown kinds may be Flux-adjacent (known at
                    // runtime); the CLI warns about them via `view_problems`.
                    crate::models::kind_filter::KindFilter::parse_allowing_unknown(value)
                        .map_err(|e| anyhow::anyhow!("views.{name}: {e}"))?;
                    config.views.entry(name.to_string()).or_default().kinds = value
                        .split(',')
                        .map(str::trim)
                        .filter(|t| !t.is_empty())
                        .map(str::to_string)
                        .collect();
                }
                "namespace" | "sort" => {
                    if field == "sort" && !value.is_empty() {
                        crate::models::list_sort::ListSort::parse(value)
                            .map_err(|e| anyhow::anyhow!("views.{name}.sort: {e}"))?;
                    }
                    let view = config.views.get_mut(name).ok_or_else(|| {
                        anyhow::anyhow!(
                            "No view '{name}' — set its kinds first: flux9s config set views.{name} ks,hr"
                        )
                    })?;
                    if field == "sort" {
                        view.sort = set(value);
                    } else {
                        view.namespace = set(value);
                    }
                }
                _ => {
                    return Err(anyhow::anyhow!(
                        "Unknown view setting '{field}' — use kinds, namespace, or sort"
                    ));
                }
            }
        }
        // A comma list; empty restores the default, `none` sweeps everywhere.
        "ownership.ignoreNamespaces" => {
            config.ownership.ignore_namespaces = match value.trim() {
                "" => None,
                "none" => Some(Vec::new()),
                list => Some(
                    list.split(',')
                        .map(str::trim)
                        .filter(|ns| !ns.is_empty())
                        .map(str::to_string)
                        .collect(),
                ),
            };
        }
        "ownership.ignore" => {
            return Err(anyhow::anyhow!(
                "ownership.ignore is a list of rules — edit the config file (flux9s config path)"
            ));
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
        // Kubernetes kinds mix in (#304).
        set_config_value(&mut config, "views.mixed", "ks,deploy").unwrap();
        assert_eq!(config.views["mixed"].kinds, ["ks", "deploy"]);

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
        set_config_value(&mut config, "defaultResourceFilter", "ks,deploy").unwrap();
        assert_eq!(
            config.default_resource_filter.as_deref(),
            Some("Kustomization,deploy"),
            "a Kubernetes kind mixes in (#304)"
        );

        set_config_value(&mut config, "views.apps", "").unwrap();
        assert!(!config.views.contains_key("apps"));
        assert!(set_config_value(&mut config, "views.", "ks").is_err());
    }

    #[test]
    fn view_options_and_default_sort_set_from_the_cli() {
        let mut config = schema::Config::default();
        let err = set_config_value(&mut config, "views.apps.sort", "type").unwrap_err();
        assert!(err.to_string().contains("set its kinds first"), "{err}");

        set_config_value(&mut config, "views.apps", "ks,hr").unwrap();
        set_config_value(&mut config, "views.apps.namespace", "apps").unwrap();
        set_config_value(&mut config, "views.apps.sort", "-type").unwrap();
        let view = &config.views["apps"];
        assert_eq!(view.namespace.as_deref(), Some("apps"));
        assert_eq!(view.sort.as_deref(), Some("-type"));

        // Changing the kinds keeps the options.
        set_config_value(&mut config, "views.apps", "ks").unwrap();
        assert_eq!(config.views["apps"].sort.as_deref(), Some("-type"));
        set_config_value(&mut config, "views.apps.sort", "").unwrap();
        assert_eq!(config.views["apps"].sort, None);

        assert!(set_config_value(&mut config, "views.apps.sort", "size").is_err());
        assert!(set_config_value(&mut config, "views.apps.colour", "red").is_err());

        set_config_value(&mut config, "defaultSort", "age:desc").unwrap();
        assert_eq!(config.default_sort.as_deref(), Some("age:desc"));
        assert!(set_config_value(&mut config, "defaultSort", "size").is_err());
        set_config_value(&mut config, "defaultSort", "").unwrap();
        assert_eq!(config.default_sort, None);
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
