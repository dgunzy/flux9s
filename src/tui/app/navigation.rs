//! Back navigation: one stack for every view.
//!
//! Opening a view pushes where you were onto `ViewState::nav_stack`; Back
//! (`Esc` / `Backspace`) pops it. Three verbs cover every transition:
//!
//! - [`App::navigate_to`] — drill into a view (Enter, `d`, `y`, `l`, …).
//!   Sibling detail views (detail ↔ YAML ↔ describe ↔ trace ↔ graph …)
//!   replace each other rather than stack, so Back from any of them returns
//!   to the list they were opened from. The graph is the exception: what you
//!   open from a graph node returns to the graph.
//! - [`App::navigate_root`] — a `:` command switching top-level view
//!   (`:flux`, `:favorites`, `:events`, `:pulse`, a first `:<kind>`). Closes
//!   everything opened so far; Back then returns to the Flux list.
//! - [`App::navigate_back`] — pop. The view being left releases what it held
//!   ([`App::leave_view`]: log streams, watches, edit state) in one place.

use super::core::App;
use super::state::{NavEntry, View};

/// Deepest Back history kept; the oldest entries are dropped beyond it.
const MAX_NAV_DEPTH: usize = 32;

impl App {
    /// Where the current view would return to on Back, if anywhere.
    pub fn back_target(&self) -> Option<View> {
        self.view_state.nav_stack.last().map(NavEntry::view)
    }

    /// The current view as a stack entry (a kind list remembers its kind).
    fn current_nav_entry(&self) -> NavEntry {
        match self.view_state.current_view {
            View::KindList => self
                .async_state
                .kind_list
                .key()
                .map(|request| NavEntry::Kind(request.spec.clone()))
                .unwrap_or(NavEntry::View(View::KindList)),
            view => NavEntry::View(view),
        }
    }

    /// Remember the current view (or kind list) for Back.
    pub(crate) fn push_current_view(&mut self) {
        let entry = self.current_nav_entry();
        let stack = &mut self.view_state.nav_stack;
        if stack.last() != Some(&entry) {
            stack.push(entry);
        }
        if stack.len() > MAX_NAV_DEPTH {
            stack.remove(0);
        }
    }

    /// Drill into `view`; Back returns to the current one. Sibling detail
    /// views replace each other (see the module docs).
    pub(crate) fn navigate_to(&mut self, view: View) {
        let current = self.view_state.current_view;
        if current == view {
            return;
        }
        let sibling =
            current.is_nested_view() && view.is_nested_view() && current != View::ResourceGraph;
        if sibling {
            self.leave_view(current, view);
        } else {
            self.push_current_view();
        }
        self.view_state.current_view = view;
        self.invalidate_layout_cache();
    }

    /// Replace the current view without remembering it (e.g. a workload
    /// fetched only to reach its pod logs).
    pub(crate) fn replace_view(&mut self, view: View) {
        let current = self.view_state.current_view;
        if current != view {
            self.leave_view(current, view);
            self.view_state.current_view = view;
            self.invalidate_layout_cache();
        }
    }

    /// Switch to a top-level view, closing everything opened so far. Back
    /// from anything but the Flux list returns to the Flux list.
    pub(crate) fn navigate_root(&mut self, view: View) {
        let mut leaving = vec![self.view_state.current_view];
        leaving.extend(
            self.view_state
                .nav_stack
                .drain(..)
                .rev()
                .map(|entry| entry.view()),
        );
        for left in leaving {
            self.leave_view(left, view);
        }
        if view != View::ResourceList {
            self.view_state
                .nav_stack
                .push(NavEntry::View(View::ResourceList));
        }
        self.view_state.current_view = view;
        self.sync_mixed_watch();
        self.restore_mixed_selection();
        self.invalidate_layout_cache();
    }

    /// Go back one step. Returns `false` when there is nowhere to go back to
    /// (the caller decides: quit confirmation at the top, or nothing).
    pub(crate) fn navigate_back(&mut self) -> bool {
        let Some(entry) = self.view_state.nav_stack.pop() else {
            return false;
        };
        let leaving = self.view_state.current_view;
        self.leave_view(leaving, entry.view());
        match entry {
            NavEntry::Kind(spec) => {
                // Still loaded (we drilled into one of its rows): keep the
                // live list and cursor. Replaced by another kind, or the
                // namespace changed meanwhile: reopen it.
                let namespace = if spec.is_namespaced() {
                    self.namespace.clone()
                } else {
                    None
                };
                let loaded = self
                    .async_state
                    .kind_list
                    .key()
                    .is_some_and(|r| r.spec == spec && r.namespace == namespace);
                if loaded && leaving != View::KindList {
                    self.view_state.current_view = View::KindList;
                } else {
                    self.show_kind_list(spec);
                }
            }
            NavEntry::View(view) => self.view_state.current_view = view,
        }
        self.sync_mixed_watch();
        self.restore_mixed_selection();
        self.invalidate_layout_cache();
        true
    }

    /// Back, or the Flux list when there's no history (e.g. an async
    /// failure that leaves nothing to show).
    pub(crate) fn go_back_or_home(&mut self) {
        if !self.navigate_back() {
            self.navigate_root(View::ResourceList);
        }
    }

    /// Back out of `view` after an async failure left it empty — only if
    /// it's still on screen (the user may have moved on).
    pub fn back_from(&mut self, view: View) {
        if self.view_state.current_view == view {
            self.go_back_or_home();
        }
    }

    /// Leave the edit view for wherever `e` was pressed, clearing the edit
    /// state (save, cancel, or failure). A no-op outside the edit view, so a
    /// late result can't navigate away from wherever the user went since.
    pub(crate) fn leave_edit(&mut self) {
        if self.view_state.current_view == View::ResourceEdit {
            self.go_back_or_home();
        } else {
            self.leave_view(View::ResourceEdit, self.view_state.current_view);
        }
    }

    /// Release what `view` held now that it's being closed on the way to
    /// `to`. Exhaustive on purpose: a new view must decide what it releases.
    pub(crate) fn leave_view(&mut self, view: View, to: View) {
        match view {
            // Its mixed-in kinds' watch (#304) runs while it's on screen or
            // under a view opened from it; another top-level view stops it.
            View::ResourceList => {
                if to != View::ResourceList {
                    self.async_state.mixed.clear();
                    self.view_state.mixed_reselect = None;
                }
            }
            View::Help => {}
            View::ResourceDetail
            | View::ResourceDescribe
            | View::ResourceYAML
            | View::ResourceTrace
            | View::ResourceHistory
            | View::ResourceGraph
            | View::HelmValues => {
                if view == View::HelmValues {
                    // Leaving the values view stops its live watch.
                    self.async_state.helm_values.clear();
                }
                if !to.is_nested_view() {
                    self.selection_state.native_object = None;
                    self.selection_state.selected_resource_key = None;
                    self.view_state.text_search.clear();
                }
            }
            View::ResourceEdit => {
                self.async_state.edit_pending = None;
                self.async_state.edit_full_yaml = None;
                self.async_state.edit_save_pending = None;
                self.async_state.edit_save_result_rx = None;
                self.async_state.edit_error_message = None;
                self.async_state.edit_editor_launched = false;
            }
            View::ResourceFavorites => {
                self.selection_state.selected_resource_key = None;
            }
            View::EventList => self.stop_kube_events_watch(),
            View::Logs => {
                self.logs.stop();
                self.view_state.text_search.clear();
            }
            View::KindList => {
                if to != View::KindList {
                    self.async_state.kind_list.clear();
                    self.view_state.kind_list_error = None;
                    self.view_state.filter.clear();
                    self.view_state.filter_mode = false;
                    self.view_state.selected_index = 0;
                    self.view_state.scroll_offset = 0;
                }
            }
            View::WorkloadList | View::InventoryList => {
                self.async_state.inventory_status.clear();
            }
            // Kept while its rows are open in describe / YAML / trace.
            View::OwnershipList => {
                if !to.is_nested_view() {
                    self.async_state.sweep.clear();
                    self.view_state.filter_mode = false;
                    let (filter, selected, scroll) =
                        self.view_state.sweep_return.take().unwrap_or_default();
                    self.view_state.filter = filter;
                    self.view_state.selected_index = selected;
                    self.view_state.scroll_offset = scroll;
                }
            }
            View::WorkloadDetail => {
                self.async_state.workload.clear();
                self.async_state.workload_metrics.clear();
                self.logs_after_workload_load = false;
                self.view_state.logs_replace_workload = false;
                self.view_state.text_search.clear();
            }
            View::Pulse => self.view_state.text_search.clear(),
        }
    }
}
