//! Application state and main TUI logic

use crate::tui::views::{self, *};
use crate::tui::{OperationRegistry, Theme};
use crate::watcher::{ResourceKey, ResourceState};
use anyhow::Result;
use crossterm::event::KeyEvent;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::Line,
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use std::collections::HashMap;
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
#[derive(Clone)]
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
    config: crate::config::Config,  // Application configuration
    namespace_hotkeys: Vec<String>, // Namespace hotkeys (0-9), where 0=all, 1=flux-system, etc.
    // Cached layout dimensions to prevent bouncing/flickering
    cached_terminal_size: Option<(u16, u16)>, // (width, height)
    cached_header_height: u16,
    cached_footer_height: u16,
}

#[derive(Clone, Copy, PartialEq)]
pub enum View {
    ResourceList,
    ResourceDetail,
    ResourceYAML,
    ResourceTrace,
    #[allow(dead_code)] // Reserved for future alternative help view implementation
    Help,
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
            splash_start_time: if config.ui.splashless {
                None
            } else {
                Some(std::time::Instant::now())
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
            // Initialize layout cache - will be populated on first render
            cached_terminal_size: None,
            cached_header_height: 7, // Default minimum
            cached_footer_height: 3, // Default minimum
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
        if !config.namespace_hotkeys.is_empty() {
            if config.namespace_hotkeys.len() > 10 {
                tracing::warn!(
                    "namespace_hotkeys has {} items, maximum is 10. Truncating to first 10.",
                    config.namespace_hotkeys.len()
                );
                return config.namespace_hotkeys[..10].to_vec();
            }
            return config.namespace_hotkeys.clone();
        }

        // Build defaults: 0=all, 1=flux-system, 2-9=discovered namespaces
        let mut hotkeys = vec!["all".to_string(), "flux-system".to_string()];

        // Add discovered namespaces (skip flux-system if already in list)
        for ns in discovered_namespaces {
            if ns != "flux-system" && hotkeys.len() < 10 {
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

    pub fn set_kube_client(&mut self, client: kube::Client) {
        self.kube_client = Some(client);
    }

    pub fn set_watcher(&mut self, watcher: crate::watcher::ResourceWatcher) {
        self.watcher = Some(watcher);
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
                    View::ResourceDetail | View::ResourceYAML | View::ResourceTrace => {
                        // Go back to resource list
                        self.current_view = View::ResourceList;
                        self.selected_resource_key = None;
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
                // Handle Flux operations - works from both list and detail view
                let resource_info = if self.current_view == View::ResourceList {
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
                // Trace command - works from both list and detail view
                let resource_info = if self.current_view == View::ResourceList {
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
                if self.current_view == View::ResourceList {
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
                    // Switch from detail to YAML view
                    if let Some(ref key) = self.selected_resource_key {
                        self.yaml_fetch_pending = Some(key.clone());
                        self.yaml_fetched = None;
                        self.yaml_scroll_offset = 0; // Reset scroll when entering YAML view
                    }
                    self.current_view = View::ResourceYAML;
                }
            }
            crossterm::event::KeyCode::Enter => {
                if self.current_view == View::ResourceList {
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
            crossterm::event::KeyCode::Backspace => {
                // Backspace goes back (same as Escape for detail view)
                if self.current_view == View::ResourceDetail
                    || self.current_view == View::ResourceYAML
                    || self.current_view == View::ResourceTrace
                {
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
        const STATUS_MESSAGE_TIMEOUT_SECS: u64 = 4;
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

        // Try to find matching command
        let commands = crate::watcher::get_all_commands();
        let mut matches: Vec<&str> = commands
            .iter()
            .flat_map(|(_, aliases)| aliases.iter())
            .filter(|alias| alias.starts_with(&cmd_lower))
            .copied()
            .collect();

        // Also check namespace commands
        if cmd_lower.starts_with("ns ") || cmd_lower.starts_with("namespace ") {
            // Don't autocomplete namespace names
            return;
        }

        // Check for special commands
        if "all".starts_with(&cmd_lower) {
            matches.push("all");
        }
        if "clear".starts_with(&cmd_lower) {
            matches.push("clear");
        }

        if matches.is_empty() {
            return;
        }

        // Use first match - replace buffer with matched command (no colon, it's shown in UI)
        if let Some(first_match) = matches.first() {
            self.command_buffer = first_match.to_string();
        }
    }

    fn execute_command(&mut self) -> Option<bool> {
        let cmd = self.command_buffer.trim();
        let cmd_lower = cmd.to_lowercase();

        // Handle help command
        if cmd_lower == "help" || cmd_lower == "h" || cmd_lower == "?" {
            self.show_help = !self.show_help;
            return None;
        }

        // Handle quit commands
        if cmd_lower == "q" || cmd_lower == "q!" || cmd_lower == "quit" || cmd_lower == "exit" {
            return Some(true); // Quit
        }

        // Handle readonly toggle command
        if cmd_lower == "readonly" || cmd_lower == "read-only" {
            self.config.read_only = !self.config.read_only;
            let status = if self.config.read_only {
                "enabled"
            } else {
                "disabled"
            };
            self.set_status_message((format!("Readonly mode {}", status), false));
            return None;
        }

        // Handle skin/theme change command
        if cmd_lower.starts_with("skin ") {
            let theme_name = cmd.split_whitespace().nth(1).map(|s| s.to_string());
            if let Some(name) = theme_name {
                match self.set_theme(&name) {
                    Ok(_) => {
                        let msg = format!("Theme changed to: {}", name);
                        self.set_status_message((msg, false));
                    }
                    Err(e) => {
                        let msg = format!("Failed to load theme '{}': {}. Use `default` to return to default theme", name, e);
                        self.set_status_message((msg, true));
                    }
                }
            } else {
                self.set_status_message(("Usage: :skin <theme-name>".to_string(), true));
            }
            return None;
        }

        // Handle trace command - trace ownership chain
        if cmd_lower.starts_with("trace ") {
            let parts: Vec<&str> = cmd.split_whitespace().skip(1).collect();
            if parts.is_empty() {
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
            } else {
                // Parse resource type/name format (e.g., "kustomization/cabot-book" or "Kustomization/cabot-book")
                let resource_str = parts.join(" ");
                let resource_parts: Vec<&str> = resource_str.split('/').collect();
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

        // Handle namespace switching - restart watchers with new namespace
        if cmd_lower.starts_with("namespace ") || cmd_lower.starts_with("ns ") {
            let ns = cmd.split_whitespace().nth(1);
            let new_namespace = match ns {
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

        // Use registry for resource type command mapping
        if cmd_lower == "all" || cmd_lower == "clear" {
            if self.selected_resource_type.is_some() {
                self.selected_resource_type = None;
                self.invalidate_layout_cache(); // Resource type filter affects header display
            }
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

        // Sort by namespace, then resource type, then name
        resources.sort_by(|a, b| {
            a.namespace
                .cmp(&b.namespace)
                .then_with(|| a.resource_type.cmp(&b.resource_type))
                .then_with(|| a.name.cmp(&b.name))
        });

        resources
    }

    pub fn render(&mut self, f: &mut Frame) {
        // Show splash screen for 1.5 seconds, then auto-dismiss
        if self.show_splash {
            if let Some(start_time) = self.splash_start_time {
                if start_time.elapsed() >= std::time::Duration::from_millis(1500) {
                    self.show_splash = false;
                    self.splash_start_time = None;
                } else {
                    let chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Min(0)])
                        .split(f.size());
                    render_splash(f, chunks[0], &self.theme);
                    return;
                }
            } else {
                // Fallback: if start_time is None but show_splash is true, hide it
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
            // Minimum height for ASCII art (5 lines) + 2 borders = 7
            let min_header_height: u16 = 7;
            self.cached_header_height = (content_lines + 2).max(min_header_height);

            // Calculate footer height using EXACT same logic as footer.rs
            // footer.rs nav_segments match these entries exactly
            let nav_segments: &[(&str, &str)] = &[
                ("j/k ", "Navigate"),
                (":", "Command"),
                ("Enter", "Details"),
                ("y", "YAML"),
                ("t", "Trace"),
                ("s", "Suspend"),
                ("r", "Resume"),
                ("R", "Reconcile"),
                ("W", "Reconcile+Source"),
                ("d", "Delete"),
                ("/", "Filter"),
                ("?", "Help"),
                ("Esc", "Back/Quit"),
            ];

            let footer_available_width = terminal_width.saturating_sub(2); // Account for borders

            // Calculate exact total length (matching footer.rs:190-198)
            let mut total_length: usize = 0;
            for (idx, (key, label)) in nav_segments.iter().enumerate() {
                let separator_len = if idx > 0 { 3 } else { 0 }; // " | "
                let segment_len = if *key == "j/k " {
                    key.len() + label.len()
                } else {
                    key.len() + 1 + label.len() // key + space + label
                };
                total_length += separator_len + segment_len;
            }

            // Calculate lines needed (matching footer.rs:201-205)
            let footer_content_lines: u16 = if footer_available_width > 0 {
                ((total_length as f32) / (footer_available_width as f32)).ceil() as u16
            } else {
                1
            };

            self.cached_footer_height = footer_content_lines.max(1) + 2; // Content + borders
        }

        let header_height = self.cached_header_height;
        let footer_constraint = self.cached_footer_height;

        // Ensure we have minimum terminal size
        let min_height = header_height + footer_constraint + 3; // header + footer + min content
        let min_width: u16 = 80;

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
            render_header(
                f,
                chunks[0],
                &self.state,
                &self.context,
                &self.namespace,
                &self.filter,
                &self.selected_resource_type,
                resources.len(),
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
                View::Help => {
                    render_help(f, area, &self.theme, self.namespace_hotkeys());
                }
            }
        }
    }
}
