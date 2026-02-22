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
}

impl UIState {
    pub fn new(show_splash: bool) -> Self {
        Self {
            command_mode: false,
            command_buffer: String::new(),
            show_help: false,
            status_message: None,
            status_message_time: None,
            show_splash,
            splash_start_time: None,
            cached_terminal_size: None,
            cached_header_height: crate::tui::constants::MIN_HEADER_HEIGHT,
            cached_footer_height: crate::tui::constants::MIN_FOOTER_HEIGHT,
        }
    }
}

/// Async operation state (pending operations and their result channels)
#[derive(Debug, Default)]
pub struct AsyncOperationState {
    // YAML fetch
    pub yaml_fetch_pending: Option<String>,
    pub yaml_fetched: Option<serde_json::Value>,
    pub yaml_fetch_rx: Option<tokio::sync::oneshot::Receiver<anyhow::Result<serde_json::Value>>>,

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
