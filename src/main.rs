//! Flux TUI - A K9s-inspired terminal UI for monitoring Flux GitOps resources
//!
//! This application provides real-time monitoring of Flux resources using
//! the Kubernetes Watch API and a familiar K9s-style interface.

mod cli;
mod config;
mod constants;
mod kube;
mod models;
mod operations;
mod trace;
mod tui;
mod watcher;

use anyhow::Result;
use clap::Parser;

/// Flux TUI - A K9s-inspired terminal UI for monitoring Flux GitOps resources
#[derive(Parser, Debug)]
#[command(name = "flux9s")]
#[command(about = "A K9s-inspired terminal UI for monitoring Flux GitOps resources", long_about = None)]
struct Args {
    /// Enable debug logging
    #[arg(long, short = 'd')]
    debug: bool,

    /// Path to kubeconfig file
    #[arg(long)]
    kubeconfig: Option<std::path::PathBuf>,

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
    /// Display version information
    Version,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Handle version command
    if let Some(Command::Version) = args.command {
        cli::display_version(args.debug);
        return Ok(());
    }

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
    let cluster: Option<&str> = None;
    let context_name: Option<&str> = None;
    let config = config::ConfigLoader::load(cluster, context_name)
        .unwrap_or_else(|_| config::ConfigLoader::load_defaults());

    if args.debug {
        tracing::debug!(
            "Loaded config: splashless={}, show_splash will be {}",
            config.ui.splashless,
            !config.ui.splashless
        );
    }

    let read_only = config.read_only;

    // Determine which skin to use (priority order):
    // 1. FLUX9S_SKIN environment variable (highest priority)
    // 2. Context-specific skin from config.context_skins
    // 3. Readonly-specific skin (config.ui.skin_read_only) if readonly mode
    // 4. Default skin (config.ui.skin)
    let skin_name = if let Ok(env_skin) = std::env::var("FLUX9S_SKIN") {
        tracing::debug!(
            "Using skin from FLUX9S_SKIN environment variable: {}",
            env_skin
        );
        env_skin
    } else if let Some(context) = context_name {
        if let Some(context_skin) = config.context_skins.get(context) {
            tracing::debug!(
                "Using context-specific skin for '{}': {}",
                context,
                context_skin
            );
            context_skin.clone()
        } else if read_only {
            if let Some(ref skin) = config.ui.skin_read_only {
                tracing::debug!("Using readonly-specific skin: {}", skin);
                skin.clone()
            } else {
                tracing::debug!("Using default skin: {}", config.ui.skin);
                config.ui.skin.clone()
            }
        } else {
            tracing::debug!("Using default skin: {}", config.ui.skin);
            config.ui.skin.clone()
        }
    } else if read_only {
        if let Some(ref skin) = config.ui.skin_read_only {
            tracing::debug!("Using readonly-specific skin: {}", skin);
            skin.clone()
        } else {
            tracing::debug!("Using default skin: {}", config.ui.skin);
            config.ui.skin.clone()
        }
    } else {
        tracing::debug!("Using default skin: {}", config.ui.skin);
        config.ui.skin.clone()
    };

    // Load theme based on determined skin name
    let theme = config::ThemeLoader::load_theme(&skin_name).unwrap_or_else(|e| {
        tracing::warn!("Failed to load skin '{}': {}, using default", skin_name, e);
        tui::Theme::default()
    });

    tracing::debug!(
        "Skin loaded: name='{}', readOnly={}, context={:?}",
        skin_name,
        read_only,
        context_name
    );

    // Start TUI immediately with splash screen, then initialize Kubernetes in background
    // This ensures splash appears instantly, not after Kubernetes API calls
    tui::run_tui_with_async_init(config, theme, args.debug, args.kubeconfig.as_deref()).await?;

    // Check for updates after TUI exits (blocking, shows notification)
    // This ensures the notification doesn't interfere with TUI display
    cli::check_for_updates_blocking(args.debug);

    Ok(())
}
