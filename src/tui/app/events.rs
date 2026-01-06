//! Event handling for the application
//!
//! This module contains all input handling logic including keyboard events,
//! command mode, filter mode, and confirmation dialogs.

use super::core::App;
use super::state::{HealthFilter, PendingOperation, View};
use crate::watcher::ResourceKey;
use crossterm::event::KeyEvent;

impl App {
    /// Main keyboard event handler
    ///
    /// Returns Some(true) to quit, Some(false) to continue with special action,
    /// None for normal continuation
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<bool> {
        // Return Some(true) to quit, Some(false) to continue, None for no action

        // If splash is showing, dismiss it immediately on any keypress
        if self.ui_state.show_splash {
            self.ui_state.show_splash = false;
            self.ui_state.splash_start_time = None;
            // Don't process the key further - just dismiss splash
            return None;
        }

        // Handle confirmation dialog first
        if self.async_state.confirmation_pending.is_some() {
            return self.handle_confirmation_key(key);
        }

        // Handle Esc to dismiss status messages
        if self.ui_state.status_message.is_some()
            && !self.ui_state.command_mode
            && !self.view_state.filter_mode
            && key.code == crossterm::event::KeyCode::Esc
        {
            self.ui_state.status_message = None;
            self.ui_state.status_message_time = None;
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

        if self.ui_state.status_message.is_some()
            && !self.ui_state.command_mode
            && !self.view_state.filter_mode
            && !is_operation_key
            && key.code != crossterm::event::KeyCode::Esc
        {
            self.ui_state.status_message = None;
            self.ui_state.status_message_time = None;
        }

        if self.ui_state.command_mode {
            if let Some(should_quit) = self.handle_command_key(key) {
                return Some(should_quit);
            }
            return None;
        }

        if self.view_state.filter_mode {
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

                        self.state.clear();
                        {
                            let mut objects = self.resource_objects.write().unwrap();
                            objects.clear();
                        }
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

                        self.view_state.selected_index = 0;
                        self.view_state.scroll_offset = 0;
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
                if self.ui_state.show_help {
                    self.ui_state.show_help = false;
                    return None;
                }
                match self.view_state.current_view {
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
                        self.view_state.current_view = self.view_state.previous_list_view;
                        self.selection_state.selected_resource_key = None;
                        return None;
                    }
                    View::ResourceFavorites => {
                        // Go back to resource list
                        self.view_state.current_view = View::ResourceList;
                        return None;
                    }
                    View::Help => {
                        self.view_state.current_view = View::ResourceList;
                        return None;
                    }
                }
            }
            crossterm::event::KeyCode::Char('?') => {
                self.ui_state.show_help = !self.ui_state.show_help;
            }
            crossterm::event::KeyCode::Char('s')
            | crossterm::event::KeyCode::Char('r')
            | crossterm::event::KeyCode::Char('d')
            | crossterm::event::KeyCode::Char('R')
            | crossterm::event::KeyCode::Char('W') => {
                // Handle Flux operations - works from list, favorites, and detail view
                let resource_info = if self.view_state.current_view == View::ResourceList
                    || self.view_state.current_view == View::ResourceFavorites
                {
                    let resources = self.get_filtered_resources();
                    resources.get(self.view_state.selected_index).cloned()
                } else if self.view_state.current_view == View::ResourceDetail {
                    if let Some(ref key) = self.selection_state.selected_resource_key {
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
                                self.async_state.confirmation_pending =
                                    Some(PendingOperation::new(
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
                let resource_info = if self.view_state.current_view == View::ResourceList
                    || self.view_state.current_view == View::ResourceFavorites
                {
                    let resources = self.get_filtered_resources();
                    resources.get(self.view_state.selected_index).cloned()
                } else if self.view_state.current_view == View::ResourceDetail {
                    if let Some(ref key) = self.selection_state.selected_resource_key {
                        self.state.get(key)
                    } else {
                        None
                    }
                } else {
                    None
                };

                if let Some(resource) = resource_info {
                    self.async_state.trace_pending = Some(ResourceKey::new(
                        resource.resource_type.clone(),
                        resource.namespace.clone(),
                        resource.name.clone(),
                    ));
                    self.async_state.trace_result = None;
                    self.view_state.trace_scroll_offset = 0;
                }
            }
            crossterm::event::KeyCode::Char(':') => {
                self.ui_state.command_mode = true;
                self.ui_state.command_buffer.clear();
            }
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k') => {
                if self.view_state.current_view == View::ResourceYAML {
                    // Scroll up in YAML view
                    if self.view_state.yaml_scroll_offset > 0 {
                        self.view_state.yaml_scroll_offset -= 1;
                    }
                } else if self.view_state.current_view == View::ResourceTrace {
                    // Scroll up in trace view
                    self.view_state.trace_scroll_offset =
                        self.view_state.trace_scroll_offset.saturating_sub(1);
                } else if self.view_state.current_view == View::ResourceHistory {
                    // Scroll up in history view
                    self.view_state.history_scroll_offset =
                        self.view_state.history_scroll_offset.saturating_sub(1);
                } else if self.view_state.current_view == View::ResourceGraph {
                    // Scroll up in graph view (line-based, like YAML)
                    self.view_state.graph_scroll_offset =
                        self.view_state.graph_scroll_offset.saturating_sub(1);
                } else {
                    // Normal navigation
                    if self.view_state.selected_index > 0 {
                        self.view_state.selected_index -= 1;
                        if self.view_state.selected_index < self.view_state.scroll_offset {
                            self.view_state.scroll_offset = self.view_state.selected_index;
                        }
                    }
                }
            }
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j') => {
                if self.view_state.current_view == View::ResourceYAML {
                    // Scroll down in YAML view - we'll handle max scroll in render
                    self.view_state.yaml_scroll_offset += 1;
                } else if self.view_state.current_view == View::ResourceTrace {
                    // Scroll down in trace view
                    self.view_state.trace_scroll_offset += 1;
                } else if self.view_state.current_view == View::ResourceHistory {
                    // Scroll down in history view
                    self.view_state.history_scroll_offset += 1;
                } else if self.view_state.current_view == View::ResourceGraph {
                    // Scroll down in graph view (line-based, like YAML)
                    self.view_state.graph_scroll_offset += 1;
                } else {
                    // Normal navigation
                    let resources = self.get_filtered_resources();
                    if self.view_state.selected_index < resources.len().saturating_sub(1) {
                        self.view_state.selected_index += 1;
                    }
                }
            }
            crossterm::event::KeyCode::Char('/') => {
                // Enter filter mode
                self.view_state.filter_mode = true;
                self.view_state.filter.clear();
                self.invalidate_layout_cache(); // Filter state affects header height
            }
            crossterm::event::KeyCode::Char('y') => {
                // View YAML - trigger async fetch
                if self.view_state.current_view == View::ResourceList
                    || self.view_state.current_view == View::ResourceFavorites
                {
                    // Save current view as previous list view before navigating
                    self.view_state.previous_list_view = self.view_state.current_view;
                    let resources = self.get_filtered_resources();
                    if let Some(resource) = resources.get(self.view_state.selected_index) {
                        let key = crate::watcher::resource_key(
                            &resource.namespace,
                            &resource.name,
                            &resource.resource_type,
                        );
                        self.selection_state.selected_resource_key = Some(key.clone());
                        self.async_state.yaml_fetch_pending = Some(key);
                        self.async_state.yaml_fetched = None; // Clear previous fetch
                        self.view_state.yaml_scroll_offset = 0; // Reset scroll when entering YAML view
                        self.view_state.current_view = View::ResourceYAML;
                    }
                } else if self.view_state.current_view == View::ResourceDetail {
                    // From detail view, preserve the previous_list_view (don't overwrite it)
                    if let Some(ref key) = self.selection_state.selected_resource_key {
                        self.async_state.yaml_fetch_pending = Some(key.clone());
                        self.async_state.yaml_fetched = None;
                        self.view_state.yaml_scroll_offset = 0; // Reset scroll when entering YAML view
                    }
                    self.view_state.current_view = View::ResourceYAML;
                }
            }
            crossterm::event::KeyCode::Enter => {
                if self.view_state.current_view == View::ResourceList
                    || self.view_state.current_view == View::ResourceFavorites
                {
                    // Save current view as previous list view before navigating
                    self.view_state.previous_list_view = self.view_state.current_view;
                    let resources = self.get_filtered_resources();
                    if let Some(resource) = resources.get(self.view_state.selected_index) {
                        let key = crate::watcher::resource_key(
                            &resource.namespace,
                            &resource.name,
                            &resource.resource_type,
                        );
                        self.selection_state.selected_resource_key = Some(key);
                        self.view_state.current_view = View::ResourceDetail;
                    }
                }
            }
            crossterm::event::KeyCode::Char('f') => {
                // Toggle favorite - works from list view
                if self.view_state.current_view == View::ResourceList
                    || self.view_state.current_view == View::ResourceFavorites
                {
                    let resources = self.get_filtered_resources();
                    if let Some(resource) = resources.get(self.view_state.selected_index) {
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
                let resource_info = if self.view_state.current_view == View::ResourceList
                    || self.view_state.current_view == View::ResourceFavorites
                {
                    let resources = self.get_filtered_resources();
                    resources.get(self.view_state.selected_index).cloned()
                } else if self.view_state.current_view == View::ResourceDetail {
                    if let Some(ref key) = self.selection_state.selected_resource_key {
                        self.state.get(key)
                    } else {
                        None
                    }
                } else {
                    None
                };

                if let Some(resource) = resource_info {
                    use crate::models::FluxResourceKind;

                    let key = crate::watcher::resource_key(
                        &resource.namespace,
                        &resource.name,
                        &resource.resource_type,
                    );

                    // Check if resource object exists and has status.history
                    let objects = self.resource_objects.read().unwrap();
                    let obj = objects.get(&key);
                    let has_history = obj
                        .and_then(|obj| obj.get("status"))
                        .and_then(|s| s.get("history"))
                        .and_then(|h| h.as_array())
                        .map(|arr| !arr.is_empty())
                        .unwrap_or(false);
                    let is_kustomization = matches!(
                        FluxResourceKind::parse_optional(&resource.resource_type),
                        Some(FluxResourceKind::Kustomization)
                    );

                    drop(objects); // Release lock before switching view

                    if has_history {
                        // Save current view as previous list view before navigating
                        self.view_state.previous_list_view = self.view_state.current_view;
                        self.selection_state.selected_resource_key = Some(key);
                        self.view_state.current_view = View::ResourceHistory;
                        self.view_state.history_scroll_offset = 0;
                    } else {
                        // Show error message immediately
                        let error_msg = if is_kustomization {
                            format!(
                                "Reconciliation history is not supported for Kustomization '{}' in this version of Flux. History requires Flux v2.3.0 or later.",
                                resource.name
                            )
                        } else {
                            let supported_types: Vec<String> =
                                FluxResourceKind::history_supported_types()
                                    .iter()
                                    .map(|k| k.as_str().to_string())
                                    .collect();
                            format!(
                                "Resource '{}' does not have reconciliation history. History is only available for: {}",
                                resource.name,
                                supported_types.join(", ")
                            )
                        };
                        self.set_status_message((error_msg, true));
                    }
                } else {
                    self.set_status_message(("No resource selected".to_string(), true));
                }
            }
            crossterm::event::KeyCode::Char('g') => {
                // View resource graph - works from list, favorites, and detail view
                let resource_info = if self.view_state.current_view == View::ResourceList
                    || self.view_state.current_view == View::ResourceFavorites
                {
                    let resources = self.get_filtered_resources();
                    resources.get(self.view_state.selected_index).cloned()
                } else if self.view_state.current_view == View::ResourceDetail {
                    if let Some(ref key) = self.selection_state.selected_resource_key {
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
                    if self.view_state.current_view == View::ResourceList
                        || self.view_state.current_view == View::ResourceFavorites
                    {
                        self.view_state.previous_list_view = self.view_state.current_view;
                    }

                    // Trigger graph building
                    let key = crate::watcher::resource_key(
                        &resource.namespace,
                        &resource.name,
                        &resource.resource_type,
                    );

                    self.selection_state.selected_resource_key = Some(key.clone());
                    self.async_state.graph_pending = Some(ResourceKey {
                        resource_type: resource.resource_type.clone(),
                        namespace: resource.namespace.clone(),
                        name: resource.name.clone(),
                    });
                    self.async_state.graph_result = None; // Clear previous graph
                    self.view_state.graph_scroll_offset = 0; // Reset scroll
                    self.view_state.current_view = View::ResourceGraph;
                } else {
                    self.set_status_message(("No resource selected".to_string(), true));
                }
            }
            crossterm::event::KeyCode::Backspace => {
                // Backspace goes back (same as Escape for detail view)
                if self.view_state.current_view == View::ResourceDetail
                    || self.view_state.current_view == View::ResourceYAML
                    || self.view_state.current_view == View::ResourceTrace
                    || self.view_state.current_view == View::ResourceHistory
                    || self.view_state.current_view == View::ResourceGraph
                {
                    // Return to previous list view (favorites if we came from there, otherwise list)
                    self.view_state.current_view = self.view_state.previous_list_view;
                    self.selection_state.selected_resource_key = None;
                } else if self.view_state.current_view == View::ResourceFavorites {
                    self.view_state.current_view = View::ResourceList;
                    self.selection_state.selected_resource_key = None;
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
                self.view_state.filter_mode = false;
                let was_filtering = !self.view_state.filter.is_empty();
                self.view_state.filter.clear();
                if was_filtering {
                    self.invalidate_layout_cache(); // Filter state affects header height
                }
                None
            }
            crossterm::event::KeyCode::Enter => {
                // Apply filter and exit filter mode
                self.view_state.filter_mode = false;
                self.view_state.selected_index = 0;
                self.view_state.scroll_offset = 0;
                // Only invalidate if filter was applied (non-empty) - this is when header changes
                if !self.view_state.filter.is_empty() {
                    self.invalidate_layout_cache();
                }
                None
            }
            crossterm::event::KeyCode::Backspace => {
                let was_empty = self.view_state.filter.is_empty();
                self.view_state.filter.pop();
                // Invalidate when transitioning from non-empty to empty (header line change)
                if !was_empty && self.view_state.filter.is_empty() {
                    self.invalidate_layout_cache();
                }
                None
            }
            crossterm::event::KeyCode::Char(c) => {
                let was_empty = self.view_state.filter.is_empty();
                self.view_state.filter.push(c);
                self.view_state.selected_index = 0;
                self.view_state.scroll_offset = 0;
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
        if let Some(ref pending) = self.async_state.confirmation_pending {
            match key.code {
                crossterm::event::KeyCode::Char('y') | crossterm::event::KeyCode::Char('Y') => {
                    // Check readonly mode before confirming
                    if self.config.read_only {
                        self.async_state.confirmation_pending = None;
                        self.set_status_message((
                            "Readonly mode is enabled. Use :readonly to toggle write actions."
                                .to_string(),
                            true,
                        ));
                        return None;
                    }
                    // Confirm operation - clone data before clearing pending state
                    let pending_clone = pending.clone();
                    self.async_state.confirmation_pending = None;
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
                    self.async_state.confirmation_pending = None;
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
            self.async_state.pending_operation = Some(PendingOperation::new(
                resource_type.to_string(),
                namespace.to_string(),
                name.to_string(),
                op_key,
            ));
        }
    }

    fn handle_command_key(&mut self, key: KeyEvent) -> Option<bool> {
        match key.code {
            crossterm::event::KeyCode::Esc => {
                self.ui_state.command_mode = false;
                self.ui_state.command_buffer.clear();
                None
            }
            crossterm::event::KeyCode::Tab => {
                // Autocomplete command
                self.autocomplete_command();
                None
            }
            crossterm::event::KeyCode::Enter => {
                if let Some(should_quit) = self.execute_command() {
                    self.ui_state.command_mode = false;
                    self.ui_state.command_buffer.clear();
                    return Some(should_quit);
                }
                self.ui_state.command_mode = false;
                self.ui_state.command_buffer.clear();
                None
            }
            crossterm::event::KeyCode::Backspace => {
                self.ui_state.command_buffer.pop();
                None
            }
            crossterm::event::KeyCode::Char(c) => {
                self.ui_state.command_buffer.push(c);
                None
            }
            _ => None,
        }
    }

    fn autocomplete_command(&mut self) {
        let cmd = self.ui_state.command_buffer.trim();

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
            self.ui_state.command_buffer = first_match.clone();
        }
    }

    fn execute_command(&mut self) -> Option<bool> {
        let cmd = self.ui_state.command_buffer.trim();
        let cmd_lower = cmd.to_lowercase();

        // Handle help command
        if crate::tui::commands::is_help_command(&cmd_lower) {
            self.ui_state.show_help = !self.ui_state.show_help;
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
                if let Some(key) = &self.selection_state.selected_resource_key {
                    if let Some(rk) = ResourceKey::parse(key) {
                        self.async_state.trace_pending = Some(rk);
                        self.async_state.trace_result = None;
                    } else {
                        tracing::warn!("Failed to parse resource key for trace command: {}", key);
                        self.ui_state.status_message =
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
                        .namespace()
                        .clone()
                        .unwrap_or_else(|| "default".to_string());
                    self.async_state.trace_pending = Some(ResourceKey::new(
                        resource_type_normalized.to_string(),
                        namespace,
                        name.to_string(),
                    ));
                    self.async_state.trace_result = None;
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
                self.state().clear();

                {
                    let mut objects = self.resource_objects().write().unwrap();
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

            self.view_state.selected_index = 0;
            self.view_state.scroll_offset = 0;
            return None;
        }

        // Handle health filter commands
        if crate::tui::commands::is_healthy_command(&cmd_lower) {
            self.view_state.health_filter = HealthFilter::Healthy;
            self.view_state.selected_index = 0;
            self.view_state.scroll_offset = 0;
            self.set_status_message(("Showing healthy resources only".to_string(), false));
            return None;
        }

        if crate::tui::commands::is_unhealthy_command(&cmd_lower) {
            self.view_state.health_filter = HealthFilter::Unhealthy;
            self.view_state.selected_index = 0;
            self.view_state.scroll_offset = 0;
            self.set_status_message(("Showing unhealthy resources only".to_string(), false));
            return None;
        }

        // Handle favorites command
        if crate::tui::commands::is_favorites_command(&cmd_lower) {
            self.view_state.current_view = View::ResourceFavorites;
            self.view_state.selected_index = 0;
            self.view_state.scroll_offset = 0;
            return None;
        }

        // Use registry for resource type command mapping
        if crate::tui::commands::is_all_command(&cmd_lower) {
            // Clear favorites view if active
            if self.view_state.current_view == View::ResourceFavorites {
                self.view_state.current_view = View::ResourceList;
            }
            if self.view_state.selected_resource_type.is_some() {
                self.view_state.selected_resource_type = None;
                self.invalidate_layout_cache(); // Resource type filter affects header display
            }
            // Clear health filter when showing all
            if self.view_state.health_filter != HealthFilter::All {
                self.view_state.health_filter = HealthFilter::All;
                self.set_status_message(("Showing all resources".to_string(), false));
            }
            self.view_state.selected_index = 0;
            self.view_state.scroll_offset = 0;
            return None;
        }

        if let Some(display_name) = crate::watcher::get_display_name_for_command(&cmd_lower) {
            self.view_state.selected_resource_type = Some(display_name.to_string());
            self.view_state.selected_index = 0;
            self.view_state.scroll_offset = 0;
            self.invalidate_layout_cache(); // Resource type filter affects header display
        }

        None
    }
}
