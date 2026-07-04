//! Application state structures
//!
//! This module contains state sub-structures that organize the App's fields
//! into logical groupings for better maintainability and testability.

use crate::tui::submenu::SubmenuState;
use crate::watcher::ResourceKey;
use std::collections::HashSet;

/// View types for the application
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum View {
    ResourceList,
    ResourceDetail,
    ResourceDescribe,
    ResourceYAML,
    ResourceTrace,
    ResourceGraph,
    ResourceFavorites,
    ResourceHistory,
    ResourceEdit,
    #[allow(dead_code)] // Reserved for future alternative help view implementation
    Help,
}

/// Connection status to the Kubernetes API server.
///
/// Drives the startup connection-error screen: while `Connecting` the splash is
/// shown, on `Failed` the full-screen error view takes over (see
/// [`crate::tui::views::render_connection_error`]).
#[derive(Debug, Default)]
pub enum ConnectionStatus {
    /// Initial connection/health check in progress.
    #[default]
    Connecting,
    /// Successfully connected and watching resources.
    Connected,
    /// Connection failed; carries the classified error for display.
    Failed(Box<crate::kube::health::ConnectionError>),
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

/// View-related state (navigation, scrolling, filtering)
#[derive(Debug)]
pub struct ViewState {
    /// Current view being displayed
    pub current_view: View,
    /// Selected resource type filter (None = unified view)
    pub selected_resource_type: Option<String>,
    /// Text filter for resource list
    pub filter: String,
    /// Whether filter mode is active (user is typing)
    pub filter_mode: bool,
    /// Health filter (All, Healthy, Unhealthy)
    pub health_filter: HealthFilter,
    /// Selected index in current list
    pub selected_index: usize,
    /// Scroll offset for resource list
    pub scroll_offset: usize,
    /// Scroll offset for YAML view
    pub yaml_scroll_offset: usize,
    /// Scroll offset for describe view
    pub describe_scroll_offset: usize,
    /// Scroll offset for trace view
    pub trace_scroll_offset: usize,
    /// Scroll offset for history view
    pub history_scroll_offset: usize,
    /// Scroll offset for graph view
    pub graph_scroll_offset: usize,
    /// Track previous list view for navigation (ResourceList or ResourceFavorites)
    pub previous_list_view: View,
    /// Active submenu state (if a submenu is currently being displayed)
    pub submenu_state: Option<SubmenuState>,
    /// Original theme name when previewing themes in submenu (for restoration on cancel)
    pub preview_original_theme: Option<String>,
    /// Cached page size for PageUp/PageDown navigation (updated each render from content area height)
    pub page_size: usize,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            current_view: View::ResourceList,
            selected_resource_type: None,
            filter: String::new(),
            filter_mode: false,
            health_filter: HealthFilter::All,
            selected_index: 0,
            scroll_offset: 0,
            yaml_scroll_offset: 0,
            describe_scroll_offset: 0,
            trace_scroll_offset: 0,
            history_scroll_offset: 0,
            graph_scroll_offset: 0,
            previous_list_view: View::ResourceList,
            submenu_state: None,
            preview_original_theme: None,
            page_size: 10,
        }
    }
}

/// Selection state (what resource is selected, favorites)
#[derive(Debug, Default)]
pub struct SelectionState {
    /// Key of currently selected resource (resource_type:namespace:name)
    pub selected_resource_key: Option<String>,
    /// Set of favorited resource keys
    pub favorites: HashSet<String>,
    /// Flag indicating favorites need to be saved
    pub favorites_pending_save: bool,
}

/// UI-related state (command mode, status messages, layout cache)
#[derive(Debug)]
pub struct UIState {
    /// Whether command mode is active
    pub command_mode: bool,
    /// Command buffer for command mode
    pub command_buffer: String,
    /// Whether to show help overlay
    pub show_help: bool,
    /// Whether to show quit confirmation dialog.
    ///
    /// Shown when `q` or `Esc` is pressed at the top-level view. This is
    /// closer to k9s behaviour, where neither key exits directly. Use `Q`,
    /// `:q`, or `Ctrl+C` to exit without going through this dialog.
    pub show_quit_confirm: bool,
    /// Status message to display (message, is_error)
    pub status_message: Option<(String, bool)>,
    /// When status message was set (for auto-clearing)
    pub status_message_time: Option<std::time::Instant>,
    /// Whether to show splash screen
    pub show_splash: bool,
    /// When splash screen started (for duration calculation)
    pub splash_start_time: Option<std::time::Instant>,
    /// Cached terminal size to detect resizes
    pub cached_terminal_size: Option<(u16, u16)>,
    /// Cached header height to prevent flicker
    pub cached_header_height: u16,
    /// Cached footer height to prevent flicker
    pub cached_footer_height: u16,
    /// Current connection status to the cluster.
    pub connection_status: ConnectionStatus,
}

impl UIState {
    pub fn new(show_splash: bool) -> Self {
        Self {
            command_mode: false,
            command_buffer: String::new(),
            show_help: false,
            show_quit_confirm: false,
            status_message: None,
            status_message_time: None,
            show_splash,
            splash_start_time: None,
            cached_terminal_size: None,
            cached_header_height: crate::tui::constants::MIN_HEADER_HEIGHT,
            cached_footer_height: crate::tui::constants::MIN_FOOTER_HEIGHT,
            connection_status: ConnectionStatus::Connecting,
        }
    }
}

/// Editor state for YAML editing
///
/// Tracks cursor position, content, and validation state while editing resource specs.
#[derive(Debug, Clone)]
pub struct EditorState {
    /// Content lines (one line per element)
    pub lines: Vec<String>,
    /// Cursor row (0-indexed, within bounds [0, lines.len()))
    pub cursor_row: usize,
    /// Cursor column (0-indexed, within bounds [0, current_line.len()])
    pub cursor_col: usize,
    /// Scroll offset for vertical scrolling (first visible line)
    pub scroll_offset: usize,
    /// Horizontal scroll offset for long lines (used in Phase 3)
    #[allow(dead_code)]
    pub horizontal_scroll: usize,
    /// Current validation error message (if any)
    pub validation_error: Option<String>,
    /// Undo stack (for potential Ctrl+Z implementation in Phase 3)
    #[allow(dead_code)]
    pub undo_stack: Vec<Vec<String>>,
}

impl EditorState {
    /// Create a new editor state from initial YAML content
    pub fn new(content: &str) -> Self {
        let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
        if lines.is_empty() {
            Self {
                lines: vec![String::new()],
                cursor_row: 0,
                cursor_col: 0,
                scroll_offset: 0,
                horizontal_scroll: 0,
                validation_error: None,
                undo_stack: vec![],
            }
        } else {
            Self {
                lines,
                cursor_row: 0,
                cursor_col: 0,
                scroll_offset: 0,
                horizontal_scroll: 0,
                validation_error: None,
                undo_stack: vec![],
            }
        }
    }

    /// Get the complete content as a string
    pub fn get_content(&self) -> String {
        self.lines.join("\n")
    }

    /// Insert a character at the current cursor position
    pub fn insert_char(&mut self, ch: char) {
        if self.cursor_row < self.lines.len() {
            let line = &mut self.lines[self.cursor_row];
            let col = self.cursor_col.min(line.len());
            line.insert(col, ch);
            self.cursor_col += 1;
            self.validation_error = None;
        }
    }

    /// Delete character at cursor position (forward delete)
    pub fn delete_char(&mut self) {
        if self.cursor_row < self.lines.len() {
            let line = &mut self.lines[self.cursor_row];
            if self.cursor_col < line.len() {
                line.remove(self.cursor_col);
                self.validation_error = None;
            }
        }
    }

    /// Delete character before cursor (backspace)
    pub fn backspace(&mut self) {
        if self.cursor_row < self.lines.len() {
            let line = &mut self.lines[self.cursor_row];
            if self.cursor_col > 0 {
                line.remove(self.cursor_col - 1);
                self.cursor_col -= 1;
                self.validation_error = None;
            } else if self.cursor_row > 0 {
                // Join with previous line
                let current_line = self.lines.remove(self.cursor_row);
                self.cursor_row -= 1;
                if self.cursor_row < self.lines.len() {
                    self.cursor_col = self.lines[self.cursor_row].len();
                    self.lines[self.cursor_row].push_str(&current_line);
                    self.validation_error = None;
                }
            }
        }
    }

    /// Move cursor up one line
    pub fn cursor_up(&mut self) {
        if self.cursor_row > 0 {
            self.cursor_row -= 1;
            let line_len = self
                .lines
                .get(self.cursor_row)
                .map(|l| l.len())
                .unwrap_or(0);
            self.cursor_col = self.cursor_col.min(line_len);
            if self.cursor_row < self.scroll_offset {
                self.scroll_offset = self.cursor_row;
            }
        }
    }

    /// Move cursor down one line
    pub fn cursor_down(&mut self, visible_lines: usize) {
        if self.cursor_row < self.lines.len().saturating_sub(1) {
            self.cursor_row += 1;
            let line_len = self
                .lines
                .get(self.cursor_row)
                .map(|l| l.len())
                .unwrap_or(0);
            self.cursor_col = self.cursor_col.min(line_len);
            let max_scroll = self.lines.len().saturating_sub(visible_lines);
            if self.cursor_row >= self.scroll_offset + visible_lines {
                self.scroll_offset = (self.cursor_row + 1).saturating_sub(visible_lines);
            }
            self.scroll_offset = self.scroll_offset.min(max_scroll);
        }
    }

    /// Move cursor left
    pub fn cursor_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.cursor_col = self
                .lines
                .get(self.cursor_row)
                .map(|l| l.len())
                .unwrap_or(0);
        }
    }

    /// Move cursor right
    pub fn cursor_right(&mut self) {
        let line_len = self
            .lines
            .get(self.cursor_row)
            .map(|l| l.len())
            .unwrap_or(0);
        if self.cursor_col < line_len {
            self.cursor_col += 1;
        } else if self.cursor_row < self.lines.len().saturating_sub(1) {
            self.cursor_row += 1;
            self.cursor_col = 0;
        }
    }

    /// Move cursor to start of line
    pub fn cursor_home(&mut self) {
        self.cursor_col = 0;
    }

    /// Move cursor to end of line
    pub fn cursor_end(&mut self) {
        let line_len = self
            .lines
            .get(self.cursor_row)
            .map(|l| l.len())
            .unwrap_or(0);
        self.cursor_col = line_len;
    }

    /// Validate YAML content
    pub fn validate_yaml(&mut self) -> bool {
        let content = self.get_content();
        match serde_yaml::from_str::<serde_json::Value>(&content) {
            Ok(_) => {
                self.validation_error = None;
                true
            }
            Err(e) => {
                self.validation_error = Some(format!("YAML error: {}", e));
                false
            }
        }
    }

    /// Clear validation error (e.g., when user starts editing)
    #[allow(dead_code)]
    pub fn clear_error(&mut self) {
        self.validation_error = None;
    }
}

impl Default for EditorState {
    fn default() -> Self {
        Self::new("")
    }
}

/// Async operation state (pending operations and their result channels)
#[derive(Debug, Default)]
pub struct AsyncOperationState {
    // YAML fetch
    pub yaml_fetch_pending: Option<String>,
    pub yaml_fetched: Option<serde_json::Value>,
    pub yaml_fetch_rx: Option<tokio::sync::oneshot::Receiver<anyhow::Result<serde_json::Value>>>,

    // Describe fetch
    pub describe_fetch_pending: Option<String>,
    pub describe_fetched: Option<serde_json::Value>,
    pub describe_fetch_rx:
        Option<tokio::sync::oneshot::Receiver<anyhow::Result<serde_json::Value>>>,

    // Trace
    pub trace_pending: Option<ResourceKey>,
    pub trace_result: Option<crate::trace::TraceResult>,
    pub trace_result_rx:
        Option<tokio::sync::oneshot::Receiver<anyhow::Result<crate::trace::TraceResult>>>,

    // Graph
    pub graph_pending: Option<ResourceKey>,
    pub graph_result: Option<crate::trace::ResourceGraph>,
    pub graph_result_rx:
        Option<tokio::sync::oneshot::Receiver<anyhow::Result<crate::trace::ResourceGraph>>>,

    // Operations (suspend, resume, etc.)
    pub pending_operation: Option<PendingOperation>,
    pub operation_result_rx: Option<tokio::sync::oneshot::Receiver<anyhow::Result<()>>>,
    pub last_operation_key: Option<char>,
    pub confirmation_pending: Option<PendingOperation>,

    // Edit operations
    pub edit_pending: Option<ResourceKey>,
    pub edit_yaml: Option<String>,
    pub edit_save_pending: Option<serde_json::Value>,
    pub edit_save_result_rx: Option<tokio::sync::oneshot::Receiver<anyhow::Result<()>>>,
    pub edit_error_message: Option<String>,
    pub editor_state: Option<EditorState>,
}

impl AsyncOperationState {
    pub fn clear_pending(&mut self) {
        self.yaml_fetch_pending = None;
        self.yaml_fetched = None;
        self.yaml_fetch_rx = None;

        self.describe_fetch_pending = None;
        self.describe_fetched = None;
        self.describe_fetch_rx = None;

        self.trace_pending = None;
        self.trace_result = None;
        self.trace_result_rx = None;

        self.graph_pending = None;
        self.graph_result = None;
        self.graph_result_rx = None;

        self.pending_operation = None;
        self.operation_result_rx = None;
        self.last_operation_key = None;
        self.confirmation_pending = None;

        self.edit_pending = None;
        self.edit_yaml = None;
        self.edit_save_pending = None;
        self.edit_save_result_rx = None;
        self.edit_error_message = None;
        self.editor_state = None;
    }
}

/// Pending operation awaiting confirmation
#[derive(Clone, Debug)]
pub struct PendingOperation {
    pub resource_type: String,
    pub namespace: String,
    pub name: String,
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

/// Information about a Flux controller pod
#[derive(Clone, Debug)]
pub struct ControllerPodInfo {
    pub name: String,
    pub ready: bool,
    pub version: Option<String>,
}

/// State for Flux controller pod monitoring
#[derive(Debug, Default)]
pub struct ControllerPodState {
    pods: std::collections::HashMap<String, ControllerPodInfo>,
    last_updated: Option<std::time::Instant>,
    /// Flux bundle version from deployment label (e.g., "v2.7.5")
    flux_bundle_version: Option<String>,
}

impl ControllerPodState {
    /// Add or update a pod in the state
    pub fn upsert_pod(&mut self, name: String, info: ControllerPodInfo) {
        self.pods.insert(name, info);
        self.last_updated = Some(std::time::Instant::now());
    }

    /// Remove a pod from the state
    pub fn remove_pod(&mut self, name: &str) {
        self.pods.remove(name);
        self.last_updated = Some(std::time::Instant::now());
    }

    /// Get all pods
    pub fn get_all_pods(&self) -> Vec<ControllerPodInfo> {
        self.pods.values().cloned().collect()
    }

    /// Clear all pod state
    pub fn clear(&mut self) {
        self.pods.clear();
        self.last_updated = None;
        self.flux_bundle_version = None;
    }

    /// Set the Flux bundle version from deployment label
    pub fn set_flux_bundle_version(&mut self, version: Option<String>) {
        self.flux_bundle_version = version;
        self.last_updated = Some(std::time::Instant::now());
    }

    /// Get the Flux bundle version (e.g., "v2.7.5") from deployment labels
    /// This represents the overall Flux release version, not individual controller versions
    pub fn get_flux_version(&self) -> Option<&str> {
        self.flux_bundle_version.as_deref()
    }
}
