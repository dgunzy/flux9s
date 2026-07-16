//! Application state structures
//!
//! This module contains state sub-structures that organize the App's fields
//! into logical groupings for better maintainability and testability.

use crate::tui::app::async_task::AsyncTask;
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
    /// Live Kubernetes events feed, opened with `:events`. The events watcher
    /// runs only while this view (or a detail view opened from it) is active.
    EventList,
    /// Controller pod log viewer, opened with `:logs`. The log stream runs
    /// only while this view is active.
    Logs,
    /// Workloads of a graph WorkloadGroup node (#194): Enter opens a
    /// workload's detail. Back returns to the graph.
    WorkloadList,
    /// One workload's rollout status, containers, pods, and events (#194).
    /// Back returns to the workload list.
    WorkloadDetail,
    /// Cluster pulse dashboard (#195): per-kind health counts, recent
    /// failures, and FluxReport distribution info. Opened with `:pulse`.
    Pulse,
    #[allow(dead_code)] // Reserved for future alternative help view implementation
    Help,
}

impl View {
    /// Mutable line-scroll offset for the simple text/log views that scroll one
    /// line at a time. Returns `None` for views with bespoke navigation: lists
    /// use a selection index and the graph view uses node focus. This is the
    /// single place that maps a view to its scroll field, so the scroll handlers
    /// don't each repeat the per-view match.
    pub fn scroll_offset_mut(self, vs: &mut ViewState) -> Option<&mut usize> {
        match self {
            View::ResourceYAML => Some(&mut vs.yaml_scroll_offset),
            View::ResourceDescribe => Some(&mut vs.describe_scroll_offset),
            View::ResourceTrace => Some(&mut vs.trace_scroll_offset),
            View::ResourceHistory => Some(&mut vs.history_scroll_offset),
            View::Logs => Some(&mut vs.log_scroll_offset),
            View::WorkloadDetail => Some(&mut vs.workload_scroll_offset),
            View::Pulse => Some(&mut vs.pulse_scroll_offset),
            _ => None,
        }
    }

    /// Whether this is a list-style view (the main resource list or favorites)
    /// from which resources are selected and opened.
    pub fn is_list_view(self) -> bool {
        matches!(self, View::ResourceList | View::ResourceFavorites)
    }

    /// Whether `/` opens an in-view text search (YAML/describe/trace/logs)
    /// rather than the resource-list filter.
    pub fn is_text_search_view(self) -> bool {
        matches!(
            self,
            View::ResourceYAML
                | View::ResourceDescribe
                | View::ResourceTrace
                | View::Logs
                | View::WorkloadDetail
                | View::Pulse
        )
    }

    /// Whether this is a nested detail-style view opened from a list — i.e. one
    /// that Back/Esc should pop back to the list (or graph) rather than quit.
    pub fn is_nested_view(self) -> bool {
        matches!(
            self,
            View::ResourceDetail
                | View::ResourceDescribe
                | View::ResourceYAML
                | View::ResourceTrace
                | View::ResourceHistory
                | View::ResourceGraph
        )
    }
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

/// Sort field for the resource list (k9s-style shift-key sorting).
///
/// Cycle per key press: ascending → descending → back to `Default` ordering.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SortField {
    /// Default ordering: namespace, then type, then name
    #[default]
    Default,
    Name,
    Age,
    Type,
    Status,
}

impl SortField {
    /// Header column name this sort field corresponds to (for the sort arrow)
    pub fn column_name(&self) -> Option<&'static str> {
        match self {
            SortField::Default => None,
            SortField::Name => Some("NAME"),
            SortField::Age => Some("AGE"),
            SortField::Type => Some("TYPE"),
            SortField::Status => Some("STATUS"),
        }
    }

    /// Human-readable name for status messages
    pub fn display_name(&self) -> &'static str {
        match self {
            SortField::Default => "default",
            SortField::Name => "name",
            SortField::Age => "age",
            SortField::Type => "type",
            SortField::Status => "status",
        }
    }
}

/// Search state for text views (YAML, describe, trace).
///
/// Matches are recomputed each render (the views own the line data); the
/// event handler only moves `current_match` and requests a jump.
#[derive(Debug, Default, Clone)]
pub struct TextSearchState {
    /// Active search query (empty = no search)
    pub query: String,
    /// Whether the user is currently typing the query
    pub input_mode: bool,
    /// Index of the current match within the matches found at render time
    pub current_match: usize,
    /// Total matches found by the last render
    pub total_matches: usize,
    /// When true, the next render scrolls to the current match
    pub pending_jump: bool,
}

impl TextSearchState {
    /// Whether a search is active (query entered and applied)
    pub fn is_active(&self) -> bool {
        !self.query.is_empty()
    }

    /// Reset to the inactive state
    pub fn clear(&mut self) {
        *self = Self::default();
    }
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
    /// Scroll offset for the controller log view
    pub log_scroll_offset: usize,
    /// Scroll offset for the workload detail view
    pub workload_scroll_offset: usize,
    /// Scroll offset for the pulse dashboard
    pub pulse_scroll_offset: usize,
    /// Where Back from the log view returns to, when logs were opened from
    /// somewhere other than a root list view (e.g. a workload's pods).
    /// Consumed on Back; `None` falls back to `previous_list_view`.
    pub logs_back_view: Option<View>,
    /// Workload rows shown by the WorkloadList view, decoded from the graph
    /// WorkloadGroup node that was opened.
    pub workload_rows: Vec<crate::kube::workloads::WorkloadRef>,
    /// Scroll offset for graph view
    pub graph_scroll_offset: usize,
    /// Index (into the graph's node list) of the currently focused graph node.
    /// `None` until a graph is built; set to the object node when one loads.
    pub graph_focus_index: Option<usize>,
    /// Track previous list view for navigation (ResourceList or ResourceFavorites)
    pub previous_list_view: View,
    /// When the detail view was entered by drilling into a node from the graph,
    /// this holds `ResourceGraph` so that Back returns to the graph instead of
    /// all the way to the list. Consumed (taken) on the next Back.
    pub detail_back_view: Option<View>,
    /// Active submenu state (if a submenu is currently being displayed)
    pub submenu_state: Option<SubmenuState>,
    /// Original theme name when previewing themes in submenu (for restoration on cancel)
    pub preview_original_theme: Option<String>,
    /// Cached page size for PageUp/PageDown navigation (updated each render from content area height)
    pub page_size: usize,
    /// Active sort field for the resource list (Shift+N/A/T/S)
    pub sort_field: SortField,
    /// Whether the active sort is reversed (second press of the same key)
    pub sort_reverse: bool,
    /// Search state for text views (YAML, describe, trace)
    pub text_search: TextSearchState,
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
            log_scroll_offset: 0,
            workload_scroll_offset: 0,
            pulse_scroll_offset: 0,
            logs_back_view: None,
            workload_rows: Vec::new(),
            graph_scroll_offset: 0,
            graph_focus_index: None,
            previous_list_view: View::ResourceList,
            detail_back_view: None,
            submenu_state: None,
            preview_original_theme: None,
            page_size: 10,
            sort_field: SortField::default(),
            sort_reverse: false,
            text_search: TextSearchState::default(),
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

/// Async operation state: one [`AsyncTask`] slot per view fetch, plus the
/// mutation-operation flow (which carries confirmation state alongside).
#[derive(Debug, Default)]
pub struct AsyncOperationState {
    /// Full-object fetch backing the YAML view.
    pub yaml: AsyncTask<ResourceKey, serde_json::Value>,
    /// Object + events fetch backing the describe view.
    pub describe: AsyncTask<ResourceKey, crate::kube::fetch::DescribeData>,
    /// Ownership-chain trace backing the trace view.
    pub trace: AsyncTask<ResourceKey, crate::trace::TraceResult>,
    /// Relationship graph backing the graph view.
    pub graph: AsyncTask<ResourceKey, crate::trace::ResourceGraph>,
    /// Workload drill-down fetch backing the workload detail view (#194).
    pub workload: AsyncTask<ResourceKey, crate::kube::workloads::WorkloadData>,

    /// Mutating operation (suspend, resume, reconcile, delete). The result
    /// payload is `()`; success/failure feeds the status message.
    pub operation: AsyncTask<PendingOperation, ()>,
    /// Keybinding of the last dispatched operation, for the success message.
    pub last_operation_key: Option<char>,
    /// Operation waiting for the user's confirmation dialog.
    pub confirmation_pending: Option<PendingOperation>,
}

impl AsyncOperationState {
    pub fn clear_pending(&mut self) {
        self.yaml.clear();
        self.describe.clear();
        self.trace.clear();
        self.graph.clear();
        self.workload.clear();
        self.operation.clear();
        self.last_operation_key = None;
        self.confirmation_pending = None;
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

/// Bounded store for the live Kubernetes events feed.
///
/// Events are deduplicated by UID (the API server aggregates repeat
/// occurrences into one Event object with a rising `count`), and the store
/// evicts the oldest-seen entries past [`crate::constants::MAX_KUBE_EVENTS`].
#[derive(Debug, Default)]
pub struct KubeEventStore {
    events: std::collections::HashMap<String, crate::kube::events::KubeEventInfo>,
}

impl KubeEventStore {
    /// Insert or update an event by UID, evicting the oldest-seen entry when
    /// the store is over capacity.
    pub fn upsert(&mut self, info: crate::kube::events::KubeEventInfo) {
        self.events.insert(info.uid.clone(), info);
        while self.events.len() > crate::constants::MAX_KUBE_EVENTS {
            let oldest_uid = self
                .events
                .values()
                .min_by_key(|event| event.last_seen)
                .map(|event| event.uid.clone());
            match oldest_uid {
                Some(uid) => {
                    self.events.remove(&uid);
                }
                None => break,
            }
        }
    }

    /// Remove an event (deleted on the cluster, usually TTL expiry).
    pub fn remove(&mut self, uid: &str) {
        self.events.remove(uid);
    }

    /// All events, newest last-seen first.
    pub fn sorted_events(&self) -> Vec<&crate::kube::events::KubeEventInfo> {
        let mut events: Vec<_> = self.events.values().collect();
        events.sort_by(|a, b| b.last_seen.cmp(&a.last_seen).then(a.uid.cmp(&b.uid)));
        events
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    #[allow(dead_code)] // Used in tests
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Drop all events (e.g. on namespace or context switch — the restarted
    /// watcher re-lists the events in scope).
    pub fn clear(&mut self) {
        self.events.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_event(uid: &str, seconds_ago: i64) -> crate::kube::events::KubeEventInfo {
        crate::kube::events::KubeEventInfo::from_json(&serde_json::json!({
            "metadata": {
                "uid": uid,
                "namespace": "flux-system",
            },
            "involvedObject": {"kind": "Kustomization", "name": uid},
            "type": "Normal",
            "reason": "Test",
            "message": "test event",
            "lastTimestamp": (chrono::Utc::now() - chrono::Duration::seconds(seconds_ago))
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        }))
        .expect("test event should parse")
    }

    #[test]
    fn kube_event_store_dedups_by_uid_and_sorts_newest_first() {
        let mut store = KubeEventStore::default();
        store.upsert(make_event("older", 120));
        store.upsert(make_event("newer", 10));
        // Same UID again — an update, not a new entry
        store.upsert(make_event("older", 60));

        assert_eq!(store.len(), 2);
        let sorted = store.sorted_events();
        assert_eq!(sorted[0].uid, "newer");
        assert_eq!(sorted[1].uid, "older");

        store.remove("newer");
        assert_eq!(store.len(), 1);
        store.clear();
        assert!(store.is_empty());
    }

    #[test]
    fn kube_event_store_evicts_oldest_past_cap() {
        let mut store = KubeEventStore::default();
        for i in 0..crate::constants::MAX_KUBE_EVENTS {
            store.upsert(make_event(&format!("uid-{i}"), 1000 + i as i64));
        }
        assert_eq!(store.len(), crate::constants::MAX_KUBE_EVENTS);

        // One more (newest) evicts the oldest-seen entry, not the new one
        store.upsert(make_event("newest", 0));
        assert_eq!(store.len(), crate::constants::MAX_KUBE_EVENTS);
        assert_eq!(store.sorted_events()[0].uid, "newest");
        let oldest_uid = format!("uid-{}", crate::constants::MAX_KUBE_EVENTS - 1);
        assert!(
            !store.sorted_events().iter().any(|e| e.uid == oldest_uid),
            "oldest-seen event should have been evicted"
        );
    }

    #[test]
    fn view_classifiers_partition_views_as_expected() {
        assert!(View::ResourceList.is_list_view());
        assert!(View::ResourceFavorites.is_list_view());
        assert!(!View::ResourceGraph.is_list_view());

        assert!(View::ResourceYAML.is_text_search_view());
        assert!(View::ResourceDescribe.is_text_search_view());
        assert!(View::ResourceTrace.is_text_search_view());
        assert!(!View::ResourceHistory.is_text_search_view());
        assert!(!View::ResourceList.is_text_search_view());

        for v in [
            View::ResourceDetail,
            View::ResourceDescribe,
            View::ResourceYAML,
            View::ResourceTrace,
            View::ResourceHistory,
            View::ResourceGraph,
        ] {
            assert!(v.is_nested_view(), "{v:?} should be a nested view");
        }
        assert!(!View::ResourceList.is_nested_view());
        assert!(!View::ResourceFavorites.is_nested_view());

        // The events feed is a root-level view with selection-based
        // navigation: not nested, not a resource list, no line scroll.
        assert!(!View::EventList.is_nested_view());
        assert!(!View::EventList.is_list_view());
        assert!(!View::EventList.is_text_search_view());
        assert!(
            View::EventList
                .scroll_offset_mut(&mut ViewState::default())
                .is_none()
        );

        // The log view is a root-level text view: line-scrolled and
        // searchable with /, but not nested and not a resource list.
        assert!(!View::Logs.is_nested_view());
        assert!(!View::Logs.is_list_view());
        assert!(View::Logs.is_text_search_view());
        assert!(
            View::Logs
                .scroll_offset_mut(&mut ViewState::default())
                .is_some()
        );

        // The pulse dashboard behaves the same way: a root-level,
        // searchable, line-scrolled text view.
        assert!(!View::Pulse.is_nested_view());
        assert!(!View::Pulse.is_list_view());
        assert!(View::Pulse.is_text_search_view());
        assert!(
            View::Pulse
                .scroll_offset_mut(&mut ViewState::default())
                .is_some()
        );
    }

    #[test]
    fn scroll_offset_mut_targets_the_right_field() {
        let mut vs = ViewState::default();

        // Text/log views expose a line-scroll offset.
        *View::ResourceYAML.scroll_offset_mut(&mut vs).unwrap() = 7;
        assert_eq!(vs.yaml_scroll_offset, 7);
        *View::ResourceHistory.scroll_offset_mut(&mut vs).unwrap() = 3;
        assert_eq!(vs.history_scroll_offset, 3);

        // List and graph views have bespoke navigation, so no scroll offset.
        assert!(View::ResourceList.scroll_offset_mut(&mut vs).is_none());
        assert!(View::ResourceGraph.scroll_offset_mut(&mut vs).is_none());
    }
}
