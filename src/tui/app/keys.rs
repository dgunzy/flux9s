//! Key input: the top-level `handle_key` dispatch and the modal inputs
//! (command line, filter, text search, confirmations, quit prompt).

use super::core::App;
use super::state::{Confirmation, ScrollModel, View};
use crossterm::event::KeyEvent;

impl App {
    /// Scroll the active view down by `amount` (j/Down, PageDown, Ctrl+F),
    /// as its [`ScrollModel`] says. Renderers clamp, so overshooting is fine.
    pub(super) fn scroll_down(&mut self, amount: usize) {
        self.scroll_by(amount as isize);
    }

    /// Scroll the active view up by `amount` (k/Up, PageUp, Ctrl+B).
    pub(super) fn scroll_up(&mut self, amount: usize) {
        self.scroll_by(-(amount as isize));
    }

    fn scroll_by(&mut self, delta: isize) {
        let view = self.view_state.current_view;
        // Manual scrolling in the log view pauses following (G resumes).
        if view == View::Logs {
            self.logs.follow = false;
        }
        let amount = delta.unsigned_abs();
        match view.scroll_model() {
            ScrollModel::Offset => {
                if let Some(offset) = view.scroll_offset_mut(&mut self.view_state) {
                    *offset = if delta >= 0 {
                        offset.saturating_add(amount)
                    } else {
                        offset.saturating_sub(amount)
                    };
                }
            }
            ScrollModel::Selection => {
                let max_index = self.selection_len(view).saturating_sub(1);
                let index = &mut self.view_state.selected_index;
                *index = if delta >= 0 {
                    index.saturating_add(amount).min(max_index)
                } else {
                    index.saturating_sub(amount)
                };
            }
            // The renderer scrolls to keep the focused node on screen.
            ScrollModel::Focus => self.move_graph_focus(delta >= 0),
            ScrollModel::Fixed => {}
        }
    }

    /// Rows of a selection-scrolled view.
    fn selection_len(&self, view: View) -> usize {
        match view {
            View::EventList => self.filtered_kube_events().len(),
            View::WorkloadList => self.view_state.workload_rows.len(),
            View::InventoryList => self.view_state.inventory_rows.len(),
            View::KindList => self.filtered_kind_rows().len(),
            _ => self.get_filtered_resources().len(),
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
                    self.toggle_help();
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
        let is_operation_key = self.action_for(&key).is_some_and(|a| a.sets_status);

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

        // The help overlay scrolls like the text views.
        if self.ui_state.show_help && self.handle_help_key(key) {
            return None;
        }

        // Text-view search input (typing the query after pressing / in YAML/describe/trace)
        if self.view_state.text_search.input_mode {
            return self.handle_text_search_key(key);
        }

        // Everything else comes from the action table (actions.rs): the
        // same entries drive the footer and help.
        self.dispatch_action(key).flatten()
    }

    pub(super) fn handle_filter_key(&mut self, key: KeyEvent) -> Option<bool> {
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
    pub(super) fn is_text_search_view(&self) -> bool {
        self.view_state.current_view.is_text_search_view()
    }

    /// Handle a key press while typing a text-view search query
    pub(super) fn handle_text_search_key(&mut self, key: KeyEvent) -> Option<bool> {
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
    pub(super) fn advance_text_search(&mut self, delta: isize) {
        let search = &mut self.view_state.text_search;
        if search.total_matches == 0 {
            return;
        }
        let total = search.total_matches as isize;
        search.current_match = (search.current_match as isize + delta).rem_euclid(total) as usize;
        search.pending_jump = true;
    }

    pub(super) fn navigate_back_or_confirm_quit(&mut self) -> Option<bool> {
        if self.ui_state.show_help {
            self.ui_state.show_help = false;
            return None;
        }
        // A kind list's Esc clears its filter first.
        if self.view_state.current_view == View::KindList {
            self.kind_list_escape();
            return None;
        }
        if !self.navigate_back() {
            if self.view_state.current_view == View::ResourceList {
                // At the top-level view there is nowhere to go back to, so ask
                // for confirmation rather than exiting immediately (k9s convention).
                self.ui_state.show_quit_confirm = true;
            } else {
                self.navigate_root(View::ResourceList);
            }
        }
        None
    }

    /// Handle a key press while the quit confirmation dialog is visible.
    ///
    /// `y`/`Y` confirms and exits. `n`/`N`/`q`/`Esc` all cancel — `q` is
    /// included so the footer hint is consistent with what actually works.
    /// All other keys are ignored while the dialog is open.
    pub(super) fn handle_quit_confirm_key(&mut self, key: KeyEvent) -> Option<bool> {
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

    pub(super) fn handle_confirmation_key(&mut self, key: KeyEvent) -> Option<bool> {
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
                    match pending_clone {
                        Confirmation::Flux(op) => self.execute_operation(
                            &op.resource_type,
                            &op.namespace,
                            &op.name,
                            op.operation_key,
                        ),
                        Confirmation::Object(action) => {
                            self.async_state.object_action.request(action);
                        }
                        Confirmation::Edit(_) => self.answer_edit_warning(true),
                    }
                }
                crossterm::event::KeyCode::Char('n')
                | crossterm::event::KeyCode::Char('N')
                | crossterm::event::KeyCode::Esc => {
                    // Cancel operation
                    let pending = self.async_state.confirmation_pending.take();
                    if matches!(pending, Some(Confirmation::Edit(_))) {
                        self.answer_edit_warning(false);
                    }
                }
                _ => {}
            }
        }
        None
    }

    pub(super) fn handle_command_key(&mut self, key: KeyEvent) -> Option<bool> {
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

    pub(super) fn autocomplete_command(&mut self) {
        let cmd = self.ui_state.command_buffer.trim();

        // Command buffer doesn't include the ':' prefix (it's shown in UI)
        // So we match against the buffer directly
        let cmd_lower = cmd.to_lowercase();

        // Don't autocomplete namespace names
        if crate::tui::commands::is_namespace_command(&cmd_lower) && cmd_lower.contains(' ') {
            return;
        }

        // A kind set (`ks,he`, #302): complete its last kind.
        if let Some((done, last)) = cmd_lower.rsplit_once(',') {
            // Nothing typed after the comma yet: no kind to pick.
            if last.trim().is_empty() {
                return;
            }
            let kind = crate::tui::commands::find_matching_commands(last.trim())
                .into_iter()
                .find(|candidate| crate::models::kind_filter::KindFilter::parse(candidate).is_ok());
            if let Some(kind) = kind {
                self.ui_state.command_buffer = format!("{done},{kind}");
            }
            return;
        }

        // User views first (#302), then the centralized command registry
        // (CRD commands before app commands).
        let mut matches: Vec<String> = self
            .config
            .views
            .keys()
            .map(|view| view.to_lowercase())
            .filter(|view| view.starts_with(&cmd_lower))
            .collect();
        matches.sort();
        for command in crate::tui::commands::find_matching_commands(&cmd_lower) {
            if !matches.contains(&command) {
                matches.push(command);
            }
        }

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

    /// Show or hide the help overlay (opening starts at the top).
    pub(super) fn toggle_help(&mut self) {
        self.ui_state.show_help = !self.ui_state.show_help;
        self.ui_state.help_scroll = 0;
    }

    /// Scroll keys while the help overlay is open. Returns whether the key
    /// was used; `?` / `Esc` / `q` fall through and close it.
    pub(super) fn handle_help_key(&mut self, key: KeyEvent) -> bool {
        use crossterm::event::{KeyCode, KeyModifiers};
        let page = self.view_state.page_size.max(1);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let scroll = &mut self.ui_state.help_scroll;
        match key.code {
            KeyCode::Char('j') | KeyCode::Down if !ctrl => *scroll = scroll.saturating_add(1),
            KeyCode::Char('k') | KeyCode::Up if !ctrl => *scroll = scroll.saturating_sub(1),
            KeyCode::PageDown => *scroll = scroll.saturating_add(page),
            KeyCode::Char('f') if ctrl => *scroll = scroll.saturating_add(page),
            KeyCode::PageUp => *scroll = scroll.saturating_sub(page),
            KeyCode::Char('b') if ctrl => *scroll = scroll.saturating_sub(page),
            KeyCode::Char('g') | KeyCode::Home => *scroll = 0,
            // Clamped to the last page when rendered.
            KeyCode::Char('G') | KeyCode::End => *scroll = usize::MAX,
            _ => return false,
        }
        true
    }
}
