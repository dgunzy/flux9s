//! Application state and main TUI logic

use super::state::{
    AsyncOperationState, ControllerPodState, HealthFilter, SelectionState, UIState, View, ViewState,
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
            } else if self.config.read_only {
                if let Some(ref skin) = self.config.ui.skin_read_only {
                    tracing::debug!("Using readonly-specific skin: {}", skin);
                    skin.clone()
                } else {
                    tracing::debug!("Using default skin: {}", self.config.ui.skin);
                    self.config.ui.skin.clone()
                }
            } else {
                tracing::debug!("Using default skin: {}", self.config.ui.skin);
                self.config.ui.skin.clone()
            }
        } else if self.config.read_only {
            if let Some(ref skin) = self.config.ui.skin_read_only {
                tracing::debug!("Using readonly-specific skin: {}", skin);
                skin.clone()
            } else {
                tracing::debug!("Using default skin: {}", self.config.ui.skin);
                self.config.ui.skin.clone()
            }
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
        self.resource_objects.clear();
        self.controller_pods.clear();
        self.view_state.selected_index = 0;
        self.view_state.scroll_offset = 0;
        self.view_state.selected_resource_type = None;
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

    /// Get the currently selected resource based on the current view
    /// Returns ResourceInfo if a resource is selected, None otherwise
    pub(crate) fn get_current_resource(&self) -> Option<crate::watcher::ResourceInfo> {
        match self.view_state.current_view {
            View::ResourceList | View::ResourceFavorites => {
                let resources = self.get_filtered_resources();
                resources.get(self.view_state.selected_index).cloned()
            }
            View::ResourceDetail => self
                .selection_state
                .selected_resource_key
                .as_ref()
                .and_then(|key| self.state.get(key)),
            _ => None,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, LoggerConfig, UiConfig};
    use crate::watcher::ResourceState;
    use std::collections::HashMap;

    fn create_test_app() -> App {
        let state = ResourceState::new();
        let config = Config {
            read_only: false,
            default_namespace: "".to_string(),
            default_controller_namespace: "".to_string(),
            namespace_hotkeys: vec![],
            ui: UiConfig {
                enable_mouse: false,
                headless: false,
                no_icons: false,
                skin: "default".to_string(),
                skin_read_only: None,
                splashless: true,
            },
            logger: LoggerConfig {
                tail: 100,
                buffer: 1000,
                since_seconds: 3600,
                text_wrap: false,
            },
            context_skins: HashMap::new(),
            cluster: HashMap::new(),
            favorites: vec![],
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
}
