//! CLI command handlers

use anyhow::{Context, Result};
use clap::Subcommand;

use crate::config::{paths, ConfigLoader, ThemeLoader};

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
    /// List available themes/skins
    Themes,
    /// Test loading a theme
    TestTheme {
        /// Theme name to test
        name: String,
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

            let yaml =
                serde_yaml::to_string(&config).context("Failed to serialize configuration")?;
            print!("{}", yaml);
        }
        ConfigSubcommand::Path => {
            let config_path = paths::root_config_path();
            println!("{}", config_path.display());
        }
        ConfigSubcommand::Validate => {
            let cluster = None; // TODO: Get from kubeconfig
            let context = None; // TODO: Get from kubeconfig
            match ConfigLoader::load(cluster, context) {
                Ok(_) => {
                    println!("Configuration is valid");
                }
                Err(e) => {
                    eprintln!("Configuration validation failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
        ConfigSubcommand::Themes => {
            let themes = ThemeLoader::list_themes();
            println!("Available themes:");
            for theme in themes {
                println!("  - {}", theme);
            }
        }
        ConfigSubcommand::TestTheme { name } => match ThemeLoader::load_theme(&name) {
            Ok(theme) => {
                println!("✓ Successfully loaded theme: {}", name);
                println!("\nTheme colors:");
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
                eprintln!("✗ Failed to load theme '{}': {}", name, e);
                eprintln!("\nChecked locations:");
                eprintln!(
                    "  - {}",
                    paths::user_skins_dir()
                        .join(format!("{}.yaml", name))
                        .display()
                );
                eprintln!(
                    "  - {}",
                    paths::skins_dir().join(format!("{}.yaml", name)).display()
                );
                std::process::exit(1);
            }
        },
    }

    Ok(())
}
