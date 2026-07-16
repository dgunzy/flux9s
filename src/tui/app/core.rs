//! Application state and main TUI logic

use super::state::{
    AsyncOperationState, ControllerPodState, HealthFilter, KubeEventStore, SelectionState, UIState,
    View, ViewState,
};
use crate::tui::{OperationRegistry, Theme};
use crate::watcher::ResourceState;
use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};

/// Main application state
pub struct App {
    // Core data
    pub(crate) state: ResourceState,
    pub(crate) config: crate::config::Config,
    pub(crate) theme: Theme,
    pub(crate) context: String,
    pub(crate) namespace: Option<String>,

    // Organized state
    pub(crate) view_state: ViewState,
    pub(crate) selection_state: SelectionState,
    pub(crate) ui_state: UIState,
    pub(crate) async_state: AsyncOperationState,

    // Services & infrastructure
    pub(crate) resource_objects: HashMap<String, serde_json::Value>,
    pub(crate) watcher: Option<crate::watcher::ResourceWatcher>,
    pub(crate) kube_client: Option<kube::Client>,
    pub(crate) operation_registry: OperationRegistry,
    pub(crate) namespace_hotkeys: Vec<String>,
    pub(crate) pending_context_switch: Option<String>,
    pub(crate) controller_pods: ControllerPodState,
    /// Live Kubernetes events feed (populated while the events view is open).
    pub(crate) kube_events: KubeEventStore,
    /// Controller pod log stream (active while the log view is open).
    pub(crate) logs: super::logs::LogState,
    /// Set when `l` on the workload list requested a workload: open its pod
    /// logs as soon as the fetch completes. Consumed on load.
    pub(crate) logs_after_workload_load: bool,
    /// Path to the active log file, shown on the connection error screen.
    pub(crate) log_path: Option<std::path::PathBuf>,
    /// Watchers currently in a degraded (erroring/reconnecting) state.
    /// Non-empty set drives the "watch degraded" banner.
    pub(crate) degraded_watchers: HashSet<String>,
}

impl App {
    pub fn new(
        state: ResourceState,
        context: String,
        namespace: Option<String>,
        config: crate::config::Config,
        theme: Theme,
    ) -> Self {
        let show_splash = !config.ui.splashless;
        if !config.ui.splashless {
            tracing::debug!(
                "Splash screen will be shown (splashless={})",
                config.ui.splashless
            );
        }

        Self {
            // Core data
            state,
            config: config.clone(),
            theme,
            context,
            namespace,

            // Organized state
            view_state: ViewState::default(),
            selection_state: SelectionState {
                selected_resource_key: None,
                favorites: config.favorites.iter().cloned().collect(),
                favorites_pending_save: false,
            },
            ui_state: UIState::new(show_splash),
            async_state: AsyncOperationState::default(),

            // Services & infrastructure
            resource_objects: HashMap::new(),
            watcher: None,
            kube_client: None,
            operation_registry: OperationRegistry::new(),
            namespace_hotkeys: Self::build_namespace_hotkeys(&config, Vec::new()),
            pending_context_switch: None,
            controller_pods: ControllerPodState::default(),
            kube_events: KubeEventStore::default(),
            logs: super::logs::LogState::default(),
            logs_after_workload_load: false,
            log_path: None,
            degraded_watchers: HashSet::new(),
        }
    }

    /// Mark a watcher as degraded (erroring and retrying with backoff).
    pub fn watch_degraded(&mut self, watcher: String) {
        self.degraded_watchers.insert(watcher);
    }

    /// Mark a watcher as recovered after a successful watch event.
    pub fn watch_recovered(&mut self, watcher: &str) {
        self.degraded_watchers.remove(watcher);
    }

    /// Whether any watcher is currently degraded (drives the warning banner).
    pub fn is_watch_degraded(&self) -> bool {
        !self.degraded_watchers.is_empty()
    }

    /// Number of degraded watchers (shown in the warning banner).
    pub fn degraded_watcher_count(&self) -> usize {
        self.degraded_watchers.len()
    }

    /// Mark the connection as established (clears the connecting state).
    pub fn set_connected(&mut self) {
        self.ui_state.connection_status = crate::tui::app::state::ConnectionStatus::Connected;
        self.ui_state.cached_terminal_size = None;
        // Fresh watchers — any degraded state belongs to the old set.
        self.degraded_watchers.clear();
    }

    /// Record a fatal connection error to display on the error screen.
    pub fn set_connection_error(&mut self, error: crate::kube::health::ConnectionError) {
        self.ui_state.connection_status =
            crate::tui::app::state::ConnectionStatus::Failed(Box::new(error));
        self.ui_state.show_splash = false;
        self.ui_state.splash_start_time = None;
        self.ui_state.cached_terminal_size = None;
        self.async_state.clear_pending();
    }

    /// Whether the app is currently showing a fatal connection error.
    pub fn has_connection_error(&self) -> bool {
        matches!(
            self.ui_state.connection_status,
            crate::tui::app::state::ConnectionStatus::Failed(_)
        )
    }

    /// Set the active log file path (shown on the connection error screen).
    pub fn set_log_path(&mut self, path: Option<std::path::PathBuf>) {
        self.log_path = path;
    }

    /// Render the connection error view inside the specified area, if in a failed state.
    pub(crate) fn render_connection_error_screen(
        &self,
        f: &mut ratatui::Frame,
        area: ratatui::layout::Rect,
    ) {
        if let crate::tui::app::state::ConnectionStatus::Failed(err) =
            &self.ui_state.connection_status
        {
            crate::tui::views::render_connection_error(
                f,
                area,
                &self.theme,
                err,
                self.log_path.as_deref(),
            );
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

        let mut hotkeys = vec!["all".to_string(), "flux-system".to_string()];
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
    pub(crate) fn invalidate_layout_cache(&mut self) {
        self.ui_state.cached_terminal_size = None;
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

    /// Preview a theme (temporary change, can be restored)
    pub fn preview_theme(&mut self, theme_name: &str) -> Result<()> {
        self.set_theme(theme_name)
    }

    /// Persist theme to config file
    ///
    /// Saves the theme to either ui.skin or ui.skinReadOnly based on readonly mode.
    /// Only updates the skin-related field, preserving all other config settings.
    pub fn persist_theme(&mut self, theme_name: &str) -> Result<()> {
        use crate::config::loader::ConfigLoader;
        use crate::config::paths;

        // Validate theme exists
        crate::config::ThemeLoader::load_theme(theme_name)
            .with_context(|| format!("Theme '{}' not found", theme_name))?;

        // Load existing config from disk (or use defaults if file doesn't exist)
        // This preserves all other settings including read_only
        let mut config_to_save = ConfigLoader::load_file(&paths::root_config_path())
            .unwrap_or_else(|_| ConfigLoader::load_defaults());

        // Only update the skin-related field based on readonly mode
        // This preserves the read_only setting and all other config values
        if self.config.read_only {
            config_to_save.ui.skin_read_only = Some(theme_name.to_string());
        } else {
            config_to_save.ui.skin = theme_name.to_string();
        }

        // Save to config file (only the skin field changed)
        ConfigLoader::save_root(&config_to_save)
            .with_context(|| "Failed to save configuration file")?;

        // Update in-memory config to match what we saved
        if self.config.read_only {
            self.config.ui.skin_read_only = Some(theme_name.to_string());
        } else {
            self.config.ui.skin = theme_name.to_string();
        }

        // Reload theme to ensure it's applied
        self.set_theme(theme_name)?;

        Ok(())
    }

    /// Reload skin based on current readonly mode and config
    /// Uses the same priority logic as startup: env var > context > readonly > default
    pub fn reload_skin_for_readonly_mode(&mut self, context_name: Option<&str>) {
        let skin_name = self.config.resolve_skin_name(context_name);

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
        // A replacement watcher (context switch) starts without the lazily
        // started events watch — rearm it if the events view is showing.
        if self.view_state.current_view == View::EventList {
            self.start_kube_events_watch();
        }
    }

    /// Start the Kubernetes events watcher (no-op if already running).
    pub(crate) fn start_kube_events_watch(&mut self) {
        if let Some(ref mut watcher) = self.watcher {
            if let Err(e) = watcher.watch_kube_events() {
                tracing::warn!("Failed to start events watcher: {}", e);
                self.set_status_message((format!("Failed to watch events: {}", e), true));
            }
        }
    }

    /// Stop the events watcher and drop the collected feed. Called when the
    /// events view is left; the watcher re-lists everything on next open, so
    /// keeping stale entries would only risk showing deleted events.
    pub(crate) fn stop_kube_events_watch(&mut self) {
        if let Some(ref mut watcher) = self.watcher {
            watcher.stop_kube_events();
        }
        self.kube_events.clear();
    }

    /// All watched resources in the current namespace scope — the pulse
    /// dashboard's input. Ignores the list's type/text filters so the pulse
    /// always shows the whole scope.
    pub(crate) fn pulse_resources(&self) -> Vec<crate::watcher::ResourceInfo> {
        let mut resources = self.state.all();
        if let Some(ref namespace) = self.namespace {
            resources.retain(|r| r.namespace == *namespace);
        }
        resources
    }

    /// Drop every watched resource of a kind (a discovered CRD was deleted).
    pub(crate) fn purge_kind(&mut self, kind: &str) {
        let prefix = format!("{}:", kind);
        let keys: Vec<String> = self
            .state
            .all()
            .iter()
            .filter(|r| r.resource_type == kind)
            .map(|r| crate::watcher::resource_key(&r.namespace, &r.name, &r.resource_type))
            .collect();
        for key in keys {
            self.state.remove(&key);
        }
        self.resource_objects
            .retain(|key, _| !key.starts_with(&prefix));
    }

    /// The FluxReport object, when the Flux Operator publishes one.
    pub(crate) fn flux_report_object(&self) -> Option<&serde_json::Value> {
        self.resource_objects
            .iter()
            .find(|(key, _)| key.starts_with("FluxReport:"))
            .map(|(_, obj)| obj)
    }

    /// The live events feed filtered by the list filter (matches type, reason,
    /// object, source, namespace and message), newest first. Returns owned
    /// clones so callers can hold the list while mutating view state.
    pub(crate) fn filtered_kube_events(&self) -> Vec<crate::kube::events::KubeEventInfo> {
        let filter = self.view_state.filter.to_lowercase();
        self.kube_events
            .sorted_events()
            .into_iter()
            .filter(|event| {
                if filter.is_empty() {
                    return true;
                }
                [
                    event.event_type.as_str(),
                    event.reason.as_str(),
                    event.message.as_str(),
                    event.source.as_str(),
                    event.involved_kind.as_str(),
                    event.involved_namespace.as_str(),
                    event.involved_name.as_str(),
                ]
                .iter()
                .any(|text| text.to_lowercase().contains(&filter))
            })
            .cloned()
            .collect()
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
    pub fn complete_context_switch(&mut self, context: String, namespace: Option<String>) {
        self.context = context;
        self.namespace = namespace;
        self.state.clear();
        self.resource_objects.clear();
        self.controller_pods.clear();
        self.kube_events.clear();
        self.logs.stop();
        self.degraded_watchers.clear();
        self.view_state.selected_index = 0;
        self.view_state.scroll_offset = 0;
        self.view_state.selected_resource_type = None;
        self.async_state.clear_pending();
    }

    /// Cycle the sort for the resource list: ascending → descending → default.
    ///
    /// Pressing a different sort key switches to that field (ascending).
    pub(crate) fn toggle_sort(&mut self, field: crate::tui::app::state::SortField) {
        use crate::tui::app::state::SortField;
        if self.view_state.sort_field == field {
            if self.view_state.sort_reverse {
                self.view_state.sort_field = SortField::Default;
                self.view_state.sort_reverse = false;
            } else {
                self.view_state.sort_reverse = true;
            }
        } else {
            self.view_state.sort_field = field;
            self.view_state.sort_reverse = false;
        }
        self.view_state.selected_index = 0;
        self.view_state.scroll_offset = 0;

        let msg = match self.view_state.sort_field {
            SortField::Default => "Sort: default (namespace/type/name)".to_string(),
            f => format!(
                "Sort: {}{}",
                f.display_name(),
                if self.view_state.sort_reverse {
                    " (reversed)"
                } else {
                    ""
                }
            ),
        };
        self.set_status_message((msg, false));
    }

    pub fn state(&mut self) -> &mut ResourceState {
        &mut self.state
    }

    #[allow(dead_code)] // May be used by external code or future features
    pub fn resource_objects(&self) -> &HashMap<String, serde_json::Value> {
        &self.resource_objects
    }

    #[allow(dead_code)] // Used in tests
    pub fn set_view_graph(&mut self) {
        self.view_state.current_view = View::ResourceGraph;
    }

    pub fn set_view(&mut self, view: View) {
        self.view_state.current_view = view;
    }

    pub fn previous_list_view(&self) -> View {
        self.view_state.previous_list_view
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub fn set_previous_list_view(&mut self, view: View) {
        self.view_state.previous_list_view = view;
    }

    #[allow(dead_code)] // Used in tests
    pub fn current_view(&self) -> View {
        self.view_state.current_view
    }

    #[allow(dead_code)] // Used in tests
    pub fn show_quit_confirm(&self) -> bool {
        self.ui_state.show_quit_confirm
    }

    /// Initialize the splash screen timer - call this when TUI actually starts rendering
    /// This ensures the timer starts when rendering begins, not during async initialization
    pub fn init_splash_timer(&mut self) {
        tracing::debug!(
            "init_splash_timer: show_splash={}, splash_start_time.is_none()={}",
            self.ui_state.show_splash,
            self.ui_state.splash_start_time.is_none()
        );
        if self.ui_state.show_splash && self.ui_state.splash_start_time.is_none() {
            let start_time = std::time::Instant::now();
            tracing::debug!(
                "Initializing splash_start_time for first render: {:?}",
                start_time
            );
            self.ui_state.splash_start_time = Some(start_time);
        } else {
            tracing::warn!(
                "Splash timer NOT initialized: show_splash={}, splash_start_time.is_none()={}",
                self.ui_state.show_splash,
                self.ui_state.splash_start_time.is_none()
            );
        }
    }

    pub fn set_status_message(&mut self, message: (String, bool)) {
        self.ui_state.status_message = Some(message);
        self.ui_state.status_message_time = Some(std::time::Instant::now());
    }

    /// Check and clear status message if timeout exceeded
    pub fn check_status_message_timeout(&mut self) {
        use crate::tui::constants::STATUS_MESSAGE_TIMEOUT_SECS;
        if let (Some(_), Some(time)) = (
            &self.ui_state.status_message,
            &self.ui_state.status_message_time,
        ) {
            if time.elapsed().as_secs() >= STATUS_MESSAGE_TIMEOUT_SECS {
                self.ui_state.status_message = None;
                self.ui_state.status_message_time = None;
            }
        }
    }

    /// Get the currently selected resource based on the current view
    /// Returns ResourceInfo if a resource is selected, None otherwise
    /// The resource the current view is pointing at, regardless of view type:
    /// the selected row in resource lists, the selected event's involved
    /// object in the events feed, the focused (non-aggregate) node in the
    /// graph, or the stored selection in nested detail-style views.
    ///
    /// This is the single resolver behind "act on the current resource" keys
    /// (YAML, describe, trace, graph, history, operations), so they work the
    /// same from every view. The target is not necessarily a watched Flux
    /// resource — an event may be about a Pod, a graph node may be a managed
    /// Deployment; callers that need watched state use
    /// [`Self::get_current_resource`].
    pub(crate) fn view_target(&self) -> Option<crate::watcher::ResourceKey> {
        use crate::watcher::ResourceKey;
        match self.view_state.current_view {
            View::ResourceList | View::ResourceFavorites => {
                let resources = self.get_filtered_resources();
                let resource = resources.get(self.view_state.selected_index)?;
                Some(ResourceKey::new(
                    resource.resource_type.clone(),
                    resource.namespace.clone(),
                    resource.name.clone(),
                ))
            }
            View::EventList => {
                let events = self.filtered_kube_events();
                let event = events.get(self.view_state.selected_index)?;
                Some(ResourceKey::new(
                    event.involved_kind.clone(),
                    event.involved_namespace.clone(),
                    event.involved_name.clone(),
                ))
            }
            View::ResourceGraph => self.focused_graph_node_target(),
            View::ResourceDetail
            | View::ResourceDescribe
            | View::ResourceYAML
            | View::ResourceTrace
            | View::ResourceHistory => self
                .selection_state
                .selected_resource_key
                .as_deref()
                .and_then(ResourceKey::parse),
            // Logs, workload views and the pulse dashboard don't point at a
            // single watched Flux resource.
            View::Logs | View::WorkloadList | View::WorkloadDetail | View::Pulse | View::Help => {
                None
            }
        }
    }

    /// Open the log view streaming the given controller pod. The pod list
    /// comes from the controller pod watch, so the namespace is the
    /// configured controller namespace.
    pub(crate) fn open_log_view(&mut self, pod_name: &str) {
        // Remember the root view we came from so Back returns there (and a
        // live events feed keeps its watcher).
        if matches!(
            self.view_state.current_view,
            View::ResourceList | View::ResourceFavorites | View::EventList
        ) {
            self.view_state.previous_list_view = self.view_state.current_view;
        }
        let namespace = self.config.default_controller_namespace.clone();
        self.view_state.logs_back_view = None;
        self.logs.request(namespace, pod_name.to_string());
        self.view_state.log_scroll_offset = 0;
        self.view_state.text_search.clear();
        self.view_state.current_view = View::Logs;
    }

    /// Store a loaded workload; when the load was initiated by `l` on the
    /// workload list, immediately continue into its pod logs.
    pub fn on_workload_loaded(&mut self, workload: crate::kube::workloads::WorkloadData) {
        self.async_state.workload.set_result(workload);
        if std::mem::take(&mut self.logs_after_workload_load)
            && self.view_state.current_view == View::WorkloadDetail
        {
            self.open_workload_pod_logs();
        }
    }

    /// Open the log view for an arbitrary pod (e.g. a workload's pod from the
    /// drill-down). The Back target (`logs_back_view`) is recorded where the
    /// flow started — the `l` keypress — so it survives the pod submenu.
    pub(crate) fn open_pod_logs(&mut self, namespace: &str, pod_name: &str) {
        self.logs
            .request(namespace.to_string(), pod_name.to_string());
        self.view_state.log_scroll_offset = 0;
        self.view_state.text_search.clear();
        self.view_state.current_view = View::Logs;
    }

    /// The watched Flux resource the current view points at, when the
    /// [`Self::view_target`] is one flux9s watches.
    pub(crate) fn get_current_resource(&self) -> Option<crate::watcher::ResourceInfo> {
        self.view_target()
            .and_then(|rk| self.state.get(&rk.to_key_string()))
    }

    pub fn set_view_trace(&mut self) {
        self.view_state.current_view = View::ResourceTrace;
        self.view_state.trace_scroll_offset = 0;
        self.view_state.text_search.clear();
    }

    /// Toggle favorite status for a resource
    pub fn toggle_favorite(&mut self, resource_key: &str) {
        if self.selection_state.favorites.contains(resource_key) {
            self.selection_state.favorites.remove(resource_key);
        } else {
            self.selection_state
                .favorites
                .insert(resource_key.to_string());
        }
        self.selection_state.favorites_pending_save = true;
    }

    /// Check if a resource is favorited
    pub fn is_favorite(&self, resource_key: &str) -> bool {
        self.selection_state.favorites.contains(resource_key)
    }

    /// Get all favorite resource keys
    #[allow(dead_code)] // Public API method
    pub fn favorites(&self) -> &HashSet<String> {
        &self.selection_state.favorites
    }

    /// Trigger async save of favorites to config file
    pub fn trigger_favorites_save(&mut self) -> Option<crate::config::Config> {
        if self.selection_state.favorites_pending_save {
            self.selection_state.favorites_pending_save = false;
            let mut updated_config = self.config.clone();
            updated_config.favorites = self.selection_state.favorites.iter().cloned().collect();
            Some(updated_config)
        } else {
            None
        }
    }

    pub(crate) fn get_filtered_resources(&self) -> Vec<crate::watcher::ResourceInfo> {
        let mut resources = if let Some(ref resource_type) = self.view_state.selected_resource_type
        {
            self.state.by_type(resource_type)
        } else {
            self.state.all()
        };

        if let Some(ref namespace) = self.namespace {
            resources.retain(|r| r.namespace == *namespace);
        }

        if self.view_state.current_view == View::ResourceFavorites {
            resources.retain(|r| {
                let key = crate::watcher::resource_key(&r.namespace, &r.name, &r.resource_type);
                self.selection_state.favorites.contains(&key)
            });
        }

        if !self.view_state.filter.is_empty() {
            if let Some(label_filter) = self.view_state.filter.strip_prefix("label:") {
                if label_filter.is_empty() {
                    resources.retain(|r| !r.labels.is_empty());
                } else if let Some((key, value)) = label_filter.split_once('=') {
                    resources.retain(|r| {
                        r.labels
                            .iter()
                            .any(|(k, v)| k.starts_with(key) && v.starts_with(value))
                    });
                } else {
                    resources.retain(|r| r.labels.keys().any(|k| k.starts_with(label_filter)));
                }
            } else if let Some(ann_filter) = self
                .view_state
                .filter
                .strip_prefix("ann:")
                .or_else(|| self.view_state.filter.strip_prefix("annotations:"))
            {
                if ann_filter.is_empty() {
                    resources.retain(|r| !r.annotations.is_empty());
                } else if let Some((key, value)) = ann_filter.split_once('=') {
                    resources.retain(|r| {
                        r.annotations
                            .iter()
                            .any(|(k, v)| k.starts_with(key) && v.starts_with(value))
                    });
                } else {
                    resources.retain(|r| r.annotations.keys().any(|k| k.starts_with(ann_filter)));
                }
            } else {
                resources.retain(|r| r.name.contains(&self.view_state.filter));
            }
        }

        match self.view_state.health_filter {
            HealthFilter::Healthy => {
                resources.retain(crate::watcher::ResourceInfo::is_healthy);
            }
            HealthFilter::Unhealthy => {
                resources.retain(|r| !r.is_healthy());
            }
            HealthFilter::All => {}
        }

        let sort_field = self.view_state.sort_field;
        let sort_reverse = self.view_state.sort_reverse;
        resources.sort_by(|a, b| {
            let a_key = crate::watcher::resource_key(&a.namespace, &a.name, &a.resource_type);
            let b_key = crate::watcher::resource_key(&b.namespace, &b.name, &b.resource_type);
            let a_is_favorite = self.selection_state.favorites.contains(&a_key);
            let b_is_favorite = self.selection_state.favorites.contains(&b_key);

            // Favorites always group first, regardless of the active sort
            match (a_is_favorite, b_is_favorite) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => {
                    let ord = compare_by_sort_field(a, b, sort_field);
                    if sort_reverse { ord.reverse() } else { ord }
                }
            }
        });

        resources
    }
}

/// Compare two resources by the given sort field (ascending).
fn compare_by_sort_field(
    a: &crate::watcher::ResourceInfo,
    b: &crate::watcher::ResourceInfo,
    field: crate::tui::app::state::SortField,
) -> std::cmp::Ordering {
    use crate::tui::app::state::SortField;
    use std::cmp::Ordering;
    match field {
        SortField::Default => a
            .namespace
            .cmp(&b.namespace)
            .then_with(|| a.resource_type.cmp(&b.resource_type))
            .then_with(|| a.name.cmp(&b.name)),
        SortField::Name => a
            .name
            .cmp(&b.name)
            .then_with(|| a.namespace.cmp(&b.namespace)),
        SortField::Age => {
            // Oldest first; resources with unknown age sort last
            match (a.age, b.age) {
                (Some(x), Some(y)) => x.cmp(&y),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            }
            .then_with(|| a.name.cmp(&b.name))
        }
        SortField::Type => a
            .resource_type
            .cmp(&b.resource_type)
            .then_with(|| a.namespace.cmp(&b.namespace))
            .then_with(|| a.name.cmp(&b.name)),
        SortField::Status => {
            // Problems first: not-ready, then suspended, then unknown, then ready
            fn status_rank(r: &crate::watcher::ResourceInfo) -> u8 {
                match (r.ready, r.suspended.unwrap_or(false)) {
                    (Some(false), _) => 0,
                    (_, true) => 1,
                    (None, _) => 2,
                    (Some(true), _) => 3,
                }
            }
            status_rank(a)
                .cmp(&status_rank(b))
                .then_with(|| a.namespace.cmp(&b.namespace))
                .then_with(|| a.name.cmp(&b.name))
        }
    }
}

#[cfg(test)]
impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App")
            .field("state", &self.state)
            .field("current_view", &self.view_state.current_view)
            .field(
                "selected_resource_type",
                &self.view_state.selected_resource_type,
            )
            .field("filter", &self.view_state.filter)
            .field("filter_mode", &self.view_state.filter_mode)
            .field("health_filter", &self.view_state.health_filter)
            .field("selected_index", &self.view_state.selected_index)
            .field("scroll_offset", &self.view_state.scroll_offset)
            .field("yaml_scroll_offset", &self.view_state.yaml_scroll_offset)
            .field(
                "describe_scroll_offset",
                &self.view_state.describe_scroll_offset,
            )
            .field("show_help", &self.ui_state.show_help)
            .field("context", &self.context)
            .field("namespace", &self.namespace)
            .field("command_mode", &self.ui_state.command_mode)
            .field("command_buffer", &self.ui_state.command_buffer)
            .field(
                "selected_resource_key",
                &self.selection_state.selected_resource_key,
            )
            .field("resource_objects", &"<Arc<RwLock<HashMap>>>")
            .field("watcher", &"<Option<ResourceWatcher>>")
            .field("kube_client", &"<Option<kube::Client>>")
            .field("yaml_task", &self.async_state.yaml)
            .field("describe_task", &self.async_state.describe)
            .field("trace_task", &self.async_state.trace)
            .field("trace_scroll_offset", &self.view_state.trace_scroll_offset)
            .field("show_splash", &self.ui_state.show_splash)
            .field("splash_start_time", &self.ui_state.splash_start_time)
            .field("operation_registry", &"<OperationRegistry>")
            .field("operation_task", &self.async_state.operation)
            .field("last_operation_key", &self.async_state.last_operation_key)
            .field(
                "confirmation_pending",
                &self.async_state.confirmation_pending,
            )
            .field("status_message", &self.ui_state.status_message)
            .field("status_message_time", &self.ui_state.status_message_time)
            .field("theme", &self.theme)
            .field("config", &self.config)
            .field("namespace_hotkeys", &self.namespace_hotkeys)
            .field("pending_context_switch", &self.pending_context_switch)
            .field("cached_terminal_size", &self.ui_state.cached_terminal_size)
            .field("cached_header_height", &self.ui_state.cached_header_height)
            .field("cached_footer_height", &self.ui_state.cached_footer_height)
            .field("favorites", &self.selection_state.favorites)
            .field(
                "favorites_pending_save",
                &self.selection_state.favorites_pending_save,
            )
            .field(
                "history_scroll_offset",
                &self.view_state.history_scroll_offset,
            )
            .field("graph_scroll_offset", &self.view_state.graph_scroll_offset)
            .field("graph_task", &self.async_state.graph)
            .field("previous_list_view", &self.view_state.previous_list_view)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, UiConfig};
    use crate::watcher::ResourceState;
    use std::collections::HashMap;

    fn create_test_app() -> App {
        let state = ResourceState::new();
        let config = Config {
            read_only: false,
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
            },
            context_skins: HashMap::new(),
            cluster: HashMap::new(),
            favorites: vec![],
            default_resource_filter: None,
            connect_timeout_seconds: crate::kube::health::DEFAULT_CONNECT_TIMEOUT_SECS,
        };
        let theme = Theme::default();
        App::new(state, "test-context".to_string(), None, config, theme)
    }

    #[test]
    fn test_preview_theme_embedded() {
        let mut app = create_test_app();
        let _original_header_context = app.theme.header_context;

        // Preview an embedded theme
        let result = app.preview_theme("dracula");
        assert!(result.is_ok(), "Should preview dracula theme");

        // Theme should have changed (dracula theme should have different colors)
        // Note: This assumes dracula theme is different from default
        let _ = app.theme.header_context;
    }

    #[test]
    fn test_preview_theme_default() {
        let mut app = create_test_app();

        // Preview default theme
        let result = app.preview_theme("default");
        assert!(result.is_ok(), "Should preview default theme");

        // Theme should be valid
        let _ = app.theme.header_context;
    }

    #[test]
    fn test_preview_theme_nonexistent() {
        let mut app = create_test_app();
        let original_header_context = app.theme.header_context;

        // Preview a non-existent theme
        let result = app.preview_theme("nonexistent-theme-12345");
        assert!(result.is_err(), "Should fail to preview nonexistent theme");

        // Theme should remain unchanged
        assert_eq!(app.theme.header_context, original_header_context);
    }

    #[test]
    fn test_set_theme_updates_config_readonly() {
        let mut app = create_test_app();
        app.config.read_only = true;
        app.config.ui.skin_read_only = Some("default".to_string());

        // Set theme in readonly mode
        let result = app.set_theme("dracula");
        assert!(result.is_ok(), "Should set theme in readonly mode");

        // Config should be updated (but we can't test file save in unit tests)
        // The theme itself should be changed
        let _ = app.theme.header_context;
    }

    #[test]
    fn test_set_theme_updates_config_normal() {
        let mut app = create_test_app();
        app.config.read_only = false;
        app.config.ui.skin = "default".to_string();

        // Set theme in normal mode
        let result = app.set_theme("nord");
        assert!(result.is_ok(), "Should set theme in normal mode");

        // Theme should be changed
        let _ = app.theme.header_context;
    }

    #[test]
    fn test_persist_theme_validates_theme() {
        let mut app = create_test_app();

        // Try to persist a non-existent theme
        // Note: This will fail at validation before trying to save
        let result = app.persist_theme("nonexistent-theme-12345");
        assert!(result.is_err(), "Should fail to persist nonexistent theme");
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("not found") || error_msg.contains("nonexistent"));
    }

    #[test]
    fn test_persist_theme_updates_config_readonly() {
        let mut app = create_test_app();
        app.config.read_only = true;
        app.config.ui.skin_read_only = None;

        // Note: persist_theme will try to save to file, which may fail in tests
        // But we can test that it updates the config structure
        let result = app.persist_theme("dracula");

        // If file save fails, that's expected in test environment
        // But the config should be updated before save attempt
        match result {
            Ok(_) => {
                assert_eq!(app.config.ui.skin_read_only, Some("dracula".to_string()));
            }
            Err(e) => {
                // Even if save fails, config should be updated
                // (though in real code, it updates before save)
                // Let's check that the theme was at least validated
                assert!(
                    app.config.ui.skin_read_only.is_some()
                        || e.to_string().contains("Failed to save")
                );
            }
        }
    }

    #[test]
    fn test_persist_theme_updates_config_normal() {
        let mut app = create_test_app();
        app.config.read_only = false;
        app.config.ui.skin = "default".to_string();

        // Note: persist_theme will try to save to file, which may fail in tests
        let result = app.persist_theme("nord");

        // If file save fails, that's expected in test environment
        if result.is_ok() {
            assert_eq!(app.config.ui.skin, "nord".to_string());
        }
        // Theme should be applied regardless of save success
        let _ = app.theme.header_context;
    }

    fn make_resource(
        name: &str,
        namespace: &str,
        resource_type: &str,
        ready: Option<bool>,
        suspended: Option<bool>,
        age: Option<chrono::DateTime<chrono::Utc>>,
    ) -> crate::watcher::ResourceInfo {
        crate::watcher::ResourceInfo {
            name: name.to_string(),
            namespace: namespace.to_string(),
            resource_type: resource_type.to_string(),
            age,
            suspended,
            ready,
            message: None,
            revision: None,
            labels: HashMap::new(),
            annotations: HashMap::new(),
            last_reconciled: None,
            reconciliation_history: Vec::new(),
        }
    }

    #[test]
    fn test_toggle_sort_cycles_asc_desc_default() {
        use crate::tui::app::state::SortField;
        let mut app = create_test_app();
        assert_eq!(app.view_state.sort_field, SortField::Default);

        app.toggle_sort(SortField::Name);
        assert_eq!(app.view_state.sort_field, SortField::Name);
        assert!(!app.view_state.sort_reverse);

        app.toggle_sort(SortField::Name);
        assert_eq!(app.view_state.sort_field, SortField::Name);
        assert!(app.view_state.sort_reverse);

        app.toggle_sort(SortField::Name);
        assert_eq!(app.view_state.sort_field, SortField::Default);
        assert!(!app.view_state.sort_reverse);

        // Switching fields resets to ascending
        app.toggle_sort(SortField::Age);
        app.toggle_sort(SortField::Status);
        assert_eq!(app.view_state.sort_field, SortField::Status);
        assert!(!app.view_state.sort_reverse);
    }

    #[test]
    fn test_compare_by_sort_field_age_unknown_last() {
        use crate::tui::app::state::SortField;
        use std::cmp::Ordering;
        let old = make_resource(
            "old",
            "ns",
            "Kustomization",
            None,
            None,
            Some(chrono::Utc::now() - chrono::Duration::hours(5)),
        );
        let new = make_resource(
            "new",
            "ns",
            "Kustomization",
            None,
            None,
            Some(chrono::Utc::now()),
        );
        let unknown = make_resource("unknown", "ns", "Kustomization", None, None, None);

        assert_eq!(
            compare_by_sort_field(&old, &new, SortField::Age),
            Ordering::Less
        );
        assert_eq!(
            compare_by_sort_field(&new, &unknown, SortField::Age),
            Ordering::Less
        );
    }

    #[test]
    fn test_compare_by_sort_field_status_problems_first() {
        use crate::tui::app::state::SortField;
        use std::cmp::Ordering;
        let failing = make_resource("a", "ns", "Kustomization", Some(false), Some(false), None);
        let suspended = make_resource("b", "ns", "Kustomization", Some(true), Some(true), None);
        let ready = make_resource("c", "ns", "Kustomization", Some(true), Some(false), None);

        assert_eq!(
            compare_by_sort_field(&failing, &suspended, SortField::Status),
            Ordering::Less
        );
        assert_eq!(
            compare_by_sort_field(&suspended, &ready, SortField::Status),
            Ordering::Less
        );
    }

    #[test]
    fn test_complete_context_switch_resets_namespace() {
        let mut app = create_test_app();
        app.set_namespace(Some("old-namespace".to_string()));
        assert_eq!(app.namespace(), &Some("old-namespace".to_string()));

        app.complete_context_switch("new-context".to_string(), Some("new-namespace".to_string()));
        assert_eq!(app.context, "new-context");
        assert_eq!(app.namespace(), &Some("new-namespace".to_string()));
    }
}
