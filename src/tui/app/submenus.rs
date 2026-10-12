//! Popup (submenu) input: navigation, filtering, and selection.

use super::core::App;
use crate::tui::commands::submenu_ids;
use crossterm::event::KeyEvent;

/// What selecting an item does, given the item's value.
type SubmenuHandler = fn(&mut App, String);

/// Selection dispatch for every popup, keyed by its id (the builder sets the
/// same id from [`submenu_ids`]). A popup without an entry does nothing on
/// Enter — the `submenu_table_covers_every_popup` test keeps the two in step.
pub(super) const SUBMENU_TABLE: &[(&str, SubmenuHandler)] = &[
    (submenu_ids::CONTEXT, App::select_context),
    (submenu_ids::NAMESPACE, App::select_namespace),
    (submenu_ids::CONTROLLER_LOGS, App::select_controller_logs),
    (submenu_ids::POD_DELETE, App::select_pod_delete),
    (submenu_ids::LOG_CONTAINER, App::select_log_container),
    (submenu_ids::POD_LOGS, App::select_pod_logs),
    (submenu_ids::SKIN, App::select_skin),
];

impl App {
    pub(super) fn handle_submenu_key(&mut self, key: KeyEvent) -> Option<bool> {
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
                    if submenu.command == submenu_ids::SKIN {
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
                    if submenu.command == submenu_ids::SKIN =>
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
                        if let Some((_, select)) =
                            SUBMENU_TABLE.iter().find(|(id, _)| *id == command)
                        {
                            select(self, value);
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
                    if submenu.command == submenu_ids::SKIN {
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
    pub(super) fn preview_theme_in_submenu(&mut self) {
        if let Some(ref submenu) = self.view_state.submenu_state {
            if submenu.command == submenu_ids::SKIN {
                if let Some(theme_name) = submenu.selected_value() {
                    // Preview the theme (don't show errors, just silently fail)
                    let _ = self.preview_theme(&theme_name);
                }
            }
        }
    }

    /// `:ctx` popup: switch context.
    fn select_context(&mut self, context: String) {
        self.set_status_message((format!("Switching to context '{}'...", context), false));
        self.pending_context_switch = Some(context);
    }

    /// `:ns` popup: "all" watches every namespace.
    fn select_namespace(&mut self, namespace: String) {
        let namespace = (namespace != "all").then_some(namespace);
        self.switch_namespace(namespace);
    }

    /// `:logs` popup: stream a controller pod.
    fn select_controller_logs(&mut self, pod: String) {
        self.open_log_view(&pod);
    }

    /// Workload pod delete popup: the value is "namespace/pod".
    fn select_pod_delete(&mut self, value: String) {
        if let Some((namespace, pod)) = value.split_once('/') {
            self.confirm_object_action(crate::kube::workloads::ObjectAction::DeletePod {
                namespace: namespace.to_string(),
                name: pod.to_string(),
            });
        }
    }

    /// Container picker in the log view.
    fn select_log_container(&mut self, container: String) {
        self.switch_log_container(container);
    }

    /// Workload pod logs popup: the value is "namespace/pod".
    fn select_pod_logs(&mut self, value: String) {
        if let Some((namespace, pod)) = value.split_once('/') {
            let (namespace, pod) = (namespace.to_string(), pod.to_string());
            self.open_pod_logs(&namespace, &pod);
        }
    }

    /// `:skin` popup: confirm the (already previewed) theme.
    fn select_skin(&mut self, theme: String) {
        match self.set_theme(&theme) {
            Ok(_) => self.set_status_message((format!("Theme changed to: {}", theme), false)),
            Err(e) => {
                self.set_status_message((format!("Failed to load theme '{}': {}", theme, e), true))
            }
        }
    }
}
