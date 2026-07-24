//! TUI module
//!
//! Provides the terminal user interface for Flux TUI.
//! Built with ratatui for a K9s-inspired experience.

pub mod app;
mod commands;
pub mod constants;
mod keybindings;
pub mod operations;
mod submenu;
mod theme;
pub mod trace;
pub mod views;

#[allow(unused_imports)] // Re-exported for backward compatibility
pub use crate::kube::api::{get_api_resource_with_fallback, get_gvk_for_resource_type};
pub use app::App;
pub use operations::*;
pub use theme::*;

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;

use crate::models::FluxResourceKind;

pub use crate::kube::fetch::fetch_resource_yaml;

/// Extract Flux bundle version from deployment metadata labels
/// Returns the app.kubernetes.io/version label if present (e.g., "v2.7.5")
fn extract_flux_bundle_version(deployment_json: &serde_json::Value) -> Option<String> {
    deployment_json["metadata"]["labels"]["app.kubernetes.io/version"]
        .as_str()
        .map(|s| s.to_string())
}

/// Parse Flux controller pod status from Kubernetes API JSON
fn extract_controller_pod_info(
    pod_json: &serde_json::Value,
) -> Option<crate::tui::app::state::ControllerPodInfo> {
    let name = pod_json["metadata"]["name"].as_str()?.to_string();

    // Extract version from container image tag
    let containers = pod_json["spec"]["containers"].as_array()?;
    let version = containers.first().and_then(|c| {
        c["image"].as_str().and_then(|img| {
            // Handle different image formats:
            // - With tag: "ghcr.io/fluxcd/source-controller:v1.4.5"
            // - With digest: "ghcr.io/fluxcd/source-controller@sha256:abc123..."
            // - Both: "ghcr.io/fluxcd/source-controller:v1.4.5@sha256:abc123..."

            if let Some(at_pos) = img.find('@') {
                // Image uses digest format - extract tag before @ if present
                let before_digest = &img[..at_pos];
                before_digest
                    .rfind(':')
                    .map(|colon_pos| before_digest[colon_pos + 1..].to_string())
            } else if let Some(colon_pos) = img.rfind(':') {
                // Tag-based image (no digest)
                let tag = &img[colon_pos + 1..];
                // Skip if it looks like a port number
                if tag.chars().all(|c| c.is_ascii_digit()) {
                    None
                } else {
                    Some(tag.to_string())
                }
            } else {
                None
            }
        })
    });

    let ready = pod_json["status"]["conditions"]
        .as_array()
        .and_then(|arr| arr.iter().find(|c| c["type"] == "Ready"))
        .and_then(|c| c["status"].as_str())
        .map(|s| s == "True")
        .unwrap_or(false);

    Some(crate::tui::app::state::ControllerPodInfo {
        name,
        ready,
        version,
    })
}

/// Best-effort terminal restore: disable raw mode, leave the alternate screen,
/// release mouse capture, and show the cursor. Safe to call multiple times and
/// when the corresponding mode was never enabled. Used by the normal exit path,
/// the error path, and the panic hook so a crash never leaves the user's
/// terminal in raw mode.
fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(
        io::stdout(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        crossterm::cursor::Show
    );
}

/// Run the TUI application with async Kubernetes initialization
/// This shows the splash screen immediately, then initializes Kubernetes in the background
pub async fn run_tui_with_async_init(
    config: crate::config::Config,
    theme: crate::tui::Theme,
    debug: bool,
    kubeconfig_path: Option<&std::path::Path>,
    config_warning: Option<String>,
    log_file: Option<std::path::PathBuf>,
) -> Result<()> {
    use crate::kube::health::ConnectionError;
    tracing::debug!("Initializing TUI with async Kubernetes setup");

    // Install a panic hook BEFORE entering raw mode so a panic anywhere in the
    // TUI (including rendering and event handling) restores the terminal before
    // the default hook prints the panic message.
    let original_panic_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        restore_terminal();
        original_panic_hook(panic_info);
    }));

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

    // Record the log file path so the connection error screen can point to it.
    app.set_log_path(log_file);

    // Initialize splash timer right before first render
    // This ensures the timer starts when TUI actually renders, not during async initialization
    app.init_splash_timer();

    // Bounded timeout for the startup connectivity probe (env-overridable).
    let connect_timeout =
        crate::kube::health::resolve_connect_timeout(config.connect_timeout_seconds);

    // Spawn async task to initialize Kubernetes and start watchers
    // This happens in the background while splash is showing
    let kubeconfig_path_clone = kubeconfig_path.map(|p| p.to_path_buf());
    let controller_namespace = config.default_controller_namespace.clone();
    let controller_namespace_for_init = controller_namespace.clone();
    let discovery_enabled = config.discover_flux_resources;
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
                        let _ = kube_init_tx.send(Err(ConnectionError::from_anyhow(e)));
                        return;
                    }
                }
            }
            None => match crate::kube::create_client().await {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("Failed to create Kubernetes client: {}", e);
                    let _ = kube_init_tx.send(Err(ConnectionError::from_anyhow(e)));
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
                    let _ = kube_init_tx.send(Err(ConnectionError::from_anyhow(e)));
                    return;
                }
            },
            None => match crate::kube::get_context().await {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("Failed to get Kubernetes context: {}", e);
                    let _ = kube_init_tx.send(Err(ConnectionError::from_anyhow(e)));
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

        // Active connectivity probe with a bounded timeout. Building a client does
        // no network I/O, so without this an unreachable/timed-out/unauthorized
        // API server would not be detected until watchers silently failed in the
        // background. Hitting /version is cheap and requires no RBAC.
        let server_url = crate::kube::health::detect_cluster_server(
            kubeconfig_path_clone.as_deref(),
            Some(&context),
        );
        if let Err(conn_err) =
            crate::kube::health::check_connectivity(&client, connect_timeout).await
        {
            let conn_err = conn_err
                .with_context(Some(context.clone()))
                .with_server(server_url.clone());
            tracing::error!("Kubernetes connectivity check failed: {}", conn_err);
            let _ = kube_init_tx.send(Err(conn_err));
            return;
        }

        // Create resource state and watcher
        tracing::debug!("Creating resource state and watcher");
        let (mut watcher, event_rx) = crate::watcher::ResourceWatcher::new(
            client.clone(),
            default_namespace.clone(),
            controller_namespace_for_init,
            discovery_enabled,
        );

        // Start watching all Flux resources
        if let Err(e) = watcher.watch_all() {
            tracing::error!("Failed to start watchers: {}", e);
            let _ = kube_init_tx.send(Err(ConnectionError::from_anyhow(e)
                .with_context(Some(context.clone()))
                .with_server(server_url.clone())));
            return;
        }

        // Discover namespaces with Flux resources for hotkeys (if not configured)
        let namespace_hotkeys = if config.namespace_hotkeys.is_empty() {
            // Bounded so a slow/unreachable server can't stall startup here.
            match tokio::time::timeout(
                connect_timeout,
                crate::kube::discover_namespaces_with_flux_resources(&client),
            )
            .await
            {
                Ok(Ok(ns)) => ns,
                Ok(Err(e)) => {
                    tracing::warn!("Namespace discovery failed: {}", e);
                    Vec::new()
                }
                Err(_) => {
                    tracing::warn!("Namespace discovery timed out");
                    Vec::new()
                }
            }
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
    let mut kube_initialized = false;
    // Tracks whether the init channel has resolved (success OR failure), so a
    // failed connection stops polling without being treated as initialized.
    let mut kube_init_done = false;

    // Run the event loop inside an async block so any `?` error inside it
    // still falls through to the terminal restore below instead of leaving
    // the terminal in raw mode.
    let run_result: Result<()> = async {
        loop {
            // Check if Kubernetes initialization is complete
            if !kube_init_done {
                if let Ok(result) = kube_init_rx.try_recv() {
                    kube_init_done = true;
                    match result {
                        Ok((client, context, namespace, w, rx, namespace_hotkeys)) => {
                            tracing::debug!("Kubernetes initialization complete");
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

                            if let Some(ref filter) = config.default_resource_filter {
                                app.view_state.selected_resource_type = Some(filter.clone());
                                tracing::debug!("Applied default resource filter: {}", filter);
                            }

                            if let Some(ref warning) = config_warning {
                                app.set_status_message((warning.clone(), true));
                            }

                            app.set_connected();
                            kube_initialized = true;
                        }
                        Err(conn_err) => {
                            // Keep the TUI alive and show a clear, full-screen error
                            // instead of tearing down and exiting. The user reads the
                            // message and quits with q / Ctrl+C.
                            tracing::error!("Kubernetes initialization failed: {}", conn_err);
                            app.set_connection_error(conn_err);
                        }
                    }
                }
            }

            terminal.draw(|f| app.render(f))?;

            // Dispatch queued view fetches (YAML, describe, trace, graph).
            // Each spawned task reports back through its AsyncTask channel.
            if kube_initialized {
                if let Some(client) = app.kube_client.clone() {
                    if let Some((rk, tx)) = app.async_state.yaml.dispatch() {
                        let client = client.clone();
                        tokio::spawn(async move {
                            tracing::debug!("Fetching YAML for {}", rk);
                            let result = fetch_resource_yaml(
                                &client,
                                &rk.resource_type,
                                &rk.namespace,
                                &rk.name,
                            )
                            .await;
                            if let Err(ref e) = result {
                                tracing::warn!("Failed to fetch YAML for {}: {}", rk, e);
                            }
                            let _ = tx.send(result);
                        });
                    }

                    if let Some((rk, tx)) = app.async_state.describe.dispatch() {
                        let client = client.clone();
                        tokio::spawn(async move {
                            tracing::debug!("Fetching describe data for {}", rk);
                            let result = crate::kube::fetch::fetch_describe_data(
                                &client,
                                &rk.resource_type,
                                &rk.namespace,
                                &rk.name,
                            )
                            .await;
                            if let Err(ref e) = result {
                                tracing::warn!("Failed to fetch describe data for {}: {}", rk, e);
                            }
                            let _ = tx.send(result);
                        });
                    }

                    if let Some((rk, tx)) = app.async_state.trace.dispatch() {
                        let client = client.clone();
                        tokio::spawn(async move {
                            tracing::debug!("Tracing {}", rk);
                            let result = crate::tui::trace::trace_object(
                                &client,
                                &rk.resource_type,
                                &rk.namespace,
                                &rk.name,
                            )
                            .await;
                            if let Err(ref e) = result {
                                tracing::warn!("Failed to trace {}: {}", rk, e);
                            }
                            let _ = tx.send(result);
                        });
                    }

                    if let Some((rk, tx)) = app.async_state.graph.dispatch() {
                        let client = client.clone();
                        tokio::spawn(async move {
                            tracing::debug!("Building graph for {}", rk);
                            let result = crate::trace::build_resource_graph(
                                &client,
                                &rk.resource_type,
                                &rk.namespace,
                                &rk.name,
                            )
                            .await;
                            if let Err(ref e) = result {
                                tracing::warn!("Failed to build graph for {}: {}", rk, e);
                            }
                            let _ = tx.send(result);
                        });
                    }

                    if let Some((rk, tx)) = app.async_state.workload.dispatch() {
                        let client = client.clone();
                        tokio::spawn(async move {
                            tracing::debug!("Fetching workload data for {}", rk);
                            let result = crate::kube::workloads::fetch_workload_data(
                                &client,
                                &rk.resource_type,
                                &rk.namespace,
                                &rk.name,
                            )
                            .await;
                            if let Err(ref e) = result {
                                tracing::warn!("Failed to fetch workload data for {}: {}", rk, e);
                            }
                            let _ = tx.send(result);
                        });
                    }

                    // Start a queued controller log stream. The task tails the
                    // pod and follows new output until aborted (view closed)
                    // or the stream ends.
                    if let Some((req, tx)) = app.logs.dispatch() {
                        let client = client.clone();
                        let handle = tokio::spawn(async move {
                            use crate::tui::app::logs::LogEvent;
                            use futures::{AsyncBufReadExt, TryStreamExt};
                            use k8s_openapi::api::core::v1::Pod;

                            tracing::debug!("Streaming logs for {}/{}", req.namespace, req.pod);
                            let api: kube::Api<Pod> = kube::Api::namespaced(client, &req.namespace);
                            let params = kube::api::LogParams {
                                follow: true,
                                tail_lines: Some(crate::constants::LOG_TAIL_LINES),
                                ..Default::default()
                            };
                            match api.log_stream(&req.pod, &params).await {
                                Ok(stream) => {
                                    let mut lines = stream.lines();
                                    loop {
                                        match lines.try_next().await {
                                            Ok(Some(line)) => {
                                                if tx.send(LogEvent::Line(line)).is_err() {
                                                    break; // View closed
                                                }
                                            }
                                            Ok(None) => {
                                                let _ = tx.send(LogEvent::Ended);
                                                break;
                                            }
                                            Err(e) => {
                                                let _ = tx.send(LogEvent::Error(e.to_string()));
                                                break;
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        "Failed to start log stream for {}/{}: {}",
                                        req.namespace,
                                        req.pod,
                                        e
                                    );
                                    let _ = tx.send(LogEvent::Error(e.to_string()));
                                }
                            }
                        });
                        app.logs.set_handle(handle);
                    }
                }
            }

            // Drain streamed log lines into the log view's buffer.
            app.logs.drain();

            // Poll fetch results and store them for the views.
            if let Some(result) = app.async_state.yaml.try_recv() {
                match result {
                    Ok(yaml) => {
                        // When editing, route to edit_full_yaml instead of the YAML view
                        if app.view_state.current_view == crate::tui::app::state::View::ResourceEdit
                        {
                            app.async_state.edit_full_yaml = Some(yaml);
                        } else {
                            app.async_state.yaml.set_result(yaml);
                        }
                    }
                    Err(e) => {
                        // If edit was pending, cancel it and return to origin view
                        if app.view_state.current_view == crate::tui::app::state::View::ResourceEdit
                        {
                            app.async_state.edit_pending = None;
                            app.async_state.edit_editor_launched = false;
                            app.view_state.current_view = app.async_state.edit_return_view;
                            app.async_state.edit_return_view =
                                crate::tui::app::state::View::ResourceList;
                        }
                        app.async_state.yaml.set_error();
                        app.set_status_message((format!("Failed to fetch YAML: {}", e), true));
                    }
                }
            }

            if let Some(result) = app.async_state.describe.try_recv() {
                match result {
                    Ok(describe) => app.async_state.describe.set_result(describe),
                    Err(e) => {
                        app.async_state.describe.set_error();
                        app.set_status_message((
                            format!("Failed to fetch description: {}", e),
                            true,
                        ));
                    }
                }
            }

            if let Some(result) = app.async_state.trace.try_recv() {
                match result {
                    Ok(trace_result) => {
                        app.async_state.trace.set_result(trace_result);
                        app.set_view_trace();
                    }
                    Err(e) => {
                        app.async_state.trace.set_error();
                        app.set_status_message((format!("Trace failed: {}", e), true));
                    }
                }
            }

            if let Some(result) = app.async_state.graph.try_recv() {
                match result {
                    // set_graph_result also places keyboard focus on the object node
                    Ok(graph_result) => app.set_graph_result(graph_result),
                    Err(e) => {
                        app.async_state.graph.set_error();
                        app.set_status_message((format!("Graph building failed: {}", e), true));
                        // Return to the previous view instead of an empty graph
                        app.set_view(app.previous_list_view());
                    }
                }
            }

            if let Some(result) = app.async_state.workload.try_recv() {
                match result {
                    // May continue straight into pod logs (l from the list)
                    Ok(workload) => app.on_workload_loaded(workload),
                    Err(e) => {
                        app.async_state.workload.set_error();
                        app.logs_after_workload_load = false;
                        app.set_status_message((format!("Failed to fetch workload: {}", e), true));
                        // Return to the workload list instead of an empty detail
                        app.set_view(crate::tui::app::state::View::WorkloadList);
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

            // If we have a full YAML ready for editing, launch the system editor synchronously.
            // Must run on the main thread so we can properly suspend/resume the TUI terminal.
            if app.view_state.current_view == crate::tui::app::state::View::ResourceEdit
                && app.async_state.edit_full_yaml.is_some()
                && app.async_state.edit_save_pending.is_none()
                && app.async_state.edit_save_result_rx.is_none()
                && !app.async_state.edit_editor_launched
            {
                app.async_state.edit_editor_launched = true;

                if let Some(full_yaml_json) = app.async_state.edit_full_yaml.take() {
                    let yaml_str =
                        serde_yaml::to_string(&full_yaml_json).unwrap_or_else(|_| "{}".to_string());
                    let editor_candidates =
                        crate::editor::editor_candidates(app.config.editor.as_deref());
                    let enable_mouse = app.config.ui.enable_mouse;

                    // Suspend TUI: leave raw mode and alternate screen so the editor
                    // can take over the terminal normally.
                    disable_raw_mode()?;
                    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                    if enable_mouse {
                        execute!(terminal.backend_mut(), DisableMouseCapture)?;
                    }

                    let edit_result: anyhow::Result<Option<String>> = (|| {
                        let mut tmp = tempfile::Builder::new().suffix(".yaml").tempfile()?;
                        use std::io::Write;
                        tmp.write_all(yaml_str.as_bytes())?;
                        let tmp_path = tmp.path().to_path_buf();

                        crate::editor::open_in_editor_with_fallback(&editor_candidates, &tmp_path)?;

                        let edited = std::fs::read_to_string(&tmp_path)?;
                        if edited.trim() == yaml_str.trim() {
                            Ok(None)
                        } else {
                            Ok(Some(edited))
                        }
                    })();

                    // Re-enter TUI: restore raw mode and alternate screen.
                    enable_raw_mode()?;
                    execute!(io::stdout(), EnterAlternateScreen)?;
                    if enable_mouse {
                        execute!(io::stdout(), EnableMouseCapture)?;
                    }
                    terminal.clear()?;

                    match edit_result {
                        Ok(Some(edited_yaml)) => {
                            app.async_state.edit_save_pending = Some(edited_yaml);
                        }
                        Ok(None) => {
                            app.set_status_message((
                                "Edit cancelled (no changes)".to_string(),
                                false,
                            ));
                            app.async_state.edit_pending = None;
                            app.async_state.edit_editor_launched = false;
                            app.view_state.current_view = app.async_state.edit_return_view;
                            app.async_state.edit_return_view =
                                crate::tui::app::state::View::ResourceList;
                        }
                        Err(e) => {
                            app.set_status_message((format!("Editor error: {}", e), true));
                            app.async_state.edit_pending = None;
                            app.async_state.edit_editor_launched = false;
                            app.view_state.current_view = app.async_state.edit_return_view;
                            app.async_state.edit_return_view =
                                crate::tui::app::state::View::ResourceList;
                        }
                    }
                }
            }

            // Trigger SSA apply if edited YAML is pending
            if kube_initialized {
                if let Some(req) = app.trigger_edit_save() {
                    tokio::spawn(async move {
                        let result = crate::operations::apply_resource_yaml(
                            &req.client,
                            &req.resource_key.resource_type,
                            &req.resource_key.namespace,
                            &req.resource_key.name,
                            &req.yaml_to_apply,
                        )
                        .await;
                        let _ = req.tx.send(result);
                    });
                }
            }

            // Check for SSA apply results
            if let Some(result) = app.try_get_edit_save_result() {
                app.set_edit_save_result(result);
            }

            // Check if favorites need to be saved
            if let Some(updated_config) = app.trigger_favorites_save() {
                let config_path = crate::config::paths::root_config_path();
                let config_clone = updated_config.clone();
                tokio::spawn(async move {
                    if let Err(e) =
                        crate::config::loader::ConfigLoader::save(&config_clone, &config_path)
                    {
                        tracing::warn!("Failed to save favorites to config: {}", e);
                    } else {
                        tracing::debug!("Favorites saved to config");
                    }
                });
            }

            // Handle context switch if pending
            if let Some(new_context) = app.take_pending_context_switch() {
                kube_init_done = true;
                tracing::info!("Switching to context: {}", new_context);

                match crate::kube::create_client_for_context(&new_context).await {
                    Ok(new_client) => {
                        // Probe connection to the new context
                        let server_url = crate::kube::health::detect_cluster_server(
                            kubeconfig_path,
                            Some(&new_context),
                        );

                        match crate::kube::health::check_connectivity(&new_client, connect_timeout)
                            .await
                        {
                            Ok(_) => {
                                // Determine the default namespace for the new context
                                let new_default_namespace =
                                    if app.config.default_namespace.is_empty()
                                        || app.config.default_namespace == "all"
                                        || app.config.default_namespace == "-A"
                                    {
                                        crate::kube::get_default_namespace().await
                                    } else {
                                        Some(app.config.default_namespace.clone())
                                    };

                                // Create new watcher with new client and resolved namespace
                                let (mut new_watcher, new_event_rx) =
                                    crate::watcher::ResourceWatcher::new(
                                        new_client.clone(),
                                        new_default_namespace.clone(),
                                        controller_namespace.clone(),
                                        app.config.discover_flux_resources,
                                    );

                                // Start watching all resources with the new watcher
                                if let Err(e) = new_watcher.watch_all() {
                                    tracing::error!(
                                        "Failed to start watchers after context switch: {}",
                                        e
                                    );
                                    app.set_status_message((
                                        format!("Failed to start watchers: {}", e),
                                        true,
                                    ));
                                    let conn_err = ConnectionError::from_anyhow(e)
                                        .with_context(Some(new_context.clone()))
                                        .with_server(server_url.clone());
                                    app.set_connection_error(conn_err);
                                    kube_initialized = false;
                                    app.kube_client = None;
                                    app.watcher = None;
                                    event_rx = None;
                                } else {
                                    // Update app with new context, namespace and watcher
                                    app.complete_context_switch(
                                        new_context.clone(),
                                        new_default_namespace,
                                    );
                                    app.set_kube_client(new_client.clone());
                                    app.set_watcher(new_watcher);

                                    // Replace event receiver
                                    event_rx = Some(new_event_rx);

                                    kube_initialized = true;

                                    // Clear any previous connection error state
                                    app.set_connected();

                                    app.set_status_message((
                                        format!(
                                            "Successfully switched to context: {}",
                                            new_context
                                        ),
                                        false,
                                    ));

                                    // Reload skin for new context
                                    app.reload_skin_for_readonly_mode(Some(&new_context));

                                    tracing::info!("Context switch completed: {}", new_context);
                                }
                            }
                            Err(conn_err) => {
                                let conn_err = conn_err
                                    .with_context(Some(new_context.clone()))
                                    .with_server(server_url.clone());
                                tracing::error!(
                                    "Kubernetes connectivity check failed after context switch: {}",
                                    conn_err
                                );
                                app.set_connection_error(conn_err);
                                kube_initialized = false;
                                app.kube_client = None;
                                app.watcher = None;
                                event_rx = None;
                            }
                        }
                    }
                    Err(e) => {
                        let server_url = crate::kube::health::detect_cluster_server(
                            kubeconfig_path,
                            Some(&new_context),
                        );
                        let conn_err = ConnectionError::from_anyhow(e)
                            .with_context(Some(new_context.clone()))
                            .with_server(server_url);
                        tracing::error!(
                            "Kubernetes context client creation failed after context switch: {}",
                            conn_err
                        );
                        app.set_connection_error(conn_err);
                        kube_initialized = false;
                        app.kube_client = None;
                        app.watcher = None;
                        event_rx = None;
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

                            // Extract reconciliation info
                            let reconciliation_event =
                                crate::watcher::extract_reconciliation_info(&obj_json);

                            // Get existing resource info to check if reconciliation occurred
                            let existing_info = app.state().get(&key);

                            // Check if reconciliation occurred (timestamp changed)
                            let should_add_history = if let (Some(event), Some(existing)) =
                                (&reconciliation_event, &existing_info)
                            {
                                existing.last_reconciled != Some(event.timestamp)
                            } else {
                                reconciliation_event.is_some()
                            };

                            let (suspended, ready, message, revision) =
                                crate::watcher::extract_status_fields(&obj_json);

                            // Stateless resources (e.g., Alert, Provider) have no status.conditions,
                            // so ready is None. Mark them as ready since they are configuration-only.
                            let ready = if ready.is_none() {
                                if let Some(kind) = FluxResourceKind::parse_optional(&resource_type)
                                {
                                    if kind.is_stateless() {
                                        Some(true)
                                    } else {
                                        ready
                                    }
                                } else {
                                    ready
                                }
                            } else {
                                ready
                            };

                            let labels = crate::watcher::extract_labels(&obj_json);
                            let annotations = crate::watcher::extract_annotations(&obj_json);

                            // Build reconciliation history
                            let mut history = if let Some(existing) = existing_info {
                                existing.reconciliation_history.clone()
                            } else {
                                Vec::new()
                            };

                            // Add new event if reconciliation occurred
                            if should_add_history {
                                if let Some(event) = reconciliation_event.clone() {
                                    history.push(event);
                                    // Limit history size
                                    use crate::tui::constants::MAX_RECONCILIATION_HISTORY;
                                    if history.len() > MAX_RECONCILIATION_HISTORY {
                                        history.remove(0); // Remove oldest
                                    }
                                }
                            }

                            app.state().upsert(
                                key.clone(),
                                crate::watcher::ResourceInfo {
                                    name,
                                    namespace: ns,
                                    resource_type,
                                    age: crate::watcher::extract_creation_timestamp(&obj_json),
                                    suspended,
                                    ready,
                                    message,
                                    revision,
                                    labels,
                                    annotations,
                                    last_reconciled: reconciliation_event
                                        .as_ref()
                                        .map(|e| e.timestamp),
                                    reconciliation_history: history,
                                },
                            );
                            // Store full object for detail view
                            app.resource_objects.insert(key.clone(), obj_json);
                        }
                        crate::watcher::WatchEvent::Deleted(resource_type, ns, name) => {
                            let key = crate::watcher::resource_key(&ns, &name, &resource_type);
                            app.state().remove(&key);
                            app.resource_objects.remove(&key);
                        }
                        crate::watcher::WatchEvent::Error(msg) => {
                            // Log errors but don't spam - only show first few
                            // Errors are also shown in the TUI if needed
                            tracing::warn!("Watch event error: {}", msg);
                        }
                        crate::watcher::WatchEvent::WatcherDegraded(name) => {
                            app.watch_degraded(name);
                        }
                        crate::watcher::WatchEvent::WatcherRecovered(name) => {
                            app.watch_recovered(&name);
                        }
                        crate::watcher::WatchEvent::PodApplied(name, pod_json) => {
                            if let Some(info) = extract_controller_pod_info(&pod_json) {
                                app.controller_pods.upsert_pod(name, info);
                            }
                        }
                        crate::watcher::WatchEvent::PodDeleted(name) => {
                            app.controller_pods.remove_pod(&name);
                        }
                        crate::watcher::WatchEvent::DeploymentApplied(deployment_json) => {
                            let version = extract_flux_bundle_version(&deployment_json);
                            app.controller_pods.set_flux_bundle_version(version);
                        }
                        crate::watcher::WatchEvent::KubeEventApplied(event_json) => {
                            if let Some(info) =
                                crate::kube::events::KubeEventInfo::from_json(&event_json)
                            {
                                app.kube_events.upsert(info);
                            }
                        }
                        crate::watcher::WatchEvent::KubeEventDeleted(uid) => {
                            app.kube_events.remove(&uid);
                        }
                        crate::watcher::WatchEvent::ExtraKindDiscovered(extra) => {
                            // Register (idempotent) and (re)start the dynamic
                            // watcher — a no-op when it is already running,
                            // which self-heals after namespace/context restarts.
                            if crate::models::extra_kinds::global().insert(extra.clone()) {
                                tracing::info!(
                                    "Discovered Flux-labeled kind {} ({}/{})",
                                    extra.kind,
                                    extra.group,
                                    extra.version
                                );
                            }
                            if let Some(ref mut w) = app.watcher {
                                w.watch_extra(&extra);
                            }
                        }
                        crate::watcher::WatchEvent::ExtraKindRemoved(kind) => {
                            if crate::models::extra_kinds::global().remove(&kind).is_some() {
                                tracing::info!("Discovered kind {} removed (CRD deleted)", kind);
                                if let Some(ref mut w) = app.watcher {
                                    w.stop_extra(&kind);
                                }
                                app.purge_kind(&kind);
                            }
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
        Ok(())
    }
    .await;

    tracing::debug!("TUI shutting down");

    // Restore terminal (best-effort, also covers the error path above)
    restore_terminal();
    run_result?;

    if app.has_connection_error() {
        if let crate::tui::app::state::ConnectionStatus::Failed(err) =
            &app.ui_state.connection_status
        {
            return Err(anyhow::anyhow!(
                "Failed to connect to Kubernetes: {}",
                err.detail()
            ));
        }
        return Err(anyhow::anyhow!("Failed to connect to Kubernetes"));
    }

    Ok(())
}
