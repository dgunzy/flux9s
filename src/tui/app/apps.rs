//! The app view: what Flux objects deploy, from their own records
//! (`kube::app`). `a` opens it for the selected Kustomization / HelmRelease
//! / ResourceSet / FluxInstance, `:app <kind>/<name>…` for one or more, and
//! a view's `apps` names a set to open by name.

use super::core::App;
use super::state::View;
use crate::kube::app::{AppRequest, AppRow, AppSnapshot, is_app_kind};
use crate::models::FluxResourceKind;
use crate::tui::commands;
use crate::watcher::ResourceKey;

/// Where a row sits in the tree, to find it again after an update: its
/// object, its depth, and how many rows like it come before it (an object
/// can show twice, e.g. a source two releases share).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowId {
    kind: String,
    namespace: String,
    name: String,
    depth: usize,
    occurrence: usize,
}

/// Identities of `rows`, in order.
fn row_ids<'a>(rows: impl Iterator<Item = &'a AppRow>) -> Vec<RowId> {
    let mut seen: Vec<RowId> = Vec::new();
    for row in rows {
        let o = &row.object;
        let occurrence = seen
            .iter()
            .filter(|id| {
                id.kind == o.kind
                    && id.namespace == o.namespace
                    && id.name == o.name
                    && id.depth == row.depth
            })
            .count();
        seen.push(RowId {
            kind: o.kind.clone(),
            namespace: o.namespace.clone(),
            name: o.name.clone(),
            depth: row.depth,
            occurrence,
        });
    }
    seen
}

impl App {
    /// `:app <kind>/<name>… [-n <ns>]` (also `<kind>/<ns>/<name>`).
    pub(super) fn cmd_app(&mut self, cmd: &str) {
        let args = match commands::parse_app_args(cmd) {
            Ok(args) => args,
            Err(message) => {
                self.set_status_message((message, true));
                return;
            }
        };
        let title = args
            .targets
            .iter()
            .map(|t| t.label())
            .collect::<Vec<_>>()
            .join(", ");
        match self.resolve_app_targets(&args.targets, args.namespace.as_deref()) {
            Ok(roots) => self.open_app(title, roots, false),
            Err(message) => self.set_status_message((message, true)),
        }
    }

    /// `a`: what the selected Flux object deploys, drilled into from the
    /// list (Back returns).
    pub(super) fn app_of_selected(&mut self) {
        let Some(key) = self.selected_app_key() else {
            self.set_status_message((
                "a works on Kustomizations, HelmReleases, ResourceSets, and FluxInstances"
                    .to_string(),
                true,
            ));
            return;
        };
        let title = format!("{} {}/{}", key.resource_type, key.namespace, key.name);
        self.open_app(title, vec![key], true);
    }

    /// The selected Flux object, when it can head an app view.
    pub(crate) fn selected_app_key(&self) -> Option<ResourceKey> {
        let key = self.view_target()?;
        let kind = FluxResourceKind::parse_optional(&key.resource_type)?;
        is_app_kind(kind).then_some(key)
    }

    /// A view's `apps`: open them under the view's name. `namespace` is
    /// where `<kind>/<name>` entries are looked up (the view's own, or
    /// `-n`); the session's namespace doesn't change.
    pub(super) fn open_app_view(
        &mut self,
        name: &str,
        apps: &[String],
        namespace: Option<&str>,
    ) -> Result<(), String> {
        let targets = apps
            .iter()
            .map(|app| commands::AppTarget::parse(app))
            .collect::<Result<Vec<_>, _>>()?;
        let roots = self.resolve_app_targets(&targets, namespace)?;
        self.open_app(name.to_string(), roots, false);
        Ok(())
    }

    /// Each target as a Flux object: its kind resolved, and its namespace
    /// from the target, else `namespace`, else the one watched object of
    /// that name.
    fn resolve_app_targets(
        &self,
        targets: &[commands::AppTarget],
        namespace: Option<&str>,
    ) -> Result<Vec<ResourceKey>, String> {
        targets
            .iter()
            .map(|target| {
                let kind = crate::models::kinds::resolve(&target.kind)
                    .and_then(|spec| FluxResourceKind::parse_optional(&spec.names.kind))
                    .filter(|kind| is_app_kind(*kind))
                    .ok_or_else(|| {
                        format!(
                            "'{}' isn't a Flux kind that applies objects — use Kustomizations, HelmReleases, ResourceSets, or FluxInstances",
                            target.kind
                        )
                    })?;
                let namespace = match (&target.namespace, namespace) {
                    (Some(ns), _) => ns.clone(),
                    (None, Some(ns)) => ns.to_string(),
                    (None, None) => self.flux_object_namespace(kind.as_str(), &target.name)?,
                };
                Ok(ResourceKey::new(kind.as_str(), namespace, &target.name))
            })
            .collect()
    }

    /// Show the app view for `roots`. `drill_in` opens it on top of the
    /// current view (Back returns); otherwise it's a top-level switch.
    fn open_app(&mut self, title: String, roots: Vec<ResourceKey>, drill_in: bool) {
        // Navigate first: leaving an earlier app view releases that one.
        if drill_in {
            self.view_state.app_return = Some((
                self.view_state.filter.clone(),
                self.view_state.selected_index,
                self.view_state.scroll_offset,
            ));
            self.navigate_to(View::AppTree);
        } else {
            self.navigate_root(View::AppTree);
            self.view_state.app_return = None;
        }
        self.view_state.app_title = title;
        self.view_state.app_reselect = None;
        self.async_state.app.request(AppRequest { roots });
        self.view_state.filter.clear();
        self.view_state.filter_mode = false;
        self.view_state.selected_index = 0;
        self.view_state.scroll_offset = 0;
        self.invalidate_layout_cache();
    }

    /// Store the tree's latest snapshot, keeping the cursor on the same row
    /// (rows are selected by index). While a view opened from the tree is
    /// on top (describe, YAML, trace), the row is remembered and selected
    /// again on the way back ([`Self::restore_app_selection`]).
    pub(crate) fn on_app_snapshot(&mut self, snapshot: AppSnapshot) {
        // The row shown before this update — or the one remembered when
        // the tree was covered (the cursor no longer points into it).
        let selected = match self.view_state.app_reselect.take() {
            Some(remembered) => Some(remembered),
            None => row_ids(self.filtered_app_rows().into_iter())
                .into_iter()
                .nth(self.view_state.selected_index),
        };
        self.async_state.app.set_result(snapshot);
        if self.view_state.current_view == View::AppTree {
            self.reselect_app_row(selected);
        } else {
            self.view_state.app_reselect = selected;
        }
    }

    /// Back on the app view: put the cursor on the row selected before an
    /// update arrived under another view.
    pub(crate) fn restore_app_selection(&mut self) {
        if self.view_state.current_view != View::AppTree {
            return;
        }
        if let Some(remembered) = self.view_state.app_reselect.take() {
            self.reselect_app_row(Some(remembered));
        }
    }

    fn reselect_app_row(&mut self, selected: Option<RowId>) {
        let rows = self.filtered_app_rows();
        let index = selected
            .and_then(|id| row_ids(rows.iter().copied()).iter().position(|r| *r == id))
            .unwrap_or_else(|| {
                self.view_state
                    .selected_index
                    .min(rows.len().saturating_sub(1))
            });
        self.view_state.selected_index = index;
    }

    /// The tree's rows after the `/` filter: kind, namespace, name, role,
    /// or status — as shown (`CrashLoopBackOff`, `Available`) or its health
    /// (`Failed`).
    pub(crate) fn filtered_app_rows(&self) -> Vec<&AppRow> {
        let filter = self.view_state.filter.to_lowercase();
        self.async_state
            .app
            .result()
            .into_iter()
            .flat_map(|snapshot| snapshot.rows.iter())
            .filter(|row| {
                filter.is_empty()
                    || [
                        row.object.kind.as_str(),
                        &row.object.namespace,
                        &row.object.name,
                        row.status.label(),
                        row.status.health.label(),
                        row.role.label(),
                    ]
                    .iter()
                    .any(|field| field.to_lowercase().contains(&filter))
            })
            .collect()
    }

    /// The selected row of the app view.
    pub(crate) fn selected_app_row(&self) -> Option<&AppRow> {
        self.filtered_app_rows()
            .get(self.view_state.selected_index)
            .copied()
    }

    /// The selected row's Flux object (Flux rows describe like Flux
    /// resources); `None` for a Kubernetes row.
    pub(crate) fn selected_app_flux_row(&self) -> Option<ResourceKey> {
        let row = self.selected_app_row()?;
        let kind = row.flux_kind()?;
        Some(ResourceKey::new(
            kind.as_str(),
            &row.object.namespace,
            &row.object.name,
        ))
    }

    /// The Flux object the selected row hangs under: the nearest row above
    /// it, with a smaller depth, that is one (a pod's is its workload's).
    pub(crate) fn app_row_manager(&self) -> Option<ResourceKey> {
        let selected = self.selected_app_row()?;
        let rows = &self.async_state.app.result()?.rows;
        let index = rows.iter().position(|row| std::ptr::eq(row, selected))?;
        let mut depth = selected.depth;
        for row in rows[..index].iter().rev() {
            if row.depth < depth {
                if let Some(key) = row.flux_app() {
                    return Some(key);
                }
                depth = row.depth;
            }
        }
        None
    }

    /// Leaving the app view for `to`: its watch and rows are kept while a
    /// row is open in describe / YAML / trace, else released.
    pub(super) fn leave_app_view(&mut self, to: View) {
        if to.is_nested_view() || to == View::AppTree {
            return;
        }
        self.async_state.app.clear();
        self.view_state.app_reselect = None;
        self.view_state.filter_mode = false;
        let (filter, selected, scroll) = self.view_state.app_return.take().unwrap_or_default();
        self.view_state.filter = filter;
        self.view_state.selected_index = selected;
        self.view_state.scroll_offset = scroll;
    }
}
