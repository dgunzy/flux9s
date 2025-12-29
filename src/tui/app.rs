//! Application state and main TUI logic

use crate::tui::views::{self, *};
use crate::tui::{OperationRegistry, Theme};
use crate::watcher::{ResourceKey, ResourceState};
use anyhow::Result;
use crossterm::event::KeyEvent;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::Line,
    widgets::{Block, Borders, Paragraph},
};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

/// Request to trace a resource's ownership chain
pub struct TraceRequest {
    /// The type of resource to trace (e.g., "Kustomization", "HelmRelease")
    pub resource_type: String,
    /// The namespace of the resource
    pub namespace: String,
    /// The name of the resource
    pub name: String,
    /// Kubernetes client to use for API calls
    pub client: kube::Client,
    /// Channel to send the trace result back
    pub tx: tokio::sync::oneshot::Sender<anyhow::Result<crate::tui::trace::TraceResult>>,
}

/// Request to execute an operation on a resource
pub struct OperationRequest {
    /// The type of resource to operate on
    pub resource_type: String,
    /// The namespace of the resource
    pub namespace: String,
    /// The name of the resource
    pub name: String,
    /// The operation keybinding character (e.g., 's' for suspend, 'r' for resume)
    pub operation_key: char,
    /// Kubernetes client to use for API calls
    pub client: kube::Client,
    /// Channel to send the operation result back
    pub tx: tokio::sync::oneshot::Sender<anyhow::Result<()>>,
}

/// Pending operation awaiting confirmation
#[derive(Clone, Debug)]
pub struct PendingOperation {
    /// The type of resource to operate on
    pub resource_type: String,
    /// The namespace of the resource
    pub namespace: String,
    /// The name of the resource
    pub name: String,
    /// The operation keybinding character
    pub operation_key: char,
}

impl PendingOperation {
    pub fn new(
        resource_type: String,
        namespace: String,
        name: String,
        operation_key: char,
    ) -> Self {
        Self {
            resource_type,
            namespace,
            name,
            operation_key,
        }
    }
}

/// Main application state
pub struct App {
    state: ResourceState,
    current_view: View,
    selected_resource_type: Option<String>,
    filter: String,
    filter_mode: bool,
    health_filter: HealthFilter,
    selected_index: usize,
    scroll_offset: usize,
    yaml_scroll_offset: usize, // Separate scroll offset for YAML view
    show_help: bool,
    context: String,
    namespace: Option<String>,
    command_mode: bool,
    command_buffer: String,
    selected_resource_key: Option<String>,
    resource_objects: Arc<RwLock<HashMap<String, serde_json::Value>>>,
    watcher: Option<crate::watcher::ResourceWatcher>,
    kube_client: Option<kube::Client>,
    yaml_fetch_pending: Option<String>, // Key of resource being fetched
    yaml_fetched: Option<serde_json::Value>, // Fetched YAML data
    yaml_fetch_rx: Option<tokio::sync::oneshot::Receiver<anyhow::Result<serde_json::Value>>>, // Channel to receive fetch result
    trace_pending: Option<ResourceKey>, // Resource to trace
    trace_result: Option<crate::tui::trace::TraceResult>, // Trace result data
    trace_result_rx:
        Option<tokio::sync::oneshot::Receiver<anyhow::Result<crate::tui::trace::TraceResult>>>, // Channel to receive trace result
    trace_scroll_offset: usize, // Scroll offset for trace view
    show_splash: bool,
    splash_start_time: Option<std::time::Instant>,
    operation_registry: OperationRegistry,
    pending_operation: Option<PendingOperation>, // Operation being executed
    operation_result_rx: Option<tokio::sync::oneshot::Receiver<anyhow::Result<()>>>, // Channel to receive operation result
    last_operation_key: Option<char>, // Store operation key for success message
    confirmation_pending: Option<PendingOperation>, // Operation awaiting user confirmation
    status_message: Option<(String, bool)>, // (message, is_error)
    status_message_time: Option<std::time::Instant>, // When status message was set
    theme: Theme,
    config: crate::config::Config,          // Application configuration
    namespace_hotkeys: Vec<String>, // Namespace hotkeys (0-9), where 0=all, 1=flux-system, etc.
    pending_context_switch: Option<String>, // Context name to switch to (handled in main loop)
    // Cached layout dimensions to prevent bouncing/flickering
    cached_terminal_size: Option<(u16, u16)>, // (width, height)
    cached_header_height: u16,
    cached_footer_height: u16,
    // Favorites management
    favorites: HashSet<String>, // Resource keys: "resource_type:namespace:name"
    favorites_pending_save: bool, // Flag to trigger async save
    history_scroll_offset: usize, // Scroll offset for history view
    graph_scroll_offset: usize, // Scroll offset for graph view (line-based, like YAML)
    graph_pending: Option<ResourceKey>, // Resource to build graph for
    graph_result: Option<crate::trace::ResourceGraph>, // Graph result data
    graph_result_rx:
        Option<tokio::sync::oneshot::Receiver<anyhow::Result<crate::trace::ResourceGraph>>>, // Channel to receive graph result
    // Track previous list view to return to correct view (ResourceList or ResourceFavorites)
    previous_list_view: View, // The list view we came from (ResourceList or ResourceFavorites)
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum View {
    ResourceList,
    ResourceDetail,
    ResourceYAML,
    ResourceTrace,
    ResourceGraph,
    ResourceFavorites,
    ResourceHistory,
    #[allow(dead_code)] // Reserved for future alternative help view implementation
    Help,
}

/// Health filter for resources
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum HealthFilter {
    /// Show only healthy resources (ready=true, not suspended, or null status)
    Healthy,
    /// Show only unhealthy resources (ready=false or suspended=true)
    Unhealthy,
    /// Show all resources (no health filter)
    All,
}

impl App {
    pub fn new(
        state: ResourceState,
        context: String,
        namespace: Option<String>,
        config: crate::config::Config,
        theme: Theme,
    ) -> Self {
        Self {
            state,
            current_view: View::ResourceList,
            selected_resource_type: None,
            filter: String::new(),
            filter_mode: false,
            health_filter: HealthFilter::All,
            selected_index: 0,
            scroll_offset: 0,
            yaml_scroll_offset: 0,
            show_help: false,
            context,
            namespace,
            command_mode: false,
            command_buffer: String::new(),
            selected_resource_key: None,
            resource_objects: Arc::new(RwLock::new(HashMap::new())),
            watcher: None,
            kube_client: None,
            yaml_fetch_pending: None,
            yaml_fetched: None,
            yaml_fetch_rx: None,
            trace_pending: None,
            trace_result: None,
            trace_result_rx: None,
            trace_scroll_offset: 0,
            show_splash: !config.ui.splashless, // Skip splash if splashless is true
            // Don't set splash_start_time here - set it when TUI actually starts rendering
            // This ensures the timer starts at the right time, not during async initialization
            splash_start_time: if config.ui.splashless {
                None
            } else {
                tracing::debug!(
                    "Splash screen will be shown (splashless={})",
                    config.ui.splashless
                );
                None // Will be set when TUI starts rendering
            },
            operation_registry: OperationRegistry::new(),
            pending_operation: None,
            operation_result_rx: None,
            last_operation_key: None,
            confirmation_pending: None,
            status_message: None,
            status_message_time: None,
            theme,
            config: config.clone(),
            namespace_hotkeys: Self::build_namespace_hotkeys(&config, Vec::new()), // Will be updated with discovered namespaces
            pending_context_switch: None, // No pending context switch on init
            // Initialize layout cache - will be populated on first render
            cached_terminal_size: None,
            cached_header_height: crate::tui::constants::MIN_HEADER_HEIGHT,
            cached_footer_height: crate::tui::constants::MIN_FOOTER_HEIGHT,
            // Load favorites from config
            favorites: config.favorites.iter().cloned().collect(),
            favorites_pending_save: false,
            history_scroll_offset: 0,
            graph_scroll_offset: 0,
            graph_pending: None,
            graph_result: None,
            graph_result_rx: None,
            previous_list_view: View::ResourceList, // Track previous list view for navigation
        }
    }

    /// Build namespace hotkeys from config and discovered namespaces
    ///
    /// If config.namespace_hotkeys is non-empty, use it (validated to max 10 items).
    /// Otherwise, build defaults: 0=all, 1=flux-system, 2-9=discovered namespaces.
    fn build_namespace_hotkeys(
        config: &crate::config::Config,
        discovered_namespaces: Vec<String>,
    ) -> Vec<String> {
        // If config has hotkeys, use them (but validate length)
        use crate::tui::constants::MAX_NAMESPACE_HOTKEYS;
        if !config.namespace_hotkeys.is_empty() {
            if config.namespace_hotkeys.len() > MAX_NAMESPACE_HOTKEYS {
                tracing::warn!(
                    "namespace_hotkeys has {} items, maximum is {}. Truncating to first {}.",
                    config.namespace_hotkeys.len(),
                    MAX_NAMESPACE_HOTKEYS,
                    MAX_NAMESPACE_HOTKEYS
                );
                return config.namespace_hotkeys[..MAX_NAMESPACE_HOTKEYS].to_vec();
            }
            return config.namespace_hotkeys.clone();
        }

        // Build defaults: 0=all, 1=flux-system, 2-9=discovered namespaces
        let mut hotkeys = vec!["all".to_string(), "flux-system".to_string()];

        // Add discovered namespaces (skip flux-system if already in list)
        for ns in discovered_namespaces {
            if ns != "flux-system" && hotkeys.len() < MAX_NAMESPACE_HOTKEYS {
                hotkeys.push(ns);
            }
        }

        hotkeys
    }

    /// Update namespace hotkeys with discovered namespaces
    pub fn update_namespace_hotkeys(&mut self, discovered_namespaces: Vec<String>) {
        self.namespace_hotkeys = Self::build_namespace_hotkeys(&self.config, discovered_namespaces);
    }

    /// Get namespace hotkeys
    pub fn namespace_hotkeys(&self) -> &[String] {
        &self.namespace_hotkeys
    }

    /// Invalidate the cached layout dimensions, forcing recalculation on next render.
    /// Call this when filter state or resource counts change (anything that affects header height).
    fn invalidate_layout_cache(&mut self) {
        self.cached_terminal_size = None;
    }

    /// Public method to invalidate layout cache when resource types change.
    /// Should be called from the main event loop when watch events add new resource types.
    pub fn notify_resource_types_changed(&mut self) {
        self.invalidate_layout_cache();
    }

    /// Change theme by name
    pub fn set_theme(&mut self, theme_name: &str) -> Result<()> {
        let theme = crate::config::ThemeLoader::load_theme(theme_name)?;
        self.theme = theme;
        Ok(())
    }

    /// Reload skin based on current readonly mode and config
    /// Uses the same priority logic as startup: env var > context > readonly > default
    pub fn reload_skin_for_readonly_mode(&mut self, context_name: Option<&str>) {
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
            if let Some(context_skin) = self.config.context_skins.get(context) {
                tracing::debug!(
                    "Using context-specific skin for '{}': {}",
                    context,
                    context_skin
                );
                context_skin.clone()
            } else if self.config.read_only && self.config.ui.skin_read_only.is_some() {
                let skin = self.config.ui.skin_read_only.as_ref().unwrap();
                tracing::debug!("Using readonly-specific skin: {}", skin);
                skin.clone()
            } else {
                tracing::debug!("Using default skin: {}", self.config.ui.skin);
                self.config.ui.skin.clone()
            }
        } else if self.config.read_only && self.config.ui.skin_read_only.is_some() {
            let skin = self.config.ui.skin_read_only.as_ref().unwrap();
            tracing::debug!("Using readonly-specific skin: {}", skin);
            skin.clone()
        } else {
            tracing::debug!("Using default skin: {}", self.config.ui.skin);
            self.config.ui.skin.clone()
        };

        // Load the skin
        match crate::config::ThemeLoader::load_theme(&skin_name) {
            Ok(theme) => {
                self.theme = theme;
                tracing::debug!(
                    "Skin reloaded: name='{}', readOnly={}, context={:?}",
                    skin_name,
                    self.config.read_only,
                    context_name
                );
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to reload skin '{}' when toggling readonly mode: {}, keeping current theme",
                    skin_name,
                    e
                );
            }
        }
    }

    pub fn set_kube_client(&mut self, client: kube::Client) {
        self.kube_client = Some(client);
    }

    pub fn set_watcher(&mut self, watcher: crate::watcher::ResourceWatcher) {
        self.watcher = Some(watcher);
    }

    pub fn set_context(&mut self, context: String) {
        self.context = context;
    }

    pub fn set_namespace(&mut self, namespace: Option<String>) {
        self.namespace = namespace;
    }

    pub fn namespace(&self) -> &Option<String> {
        &self.namespace
    }

    /// Check if there's a pending context switch and return the context name
    pub fn take_pending_context_switch(&mut self) -> Option<String> {
        self.pending_context_switch.take()
    }

    /// Update the app with a new context after successful switch
    pub fn complete_context_switch(&mut self, context: String) {
        self.context = context;
        // Clear state when switching contexts
        self.state.clear();
        // Clear resource objects
        {
            let mut objects = self.resource_objects.write().unwrap();
            objects.clear();
        }
        // Reset selection
        self.selected_index = 0;
        self.scroll_offset = 0;
        self.selected_resource_type = None;
    }

    pub fn state(&mut self) -> &mut ResourceState {
        &mut self.state
    }

    pub fn resource_objects(&self) -> &Arc<RwLock<HashMap<String, serde_json::Value>>> {
        &self.resource_objects
    }

    pub fn trigger_yaml_fetch(
        &mut self,
    ) -> Option<(
        String,
        kube::Client,
        tokio::sync::oneshot::Sender<anyhow::Result<serde_json::Value>>,
    )> {
        // Return the key, client, and channel if we need to fetch
        if let Some(ref key) = self.yaml_fetch_pending {
            if let Some(ref client) = self.kube_client {
                let (tx, rx) = tokio::sync::oneshot::channel();
                let key_clone = key.clone();
                let client_clone = client.clone();
                self.yaml_fetch_pending = None; // Clear pending flag
                self.yaml_fetch_rx = Some(rx);
                return Some((key_clone, client_clone, tx));
            }
        }
        None
    }

    pub fn set_yaml_fetched(&mut self, yaml: serde_json::Value) {
        self.yaml_fetched = Some(yaml);
    }

    pub fn set_yaml_fetch_error(&mut self) {
        self.yaml_fetched = None;
        self.yaml_fetch_pending = None;
    }

    pub fn trigger_trace(&mut self) -> Option<TraceRequest> {
        if let Some(ref rk) = self.trace_pending {
            if let Some(ref client) = self.kube_client {
                let (tx, rx) = tokio::sync::oneshot::channel();
                let request = TraceRequest {
                    resource_type: rk.resource_type.clone(),
                    namespace: rk.namespace.clone(),
                    name: rk.name.clone(),
                    client: client.clone(),
                    tx,
                };
                self.trace_pending = None;
                self.trace_result_rx = Some(rx);
                return Some(request);
            }
        }
        None
    }

    pub fn set_trace_result(&mut self, result: crate::tui::trace::TraceResult) {
        self.trace_result = Some(result);
    }

    pub fn set_trace_error(&mut self) {
        self.trace_result = None;
        self.trace_pending = None;
    }

    pub fn try_get_trace_result(
        &mut self,
    ) -> Option<anyhow::Result<crate::tui::trace::TraceResult>> {
        if let Some(ref mut rx) = self.trace_result_rx {
            match rx.try_recv() {
                Ok(result) => {
                    self.trace_result_rx = None;
                    return Some(result);
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                    return None;
                }
                Err(_) => {
                    self.trace_result_rx = None;
                    return Some(Err(anyhow::anyhow!("Trace failed")));
                }
            }
        }
        None
    }

    pub fn trigger_graph(
        &mut self,
    ) -> Option<(
        ResourceKey,
        kube::Client,
        tokio::sync::oneshot::Sender<anyhow::Result<crate::trace::ResourceGraph>>,
    )> {
        if let Some(ref rk) = self.graph_pending {
            if let Some(ref client) = self.kube_client {
                let (tx, rx) = tokio::sync::oneshot::channel();
                let request = (rk.clone(), client.clone(), tx);
                self.graph_pending = None;
                self.graph_result_rx = Some(rx);
                return Some(request);
            }
        }
        None
    }

    pub fn try_get_graph_result(&mut self) -> Option<anyhow::Result<crate::trace::ResourceGraph>> {
        if let Some(ref mut rx) = self.graph_result_rx {
            match rx.try_recv() {
                Ok(result) => {
                    self.graph_result_rx = None;
                    return Some(result);
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                    return None;
                }
                Err(_) => {
                    self.graph_result_rx = None;
                    return Some(Err(anyhow::anyhow!("Graph building failed")));
                }
            }
        }
        None
    }

    pub fn set_graph_result(&mut self, result: crate::trace::ResourceGraph) {
        self.graph_result = Some(result);
    }

    pub fn set_graph_error(&mut self) {
        self.graph_result = None;
        self.graph_pending = None;
    }

    #[allow(dead_code)] // Used in tests
    pub fn set_view_graph(&mut self) {
        self.current_view = View::ResourceGraph;
    }

    pub fn set_view(&mut self, view: View) {
        self.current_view = view;
    }

    pub fn previous_list_view(&self) -> View {
        self.previous_list_view
    }

    #[cfg(test)]
    pub fn set_previous_list_view(&mut self, view: View) {
        self.previous_list_view = view;
    }

    #[allow(dead_code)] // Used in tests
    pub fn current_view(&self) -> View {
        self.current_view
    }

    pub fn try_get_yaml_result(&mut self) -> Option<anyhow::Result<serde_json::Value>> {
        // Check for async YAML fetch result
        if let Some(ref mut rx) = self.yaml_fetch_rx {
            match rx.try_recv() {
                Ok(result) => {
                    self.yaml_fetch_rx = None;
                    return Some(result);
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                    // Still waiting
                    return None;
                }
                Err(_) => {
                    // Channel closed or error
                    self.yaml_fetch_rx = None;
                    return Some(Err(anyhow::anyhow!("YAML fetch failed")));
                }
            }
        }
        None
    }

    /// Initialize the splash screen timer - call this when TUI actually starts rendering
    /// This ensures the timer starts when rendering begins, not during async initialization
    pub fn init_splash_timer(&mut self) {
        tracing::debug!(
            "init_splash_timer: show_splash={}, splash_start_time.is_none()={}",
            self.show_splash,
            self.splash_start_time.is_none()
        );
        if self.show_splash && self.splash_start_time.is_none() {
            let start_time = std::time::Instant::now();
            tracing::debug!(
                "Initializing splash_start_time for first render: {:?}",
                start_time
            );
            self.splash_start_time = Some(start_time);
        } else {
            tracing::warn!(
                "Splash timer NOT initialized: show_splash={}, splash_start_time.is_none()={}",
                self.show_splash,
                self.splash_start_time.is_none()
            );
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<bool> {
        // Return Some(true) to quit, Some(false) to continue, None for no action

        // If splash is showing, dismiss it immediately on any keypress
        if self.show_splash {
            self.show_splash = false;
            self.splash_start_time = None;
            // Don't process the key further - just dismiss splash
            return None;
        }

        // Handle confirmation dialog first
        if self.confirmation_pending.is_some() {
            return self.handle_confirmation_key(key);
        }

        // Handle Esc to dismiss status messages
        if self.status_message.is_some()
            && !self.command_mode
            && !self.filter_mode
            && key.code == crossterm::event::KeyCode::Esc
        {
            self.status_message = None;
            self.status_message_time = None;
            return None;
        }

        // Check status message timeout
        self.check_status_message_timeout();

        // Clear status messages on any key press (except in special modes and operation keys)
        // Don't clear if this is an operation key - we'll set a new message
        let is_operation_key = matches!(
            key.code,
            crossterm::event::KeyCode::Char('s')
                | crossterm::event::KeyCode::Char('r')
                | crossterm::event::KeyCode::Char('d')
                | crossterm::event::KeyCode::Char('R')
                | crossterm::event::KeyCode::Char('W')
        );

        if self.status_message.is_some()
            && !self.command_mode
            && !self.filter_mode
            && !is_operation_key
            && key.code != crossterm::event::KeyCode::Esc
        {
            self.status_message = None;
            self.status_message_time = None;
        }

        if self.command_mode {
            if let Some(should_quit) = self.handle_command_key(key) {
                return Some(should_quit);
            }
            return None;
        }

        if self.filter_mode {
            return self.handle_filter_key(key);
        }

        // Handle namespace hotkeys (0-9)
        if let crossterm::event::KeyCode::Char(c) = key.code {
            if c.is_ascii_digit() {
                let index = c as usize - '0' as usize;
                if index < self.namespace_hotkeys.len() {
                    let ns_name = &self.namespace_hotkeys[index];
                    let new_namespace = if ns_name == "all" {
                        None
                    } else {
                        Some(ns_name.clone())
                    };

                    // Update namespace and restart watchers if changed
                    if self.namespace != new_namespace {
                        self.namespace = new_namespace.clone();

                        // Clear state when switching namespaces
                        self.state.clear();
                        {
                            let mut objects = self.resource_objects.write().unwrap();
                            objects.clear();
                        }

                        // Restart watchers with new namespace
                        if let Some(ref mut watcher) = self.watcher {
                            if let Err(e) = watcher.set_namespace(new_namespace) {
                                self.set_status_message((
                                    format!("Failed to switch namespace: {}", e),
                                    true,
                                ));
                            } else {
                                self.set_status_message((
                                    format!("Switched to namespace: {}", ns_name),
                                    false,
                                ));
                            }
                        }

                        self.selected_index = 0;
                        self.scroll_offset = 0;
                    }
                    return None;
                }
            }
        }

        match key.code {
            crossterm::event::KeyCode::Char('q') => {
                // Always quit on 'q'
                return Some(true);
            }
            crossterm::event::KeyCode::Char('Q') => {
                // Force quit on 'Q' or 'q!'
                return Some(true);
            }
            crossterm::event::KeyCode::Esc => {
                // Escape navigation: go back a level or exit
                if self.show_help {
                    self.show_help = false;
                    return None;
                }
                match self.current_view {
                    View::ResourceList => {
                        // At main menu - exit program
                        return Some(true);
                    }
                    View::ResourceDetail
                    | View::ResourceYAML
                    | View::ResourceTrace
                    | View::ResourceHistory
                    | View::ResourceGraph => {
                        // Go back to previous list view (favorites if we came from there, otherwise list)
                        self.current_view = self.previous_list_view;
                        self.selected_resource_key = None;
                        return None;
                    }
                    View::ResourceFavorites => {
                        // Go back to resource list
                        self.current_view = View::ResourceList;
                        return None;
                    }
                    View::Help => {
                        self.current_view = View::ResourceList;
                        return None;
                    }
                }
            }
            crossterm::event::KeyCode::Char('?') => {
                self.show_help = !self.show_help;
            }
            crossterm::event::KeyCode::Char('s')
            | crossterm::event::KeyCode::Char('r')
            | crossterm::event::KeyCode::Char('d')
            | crossterm::event::KeyCode::Char('R')
            | crossterm::event::KeyCode::Char('W') => {
                // Handle Flux operations - works from list, favorites, and detail view
                let resource_info = if self.current_view == View::ResourceList
                    || self.current_view == View::ResourceFavorites
                {
                    let resources = self.get_filtered_resources();
                    resources.get(self.selected_index).cloned()
                } else if self.current_view == View::ResourceDetail {
                    // Get resource from selected_resource_key
                    if let Some(ref key) = self.selected_resource_key {
                        self.state.get(key)
                    } else {
                        None
                    }
                } else {
                    None
                };

                if let Some(resource) = resource_info {
                    let op_key = match key.code {
                        crossterm::event::KeyCode::Char('s') => 's',
                        crossterm::event::KeyCode::Char('r') => 'r',
                        crossterm::event::KeyCode::Char('d') => 'd',
                        crossterm::event::KeyCode::Char('R') => 'R',
                        crossterm::event::KeyCode::Char('W') => 'W',
                        _ => return None,
                    };

                    if let Some(operation) = self.operation_registry.get_by_keybinding(op_key) {
                        if operation.is_valid_for(&resource.resource_type) {
                            // Check readonly mode first
                            if self.config.read_only {
                                self.set_status_message((
                                    "Readonly mode is enabled. Use :readonly to toggle write actions."
                                        .to_string(),
                                    true,
                                ));
                            } else if operation.requires_confirmation() {
                                // Show confirmation dialog
                                self.confirmation_pending = Some(PendingOperation::new(
                                    resource.resource_type.clone(),
                                    resource.namespace.clone(),
                                    resource.name.clone(),
                                    op_key,
                                ));
                            } else {
                                // Show immediate feedback
                                if let Some(operation) =
                                    self.operation_registry.get_by_keybinding(op_key)
                                {
                                    let feedback_msg = if op_key == 'W' {
                                        // Special message for reconcile with source
                                        format!(
                                            "Reconciling {}/{} with source...",
                                            resource.resource_type, resource.name
                                        )
                                    } else {
                                        format!(
                                            "{} {}/{}...",
                                            operation.name(),
                                            resource.resource_type,
                                            resource.name
                                        )
                                    };
                                    self.set_status_message((feedback_msg, false));
                                }
                                // Execute immediately
                                self.execute_operation(
                                    &resource.resource_type,
                                    &resource.namespace,
                                    &resource.name,
                                    op_key,
                                );
                            }
                        } else {
                            // Operation not valid for this resource type
                            self.set_status_message((
                                format!(
                                    "Operation '{}' is not valid for {}",
                                    operation.name(),
                                    resource.resource_type
                                ),
                                true,
                            ));
                        }
                    }
                }
            }
            crossterm::event::KeyCode::Char('t') => {
                // Trace command - works from list, favorites, and detail view
                let resource_info = if self.current_view == View::ResourceList
                    || self.current_view == View::ResourceFavorites
                {
                    let resources = self.get_filtered_resources();
                    resources.get(self.selected_index).cloned()
                } else if self.current_view == View::ResourceDetail {
                    // Get resource from selected_resource_key
                    if let Some(ref key) = self.selected_resource_key {
                        self.state.get(key)
                    } else {
                        None
                    }
                } else {
                    None
                };

                if let Some(resource) = resource_info {
                    self.trace_pending = Some(ResourceKey::new(
                        resource.resource_type.clone(),
                        resource.namespace.clone(),
                        resource.name.clone(),
                    ));
                    self.trace_result = None;
                    self.trace_scroll_offset = 0;
                }
            }
            crossterm::event::KeyCode::Char(':') => {
                self.command_mode = true;
                self.command_buffer.clear();
            }
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k') => {
                if self.current_view == View::ResourceYAML {
                    // Scroll up in YAML view
                    if self.yaml_scroll_offset > 0 {
                        self.yaml_scroll_offset -= 1;
                    }
                } else if self.current_view == View::ResourceTrace {
                    // Scroll up in trace view
                    self.trace_scroll_offset = self.trace_scroll_offset.saturating_sub(1);
                } else if self.current_view == View::ResourceHistory {
                    // Scroll up in history view
                    self.history_scroll_offset = self.history_scroll_offset.saturating_sub(1);
                } else if self.current_view == View::ResourceGraph {
                    // Scroll up in graph view (line-based, like YAML)
                    self.graph_scroll_offset = self.graph_scroll_offset.saturating_sub(1);
                } else {
                    // Normal navigation
                    if self.selected_index > 0 {
                        self.selected_index -= 1;
                        if self.selected_index < self.scroll_offset {
                            self.scroll_offset = self.selected_index;
                        }
                    }
                }
            }
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j') => {
                if self.current_view == View::ResourceYAML {
                    // Scroll down in YAML view - we'll handle max scroll in render
                    self.yaml_scroll_offset += 1;
                } else if self.current_view == View::ResourceTrace {
                    // Scroll down in trace view
                    self.trace_scroll_offset += 1;
                } else if self.current_view == View::ResourceHistory {
                    // Scroll down in history view
                    self.history_scroll_offset += 1;
                } else if self.current_view == View::ResourceGraph {
                    // Scroll down in graph view (line-based, like YAML)
                    self.graph_scroll_offset += 1;
                } else {
                    // Normal navigation
                    let resources = self.get_filtered_resources();
                    if self.selected_index < resources.len().saturating_sub(1) {
                        self.selected_index += 1;
                    }
                }
            }
            crossterm::event::KeyCode::Char('/') => {
                // Enter filter mode
                self.filter_mode = true;
                self.filter.clear();
                self.invalidate_layout_cache(); // Filter state affects header height
            }
            crossterm::event::KeyCode::Char('y') => {
                // View YAML - trigger async fetch
                if self.current_view == View::ResourceList
                    || self.current_view == View::ResourceFavorites
                {
                    // Save current view as previous list view before navigating
                    self.previous_list_view = self.current_view;
                    let resources = self.get_filtered_resources();
                    if let Some(resource) = resources.get(self.selected_index) {
                        let key = crate::watcher::resource_key(
                            &resource.namespace,
                            &resource.name,
                            &resource.resource_type,
                        );
                        self.selected_resource_key = Some(key.clone());
                        self.yaml_fetch_pending = Some(key);
                        self.yaml_fetched = None; // Clear previous fetch
                        self.yaml_scroll_offset = 0; // Reset scroll when entering YAML view
                        self.current_view = View::ResourceYAML;
                    }
                } else if self.current_view == View::ResourceDetail {
                    // From detail view, preserve the previous_list_view (don't overwrite it)
                    if let Some(ref key) = self.selected_resource_key {
                        self.yaml_fetch_pending = Some(key.clone());
                        self.yaml_fetched = None;
                        self.yaml_scroll_offset = 0; // Reset scroll when entering YAML view
                    }
                    self.current_view = View::ResourceYAML;
                }
            }
            crossterm::event::KeyCode::Enter => {
                if self.current_view == View::ResourceList
                    || self.current_view == View::ResourceFavorites
                {
                    // Save current view as previous list view before navigating
                    self.previous_list_view = self.current_view;
                    // Get selected resource
                    let resources = self.get_filtered_resources();
                    if let Some(resource) = resources.get(self.selected_index) {
                        let key = crate::watcher::resource_key(
                            &resource.namespace,
                            &resource.name,
                            &resource.resource_type,
                        );
                        self.selected_resource_key = Some(key);
                        self.current_view = View::ResourceDetail;
                    }
                }
            }
            crossterm::event::KeyCode::Char('f') => {
                // Toggle favorite - works from list view
                if self.current_view == View::ResourceList
                    || self.current_view == View::ResourceFavorites
                {
                    let resources = self.get_filtered_resources();
                    if let Some(resource) = resources.get(self.selected_index) {
                        let key = crate::watcher::resource_key(
                            &resource.namespace,
                            &resource.name,
                            &resource.resource_type,
                        );
                        self.toggle_favorite(&key);
                        self.set_status_message((
                            if self.is_favorite(&key) {
                                format!("Added {} to favorites", resource.name)
                            } else {
                                format!("Removed {} from favorites", resource.name)
                            },
                            false,
                        ));
                    }
                }
            }
            crossterm::event::KeyCode::Char('h') => {
                // View reconciliation history - works from list, favorites, and detail view
                let resource_info = if self.current_view == View::ResourceList
                    || self.current_view == View::ResourceFavorites
                {
                    let resources = self.get_filtered_resources();
                    resources.get(self.selected_index).cloned()
                } else if self.current_view == View::ResourceDetail {
                    // Get resource from selected_resource_key
                    if let Some(ref key) = self.selected_resource_key {
                        self.state.get(key)
                    } else {
                        None
                    }
                } else {
                    None
                };

                if let Some(resource) = resource_info {
                    let key = crate::watcher::resource_key(
                        &resource.namespace,
                        &resource.name,
                        &resource.resource_type,
                    );

                    // Check if resource object exists and has status.history
                    let objects = self.resource_objects.read().unwrap();
                    let has_history = objects
                        .get(&key)
                        .and_then(|obj| obj.get("status"))
                        .and_then(|s| s.get("history"))
                        .and_then(|h| h.as_array())
                        .map(|arr| !arr.is_empty())
                        .unwrap_or(false);

                    drop(objects); // Release lock before switching view

                    if has_history {
                        // Save current view as previous list view before navigating
                        self.previous_list_view = self.current_view;
                        self.selected_resource_key = Some(key);
                        self.current_view = View::ResourceHistory;
                        self.history_scroll_offset = 0;
                    } else {
                        // Show error message immediately
                        use crate::models::FluxResourceKind;
                        let supported_types: Vec<String> =
                            FluxResourceKind::history_supported_types()
                                .iter()
                                .map(|k| k.as_str().to_string())
                                .collect();
                        self.set_status_message((
                            format!(
                                "Resource '{}' does not have reconciliation history. History is only available for: {}",
                                resource.name,
                                supported_types.join(", ")
                            ),
                            true,
                        ));
                    }
                } else {
                    self.set_status_message(("No resource selected".to_string(), true));
                }
            }
            crossterm::event::KeyCode::Char('g') => {
                // View resource graph - works from list, favorites, and detail view
                let resource_info = if self.current_view == View::ResourceList
                    || self.current_view == View::ResourceFavorites
                {
                    let resources = self.get_filtered_resources();
                    resources.get(self.selected_index).cloned()
                } else if self.current_view == View::ResourceDetail {
                    // Get resource from selected_resource_key
                    if let Some(ref key) = self.selected_resource_key {
                        self.state.get(key)
                    } else {
                        None
                    }
                } else {
                    None
                };

                if let Some(resource) = resource_info {
                    // Check if resource type supports graph view
                    if !crate::trace::is_resource_type_with_graph(&resource.resource_type) {
                        self.set_status_message((
                            format!(
                                "Graph view not supported for {} resources",
                                resource.resource_type
                            ),
                            true,
                        ));
                        return None;
                    }

                    // Save current view as previous list view before navigating
                    if self.current_view == View::ResourceList
                        || self.current_view == View::ResourceFavorites
                    {
                        self.previous_list_view = self.current_view;
                    }

                    // Trigger graph building
                    let key = crate::watcher::resource_key(
                        &resource.namespace,
                        &resource.name,
                        &resource.resource_type,
                    );

                    self.selected_resource_key = Some(key.clone());
                    self.graph_pending = Some(ResourceKey {
                        resource_type: resource.resource_type.clone(),
                        namespace: resource.namespace.clone(),
                        name: resource.name.clone(),
                    });
                    self.graph_result = None; // Clear previous graph
                    self.graph_scroll_offset = 0; // Reset scroll
                    self.current_view = View::ResourceGraph;
                } else {
                    self.set_status_message(("No resource selected".to_string(), true));
                }
            }
            crossterm::event::KeyCode::Backspace => {
                // Backspace goes back (same as Escape for detail view)
                if self.current_view == View::ResourceDetail
                    || self.current_view == View::ResourceYAML
                    || self.current_view == View::ResourceTrace
                    || self.current_view == View::ResourceHistory
                    || self.current_view == View::ResourceGraph
                {
                    // Return to previous list view (favorites if we came from there, otherwise list)
                    self.current_view = self.previous_list_view;
                    self.selected_resource_key = None;
                } else if self.current_view == View::ResourceFavorites {
                    self.current_view = View::ResourceList;
                    self.selected_resource_key = None;
                }
            }
            _ => {}
        }
        None
    }

    fn handle_filter_key(&mut self, key: KeyEvent) -> Option<bool> {
        match key.code {
            crossterm::event::KeyCode::Esc => {
                // Exit filter mode
                self.filter_mode = false;
                let was_filtering = !self.filter.is_empty();
                self.filter.clear();
                if was_filtering {
                    self.invalidate_layout_cache(); // Filter state affects header height
                }
                None
            }
            crossterm::event::KeyCode::Enter => {
                // Apply filter and exit filter mode
                self.filter_mode = false;
                self.selected_index = 0;
                self.scroll_offset = 0;
                // Only invalidate if filter was applied (non-empty) - this is when header changes
                if !self.filter.is_empty() {
                    self.invalidate_layout_cache();
                }
                None
            }
            crossterm::event::KeyCode::Backspace => {
                let was_empty = self.filter.is_empty();
                self.filter.pop();
                // Invalidate when transitioning from non-empty to empty (header line change)
                if !was_empty && self.filter.is_empty() {
                    self.invalidate_layout_cache();
                }
                None
            }
            crossterm::event::KeyCode::Char(c) => {
                let was_empty = self.filter.is_empty();
                self.filter.push(c);
                self.selected_index = 0;
                self.scroll_offset = 0;
                // Invalidate when transitioning from empty to non-empty (header line change)
                if was_empty {
                    self.invalidate_layout_cache();
                }
                None
            }
            _ => None,
        }
    }

    fn handle_confirmation_key(&mut self, key: KeyEvent) -> Option<bool> {
        if let Some(ref pending) = self.confirmation_pending {
            match key.code {
                crossterm::event::KeyCode::Char('y') | crossterm::event::KeyCode::Char('Y') => {
                    // Check readonly mode before confirming
                    if self.config.read_only {
                        self.confirmation_pending = None;
                        self.set_status_message((
                            "Readonly mode is enabled. Use :readonly to toggle write actions."
                                .to_string(),
                            true,
                        ));
                        return None;
                    }
                    // Confirm operation - clone data before clearing pending state
                    let pending_clone = pending.clone();
                    self.confirmation_pending = None;
                    self.execute_operation(
                        &pending_clone.resource_type,
                        &pending_clone.namespace,
                        &pending_clone.name,
                        pending_clone.operation_key,
                    );
                }
                crossterm::event::KeyCode::Char('n')
                | crossterm::event::KeyCode::Char('N')
                | crossterm::event::KeyCode::Esc => {
                    // Cancel operation
                    self.confirmation_pending = None;
                }
                _ => {}
            }
        }
        None
    }

    fn execute_operation(
        &mut self,
        resource_type: &str,
        namespace: &str,
        name: &str,
        op_key: char,
    ) {
        // Check readonly mode - prevent modification operations
        if self.config.read_only && self.operation_registry.get_by_keybinding(op_key).is_some() {
            // All operations are modifications, so block them all in readonly mode
            self.set_status_message((
                "Readonly mode is enabled. Use :readonly to toggle write actions.".to_string(),
                true,
            ));
            return;
        }

        if self.operation_registry.get_by_keybinding(op_key).is_some() && self.kube_client.is_some()
        {
            // Mark operation as pending - will be executed in main loop
            self.pending_operation = Some(PendingOperation::new(
                resource_type.to_string(),
                namespace.to_string(),
                name.to_string(),
                op_key,
            ));
        }
    }

    pub fn trigger_operation_execution(&mut self) -> Option<OperationRequest> {
        if let Some(ref pending) = self.pending_operation {
            if let Some(ref client) = self.kube_client {
                if self
                    .operation_registry
                    .get_by_keybinding(pending.operation_key)
                    .is_some()
                {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    let request = OperationRequest {
                        resource_type: pending.resource_type.clone(),
                        namespace: pending.namespace.clone(),
                        name: pending.name.clone(),
                        operation_key: pending.operation_key,
                        client: client.clone(),
                        tx,
                    };

                    self.last_operation_key = Some(pending.operation_key); // Store operation key for success message
                    self.pending_operation = None;
                    self.operation_result_rx = Some(rx);

                    return Some(request);
                }
            }
        }
        None
    }

    pub fn try_get_operation_result(&mut self) -> Option<anyhow::Result<()>> {
        if let Some(ref mut rx) = self.operation_result_rx {
            match rx.try_recv() {
                Ok(result) => {
                    self.operation_result_rx = None;
                    return Some(result);
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                    return None;
                }
                Err(_) => {
                    self.operation_result_rx = None;
                    return Some(Err(anyhow::anyhow!("Operation failed")));
                }
            }
        }
        None
    }

    pub fn set_status_message(&mut self, message: (String, bool)) {
        self.status_message = Some(message);
        self.status_message_time = Some(std::time::Instant::now());
    }

    /// Check and clear status message if timeout exceeded
    pub fn check_status_message_timeout(&mut self) {
        use crate::tui::constants::STATUS_MESSAGE_TIMEOUT_SECS;
        if let (Some(_), Some(time)) = (&self.status_message, &self.status_message_time) {
            if time.elapsed().as_secs() >= STATUS_MESSAGE_TIMEOUT_SECS {
                self.status_message = None;
                self.status_message_time = None;
            }
        }
    }

    pub fn set_view_trace(&mut self) {
        self.current_view = View::ResourceTrace;
        self.trace_scroll_offset = 0;
    }

    /// Toggle favorite status for a resource
    pub fn toggle_favorite(&mut self, resource_key: &str) {
        if self.favorites.contains(resource_key) {
            self.favorites.remove(resource_key);
        } else {
            self.favorites.insert(resource_key.to_string());
        }
        self.favorites_pending_save = true;
    }

    /// Check if a resource is favorited
    pub fn is_favorite(&self, resource_key: &str) -> bool {
        self.favorites.contains(resource_key)
    }

    /// Get all favorite resource keys
    #[allow(dead_code)] // Public API method
    pub fn favorites(&self) -> &HashSet<String> {
        &self.favorites
    }

    /// Trigger async save of favorites to config file
    pub fn trigger_favorites_save(&mut self) -> Option<crate::config::Config> {
        if self.favorites_pending_save {
            self.favorites_pending_save = false;
            // Create updated config with favorites
            let mut updated_config = self.config.clone();
            updated_config.favorites = self.favorites.iter().cloned().collect();
            Some(updated_config)
        } else {
            None
        }
    }

    pub fn set_operation_result(&mut self, result: anyhow::Result<()>) {
        match result {
            Ok(_) => {
                if let Some(op_key) = self.last_operation_key.take() {
                    if let Some(operation) = self.operation_registry.get_by_keybinding(op_key) {
                        self.set_status_message((
                            format!("{} completed successfully", operation.name()),
                            false,
                        ));
                    } else {
                        self.set_status_message((
                            "Operation completed successfully".to_string(),
                            false,
                        ));
                    }
                } else {
                    self.set_status_message((
                        "Operation completed successfully".to_string(),
                        false,
                    ));
                }
            }
            Err(e) => {
                self.last_operation_key = None;
                self.set_status_message((format!("Operation failed: {}", e), true));
            }
        }
    }

    fn handle_command_key(&mut self, key: KeyEvent) -> Option<bool> {
        match key.code {
            crossterm::event::KeyCode::Esc => {
                self.command_mode = false;
                self.command_buffer.clear();
                None
            }
            crossterm::event::KeyCode::Tab => {
                // Autocomplete command
                self.autocomplete_command();
                None
            }
            crossterm::event::KeyCode::Enter => {
                if let Some(should_quit) = self.execute_command() {
                    self.command_mode = false;
                    self.command_buffer.clear();
                    return Some(should_quit);
                }
                self.command_mode = false;
                self.command_buffer.clear();
                None
            }
            crossterm::event::KeyCode::Backspace => {
                self.command_buffer.pop();
                None
            }
            crossterm::event::KeyCode::Char(c) => {
                self.command_buffer.push(c);
                None
            }
            _ => None,
        }
    }

    fn autocomplete_command(&mut self) {
        let cmd = self.command_buffer.trim();

        // Command buffer doesn't include the ':' prefix (it's shown in UI)
        // So we match against the buffer directly
        let cmd_lower = cmd.to_lowercase();

        // Don't autocomplete namespace names
        if crate::tui::commands::is_namespace_command(&cmd_lower) && cmd_lower.contains(' ') {
            return;
        }

        // Use centralized command registry to find matches
        // This prioritizes CRD commands over app commands
        let matches = crate::tui::commands::find_matching_commands(&cmd_lower);

        if matches.is_empty() {
            return;
        }

        // Use first match (prioritized: CRD commands first, then app commands)
        // Replace buffer with matched command (no colon, it's shown in UI)
        // Commands with args already include trailing space
        if let Some(first_match) = matches.first() {
            self.command_buffer = first_match.clone();
        }
    }

    fn execute_command(&mut self) -> Option<bool> {
        let cmd = self.command_buffer.trim();
        let cmd_lower = cmd.to_lowercase();

        // Handle help command
        if crate::tui::commands::is_help_command(&cmd_lower) {
            self.show_help = !self.show_help;
            return None;
        }

        // Handle quit commands
        if crate::tui::commands::is_quit_command(&cmd_lower) {
            return Some(true); // Quit
        }

        // Handle readonly toggle command
        if crate::tui::commands::is_readonly_command(&cmd_lower) {
            self.config.read_only = !self.config.read_only;
            let status = if self.config.read_only {
                "enabled"
            } else {
                "disabled"
            };

            // Reload skin based on readonly mode
            // Clone context name to avoid borrow checker issues
            let context_name = self.context.clone();
            self.reload_skin_for_readonly_mode(Some(&context_name));

            self.set_status_message((format!("Readonly mode {}", status), false));
            return None;
        }

        // Handle skin/theme change command
        if crate::tui::commands::is_skin_command(&cmd_lower) {
            let theme_name = crate::tui::commands::extract_command_arg(cmd, "skin");
            if let Some(name) = theme_name {
                match self.set_theme(&name) {
                    Ok(_) => {
                        let msg = format!("Theme changed to: {}", name);
                        self.set_status_message((msg, false));
                    }
                    Err(e) => {
                        let msg = format!(
                            "Failed to load theme '{}': {}. Use `default` to return to default theme",
                            name, e
                        );
                        self.set_status_message((msg, true));
                    }
                }
            } else {
                self.set_status_message(("Usage: :skin <theme-name>".to_string(), true));
            }
            return None;
        }

        // Handle trace command - trace ownership chain
        if crate::tui::commands::is_trace_command(&cmd_lower) {
            let trace_arg = crate::tui::commands::extract_command_arg(cmd, "trace");
            if trace_arg.is_none() {
                // If no args, trace currently selected resource
                if let Some(ref key) = self.selected_resource_key {
                    if let Some(rk) = ResourceKey::parse(key) {
                        self.trace_pending = Some(rk);
                        self.trace_result = None;
                    } else {
                        tracing::warn!("Failed to parse resource key for trace command: {}", key);
                        self.status_message =
                            Some(("Invalid resource key format".to_string(), true));
                    }
                } else {
                    self.set_status_message(("No resource selected".to_string(), true));
                }
            } else if let Some(trace_arg) = trace_arg {
                // Parse resource type/name format (e.g., "kustomization/cabot-book" or "Kustomization/cabot-book")
                let resource_parts: Vec<&str> = trace_arg.split('/').collect();
                if resource_parts.len() == 2 {
                    let resource_type = resource_parts[0];
                    let name = resource_parts[1];
                    use crate::models::FluxResourceKind;
                    // Normalize resource type to proper case
                    let resource_type_normalized =
                        match FluxResourceKind::from_str_case_insensitive(resource_type) {
                            Some(kind) => kind.as_str(),
                            None => {
                                // Handle standard Kubernetes resources
                                match resource_type.to_lowercase().as_str() {
                                    "deployment" | "deploy" => "Deployment",
                                    "service" => "Service",
                                    "pod" => "Pod",
                                    _ => resource_type,
                                }
                            }
                        };
                    let namespace = self
                        .namespace
                        .clone()
                        .unwrap_or_else(|| "default".to_string());
                    self.trace_pending = Some(ResourceKey::new(
                        resource_type_normalized.to_string(),
                        namespace,
                        name.to_string(),
                    ));
                    self.trace_result = None;
                } else {
                    self.set_status_message((
                        "Usage: :trace <resource-type>/<name> or :trace (for selected)".to_string(),
                        true,
                    ));
                }
            }
            return None;
        }

        // Handle context switching - reconnect to different cluster
        if crate::tui::commands::is_context_command(&cmd_lower) {
            // Try "context" first, then "ctx" as fallback
            let context_name = crate::tui::commands::extract_command_arg(cmd, "context")
                .or_else(|| crate::tui::commands::extract_command_arg(cmd, "ctx"));

            match context_name {
                Some(ctx) => {
                    // Mark context switch as pending - will be handled in main loop
                    self.pending_context_switch = Some(ctx.to_string());
                    self.set_status_message((format!("Switching to context '{}'...", ctx), false));
                }
                None => {
                    // List available contexts
                    match crate::kube::list_contexts() {
                        Ok(contexts) => {
                            let current = self.context.clone();
                            let msg = format!(
                                "Available contexts: {}. Current: {}. Usage: :ctx <context-name>",
                                contexts.join(", "),
                                current
                            );
                            self.set_status_message((msg, false));
                        }
                        Err(e) => {
                            self.set_status_message((
                                format!("Failed to list contexts: {}", e),
                                true,
                            ));
                        }
                    }
                }
            }
            return None;
        }

        // Handle namespace switching - restart watchers with new namespace
        if crate::tui::commands::is_namespace_command(&cmd_lower) {
            // Try "namespace" first, then "ns" as fallback
            let ns = crate::tui::commands::extract_command_arg(cmd, "namespace")
                .or_else(|| crate::tui::commands::extract_command_arg(cmd, "ns"));
            let new_namespace = match ns.as_deref() {
                Some("all") | Some("-A") => None,
                Some(ns_name) => Some(ns_name.to_string()),
                None => {
                    // Show current namespace - do nothing
                    return None;
                }
            };

            // Update namespace and restart watchers if changed
            if self.namespace != new_namespace {
                self.namespace = new_namespace.clone();

                // Clear state when switching namespaces (will repopulate from new watchers)
                // This ensures we don't show resources from the old namespace
                self.state.clear();

                // Clear resource objects too
                {
                    let mut objects = self.resource_objects.write().unwrap();
                    objects.clear();
                }

                // Restart watchers with new namespace (more efficient than watching all)
                if let Some(ref mut watcher) = self.watcher {
                    if let Err(e) = watcher.set_namespace(new_namespace) {
                        // Log error but continue - watcher will retry
                        eprintln!("Failed to switch namespace: {}", e);
                    }
                }
            }

            self.selected_index = 0;
            self.scroll_offset = 0;
            return None;
        }

        // Handle health filter commands
        if crate::tui::commands::is_healthy_command(&cmd_lower) {
            self.health_filter = HealthFilter::Healthy;
            self.selected_index = 0;
            self.scroll_offset = 0;
            self.set_status_message(("Showing healthy resources only".to_string(), false));
            return None;
        }

        if crate::tui::commands::is_unhealthy_command(&cmd_lower) {
            self.health_filter = HealthFilter::Unhealthy;
            self.selected_index = 0;
            self.scroll_offset = 0;
            self.set_status_message(("Showing unhealthy resources only".to_string(), false));
            return None;
        }

        // Handle favorites command
        if crate::tui::commands::is_favorites_command(&cmd_lower) {
            self.current_view = View::ResourceFavorites;
            self.selected_index = 0;
            self.scroll_offset = 0;
            return None;
        }

        // Use registry for resource type command mapping
        if crate::tui::commands::is_all_command(&cmd_lower) {
            // Clear favorites view if active
            if self.current_view == View::ResourceFavorites {
                self.current_view = View::ResourceList;
            }
            if self.selected_resource_type.is_some() {
                self.selected_resource_type = None;
                self.invalidate_layout_cache(); // Resource type filter affects header display
            }
            // Clear health filter when showing all
            if self.health_filter != HealthFilter::All {
                self.health_filter = HealthFilter::All;
                self.set_status_message(("Showing all resources".to_string(), false));
            }
            self.selected_index = 0;
            self.scroll_offset = 0;
            return None;
        }

        if let Some(display_name) = crate::watcher::get_display_name_for_command(&cmd_lower) {
            self.selected_resource_type = Some(display_name.to_string());
            self.selected_index = 0;
            self.scroll_offset = 0;
            self.invalidate_layout_cache(); // Resource type filter affects header display
        }

        None
    }

    fn get_filtered_resources(&self) -> Vec<crate::watcher::ResourceInfo> {
        let mut resources = if let Some(ref resource_type) = self.selected_resource_type {
            self.state.by_type(resource_type)
        } else {
            self.state.all()
        };

        // Apply namespace filter if set (safety check - watcher should already filter, but ensure consistency)
        if let Some(ref namespace) = self.namespace {
            resources.retain(|r| r.namespace == *namespace);
        }

        // Apply favorites filter if in favorites view
        if self.current_view == View::ResourceFavorites {
            resources.retain(|r| {
                let key = crate::watcher::resource_key(&r.namespace, &r.name, &r.resource_type);
                self.favorites.contains(&key)
            });
        }

        // Apply filter if set
        // Supports special syntax:
        //   /label:          - resources with any label
        //   /label:key       - resources with label key (any value)
        //   /label:key=value - resources with label key=value
        //   /ann:            - resources with any annotation
        //   /ann:key         - resources with annotation key (any value)
        //   /ann:key=value   - resources with annotation key=value
        //   /annotations:... - alias for /ann:...
        //   /text            - matches name
        if !self.filter.is_empty() {
            if let Some(label_filter) = self.filter.strip_prefix("label:") {
                // Label filter
                if label_filter.is_empty() {
                    // Just "label:" - show only resources that have at least one label
                    resources.retain(|r| !r.labels.is_empty());
                } else if let Some((key, value)) = label_filter.split_once('=') {
                    // key=value match - filter by prefix match for progressive filtering
                    resources.retain(|r| {
                        r.labels
                            .iter()
                            .any(|(k, v)| k.starts_with(key) && v.starts_with(value))
                    });
                } else {
                    // Key prefix match (any value) - progressive filtering as user types
                    resources.retain(|r| r.labels.keys().any(|k| k.starts_with(label_filter)));
                }
            } else if let Some(ann_filter) = self
                .filter
                .strip_prefix("ann:")
                .or_else(|| self.filter.strip_prefix("annotations:"))
            {
                // Annotation filter
                if ann_filter.is_empty() {
                    // Just "ann:" or "annotations:" - show only resources with at least one annotation
                    resources.retain(|r| !r.annotations.is_empty());
                } else if let Some((key, value)) = ann_filter.split_once('=') {
                    // key=value match - filter by prefix match for progressive filtering
                    resources.retain(|r| {
                        r.annotations
                            .iter()
                            .any(|(k, v)| k.starts_with(key) && v.starts_with(value))
                    });
                } else {
                    // Key prefix match (any value) - progressive filtering as user types
                    resources.retain(|r| r.annotations.keys().any(|k| k.starts_with(ann_filter)));
                }
            } else {
                // Standard text filter - matches name
                resources.retain(|r| r.name.contains(&self.filter));
            }
        }

        // Apply health filter
        match self.health_filter {
            HealthFilter::Healthy => {
                resources.retain(|r| {
                    // Healthy: ready=true and not suspended, or null status (treat as healthy)
                    let is_ready = r.ready.unwrap_or(true); // null status treated as healthy
                    let is_suspended = r.suspended.unwrap_or(false);
                    is_ready && !is_suspended
                });
            }
            HealthFilter::Unhealthy => {
                resources.retain(|r| {
                    // Unhealthy: ready=false or suspended=true
                    let is_ready = r.ready.unwrap_or(true);
                    let is_suspended = r.suspended.unwrap_or(false);
                    !is_ready || is_suspended
                });
            }
            HealthFilter::All => {
                // No filtering - show all resources
            }
        }

        // Sort: favorites first, then by namespace, resource type, and name
        resources.sort_by(|a, b| {
            let a_key = crate::watcher::resource_key(&a.namespace, &a.name, &a.resource_type);
            let b_key = crate::watcher::resource_key(&b.namespace, &b.name, &b.resource_type);
            let a_is_favorite = self.favorites.contains(&a_key);
            let b_is_favorite = self.favorites.contains(&b_key);

            // Favorites first
            match (a_is_favorite, b_is_favorite) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => {
                    // Both favorites or both not favorites - sort normally
                    a.namespace
                        .cmp(&b.namespace)
                        .then_with(|| a.resource_type.cmp(&b.resource_type))
                        .then_with(|| a.name.cmp(&b.name))
                }
            }
        });

        resources
    }

    /// Calculate health percentage based on filtered resources
    /// This calculates health for resources matching the current name/resource type filters,
    /// but before applying the health filter itself.
    fn calculate_health_percentage(&self) -> f64 {
        // Get resources filtered by name/resource type (but not health filter)
        let mut filtered_resources = if let Some(ref resource_type) = self.selected_resource_type {
            self.state.by_type(resource_type)
        } else {
            self.state.all()
        };

        // Apply namespace filter if set
        if let Some(ref namespace) = self.namespace {
            filtered_resources.retain(|r| r.namespace == *namespace);
        }

        // Apply name/label/annotation filter if set
        if !self.filter.is_empty() {
            if let Some(label_filter) = self.filter.strip_prefix("label:") {
                if label_filter.is_empty() {
                    filtered_resources.retain(|r| !r.labels.is_empty());
                } else if let Some((key, value)) = label_filter.split_once('=') {
                    filtered_resources.retain(|r| {
                        r.labels
                            .iter()
                            .any(|(k, v)| k.starts_with(key) && v.starts_with(value))
                    });
                } else {
                    filtered_resources
                        .retain(|r| r.labels.keys().any(|k| k.starts_with(label_filter)));
                }
            } else if let Some(ann_filter) = self
                .filter
                .strip_prefix("ann:")
                .or_else(|| self.filter.strip_prefix("annotations:"))
            {
                if ann_filter.is_empty() {
                    filtered_resources.retain(|r| !r.annotations.is_empty());
                } else if let Some((key, value)) = ann_filter.split_once('=') {
                    filtered_resources.retain(|r| {
                        r.annotations
                            .iter()
                            .any(|(k, v)| k.starts_with(key) && v.starts_with(value))
                    });
                } else {
                    filtered_resources
                        .retain(|r| r.annotations.keys().any(|k| k.starts_with(ann_filter)));
                }
            } else {
                filtered_resources.retain(|r| r.name.contains(&self.filter));
            }
        }

        if filtered_resources.is_empty() {
            return 100.0; // No resources = 100% healthy (nothing to be unhealthy)
        }

        let healthy_count = filtered_resources
            .iter()
            .filter(|r| {
                let is_ready = r.ready.unwrap_or(true); // null status treated as healthy
                let is_suspended = r.suspended.unwrap_or(false);
                is_ready && !is_suspended
            })
            .count();

        (healthy_count as f64 / filtered_resources.len() as f64) * 100.0
    }

    pub fn render(&mut self, f: &mut Frame) {
        // Show splash screen for 1.5 seconds, then auto-dismiss
        if self.show_splash {
            if let Some(start_time) = self.splash_start_time {
                let elapsed = start_time.elapsed();
                tracing::debug!(
                    "Splash render check: elapsed={:?}ms, show_splash={}",
                    elapsed.as_millis(),
                    self.show_splash
                );
                use crate::tui::constants::SPLASH_DISPLAY_MS;
                if elapsed >= std::time::Duration::from_millis(SPLASH_DISPLAY_MS) {
                    tracing::debug!(
                        "Splash screen auto-dismissing after {:?}ms",
                        elapsed.as_millis()
                    );
                    self.show_splash = false;
                    self.splash_start_time = None;
                } else {
                    tracing::debug!(
                        "Rendering splash screen (elapsed: {:?}ms)",
                        elapsed.as_millis()
                    );
                    let chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Min(0)])
                        .split(f.size());
                    render_splash(f, chunks[0], &self.theme);
                    return;
                }
            } else {
                // Fallback: if start_time is None but show_splash is true, hide it
                tracing::warn!(
                    "Splash screen should show but splash_start_time is None - hiding splash"
                );
                self.show_splash = false;
            }
        }

        let terminal_width = f.size().width;
        let terminal_height = f.size().height;
        let current_size = (terminal_width, terminal_height);

        // Only recalculate layout dimensions when terminal size changes
        // This prevents flickering/bouncing caused by per-frame recalculation
        let size_changed = self.cached_terminal_size != Some(current_size);
        if size_changed {
            self.cached_terminal_size = Some(current_size);

            // Calculate header height using EXACT same logic as header.rs
            // header.rs uses: left_area.width.saturating_sub(12) where left_area is 70% of total
            // We need to match this exactly to prevent mismatched wrapping calculations
            let header_left_width = {
                // Layout::split with Percentage(70) gives floor(width * 70 / 100)
                // but we need to account for potential rounding - use the same method
                let header_chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
                    .split(Rect::new(0, 0, terminal_width, 1));
                header_chunks[0].width
            };
            let available_width_for_resources = header_left_width.saturating_sub(12);

            // Header content lines:
            // 1. Context line (Context: xxx  Namespace: xxx)
            // 2. Flux9s | Total line
            // 3+ Resource type lines (variable based on wrapping)
            // +1 if filter is active (filter status line)
            // Plus 2 for borders
            let base_content_lines: u16 = 2; // Context line + Flux9s/Total line
            let filter_line: u16 =
                if !self.filter.is_empty() || self.selected_resource_type.is_some() {
                    1
                } else {
                    0
                };

            let resource_type_lines: u16 = {
                let counts = self.state.count_by_type();
                if counts.is_empty() {
                    1 // At least one line for "no resources"
                } else {
                    let mut lines: u16 = 1;
                    let mut current_len: usize = 11; // "Resources: " prefix

                    // Sort counts to match header.rs rendering order (alphabetical)
                    let mut type_counts: Vec<_> = counts.iter().collect();
                    type_counts.sort_by_key(|(resource_type, _)| *resource_type);

                    for (rt, count) in type_counts.iter() {
                        let part = format!("{}:{} ", rt, count);
                        // Match header.rs wrapping logic exactly (line 77-78)
                        if current_len + part.len() > available_width_for_resources as usize
                            && current_len > 11
                        {
                            lines += 1;
                            current_len = part.len();
                        } else {
                            current_len += part.len();
                        }
                    }
                    lines
                }
            };

            // Total content lines + 2 for borders
            let content_lines = base_content_lines + filter_line + resource_type_lines;
            // Minimum height for ASCII art + borders
            use crate::tui::constants::MIN_HEADER_HEIGHT;
            self.cached_header_height = (content_lines + 2).max(MIN_HEADER_HEIGHT);

            // Calculate footer height using EXACT same logic as footer.rs
            // Match the wrapping logic in footer.rs:render_navigation_footer
            let mut nav_segments: Vec<(String, String)> = vec![
                ("j/k ".to_string(), "Navigate".to_string()),
                (":".to_string(), "Command".to_string()),
                ("Enter".to_string(), "Details".to_string()),
                ("y".to_string(), "YAML".to_string()),
                ("t".to_string(), "Trace".to_string()),
                ("g".to_string(), "Graph".to_string()),
                ("f".to_string(), "Favorite".to_string()),
                ("h".to_string(), "History".to_string()),
                ("s".to_string(), "Suspend".to_string()),
                ("r".to_string(), "Resume".to_string()),
                ("R".to_string(), "Reconcile".to_string()),
                ("W".to_string(), "Reconcile+Source".to_string()),
                ("d".to_string(), "Delete".to_string()),
                ("/".to_string(), "Filter(Name)".to_string()),
                ("?".to_string(), "Help".to_string()),
                ("Esc".to_string(), "Back/Quit".to_string()),
            ];

            // Add namespace hotkeys (matching footer.rs)
            use crate::tui::constants::{
                MAX_FOOTER_NAMESPACE_HOTKEYS, MAX_FOOTER_NAMESPACE_LENGTH,
            };
            if !self.namespace_hotkeys.is_empty() {
                for (idx, ns) in self
                    .namespace_hotkeys
                    .iter()
                    .take(MAX_FOOTER_NAMESPACE_HOTKEYS)
                    .enumerate()
                {
                    let display_ns = if ns == "all" {
                        "all".to_string()
                    } else if ns.len() > MAX_FOOTER_NAMESPACE_LENGTH {
                        ns[..MAX_FOOTER_NAMESPACE_LENGTH].to_string()
                    } else {
                        ns.clone()
                    };
                    let label = if (ns == "all" && self.namespace.is_none())
                        || self.namespace.as_ref() == Some(ns)
                    {
                        format!("NS:{}*", display_ns)
                    } else {
                        format!("NS:{}", display_ns)
                    };
                    nav_segments.push((idx.to_string(), label));
                }
            }

            let footer_available_width = terminal_width.saturating_sub(2); // Account for borders

            // Calculate segment lengths (matching footer.rs:223-233)
            let mut segment_lengths: Vec<usize> = Vec::new();
            for (idx, (key, label)) in nav_segments.iter().enumerate() {
                let separator_len = if idx > 0 { 3 } else { 0 }; // " | "
                let segment_len = if key == "j/k " {
                    key.len() + label.len()
                } else {
                    key.len() + 1 + label.len() // key + space + label
                };
                segment_lengths.push(separator_len + segment_len);
            }

            // Split segments into two lines (matching footer.rs:235-260)
            let mut line1_length = 0;
            let mut use_line2 = false;

            for (idx, _) in nav_segments.iter().enumerate() {
                let segment_len = segment_lengths[idx];

                // If adding this segment would exceed width and we're on line 1, start line 2
                if line1_length + segment_len > footer_available_width as usize
                    && !use_line2
                    && line1_length > 0
                {
                    use_line2 = true;
                    break; // We've determined we need 2 lines, no need to continue
                }

                if !use_line2 {
                    line1_length += segment_len;
                }
            }

            // Calculate number of lines needed (1 or 2)
            let footer_content_lines: u16 = if use_line2 { 2 } else { 1 };

            self.cached_footer_height = footer_content_lines + 2; // Content + borders
        }

        let header_height = self.cached_header_height;
        let footer_constraint = self.cached_footer_height;

        // Ensure we have minimum terminal size
        use crate::tui::constants::MIN_TERMINAL_WIDTH;
        let min_height = header_height + footer_constraint + 3; // header + footer + min content
        let min_width = MIN_TERMINAL_WIDTH;

        if terminal_height < min_height || terminal_width < min_width {
            // Terminal too small - show error
            let error_msg = format!(
                "Terminal too small! Need at least {}x{} (current: {}x{})",
                min_width, min_height, terminal_width, terminal_height
            );
            let error_lines = vec![
                Line::from(""),
                Line::from(error_msg),
                Line::from("Please resize your terminal window."),
            ];
            let error_block = Block::default().title("Error").borders(Borders::ALL);
            let error_para = Paragraph::new(error_lines).block(error_block);
            f.render_widget(error_para, f.size());
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                if self.config.ui.headless {
                    Constraint::Length(0) // No header in headless mode
                } else {
                    Constraint::Length(header_height) // Cached header height
                },
                Constraint::Min(0),                    // Main content (flexible)
                Constraint::Length(footer_constraint), // Cached footer height
            ])
            .split(f.size());

        let resources = self.get_filtered_resources();
        // Only render header if not in headless mode
        if !self.config.ui.headless {
            let health_percentage = self.calculate_health_percentage();
            let health_filter_status = match self.health_filter {
                HealthFilter::Healthy => Some("healthy"),
                HealthFilter::Unhealthy => Some("unhealthy"),
                HealthFilter::All => None,
            };
            render_header(
                f,
                chunks[0],
                &self.state,
                &self.context,
                &self.namespace,
                &self.filter,
                &self.selected_resource_type,
                resources.len(),
                health_percentage,
                health_filter_status,
                self.config.read_only,
                &self.theme,
                self.config.ui.no_icons,
                self.namespace_hotkeys(),
            );
        }
        self.render_main(f, chunks[1]);
        render_footer(
            f,
            chunks[2],
            self.command_mode,
            &self.command_buffer,
            self.filter_mode,
            &self.filter,
            self.show_help,
            &self.confirmation_pending,
            &self.status_message,
            &self.operation_registry,
            &self.state,
            &self.theme,
            self.namespace_hotkeys(),
            &self.namespace,
        );
    }

    fn render_main(&mut self, f: &mut Frame, area: Rect) {
        if self.confirmation_pending.is_some() {
            if let Some(ref confirmation) = self.confirmation_pending {
                render_confirmation(
                    f,
                    area,
                    confirmation,
                    &self.operation_registry,
                    &self.state,
                    &self.theme,
                );
            }
            return;
        }

        if self.show_help {
            render_help(f, area, &self.theme, self.namespace_hotkeys());
        } else {
            match self.current_view {
                View::ResourceList => {
                    let resources = self.get_filtered_resources();
                    render_resource_list(
                        f,
                        area,
                        &resources,
                        self.selected_index,
                        &mut self.scroll_offset,
                        &self.selected_resource_type,
                        &self.resource_objects,
                        &self.theme,
                        self.config.ui.no_icons,
                        &self.favorites,
                    );
                }
                View::ResourceDetail => {
                    render_resource_detail(
                        f,
                        area,
                        &self.selected_resource_key,
                        &self.state,
                        &self.resource_objects,
                        &self.theme,
                    );
                }
                View::ResourceYAML => {
                    render_resource_yaml(
                        f,
                        area,
                        &self.selected_resource_key,
                        &self.state,
                        &self.resource_objects,
                        &self.yaml_fetched,
                        &self.yaml_fetch_pending,
                        &mut self.yaml_scroll_offset,
                        &self.theme,
                    );
                }
                View::ResourceTrace => {
                    views::trace::render_resource_trace(
                        f,
                        area,
                        &self.selected_resource_key,
                        &self.trace_result,
                        &self.trace_pending,
                        &mut self.trace_scroll_offset,
                        &self.theme,
                    );
                }
                View::ResourceFavorites => {
                    // Get filtered resources (favorites only)
                    let resources = self.get_filtered_resources();
                    render_resource_list(
                        f,
                        area,
                        &resources,
                        self.selected_index,
                        &mut self.scroll_offset,
                        &self.selected_resource_type,
                        &self.resource_objects,
                        &self.theme,
                        self.config.ui.no_icons,
                        &self.favorites,
                    );
                }
                View::ResourceGraph => {
                    views::render_resource_graph(
                        f,
                        area,
                        &self.selected_resource_key,
                        &self.graph_result,
                        &self.graph_pending,
                        &mut self.graph_scroll_offset,
                        &self.theme,
                    );
                }
                View::ResourceHistory => {
                    if let Some(ref key) = self.selected_resource_key {
                        if let Some(resource) = self.state.get(key) {
                            if render_reconciliation_history(
                                f,
                                area,
                                &resource,
                                &self.resource_objects,
                                &mut self.history_scroll_offset,
                                &self.theme,
                            )
                            .is_err()
                            {
                                // Error already rendered in the function
                            }
                        } else {
                            let text = vec![
                                ratatui::text::Line::from("Resource not found"),
                                ratatui::text::Line::from(""),
                                ratatui::text::Line::from("Press Esc to go back"),
                            ];
                            let paragraph = Paragraph::new(text)
                                .style(Style::default().fg(self.theme.text_secondary));
                            f.render_widget(paragraph, area);
                        }
                    } else {
                        let text = vec![
                            ratatui::text::Line::from("No resource selected"),
                            ratatui::text::Line::from(""),
                            ratatui::text::Line::from(
                                "Select a resource and press 'h' to view history",
                            ),
                        ];
                        let paragraph = Paragraph::new(text)
                            .style(Style::default().fg(self.theme.text_secondary));
                        f.render_widget(paragraph, area);
                    }
                }
                View::Help => {
                    render_help(f, area, &self.theme, self.namespace_hotkeys());
                }
            }
        }
    }
}

#[cfg(test)]
impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App")
            .field("state", &self.state)
            .field("current_view", &self.current_view)
            .field("selected_resource_type", &self.selected_resource_type)
            .field("filter", &self.filter)
            .field("filter_mode", &self.filter_mode)
            .field("health_filter", &self.health_filter)
            .field("selected_index", &self.selected_index)
            .field("scroll_offset", &self.scroll_offset)
            .field("yaml_scroll_offset", &self.yaml_scroll_offset)
            .field("show_help", &self.show_help)
            .field("context", &self.context)
            .field("namespace", &self.namespace)
            .field("command_mode", &self.command_mode)
            .field("command_buffer", &self.command_buffer)
            .field("selected_resource_key", &self.selected_resource_key)
            .field("resource_objects", &"<Arc<RwLock<HashMap>>>")
            .field("watcher", &"<Option<ResourceWatcher>>")
            .field("kube_client", &"<Option<kube::Client>>")
            .field("yaml_fetch_pending", &self.yaml_fetch_pending)
            .field("yaml_fetched", &self.yaml_fetched.is_some())
            .field("yaml_fetch_rx", &self.yaml_fetch_rx.is_some())
            .field("trace_pending", &self.trace_pending)
            .field("trace_result", &self.trace_result.is_some())
            .field("trace_result_rx", &self.trace_result_rx.is_some())
            .field("trace_scroll_offset", &self.trace_scroll_offset)
            .field("show_splash", &self.show_splash)
            .field("splash_start_time", &self.splash_start_time)
            .field("operation_registry", &"<OperationRegistry>")
            .field("pending_operation", &self.pending_operation)
            .field("operation_result_rx", &self.operation_result_rx.is_some())
            .field("last_operation_key", &self.last_operation_key)
            .field("confirmation_pending", &self.confirmation_pending)
            .field("status_message", &self.status_message)
            .field("status_message_time", &self.status_message_time)
            .field("theme", &self.theme)
            .field("config", &self.config)
            .field("namespace_hotkeys", &self.namespace_hotkeys)
            .field("pending_context_switch", &self.pending_context_switch)
            .field("cached_terminal_size", &self.cached_terminal_size)
            .field("cached_header_height", &self.cached_header_height)
            .field("cached_footer_height", &self.cached_footer_height)
            .field("favorites", &self.favorites)
            .field("favorites_pending_save", &self.favorites_pending_save)
            .field("history_scroll_offset", &self.history_scroll_offset)
            .field("graph_scroll_offset", &self.graph_scroll_offset)
            .field("graph_pending", &self.graph_pending)
            .field("graph_result", &self.graph_result.is_some())
            .field("graph_result_rx", &self.graph_result_rx.is_some())
            .field("previous_list_view", &self.previous_list_view)
            .finish()
    }
}
