//! The `:<kind>` browser (#267): opening kind lists, list history, and the
//! way home to the Flux view.
//!
//! Flux resources stay always-watched (the header, pulse, and favorites need
//! them); a kind list's watch runs only while that list is on screen. `Esc`
//! walks back through previously opened lists, and `:flux` / `:all` return
//! to the Flux list from anywhere.

use super::core::App;
use super::state::{ListTarget, View};
use crate::kube::kind_list::{KindListRequest, KindListSnapshot, KindRow};
use crate::kube::workloads::WorkloadAction;
use crate::models::kinds::{Capabilities, KindSpec};
use crate::tui::keybindings::NavigationCommand;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// A key a kind list offers when the selected kind has the capability.
///
/// This table is the single source for kind-list key dispatch **and** the
/// footer, so the two can't drift (the first slice of #276's action table).
struct KindAction {
    code: KeyCode,
    modifiers: KeyModifiers,
    /// Footer key label (`r`, `^d`, …).
    footer_key: &'static str,
    label: &'static str,
    applies: fn(&Capabilities) -> bool,
    run: fn(&mut App),
}

const KIND_ACTIONS: &[KindAction] = &[
    KindAction {
        code: KeyCode::Char('l'),
        modifiers: KeyModifiers::NONE,
        footer_key: "l",
        label: "Logs",
        applies: |caps| caps.pod_logs,
        run: App::kind_logs_selected,
    },
    KindAction {
        code: KeyCode::Char('r'),
        modifiers: KeyModifiers::NONE,
        footer_key: "r",
        label: "Restart",
        applies: |caps| caps.restart,
        run: App::kind_restart_selected,
    },
    KindAction {
        code: KeyCode::Char('d'),
        modifiers: KeyModifiers::CONTROL,
        footer_key: "^d",
        label: "Delete pod",
        applies: |caps| caps.pod_delete,
        run: App::kind_delete_pod_selected,
    },
];

/// Lists remembered for `Esc`.
const MAX_LIST_HISTORY: usize = 20;

/// Minimum gap between discovery re-checks triggered by unknown kinds.
const REDISCOVERY_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

impl App {
    /// The list on screen, as something `Esc` can return to.
    fn current_list_target(&self) -> Option<ListTarget> {
        match self.view_state.current_view {
            View::ResourceList => Some(ListTarget::Flux),
            View::KindList => self
                .async_state
                .kind_list
                .key()
                .map(|request| ListTarget::Kind(request.spec.clone())),
            _ => None,
        }
    }

    /// Open a live list of `spec`, remembering the current list for `Esc`.
    pub(crate) fn open_kind_list(&mut self, spec: KindSpec) {
        if !self.config.native_resources {
            self.set_status_message((
                "Browsing Kubernetes kinds is a preview — :native turns it on".to_string(),
                true,
            ));
            return;
        }
        let target = ListTarget::Kind(spec.clone());
        if let Some(current) = self.current_list_target()
            && current != target
        {
            self.view_state.list_history.push(current);
            if self.view_state.list_history.len() > MAX_LIST_HISTORY {
                self.view_state.list_history.remove(0);
            }
        }
        self.show_kind_list(spec);
    }

    /// Start (or restart) the list for `spec` in the current namespace.
    fn show_kind_list(&mut self, spec: KindSpec) {
        if self.view_state.current_view == View::EventList {
            self.stop_kube_events_watch();
        }
        let namespace = if spec.is_namespaced() {
            self.namespace.clone()
        } else {
            None
        };
        self.async_state
            .kind_list
            .request(KindListRequest { spec, namespace });
        self.view_state.kind_list_error = None;
        self.view_state.filter.clear();
        self.view_state.filter_mode = false;
        self.view_state.selected_index = 0;
        self.view_state.scroll_offset = 0;
        self.selection_state.native_object = None;
        self.view_state.current_view = View::KindList;
        self.invalidate_layout_cache();
    }

    /// `Esc` in a kind list: the previous list, else the Flux list.
    pub(crate) fn kind_list_back(&mut self) {
        match self.view_state.list_history.pop() {
            Some(ListTarget::Kind(spec)) => self.show_kind_list(spec),
            Some(ListTarget::Flux) | None => self.go_home_flux(),
        }
    }

    /// Return to the Flux list, dropping kind-list state and history.
    pub(crate) fn go_home_flux(&mut self) {
        self.async_state.kind_list.clear();
        self.view_state.list_history.clear();
        self.view_state.kind_list_error = None;
        if self.view_state.current_view == View::KindList {
            self.view_state.filter.clear();
            self.view_state.selected_index = 0;
            self.view_state.scroll_offset = 0;
        }
        self.view_state.current_view = View::ResourceList;
        self.invalidate_layout_cache();
    }

    /// Kind-list rows after the `/` filter (name or namespace substring).
    pub(crate) fn filtered_kind_rows(&self) -> Vec<&KindRow> {
        let filter = self.view_state.filter.to_lowercase();
        self.async_state
            .kind_list
            .result()
            .map(|snapshot| {
                snapshot
                    .rows
                    .iter()
                    .filter(|row| {
                        filter.is_empty()
                            || row.name.to_lowercase().contains(&filter)
                            || row.namespace.to_lowercase().contains(&filter)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Store a live snapshot, keeping the cursor on the same object. Rows
    /// are selected by index, so without this an insert above the cursor
    /// would silently move the selection — and `Enter` would open a
    /// different object than the one the user was looking at.
    pub(crate) fn on_kind_list_snapshot(&mut self, snapshot: KindListSnapshot) {
        let selected = self
            .filtered_kind_rows()
            .get(self.view_state.selected_index)
            .map(|row| (row.namespace.clone(), row.name.clone()));
        self.view_state.kind_list_error = None;
        self.async_state.kind_list.set_result(snapshot);

        let rows = self.filtered_kind_rows();
        let index = selected
            .and_then(|(ns, name)| {
                rows.iter()
                    .position(|r| r.namespace == ns && r.name == name)
            })
            .unwrap_or_else(|| {
                self.view_state
                    .selected_index
                    .min(rows.len().saturating_sub(1))
            });
        self.view_state.selected_index = index;
    }

    /// `Esc` in a kind list: clear an applied filter first (k9s-style), then
    /// walk back.
    pub(crate) fn kind_list_escape(&mut self) {
        if self.view_state.filter.is_empty() {
            self.kind_list_back();
        } else {
            self.view_state.filter.clear();
            self.view_state.selected_index = 0;
            self.view_state.scroll_offset = 0;
            self.invalidate_layout_cache();
        }
    }

    /// Restart the kind list after a namespace switch so it follows `:ns`.
    pub(crate) fn refresh_kind_list_namespace(&mut self) {
        if self.view_state.current_view != View::KindList {
            return;
        }
        if let Some(spec) = self
            .async_state
            .kind_list
            .key()
            .map(|request| request.spec.clone())
        {
            self.show_kind_list(spec);
        }
    }

    /// Header label while browsing a kind: `Cluster › deployments (27)`.
    pub(crate) fn browsing_label(&self) -> Option<String> {
        if self.view_state.current_view != View::KindList {
            return None;
        }
        let request = self.async_state.kind_list.key()?;
        let mut label = format!("Cluster › {}", request.spec.names.plural);
        if let Some(snapshot) = self.async_state.kind_list.result() {
            let shown = self.filtered_kind_rows().len();
            if shown == snapshot.rows.len() {
                label.push_str(&format!(" ({shown})"));
            } else {
                label.push_str(&format!(" ({shown} of {})", snapshot.rows.len()));
            }
        }
        if !self.view_state.filter.is_empty() {
            label.push_str(&format!("  name='{}'", self.view_state.filter));
        }
        Some(label)
    }

    /// Queue API discovery for the kind registry (on connect and context
    /// switch). The current catalog stays usable until the result replaces
    /// it — a context switch clears it beforehand so kinds never leak across
    /// clusters. Does nothing when `nativeResources` is off.
    pub(crate) fn request_kind_discovery(&mut self) {
        if self.config.native_resources {
            self.async_state.kind_discovery.request(());
            self.async_state.kind_discovery_at = Some(std::time::Instant::now());
        }
    }

    /// Re-run discovery for an unknown `:<kind>` (a CRD may have been
    /// installed since connecting), at most once per
    /// [`REDISCOVERY_INTERVAL`] so typos don't hammer the API. Returns
    /// whether a re-check was started.
    pub(crate) fn refresh_kind_discovery(&mut self) -> bool {
        let recent = self
            .async_state
            .kind_discovery_at
            .is_some_and(|at| at.elapsed() < REDISCOVERY_INTERVAL);
        if !self.config.native_resources || recent || self.async_state.kind_discovery.is_loading() {
            return false;
        }
        self.request_kind_discovery();
        true
    }

    /// Refresh the cluster-wide namespace list for the `:ns` picker. Skipped
    /// when `nativeResources` is off so that mode makes no extra calls.
    pub(crate) fn request_namespace_list(&mut self) {
        if self.config.native_resources && !self.async_state.namespace_list.is_loading() {
            self.async_state.namespace_list.request(());
        }
    }

    /// Capabilities of the kind on screen (view-only when none).
    fn kind_list_caps(&self) -> Capabilities {
        self.async_state
            .kind_list
            .key()
            .map_or(Capabilities::VIEW_ONLY, |r| r.spec.caps)
    }

    /// The selected row's `(kind, namespace, name)`.
    fn selected_kind_object(&self) -> Option<(String, String, String)> {
        let kind = self.async_state.kind_list.key()?.spec.gvk.kind.clone();
        let row = self
            .filtered_kind_rows()
            .get(self.view_state.selected_index)
            .map(|row| (row.namespace.clone(), row.name.clone()))?;
        Some((kind, row.0, row.1))
    }

    /// Run a kind-list action for `key`, if the selected kind offers one.
    /// Returns whether the key was handled.
    pub(crate) fn handle_kind_list_key(&mut self, key: KeyEvent) -> bool {
        let caps = self.kind_list_caps();
        let Some(action) = KIND_ACTIONS
            .iter()
            .find(|a| a.code == key.code && a.modifiers == key.modifiers && (a.applies)(&caps))
        else {
            return false;
        };
        (action.run)(self);
        true
    }

    /// Footer for the kind list: navigation plus the actions the kind offers.
    pub(crate) fn kind_list_footer(&self) -> Vec<NavigationCommand> {
        let caps = self.kind_list_caps();
        let describe = if caps.workload_detail {
            "Details"
        } else {
            "Describe"
        };
        let mut commands = vec![
            NavigationCommand::new("j/k ", "Navigate"),
            NavigationCommand::new("^f/^b", "PgDn/Up"),
            NavigationCommand::new("Enter", describe),
            NavigationCommand::new("d", "Describe"),
            NavigationCommand::new("y", "YAML"),
        ];
        commands.extend(
            KIND_ACTIONS
                .iter()
                .filter(|a| (a.applies)(&caps))
                .map(|a| NavigationCommand::new(a.footer_key, a.label)),
        );
        commands.extend([
            NavigationCommand::new("/", "Filter"),
            NavigationCommand::new(":flux", "Flux view"),
            NavigationCommand::new(":", "Command"),
            NavigationCommand::new("?", "Help"),
            NavigationCommand::new("Esc/q", "Back"),
        ]);
        commands
    }

    /// Enter on a kind row: workload kinds open the shared workload detail
    /// view (the same one the graph drill-down uses); others are described.
    pub(crate) fn kind_enter_selected(&mut self) {
        if !self.kind_list_caps().workload_detail {
            self.open_describe_view();
            return;
        }
        if let Some((kind, namespace, name)) = self.selected_kind_object() {
            self.open_workload_detail(kind, namespace, name, View::KindList, false);
        }
    }

    /// `l`: pods stream directly; workloads load their detail and continue
    /// into pod logs (container picker included), like the workload list.
    fn kind_logs_selected(&mut self) {
        let Some((kind, namespace, name)) = self.selected_kind_object() else {
            return;
        };
        if self.kind_list_caps().workload_detail {
            self.open_workload_detail(kind, namespace, name, View::KindList, true);
        } else {
            self.view_state.logs_back_view = Some(View::KindList);
            self.open_pod_logs(&namespace, &name);
        }
    }

    /// `r`: confirm a rollout restart of the selected workload.
    fn kind_restart_selected(&mut self) {
        if let Some((kind, namespace, name)) = self.selected_kind_object() {
            self.confirm_workload_action(WorkloadAction::Restart {
                kind,
                namespace,
                name,
            });
        }
    }

    /// `Ctrl+d`: confirm deleting the selected pod.
    fn kind_delete_pod_selected(&mut self) {
        if let Some((_, namespace, name)) = self.selected_kind_object() {
            self.confirm_workload_action(WorkloadAction::DeletePod { namespace, name });
        }
    }

    /// Discovery finished: open a kind command that was typed while it ran.
    /// Skipped if the user has since moved to a view it would yank them out
    /// of (only list views pick it up).
    pub(crate) fn on_kind_discovery_complete(&mut self) {
        let Some(token) = self.pending_kind_command.take() else {
            return;
        };
        let on_a_list = matches!(
            self.view_state.current_view,
            View::ResourceList | View::KindList | View::ResourceFavorites
        );
        match crate::models::kinds::resolve(&token) {
            Some(spec) if on_a_list && spec.family == crate::models::kinds::Family::Native => {
                self.open_kind_list(spec);
            }
            Some(_) => {}
            None => self.set_status_message((
                format!("Unknown command: '{token}'. Type :help for available commands"),
                true,
            )),
        }
    }

    /// Apply a Flux-adjacent CRD discovery event (#197). Ignored whenever
    /// discovery is off — including `nativeResources: false` — so events
    /// already queued when it was turned off can't resurrect kinds or start
    /// watches.
    pub(crate) fn apply_extra_kind_event(&mut self, event: crate::watcher::WatchEvent) {
        use crate::watcher::WatchEvent;
        if !self.config.flux_crd_discovery_enabled() {
            return;
        }
        match event {
            WatchEvent::ExtraKindDiscovered(extra) => {
                // Register (idempotent) and (re)start the dynamic watcher — a
                // no-op when already running, which self-heals after
                // namespace/context restarts.
                if crate::models::extra_kinds::global().insert(extra.clone()) {
                    tracing::info!(
                        "Discovered Flux-labeled kind {} ({}/{})",
                        extra.kind,
                        extra.group,
                        extra.version
                    );
                }
                if let Some(ref mut w) = self.watcher {
                    w.watch_extra(&extra);
                }
            }
            WatchEvent::ExtraKindRemoved(kind) => self.remove_extra_kind(&kind),
            _ => {}
        }
    }

    /// A discovered kind's CRD went away: unregister it, stop its watcher,
    /// and drop its rows.
    fn remove_extra_kind(&mut self, kind: &str) {
        if crate::models::extra_kinds::global().remove(kind).is_none() {
            return;
        }
        tracing::info!("Discovered kind {} removed (CRD deleted)", kind);
        if let Some(ref mut w) = self.watcher {
            w.stop_extra(kind);
        }
        self.purge_kind(kind);
    }
}
