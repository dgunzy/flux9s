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

pub use api::{get_api_resource_with_fallback, get_gvk_for_resource_type};

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

use crate::watcher::ResourceKey;

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
        Alert, Bucket, ExternalArtifact, FluxInstance, FluxReport, GitRepository, HelmChart,
        HelmRelease, HelmRepository, ImagePolicy, ImageRepository, ImageUpdateAutomation,
        Kustomization, OCIRepository, Provider, Receiver, ResourceSet, ResourceSetInputProvider,
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

    match FluxResourceKind::parse_optional(resource_type) {
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
        Some(FluxResourceKind::ResourceSet) => fetch_resource!(ResourceSet),
        Some(FluxResourceKind::ResourceSetInputProvider) => {
            fetch_resource!(ResourceSetInputProvider)
        }
        Some(FluxResourceKind::FluxReport) => fetch_resource!(FluxReport),
        Some(FluxResourceKind::FluxInstance) => fetch_resource!(FluxInstance),
        None => Err(anyhow::anyhow!("Unknown resource type: {}", resource_type)),
    }
}

/// Run the TUI application with async Kubernetes initialization
/// This shows the splash screen immediately, then initializes Kubernetes in the background
pub async fn run_tui_with_async_init(
    config: crate::config::Config,
    theme: crate::tui::Theme,
    debug: bool,
    kubeconfig_path: Option<&std::path::Path>,
) -> Result<()> {
    tracing::debug!("Initializing TUI with async Kubernetes setup");

    // Setup terminal IMMEDIATELY - this is the first thing we do
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    // Conditionally enable mouse capture based on config
    if config.ui.enable_mouse {
        execute!(stdout, EnableMouseCapture)?;
    }
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state with empty state - will be populated by async initialization
    let state = crate::watcher::ResourceState::new();

    // Debug: Log splashless config value
    tracing::debug!(
        "Creating app with splashless={}, show_splash will be {}",
        config.ui.splashless,
        !config.ui.splashless
    );

    let mut app = App::new(
        state,
        "Connecting...".to_string(), // Placeholder context
        None,                        // Placeholder namespace
        config.clone(),
        theme,
    );

    // Initialize splash timer right before first render
    // This ensures the timer starts when TUI actually renders, not during async initialization
    app.init_splash_timer();

    // Spawn async task to initialize Kubernetes and start watchers
    // This happens in the background while splash is showing
    let kubeconfig_path_clone = kubeconfig_path.map(|p| p.to_path_buf());
    let (kube_init_tx, mut kube_init_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        tracing::debug!("Starting async Kubernetes initialization");

        // Initialize Kubernetes client - use kubeconfig path if provided
        let client = match kubeconfig_path_clone {
            Some(ref path) => {
                tracing::debug!("Using kubeconfig from: {}", path.display());
                match crate::kube::create_client_from_kubeconfig_path(path).await {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::error!(
                            "Failed to create Kubernetes client from {}: {}",
                            path.display(),
                            e
                        );
                        let _ = kube_init_tx.send(Err(e));
                        return;
                    }
                }
            }
            None => match crate::kube::create_client().await {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("Failed to create Kubernetes client: {}", e);
                    let _ = kube_init_tx.send(Err(e));
                    return;
                }
            },
        };

        // Get context - use kubeconfig path if provided
        let context = match kubeconfig_path_clone {
            Some(ref path) => match crate::kube::get_context_from_kubeconfig_path(path) {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!(
                        "Failed to get Kubernetes context from {}: {}",
                        path.display(),
                        e
                    );
                    let _ = kube_init_tx.send(Err(e));
                    return;
                }
            },
            None => match crate::kube::get_context().await {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("Failed to get Kubernetes context: {}", e);
                    let _ = kube_init_tx.send(Err(e));
                    return;
                }
            },
        };

        // Use config.default_namespace if set, otherwise fall back to environment/default
        let default_namespace = if config.default_namespace.is_empty()
            || config.default_namespace == "all"
            || config.default_namespace == "-A"
        {
            crate::kube::get_default_namespace().await
        } else {
            Some(config.default_namespace.clone())
        };

        if debug {
            tracing::info!("Connected to Kubernetes cluster: {}", context);
            if let Some(ref ns) = default_namespace {
                tracing::info!("Default namespace: {}", ns);
            } else {
                tracing::info!("Watching all namespaces");
            }
        }

        // Create resource state and watcher
        tracing::debug!("Creating resource state and watcher");
        let (mut watcher, event_rx) =
            crate::watcher::ResourceWatcher::new(client.clone(), default_namespace.clone());

        // Start watching all Flux resources
        if let Err(e) = watcher.watch_all() {
            tracing::error!("Failed to start watchers: {}", e);
            let _ = kube_init_tx.send(Err(anyhow::anyhow!("Failed to start watchers: {}", e)));
            return;
        }

        // Discover namespaces with Flux resources for hotkeys (if not configured)
        let namespace_hotkeys = if config.namespace_hotkeys.is_empty() {
            crate::kube::discover_namespaces_with_flux_resources(&client)
                .await
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        let _ = kube_init_tx.send(Ok((
            client,
            context,
            default_namespace,
            watcher,
            event_rx,
            namespace_hotkeys,
        )));
    });

    // Main event loop - start rendering immediately with splash
    let mut event_rx: Option<tokio::sync::mpsc::UnboundedReceiver<crate::watcher::WatchEvent>> =
        None;
    let mut kube_client: Option<kube::Client> = None;
    let mut kube_initialized = false;

    loop {
        // Check if Kubernetes initialization is complete
        if !kube_initialized {
            if let Ok(result) = kube_init_rx.try_recv() {
                match result {
                    Ok((client, context, namespace, w, rx, namespace_hotkeys)) => {
                        tracing::debug!("Kubernetes initialization complete");
                        kube_client = Some(client.clone());
                        event_rx = Some(rx);
                        app.set_kube_client(client.clone());
                        app.set_watcher(w);
                        app.set_context(context);
                        app.set_namespace(namespace.clone());

                        if !namespace_hotkeys.is_empty() {
                            app.update_namespace_hotkeys(namespace_hotkeys);
                            tracing::debug!(
                                "Discovered {} namespaces for hotkeys",
                                app.namespace_hotkeys().len()
                            );
                        }

                        kube_initialized = true;
                    }
                    Err(e) => {
                        tracing::error!("Kubernetes initialization failed: {}", e);

                        // Clean up terminal before exiting
                        disable_raw_mode()?;
                        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                        if config.ui.enable_mouse {
                            execute!(terminal.backend_mut(), DisableMouseCapture)?;
                        }

                        // Print error to stderr and exit
                        eprintln!("Error: Failed to connect to Kubernetes: {}", e);
                        if let Some(kubeconfig_path) = kubeconfig_path {
                            eprintln!("Kubeconfig file: {}", kubeconfig_path.display());
                        }
                        std::process::exit(1);
                    }
                }
            }
        }

        terminal.draw(|f| app.render(f))?;

        // Check if we need to fetch YAML asynchronously
        // Only if Kubernetes is initialized
        if kube_initialized {
            if let Some((key, client, tx)) = app.trigger_yaml_fetch() {
                // Parse key using type-safe ResourceKey
                if let Some(rk) = ResourceKey::parse(&key) {
                    tracing::debug!(
                        "Fetching YAML for {}/{} in namespace {}",
                        rk.resource_type,
                        rk.name,
                        rk.namespace
                    );

                    // Spawn async task to fetch resource
                    let client_clone = client.clone();
                    tokio::spawn(async move {
                        let result = fetch_resource_yaml(
                            &client_clone,
                            &rk.resource_type,
                            &rk.namespace,
                            &rk.name,
                        )
                        .await;
                        if let Err(ref e) = result {
                            tracing::warn!(
                                "Failed to fetch YAML for {}/{} in namespace {}: {}",
                                rk.resource_type,
                                rk.name,
                                rk.namespace,
                                e
                            );
                        } else {
                            tracing::debug!(
                                "Successfully fetched YAML for {}/{}",
                                rk.resource_type,
                                rk.name
                            );
                        }
                        let _ = tx.send(result);
                    });
                } else {
                    tracing::error!("Failed to parse resource key for YAML fetch: {}", key);
                    let _ = tx.send(Err(anyhow::anyhow!("Invalid resource key format: {}", key)));
                }
            }
        }

        // Check if we need to trace a resource asynchronously
        // Only if Kubernetes is initialized
        if kube_initialized {
            if let Some(req) = app.trigger_trace() {
                tracing::debug!(
                    "Tracing {}/{} in namespace {}",
                    req.resource_type,
                    req.name,
                    req.namespace
                );

                // Spawn async task to trace resource
                let client_clone = req.client.clone();
                let resource_type = req.resource_type;
                let namespace = req.namespace;
                let name = req.name;
                let tx = req.tx;
                tokio::spawn(async move {
                    use crate::tui::trace;
                    let result =
                        trace::trace_object(&client_clone, &resource_type, &namespace, &name).await;
                    match result {
                        Ok(trace_result) => {
                            tracing::debug!(
                                "Successfully traced {}/{} in namespace {}",
                                resource_type,
                                name,
                                namespace
                            );
                            let _ = tx.send(Ok(trace_result));
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Failed to trace {}/{} in namespace {}: {}",
                                resource_type,
                                name,
                                namespace,
                                e
                            );
                            let _ = tx.send(Err(anyhow::anyhow!(
                                "Trace failed for {}/{} in {}: {}",
                                resource_type,
                                name,
                                namespace,
                                e
                            )));
                        }
                    }
                });
            }
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
                Err(e) => {
                    tracing::debug!("YAML fetch error result received: {}", e);
                    app.set_yaml_fetch_error();
                    app.set_status_message((format!("Failed to fetch YAML: {}", e), true));
                }
            }
        }

        // Check if we need to execute an operation asynchronously
        if let Some(req) = app.trigger_operation_execution() {
            // We need to get the operation from the registry, but we can't store a reference
            // So we'll use a different approach - pass the operation key and look it up in the spawned task
            let op_key = req.operation_key;
            let client_clone = req.client.clone();
            let resource_type = req.resource_type;
            let namespace = req.namespace;
            let name = req.name;
            let tx = req.tx;

            tracing::debug!(
                "Executing operation '{}' on {}/{} in namespace {}",
                op_key,
                resource_type,
                name,
                namespace
            );

            tokio::spawn(async move {
                // Create a new registry instance in the spawned task
                // This is safe because operations are stateless
                let registry = OperationRegistry::new();
                if let Some(operation) = registry.get_by_keybinding(op_key) {
                    let result = operation
                        .execute(&client_clone, &resource_type, &namespace, &name)
                        .await;
                    match &result {
                        Ok(_) => tracing::info!(
                            "Operation '{}' succeeded on {}/{}",
                            op_key,
                            resource_type,
                            name
                        ),
                        Err(e) => tracing::warn!(
                            "Operation '{}' failed on {}/{}: {}",
                            op_key,
                            resource_type,
                            name,
                            e
                        ),
                    }
                    let _ = tx.send(result);
                } else {
                    tracing::warn!("Unknown operation keybinding: {}", op_key);
                    let _ = tx.send(Err(anyhow::anyhow!("Unknown operation")));
                }
            });
        }

        // Check for operation execution results
        if let Some(result) = app.try_get_operation_result() {
            app.set_operation_result(result);
        }

        // Handle context switch if pending
        if let Some(new_context) = app.take_pending_context_switch() {
            tracing::info!("Switching to context: {}", new_context);

            if kube_client.is_some() {
                match crate::kube::create_client_for_context(&new_context).await {
                    Ok(new_client) => {
                        // Create new watcher with new client
                        // ResourceWatcher::new returns a tuple (watcher, receiver), not a Result
                        let namespace = app.namespace().clone();
                        let (mut new_watcher, new_event_rx) =
                            crate::watcher::ResourceWatcher::new(new_client.clone(), namespace);

                        // Start watching all resources with the new watcher
                        if let Err(e) = new_watcher.watch_all() {
                            tracing::error!("Failed to start watchers after context switch: {}", e);
                            app.set_status_message((
                                format!("Failed to start watchers: {}", e),
                                true,
                            ));
                            return Ok(());
                        }

                        // Update app with new context and watcher
                        app.complete_context_switch(new_context.clone());
                        app.set_kube_client(new_client.clone());
                        app.set_watcher(new_watcher);

                        // Replace event receiver
                        event_rx = Some(new_event_rx);

                        app.set_status_message((
                            format!("Successfully switched to context: {}", new_context),
                            false,
                        ));

                        // Reload skin for new context
                        app.reload_skin_for_readonly_mode(Some(&new_context));

                        tracing::info!("Context switch completed: {}", new_context);
                    }
                    Err(e) => {
                        app.set_status_message((format!("Failed to switch context: {}", e), true));
                        tracing::error!("Context switch failed: {}", e);
                    }
                }
            }
        }

        // Check status message timeout (non-blocking check)
        app.check_status_message_timeout();

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
        // Track resource type count to detect when header layout needs recalculation
        let resource_type_count_before = app.state().count_by_type().len();

        if let Some(ref mut rx) = event_rx {
            while let Ok(event) = rx.try_recv() {
                events_processed += 1;
                match event {
                    crate::watcher::WatchEvent::Applied(resource_type, ns, name, obj_json) => {
                        let key = crate::watcher::resource_key(&ns, &name, &resource_type);
                        let (suspended, ready, message, revision) =
                            crate::watcher::extract_status_fields(&obj_json);
                        let labels = crate::watcher::extract_labels(&obj_json);
                        let annotations = crate::watcher::extract_annotations(&obj_json);
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
                                labels,
                                annotations,
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
        }

        // Force a redraw if we processed events
        if events_processed > 0 {
            // Check if number of resource types changed (affects header layout)
            let resource_type_count_after = app.state().count_by_type().len();
            if resource_type_count_after != resource_type_count_before {
                app.notify_resource_types_changed();
            }
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
