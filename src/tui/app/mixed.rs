//! Kubernetes kinds in the Flux list (#304): `:ks,deploy`, or a view whose
//! `kinds` mix them in. Each mixed-in kind is watched like a kind list
//! (`kube::kind_list::watch_kinds`) while the Flux list is on screen, and
//! its objects become rows of the list with the shared columns. Keys follow
//! the selected row: Flux operations on Flux rows, the kind's capabilities
//! on Kubernetes rows (see `actions.rs`).

use super::core::App;
use super::state::View;
use crate::kube::kind_list::{KindListRequest, KindRow, MixedRequest, MixedSnapshot};
use crate::kube::object_status::ObjectHealth;
use crate::models::FluxResourceKind;
use crate::models::kind_filter::NativeKind;
use crate::models::kinds::{Capabilities, Family, KindSpec};
use crate::watcher::ResourceInfo;

/// The TYPE a mixed-in kind's rows show — its kind name, qualified by group
/// when a Flux kind (a Crossplane `Bucket`) or another kind in the list (core
/// and Knative `Service`) has the same name, so rows read apart. Display
/// only: rows are identified by group too ([`ResourceInfo::api_group`]).
pub(crate) fn native_type(spec: &KindSpec, listed: &[&KindSpec]) -> String {
    let shared = FluxResourceKind::parse_optional(&spec.gvk.kind).is_some()
        || listed
            .iter()
            .any(|other| other.gvk.kind == spec.gvk.kind && other.gvk.group != spec.gvk.group);
    if shared && !spec.gvk.group.is_empty() {
        format!("{}.{}", spec.gvk.kind, spec.gvk.group)
    } else {
        spec.gvk.kind.clone()
    }
}

/// What identifies a Flux-list row across updates: group (Kubernetes rows),
/// type, namespace, and name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowId {
    group: Option<String>,
    resource_type: String,
    namespace: String,
    name: String,
}

impl RowId {
    pub(crate) fn of(row: &ResourceInfo) -> Self {
        Self {
            group: row.api_group.clone(),
            resource_type: row.resource_type.clone(),
            namespace: row.namespace.clone(),
            name: row.name.clone(),
        }
    }
}

/// The discovered kind a mixed-in entry names: by group and kind once it
/// resolved (exact — never a same-named kind of another group), else by the
/// token as written.
pub(crate) fn resolve_native(kind: &NativeKind) -> Option<KindSpec> {
    match &kind.resolved {
        Some((group, name)) => crate::models::kinds::catalog().find_by_group_kind(group, name),
        None => crate::models::kinds::resolve(&kind.token),
    }
    .filter(|spec| spec.family == Family::Native)
}

/// A Kubernetes row in the shape the Flux list draws: kstatus health in
/// STATUS / READY — only `Current` is ready; in progress, failed,
/// terminating, or unreadable are not (an unknown READY would count as
/// healthy, and `:healthy` would keep a rolling Deployment) — its message,
/// and its age. Flux suspension doesn't apply, so it's never suspended.
fn as_list_row(spec: &KindSpec, listed: &[&KindSpec], row: &KindRow) -> ResourceInfo {
    ResourceInfo {
        api_group: Some(spec.gvk.group.clone()),
        name: row.name.clone(),
        namespace: row.namespace.clone(),
        resource_type: native_type(spec, listed),
        age: row.created,
        suspended: Some(false),
        ready: Some(row.health == ObjectHealth::Current),
        message: (!row.message.is_empty()).then(|| row.message.clone()),
        revision: None,
        labels: row.labels.clone(),
        annotations: row.annotations.clone(),
        last_reconciled: None,
        reconciliation_history: Vec::new(),
    }
}

impl App {
    /// The mixed-in kinds the Flux list can list now: each resolves to a
    /// discovered Kubernetes kind and native resources are on. `None` when
    /// there are none.
    fn mixed_request(&self) -> Option<MixedRequest> {
        if !self.config.native_resources {
            return None;
        }
        let kinds: Vec<KindListRequest> = self
            .view_state
            .kind_filter
            .native_kinds()
            .iter()
            .filter_map(resolve_native)
            .map(|spec| KindListRequest {
                namespace: if spec.is_namespaced() {
                    self.namespace.clone()
                } else {
                    None
                },
                spec,
            })
            .collect();
        (!kinds.is_empty()).then_some(MixedRequest { kinds })
    }

    /// The mixed-in kinds that can't be listed now, for a notice: native
    /// resources off, or not a kind this cluster serves.
    pub(super) fn skipped_mixed_kinds(&self) -> Vec<String> {
        self.view_state
            .kind_filter
            .native_kinds()
            .iter()
            .filter(|kind| !self.config.native_resources || resolve_native(kind).is_none())
            .map(|kind| kind.label().to_string())
            .collect()
    }

    /// Start, restart, or stop the mixed-in kinds' watch to match the kind
    /// filter and namespace. Only acts on the Flux list itself: views opened
    /// from it keep the watch (their rows stay valid), other top-level views
    /// stopped it on the way out ([`App::leave_view`]).
    pub(crate) fn sync_mixed_watch(&mut self) {
        if self.view_state.current_view != View::ResourceList {
            return;
        }
        let wanted = self.mixed_request();
        if self.async_state.mixed.key() == wanted.as_ref() {
            return;
        }
        match wanted {
            Some(request) => self.async_state.mixed.request(request),
            None => self.async_state.mixed.clear(),
        }
    }

    /// Store the mixed-in kinds' latest rows, keeping the cursor on the same
    /// object (rows are selected by index). While a view opened from the
    /// list is on top (describe, workload detail, …) the selection's
    /// identity is kept and restored on the way back
    /// ([`Self::restore_mixed_selection`]).
    pub(crate) fn on_mixed_snapshot(&mut self, snapshot: MixedSnapshot) {
        let on_list = self.view_state.current_view == View::ResourceList;
        // The displayed row, Flux or Kubernetes (`get_current_resource`
        // skips Kubernetes rows: they aren't watched Flux resources) — or
        // the one remembered when the list was covered.
        let selected = match self.view_state.mixed_reselect.take() {
            Some(remembered) => Some(remembered),
            None => self.list_row_id_at(self.view_state.selected_index),
        };
        self.async_state.mixed.set_result(snapshot);
        if on_list {
            self.reselect_list_row(selected);
        } else {
            self.view_state.mixed_reselect = selected;
        }
    }

    /// Back on the Flux list: put the cursor on the row selected before a
    /// watch update arrived under another view.
    pub(crate) fn restore_mixed_selection(&mut self) {
        if self.view_state.current_view != View::ResourceList {
            return;
        }
        if let Some(remembered) = self.view_state.mixed_reselect.take() {
            self.reselect_list_row(Some(remembered));
        }
    }

    /// The identity of the Flux list's row at `index`, as the list shows it
    /// now — computed for the Flux list even while another view is on top.
    fn list_row_id_at(&self, index: usize) -> Option<RowId> {
        // Mixed-in updates only arrive while the Flux list is open, so the
        // rows here are its rows even under a view opened from it.
        self.get_filtered_resources().get(index).map(RowId::of)
    }

    fn reselect_list_row(&mut self, selected: Option<RowId>) {
        let rows = self.get_filtered_resources();
        let index = selected
            .and_then(|id| rows.iter().position(|r| RowId::of(r) == id))
            .unwrap_or_else(|| {
                self.view_state
                    .selected_index
                    .min(rows.len().saturating_sub(1))
            });
        self.view_state.selected_index = index;
    }

    /// The mixed-in kinds' objects as Flux-list rows (none in favorites,
    /// which lists Flux resources only).
    pub(crate) fn mixed_resources(&self) -> Vec<ResourceInfo> {
        if self.view_state.current_view == View::ResourceFavorites {
            return Vec::new();
        }
        let Some(snapshot) = self.async_state.mixed.result() else {
            return Vec::new();
        };
        let listed: Vec<&KindSpec> = snapshot.kinds.iter().map(|k| &k.spec).collect();
        snapshot
            .kinds
            .iter()
            .flat_map(|kind| {
                let listed = &listed;
                kind.snapshot
                    .iter()
                    .flat_map(|s| &s.rows)
                    .map(move |row| as_list_row(&kind.spec, listed, row))
            })
            .collect()
    }

    /// The mixed-in kinds' TYPE names, for sorting the list in the order the
    /// filter gives them.
    pub(crate) fn mixed_type_names(&self) -> Vec<String> {
        let specs: Vec<KindSpec> = self
            .view_state
            .kind_filter
            .native_kinds()
            .iter()
            .filter_map(resolve_native)
            .collect();
        let listed: Vec<&KindSpec> = specs.iter().collect();
        specs
            .iter()
            .map(|spec| native_type(spec, &listed))
            .collect()
    }

    /// The mixed-in kind and row behind a Flux-list row, if it is one.
    /// Matched by group and kind, never by TYPE text alone.
    pub(crate) fn native_row_of(&self, row: &ResourceInfo) -> Option<(&KindSpec, &KindRow)> {
        let group = row.api_group.as_deref()?;
        let snapshot = self.async_state.mixed.result()?;
        let listed: Vec<&KindSpec> = snapshot.kinds.iter().map(|k| &k.spec).collect();
        snapshot
            .kinds
            .iter()
            .filter(|kind| {
                kind.spec.gvk.group == group
                    && native_type(&kind.spec, &listed) == row.resource_type
            })
            .find_map(|kind| {
                let found = kind
                    .snapshot
                    .as_ref()?
                    .rows
                    .iter()
                    .find(|r| r.namespace == row.namespace && r.name == row.name)?;
                Some((&kind.spec, found))
            })
    }

    /// The selected Flux-list row when it's a Kubernetes object (#304).
    pub(crate) fn selected_native_row(&self) -> Option<(&KindSpec, &KindRow)> {
        if self.view_state.current_view != View::ResourceList {
            return None;
        }
        let resources = self.get_filtered_resources();
        let selected = resources.get(self.view_state.selected_index)?;
        // Borrow from the snapshot, not the temporary list.
        let (spec, row) = self.native_row_of(selected)?;
        Some((spec, row))
    }

    /// Whether the Flux list's selected row is a Kubernetes object, so Flux
    /// keys stand aside for the kind's.
    pub(crate) fn on_native_row(&self) -> bool {
        self.selected_native_row().is_some()
    }

    /// The capabilities of the Kubernetes object the current list selects:
    /// a kind list's kind, or the selected mixed-in row's kind.
    pub(crate) fn selected_native_caps(&self) -> Option<Capabilities> {
        match self.view_state.current_view {
            View::KindList => Some(self.kind_list_caps()),
            View::ResourceList => self.selected_native_row().map(|(spec, _)| spec.caps),
            _ => None,
        }
    }

    /// Why some mixed-in kinds are missing, for the list's title.
    pub(crate) fn mixed_notice(&self) -> Option<String> {
        let mut notes = Vec::new();
        let skipped = self.skipped_mixed_kinds();
        if !skipped.is_empty() {
            let why = if self.config.native_resources {
                "not served by this cluster"
            } else {
                ":native lists Kubernetes kinds"
            };
            notes.push(format!("skipped {} ({why})", skipped.join(", ")));
        }
        if let Some(snapshot) = self.async_state.mixed.result() {
            for kind in &snapshot.kinds {
                let plural = &kind.spec.names.plural;
                if let Some(error) = &kind.error {
                    notes.push(format!("{plural}: {error}"));
                }
                if let Some(listed) = kind.snapshot.as_ref().filter(|s| s.truncated()) {
                    notes.push(format!(
                        "{plural} truncated: {} of {}",
                        listed.rows.len(),
                        listed.total
                    ));
                }
            }
        }
        (!notes.is_empty()).then(|| notes.join(" · "))
    }
}
