//! Flux TUI - A K9s-inspired terminal UI for monitoring Flux GitOps resources
//!
//! This application provides real-time monitoring of Flux resources using
//! the Kubernetes Watch API and a familiar K9s-style interface.

mod kube;
mod models;
mod tui;
mod watcher;

use anyhow::Result;
use watcher::{ResourceState, ResourceWatcher};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize Kubernetes client silently
    let client = kube::create_client().await?;
    let context = kube::get_context().await?;
    let default_namespace = kube::get_default_namespace().await;

    // Create resource state and watcher
    let state = ResourceState::new();
    let (mut watcher, event_rx) = ResourceWatcher::new(client.clone(), default_namespace.clone());

    // Start watching all Flux resources
    watcher.watch_all()?;

    // Give watchers a moment to load initial resources (silently)
    tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;

    // Start TUI immediately (like k9s)
    tui::run_tui(state, event_rx, context, default_namespace, watcher, client).await?;

    Ok(())
}
