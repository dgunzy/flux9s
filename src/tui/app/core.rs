//! Application state and main TUI logic

use super::state::{
    AsyncOperationState, ControllerPodState, HealthFilter, SelectionState, UIState, View, ViewState,
};
use crate::tui::{OperationRegistry, Theme};
use crate::watcher::ResourceState;
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

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
    pub(crate) resource_objects: Arc<RwLock<HashMap<String, serde_json::Value>>>,
    pub(crate) watcher: Option<crate::watcher::ResourceWatcher>,
    pub(crate) kube_client: Option<kube::Client>,
    pub(crate) operation_registry: OperationRegistry,
    pub(crate) namespace_hotkeys: Vec<String>,
    pub(crate) pending_context_switch: Option<String>,
    pub(crate) controller_pods: Arc<RwLock<ControllerPodState>>,
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
            resource_objects: Arc::new(RwLock::new(HashMap::new())),
            watcher: None,
            kube_client: None,
            operation_registry: OperationRegistry::new(),
            namespace_hotkeys: Self::build_namespace_hotkeys(&config, Vec::new()),
            pending_context_switch: None,
            controller_pods: Arc::new(RwLock::new(ControllerPodState::default())),
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

    /// Reload skin based on current readonly mode and config
    /// Uses the same priority logic as startup: env var > context > readonly > default
    pub fn reload_skin_for_readonly_mode(&mut self, context_name: Option<&str>) {
        let skin_name = if let Ok(env_skin) = std::env::var("FLUX9S_SKIN") {
            tracing::debug!(
                "Using skin from FLUX9S_SKIN environment variable: {}",
                env_skin
            );
            env_skin
        } else if let Some(context) = context_name {
            if let Some(context_skin) = self.config.context_skins.get(context) {
                tracing::debug!(
                    "Using context-specific skin for '{}': {}",
                    context,
                    context_skin
                );
                context_skin.clone()
            } else if self.config.read_only && self.config.ui.skin_read_only.is_some() {
                let skin = self.config.ui.skin_read_only.as_ref().unwrap();
                tracing::debug!("Using readonly-specific skin: {}", skin);
                skin.clone()
            } else {
                tracing::debug!("Using default skin: {}", self.config.ui.skin);
                self.config.ui.skin.clone()
            }
        } else if self.config.read_only && self.config.ui.skin_read_only.is_some() {
            let skin = self.config.ui.skin_read_only.as_ref().unwrap();
            tracing::debug!("Using readonly-specific skin: {}", skin);
            skin.clone()
        } else {
            tracing::debug!("Using default skin: {}", self.config.ui.skin);
            self.config.ui.skin.clone()
        };

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
    pub fn complete_context_switch(&mut self, context: String) {
        self.context = context;
        self.state.clear();
        {
            let mut objects = self.resource_objects.write().unwrap();
            objects.clear();
        }
        {
            let mut controller_pods = self.controller_pods.write().unwrap();
            controller_pods.clear();
        }
        self.view_state.selected_index = 0;
        self.view_state.scroll_offset = 0;
        self.view_state.selected_resource_type = None;
    }

    pub fn state(&mut self) -> &mut ResourceState {
        &mut self.state
    }

    pub fn resource_objects(&self) -> &Arc<RwLock<HashMap<String, serde_json::Value>>> {
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
    pub fn set_previous_list_view(&mut self, view: View) {
        self.view_state.previous_list_view = view;
    }

    #[allow(dead_code)] // Used in tests
    pub fn current_view(&self) -> View {
        self.view_state.current_view
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

    pub fn set_view_trace(&mut self) {
        self.view_state.current_view = View::ResourceTrace;
        self.view_state.trace_scroll_offset = 0;
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
                resources.retain(|r| {
                    let is_ready = r.ready.unwrap_or(true);
                    let is_suspended = r.suspended.unwrap_or(false);
                    is_ready && !is_suspended
                });
            }
            HealthFilter::Unhealthy => {
                resources.retain(|r| {
                    let is_ready = r.ready.unwrap_or(true);
                    let is_suspended = r.suspended.unwrap_or(false);
                    !is_ready || is_suspended
                });
            }
            HealthFilter::All => {}
        }

        resources.sort_by(|a, b| {
            let a_key = crate::watcher::resource_key(&a.namespace, &a.name, &a.resource_type);
            let b_key = crate::watcher::resource_key(&b.namespace, &b.name, &b.resource_type);
            let a_is_favorite = self.selection_state.favorites.contains(&a_key);
            let b_is_favorite = self.selection_state.favorites.contains(&b_key);

            match (a_is_favorite, b_is_favorite) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a
                    .namespace
                    .cmp(&b.namespace)
                    .then_with(|| a.resource_type.cmp(&b.resource_type))
                    .then_with(|| a.name.cmp(&b.name)),
            }
        });

        resources
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
            .field("yaml_fetch_pending", &self.async_state.yaml_fetch_pending)
            .field("yaml_fetched", &self.async_state.yaml_fetched.is_some())
            .field("yaml_fetch_rx", &self.async_state.yaml_fetch_rx.is_some())
            .field("trace_pending", &self.async_state.trace_pending)
            .field("trace_result", &self.async_state.trace_result.is_some())
            .field(
                "trace_result_rx",
                &self.async_state.trace_result_rx.is_some(),
            )
            .field("trace_scroll_offset", &self.view_state.trace_scroll_offset)
            .field("show_splash", &self.ui_state.show_splash)
            .field("splash_start_time", &self.ui_state.splash_start_time)
            .field("operation_registry", &"<OperationRegistry>")
            .field("pending_operation", &self.async_state.pending_operation)
            .field(
                "operation_result_rx",
                &self.async_state.operation_result_rx.is_some(),
            )
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
            .field("graph_pending", &self.async_state.graph_pending)
            .field("graph_result", &self.async_state.graph_result.is_some())
            .field(
                "graph_result_rx",
                &self.async_state.graph_result_rx.is_some(),
            )
            .field("previous_list_view", &self.view_state.previous_list_view)
            .finish()
    }
}
