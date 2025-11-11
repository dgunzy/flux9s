//! Application state and main TUI logic

use crate::tui::views::*;
use crate::tui::{default_theme, OperationRegistry, Theme};
use crate::watcher::{ResourceState, WatchEvent};
use crossterm::event::KeyEvent;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::Line,
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

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
    show_splash: bool,
    splash_start_time: Option<std::time::Instant>,
    operation_registry: OperationRegistry,
    pending_operation: Option<(String, String, String, char)>, // (resource_type, namespace, name, operation_key)
    operation_result_rx: Option<tokio::sync::oneshot::Receiver<anyhow::Result<()>>>, // Channel to receive operation result
    last_operation_key: Option<char>, // Store operation key for success message
    confirmation_pending: Option<(String, String, String, char)>, // (resource_type, namespace, name, operation_key)
    status_message: Option<(String, bool)>,                       // (message, is_error)
    theme: Theme,
}

#[derive(Clone, Copy, PartialEq)]
pub enum View {
    ResourceList,
    ResourceDetail,
    ResourceYAML,
    Help,
}

impl App {
    pub fn new(state: ResourceState, context: String, namespace: Option<String>) -> Self {
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
            show_splash: true,
            splash_start_time: Some(std::time::Instant::now()),
            operation_registry: OperationRegistry::new(),
            pending_operation: None,
            operation_result_rx: None,
            last_operation_key: None,
            confirmation_pending: None,
            status_message: None,
            theme: default_theme(),
        }
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

    pub fn handle_watch_event(&mut self, event: WatchEvent) {
        // Watch events update the state directly through the watcher
        // This method is here for future use if we need to handle events in the TUI
        match event {
            WatchEvent::Applied(_, _, _, _) | WatchEvent::Deleted(_, _, _) => {
                // State is updated by the watcher, we just need to refresh
            }
            WatchEvent::Error(_) => {
                // Errors are handled elsewhere
            }
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

        // Clear status messages on any key press (except in special modes)
        if self.status_message.is_some() && !self.command_mode && !self.filter_mode {
            // Don't clear on Esc (might be used for navigation)
            if key.code != crossterm::event::KeyCode::Esc {
                self.status_message = None;
            }
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
                    View::ResourceDetail | View::ResourceYAML => {
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
            | crossterm::event::KeyCode::Char('R') => {
                // Handle Flux operations
                if self.current_view == View::ResourceList {
                    let resources = self.get_filtered_resources();
                    if let Some(resource) = resources.get(self.selected_index) {
                        let op_key = match key.code {
                            crossterm::event::KeyCode::Char('s') => 's',
                            crossterm::event::KeyCode::Char('r') => 'r',
                            crossterm::event::KeyCode::Char('d') => 'd',
                            crossterm::event::KeyCode::Char('R') => 'R',
                            _ => return None,
                        };

                        if let Some(operation) = self.operation_registry.get_by_keybinding(op_key) {
                            if operation.is_valid_for(&resource.resource_type) {
                                if operation.requires_confirmation() {
                                    // Show confirmation dialog
                                    self.confirmation_pending = Some((
                                        resource.resource_type.clone(),
                                        resource.namespace.clone(),
                                        resource.name.clone(),
                                        op_key,
                                    ));
                                } else {
                                    // Execute immediately
                                    self.execute_operation(
                                        &resource.resource_type,
                                        &resource.namespace,
                                        &resource.name,
                                        op_key,
                                    );
                                }
                            }
                        }
                    }
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
                self.filter.clear();
                None
            }
            crossterm::event::KeyCode::Enter => {
                // Apply filter and exit filter mode
                self.filter_mode = false;
                self.selected_index = 0;
                self.scroll_offset = 0;
                None
            }
            crossterm::event::KeyCode::Backspace => {
                self.filter.pop();
                None
            }
            crossterm::event::KeyCode::Char(c) => {
                self.filter.push(c);
                self.selected_index = 0;
                self.scroll_offset = 0;
                None
            }
            _ => None,
        }
    }

    fn handle_confirmation_key(&mut self, key: KeyEvent) -> Option<bool> {
        if let Some((resource_type, namespace, name, op_key)) = &self.confirmation_pending {
            match key.code {
                crossterm::event::KeyCode::Char('y') | crossterm::event::KeyCode::Char('Y') => {
                    // Confirm operation
                    let rt = resource_type.clone();
                    let ns = namespace.clone();
                    let n = name.clone();
                    let key = *op_key;
                    self.confirmation_pending = None;
                    self.execute_operation(&rt, &ns, &n, key);
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
        if self.operation_registry.get_by_keybinding(op_key).is_some() {
            if self.kube_client.is_some() {
                let rt = resource_type.to_string();
                let ns = namespace.to_string();
                let n = name.to_string();

                // Mark operation as pending - will be executed in main loop
                self.pending_operation = Some((rt, ns, n, op_key));
            }
        }
    }

    pub fn trigger_operation_execution(
        &mut self,
    ) -> Option<(
        String,
        String,
        String,
        char,
        kube::Client,
        tokio::sync::oneshot::Sender<anyhow::Result<()>>,
    )> {
        if let Some((ref resource_type, ref namespace, ref name, op_key)) = self.pending_operation {
            if let Some(ref client) = self.kube_client {
                if self.operation_registry.get_by_keybinding(op_key).is_some() {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    let rt = resource_type.clone();
                    let ns = namespace.clone();
                    let n = name.clone();
                    let key = op_key;
                    let client_clone = client.clone();

                    self.pending_operation = None;
                    self.last_operation_key = Some(key); // Store operation key for success message
                    self.operation_result_rx = Some(rx);

                    return Some((rt, ns, n, key, client_clone, tx));
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

    pub fn set_operation_result(&mut self, result: anyhow::Result<()>) {
        match result {
            Ok(_) => {
                if let Some(op_key) = self.last_operation_key.take() {
                    if let Some(operation) = self.operation_registry.get_by_keybinding(op_key) {
                        self.status_message = Some((
                            format!("{} completed successfully", operation.name()),
                            false,
                        ));
                    } else {
                        self.status_message =
                            Some(("Operation completed successfully".to_string(), false));
                    }
                } else {
                    self.status_message =
                        Some(("Operation completed successfully".to_string(), false));
                }
            }
            Err(e) => {
                self.last_operation_key = None;
                self.status_message = Some((format!("Operation failed: {}", e), true));
            }
        }
    }

    pub fn operation_registry(&self) -> &OperationRegistry {
        &self.operation_registry
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

        // Handle quit commands
        if cmd_lower == "q" || cmd_lower == "q!" {
            return Some(true); // Quit
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
            self.selected_resource_type = None;
            return None;
        }

        if let Some(display_name) = crate::watcher::get_display_name_for_command(&cmd_lower) {
            self.selected_resource_type = Some(display_name.to_string());
            self.selected_index = 0;
            self.scroll_offset = 0;
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

        // Apply text filter if set
        if !self.filter.is_empty() {
            resources.retain(|r| {
                r.name.contains(&self.filter)
                    || r.namespace.contains(&self.filter)
                    || r.resource_type.contains(&self.filter)
            });
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

        // Calculate header height dynamically based on resource type wrapping
        // Filter info is now on its own line when active, so add 1 line if filtering
        // Ensure header is tall enough for ASCII art (5 lines) + borders + padding
        let base_height = 3; // Context line, Flux9s line, at least one resource line
        let filter_line = if !self.filter.is_empty() || self.selected_resource_type.is_some() {
            1
        } else {
            0
        };
        let resource_type_lines = {
            let counts = self.state.count_by_type();
            let available_width = (f.size().width * 70 / 100).saturating_sub(12);
            let mut lines = 1;
            let mut current_len = 11; // "Resources: "
            for (rt, count) in counts.iter() {
                let part = format!("{}:{} ", rt, count);
                if current_len + part.len() > available_width as usize && current_len > 11 {
                    lines += 1;
                    current_len = part.len();
                } else {
                    current_len += part.len();
                }
            }
            lines
        };
        // Ensure header is at least tall enough for ASCII art (5 lines) + borders
        let min_header_height = 7; // 5 ASCII lines + 2 borders
        let header_height =
            (base_height + filter_line + resource_type_lines).max(min_header_height);

        // Calculate footer height dynamically - footer can be 1-2 lines
        // We need to calculate this before rendering to prevent bouncing
        let footer_height = {
            let available_width = f.size().width.saturating_sub(2);
            // Calculate if footer would wrap (simplified calculation)
            // Navigation segments: j/k Navigate, : Command, Enter Details, y YAML, s Suspend, r Resume, d Delete, R Reconcile, / Filter(Name), ? Help, Esc Back/Quit
            let nav_segments_count = 11;
            let estimated_chars_per_segment = 12; // Average chars per segment including key and label
            let estimated_separators = (nav_segments_count - 1) * 3; // " | " separators
            let estimated_total =
                (nav_segments_count * estimated_chars_per_segment) + estimated_separators;
            if estimated_total > available_width as usize {
                2 // Would wrap to 2 lines
            } else {
                1 // Single line
            }
        };

        // Ensure we have minimum terminal size - if too small, show error message
        let terminal_height = f.size().height;
        let terminal_width = f.size().width;
        let footer_constraint = footer_height + 2; // Footer content + borders
        let min_height = header_height + footer_constraint + 3; // header + footer + min content
        let min_width = 80;

        if terminal_height < min_height as u16 || terminal_width < min_width {
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
                Constraint::Length(header_height as u16), // Dynamic header height
                Constraint::Min(0),                       // Main content (flexible)
                Constraint::Length(footer_constraint as u16), // Footer (content + borders)
            ])
            .split(f.size());

        let resources = self.get_filtered_resources();
        render_header(
            f,
            chunks[0],
            &self.state,
            &self.context,
            &self.namespace,
            &self.filter,
            &self.selected_resource_type,
            resources.len(),
            &self.theme,
        );
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
            render_help(f, area, &self.theme);
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
                View::Help => {
                    render_help(f, area, &self.theme);
                }
            }
        }
    }
}
