//! Configuration command handlers

use anyhow::{Context, Result};
use clap::Subcommand;
use std::path::PathBuf;

use crate::config::schema::Config;
use crate::config::{ConfigLoader, ThemeLoader, paths};

/// Skins management subcommands
#[derive(Subcommand, Debug)]
pub enum SkinsSubcommand {
    /// List available skins
    List,
    /// Install a skin from a YAML file
    Set {
        /// Path to the skin YAML file
        file: PathBuf,
    },
    /// Test loading a skin
    Test {
        /// Skin name to test
        name: String,
    },
}

/// Configuration management subcommands
#[derive(Subcommand, Debug)]
pub enum ConfigSubcommand {
    /// Get configuration value
    Get {
        /// Configuration key (e.g., "readOnly", "ui.skin")
        key: Option<String>,
    },
    /// Set configuration value
    Set {
        /// Configuration key (e.g., "readOnly", "ui.skin")
        key: String,
        /// Configuration value
        value: String,
        /// Cluster name for cluster-specific config
        #[arg(long)]
        cluster: Option<String>,
        /// Context name for context-specific config
        #[arg(long)]
        context: Option<String>,
    },
    /// List all configuration
    List,
    /// Show configuration file path
    Path,
    /// Validate configuration
    Validate,
    /// Manage skins
    Skins {
        #[command(subcommand)]
        subcommand: SkinsSubcommand,
    },
    /// Restore namespace hotkeys to defaults (empty, will auto-discover)
    RestoreNamespaceHotkeys {
        /// Cluster name for cluster-specific config
        #[arg(long)]
        cluster: Option<String>,
        /// Context name for context-specific config
        #[arg(long)]
        context: Option<String>,
    },
}

/// Handle configuration subcommands
pub async fn handle_config_command(cmd: ConfigSubcommand) -> Result<()> {
    match cmd {
        ConfigSubcommand::Get { key } => {
            // Load config (will use defaults if no file exists)
            let cluster = None;
            let context = None;
            let config =
                ConfigLoader::load(cluster, context).context("Failed to load configuration")?;

            if let Some(key) = key {
                // Get specific key
                let value = crate::config::get_config_value(&config, &key)?;
                println!("{}", value);
            } else {
                // Print all config as YAML
                let yaml =
                    serde_yaml::to_string(&config).context("Failed to serialize configuration")?;
                print!("{}", yaml);
            }
        }
        ConfigSubcommand::Set {
            key,
            value,
            cluster,
            context,
        } => {
            // Load existing config or create default
            let mut config = ConfigLoader::load(cluster.as_deref(), context.as_deref())
                .unwrap_or_else(|_| ConfigLoader::load_defaults());

            // Set the value
            crate::config::set_config_value(&mut config, &key, &value)
                .with_context(|| format!("Failed to set {} = {}", key, value))?;

            // Save config
            if let Some(cluster_name) = cluster {
                ConfigLoader::save_cluster(&config, &cluster_name, context.as_deref())
                    .context("Failed to save cluster configuration")?;
                println!("Configuration saved for cluster: {}", cluster_name);
            } else {
                ConfigLoader::save_root(&config).context("Failed to save configuration")?;
                println!("Configuration saved");
            }
        }
        ConfigSubcommand::List => {
            let cluster = None;
            let context = None;
            let config =
                ConfigLoader::load(cluster, context).context("Failed to load configuration")?;

            // Display config with all fields visible, showing defaults
            display_config_with_defaults(&config);
        }
        ConfigSubcommand::Path => {
            let config_path = paths::root_config_path();
            println!("{}", config_path.display());
        }
        ConfigSubcommand::Validate => {
            let cluster = None;
            let context = None;

            // Validate by actually loading and parsing the config
            // This will catch YAML syntax errors, invalid types, etc.
            match ConfigLoader::validate(cluster, context) {
                Ok(_) => {
                    println!("flux9s configuration is valid");
                }
                Err(e) => {
                    eprintln!("flux9s configuration validation failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
        ConfigSubcommand::Skins { subcommand } => match subcommand {
            SkinsSubcommand::List => {
                let themes = ThemeLoader::list_themes();
                println!("Available skins:");
                for theme in themes {
                    println!("  - {}", theme);
                }
                println!("\nSkin locations:");
                println!("  Config directory: {}", paths::skins_dir().display());
                println!(
                    "  Legacy data directory: {}",
                    paths::user_skins_dir().display()
                );
                println!("\nTo install a skin:");
                println!("  flux9s config skins set <path-to-skin.yaml>");
            }
            SkinsSubcommand::Set { file } => {
                // Validate and install the skin
                let skin_name =
                    ThemeLoader::install_theme(&file).context("Failed to install skin")?;

                // Automatically set the skin in config
                let cluster = None;
                let context = None;
                let mut config = ConfigLoader::load(cluster, context)
                    .unwrap_or_else(|_| ConfigLoader::load_defaults());

                config.ui.skin = skin_name.clone();

                // Save config
                ConfigLoader::save_root(&config).context("Failed to save configuration")?;

                println!("✓ Skin '{}' set in configuration", skin_name);
            }
            SkinsSubcommand::Test { name } => match ThemeLoader::load_theme(&name) {
                Ok(theme) => {
                    println!("✓ Successfully loaded skin: {}", name);
                    println!("\nSkin colors:");
                    println!("  Header context: {:?}", theme.header_context);
                    println!("  Header ASCII: {:?}", theme.header_ascii);
                    println!("  Text primary: {:?}", theme.text_primary);
                    println!("  Status ready: {:?}", theme.status_ready);
                    println!("  Status error: {:?}", theme.status_error);
                    println!("  Table header: {:?}", theme.table_header);
                    println!("  Table normal: {:?}", theme.table_normal);
                    println!("  Footer key: {:?}", theme.footer_key);
                }
                Err(e) => {
                    eprintln!("✗ Failed to load skin '{}': {}", name, e);
                    eprintln!("\nChecked locations:");
                    eprintln!(
                        "  - {}",
                        paths::skins_dir().join(format!("{}.yaml", name)).display()
                    );
                    eprintln!(
                        "  - {}",
                        paths::user_skins_dir()
                            .join(format!("{}.yaml", name))
                            .display()
                    );
                    std::process::exit(1);
                }
            },
        },
        ConfigSubcommand::RestoreNamespaceHotkeys { cluster, context } => {
            // Load existing config or create default
            let mut config = ConfigLoader::load(cluster.as_deref(), context.as_deref())
                .unwrap_or_else(|_| ConfigLoader::load_defaults());

            // Clear namespace hotkeys (empty means use auto-discovered defaults)
            config.namespace_hotkeys = Vec::new();

            // Save config
            if let Some(cluster_name) = cluster {
                ConfigLoader::save_cluster(&config, &cluster_name, context.as_deref())
                    .context("Failed to save cluster configuration")?;
                println!(
                    "Namespace hotkeys restored to defaults for cluster: {}",
                    cluster_name
                );
            } else {
                ConfigLoader::save_root(&config).context("Failed to save configuration")?;
                println!("Namespace hotkeys restored to defaults");
                println!("(Empty config will auto-discover namespaces at startup)");
            }
        }
    }

    Ok(())
}

/// Display configuration with all fields visible, annotating defaults.
fn display_config_with_defaults(config: &Config) {
    print!("{}", render_config_listing(config));
    print!("{}", reference_text());
}

/// Render the full configuration as annotated YAML.
///
/// The field set comes from [`Config::fully_populated`] — serde serialization
/// of the *real* config omits `skip_serializing_if` fields, so walking the
/// fully-populated skeleton is what guarantees every field (present and
/// future) appears in the listing. Values come from the user's config;
/// fields it omitted render as their empty form with a `# (default)` marker.
fn render_config_listing(config: &Config) -> String {
    let current = serde_yaml::to_value(config).unwrap_or_default();
    let defaults = serde_yaml::to_value(Config::default()).unwrap_or_default();
    let skeleton = serde_yaml::to_value(Config::fully_populated()).unwrap_or_default();

    let mut out = String::new();
    if let Some(skel) = skeleton.as_mapping() {
        render_section(
            &mut out,
            skel,
            current.as_mapping(),
            defaults.as_mapping(),
            0,
        );
    }
    out
}

fn render_section(
    out: &mut String,
    skeleton: &serde_yaml::Mapping,
    current: Option<&serde_yaml::Mapping>,
    defaults: Option<&serde_yaml::Mapping>,
    indent: usize,
) {
    use serde_yaml::Value;

    let pad = "  ".repeat(indent);
    for (key, skel_val) in skeleton {
        let key_str = key.as_str().unwrap_or_default();
        let cur = current.and_then(|m| m.get(key));
        let def = defaults.and_then(|m| m.get(key));
        // A field the user's config omitted is at its (empty) default.
        let marker = if normalize(cur, skel_val) == normalize(def, skel_val) {
            "  # (default)"
        } else {
            ""
        };

        match skel_val {
            // Struct sections (e.g. `ui`) recurse over the skeleton's keys so
            // omitted subfields still show. Free-form user maps (contextSkins,
            // cluster) render the user's actual entries — the skeleton only
            // holds sample data for those. The defaults config tells them
            // apart: struct sections serialize their fields, user maps are
            // empty-and-skipped by default.
            Value::Mapping(skel_inner) => {
                let struct_like = def
                    .and_then(Value::as_mapping)
                    .is_some_and(|m| !m.is_empty());
                if struct_like {
                    out.push_str(&format!("{pad}{key_str}:\n"));
                    render_section(
                        out,
                        skel_inner,
                        cur.and_then(Value::as_mapping),
                        def.and_then(Value::as_mapping),
                        indent + 1,
                    );
                } else {
                    match cur.and_then(Value::as_mapping).filter(|m| !m.is_empty()) {
                        Some(m) => {
                            out.push_str(&format!("{pad}{key_str}:\n"));
                            let yaml = serde_yaml::to_string(&Value::Mapping(m.clone()))
                                .unwrap_or_default();
                            for line in yaml.lines() {
                                out.push_str(&format!("{pad}  {line}\n"));
                            }
                        }
                        None => out.push_str(&format!("{pad}{key_str}: {{}}{marker}\n")),
                    }
                }
            }
            Value::Sequence(_) => {
                match cur.and_then(Value::as_sequence).filter(|s| !s.is_empty()) {
                    Some(seq) => {
                        out.push_str(&format!("{pad}{key_str}:{marker}\n"));
                        for item in seq {
                            out.push_str(&format!("{pad}  - {}\n", scalar_str(item)));
                        }
                    }
                    None => out.push_str(&format!("{pad}{key_str}: []{marker}\n")),
                }
            }
            _ => {
                let rendered = cur.map(scalar_str).unwrap_or_else(|| "~".to_string());
                out.push_str(&format!("{pad}{key_str}: {rendered}{marker}\n"));
            }
        }
    }
}

/// A field missing from a serialized config was skipped because it is empty;
/// resolve it to the empty value of its type (per the skeleton) so it
/// compares equal to an equally-empty default.
fn normalize(value: Option<&serde_yaml::Value>, skeleton: &serde_yaml::Value) -> serde_yaml::Value {
    use serde_yaml::Value;
    match value {
        Some(v) => v.clone(),
        None => match skeleton {
            Value::Mapping(_) => Value::Mapping(serde_yaml::Mapping::new()),
            Value::Sequence(_) => Value::Sequence(Vec::new()),
            _ => Value::Null,
        },
    }
}

fn scalar_str(value: &serde_yaml::Value) -> String {
    use serde_yaml::Value;
    match value {
        Value::Null => "~".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        other => serde_yaml::to_string(other)
            .unwrap_or_default()
            .trim_end()
            .to_string(),
    }
}

/// The configuration reference appended to `config list`. Every schema field
/// must be mentioned here — enforced by `reference_docs_cover_every_field`.
fn reference_text() -> String {
    let mut s = String::from("\n# Configuration Reference:\n");
    for line in [
        "readOnly - Disable modification operations (default: true)",
        "defaultNamespace - Starting namespace (default: flux-system)",
        "defaultControllerNamespace - Flux controller namespace (default: flux-system)",
        "discoverFluxResources - Discover CRDs labeled app.kubernetes.io/part-of=flux as view-only kinds (default: false)",
        "defaultResourceFilter - Resource type filter at startup, e.g. \"Kustomization\" (default: none, shows all)",
        "connectTimeoutSeconds - Startup Kubernetes API health-check timeout in seconds (default: 10)",
        "editor - Editor command for resource editing; falls back through $VISUAL, $EDITOR, vi (default: none)",
        "ui.enableMouse - Enable mouse support (default: false)",
        "ui.headless - Hide header (default: false)",
        "ui.noIcons - Disable Unicode icons (default: false)",
        "ui.skin - Default skin name (default: default)",
        "ui.skinReadOnly - Skin for readonly mode, overrides ui.skin when readOnly=true",
        "ui.splashless - Skip startup splash (default: false)",
        "namespaceHotkeys - Array of namespace names for 0-9 hotkeys (max 10, default: auto-discover)",
        "contextSkins - Map of context name to skin name (default: empty)",
        "cluster - Map of cluster name to cluster-specific settings (default: empty)",
        "favorites - List of favorited resource keys, e.g. \"Kustomization:flux-system:my-app\" (default: empty)",
    ] {
        s.push_str(&format!("#   {line}\n"));
    }
    s.push_str("#\n# Environment Variables (override config):\n");
    for line in [
        "FLUX9S_SKIN - Override skin (highest priority)",
        "FLUX9S_READ_ONLY - Override readonly mode",
        "FLUX9S_DEFAULT_NAMESPACE - Override default namespace",
        "FLUX9S_DEFAULT_RESOURCE_FILTER - Override default resource filter",
        "FLUX9S_CONNECT_TIMEOUT - Override Kubernetes API connect timeout in seconds",
    ] {
        s.push_str(&format!("#   {line}\n"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dotted camelCase paths for every config field, from the fully
    /// populated skeleton (a serialized real config hides skipped fields).
    fn all_field_paths() -> Vec<String> {
        let skeleton = serde_yaml::to_value(Config::fully_populated()).unwrap();
        let defaults = serde_yaml::to_value(Config::default()).unwrap();
        let mut paths = Vec::new();
        collect_paths(
            skeleton.as_mapping().unwrap(),
            defaults.as_mapping(),
            "",
            &mut paths,
        );
        paths
    }

    fn collect_paths(
        skeleton: &serde_yaml::Mapping,
        defaults: Option<&serde_yaml::Mapping>,
        prefix: &str,
        out: &mut Vec<String>,
    ) {
        use serde_yaml::Value;
        for (key, skel_val) in skeleton {
            let key_str = key.as_str().unwrap();
            let path = if prefix.is_empty() {
                key_str.to_string()
            } else {
                format!("{prefix}.{key_str}")
            };
            let def = defaults.and_then(|m| m.get(key));
            // Recurse into struct sections only — free-form maps hold sample
            // data, not fields (same rule as render_section).
            match skel_val {
                Value::Mapping(inner)
                    if def
                        .and_then(Value::as_mapping)
                        .is_some_and(|m| !m.is_empty()) =>
                {
                    collect_paths(inner, def.and_then(Value::as_mapping), &path, out);
                }
                _ => out.push(path),
            }
        }
    }

    #[test]
    fn listing_shows_every_field_even_when_unset() {
        // A default config skips every optional field when serialized — the
        // listing must show them all anyway (the bug this guards against:
        // discoverFluxResources missing from `config list`).
        let listing = render_config_listing(&Config::default());
        for path in all_field_paths() {
            let leaf = path.rsplit('.').next().unwrap();
            assert!(
                listing.contains(&format!("{leaf}:")),
                "config list output is missing field '{path}'"
            );
        }
        // Everything in a default config is at its default.
        assert!(listing.contains("readOnly: true  # (default)"));
        assert!(listing.contains("favorites: []  # (default)"));
        assert!(listing.contains("contextSkins: {}  # (default)"));
        assert!(listing.contains("defaultResourceFilter: ~  # (default)"));
    }

    #[test]
    fn listing_marks_only_defaults() {
        let mut config = Config {
            discover_flux_resources: true,
            favorites: vec!["Kustomization:flux-system:apps".to_string()],
            ..Config::default()
        };
        config.ui.skin = "nord".to_string();

        let listing = render_config_listing(&config);
        assert!(listing.contains("discoverFluxResources: true\n"));
        assert!(listing.contains("skin: nord\n"));
        assert!(listing.contains("- Kustomization:flux-system:apps"));
        assert!(!listing.contains("discoverFluxResources: true  # (default)"));
        assert!(!listing.contains("skin: nord  # (default)"));
    }

    #[test]
    fn reference_docs_cover_every_field() {
        // Adding a schema field first breaks Config::fully_populated (a
        // compile error), then this test until the reference documents it.
        let reference = reference_text();
        for path in all_field_paths() {
            assert!(
                reference.contains(&path),
                "configuration reference is missing an entry for '{path}'"
            );
        }
    }
}
