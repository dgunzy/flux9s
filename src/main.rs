//! Flux TUI - A K9s-inspired terminal UI for monitoring Flux GitOps resources
//!
//! This application provides real-time monitoring of Flux resources using
//! the Kubernetes Watch API and a familiar K9s-style interface.

mod kube;
mod models;
mod tui;
mod watcher;

use anyhow::Result;
use clap::Parser;
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
    tui::run_tui(state, event_rx, context, default_namespace, watcher, client).await?;

    Ok(())
}
