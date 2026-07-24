//! Event handling for the application
//!
//! This module contains all input handling logic including keyboard events,
//! command mode, filter mode, and confirmation dialogs.

use super::core::App;
use super::state::{HealthFilter, PendingOperation, View};
use crate::tui::commands;
use crate::watcher::ResourceKey;
use crossterm::event::KeyEvent;

/// A `:` command handler dispatched from [`App::execute_command`]. Receives the
/// app and the original (case-preserving) command string.
type CommandHandler = fn(&mut App, &str);

/// Data-driven dispatch for the uniform `(predicate over the lowercased command,
/// handler)` `:` commands. Checked in order; the first matching predicate wins.
/// Special commands (help/quit/readonly, the connection gate) and the
/// resource-type fallback are handled directly in [`App::execute_command`].
const COMMAND_TABLE: &[(fn(&str) -> bool, CommandHandler)] = &[
    (commands::is_skin_command, App::cmd_set_skin),
    (commands::is_trace_command, App::cmd_trace),
    (commands::is_context_command, App::cmd_switch_context),
    (commands::is_namespace_command, App::cmd_switch_namespace),
    (commands::is_healthy_command, App::cmd_filter_healthy),
    (commands::is_unhealthy_command, App::cmd_filter_unhealthy),
    (commands::is_favorites_command, App::cmd_show_favorites),
    (commands::is_events_command, App::cmd_show_events),
    (commands::is_pulse_command, App::cmd_show_pulse),
    (commands::is_logs_command, App::cmd_show_logs),
    (commands::is_all_command, App::cmd_show_all),
];

impl App {
    /// Scroll the active view down by `amount` lines, or advance the list
    /// selection when a list view is active. Shared by j/Down, PageDown and
    /// Ctrl+F so all scroll keys behave identically in every view.
    fn scroll_down(&mut self, amount: usize) {
        let view = self.view_state.current_view;
        // Manual scrolling in the log view pauses following (G resumes).
        if view == View::Logs {
            self.logs.follow = false;
        }
        // In the graph, j/Down/PageDown move keyboard focus between nodes instead
        // of free-scrolling; the renderer scrolls to keep the focused node on screen.
        if view == View::ResourceGraph {
            self.move_graph_focus(true);
        } else if let Some(offset) = view.scroll_offset_mut(&mut self.view_state) {
            *offset += amount;
        } else {
            let max_index = if view == View::EventList {
                self.filtered_kube_events().len().saturating_sub(1)
            } else if view == View::WorkloadList {
                self.view_state.workload_rows.len().saturating_sub(1)
            } else {
                self.get_filtered_resources().len().saturating_sub(1)
            };
            self.view_state.selected_index =
                (self.view_state.selected_index + amount).min(max_index);
        }
    }

    /// Scroll the active view up by `amount` lines, or move the list selection
    /// up when a list view is active (keeping the selection visible).
    fn scroll_up(&mut self, amount: usize) {
        let view = self.view_state.current_view;
        // Manual scrolling in the log view pauses following (G resumes).
        if view == View::Logs {
            self.logs.follow = false;
        }
        if view == View::ResourceGraph {
            self.move_graph_focus(false);
        } else if let Some(offset) = view.scroll_offset_mut(&mut self.view_state) {
            *offset = offset.saturating_sub(amount);
        } else {
            self.view_state.selected_index = self.view_state.selected_index.saturating_sub(amount);
            if self.view_state.selected_index < self.view_state.scroll_offset {
                self.view_state.scroll_offset = self.view_state.selected_index;
            }
        }
    }

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

        // Handle quit confirmation dialog (shown when q/Esc is pressed at top level)
        if self.ui_state.show_quit_confirm {
            return self.handle_quit_confirm_key(key);
        }

        // Handle submenu navigation if a submenu is active
        if self.view_state.submenu_state.is_some() {
            return self.handle_submenu_key(key);
        }

        // Handle connection error state keys
        if self.has_connection_error() {
            // Check status message timeout
            self.check_status_message_timeout();

            // Clear status messages on Esc
            if self.ui_state.status_message.is_some()
                && !self.ui_state.command_mode
                && key.code == crossterm::event::KeyCode::Esc
            {
                self.ui_state.status_message = None;
                self.ui_state.status_message_time = None;
                return None;
            }

            if self.ui_state.command_mode {
                if let Some(should_quit) = self.handle_command_key(key) {
                    return Some(should_quit);
                }
                return None;
            }

            // Only allow quit/Esc, Ctrl+C, :, ? when in connection error state
            match (key.modifiers, key.code) {
                (crossterm::event::KeyModifiers::CONTROL, crossterm::event::KeyCode::Char('c')) => {
                    return Some(true);
                }
                (crossterm::event::KeyModifiers::NONE, crossterm::event::KeyCode::Char(':')) => {
                    self.ui_state.command_mode = true;
                    self.ui_state.command_buffer.clear();
                    return None;
                }
                (crossterm::event::KeyModifiers::NONE, crossterm::event::KeyCode::Char('?')) => {
                    self.ui_state.show_help = !self.ui_state.show_help;
                    return None;
                }
                (
                    crossterm::event::KeyModifiers::NONE,
                    crossterm::event::KeyCode::Char('q') | crossterm::event::KeyCode::Esc,
                ) => {
                    return self.navigate_back_or_confirm_quit();
                }
                _ => {
                    // Clear status messages on any key press (except in command mode/etc)
                    if self.ui_state.status_message.is_some() {
                        self.ui_state.status_message = None;
                        self.ui_state.status_message_time = None;
                    }
                    // Ignore all other keys
                    return None;
                }
            }
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
            (key.modifiers, key.code),
            (
                crossterm::event::KeyModifiers::NONE,
                crossterm::event::KeyCode::Char('s')
                    | crossterm::event::KeyCode::Char('r')
                    | crossterm::event::KeyCode::Char('R')
                    | crossterm::event::KeyCode::Char('W')
                    | crossterm::event::KeyCode::Char('e')
            ) | (
                crossterm::event::KeyModifiers::CONTROL,
                crossterm::event::KeyCode::Char('d')
            )
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

        // Text-view search input (typing the query after pressing / in YAML/describe/trace)
        if self.view_state.text_search.input_mode {
            return self.handle_text_search_key(key);
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
                        self.resource_objects.clear();
                        self.controller_pods.clear();
                        // Restarted watchers start clean; stale degraded/forbidden
                        // state from the old set would otherwise never clear.
                        self.degraded_watchers.clear();
                        self.forbidden_watchers.clear();
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

        // Handle Ctrl+F (page down), Ctrl+B (page up), and Ctrl+C (quit) before main key dispatch.
        // These must be checked here so they don't collide with the plain 'f' / 'b' handlers below.
        //
        // Note: in raw mode the OS no longer converts Ctrl+C into SIGINT — it arrives as a
        // regular key event that the application must handle explicitly.
        if key.modifiers == crossterm::event::KeyModifiers::CONTROL {
            let page_size = self.view_state.page_size;
            match key.code {
                crossterm::event::KeyCode::Char('c') => {
                    return Some(true); // Unconditional quit, matching terminal convention
                }
                crossterm::event::KeyCode::Char('f') => {
                    self.scroll_down(page_size);
                    return None;
                }
                crossterm::event::KeyCode::Char('b') => {
                    self.scroll_up(page_size);
                    return None;
                }
                crossterm::event::KeyCode::Char('d') => {
                    self.handle_operation_key('d');
                    return None;
                }
                _ => {}
            }
        }

        // Handle PageDown / PageUp keys (no modifiers required).
        if key.modifiers == crossterm::event::KeyModifiers::NONE {
            let page_size = self.view_state.page_size;
            match key.code {
                crossterm::event::KeyCode::PageDown => {
                    self.scroll_down(page_size);
                    return None;
                }
                crossterm::event::KeyCode::PageUp => {
                    self.scroll_up(page_size);
                    return None;
                }
                _ => {}
            }
        }

        match key.code {
            crossterm::event::KeyCode::Char('q') => {
                // Navigate back a level, closer to k9s behaviour where q never
                // exits directly. At the top-level view a confirmation dialog is shown
                // instead. Use Q, :q, or Ctrl+C to exit without the dialog.
                return self.navigate_back_or_confirm_quit();
            }
            crossterm::event::KeyCode::Char('Q') => {
                // Immediate unconditional quit (uppercase, intentional).
                // Provides a direct exit for users who do not want the confirmation
                // dialog that q/Esc shows at the top-level view.
                return Some(true);
            }
            crossterm::event::KeyCode::Esc => {
                // In a text view with an active search, Esc clears the search first
                if self.is_text_search_view() && self.view_state.text_search.is_active() {
                    self.view_state.text_search.clear();
                    return None;
                }
                // Navigate back a level, closer to k9s behaviour where Esc never
                // exits directly. At the top-level view a confirmation dialog is shown.
                return self.navigate_back_or_confirm_quit();
            }
            crossterm::event::KeyCode::Char('?') => {
                self.ui_state.show_help = !self.ui_state.show_help;
            }
            crossterm::event::KeyCode::Char('s')
            | crossterm::event::KeyCode::Char('r')
            | crossterm::event::KeyCode::Char('R')
            | crossterm::event::KeyCode::Char('W') => {
                let op_key = match key.code {
                    crossterm::event::KeyCode::Char('s') => 's',
                    crossterm::event::KeyCode::Char('r') => 'r',
                    crossterm::event::KeyCode::Char('R') => 'R',
                    crossterm::event::KeyCode::Char('W') => 'W',
                    _ => return None,
                };
                self.handle_operation_key(op_key);
            }
            crossterm::event::KeyCode::Char('t') => {
                // Trace command - works from list, favorites, and detail view
                if let Some(resource) = self.get_current_resource() {
                    self.async_state.trace.request(ResourceKey::new(
                        resource.resource_type.clone(),
                        resource.namespace.clone(),
                        resource.name.clone(),
                    ));
                    self.view_state.trace_scroll_offset = 0;
                }
            }
            crossterm::event::KeyCode::Char(':') => {
                self.ui_state.command_mode = true;
                self.ui_state.command_buffer.clear();
            }
            // Jump to the newest log line and resume following.
            crossterm::event::KeyCode::Char('G') if self.view_state.current_view == View::Logs => {
                self.logs.follow = true;
            }
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k') => {
                self.scroll_up(1);
            }
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j') => {
                // Max scroll in scrollable views is clamped during render
                self.scroll_down(1);
            }
            crossterm::event::KeyCode::Char('/') => {
                if self.is_text_search_view() {
                    // Search within the current text view (YAML/describe/trace)
                    self.view_state.text_search.clear();
                    self.view_state.text_search.input_mode = true;
                } else {
                    // Enter filter mode
                    self.view_state.filter_mode = true;
                    self.view_state.filter.clear();
                    self.invalidate_layout_cache(); // Filter state affects header height
                }
            }
            // Cycle search matches in text views (vim-style n/N)
            crossterm::event::KeyCode::Char('n')
                if self.is_text_search_view() && self.view_state.text_search.is_active() =>
            {
                self.advance_text_search(1);
            }
            crossterm::event::KeyCode::Char('N')
                if self.is_text_search_view() && self.view_state.text_search.is_active() =>
            {
                self.advance_text_search(-1);
            }
            // Column sorting in list views (k9s-style shift-key sort)
            crossterm::event::KeyCode::Char(c @ ('N' | 'A' | 'T' | 'S'))
                if matches!(
                    self.view_state.current_view,
                    View::ResourceList | View::ResourceFavorites
                ) =>
            {
                use crate::tui::app::state::SortField;
                let field = match c {
                    'N' => SortField::Name,
                    'A' => SortField::Age,
                    'T' => SortField::Type,
                    _ => SortField::Status,
                };
                self.toggle_sort(field);
            }
            crossterm::event::KeyCode::Char('y') => {
                // View YAML - trigger async fetch
                if let Some(key) = self.prepare_selected_resource_key_for_nested_view() {
                    self.async_state.yaml.request(key);
                    self.view_state.yaml_scroll_offset = 0;
                    self.view_state.text_search.clear();
                    self.view_state.current_view = View::ResourceYAML;
                }
            }
            crossterm::event::KeyCode::Char('d') => {
                if let Some(key) = self.prepare_selected_resource_key_for_nested_view() {
                    self.async_state.describe.request(key);
                    self.view_state.describe_scroll_offset = 0;
                    self.view_state.text_search.clear();
                    self.view_state.current_view = View::ResourceDescribe;
                }
            }
            crossterm::event::KeyCode::Char('e') => {
                if self.config.read_only {
                    self.set_status_message((
                        "Editing disabled in read-only mode".to_string(),
                        true,
                    ));
                } else if let Some(resource) = self.get_current_resource() {
                    // Remember the exact view we came from so cancel/save returns there
                    self.async_state.edit_return_view = self.view_state.current_view;
                    // Trigger YAML fetch for the full resource object (via AsyncTask)
                    let rk = ResourceKey::new(
                        resource.resource_type.clone(),
                        resource.namespace.clone(),
                        resource.name.clone(),
                    );
                    self.async_state.yaml.request(rk.clone());
                    self.async_state.edit_pending = Some(rk);
                    self.async_state.edit_full_yaml = None;
                    self.async_state.edit_editor_launched = false;
                    self.async_state.edit_error_message = None;
                    self.view_state.current_view = View::ResourceEdit;
                } else {
                    self.set_status_message(("No resource selected for editing".to_string(), true));
                }
            }
            crossterm::event::KeyCode::Enter
                if self.view_state.current_view == View::ResourceGraph =>
            {
                // Drill into the focused graph node's resource.
                self.navigate_to_focused_graph_node();
            }
            crossterm::event::KeyCode::Enter if self.view_state.current_view == View::EventList => {
                // Jump to the event's involved resource when flux9s watches it.
                self.navigate_to_selected_event_resource();
            }
            crossterm::event::KeyCode::Enter
                if self.view_state.current_view == View::WorkloadList =>
            {
                // Open the selected workload's detail (async fetch).
                if let Some(row) = self
                    .view_state
                    .workload_rows
                    .get(self.view_state.selected_index)
                {
                    self.async_state.workload.request(ResourceKey::new(
                        row.kind.clone(),
                        row.namespace.clone(),
                        row.name.clone(),
                    ));
                    self.logs_after_workload_load = false;
                    self.view_state.workload_scroll_offset = 0;
                    self.view_state.text_search.clear();
                    self.view_state.current_view = View::WorkloadDetail;
                }
            }
            // Stream logs for the detailed workload's pods (#194).
            crossterm::event::KeyCode::Char('l')
                if self.view_state.current_view == View::WorkloadDetail =>
            {
                self.view_state.logs_back_view = Some(View::WorkloadDetail);
                self.open_workload_pod_logs();
            }
            // `l` straight from the workload list: fetch the workload and
            // open its pod logs as soon as the data arrives.
            crossterm::event::KeyCode::Char('l')
                if self.view_state.current_view == View::WorkloadList =>
            {
                if let Some(row) = self
                    .view_state
                    .workload_rows
                    .get(self.view_state.selected_index)
                {
                    self.async_state.workload.request(ResourceKey::new(
                        row.kind.clone(),
                        row.namespace.clone(),
                        row.name.clone(),
                    ));
                    self.logs_after_workload_load = true;
                    self.view_state.logs_back_view = Some(View::WorkloadList);
                    self.view_state.workload_scroll_offset = 0;
                    self.view_state.text_search.clear();
                    self.view_state.current_view = View::WorkloadDetail;
                }
            }
            crossterm::event::KeyCode::Enter if self.view_state.current_view.is_list_view() => {
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
                    // Opened from the list, so Back returns to the list.
                    self.view_state.detail_back_view = None;
                    self.view_state.current_view = View::ResourceDetail;
                }
            }
            // Toggle favorite - works from list view
            crossterm::event::KeyCode::Char('f') if self.view_state.current_view.is_list_view() => {
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
            crossterm::event::KeyCode::Char('h') => {
                // View reconciliation history - works from list, favorites, and detail view
                if let Some(resource) = self.get_current_resource() {
                    use crate::models::FluxResourceKind;

                    let key = crate::watcher::resource_key(
                        &resource.namespace,
                        &resource.name,
                        &resource.resource_type,
                    );

                    // Check if resource object exists and has status.history
                    let obj = self.resource_objects.get(&key);
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
                if let Some(resource) = self.get_current_resource() {
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
                    if self.view_state.current_view.is_list_view()
                        || self.view_state.current_view == View::EventList
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
                    self.async_state.graph.request(ResourceKey {
                        resource_type: resource.resource_type.clone(),
                        namespace: resource.namespace.clone(),
                        name: resource.name.clone(),
                    });
                    self.view_state.graph_scroll_offset = 0; // Reset scroll
                    self.view_state.graph_focus_index = None; // Reset focus (set when graph loads)
                    self.view_state.current_view = View::ResourceGraph;
                } else {
                    self.set_status_message(("No resource selected".to_string(), true));
                }
            }
            crossterm::event::KeyCode::Backspace => {
                // Backspace goes back (same as Escape for detail view)
                if self.view_state.current_view == View::ResourceEdit {
                    // Cancel edit — clear all edit state
                    self.async_state.edit_pending = None;
                    self.async_state.edit_full_yaml = None;
                    self.async_state.edit_save_pending = None;
                    self.async_state.edit_save_result_rx = None;
                    self.async_state.edit_error_message = None;
                    self.async_state.edit_editor_launched = false;
                    self.view_state.current_view = self.async_state.edit_return_view;
                    self.async_state.edit_return_view = View::ResourceList;
                } else if self.view_state.current_view.is_nested_view() {
                    // Mirror Esc: return to the graph if we came from there,
                    // otherwise to the previous list view.
                    if let Some(back) = self.detail_graph_back() {
                        self.view_state.current_view = back;
                    } else {
                        self.view_state.current_view = self.view_state.previous_list_view;
                        self.selection_state.selected_resource_key = None;
                        self.view_state.text_search.clear();
                    }
                } else if self.view_state.current_view == View::ResourceFavorites {
                    self.view_state.current_view = View::ResourceList;
                    self.selection_state.selected_resource_key = None;
                } else if self.view_state.current_view == View::EventList {
                    self.stop_kube_events_watch();
                    self.view_state.current_view = View::ResourceList;
                    self.selection_state.selected_resource_key = None;
                } else if self.view_state.current_view == View::Logs {
                    self.logs.stop();
                    self.view_state.text_search.clear();
                    self.view_state.current_view = self
                        .view_state
                        .logs_back_view
                        .take()
                        .unwrap_or(self.view_state.previous_list_view);
                } else if self.view_state.current_view == View::WorkloadList {
                    self.view_state.current_view = View::ResourceGraph;
                } else if self.view_state.current_view == View::WorkloadDetail {
                    self.async_state.workload.clear();
                    self.logs_after_workload_load = false;
                    self.view_state.text_search.clear();
                    self.view_state.current_view = View::WorkloadList;
                } else if self.view_state.current_view == View::Pulse {
                    self.view_state.text_search.clear();
                    self.view_state.current_view = View::ResourceList;
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

    /// Whether the current view supports text search (`/`)
    fn is_text_search_view(&self) -> bool {
        self.view_state.current_view.is_text_search_view()
    }

    /// Handle a key press while typing a text-view search query
    fn handle_text_search_key(&mut self, key: KeyEvent) -> Option<bool> {
        match key.code {
            crossterm::event::KeyCode::Esc => {
                self.view_state.text_search.clear();
            }
            crossterm::event::KeyCode::Enter => {
                let search = &mut self.view_state.text_search;
                search.input_mode = false;
                if search.is_active() {
                    search.current_match = 0;
                    search.pending_jump = true;
                } else {
                    search.clear();
                }
            }
            crossterm::event::KeyCode::Backspace => {
                self.view_state.text_search.query.pop();
            }
            crossterm::event::KeyCode::Char(c) => {
                self.view_state.text_search.query.push(c);
            }
            _ => {}
        }
        None
    }

    /// Move to the next (+1) or previous (-1) search match, wrapping around
    fn advance_text_search(&mut self, delta: isize) {
        let search = &mut self.view_state.text_search;
        if search.total_matches == 0 {
            return;
        }
        let total = search.total_matches as isize;
        search.current_match = (search.current_match as isize + delta).rem_euclid(total) as usize;
        search.pending_jump = true;
    }

    fn handle_submenu_key(&mut self, key: KeyEvent) -> Option<bool> {
        if let Some(ref mut submenu) = self.view_state.submenu_state {
            // Filter input mode mirrors the resource-list filter: `/` enters,
            // typing edits, Enter applies (keeps the filter), Esc cancels.
            if submenu.filter_mode {
                match key.code {
                    crossterm::event::KeyCode::Esc => {
                        submenu.clear_filter();
                        self.preview_theme_in_submenu();
                    }
                    crossterm::event::KeyCode::Enter => {
                        submenu.filter_mode = false;
                    }
                    crossterm::event::KeyCode::Backspace => {
                        submenu.pop_filter_char();
                        self.preview_theme_in_submenu();
                    }
                    crossterm::event::KeyCode::Char(c)
                        if !key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL) =>
                    {
                        submenu.push_filter_char(c);
                        self.preview_theme_in_submenu();
                    }
                    _ => {}
                }
                return None;
            }

            match key.code {
                // Enter filter mode, same as the resource list (#128)
                crossterm::event::KeyCode::Char('/') => {
                    submenu.filter_mode = true;
                }
                // `:` falls through to command mode: close the submenu
                // (restoring any theme preview) and start typing a command.
                crossterm::event::KeyCode::Char(':') => {
                    if submenu.command == "skin" {
                        if let Some(original_theme) = self.view_state.preview_original_theme.clone()
                        {
                            let _ = self.set_theme(&original_theme);
                        }
                    }
                    self.view_state.submenu_state = None;
                    self.view_state.preview_original_theme = None;
                    self.ui_state.command_mode = true;
                    self.ui_state.command_buffer.clear();
                }
                // Scroll is reconciled at render time from the popup's actual
                // height, so navigation only moves the selection here.
                crossterm::event::KeyCode::Char('j') | crossterm::event::KeyCode::Down => {
                    submenu.move_down(1);
                    // Preview theme if this is a skin submenu
                    self.preview_theme_in_submenu();
                }
                crossterm::event::KeyCode::Char('k') | crossterm::event::KeyCode::Up => {
                    submenu.move_up(1);
                    // Preview theme if this is a skin submenu
                    self.preview_theme_in_submenu();
                }
                // Page through long menus (#128)
                crossterm::event::KeyCode::PageDown => {
                    submenu.move_down(crate::constants::SUBMENU_PAGE_JUMP);
                    self.preview_theme_in_submenu();
                }
                crossterm::event::KeyCode::PageUp => {
                    submenu.move_up(crate::constants::SUBMENU_PAGE_JUMP);
                    self.preview_theme_in_submenu();
                }
                crossterm::event::KeyCode::Char('f')
                    if key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL) =>
                {
                    submenu.move_down(crate::constants::SUBMENU_PAGE_JUMP);
                    self.preview_theme_in_submenu();
                }
                crossterm::event::KeyCode::Char('b')
                    if key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL) =>
                {
                    submenu.move_up(crate::constants::SUBMENU_PAGE_JUMP);
                    self.preview_theme_in_submenu();
                }
                // Save/persist current selection (for skin submenu).
                // Filter typing happens in filter mode, so 's' is free here.
                crossterm::event::KeyCode::Char('s') | crossterm::event::KeyCode::Char('S')
                    if submenu.command == "skin" =>
                {
                    if let Some(value) = submenu.selected_value() {
                        match self.persist_theme(&value) {
                            Ok(_) => {
                                // Close submenu and clear preview
                                self.view_state.submenu_state = None;
                                self.view_state.preview_original_theme = None;
                                let readonly_msg = if self.config.read_only {
                                    " (readonly mode)"
                                } else {
                                    ""
                                };
                                let msg =
                                    format!("Theme '{}' saved to config{}", value, readonly_msg);
                                self.set_status_message((msg, false));
                            }
                            Err(e) => {
                                let msg = format!("Failed to save theme: {}", e);
                                self.set_status_message((msg, true));
                            }
                        }
                    }
                }
                crossterm::event::KeyCode::Enter => {
                    // Select the current item and execute the command
                    if let Some(value) = submenu.selected_value() {
                        let command = submenu.command.clone();
                        // Close submenu and clear preview
                        self.view_state.submenu_state = None;
                        self.view_state.preview_original_theme = None;
                        // Execute the command with the selected value
                        // For context command, trigger context switch
                        if command == "ctx" {
                            self.pending_context_switch = Some(value.clone());
                            self.set_status_message((
                                format!("Switching to context '{}'...", value),
                                false,
                            ));
                        } else if command == "ns" {
                            // Namespace picker: "all" watches every namespace.
                            let new_namespace = if value == "all" {
                                None
                            } else {
                                Some(value.clone())
                            };
                            self.switch_namespace(new_namespace);
                        } else if command == "logs" {
                            self.open_log_view(&value);
                        } else if command == "pod-logs" {
                            // Workload pod logs: the value is "namespace/pod".
                            if let Some((namespace, pod)) = value.split_once('/') {
                                let (namespace, pod) = (namespace.to_string(), pod.to_string());
                                self.open_pod_logs(&namespace, &pod);
                            }
                        } else if command == "skin" {
                            // Change theme (already previewed, so just confirm)
                            match self.set_theme(&value) {
                                Ok(_) => {
                                    let msg = format!("Theme changed to: {}", value);
                                    self.set_status_message((msg, false));
                                }
                                Err(e) => {
                                    let msg = format!("Failed to load theme '{}': {}", value, e);
                                    self.set_status_message((msg, true));
                                }
                            }
                        }
                    }
                }
                crossterm::event::KeyCode::Esc => {
                    // An active type-ahead filter clears first; a second Esc
                    // cancels the submenu (restoring the theme if previewing).
                    if submenu.clear_filter() {
                        self.preview_theme_in_submenu();
                        return None;
                    }
                    if submenu.command == "skin" {
                        if let Some(original_theme) = self.view_state.preview_original_theme.clone()
                        {
                            let _ = self.set_theme(&original_theme);
                        }
                    }
                    self.view_state.submenu_state = None;
                    self.view_state.preview_original_theme = None;
                }
                _ => {}
            }
        }
        None
    }

    /// Preview theme when navigating skin submenu
    fn preview_theme_in_submenu(&mut self) {
        if let Some(ref submenu) = self.view_state.submenu_state {
            if submenu.command == "skin" {
                if let Some(theme_name) = submenu.selected_value() {
                    // Preview the theme (don't show errors, just silently fail)
                    let _ = self.preview_theme(&theme_name);
                }
            }
        }
    }

    /// Navigate back one level, or show the quit confirmation dialog at the top level.
    ///
    /// Shared implementation for `q` and `Esc`, matching k9s behaviour where
    /// neither key exits the application directly. The help overlay is treated as
    /// a navigable layer and is dismissed first before any view transition occurs.
    /// Move keyboard focus between graph nodes in visual (top-to-bottom) order.
    /// Clamps at the ends rather than wrapping so the direction stays intuitive.
    /// Auto-scrolling to keep the focused node visible is handled by the renderer.
    fn move_graph_focus(&mut self, forward: bool) {
        let Some(graph) = self.async_state.graph.result() else {
            return;
        };
        let order = graph.focus_order();
        if order.is_empty() {
            return;
        }

        // Where the current focus sits within the visual order (start by default).
        let current_pos = self
            .view_state
            .graph_focus_index
            .and_then(|idx| order.iter().position(|&i| i == idx));

        let next_pos = match current_pos {
            Some(pos) if forward => (pos + 1).min(order.len() - 1),
            Some(pos) => pos.saturating_sub(1),
            None => 0,
        };
        self.view_state.graph_focus_index = Some(order[next_pos]);
    }

    /// Identity of the keyboard-focused graph node when it is an individual
    /// resource. Aggregate nodes (workload/resource groups) and external
    /// upstream URLs have no single resource to act on, so they resolve to
    /// `None`.
    pub(crate) fn focused_graph_node_target(&self) -> Option<ResourceKey> {
        use crate::trace::NodeType;
        let node = self.async_state.graph.result().and_then(|graph| {
            self.view_state
                .graph_focus_index
                .and_then(|idx| graph.nodes.get(idx))
        })?;
        if matches!(
            node.node_type,
            NodeType::WorkloadGroup | NodeType::ResourceGroup | NodeType::Upstream
        ) {
            return None;
        }
        Some(ResourceKey::new(
            node.kind.clone(),
            node.namespace.clone(),
            node.name.clone(),
        ))
    }

    /// Open the detail view for the focused graph node when it maps to a watched
    /// resource. Aggregate nodes (workload/resource groups) and external upstream
    /// URLs are not directly navigable and just show a hint instead.
    fn navigate_to_focused_graph_node(&mut self) {
        let focused_node = self
            .async_state
            .graph
            .result()
            .and_then(|graph| {
                self.view_state
                    .graph_focus_index
                    .and_then(|idx| graph.nodes.get(idx))
            })
            .map(|node| (node.node_type, node.description.clone()));
        let Some(rk) = self.focused_graph_node_target() else {
            match focused_node {
                // The workload group drills into the workload list (#194):
                // its description carries one encoded WorkloadRef per line.
                Some((crate::trace::NodeType::WorkloadGroup, description)) => {
                    let rows: Vec<_> = description
                        .as_deref()
                        .unwrap_or_default()
                        .lines()
                        .filter_map(crate::kube::workloads::WorkloadRef::parse_graph_line)
                        .collect();
                    if rows.is_empty() {
                        self.set_status_message(("No workloads in this group".to_string(), false));
                        return;
                    }
                    self.view_state.workload_rows = rows;
                    self.view_state.selected_index = 0;
                    self.view_state.scroll_offset = 0;
                    self.view_state.current_view = View::WorkloadList;
                }
                Some(_) => {
                    self.set_status_message((
                        "Aggregate node — select an individual Flux resource to open it"
                            .to_string(),
                        false,
                    ));
                }
                None => {}
            }
            return;
        };

        let key = rk.to_key_string();
        if self.state.get(&key).is_some() {
            self.selection_state.selected_resource_key = Some(key);
            // Remember to return to the graph (not the list) when the user backs
            // out of the detail view.
            self.view_state.detail_back_view = Some(View::ResourceGraph);
            self.view_state.current_view = View::ResourceDetail;
        } else {
            self.set_status_message((
                format!(
                    "{} {} is not in the current view",
                    rk.resource_type, rk.name
                ),
                false,
            ));
        }
    }

    /// If the current nested view was entered by drilling into a graph node
    /// (detail via Enter, or YAML/describe/etc. directly on the focused node),
    /// consume and return the stored back target (the graph). Returns `None`
    /// for any other entry path, leaving normal back-to-list behaviour in place.
    fn detail_graph_back(&mut self) -> Option<View> {
        if self.view_state.current_view.is_nested_view()
            && self.view_state.current_view != View::ResourceGraph
        {
            self.view_state.detail_back_view.take()
        } else {
            None
        }
    }

    fn navigate_back_or_confirm_quit(&mut self) -> Option<bool> {
        if self.ui_state.show_help {
            self.ui_state.show_help = false;
            return None;
        }
        match self.view_state.current_view {
            View::ResourceList => {
                // At the top-level view there is nowhere to go back to, so ask
                // for confirmation rather than exiting immediately (k9s convention).
                self.ui_state.show_quit_confirm = true;
                None
            }
            View::ResourceDetail
            | View::ResourceDescribe
            | View::ResourceYAML
            | View::ResourceTrace
            | View::ResourceHistory
            | View::ResourceGraph => {
                // If we drilled into this detail view from the graph, return to
                // the graph; otherwise go back to the previous list view
                // (favourites if we came from there, else the main resource list).
                if let Some(back) = self.detail_graph_back() {
                    self.view_state.current_view = back;
                } else {
                    self.view_state.current_view = self.view_state.previous_list_view;
                    self.selection_state.selected_resource_key = None;
                    self.view_state.text_search.clear();
                }
                None
            }
            View::ResourceEdit => {
                // Cancel edit — clear all edit state and return to origin view
                self.async_state.edit_pending = None;
                self.async_state.edit_full_yaml = None;
                self.async_state.edit_save_pending = None;
                self.async_state.edit_save_result_rx = None;
                self.async_state.edit_error_message = None;
                self.async_state.edit_editor_launched = false;
                self.view_state.current_view = self.async_state.edit_return_view;
                self.async_state.edit_return_view = View::ResourceList;
                None
            }
            View::ResourceFavorites => {
                self.view_state.current_view = View::ResourceList;
                None
            }
            View::EventList => {
                self.stop_kube_events_watch();
                self.view_state.current_view = View::ResourceList;
                None
            }
            View::Logs => {
                // Stop the stream; return to wherever logs were opened from.
                self.logs.stop();
                self.view_state.text_search.clear();
                self.view_state.current_view = self
                    .view_state
                    .logs_back_view
                    .take()
                    .unwrap_or(self.view_state.previous_list_view);
                None
            }
            View::WorkloadList => {
                // Entered from a graph workload group — return to the graph.
                self.view_state.current_view = View::ResourceGraph;
                None
            }
            View::WorkloadDetail => {
                self.async_state.workload.clear();
                self.logs_after_workload_load = false;
                self.view_state.text_search.clear();
                self.view_state.current_view = View::WorkloadList;
                None
            }
            View::Pulse => {
                self.view_state.text_search.clear();
                self.view_state.current_view = View::ResourceList;
                None
            }
            View::Help => {
                self.view_state.current_view = View::ResourceList;
                None
            }
        }
    }

    /// Handle a key press while the quit confirmation dialog is visible.
    ///
    /// `y`/`Y` confirms and exits. `n`/`N`/`q`/`Esc` all cancel — `q` is
    /// included so the footer hint is consistent with what actually works.
    /// All other keys are ignored while the dialog is open.
    fn handle_quit_confirm_key(&mut self, key: KeyEvent) -> Option<bool> {
        match key.code {
            crossterm::event::KeyCode::Char('y') | crossterm::event::KeyCode::Char('Y') => {
                Some(true) // Confirmed — exit the application
            }
            crossterm::event::KeyCode::Char('n')
            | crossterm::event::KeyCode::Char('N')
            | crossterm::event::KeyCode::Char('q')
            | crossterm::event::KeyCode::Esc => {
                self.ui_state.show_quit_confirm = false;
                None // Cancelled — return to normal view
            }
            _ => None, // Ignore all other keys while the dialog is open
        }
    }

    /// Open the detail view for the resource the selected event is about,
    /// when it is a Flux resource flux9s is watching. Back returns to the
    /// events feed (the events watcher keeps running meanwhile).
    fn navigate_to_selected_event_resource(&mut self) {
        let events = self.filtered_kube_events();
        let Some(event) = events.get(self.view_state.selected_index) else {
            return;
        };

        let key = crate::watcher::resource_key(
            &event.involved_namespace,
            &event.involved_name,
            &event.involved_kind,
        );
        if self.state.get(&key).is_none() {
            // Not in the watch state: outside the namespace scope, a non-Flux
            // kind, or its watcher isn't running. Name the namespace so a
            // scope mismatch is visible, and point at the keys that still work.
            self.set_status_message((
                format!(
                    "{} (ns: {}) is not in the watched resources — press y/d to view it",
                    event.object_label(),
                    event.involved_namespace
                ),
                false,
            ));
            return;
        }

        self.view_state.previous_list_view = View::EventList;
        self.view_state.detail_back_view = None;
        self.selection_state.selected_resource_key = Some(key);
        self.view_state.current_view = View::ResourceDetail;
    }

    /// Stream logs for the detailed workload's pods: directly when there is
    /// exactly one pod, via a pod submenu when there are several.
    pub(crate) fn open_workload_pod_logs(&mut self) {
        let Some(workload) = self.async_state.workload.result() else {
            return; // Still loading
        };
        match workload.pods.as_slice() {
            [] => {
                self.set_status_message((
                    format!("{}/{} has no pods to stream", workload.kind, workload.name),
                    false,
                ));
            }
            [only] => {
                let (namespace, pod) = (workload.namespace.clone(), only.name.clone());
                self.open_pod_logs(&namespace, &pod);
            }
            pods => {
                let items: Vec<crate::tui::submenu::SubmenuItem> = pods
                    .iter()
                    .map(|pod| {
                        crate::tui::submenu::SubmenuItem::with_display(
                            format!("{}/{}", workload.namespace, pod.name),
                            format!("{} ({}, ready {})", pod.name, pod.phase, pod.ready),
                        )
                    })
                    .collect();
                self.view_state.submenu_state = Some(
                    crate::tui::submenu::SubmenuState::new("pod-logs".to_string(), items)
                        .with_title("Pod Logs".to_string())
                        .with_help(
                            "j/k: Navigate | /: Filter | Enter: Stream logs | Esc: Cancel"
                                .to_string(),
                        ),
                );
            }
        }
    }

    fn prepare_selected_resource_key_for_nested_view(&mut self) -> Option<ResourceKey> {
        let rk = self.view_target()?;
        match self.view_state.current_view {
            // Root list-style views: remember where Back should return to and
            // drop any stale graph back-target from an earlier drill-down.
            View::ResourceList | View::ResourceFavorites | View::EventList => {
                self.view_state.previous_list_view = self.view_state.current_view;
                self.view_state.detail_back_view = None;
            }
            // Drilling out of the graph: Back returns to the graph.
            View::ResourceGraph => {
                self.view_state.detail_back_view = Some(View::ResourceGraph);
            }
            _ => {}
        }
        self.selection_state.selected_resource_key = Some(rk.to_key_string());
        Some(rk)
    }

    fn handle_operation_key(&mut self, op_key: char) {
        if let Some(resource) = self.get_current_resource() {
            if let Some(operation) = self.operation_registry.get_by_keybinding(op_key) {
                if !operation.is_valid_for(&resource.resource_type) {
                    self.set_status_message((
                        format!(
                            "Operation '{}' is not valid for {}",
                            operation.name(),
                            resource.resource_type
                        ),
                        true,
                    ));
                    return;
                }

                if self.config.read_only {
                    self.set_status_message((
                        crate::constants::READ_ONLY_WRITE_ACTION_MESSAGE.to_string(),
                        true,
                    ));
                    return;
                }

                if operation.requires_confirmation() {
                    self.async_state.confirmation_pending = Some(PendingOperation::new(
                        resource.resource_type.clone(),
                        resource.namespace.clone(),
                        resource.name.clone(),
                        op_key,
                    ));
                    return;
                }

                let feedback_msg = if op_key == 'W' {
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
                self.execute_operation(
                    &resource.resource_type,
                    &resource.namespace,
                    &resource.name,
                    op_key,
                );
            }
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
                            crate::constants::READ_ONLY_WRITE_ACTION_MESSAGE.to_string(),
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
                crate::constants::READ_ONLY_WRITE_ACTION_MESSAGE.to_string(),
                true,
            ));
            return;
        }

        if self.operation_registry.get_by_keybinding(op_key).is_some() && self.kube_client.is_some()
        {
            // Mark operation as pending - will be executed in main loop
            self.async_state.operation.request(PendingOperation::new(
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

    /// Execute the command currently in the command buffer.
    ///
    /// A few commands are special and handled up front: help/quit toggle global
    /// state, readonly runs before the connection gate, and the gate blocks most
    /// commands while disconnected. Everything else is dispatched through
    /// [`COMMAND_TABLE`] (a data-driven `(predicate, handler)` list); unmatched
    /// input falls through to resource-type selection or an "unknown command"
    /// message. Returns `Some(true)` only to quit.
    fn execute_command(&mut self) -> Option<bool> {
        // Own the command string so the per-command handlers can take `&mut self`
        // without conflicting with a borrow of the command buffer.
        let cmd = self.ui_state.command_buffer.trim().to_string();
        let cmd_lower = cmd.to_lowercase();

        if commands::is_help_command(&cmd_lower) {
            self.ui_state.show_help = !self.ui_state.show_help;
            return None;
        }
        if commands::is_quit_command(&cmd_lower) {
            return Some(true);
        }
        if commands::is_readonly_command(&cmd_lower) {
            self.cmd_toggle_readonly();
            return None;
        }

        // While disconnected, only context/skin commands are allowed through.
        if self.has_connection_error()
            && !commands::is_context_command(&cmd_lower)
            && !commands::is_skin_command(&cmd_lower)
        {
            self.set_status_message((
                "Commands are disabled when disconnected (except :ctx, :skin, :q)".to_string(),
                true,
            ));
            return None;
        }

        // Data-driven dispatch: first predicate to match owns the command.
        for (matches, handle) in COMMAND_TABLE {
            if matches(&cmd_lower) {
                handle(self, &cmd);
                return None;
            }
        }

        // Fallback: a resource-type command (e.g. `:ks`, `:hr`), else a
        // dynamically discovered kind (#197), else unknown.
        if let Some(display_name) = crate::watcher::get_display_name_for_command(&cmd_lower) {
            self.view_state.selected_resource_type = Some(display_name.to_string());
            self.reset_list_position();
            self.invalidate_layout_cache(); // Resource type filter affects header display
        } else if let Some(kind) = crate::models::extra_kinds::global().resolve_command(&cmd_lower)
        {
            self.view_state.selected_resource_type = Some(kind);
            self.reset_list_position();
            self.invalidate_layout_cache();
        } else if !cmd.is_empty() {
            self.set_status_message((
                format!(
                    "Unknown command: '{}'. Type :help for available commands",
                    cmd
                ),
                true,
            ));
        }

        None
    }

    /// Reset the list selection and scroll to the top. Shared by the commands
    /// that change what the list shows (namespace, filters, resource type).
    fn reset_list_position(&mut self) {
        self.view_state.selected_index = 0;
        self.view_state.scroll_offset = 0;
    }

    /// Toggle read-only mode and reload the matching skin.
    fn cmd_toggle_readonly(&mut self) {
        self.config.read_only = !self.config.read_only;
        let status = if self.config.read_only {
            "enabled"
        } else {
            "disabled"
        };
        // Clone context name to avoid borrowing self across the reload.
        let context_name = self.context.clone();
        self.reload_skin_for_readonly_mode(Some(&context_name));
        self.set_status_message((format!("Readonly mode {}", status), false));
    }

    /// Open the interactive submenu for `cmd` if it has one (contexts, skins, …),
    /// previewing the first skin entry. Returns whether a submenu was opened, so
    /// callers can fall back to a status-message listing when it wasn't.
    fn try_open_command_submenu(&mut self, cmd: &str) -> bool {
        let current_theme = self.config.resolve_skin_name(Some(&self.context));
        let Some(submenu) =
            crate::tui::commands::get_command_submenu(cmd, &self.context, &current_theme)
        else {
            return false;
        };
        // Skin submenus preview as the user scrolls; remember the original to
        // restore on cancel, and preview the first entry immediately.
        if submenu.command == "skin" {
            self.view_state.preview_original_theme = Some(current_theme.clone());
            if let Some(first_theme) = submenu.selected_value() {
                let _ = self.preview_theme(&first_theme);
            }
        }
        self.view_state.submenu_state = Some(submenu);
        true
    }

    /// `:skin [name]` — change the theme, or open the skin submenu / list themes.
    fn cmd_set_skin(&mut self, cmd: &str) {
        match crate::tui::commands::extract_command_arg(cmd, "skin") {
            Some(name) => match self.set_theme(&name) {
                Ok(_) => self.set_status_message((format!("Theme changed to: {}", name), false)),
                Err(e) => self.set_status_message((
                    format!(
                        "Failed to load theme '{}': {}. Use `default` to return to default theme",
                        name, e
                    ),
                    true,
                )),
            },
            None if !self.try_open_command_submenu(cmd) => {
                let themes = crate::config::theme_loader::ThemeLoader::list_themes();
                let current = self.config.resolve_skin_name(Some(&self.context));
                self.set_status_message((
                    format!(
                        "Available themes: {}. Current: {}. Usage: :skin <theme-name>",
                        themes.join(", "),
                        current
                    ),
                    false,
                ));
            }
            None => {}
        }
    }

    /// `:trace [type/name]` — trace the ownership chain of the given (or selected)
    /// resource.
    fn cmd_trace(&mut self, cmd: &str) {
        let Some(trace_arg) = crate::tui::commands::extract_command_arg(cmd, "trace") else {
            // No argument: trace the currently selected resource.
            if let Some(key) = &self.selection_state.selected_resource_key {
                if let Some(rk) = ResourceKey::parse(key) {
                    self.async_state.trace.request(rk);
                } else {
                    tracing::warn!("Failed to parse resource key for trace command: {}", key);
                    self.ui_state.status_message =
                        Some(("Invalid resource key format".to_string(), true));
                }
            } else {
                self.set_status_message(("No resource selected".to_string(), true));
            }
            return;
        };

        // Parse "<type>/<name>" (e.g. "kustomization/cabot-book").
        let parts: Vec<&str> = trace_arg.split('/').collect();
        if parts.len() == 2 {
            use crate::models::FluxResourceKind;
            // Normalize the resource type to its canonical kind name.
            let lowered = parts[0].to_lowercase();
            let resource_type = match FluxResourceKind::from_str_case_insensitive(parts[0]) {
                Some(kind) => kind.as_str(),
                None => match lowered.as_str() {
                    "deployment" | "deploy" => "Deployment",
                    "service" => "Service",
                    "pod" => "Pod",
                    _ => parts[0],
                },
            };
            let namespace = self
                .namespace()
                .clone()
                .unwrap_or_else(|| "default".to_string());
            self.async_state.trace.request(ResourceKey::new(
                resource_type.to_string(),
                namespace,
                parts[1].to_string(),
            ));
        } else {
            self.set_status_message((
                "Usage: :trace <resource-type>/<name> or :trace (for selected)".to_string(),
                true,
            ));
        }
    }

    /// `:ctx [name]` — switch kube context, or open the context submenu / list
    /// available contexts.
    fn cmd_switch_context(&mut self, cmd: &str) {
        let context_name = commands::extract_command_arg(cmd, "context")
            .or_else(|| commands::extract_command_arg(cmd, "ctx"));

        let Some(ctx) = context_name else {
            // No argument: open the submenu, or list contexts as a fallback.
            if !self.try_open_command_submenu(cmd) {
                match crate::kube::list_contexts() {
                    Ok(contexts) => {
                        let current = self.context.clone();
                        self.set_status_message((
                            format!(
                                "Available contexts: {}. Current: {}. Usage: :ctx <context-name>",
                                contexts.join(", "),
                                current
                            ),
                            false,
                        ));
                    }
                    Err(e) => {
                        self.set_status_message((format!("Failed to list contexts: {}", e), true))
                    }
                }
            }
            return;
        };

        // Mark the switch as pending; the main loop performs the reconnect.
        self.pending_context_switch = Some(ctx.to_string());
        self.set_status_message((format!("Switching to context '{}'...", ctx), false));
    }

    /// `:ns [name|all]` — switch the watched namespace, restarting watchers.
    ///
    /// Without an argument, opens the searchable namespace picker submenu;
    /// `:ns <name>` (or `:ns all` / `:ns -A`) switches directly.
    fn cmd_switch_namespace(&mut self, cmd: &str) {
        let ns = commands::extract_command_arg(cmd, "namespace")
            .or_else(|| commands::extract_command_arg(cmd, "ns"));
        match ns.as_deref() {
            Some("all") | Some("-A") => self.switch_namespace(None),
            Some(name) => self.switch_namespace(Some(name.to_string())),
            None => {
                // No argument: open the searchable picker (same reusable submenu
                // as :ctx / :skin) instead of listing options in the bars.
                let options = self.namespace_picker_options();
                match commands::namespace_submenu(&options, &self.namespace) {
                    Some(submenu) => self.view_state.submenu_state = Some(submenu),
                    None => {
                        self.set_status_message(("No namespaces discovered yet".to_string(), true))
                    }
                }
            }
        }
    }

    /// Switch the watched namespace, restarting watchers. `None` watches all
    /// namespaces. Shared by `:ns <name>` and the namespace picker submenu.
    fn switch_namespace(&mut self, new_namespace: Option<String>) {
        if self.namespace != new_namespace {
            self.namespace = new_namespace.clone();

            // Clear state; the restarted watchers repopulate it. Stale degraded
            // state from the old watcher set would otherwise never clear.
            self.state().clear();
            self.resource_objects.clear();
            self.controller_pods.clear();
            self.kube_events.clear();
            self.degraded_watchers.clear();
            self.forbidden_watchers.clear();

            if let Some(ref mut watcher) = self.watcher {
                if let Err(e) = watcher.set_namespace(new_namespace) {
                    tracing::warn!("Failed to switch namespace: {}", e);
                    self.set_status_message((format!("Failed to switch namespace: {}", e), true));
                }
            }
        }

        self.reset_list_position();
    }

    /// `:healthy` — filter the list to healthy resources.
    fn cmd_filter_healthy(&mut self, _cmd: &str) {
        self.view_state.health_filter = HealthFilter::Healthy;
        self.reset_list_position();
        self.set_status_message(("Showing healthy resources only".to_string(), false));
    }

    /// `:unhealthy` — filter the list to unhealthy resources.
    fn cmd_filter_unhealthy(&mut self, _cmd: &str) {
        self.view_state.health_filter = HealthFilter::Unhealthy;
        self.reset_list_position();
        self.set_status_message(("Showing unhealthy resources only".to_string(), false));
    }

    /// `:favorites` — switch to the favorites view.
    fn cmd_show_favorites(&mut self, _cmd: &str) {
        self.view_state.current_view = View::ResourceFavorites;
        self.reset_list_position();
    }

    /// `:events` — open the live Kubernetes events feed for the current
    /// namespace scope, lazily starting the events watcher.
    fn cmd_show_events(&mut self, _cmd: &str) {
        self.start_kube_events_watch();
        self.view_state.current_view = View::EventList;
        self.reset_list_position();
    }

    /// `:pulse` — open the cluster pulse dashboard.
    fn cmd_show_pulse(&mut self, _cmd: &str) {
        self.view_state.pulse_scroll_offset = 0;
        self.view_state.text_search.clear();
        self.view_state.current_view = View::Pulse;
    }

    /// `:logs [pod]` — stream a Flux controller pod's logs. Without an
    /// argument, opens a submenu of the discovered controller pods; with one,
    /// matches a pod by exact name or unique prefix.
    fn cmd_show_logs(&mut self, cmd: &str) {
        let arg = commands::extract_command_arg(cmd, "logs")
            .or_else(|| commands::extract_command_arg(cmd, "log"));
        let pods = self.controller_pods.get_all_pods();

        let Some(prefix) = arg else {
            match commands::logs_submenu(&pods, self.config.ui.no_icons) {
                Some(submenu) => self.view_state.submenu_state = Some(submenu),
                None => {
                    self.set_status_message(("No controller pods discovered yet".to_string(), true))
                }
            }
            return;
        };

        let matched = pods
            .iter()
            .find(|pod| pod.name == prefix)
            .map(|pod| pod.name.clone())
            .or_else(|| {
                let mut prefixed = pods.iter().filter(|pod| pod.name.starts_with(&prefix));
                match (prefixed.next(), prefixed.next()) {
                    (Some(only), None) => Some(only.name.clone()),
                    _ => None, // no match, or ambiguous prefix
                }
            });
        match matched {
            Some(pod) => self.open_log_view(&pod),
            None => {
                self.set_status_message((format!("No controller pod matching '{}'", prefix), true))
            }
        }
    }

    /// `:all` — clear resource-type and health filters and return to the main
    /// resource list (also from the favorites and events views).
    fn cmd_show_all(&mut self, _cmd: &str) {
        if self.view_state.current_view == View::ResourceFavorites {
            self.view_state.current_view = View::ResourceList;
        }
        if self.view_state.current_view == View::EventList {
            self.stop_kube_events_watch();
            self.view_state.current_view = View::ResourceList;
        }
        if self.view_state.selected_resource_type.is_some() {
            self.view_state.selected_resource_type = None;
            self.invalidate_layout_cache(); // Resource type filter affects header display
        }
        if self.view_state.health_filter != HealthFilter::All {
            self.view_state.health_filter = HealthFilter::All;
            self.set_status_message(("Showing all resources".to_string(), false));
        }
        self.reset_list_position();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, UiConfig};
    use crate::tui::Theme;
    use crate::watcher::{ResourceInfo, ResourceState, resource_key};
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
    use std::collections::HashMap;

    fn make_key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn make_ctrl_key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn create_test_app(read_only: bool) -> App {
        let state = ResourceState::new();
        let config = Config {
            read_only,
            default_namespace: "".to_string(),
            default_controller_namespace: "".to_string(),
            discover_flux_resources: false,
            namespace_hotkeys: vec![],
            ui: UiConfig {
                enable_mouse: false,
                headless: false,
                no_icons: false,
                skin: "default".to_string(),
                skin_read_only: None,
                splashless: true,
                rbac_warnings: true,
            },
            context_skins: HashMap::new(),
            cluster: HashMap::new(),
            favorites: vec![],
            default_resource_filter: None,
            connect_timeout_seconds: crate::kube::health::DEFAULT_CONNECT_TIMEOUT_SECS,
            editor: None,
        };
        let theme = Theme::default();
        App::new(state, "test-context".to_string(), None, config, theme)
    }

    fn add_resource(app: &mut App) {
        let resource = ResourceInfo {
            name: "my-kustomization".to_string(),
            namespace: "flux-system".to_string(),
            resource_type: "Kustomization".to_string(),
            age: None,
            suspended: Some(false),
            ready: Some(true),
            message: Some("Applied revision".to_string()),
            revision: Some("main@sha1:abc123".to_string()),
            labels: HashMap::new(),
            annotations: HashMap::new(),
            last_reconciled: None,
            reconciliation_history: vec![],
        };
        app.state.upsert(
            resource_key(&resource.namespace, &resource.name, &resource.resource_type),
            resource,
        );
    }

    #[test]
    fn test_d_opens_describe_view() {
        let mut app = create_test_app(false);
        add_resource(&mut app);
        app.view_state.current_view = View::ResourceList;

        let result = app.handle_key(make_key(KeyCode::Char('d')));

        assert_eq!(result, None);
        assert_eq!(app.view_state.current_view, View::ResourceDescribe);
        assert_eq!(
            app.async_state
                .describe
                .pending()
                .map(ResourceKey::to_key_string)
                .as_deref(),
            Some("Kustomization:flux-system:my-kustomization")
        );
        assert!(app.async_state.confirmation_pending.is_none());
    }

    #[test]
    fn test_ctrl_d_still_requires_delete_confirmation() {
        let mut app = create_test_app(false);
        add_resource(&mut app);
        app.view_state.current_view = View::ResourceList;

        let result = app.handle_key(make_ctrl_key(KeyCode::Char('d')));

        assert_eq!(result, None);
        assert!(app.async_state.confirmation_pending.is_some());
        assert_eq!(app.view_state.current_view, View::ResourceList);
        assert!(app.async_state.operation.pending().is_none());
    }

    #[test]
    fn test_ctrl_d_is_blocked_in_readonly_mode() {
        let mut app = create_test_app(true);
        add_resource(&mut app);
        app.view_state.current_view = View::ResourceList;

        let result = app.handle_key(make_ctrl_key(KeyCode::Char('d')));

        assert_eq!(result, None);
        assert!(app.async_state.confirmation_pending.is_none());
        assert_eq!(
            app.ui_state.status_message,
            Some((
                crate::constants::READ_ONLY_WRITE_ACTION_MESSAGE.to_string(),
                true,
            ))
        );
    }

    #[test]
    fn test_delete_confirmation_still_blocks_execution_in_readonly_mode() {
        let mut app = create_test_app(true);
        app.async_state.confirmation_pending = Some(PendingOperation::new(
            "Kustomization".to_string(),
            "flux-system".to_string(),
            "my-kustomization".to_string(),
            'd',
        ));

        let result = app.handle_key(make_key(KeyCode::Char('y')));

        assert_eq!(result, None);
        assert!(app.async_state.confirmation_pending.is_none());
        assert!(app.async_state.operation.pending().is_none());
        assert_eq!(
            app.ui_state.status_message,
            Some((
                crate::constants::READ_ONLY_WRITE_ACTION_MESSAGE.to_string(),
                true,
            ))
        );
    }

    #[test]
    fn test_ctx_command_sets_pending_context_switch() {
        let mut app = create_test_app(false);
        app.ui_state.command_buffer = "ctx new-context-xyz".to_string();

        let result = app.execute_command();

        assert_eq!(result, None);
        assert_eq!(
            app.pending_context_switch,
            Some("new-context-xyz".to_string())
        );
        assert_eq!(
            app.ui_state.status_message,
            Some((
                "Switching to context 'new-context-xyz'...".to_string(),
                false
            ))
        );
    }

    #[test]
    fn table_command_healthy_sets_filter() {
        let mut app = create_test_app(false);
        app.ui_state.command_buffer = "healthy".to_string();

        assert_eq!(app.execute_command(), None);
        assert_eq!(app.view_state.health_filter, HealthFilter::Healthy);
    }

    #[test]
    fn table_command_all_clears_filters() {
        let mut app = create_test_app(false);
        app.view_state.selected_resource_type = Some("Kustomization".to_string());
        app.view_state.health_filter = HealthFilter::Unhealthy;
        app.ui_state.command_buffer = "all".to_string();

        assert_eq!(app.execute_command(), None);
        assert_eq!(app.view_state.selected_resource_type, None);
        assert_eq!(app.view_state.health_filter, HealthFilter::All);
    }

    #[test]
    fn table_command_favorites_switches_view() {
        let mut app = create_test_app(false);
        app.ui_state.command_buffer = "favorites".to_string();

        assert_eq!(app.execute_command(), None);
        assert_eq!(app.view_state.current_view, View::ResourceFavorites);
    }

    #[test]
    fn quit_command_returns_true() {
        let mut app = create_test_app(false);
        app.ui_state.command_buffer = "q".to_string();
        assert_eq!(app.execute_command(), Some(true));
    }

    #[test]
    fn unknown_command_reports_error() {
        let mut app = create_test_app(false);
        app.ui_state.command_buffer = "definitely-not-a-command".to_string();

        assert_eq!(app.execute_command(), None);
        let (msg, is_error) = app.ui_state.status_message.clone().unwrap();
        assert!(is_error);
        assert!(msg.contains("Unknown command"));
    }

    #[test]
    fn readonly_command_toggles_mode() {
        let mut app = create_test_app(false);
        assert!(!app.config.read_only);

        app.ui_state.command_buffer = "readonly".to_string();
        assert_eq!(app.execute_command(), None);
        assert!(app.config.read_only);

        app.ui_state.command_buffer = "readonly".to_string();
        assert_eq!(app.execute_command(), None);
        assert!(!app.config.read_only);
    }

    #[test]
    fn test_sort_keys_in_list_view() {
        use crate::tui::app::state::SortField;
        let mut app = create_test_app(false);
        assert_eq!(app.view_state.current_view, View::ResourceList);

        app.handle_key(make_key(KeyCode::Char('N')));
        assert_eq!(app.view_state.sort_field, SortField::Name);
        assert!(!app.view_state.sort_reverse);

        app.handle_key(make_key(KeyCode::Char('N')));
        assert!(app.view_state.sort_reverse);

        app.handle_key(make_key(KeyCode::Char('A')));
        assert_eq!(app.view_state.sort_field, SortField::Age);
        assert!(!app.view_state.sort_reverse);

        app.handle_key(make_key(KeyCode::Char('S')));
        assert_eq!(app.view_state.sort_field, SortField::Status);
        app.handle_key(make_key(KeyCode::Char('T')));
        assert_eq!(app.view_state.sort_field, SortField::Type);
    }

    #[test]
    fn test_sort_keys_ignored_outside_list_views() {
        use crate::tui::app::state::SortField;
        let mut app = create_test_app(false);
        app.view_state.current_view = View::ResourceYAML;

        app.handle_key(make_key(KeyCode::Char('A')));
        assert_eq!(app.view_state.sort_field, SortField::Default);
    }

    #[test]
    fn test_text_search_input_flow_in_yaml_view() {
        let mut app = create_test_app(false);
        app.view_state.current_view = View::ResourceYAML;

        // '/' opens search input
        app.handle_key(make_key(KeyCode::Char('/')));
        assert!(app.view_state.text_search.input_mode);
        // Filter mode must NOT be entered in text views
        assert!(!app.view_state.filter_mode);

        // Type a query and apply it
        app.handle_key(make_key(KeyCode::Char('a')));
        app.handle_key(make_key(KeyCode::Char('b')));
        app.handle_key(make_key(KeyCode::Backspace));
        app.handle_key(make_key(KeyCode::Enter));
        assert!(!app.view_state.text_search.input_mode);
        assert_eq!(app.view_state.text_search.query, "a");
        assert!(app.view_state.text_search.pending_jump);

        // Esc clears the active search before navigating back
        app.handle_key(make_key(KeyCode::Esc));
        assert!(app.view_state.text_search.query.is_empty());
        assert_eq!(app.view_state.current_view, View::ResourceYAML);

        // Second Esc navigates back to the list
        app.handle_key(make_key(KeyCode::Esc));
        assert_eq!(app.view_state.current_view, View::ResourceList);
    }

    #[test]
    fn test_text_search_next_prev_match_keys() {
        let mut app = create_test_app(false);
        app.view_state.current_view = View::ResourceDescribe;
        app.view_state.text_search.query = "spec".to_string();
        app.view_state.text_search.total_matches = 3;
        app.view_state.text_search.current_match = 0;

        app.handle_key(make_key(KeyCode::Char('n')));
        assert_eq!(app.view_state.text_search.current_match, 1);
        assert!(app.view_state.text_search.pending_jump);

        // 'N' wraps backwards from 1 -> 0 -> 2
        app.handle_key(make_key(KeyCode::Char('N')));
        app.handle_key(make_key(KeyCode::Char('N')));
        assert_eq!(app.view_state.text_search.current_match, 2);
    }

    #[test]
    fn test_slash_in_list_view_still_enters_filter_mode() {
        let mut app = create_test_app(false);
        assert_eq!(app.view_state.current_view, View::ResourceList);

        app.handle_key(make_key(KeyCode::Char('/')));
        assert!(app.view_state.filter_mode);
        assert!(!app.view_state.text_search.input_mode);
    }

    // --- Graph view focus navigation -------------------------------------

    fn graph_node(
        kind: &str,
        name: &str,
        node_type: crate::trace::NodeType,
    ) -> crate::trace::GraphNode {
        crate::trace::GraphNode {
            id: format!("{}:flux-system:{}", kind, name),
            kind: kind.to_string(),
            name: name.to_string(),
            namespace: "flux-system".to_string(),
            node_type,
            ready: None,
            position: None,
            description: None,
        }
    }

    /// Build a small graph: a source (watched), the object (watched), and a
    /// workload group (aggregate). Returns the app already on the graph view.
    fn app_on_graph() -> App {
        use crate::trace::{NodeType, ResourceGraph};

        let mut app = create_test_app(false);
        add_resource(&mut app); // Kustomization:flux-system:my-kustomization (watched)

        let mut graph = ResourceGraph::new();
        // idx 0: object (layer 3) — matches the watched resource
        graph.add_node(graph_node(
            "Kustomization",
            "my-kustomization",
            NodeType::Object,
        ));
        // idx 1: source (layer 1) — not in the watched state
        graph.add_node(graph_node("GitRepository", "my-repo", NodeType::Source));
        // idx 2: workload group (layer 5) — aggregate, not navigable
        graph.add_node(graph_node(
            "Workloads",
            "Workloads (1)",
            NodeType::WorkloadGroup,
        ));

        app.set_graph_result(graph);
        app.view_state.current_view = View::ResourceGraph;
        app
    }

    #[test]
    fn graph_focus_starts_on_object_node() {
        let app = app_on_graph();
        // object_node_index() == 0 in app_on_graph's graph
        assert_eq!(app.view_state.graph_focus_index, Some(0));
    }

    #[test]
    fn graph_j_k_move_focus_in_visual_order() {
        let mut app = app_on_graph();
        // Visual order by layer: source(1), object(0), workload group(2).
        // Focus starts on the object (pos 1 in that order).
        app.handle_key(make_key(KeyCode::Char('j'))); // down -> workload group
        assert_eq!(app.view_state.graph_focus_index, Some(2));

        app.handle_key(make_key(KeyCode::Char('j'))); // clamp at the bottom
        assert_eq!(app.view_state.graph_focus_index, Some(2));

        app.handle_key(make_key(KeyCode::Char('k'))); // up -> object
        assert_eq!(app.view_state.graph_focus_index, Some(0));
        app.handle_key(make_key(KeyCode::Char('k'))); // up -> source
        assert_eq!(app.view_state.graph_focus_index, Some(1));
        app.handle_key(make_key(KeyCode::Char('k'))); // clamp at the top
        assert_eq!(app.view_state.graph_focus_index, Some(1));
    }

    #[test]
    fn graph_enter_navigates_into_watched_node_and_esc_returns_to_graph() {
        let mut app = app_on_graph();
        // Focus is on the object node, which is in the watched state.
        app.handle_key(make_key(KeyCode::Enter));

        assert_eq!(app.view_state.current_view, View::ResourceDetail);
        assert_eq!(
            app.selection_state.selected_resource_key.as_deref(),
            Some("Kustomization:flux-system:my-kustomization")
        );
        assert_eq!(app.view_state.detail_back_view, Some(View::ResourceGraph));

        // Esc from the detail view returns to the graph, not the list.
        app.handle_key(make_key(KeyCode::Esc));
        assert_eq!(app.view_state.current_view, View::ResourceGraph);
        assert_eq!(app.view_state.detail_back_view, None);
        // Focus is preserved so the user lands back where they were.
        assert_eq!(app.view_state.graph_focus_index, Some(0));
    }

    #[test]
    fn graph_enter_on_unwatched_node_shows_message_and_stays() {
        let mut app = app_on_graph();
        app.view_state.graph_focus_index = Some(1); // the source, not in state

        app.handle_key(make_key(KeyCode::Enter));

        assert_eq!(app.view_state.current_view, View::ResourceGraph);
        assert!(app.ui_state.status_message.is_some());
    }

    #[test]
    fn graph_enter_on_aggregate_node_shows_message_and_stays() {
        let mut app = app_on_graph();
        app.view_state.graph_focus_index = Some(2); // the workload group aggregate

        app.handle_key(make_key(KeyCode::Enter));

        assert_eq!(app.view_state.current_view, View::ResourceGraph);
        assert!(app.ui_state.status_message.is_some());
    }

    #[test]
    fn list_enter_clears_graph_back_target() {
        let mut app = app_on_graph();
        // Simulate having drilled in from the graph earlier.
        app.view_state.detail_back_view = Some(View::ResourceGraph);

        // Now go back to the list and open a resource the normal way.
        app.view_state.current_view = View::ResourceList;
        app.view_state.selected_index = 0;
        app.handle_key(make_key(KeyCode::Enter));

        assert_eq!(app.view_state.current_view, View::ResourceDetail);
        // Back target was reset, so Esc here returns to the list.
        assert_eq!(app.view_state.detail_back_view, None);
        app.handle_key(make_key(KeyCode::Esc));
        assert_eq!(app.view_state.current_view, View::ResourceList);
    }

    fn add_kube_event(app: &mut App, uid: &str, kind: &str, name: &str) {
        let info = crate::kube::events::KubeEventInfo::from_json(&serde_json::json!({
            "metadata": {"uid": uid, "namespace": "flux-system"},
            "involvedObject": {
                "kind": kind,
                "namespace": "flux-system",
                "name": name
            },
            "type": "Normal",
            "reason": "Test",
            "message": "test",
            "lastTimestamp": "2026-07-01T12:30:00Z"
        }))
        .unwrap();
        app.kube_events.upsert(info);
    }

    #[test]
    fn events_command_opens_event_list_view() {
        let mut app = create_test_app(false);
        app.ui_state.command_buffer = "events".to_string();

        let result = app.execute_command();

        assert_eq!(result, None);
        assert_eq!(app.view_state.current_view, View::EventList);
    }

    #[test]
    fn esc_from_event_list_returns_to_resource_list_and_clears_feed() {
        let mut app = create_test_app(false);
        add_kube_event(&mut app, "uid-1", "Kustomization", "my-kustomization");
        app.view_state.current_view = View::EventList;

        let result = app.handle_key(make_key(KeyCode::Esc));

        assert_eq!(result, None);
        assert_eq!(app.view_state.current_view, View::ResourceList);
        assert!(
            app.kube_events.is_empty(),
            "leaving the events view drops the collected feed"
        );
    }

    #[test]
    fn enter_on_event_opens_watched_resource_detail() {
        let mut app = create_test_app(false);
        add_resource(&mut app);
        add_kube_event(&mut app, "uid-1", "Kustomization", "my-kustomization");
        app.view_state.current_view = View::EventList;
        app.view_state.selected_index = 0;

        app.handle_key(make_key(KeyCode::Enter));

        assert_eq!(app.view_state.current_view, View::ResourceDetail);
        assert_eq!(
            app.selection_state.selected_resource_key.as_deref(),
            Some("Kustomization:flux-system:my-kustomization")
        );
        // Back returns to the events feed, not the resource list
        assert_eq!(app.view_state.previous_list_view, View::EventList);
        app.handle_key(make_key(KeyCode::Esc));
        assert_eq!(app.view_state.current_view, View::EventList);
    }

    #[test]
    fn enter_on_event_for_unwatched_object_stays_in_event_list() {
        let mut app = create_test_app(false);
        add_kube_event(&mut app, "uid-1", "Pod", "some-pod");
        app.view_state.current_view = View::EventList;
        app.view_state.selected_index = 0;

        app.handle_key(make_key(KeyCode::Enter));

        assert_eq!(app.view_state.current_view, View::EventList);
        assert!(
            app.ui_state
                .status_message
                .as_ref()
                .is_some_and(|(msg, _)| msg.contains("Pod/some-pod")
                    && msg.contains("ns: flux-system")
                    && msg.contains("y/d")),
            "names the object + namespace and points at y/d"
        );
    }

    #[test]
    fn all_command_returns_from_event_list_and_clears_filters() {
        let mut app = create_test_app(false);
        add_kube_event(&mut app, "uid-1", "Kustomization", "my-kustomization");
        app.view_state.current_view = View::EventList;
        app.view_state.selected_resource_type = Some("HelmChart".to_string());
        app.view_state.health_filter = HealthFilter::Unhealthy;

        app.ui_state.command_buffer = "all".to_string();
        let result = app.execute_command();

        assert_eq!(result, None);
        assert_eq!(app.view_state.current_view, View::ResourceList);
        assert_eq!(app.view_state.selected_resource_type, None);
        assert_eq!(app.view_state.health_filter, HealthFilter::All);
        assert!(
            app.kube_events.is_empty(),
            "leaving the events view via :all drops the feed"
        );
    }

    #[test]
    fn y_and_d_work_from_event_list_on_involved_object() {
        let mut app = create_test_app(false);
        add_kube_event(&mut app, "uid-1", "Kustomization", "my-kustomization");
        app.view_state.current_view = View::EventList;
        app.view_state.selected_index = 0;

        app.handle_key(make_key(KeyCode::Char('y')));

        assert_eq!(app.view_state.current_view, View::ResourceYAML);
        assert_eq!(
            app.async_state
                .yaml
                .pending()
                .map(ResourceKey::to_key_string)
                .as_deref(),
            Some("Kustomization:flux-system:my-kustomization")
        );
        // Back returns to the events feed, not the resource list
        assert_eq!(app.view_state.previous_list_view, View::EventList);
        app.handle_key(make_key(KeyCode::Esc));
        assert_eq!(app.view_state.current_view, View::EventList);

        app.handle_key(make_key(KeyCode::Char('d')));
        assert_eq!(app.view_state.current_view, View::ResourceDescribe);
        assert_eq!(
            app.async_state
                .describe
                .pending()
                .map(ResourceKey::to_key_string)
                .as_deref(),
            Some("Kustomization:flux-system:my-kustomization")
        );
    }

    #[test]
    fn view_target_resolves_current_resource_from_every_view() {
        let mut app = create_test_app(false);
        add_resource(&mut app);
        add_kube_event(&mut app, "uid-1", "Kustomization", "my-kustomization");

        // Events feed: the selected event's involved object — including for
        // operations/trace/graph/history, which resolve via get_current_resource
        app.view_state.current_view = View::EventList;
        app.view_state.selected_index = 0;
        assert_eq!(
            app.view_target().map(|rk| rk.to_key_string()).as_deref(),
            Some("Kustomization:flux-system:my-kustomization")
        );
        assert!(
            app.get_current_resource()
                .is_some_and(|r| r.name == "my-kustomization"),
            "watched involved objects resolve for operations from the events feed"
        );

        // Resource list: the selected row
        app.view_state.current_view = View::ResourceList;
        assert_eq!(
            app.view_target().map(|rk| rk.to_key_string()).as_deref(),
            Some("Kustomization:flux-system:my-kustomization")
        );

        // Nested views: the stored selection
        app.selection_state.selected_resource_key =
            Some("Kustomization:flux-system:my-kustomization".to_string());
        for view in [
            View::ResourceDetail,
            View::ResourceDescribe,
            View::ResourceYAML,
            View::ResourceTrace,
            View::ResourceHistory,
        ] {
            app.view_state.current_view = view;
            assert!(app.view_target().is_some(), "{view:?} should resolve");
        }
    }

    #[test]
    fn yaml_from_focused_graph_node_returns_to_graph_on_esc() {
        let mut app = app_on_graph();
        // Focus starts on the object node (a real resource)
        app.handle_key(make_key(KeyCode::Char('y')));

        assert_eq!(app.view_state.current_view, View::ResourceYAML);
        assert_eq!(
            app.async_state
                .yaml
                .pending()
                .map(ResourceKey::to_key_string)
                .as_deref(),
            Some("Kustomization:flux-system:my-kustomization")
        );

        app.handle_key(make_key(KeyCode::Esc));
        assert_eq!(
            app.view_state.current_view,
            View::ResourceGraph,
            "Back from a view opened off a graph node returns to the graph"
        );
    }

    #[test]
    fn yaml_on_aggregate_graph_node_does_nothing() {
        let mut app = app_on_graph();
        // Move focus to the workload group (aggregate) node
        app.view_state.graph_focus_index = Some(2);

        app.handle_key(make_key(KeyCode::Char('y')));

        assert_eq!(app.view_state.current_view, View::ResourceGraph);
        assert!(app.async_state.yaml.pending().is_none());
    }

    #[test]
    fn event_list_filter_narrows_feed() {
        let mut app = create_test_app(false);
        add_kube_event(&mut app, "uid-1", "Kustomization", "podinfo");
        add_kube_event(&mut app, "uid-2", "HelmRelease", "cert-manager");
        app.view_state.current_view = View::EventList;

        app.view_state.filter = "helmrelease".to_string();
        let filtered = app.filtered_kube_events();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].involved_name, "cert-manager");

        app.view_state.filter.clear();
        assert_eq!(app.filtered_kube_events().len(), 2);
    }

    fn add_controller_pod(app: &mut App, name: &str) {
        app.controller_pods.upsert_pod(
            name.to_string(),
            crate::tui::app::state::ControllerPodInfo {
                name: name.to_string(),
                ready: true,
                version: Some("v1.0.0".to_string()),
            },
        );
    }

    #[test]
    fn logs_command_matches_pod_by_exact_name_and_prefix() {
        let mut app = create_test_app(false);
        add_controller_pod(&mut app, "source-controller-abc123");
        add_controller_pod(&mut app, "source-watcher-def456");

        // Unique prefix opens the log view
        app.ui_state.command_buffer = "logs source-c".to_string();
        app.execute_command();
        assert_eq!(app.view_state.current_view, View::Logs);
        assert!(app.logs.follow, "streams start in follow mode");

        // Back stops the session and returns to the list
        app.handle_key(make_key(KeyCode::Esc));
        assert_eq!(app.view_state.current_view, View::ResourceList);
        assert!(app.logs.session.is_none(), "Esc stops the stream");

        // Ambiguous prefix ("source" matches both pods) does not open a stream
        app.ui_state.command_buffer = "logs source".to_string();
        app.execute_command();
        assert_eq!(app.view_state.current_view, View::ResourceList);

        // No match reports an error
        app.ui_state.command_buffer = "logs nonexistent".to_string();
        app.execute_command();
        assert!(
            app.ui_state
                .status_message
                .as_ref()
                .is_some_and(|(msg, is_err)| *is_err && msg.contains("nonexistent"))
        );
    }

    #[test]
    fn logs_command_without_arg_opens_pod_submenu() {
        let mut app = create_test_app(false);

        // No pods discovered yet: error message, no submenu
        app.ui_state.command_buffer = "logs".to_string();
        app.execute_command();
        assert!(app.view_state.submenu_state.is_none());
        assert!(app.ui_state.status_message.is_some());

        add_controller_pod(&mut app, "helm-controller-xyz");
        app.ui_state.command_buffer = "logs".to_string();
        app.execute_command();
        let submenu = app
            .view_state
            .submenu_state
            .as_ref()
            .expect("submenu opens");
        assert_eq!(submenu.command, "logs");
        assert_eq!(submenu.items.len(), 1);

        // Selecting a pod opens the log view
        app.handle_key(make_key(KeyCode::Enter));
        assert_eq!(app.view_state.current_view, View::Logs);
        assert!(app.view_state.submenu_state.is_none());
    }

    #[test]
    fn log_view_scrolling_pauses_follow_and_g_resumes() {
        let mut app = create_test_app(false);
        add_controller_pod(&mut app, "source-controller-abc");
        app.ui_state.command_buffer = "logs source-controller-abc".to_string();
        app.execute_command();
        assert!(app.logs.follow);

        app.handle_key(make_key(KeyCode::Char('k')));
        assert!(!app.logs.follow, "scrolling up pauses following");

        app.handle_key(make_key(KeyCode::Char('G')));
        assert!(app.logs.follow, "G resumes following");
    }

    #[test]
    fn logs_from_event_list_returns_to_event_list() {
        let mut app = create_test_app(false);
        add_controller_pod(&mut app, "source-controller-abc");
        app.view_state.current_view = View::EventList;

        app.ui_state.command_buffer = "logs source-controller-abc".to_string();
        app.execute_command();
        assert_eq!(app.view_state.current_view, View::Logs);

        app.handle_key(make_key(KeyCode::Esc));
        assert_eq!(
            app.view_state.current_view,
            View::EventList,
            "Back returns to the events feed logs were opened from"
        );
    }

    /// Build an app on a graph whose workload group carries two encoded
    /// workload rows (the #194 drill-down entry point).
    fn app_on_graph_with_workloads() -> App {
        use crate::kube::workloads::WorkloadRef;
        use crate::trace::{NodeType, ResourceGraph};

        let mut app = create_test_app(false);
        add_resource(&mut app);

        let lines = [
            WorkloadRef {
                kind: "Deployment".to_string(),
                name: "podinfo".to_string(),
                namespace: "flux-system".to_string(),
                indicator: "\u{25cf}".to_string(),
                status: "Replicas: 1/1".to_string(),
            },
            WorkloadRef {
                kind: "StatefulSet".to_string(),
                name: "redis".to_string(),
                namespace: "flux-system".to_string(),
                indicator: "\u{25cb}".to_string(),
                status: "Replicas: 0/1".to_string(),
            },
        ]
        .iter()
        .map(WorkloadRef::to_graph_line)
        .collect::<Vec<_>>()
        .join("\n");

        let mut graph = ResourceGraph::new();
        graph.add_node(graph_node(
            "Kustomization",
            "my-kustomization",
            NodeType::Object,
        ));
        let mut group = graph_node("Workloads", "Workloads (2)", NodeType::WorkloadGroup);
        group.description = Some(lines);
        graph.add_node(group);
        app.set_graph_result(graph);
        app.view_state.current_view = View::ResourceGraph;
        app.view_state.graph_focus_index = Some(1);
        app
    }

    #[test]
    fn workload_group_enter_opens_workload_list() {
        let mut app = app_on_graph_with_workloads();

        app.handle_key(make_key(KeyCode::Enter));

        assert_eq!(app.view_state.current_view, View::WorkloadList);
        assert_eq!(app.view_state.workload_rows.len(), 2);
        assert_eq!(app.view_state.workload_rows[0].name, "podinfo");
        assert_eq!(app.view_state.selected_index, 0);

        // Back returns to the graph
        app.handle_key(make_key(KeyCode::Esc));
        assert_eq!(app.view_state.current_view, View::ResourceGraph);
    }

    #[test]
    fn workload_group_without_rows_shows_hint() {
        let mut app = app_on_graph();
        app.view_state.graph_focus_index = Some(2); // undescribed workload group

        app.handle_key(make_key(KeyCode::Enter));

        assert_eq!(app.view_state.current_view, View::ResourceGraph);
        assert!(app.ui_state.status_message.is_some());
    }

    #[test]
    fn workload_list_enter_requests_detail_and_back_chain_holds() {
        let mut app = app_on_graph_with_workloads();
        app.handle_key(make_key(KeyCode::Enter)); // -> WorkloadList
        app.handle_key(make_key(KeyCode::Char('j'))); // select redis
        app.handle_key(make_key(KeyCode::Enter)); // -> WorkloadDetail

        assert_eq!(app.view_state.current_view, View::WorkloadDetail);
        assert_eq!(
            app.async_state
                .workload
                .pending()
                .map(crate::watcher::ResourceKey::to_key_string)
                .as_deref(),
            Some("StatefulSet:flux-system:redis")
        );

        // Full back chain: detail -> list -> graph
        app.handle_key(make_key(KeyCode::Esc));
        assert_eq!(app.view_state.current_view, View::WorkloadList);
        app.handle_key(make_key(KeyCode::Esc));
        assert_eq!(app.view_state.current_view, View::ResourceGraph);
    }

    fn workload_data(pods: &[&str]) -> crate::kube::workloads::WorkloadData {
        crate::kube::workloads::WorkloadData {
            kind: "Deployment".to_string(),
            name: "podinfo".to_string(),
            namespace: "flux-system".to_string(),
            ready: Some(true),
            summary: Vec::new(),
            containers: Vec::new(),
            pods: pods
                .iter()
                .map(|name| crate::kube::workloads::PodRow {
                    name: name.to_string(),
                    phase: "Running".to_string(),
                    ready: "1/1".to_string(),
                    restarts: 0,
                    age: None,
                })
                .collect(),
            events: Vec::new(),
            events_error: None,
        }
    }

    #[test]
    fn workload_logs_single_pod_streams_and_returns_to_detail() {
        let mut app = app_on_graph_with_workloads();
        app.view_state.current_view = View::WorkloadDetail;
        app.async_state
            .workload
            .set_result(workload_data(&["podinfo-abc"]));

        app.handle_key(make_key(KeyCode::Char('l')));

        assert_eq!(app.view_state.current_view, View::Logs);
        assert_eq!(
            app.view_state.logs_back_view,
            Some(View::WorkloadDetail),
            "Back from pod logs returns to the workload detail"
        );

        app.handle_key(make_key(KeyCode::Esc));
        assert_eq!(app.view_state.current_view, View::WorkloadDetail);
        assert!(app.logs.session.is_none(), "Esc stops the stream");
    }

    #[test]
    fn workload_logs_multiple_pods_open_submenu() {
        let mut app = app_on_graph_with_workloads();
        app.view_state.current_view = View::WorkloadDetail;
        app.async_state
            .workload
            .set_result(workload_data(&["podinfo-abc", "podinfo-def"]));

        app.handle_key(make_key(KeyCode::Char('l')));

        let submenu = app
            .view_state
            .submenu_state
            .as_ref()
            .expect("multiple pods open a submenu");
        assert_eq!(submenu.command, "pod-logs");
        assert_eq!(submenu.items.len(), 2);
        assert_eq!(submenu.items[0].value, "flux-system/podinfo-abc");

        // Selecting a pod opens its log stream
        app.handle_key(make_key(KeyCode::Enter));
        assert_eq!(app.view_state.current_view, View::Logs);
        assert_eq!(app.view_state.logs_back_view, Some(View::WorkloadDetail));
    }

    #[test]
    fn l_from_workload_list_continues_into_pod_logs_on_load() {
        let mut app = app_on_graph_with_workloads();
        app.handle_key(make_key(KeyCode::Enter)); // -> WorkloadList

        app.handle_key(make_key(KeyCode::Char('l')));
        assert_eq!(app.view_state.current_view, View::WorkloadDetail);
        assert!(app.logs_after_workload_load, "auto-logs intent recorded");
        assert!(app.async_state.workload.pending().is_some());

        // Simulate the fetch completing (what the main loop does)
        let _ = app.async_state.workload.dispatch();
        app.on_workload_loaded(workload_data(&["podinfo-abc"]));

        assert_eq!(
            app.view_state.current_view,
            View::Logs,
            "single-pod workload streams immediately after loading"
        );
        assert!(!app.logs_after_workload_load, "intent consumed");

        // Back returns to where `l` was pressed — the workload LIST
        app.handle_key(make_key(KeyCode::Esc));
        assert_eq!(app.view_state.current_view, View::WorkloadList);
    }

    #[test]
    fn plain_enter_does_not_auto_open_logs_on_load() {
        let mut app = app_on_graph_with_workloads();
        app.handle_key(make_key(KeyCode::Enter)); // -> WorkloadList
        app.handle_key(make_key(KeyCode::Enter)); // -> WorkloadDetail (plain)

        let _ = app.async_state.workload.dispatch();
        app.on_workload_loaded(workload_data(&["podinfo-abc"]));

        assert_eq!(
            app.view_state.current_view,
            View::WorkloadDetail,
            "Enter shows the detail; it must not jump into logs"
        );
    }

    #[test]
    fn ns_command_without_arg_opens_picker_and_selects() {
        let mut app = create_test_app(false);
        // Default hotkeys (no config, no discovery) are ["all", "flux-system"].
        app.ui_state.command_buffer = "ns".to_string();
        app.execute_command();

        let submenu = app
            .view_state
            .submenu_state
            .as_ref()
            .expect(":ns with no argument opens the namespace picker");
        assert_eq!(submenu.command, "ns");
        assert_eq!(submenu.items[0].value, "all");
        assert!(
            submenu.items.iter().any(|i| i.value == "flux-system"),
            "picker offers the discovered/hotkey namespaces"
        );

        // Selecting a namespace switches to it and closes the submenu.
        app.handle_key(make_key(KeyCode::Char('j'))); // -> flux-system
        app.handle_key(make_key(KeyCode::Enter));
        assert!(app.view_state.submenu_state.is_none());
        assert_eq!(app.namespace, Some("flux-system".to_string()));
    }

    #[test]
    fn ns_command_with_arg_switches_directly_without_picker() {
        let mut app = create_test_app(false);
        app.ui_state.command_buffer = "ns kube-system".to_string();
        app.execute_command();

        assert!(
            app.view_state.submenu_state.is_none(),
            ":ns <name> switches directly, no picker"
        );
        assert_eq!(app.namespace, Some("kube-system".to_string()));

        // `:ns all` returns to the cluster-wide scope.
        app.ui_state.command_buffer = "ns all".to_string();
        app.execute_command();
        assert_eq!(app.namespace, None);
    }

    #[test]
    fn forbidden_watcher_state_tracks_and_clears() {
        let mut app = create_test_app(false);
        assert!(app.forbidden_watchers.is_empty());

        app.watch_forbidden("HelmRelease".to_string());
        assert!(!app.forbidden_watchers.is_empty());
        assert!(app.forbidden_watchers.contains("HelmRelease"));

        // A recovered watch for the same kind clears the restriction.
        app.watch_recovered("HelmRelease");
        assert!(app.forbidden_watchers.is_empty());

        // Marking forbidden also drops any prior degraded flag for that kind.
        app.watch_degraded("HelmRelease".to_string());
        app.watch_forbidden("HelmRelease".to_string());
        assert!(!app.degraded_watchers.contains("HelmRelease"));
        assert!(app.forbidden_watchers.contains("HelmRelease"));
    }

    #[test]
    fn access_notice_is_contextual_and_respects_config() {
        let mut app = create_test_app(false);
        // Nothing restricted -> no notice.
        assert_eq!(app.access_notice(), None);

        app.watch_forbidden("HelmRelease".to_string());

        // Unified list (no selected type) -> summary mentioning the kind.
        app.view_state.selected_resource_type = None;
        let notice = app.access_notice().expect("unified notice");
        assert!(
            notice.contains("HelmRelease"),
            "notice names the kind: {notice}"
        );

        // Selected the restricted kind -> specific notice.
        app.view_state.selected_resource_type = Some("HelmRelease".to_string());
        assert!(app.access_notice().unwrap().contains("HelmRelease"));

        // Selected a DIFFERENT, non-restricted kind -> no false alarm.
        app.view_state.selected_resource_type = Some("Kustomization".to_string());
        assert_eq!(app.access_notice(), None);

        // Config off -> silent, even when restricted and viewing the kind.
        app.view_state.selected_resource_type = Some("HelmRelease".to_string());
        app.config.ui.rbac_warnings = false;
        assert_eq!(app.access_notice(), None);
    }

    #[test]
    fn switching_namespace_clears_forbidden_state() {
        let mut app = create_test_app(false);
        app.watch_forbidden("HelmRelease".to_string());
        assert!(!app.forbidden_watchers.is_empty());

        // A namespace switch restarts watchers, so stale RBAC state must clear.
        app.ui_state.command_buffer = "ns kube-system".to_string();
        app.execute_command();
        assert!(app.forbidden_watchers.is_empty());
    }

    #[test]
    fn submenu_filter_follows_resource_filter_convention() {
        let mut app = create_test_app(false);
        add_controller_pod(&mut app, "source-controller-abc");
        add_controller_pod(&mut app, "helm-controller-def");
        app.ui_state.command_buffer = "logs".to_string();
        app.execute_command();

        // Keys don't filter until `/` enters filter mode
        app.handle_key(make_key(KeyCode::Char('x')));
        let submenu = app.view_state.submenu_state.as_ref().unwrap();
        assert!(
            submenu.filter.is_empty(),
            "typing without / must not filter"
        );
        assert!(!submenu.filter_mode);

        // `/` enters filter mode; typing narrows; Enter applies (keeps filter)
        app.handle_key(make_key(KeyCode::Char('/')));
        assert!(app.view_state.submenu_state.as_ref().unwrap().filter_mode);
        app.handle_key(make_key(KeyCode::Char('h')));
        let submenu = app.view_state.submenu_state.as_ref().unwrap();
        assert_eq!(submenu.filter, "h");
        assert_eq!(submenu.filtered_items().len(), 1);
        app.handle_key(make_key(KeyCode::Enter));
        let submenu = app.view_state.submenu_state.as_ref().unwrap();
        assert!(!submenu.filter_mode, "Enter applies the filter");
        assert_eq!(submenu.filter, "h", "filter survives applying");

        // Enter now selects the filtered item (streams its logs)
        app.handle_key(make_key(KeyCode::Enter));
        assert_eq!(app.view_state.current_view, View::Logs);
        let (request, _tx) = app.logs.dispatch().expect("log stream queued");
        assert_eq!(request.pod, "helm-controller-def");
    }

    #[test]
    fn esc_in_submenu_filter_mode_cancels_like_resource_filter() {
        let mut app = create_test_app(false);
        add_controller_pod(&mut app, "source-controller-abc");
        app.ui_state.command_buffer = "logs".to_string();
        app.execute_command();

        app.handle_key(make_key(KeyCode::Char('/')));
        app.handle_key(make_key(KeyCode::Char('z')));
        // Esc exits filter mode and clears the filter — submenu stays open
        app.handle_key(make_key(KeyCode::Esc));
        let submenu = app.view_state.submenu_state.as_ref().unwrap();
        assert!(!submenu.filter_mode);
        assert!(submenu.filter.is_empty());
        assert_eq!(submenu.filtered_items().len(), 1);

        // A second Esc closes the submenu
        app.handle_key(make_key(KeyCode::Esc));
        assert!(app.view_state.submenu_state.is_none());
    }

    #[test]
    fn colon_in_submenu_opens_command_mode() {
        let mut app = create_test_app(false);
        add_controller_pod(&mut app, "source-controller-abc");
        app.ui_state.command_buffer = "logs".to_string();
        app.execute_command();
        assert!(app.view_state.submenu_state.is_some());

        app.handle_key(make_key(KeyCode::Char(':')));

        assert!(app.view_state.submenu_state.is_none(), "submenu closes");
        assert!(app.ui_state.command_mode, "command mode opens");
    }

    #[test]
    fn pulse_command_opens_dashboard_and_esc_returns() {
        let mut app = create_test_app(false);
        add_resource(&mut app);

        app.ui_state.command_buffer = "pulse".to_string();
        let result = app.execute_command();

        assert_eq!(result, None);
        assert_eq!(app.view_state.current_view, View::Pulse);

        app.handle_key(make_key(KeyCode::Esc));
        assert_eq!(app.view_state.current_view, View::ResourceList);
    }

    /// #197: without discovery, unknown kinds stay unknown (zero impact);
    /// once a kind is registered, its CRD-derived aliases resolve like
    /// built-in resource commands. Uses a unique kind name because the
    /// registry is process-global.
    #[test]
    fn discovered_kind_commands_resolve_only_when_registered() {
        use crate::models::extra_kinds::{ExtraKind, global};

        let mut app = create_test_app(false);

        // Zero impact: unregistered kind is an unknown command
        app.ui_state.command_buffer = "zephyrtest".to_string();
        app.execute_command();
        assert!(app.view_state.selected_resource_type.is_none());
        assert!(
            app.ui_state
                .status_message
                .as_ref()
                .is_some_and(|(msg, is_err)| *is_err && msg.contains("Unknown command"))
        );

        global().insert(ExtraKind {
            kind: "ZephyrTest".to_string(),
            group: "example.com".to_string(),
            version: "v1".to_string(),
            plural: "zephyrtests".to_string(),
            short_names: vec!["zt9".to_string()],
        });

        // Kind, plural, and short name now resolve to the type filter
        for token in ["zephyrtest", "zephyrtests", "zt9"] {
            app.view_state.selected_resource_type = None;
            app.ui_state.command_buffer = token.to_string();
            app.execute_command();
            assert_eq!(
                app.view_state.selected_resource_type.as_deref(),
                Some("ZephyrTest"),
                "{token} should resolve"
            );
        }

        // Autocomplete offers the discovered aliases
        let matches = crate::tui::commands::find_matching_commands("zephyr");
        assert!(matches.contains(&"zephyrtest".to_string()));
        assert!(matches.contains(&"zephyrtests".to_string()));

        // GVK resolution works for read-only fetches (y/d)
        let gvk = crate::kube::get_gvk_for_resource_type("ZephyrTest").unwrap();
        assert_eq!(
            gvk,
            (
                "example.com".to_string(),
                "v1".to_string(),
                "zephyrtests".to_string()
            )
        );

        // Discovered kinds stay view-only: no operation considers them valid
        let registry = crate::tui::OperationRegistry::new();
        for key in ['s', 'r', 'R', 'W'] {
            if let Some(op) = registry.get_by_keybinding(key) {
                assert!(
                    !op.is_valid_for("ZephyrTest"),
                    "operation '{key}' must not apply to discovered kinds"
                );
            }
        }

        // Cleanup the process-global registry
        global().remove("ZephyrTest");
        assert!(
            crate::kube::get_gvk_for_resource_type("ZephyrTest").is_err(),
            "removal restores the unknown-kind error"
        );
    }
}
