//! TUI module
//!
//! Provides the terminal user interface for Flux TUI.
//! Built with ratatui for a K9s-inspired experience.

mod api;
mod app;
mod operations;
mod theme;
mod trace;
pub mod views;

pub use app::*;
pub use operations::*;
pub use theme::*;
// trace module functions are used internally, not exported

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use kube::Api;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;

// Helper function to fetch resource YAML from API
async fn fetch_resource_yaml(
    client: &kube::Client,
    resource_type: &str,
    namespace: &str,
    name: &str,
) -> anyhow::Result<serde_json::Value> {
    // Import resource types - use the public re-exports from watcher module
    use crate::models::FluxResourceKind;
    use crate::watcher::{
        Alert, Bucket, ExternalArtifact, GitRepository, HelmChart, HelmRelease, HelmRepository,
        ImagePolicy, ImageRepository, ImageUpdateAutomation, Kustomization, OCIRepository,
        Provider, Receiver,
    };

    // Match resource type and fetch using appropriate API
    macro_rules! fetch_resource {
        ($type:ty) => {{
            let api: Api<$type> = Api::namespaced(client.clone(), namespace);
            match api.get(name).await {
                Ok(obj) => {
                    return Ok(serde_json::to_value(&obj)?);
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("Failed to fetch {}: {}", resource_type, e));
                }
            }
        }};
    }

    match FluxResourceKind::from_str(resource_type) {
        Some(FluxResourceKind::GitRepository) => fetch_resource!(GitRepository),
        Some(FluxResourceKind::OCIRepository) => fetch_resource!(OCIRepository),
        Some(FluxResourceKind::HelmRepository) => fetch_resource!(HelmRepository),
        Some(FluxResourceKind::Bucket) => fetch_resource!(Bucket),
        Some(FluxResourceKind::HelmChart) => fetch_resource!(HelmChart),
        Some(FluxResourceKind::ExternalArtifact) => fetch_resource!(ExternalArtifact),
        Some(FluxResourceKind::Kustomization) => fetch_resource!(Kustomization),
        Some(FluxResourceKind::HelmRelease) => fetch_resource!(HelmRelease),
        Some(FluxResourceKind::ImageRepository) => fetch_resource!(ImageRepository),
        Some(FluxResourceKind::ImagePolicy) => fetch_resource!(ImagePolicy),
        Some(FluxResourceKind::ImageUpdateAutomation) => fetch_resource!(ImageUpdateAutomation),
        Some(FluxResourceKind::Alert) => fetch_resource!(Alert),
        Some(FluxResourceKind::Provider) => fetch_resource!(Provider),
        Some(FluxResourceKind::Receiver) => fetch_resource!(Receiver),
        None => Err(anyhow::anyhow!("Unknown resource type: {}", resource_type)),
    }
}

/// Run the TUI application
pub async fn run_tui(
    state: crate::watcher::ResourceState,
    mut event_rx: tokio::sync::mpsc::UnboundedReceiver<crate::watcher::WatchEvent>,
    context: String,
    namespace: Option<String>,
    watcher: crate::watcher::ResourceWatcher,
    client: kube::Client,
    config: crate::config::Config,
    theme: crate::tui::Theme,
) -> Result<()> {
    tracing::debug!("Initializing TUI");

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    // Conditionally enable mouse capture based on config
    if config.ui.enable_mouse {
        execute!(stdout, EnableMouseCapture)?;
    }
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state with config and theme
    let mut app = App::new(state, context, namespace.clone(), config.clone(), theme);
    app.set_watcher(watcher);
    app.set_kube_client(client);

    tracing::debug!("TUI initialized, entering main loop");

    // Main event loop
    loop {
        terminal.draw(|f| app.render(f))?;

        // Check if we need to fetch YAML asynchronously
        if let Some((key, client, tx)) = app.trigger_yaml_fetch() {
            // Parse key: format is "resource_type:namespace:name"
            let parts: Vec<&str> = key.split(':').collect();
            if parts.len() == 3 {
                let resource_type = parts[0].to_string();
                let namespace = parts[1].to_string();
                let name = parts[2].to_string();

                tracing::debug!(
                    "Fetching YAML for {}/{} in namespace {}",
                    resource_type,
                    name,
                    namespace
                );

                // Spawn async task to fetch resource
                let client_clone = client.clone();
                tokio::spawn(async move {
                    let result =
                        fetch_resource_yaml(&client_clone, &resource_type, &namespace, &name).await;
                    if let Err(ref e) = result {
                        tracing::warn!(
                            "Failed to fetch YAML for {}/{}: {}",
                            resource_type,
                            name,
                            e
                        );
                    } else {
                        tracing::debug!("Successfully fetched YAML for {}/{}", resource_type, name);
                    }
                    let _ = tx.send(result);
                });
            }
        }

        // Check if we need to trace a resource asynchronously
        if let Some((resource_type, namespace, name, client, tx)) = app.trigger_trace() {
            tracing::debug!(
                "Tracing {}/{} in namespace {}",
                resource_type,
                name,
                namespace
            );

            // Spawn async task to trace resource
            let client_clone = client.clone();
            tokio::spawn(async move {
                use crate::tui::trace;
                let result =
                    trace::trace_object(&client_clone, &resource_type, &namespace, &name).await;
                match result {
                    Ok(trace_result) => {
                        tracing::debug!("Successfully traced {}/{}", resource_type, name);
                        let _ = tx.send(Ok(trace_result));
                    }
                    Err(e) => {
                        tracing::warn!("Failed to trace {}/{}: {}", resource_type, name, e);
                        let _ = tx.send(Err(anyhow::anyhow!("Trace failed: {}", e)));
                    }
                }
            });
        }

        // Check for trace results
        if let Some(result) = app.try_get_trace_result() {
            match result {
                Ok(trace_result) => {
                    app.set_trace_result(trace_result);
                    // Switch to trace view - use public method if available
                    // For now, we'll set it via a method we need to add
                    app.set_view_trace();
                }
                Err(e) => {
                    app.set_trace_error();
                    app.set_status_message((format!("Trace failed: {}", e), true));
                }
            }
        }

        // Check for YAML fetch results
        if let Some(result) = app.try_get_yaml_result() {
            match result {
                Ok(yaml) => app.set_yaml_fetched(yaml),
                Err(_) => app.set_yaml_fetch_error(),
            }
        }

        // Check if we need to execute an operation asynchronously
        if let Some((resource_type, namespace, name, op_key, client, tx)) =
            app.trigger_operation_execution()
        {
            // We need to get the operation from the registry, but we can't store a reference
            // So we'll use a different approach - pass the operation key and look it up in the spawned task
            let op_key_clone = op_key;
            let client_clone = client.clone();
            let rt_clone = resource_type.clone();
            let ns_clone = namespace.clone();
            let n_clone = name.clone();

            tracing::debug!(
                "Executing operation '{}' on {}/{} in namespace {}",
                op_key_clone,
                rt_clone,
                n_clone,
                ns_clone
            );

            tokio::spawn(async move {
                // Create a new registry instance in the spawned task
                // This is safe because operations are stateless
                let registry = OperationRegistry::new();
                if let Some(operation) = registry.get_by_keybinding(op_key_clone) {
                    let result = operation
                        .execute(&client_clone, &rt_clone, &ns_clone, &n_clone)
                        .await;
                    match &result {
                        Ok(_) => tracing::info!(
                            "Operation '{}' succeeded on {}/{}",
                            op_key_clone,
                            rt_clone,
                            n_clone
                        ),
                        Err(e) => tracing::warn!(
                            "Operation '{}' failed on {}/{}: {}",
                            op_key_clone,
                            rt_clone,
                            n_clone,
                            e
                        ),
                    }
                    let _ = tx.send(result);
                } else {
                    tracing::warn!("Unknown operation keybinding: {}", op_key_clone);
                    let _ = tx.send(Err(anyhow::anyhow!("Unknown operation")));
                }
            });
        }

        // Check for operation execution results
        if let Some(result) = app.try_get_operation_result() {
            app.set_operation_result(result);
        }

        // Handle input events (non-blocking)
        if crossterm::event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    if let Some(should_quit) = app.handle_key(key) {
                        if should_quit {
                            break;
                        }
                    }
                }
            }
        }

        // Process watch events (non-blocking)
        // Update state from watch events
        let mut events_processed = 0;
        while let Ok(event) = event_rx.try_recv() {
            events_processed += 1;
            match event {
                crate::watcher::WatchEvent::Applied(resource_type, ns, name, obj_json) => {
                    let key = crate::watcher::resource_key(&ns, &name, &resource_type);
                    let (suspended, ready, message, revision) =
                        crate::watcher::extract_status_fields(&obj_json);
                    app.state().upsert(
                        key.clone(),
                        crate::watcher::ResourceInfo {
                            name,
                            namespace: ns,
                            resource_type,
                            age: Some(chrono::Utc::now()),
                            suspended,
                            ready,
                            message,
                            revision,
                        },
                    );
                    // Store full object for detail view
                    {
                        let mut objects = app.resource_objects().write().unwrap();
                        objects.insert(key.clone(), obj_json);
                    }
                }
                crate::watcher::WatchEvent::Deleted(resource_type, ns, name) => {
                    let key = crate::watcher::resource_key(&ns, &name, &resource_type);
                    app.state().remove(&key);
                }
                crate::watcher::WatchEvent::Error(msg) => {
                    // Log errors but don't spam - only show first few
                    // Errors are also shown in the TUI if needed
                    tracing::warn!("Watch event error: {}", msg);
                }
            }
        }

        // Force a redraw if we processed events
        if events_processed > 0 {
            terminal.draw(|f| app.render(f))?;
        }
    }

    tracing::debug!("TUI shutting down");

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    // Only disable mouse if it was enabled
    if config.ui.enable_mouse {
        execute!(terminal.backend_mut(), DisableMouseCapture)?;
    }
    terminal.show_cursor()?;

    Ok(())
}
