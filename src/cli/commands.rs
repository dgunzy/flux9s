//! CLI command handlers

use anyhow::{Context, Result};
use clap::Subcommand;
use std::path::PathBuf;

use crate::config::schema::Config;
use crate::config::{paths, ConfigLoader, ThemeLoader};

/// Display version information
pub fn display_version() {
    println!("flux9s {}", env!("CARGO_PKG_VERSION"));
    println!("  {}", env!("CARGO_PKG_DESCRIPTION"));
    println!("  {}", env!("CARGO_PKG_AUTHORS"));
    println!("  License: {}", env!("CARGO_PKG_LICENSE"));
    println!("  Repository: {}", env!("CARGO_PKG_REPOSITORY"));
}

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
            let cluster = None; // TODO: Get from kubeconfig
            let context = None; // TODO: Get from kubeconfig
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
            let cluster = None; // TODO: Get from kubeconfig
            let context = None; // TODO: Get from kubeconfig
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
            let cluster = None; // TODO: Get from kubeconfig
            let context = None; // TODO: Get from kubeconfig

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
                let cluster = None; // TODO: Get from kubeconfig
                let context = None; // TODO: Get from kubeconfig
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

/// Display configuration with all fields visible, indicating defaults
fn display_config_with_defaults(config: &Config) {
    println!("readOnly: {}", config.read_only);
    println!("defaultNamespace: {}", config.default_namespace);
    println!();
    println!("ui:");
    println!("  enableMouse: {}", config.ui.enable_mouse);
    println!("  headless: {}", config.ui.headless);
    println!("  noIcons: {}", config.ui.no_icons);
    println!("  skin: {}", config.ui.skin);
    if let Some(ref skin_ro) = config.ui.skin_read_only {
        println!("  skinReadOnly: {}", skin_ro);
    } else {
        println!("  skinReadOnly: null  # (default: uses 'skin' when readOnly=true)");
    }
    println!("  splashless: {}", config.ui.splashless);
    println!();
    println!("logger:");
    println!("  tail: {}", config.logger.tail);
    println!("  buffer: {}", config.logger.buffer);
    println!("  sinceSeconds: {}", config.logger.since_seconds);
    println!("  textWrap: {}", config.logger.text_wrap);
    println!();
    if config.namespace_hotkeys.is_empty() {
        println!("namespaceHotkeys: []  # (default: auto-discover at startup)");
    } else {
        println!("namespaceHotkeys:");
        for (idx, ns) in config.namespace_hotkeys.iter().enumerate() {
            println!("  - {}  # Hotkey {}", ns, idx);
        }
    }
    println!();
    if config.context_skins.is_empty() {
        println!("contextSkins: {{}}  # (default: empty, no context-specific skins)");
    } else {
        println!("contextSkins:");
        for (context, skin) in &config.context_skins {
            println!("  {}: {}", context, skin);
        }
    }
    println!();
    println!();
    println!("# Configuration Reference:");
    println!("#   readOnly - Disable modification operations (default: true)");
    println!("#   defaultNamespace - Starting namespace (default: flux-system)");
    println!("#   ui.enableMouse - Enable mouse support (default: false)");
    println!("#   ui.headless - Hide header (default: false)");
    println!("#   ui.noIcons - Disable Unicode icons (default: false)");
    println!("#   ui.skin - Default skin name (default: default)");
    println!("#   ui.skinReadOnly - Skin for readonly mode, overrides ui.skin when readOnly=true");
    println!("#   ui.splashless - Skip startup splash (default: false)");
    println!("#   logger.tail - Default log line count (default: 100)");
    println!("#   logger.buffer - Max log lines in view (default: 5000)");
    println!("#   logger.sinceSeconds - Historical log timeframe in seconds (default: 300)");
    println!("#   logger.textWrap - Enable line wrapping (default: false)");
    println!("#   namespaceHotkeys - Array of namespace names for 0-9 hotkeys (max 10, default: auto-discover)");
    println!("#   contextSkins - Map of context name to skin name (default: empty)");
    println!("#");
    println!("# Environment Variables (override config):");
    println!("#   FLUX9S_SKIN - Override skin (highest priority)");
    println!("#   FLUX9S_READ_ONLY - Override readonly mode");
    println!("#   FLUX9S_DEFAULT_NAMESPACE - Override default namespace");
}
