//! Flux TUI - A K9s-inspired terminal UI for monitoring Flux GitOps resources
//!
//! This application provides real-time monitoring of Flux resources using
//! the Kubernetes Watch API and a familiar K9s-style interface.

mod cli;
mod config;
mod kube;
mod models;
mod tui;
mod watcher;

use anyhow::Result;
use clap::Parser;
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
#[derive(clap::Subcommand, Debug)]
enum Command {
    /// Configuration management
    Config {
        #[command(subcommand)]
        subcommand: cli::ConfigSubcommand,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Handle config subcommand
    if let Some(Command::Config { subcommand }) = args.command {
        return cli::handle_config_command(subcommand).await;
    }

    // Initialize logging if debug flag is set
    let log_file = cli::init_logging(args.debug);

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
    // Use config.default_namespace if set, otherwise fall back to environment/default
    let default_namespace = if config.default_namespace.is_empty()
        || config.default_namespace == "all"
        || config.default_namespace == "-A"
    {
        kube::get_default_namespace().await
    } else {
        Some(config.default_namespace.clone())
    };

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
        config,
        theme,
    )
    .await?;

    Ok(())
}
