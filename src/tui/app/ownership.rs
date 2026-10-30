//! `:unmanaged`, `:orphaned`, and `O` (#274): what Flux doesn't manage.
//!
//! A sweep lists every discovered kind once (see `kube::sweep`), so it needs
//! kind discovery — the `nativeResources` preview. It isn't live: the list
//! shows the sweep's result, and `Ctrl-r` runs it again.

use super::core::App;
use super::state::View;
use crate::kube::sweep::{self, SweepRequest, SweepRow, SweepScope, SweepShow};
use crate::tui::commands;
use crate::watcher::ResourceKey;

impl App {
    /// `:unmanaged [-n <ns> | -A]` (`all` still accepted).
    pub(super) fn cmd_unmanaged(&mut self, cmd: &str) {
        match commands::parse_sweep_args(cmd) {
            // `all` is accepted for compatibility: everything is the default.
            Ok(args) if args.target.is_none() => {
                self.start_sweep(SweepScope::NotFlux, SweepShow::All, args.namespace, false)
            }
            Ok(_) => self.set_status_message((
                ":orphaned <kind>/<name> lists what one Flux object left behind".to_string(),
                true,
            )),
            Err(message) => self.set_status_message((message, true)),
        }
    }

    /// `:orphaned [<kind>/<name>] [-n <ns> | -A]`.
    pub(super) fn cmd_orphaned(&mut self, cmd: &str) {
        let args = match commands::parse_sweep_args(cmd) {
            Ok(args) if !args.include_other => args,
            Ok(_) => {
                self.set_status_message((commands::SWEEP_USAGE.to_string(), true));
                return;
            }
            Err(message) => {
                self.set_status_message((message, true));
                return;
            }
        };
        let Some((kind_token, name)) = args.target else {
            self.start_sweep(
                SweepScope::NotFlux,
                SweepShow::Orphaned,
                args.namespace,
                false,
            );
            return;
        };
        let kind = crate::models::kinds::resolve(&kind_token)
            .map(|spec| spec.names.kind)
            .filter(|kind| sweep::can_orphan(kind));
        let Some(kind) = kind else {
            self.set_status_message((
                format!(
                    "'{kind_token}' can't leave orphans — Kustomizations, HelmReleases, ResourceSets, and FluxInstances can"
                ),
                true,
            ));
            return;
        };
        let namespace = match args.namespace {
            Some(Some(namespace)) => namespace,
            _ => match self.flux_object_namespace(&kind, &name) {
                Ok(namespace) => namespace,
                Err(message) => {
                    self.set_status_message((message, true));
                    return;
                }
            },
        };
        self.start_sweep(
            SweepScope::OrphansOf(ResourceKey::new(kind, namespace, name)),
            SweepShow::Orphaned,
            None,
            false,
        );
    }

    /// The namespace of the Flux object `kind`/`name`: the selected
    /// namespace, else the one watched object of that name.
    pub(super) fn flux_object_namespace(&self, kind: &str, name: &str) -> Result<String, String> {
        if let Some(namespace) = self.namespace() {
            return Ok(namespace.clone());
        }
        let mut found = self
            .state
            .all()
            .into_iter()
            .filter(|r| r.resource_type == kind && r.name == name)
            .map(|r| r.namespace);
        match (found.next(), found.next()) {
            (Some(namespace), None) => Ok(namespace),
            (None, _) => Err(format!("No {kind} named {name} — add -n <namespace>")),
            (Some(_), Some(_)) => Err(format!(
                "{kind} {name} is in several namespaces — add -n <namespace>"
            )),
        }
    }

    /// `O`: what the selected Flux object left behind.
    pub(super) fn orphans_of_selected(&mut self) {
        match self.view_target() {
            Some(key) if sweep::can_orphan(&key.resource_type) => {
                self.start_sweep(SweepScope::OrphansOf(key), SweepShow::Orphaned, None, true);
            }
            Some(key) => self.set_status_message((
                format!(
                    "A {} doesn't apply objects — O works on Kustomizations, HelmReleases, ResourceSets, and FluxInstances",
                    key.resource_type
                ),
                true,
            )),
            None => self.set_status_message(("No resource selected".to_string(), true)),
        }
    }

    /// `Ctrl-r`: run the current sweep again, for the namespace the list
    /// shows. The rows stay on screen until the new ones arrive.
    pub(super) fn rescan_sweep(&mut self) {
        if !self.config.native_resources {
            return; // `:native` off: no discovery, no sweeps
        }
        let Some(mut request) = self.view_state.sweep_request.clone() else {
            return;
        };
        if request.scope == SweepScope::NotFlux {
            request.namespace = self.view_state.sweep_namespace.clone();
        }
        self.view_state.sweep_reselect = self.selected_sweep_row().map(SweepRow::key);
        self.view_state.sweep_request = Some(request.clone());
        self.async_state.sweep.refresh(request);
    }

    /// `:ns` while a sweep is open: show the new namespace — from the rows
    /// already scanned when the sweep covers it (a sweep of every
    /// namespace covers each one), else by sweeping it.
    pub(super) fn follow_sweep_namespace(&mut self) {
        let Some(request) = self.view_state.sweep_request.as_ref() else {
            return;
        };
        let namespace = self.namespace().clone();
        self.view_state.sweep_namespace = namespace.clone();
        if request.covers(namespace.as_deref()) || !self.config.native_resources {
            return;
        }
        let request = SweepRequest {
            namespace,
            ..request.clone()
        };
        self.view_state.sweep_request = Some(request.clone());
        self.view_state.sweep_reselect = None;
        self.async_state.sweep.request(request);
    }

    /// `o` / `u` / `a`: switch which standings the list shows — no
    /// rescan, the sweep already has them. The cursor stays on the same
    /// object when it's still shown.
    pub(super) fn set_sweep_show(&mut self, show: SweepShow) {
        if self.view_state.sweep_show == show {
            return;
        }
        let selected = self.selected_sweep_row().map(SweepRow::key);
        self.view_state.sweep_show = show;
        self.reselect_sweep_row(selected);
    }

    /// Whether `o` / `u` / `a` apply: on the sweep list, for a whole-cluster
    /// sweep — a per-object one only has that object's orphans.
    pub(crate) fn sweep_can_switch_show(&self) -> bool {
        self.view_state.current_view == View::OwnershipList
            && self
                .view_state
                .sweep_request
                .as_ref()
                .is_some_and(|request| request.scope == SweepScope::NotFlux)
    }

    /// Start a sweep and show its list. `namespace`: `Some(None)` for every
    /// namespace, `None` for the current one. `drill_in` opens it on top of
    /// the current view (Back returns); otherwise it's a top-level switch.
    fn start_sweep(
        &mut self,
        scope: SweepScope,
        show: SweepShow,
        namespace: Option<Option<String>>,
        drill_in: bool,
    ) {
        if !self.config.native_resources {
            self.set_status_message((
                "Unmanaged / orphaned sweeps list every kind, a preview — :native turns it on"
                    .to_string(),
                true,
            ));
            return;
        }
        // A per-owner sweep looks everywhere; the list shows all of it.
        let namespace = match scope {
            SweepScope::NotFlux => namespace.unwrap_or_else(|| self.namespace().clone()),
            SweepScope::OrphansOf(_) => None,
        };
        let request = SweepRequest {
            scope,
            namespace: namespace.clone(),
            config: self.config.ownership.clone(),
        };
        // Navigate first: leaving an earlier sweep (e.g. from its describe
        // view) releases that one, and must not take the new request along.
        if drill_in {
            self.view_state.sweep_return = Some((
                self.view_state.filter.clone(),
                self.view_state.selected_index,
                self.view_state.scroll_offset,
            ));
            self.navigate_to(View::OwnershipList);
        } else if self.view_state.current_view != View::OwnershipList {
            self.navigate_root(View::OwnershipList);
        }
        self.view_state.sweep_request = Some(request.clone());
        self.view_state.sweep_namespace = namespace;
        self.view_state.sweep_show = show;
        self.view_state.sweep_reselect = None;
        self.async_state.sweep.request(request);
        self.view_state.filter.clear();
        self.view_state.filter_mode = false;
        self.view_state.selected_index = 0;
        self.view_state.scroll_offset = 0;
        self.invalidate_layout_cache();
    }

    /// Store a finished sweep, keeping the cursor on the same object
    /// across a rescan.
    pub(crate) fn on_sweep_result(&mut self, result: crate::kube::sweep::SweepResult) {
        let selected = self.view_state.sweep_reselect.take();
        self.async_state.sweep.set_result(result);
        // The cursor is shared: only the sweep's own list may move it.
        if self.view_state.current_view != View::OwnershipList {
            return;
        }
        self.reselect_sweep_row(selected);
    }

    /// Put the cursor on `selected` if the list shows it, else keep it in
    /// range.
    fn reselect_sweep_row(&mut self, selected: Option<ResourceKey>) {
        let rows = self.filtered_sweep_rows();
        let index = selected
            .and_then(|key| rows.iter().position(|r| r.key() == key))
            .unwrap_or_else(|| {
                self.view_state
                    .selected_index
                    .min(rows.len().saturating_sub(1))
            });
        self.view_state.selected_index = index;
    }

    /// Forget the sweep and what it remembered (context switch, `:native`
    /// off): its rows belong to that cluster / session.
    pub(crate) fn clear_sweep(&mut self) {
        self.async_state.sweep.clear();
        self.view_state.sweep_request = None;
        self.view_state.sweep_namespace = None;
        self.view_state.sweep_reselect = None;
        self.view_state.sweep_return = None;
    }

    /// Sweep rows in the namespace the list shows, after the `/` filter
    /// (kind, namespace, name, or standing).
    pub(crate) fn filtered_sweep_rows(&self) -> Vec<&SweepRow> {
        self.sweep_rows_matching().collect()
    }

    /// The selected sweep row.
    pub(crate) fn selected_sweep_row(&self) -> Option<&SweepRow> {
        self.sweep_rows_matching()
            .nth(self.view_state.selected_index)
    }

    fn sweep_rows_matching(&self) -> impl Iterator<Item = &SweepRow> {
        let filter = self.view_state.filter.to_lowercase();
        self.async_state
            .sweep
            .result()
            .into_iter()
            .flat_map(|result| result.rows.iter())
            .filter(|row| row.in_namespace(self.view_state.sweep_namespace.as_deref()))
            .filter(|row| self.view_state.sweep_show.shows(&row.standing))
            .filter(move |row| {
                filter.is_empty()
                    || [
                        row.name.as_str(),
                        &row.namespace,
                        &row.kind,
                        row.standing.label(),
                    ]
                    .iter()
                    .any(|field| field.to_lowercase().contains(&filter))
            })
    }
}
