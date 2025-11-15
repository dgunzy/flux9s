//! Flux TUI - A K9s-inspired terminal UI for monitoring Flux GitOps resources
//!
//! This application provides real-time monitoring of Flux resources using
//! the Kubernetes Watch API and a familiar K9s-style interface.

mod config;
mod kube;
mod models;
mod tui;
mod watcher;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use watcher::{ResourceState, ResourceWatcher};

/// Flux TUI - A K9s-inspired terminal UI for monitoring Flux GitOps resources
#[derive(Parser, Debug)]
#[command(name = "flux9s")]
#[command(about = "A K9s-inspired terminal UI for monitoring Flux GitOps resources", long_about = None)]
struct Args {
    /// Enable debug logging
    #[arg(long, short = 'd')]
    debug: bool,

    /// Configuration subcommand
    #[command(subcommand)]
    command: Option<Command>,
}

/// Main commands
#[derive(Subcommand, Debug)]
enum Command {
    /// Configuration management
    Config {
        #[command(subcommand)]
        subcommand: ConfigSubcommand,
    },
}

/// Configuration management subcommands
#[derive(Subcommand, Debug)]
enum ConfigSubcommand {
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

/// Initialize logging based on debug flag
/// Returns the log file path if debug logging is enabled
fn init_logging(debug: bool) -> Option<PathBuf> {
    if debug {
        // Create a temporary log file using tempfile crate for cross-platform support
        // This works on Windows, macOS, and Linux
        // Use Builder to create a named temp file that persists
        let temp_file = tempfile::Builder::new()
            .prefix("flux9s-")
            .suffix(".log")
            .tempfile()
            .map(|f| {
                let path = f.path().to_path_buf();
                // Keep the file alive by leaking it (it will be cleaned up by the OS)
                // Alternatively, we could use persist(), but that requires a target path
                std::mem::forget(f);
                path
            })
            .unwrap_or_else(|_| {
                // Fallback: create file directly in temp_dir
                let temp_dir = std::env::temp_dir();
                temp_dir.join(format!("flux9s-{}.log", std::process::id()))
            });

        // Open the file for writing (it already exists from tempfile)
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temp_file)
            .expect("Failed to open log file");

        // Enable debug logging with tracing-subscriber
        // Write to file so TUI can use stdout/stderr without interference
        // File implements MakeWriter directly, so we can use it as-is
        tracing_subscriber::fmt()
            .with_writer(file)
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("debug")),
            )
            .with_ansi(false) // No ANSI codes in log file
            .with_target(true)
            .with_file(true)
            .with_line_number(true)
            .init();

        Some(temp_file)
    } else {
        // No logging by default (silent operation)
        None
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Handle config subcommand
    if let Some(Command::Config { subcommand }) = args.command {
        return handle_config_command(subcommand).await;
    }

    // Initialize logging if debug flag is set
    let log_file = init_logging(args.debug);

    // Print log file location to stderr before starting TUI (so it doesn't interfere)
    if let Some(ref log_path) = log_file {
        eprintln!(
            "Debug logging enabled. Logs written to: {}",
            log_path.display()
        );
    }

    if args.debug {
        tracing::debug!("Debug logging enabled");
    }

    // Load configuration
    let cluster: Option<&str> = None; // TODO: Get from kubeconfig
    let context_name: Option<&str> = None; // TODO: Get from kubeconfig
    let config = config::ConfigLoader::load(cluster, context_name)
        .unwrap_or_else(|_| config::ConfigLoader::load_defaults());
    let read_only = config.read_only;

    // Load theme based on config
    let theme = config::ThemeLoader::load_theme(&config.ui.skin).unwrap_or_else(|e| {
        if args.debug {
            tracing::warn!(
                "Failed to load theme '{}': {}, using default",
                config.ui.skin,
                e
            );
        }
        tui::Theme::default()
    });

    if args.debug {
        tracing::debug!(
            "Configuration loaded: readOnly={}, skin={}",
            read_only,
            config.ui.skin
        );
    }

    // Initialize Kubernetes client
    tracing::debug!("Initializing Kubernetes client");
    let client = kube::create_client().await?;
    let context = kube::get_context().await?;
    let default_namespace = kube::get_default_namespace().await;

    if args.debug {
        tracing::info!("Connected to Kubernetes cluster: {}", context);
        if let Some(ref ns) = default_namespace {
            tracing::info!("Default namespace: {}", ns);
        } else {
            tracing::info!("Watching all namespaces");
        }
    }

    // Create resource state and watcher
    tracing::debug!("Creating resource state and watcher");
    let state = ResourceState::new();
    let (mut watcher, event_rx) = ResourceWatcher::new(client.clone(), default_namespace.clone());

    // Start watching all Flux resources
    tracing::debug!("Starting watchers for all Flux resources");
    watcher.watch_all()?;

    // Give watchers a moment to load initial resources
    tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;

    if args.debug {
        tracing::debug!("Watchers initialized, starting TUI");
    }

    // Start TUI immediately (like k9s)
    tui::run_tui(
        state,
        event_rx,
        context,
        default_namespace,
        watcher,
        client,
        read_only,
        theme,
    )
    .await?;

    Ok(())
}

/// Handle configuration subcommands
async fn handle_config_command(cmd: ConfigSubcommand) -> Result<()> {
    use config::{paths, ConfigLoader};

    match cmd {
        ConfigSubcommand::Get { key } => {
            // Load config (will use defaults if no file exists)
            let cluster = None; // TODO: Get from kubeconfig
            let context = None; // TODO: Get from kubeconfig
            let config =
                ConfigLoader::load(cluster, context).context("Failed to load configuration")?;

            if let Some(key) = key {
                // Get specific key
                let value = get_config_value(&config, &key)?;
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
            set_config_value(&mut config, &key, &value)
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
            use config::ThemeLoader;
            let themes = ThemeLoader::list_themes();
            println!("Available themes:");
            for theme in themes {
                println!("  - {}", theme);
            }
        }
        ConfigSubcommand::TestTheme { name } => {
            use config::ThemeLoader;
            match ThemeLoader::load_theme(&name) {
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
                        config::paths::user_skins_dir()
                            .join(format!("{}.yaml", name))
                            .display()
                    );
                    eprintln!(
                        "  - {}",
                        config::paths::skins_dir()
                            .join(format!("{}.yaml", name))
                            .display()
                    );
                    std::process::exit(1);
                }
            }
        }
    }

    Ok(())
}

/// Get a configuration value by key (dot notation)
fn get_config_value(config: &config::schema::Config, key: &str) -> Result<String> {
    match key {
        "readOnly" => Ok(config.read_only.to_string()),
        "defaultNamespace" => Ok(config.default_namespace.clone()),
        "ui.enableMouse" => Ok(config.ui.enable_mouse.to_string()),
        "ui.headless" => Ok(config.ui.headless.to_string()),
        "ui.noIcons" => Ok(config.ui.no_icons.to_string()),
        "ui.skin" => Ok(config.ui.skin.clone()),
        "ui.splashless" => Ok(config.ui.splashless.to_string()),
        "logger.tail" => Ok(config.logger.tail.to_string()),
        "logger.buffer" => Ok(config.logger.buffer.to_string()),
        "logger.sinceSeconds" => Ok(config.logger.since_seconds.to_string()),
        "logger.textWrap" => Ok(config.logger.text_wrap.to_string()),
        _ => Err(anyhow::anyhow!("Unknown configuration key: {}", key)),
    }
}

/// Set a configuration value by key (dot notation)
fn set_config_value(config: &mut config::schema::Config, key: &str, value: &str) -> Result<()> {
    match key {
        "readOnly" => {
            config.read_only = value
                .parse()
                .context("readOnly must be 'true' or 'false'")?;
        }
        "defaultNamespace" => {
            config.default_namespace = value.to_string();
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
        "ui.splashless" => {
            config.ui.splashless = value
                .parse()
                .context("ui.splashless must be 'true' or 'false'")?;
        }
        "logger.tail" => {
            config.logger.tail = value.parse().context("logger.tail must be a number")?;
        }
        "logger.buffer" => {
            config.logger.buffer = value.parse().context("logger.buffer must be a number")?;
        }
        "logger.sinceSeconds" => {
            config.logger.since_seconds = value
                .parse()
                .context("logger.sinceSeconds must be a number")?;
        }
        "logger.textWrap" => {
            config.logger.text_wrap = value
                .parse()
                .context("logger.textWrap must be 'true' or 'false'")?;
        }
        _ => return Err(anyhow::anyhow!("Unknown configuration key: {}", key)),
    }

    Ok(())
}
