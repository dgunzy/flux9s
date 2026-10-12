//! View actions: opening views and acting on what they show (graph
//! drill-down, workload and pod actions, Flux operations, YAML/describe).
//!
//! Input is handled in `keys.rs`, `:` commands in `command_mode.rs`, and
//! popups in `submenus.rs`; this module holds what those dispatch to.

use super::core::App;
use super::state::{Confirmation, PendingOperation, View};
use crate::kube::objects::ObjectRef;
use crate::models::FluxResourceKind;
use crate::watcher::ResourceKey;

/// Status message for `e` on a native object flux9s won't edit: why, and the
/// kubectl command that does it. Secrets are never edited (flux9s only holds
/// them redacted); other kinds when the API doesn't allow patching them or
/// kind lookup (`:native`) is off. A Flux kind only lands here when it's
/// outside the watched namespace scope (e.g. from the events feed).
pub(super) fn native_edit_message(target: &ObjectRef) -> String {
    if FluxResourceKind::parse_optional(&target.kind).is_some() {
        return format!(
            "{} {}/{} isn't in the watched namespace — :ns {} to edit it",
            target.kind, target.namespace, target.name, target.namespace
        );
    }
    let group = target
        .api_version
        .as_deref()
        .and_then(|av| av.split_once('/'))
        .map(|(group, _)| format!(".{group}"))
        .unwrap_or_default();
    let namespace = if target.namespace.is_empty() {
        String::new()
    } else {
        format!(" -n {}", target.namespace)
    };
    let kubectl = format!(
        "kubectl edit {}{}/{}{}",
        target.kind.to_lowercase(),
        group,
        target.name,
        namespace
    );
    if target.kind == "Secret" && group.is_empty() {
        return format!(
            "Secrets can't be edited in flux9s (their values are redacted). Try: {kubectl}"
        );
    }
    format!(
        "{} can't be edited here (the API doesn't allow patching it, or kind lookup is off — :native). Try: {kubectl}",
        target.kind
    )
}

impl App {
    /// Navigate back one level, or show the quit confirmation dialog at the top level.
    ///
    /// Shared implementation for `q` and `Esc`, matching k9s behaviour where
    /// neither key exits the application directly. The help overlay is treated as
    /// a navigable layer and is dismissed first before any view transition occurs.
    /// Move keyboard focus between graph nodes in visual (top-to-bottom) order.
    /// Clamps at the ends rather than wrapping so the direction stays intuitive.
    /// Auto-scrolling to keep the focused node visible is handled by the renderer.
    pub(super) fn move_graph_focus(&mut self, forward: bool) {
        let Some(graph) = self.async_state.graph.result() else {
            return;
        };
        let order = graph.focus_order();
        if order.is_empty() {
            return;
        }

        // Where the current focus sits within the visual order (start by default).
        let current_pos = self
            .view_state
            .graph_focus_index
            .and_then(|idx| order.iter().position(|&i| i == idx));

        let next_pos = match current_pos {
            Some(pos) if forward => (pos + 1).min(order.len() - 1),
            Some(pos) => pos.saturating_sub(1),
            None => 0,
        };
        self.view_state.graph_focus_index = Some(order[next_pos]);
    }

    /// Identity of the keyboard-focused graph node when it is an individual
    /// resource. Aggregate nodes (workload/resource groups) and external
    /// upstream URLs have no single resource to act on, so they resolve to
    /// `None`.
    pub(crate) fn focused_graph_node_target(&self) -> Option<ResourceKey> {
        use crate::trace::NodeType;
        let node = self.async_state.graph.result().and_then(|graph| {
            self.view_state
                .graph_focus_index
                .and_then(|idx| graph.nodes.get(idx))
        })?;
        if matches!(
            node.node_type,
            NodeType::WorkloadGroup | NodeType::ResourceGroup | NodeType::Upstream
        ) {
            return None;
        }
        Some(ResourceKey::new(
            node.kind.clone(),
            node.namespace.clone(),
            node.name.clone(),
        ))
    }

    /// Open the detail view for the focused graph node when it maps to a watched
    /// resource. Aggregate nodes (workload/resource groups) and external upstream
    /// URLs are not directly navigable and just show a hint instead.
    pub(super) fn navigate_to_focused_graph_node(&mut self) {
        let focused_node = self
            .async_state
            .graph
            .result()
            .and_then(|graph| {
                self.view_state
                    .graph_focus_index
                    .and_then(|idx| graph.nodes.get(idx))
            })
            .map(|node| (node.node_type, node.description.clone()));
        let Some(rk) = self.focused_graph_node_target() else {
            match focused_node {
                // The workload group drills into the workload list (#194):
                // its description carries one encoded WorkloadRef per line.
                Some((crate::trace::NodeType::WorkloadGroup, description)) => {
                    let rows: Vec<_> = description
                        .as_deref()
                        .unwrap_or_default()
                        .lines()
                        .filter_map(crate::kube::workloads::WorkloadRef::parse_graph_line)
                        .collect();
                    if rows.is_empty() {
                        self.set_status_message(("No workloads in this group".to_string(), false));
                        return;
                    }
                    // Readiness stays live (watch), like the inventory list.
                    self.async_state
                        .inventory_status
                        .request(rows.iter().map(|r| r.status_target()).collect());
                    self.view_state.workload_rows = rows;
                    self.view_state.selected_index = 0;
                    self.view_state.scroll_offset = 0;
                    self.navigate_to(View::WorkloadList);
                }
                // The resource group drills into the inventory breakdown
                // (#245): its description carries one encoded InventoryEntry
                // per line.
                Some((crate::trace::NodeType::ResourceGroup, description)) => {
                    let rows: Vec<_> = description
                        .as_deref()
                        .unwrap_or_default()
                        .lines()
                        .filter_map(crate::kube::inventory::InventoryEntry::parse_graph_line)
                        .collect();
                    if rows.is_empty() {
                        self.set_status_message(("No resources in this group".to_string(), false));
                        return;
                    }
                    self.async_state.inventory_status.request(rows.clone());
                    self.view_state.inventory_rows = rows;
                    self.view_state.selected_index = 0;
                    self.view_state.scroll_offset = 0;
                    self.navigate_to(View::InventoryList);
                }
                Some(_) => {
                    self.set_status_message((
                        "Aggregate node — select an individual Flux resource to open it"
                            .to_string(),
                        false,
                    ));
                }
                None => {}
            }
            return;
        };

        let key = rk.to_key_string();
        if self.state.get(&key).is_some() {
            self.selection_state.selected_resource_key = Some(key);
            // Back returns to the graph (not the list).
            self.navigate_to(View::ResourceDetail);
        } else {
            self.set_status_message((
                format!(
                    "{} {} is not in the current view",
                    rk.resource_type, rk.name
                ),
                false,
            ));
        }
    }

    /// Open the detail view for the resource the selected event is about,
    /// when it is a Flux resource flux9s is watching. Back returns to the
    /// events feed (the events watcher keeps running meanwhile).
    pub(super) fn navigate_to_selected_event_resource(&mut self) {
        let events = self.filtered_kube_events();
        let Some(event) = events.get(self.view_state.selected_index) else {
            return;
        };

        let key = crate::watcher::resource_key(
            &event.involved_namespace,
            &event.involved_name,
            &event.involved_kind,
        );
        if self.state.get(&key).is_none() {
            // Not watched but resolvable (a native object, #262): describe it.
            if !event.involved_api_version.is_empty() {
                self.open_describe_view();
                return;
            }
            // Not in the watch state: outside the namespace scope, a non-Flux
            // kind, or its watcher isn't running. Name the namespace so a
            // scope mismatch is visible, and point at the keys that still work.
            self.set_status_message((
                format!(
                    "{} (ns: {}) is not in the watched resources — press y/d to view it",
                    event.object_label(),
                    event.involved_namespace
                ),
                false,
            ));
            return;
        }

        self.selection_state.selected_resource_key = Some(key);
        self.navigate_to(View::ResourceDetail);
    }

    /// Stream logs for the detailed workload's pods: directly when there is
    /// exactly one pod, via a pod submenu when there are several.
    pub(crate) fn open_workload_pod_logs(&mut self) {
        let Some(workload) = self.async_state.workload.result() else {
            return; // Still loading
        };
        match workload.pods.as_slice() {
            [] => {
                self.set_status_message((
                    format!("{}/{} has no pods to stream", workload.kind, workload.name),
                    false,
                ));
            }
            [only] => {
                let (namespace, pod) = (workload.namespace.clone(), only.name.clone());
                self.open_pod_logs(&namespace, &pod);
            }
            pods => {
                let items: Vec<crate::tui::submenu::SubmenuItem> = pods
                    .iter()
                    .map(|pod| {
                        crate::tui::submenu::SubmenuItem::with_display(
                            format!("{}/{}", workload.namespace, pod.name),
                            format!("{} ({}, ready {})", pod.name, pod.phase, pod.ready),
                        )
                    })
                    .collect();
                self.view_state.submenu_state = Some(
                    crate::tui::submenu::SubmenuState::new(
                        crate::tui::commands::submenu_ids::POD_LOGS.to_string(),
                        items,
                    )
                    .with_title("Pod Logs".to_string())
                    .with_help(
                        "j/k: Navigate | /: Filter | Enter: Stream logs | Esc: Cancel".to_string(),
                    ),
                );
            }
        }
    }

    /// `v`: open the effective values of the current HelmRelease.
    pub(super) fn open_helm_values(&mut self) {
        let Some(resource) = self.get_current_resource() else {
            return;
        };
        if FluxResourceKind::parse_optional(&resource.resource_type)
            != Some(FluxResourceKind::HelmRelease)
        {
            self.set_status_message((
                format!(
                    "Values are only available for HelmReleases, not {}",
                    resource.resource_type
                ),
                true,
            ));
            return;
        }
        let Some(key) = self.prepare_selected_resource_key_for_nested_view() else {
            return;
        };
        self.selection_state.native_object = None;
        self.async_state
            .helm_values
            .request(super::state::HelmValuesRequest {
                key,
                reveal_secrets: false,
            });
        self.view_state.helm_values_scroll_offset = 0;
        self.view_state.text_search.clear();
        self.navigate_to(View::HelmValues);
    }

    /// `x` in the values view: re-resolve with Secret values shown / hidden.
    pub(super) fn toggle_helm_values_secrets(&mut self) {
        let Some(key) = self
            .selection_state
            .selected_resource_key
            .as_deref()
            .and_then(ResourceKey::parse)
        else {
            return;
        };
        let reveal = !self
            .async_state
            .helm_values
            .result()
            .is_some_and(|v| v.secrets_revealed);
        self.async_state
            .helm_values
            .request(super::state::HelmValuesRequest {
                key,
                reveal_secrets: reveal,
            });
        // No status message: the title's "[secrets shown]" marks the state,
        // and a message would swallow the next Esc.
    }

    /// `c` in the log view: pick another container of the streamed pod.
    pub(super) fn open_log_container_picker(&mut self) {
        let Some(session) = self.logs.session.as_ref() else {
            return;
        };
        if session.containers.len() < 2 {
            self.set_status_message(("This pod has a single container".to_string(), false));
            return;
        }
        let items = session
            .containers
            .iter()
            .map(|name| {
                let display = if session.container.as_deref() == Some(name.as_str()) {
                    format!("{name} (current)")
                } else {
                    name.clone()
                };
                crate::tui::submenu::SubmenuItem::with_display(name.clone(), display)
            })
            .collect();
        self.view_state.submenu_state = Some(
            crate::tui::submenu::SubmenuState::new(
                crate::tui::commands::submenu_ids::LOG_CONTAINER.to_string(),
                items,
            )
            .with_title("Container".to_string())
            .with_help("j/k: Navigate | /: Filter | Enter: Stream | Esc: Cancel".to_string()),
        );
    }

    /// Restart the log stream on another container of the same pod.
    pub(super) fn switch_log_container(&mut self, container: String) {
        let Some(session) = self.logs.session.as_ref() else {
            return;
        };
        let (namespace, pod) = (session.namespace.clone(), session.pod.clone());
        self.logs.request_container(namespace, pod, Some(container));
        self.view_state.log_scroll_offset = 0;
        self.view_state.text_search.clear();
    }

    /// Open the shared workload detail view for a workload; Back returns to
    /// where it was opened from (the graph's workload list or a `:<kind>`
    /// list). With `then_logs`, continue into pod logs once the data arrives
    /// — the logs then replace the detail, so Back skips it.
    pub(crate) fn open_workload_detail(
        &mut self,
        kind: String,
        namespace: String,
        name: String,
        then_logs: bool,
    ) {
        self.async_state.workload_metrics.clear();
        self.async_state
            .workload
            .request(ResourceKey::new(kind, namespace, name));
        self.view_state.workload_scroll_offset = 0;
        self.view_state.text_search.clear();
        self.navigate_to(View::WorkloadDetail);
        self.logs_after_workload_load = then_logs;
        self.view_state.logs_replace_workload = then_logs;
    }

    /// Queue a workload action for confirmation, unless read-only (#263).
    pub(crate) fn confirm_object_action(&mut self, action: crate::kube::workloads::ObjectAction) {
        if self.config.read_only {
            self.set_status_message((
                crate::constants::READ_ONLY_WRITE_ACTION_MESSAGE.to_string(),
                true,
            ));
            return;
        }
        self.async_state.confirmation_pending = Some(Confirmation::Object(action));
    }

    /// The object a write action (restart, annotate, …) targets in the
    /// current view: the selected kind-list or workload row, the workload on
    /// screen, or the view's native / Flux target.
    pub(super) fn write_target(&self) -> Option<ObjectRef> {
        match self.view_state.current_view {
            View::KindList => {
                let spec = &self.async_state.kind_list.key()?.spec;
                let (kind, namespace, name) = self.selected_kind_object()?;
                Some(ObjectRef::native(
                    spec.gvk.api_version(),
                    kind,
                    namespace,
                    name,
                ))
            }
            View::WorkloadList => self
                .view_state
                .workload_rows
                .get(self.view_state.selected_index)
                .map(|row| {
                    ObjectRef::native(
                        row.api_version.clone(),
                        row.kind.clone(),
                        row.namespace.clone(),
                        row.name.clone(),
                    )
                }),
            View::WorkloadDetail => self.async_state.workload.result().map(|w| {
                ResourceKey::new(w.kind.clone(), w.namespace.clone(), w.name.clone()).into()
            }),
            _ => self
                .native_view_target()
                .or_else(|| self.view_target().map(ObjectRef::from)),
        }
    }

    /// Run a data-driven write action ([`ActionSpec::Patch`] /
    /// [`ActionSpec::Annotate`]) on the current target: kind filter,
    /// read-only gate, then the usual confirmation.
    ///
    /// [`ActionSpec::Patch`]: super::actions::ActionSpec::Patch
    /// [`ActionSpec::Annotate`]: super::actions::ActionSpec::Annotate
    pub(super) fn request_write(&mut self, spec: &super::actions::ActionSpec) {
        let Some(write) = spec.write_info() else {
            return;
        };
        let Some(target) = self.write_target() else {
            return; // Nothing selected, or still loading
        };
        if !write.kinds.is_empty() && !write.kinds.contains(&target.kind.as_str()) {
            self.set_status_message((
                format!(
                    "{} does not support {}",
                    target.kind,
                    write.verb.to_lowercase()
                ),
                true,
            ));
            return;
        }
        if self.config.read_only {
            self.set_status_message((
                crate::constants::READ_ONLY_WRITE_ACTION_MESSAGE.to_string(),
                true,
            ));
            return;
        }
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let patch = match spec.patch_document(&now) {
            Some(Ok(patch)) => patch,
            Some(Err(e)) => {
                self.set_status_message((format!("{e:#}"), true));
                return;
            }
            None => return,
        };
        self.confirm_object_action(crate::kube::workloads::ObjectAction::Patch {
            target,
            verb: write.verb.to_string(),
            done: write.done.to_string(),
            detail: write.detail.to_string(),
            patch,
        });
    }

    /// Ctrl+d in workload detail: confirm deleting the only pod directly, or
    /// pick one from a submenu when there are several.
    pub(super) fn request_pod_delete(&mut self) {
        let Some(workload) = self.async_state.workload.result() else {
            return; // Still loading
        };
        match workload.pods.as_slice() {
            [] => {
                let msg = format!("{}/{} has no pods to delete", workload.kind, workload.name);
                self.set_status_message((msg, false));
            }
            [only] => {
                let action = crate::kube::workloads::ObjectAction::DeletePod {
                    namespace: workload.namespace.clone(),
                    name: only.name.clone(),
                };
                self.confirm_object_action(action);
            }
            pods => {
                if self.config.read_only {
                    self.set_status_message((
                        crate::constants::READ_ONLY_WRITE_ACTION_MESSAGE.to_string(),
                        true,
                    ));
                    return;
                }
                let items: Vec<crate::tui::submenu::SubmenuItem> = pods
                    .iter()
                    .map(|pod| {
                        crate::tui::submenu::SubmenuItem::with_display(
                            format!("{}/{}", workload.namespace, pod.name),
                            format!("{} ({}, ready {})", pod.name, pod.phase, pod.ready),
                        )
                    })
                    .collect();
                self.view_state.submenu_state = Some(
                    crate::tui::submenu::SubmenuState::new(
                        crate::tui::commands::submenu_ids::POD_DELETE.to_string(),
                        items,
                    )
                    .with_title("Delete Pod".to_string())
                    .with_help(
                        "j/k: Navigate | /: Filter | Enter: Delete (confirm) | Esc: Cancel"
                            .to_string(),
                    ),
                );
            }
        }
    }

    /// Open the YAML view for the current view's target (Flux or native).
    pub(super) fn open_yaml_view(&mut self) {
        if let Some(target) = self.prepare_nested_view_target() {
            self.async_state.yaml.request(target);
            self.view_state.yaml_scroll_offset = 0;
            self.view_state.text_search.clear();
            self.navigate_to(View::ResourceYAML);
        }
    }

    /// Open the describe view for the current view's target (Flux or native).
    pub(crate) fn open_describe_view(&mut self) {
        if let Some(target) = self.prepare_nested_view_target() {
            self.async_state.describe.request(target);
            self.view_state.describe_scroll_offset = 0;
            self.view_state.text_search.clear();
            self.navigate_to(View::ResourceDescribe);
        }
    }

    /// The native (non-watched) object the current view points at, if any:
    /// the selected inventory row, an event's unwatched involved object, or
    /// the native object already on screen when toggling YAML ↔ describe.
    pub(super) fn native_view_target(&self) -> Option<ObjectRef> {
        match self.view_state.current_view {
            View::InventoryList => self
                .view_state
                .inventory_rows
                .get(self.view_state.selected_index)
                .map(ObjectRef::from),
            View::KindList => {
                let spec = &self.async_state.kind_list.key()?.spec;
                let rows = self.filtered_kind_rows();
                let row = rows.get(self.view_state.selected_index)?;
                Some(ObjectRef::native(
                    spec.gvk.api_version(),
                    spec.gvk.kind.clone(),
                    row.namespace.clone(),
                    row.name.clone(),
                ))
            }
            View::EventList => {
                let events = self.filtered_kube_events();
                let event = events.get(self.view_state.selected_index)?;
                let key = crate::watcher::resource_key(
                    &event.involved_namespace,
                    &event.involved_name,
                    &event.involved_kind,
                );
                if self.state.get(&key).is_some() || event.involved_api_version.is_empty() {
                    return None;
                }
                Some(ObjectRef::native(
                    event.involved_api_version.clone(),
                    event.involved_kind.clone(),
                    event.involved_namespace.clone(),
                    event.involved_name.clone(),
                ))
            }
            View::ResourceYAML | View::ResourceDescribe => {
                self.selection_state.native_object.clone()
            }
            _ => None,
        }
    }

    /// Resolve and select the object a YAML/describe view should open,
    /// recording where Back should return to. Native objects are tracked in
    /// `native_object`; Flux resources clear it.
    pub(super) fn prepare_nested_view_target(&mut self) -> Option<ObjectRef> {
        let Some(native) = self.native_view_target() else {
            let rk = self.prepare_selected_resource_key_for_nested_view()?;
            self.selection_state.native_object = None;
            return Some(rk.into());
        };
        self.selection_state.selected_resource_key = Some(native.to_resource_key().to_key_string());
        self.selection_state.native_object = Some(native.clone());
        Some(native)
    }

    pub(super) fn prepare_selected_resource_key_for_nested_view(&mut self) -> Option<ResourceKey> {
        let rk = self.view_target()?;
        self.selection_state.selected_resource_key = Some(rk.to_key_string());
        Some(rk)
    }

    pub(super) fn handle_operation_key(&mut self, op_key: char) {
        if let Some(resource) = self.get_current_resource() {
            if let Some(operation) = self.operation_registry.get_by_keybinding(op_key) {
                if !operation.is_valid_for(&resource.resource_type) {
                    self.set_status_message((
                        format!(
                            "Operation '{}' is not valid for {}",
                            operation.name(),
                            resource.resource_type
                        ),
                        true,
                    ));
                    return;
                }

                if self.config.read_only {
                    self.set_status_message((
                        crate::constants::READ_ONLY_WRITE_ACTION_MESSAGE.to_string(),
                        true,
                    ));
                    return;
                }

                if operation.requires_confirmation() {
                    self.async_state.confirmation_pending =
                        Some(Confirmation::Flux(PendingOperation::new(
                            resource.resource_type.clone(),
                            resource.namespace.clone(),
                            resource.name.clone(),
                            op_key,
                        )));
                    return;
                }

                let feedback_msg = if op_key == 'W' {
                    format!(
                        "Reconciling {}/{} with source...",
                        resource.resource_type, resource.name
                    )
                } else {
                    format!(
                        "{} {}/{}...",
                        operation.name(),
                        resource.resource_type,
                        resource.name
                    )
                };
                self.set_status_message((feedback_msg, false));
                self.execute_operation(
                    &resource.resource_type,
                    &resource.namespace,
                    &resource.name,
                    op_key,
                );
            }
        }
    }

    pub(super) fn execute_operation(
        &mut self,
        resource_type: &str,
        namespace: &str,
        name: &str,
        op_key: char,
    ) {
        // Check readonly mode - prevent modification operations
        if self.config.read_only && self.operation_registry.get_by_keybinding(op_key).is_some() {
            // All operations are modifications, so block them all in readonly mode
            self.set_status_message((
                crate::constants::READ_ONLY_WRITE_ACTION_MESSAGE.to_string(),
                true,
            ));
            return;
        }

        if self.operation_registry.get_by_keybinding(op_key).is_some() && self.kube_client.is_some()
        {
            // Mark operation as pending - will be executed in main loop
            self.async_state.operation.request(PendingOperation::new(
                resource_type.to_string(),
                namespace.to_string(),
                name.to_string(),
                op_key,
            ));
        }
    }

    /// Namespace hotkeys (0-9): one code path with `:ns`, so everything that
    /// follows the namespace (events feed, kind lists) follows hotkeys too.
    pub(super) fn namespace_hotkey(&mut self, c: char) {
        let Some(index) = c.to_digit(10).map(|d| d as usize) else {
            return;
        };
        let Some(ns_name) = self.namespace_hotkeys.get(index).cloned() else {
            return;
        };
        let new_namespace = (ns_name != "all").then(|| ns_name.clone());
        if self.namespace != new_namespace {
            self.switch_namespace(new_namespace);
            if self.ui_state.status_message.is_none() {
                self.set_status_message((format!("Switched to namespace: {}", ns_name), false));
            }
        }
    }

    /// `t`: trace the current resource.
    pub(super) fn trace_selected(&mut self) {
        // Trace command - works from list, favorites, and detail view
        if let Some(resource) = self.get_current_resource() {
            self.async_state.trace.request(ResourceKey::new(
                resource.resource_type.clone(),
                resource.namespace.clone(),
                resource.name.clone(),
            ));
            self.view_state.trace_scroll_offset = 0;
        }
    }

    /// `/`: search inside text views, filter the rest.
    pub(super) fn start_filter_or_search(&mut self) {
        if self.is_text_search_view() {
            // Search within the current text view (YAML/describe/trace)
            self.view_state.text_search.clear();
            self.view_state.text_search.input_mode = true;
        } else {
            // Enter filter mode
            self.view_state.filter_mode = true;
            self.view_state.filter.clear();
            self.invalidate_layout_cache(); // Filter state affects header height
        }
    }

    /// `N`/`A`/`T`/`S`: sort the Flux list (k9s-style shift-key sort).
    pub(super) fn sort_by_key(&mut self, c: char) {
        use crate::tui::app::state::SortField;
        let field = match c {
            'N' => SortField::Name,
            'A' => SortField::Age,
            'T' => SortField::Type,
            _ => SortField::Status,
        };
        self.toggle_sort(field);
    }

    /// `e`: edit the current Flux resource in the system editor.
    pub(super) fn edit_selected(&mut self) {
        let native = self.native_view_target();
        if let Some(target) = native.as_ref().filter(|t| !self.native_editable(t)) {
            // Not editable here: say why and point at kubectl.
            self.set_status_message((native_edit_message(target), true));
            return;
        }
        if self.config.read_only {
            self.set_status_message(("Editing disabled in read-only mode".to_string(), true));
            return;
        }
        let target = match native {
            // Native: fetch it and check who manages it before the editor
            // opens (the warning comes from `on_edit_preflight`).
            Some(target) => {
                self.async_state.edit_preflight.request(target.clone());
                target
            }
            None => match self.get_current_resource() {
                Some(resource) => {
                    let rk = ResourceKey::new(
                        resource.resource_type.clone(),
                        resource.namespace.clone(),
                        resource.name.clone(),
                    );
                    // Flux resources go straight to the editor (via the YAML fetch).
                    self.async_state.yaml.request(rk.clone().into());
                    ObjectRef::from(rk)
                }
                None => {
                    self.set_status_message(("No resource selected for editing".to_string(), true));
                    return;
                }
            },
        };
        self.async_state.edit_pending = Some(target);
        self.async_state.edit_full_yaml = None;
        self.async_state.edit_editor_launched = false;
        self.async_state.edit_error_message = None;
        // Back (cancel/save) returns to the exact view `e` came from.
        self.navigate_to(View::ResourceEdit);
    }

    /// Whether `e` can edit a native object: its kind is patchable
    /// (discovery's verbs) and isn't a Secret.
    pub(super) fn native_editable(&self, target: &ObjectRef) -> bool {
        if self.view_state.current_view == View::KindList {
            return self.kind_list_caps().edit;
        }
        let Some(api_version) = target.api_version.as_deref() else {
            return true; // A Flux resource reached through a native path.
        };
        let group = api_version.split_once('/').map_or("", |(group, _)| group);
        crate::models::kinds::catalog()
            .find_by_group_kind(group, &target.kind)
            .is_some_and(|spec| spec.caps.edit)
    }

    /// The native edit pre-check finished: warn when something manages the
    /// object, else open the editor.
    pub fn on_edit_preflight(&mut self, preflight: super::state::EditPreflight) {
        let editing = self.view_state.current_view == View::ResourceEdit;
        let Some(target) = self.async_state.edit_pending.clone().filter(|_| editing) else {
            return; // The user left the edit meanwhile.
        };
        match self.edit_warning(&target, &preflight.manager, preflight.manager_suspended) {
            Some(warning) => {
                self.async_state.edit_preflight.set_result(preflight);
                self.async_state.confirmation_pending = Some(Confirmation::Edit(warning));
            }
            None => self.async_state.edit_full_yaml = Some(preflight.object),
        }
    }

    /// The edit confirmation was answered.
    pub(super) fn answer_edit_warning(&mut self, proceed: bool) {
        match self.async_state.edit_preflight.result().cloned() {
            Some(preflight) if proceed => {
                self.async_state.edit_full_yaml = Some(preflight.object);
            }
            _ => {
                self.set_status_message(("Edit cancelled".to_string(), false));
                self.leave_edit();
            }
        }
    }

    /// The warning for editing `target`, or `None` when nothing manages it.
    /// A Flux manager that is suspended gets its own wording: the change
    /// sticks until it's resumed. Suspension comes from the watched state,
    /// else from `fetched_suspended` (read from the cluster by the
    /// pre-check, for managers outside the watched namespaces).
    pub(super) fn edit_warning(
        &self,
        target: &ObjectRef,
        lookup: &crate::kube::ownership::ManagerLookup,
        fetched_suspended: Option<bool>,
    ) -> Option<String> {
        use crate::kube::ownership::{Manager, ManagerLookup, Resolved};
        let object = format!("{} {}", target.kind, target.name);
        let (manager, via) = match lookup {
            ManagerLookup::Unmanaged => return None,
            ManagerLookup::Managed(manager) => (manager, String::new()),
            ManagerLookup::Owned {
                owner,
                resolved: Some(Resolved::Manager(manager)),
                ..
            } => (manager, format!(" (via {}/{})", owner.kind, owner.name)),
            ManagerLookup::Owned {
                resolved: Some(Resolved::Owner { kind, name }),
                ..
            } => {
                return Some(format!(
                    "{object} is owned by {kind}/{name} — its controller may overwrite your change. Edit anyway?"
                ));
            }
            ManagerLookup::Owned { owner, .. } => {
                return Some(format!(
                    "{object} is owned by {}/{} (its manager couldn't be checked) — a controller may overwrite your change. Edit anyway?",
                    owner.kind, owner.name
                ));
            }
        };
        let consequence = match manager {
            Manager::Flux {
                kind,
                namespace,
                name,
            } => {
                let key = crate::watcher::resource_key(namespace, name, kind);
                let suspended = self
                    .state
                    .get(&key)
                    .and_then(|r| r.suspended)
                    .or(fetched_suspended);
                match suspended {
                    Some(true) => format!(
                        ", which is suspended — your change stays until {kind} {name} is resumed, then its next reconcile reverts it"
                    ),
                    Some(false) => " — its next reconcile will revert your change".to_string(),
                    None => " — unless it's suspended, its next reconcile will revert your change"
                        .to_string(),
                }
            }
            Manager::Helm { .. } => {
                " — the next helm upgrade may overwrite your change".to_string()
            }
            Manager::ArgoCd { .. } => {
                " — Argo CD may revert it (self-heal) or report it OutOfSync".to_string()
            }
            Manager::Other(_) => " — it may overwrite your change".to_string(),
        };
        Some(format!(
            "{object} is managed by {}{via}{consequence}. Edit anyway?",
            manager.describe()
        ))
    }

    /// Enter in the Flux list: open the selected resource's detail.
    pub(super) fn open_selected_detail(&mut self) {
        let resources = self.get_filtered_resources();
        if let Some(resource) = resources.get(self.view_state.selected_index) {
            let key = crate::watcher::resource_key(
                &resource.namespace,
                &resource.name,
                &resource.resource_type,
            );
            self.selection_state.selected_resource_key = Some(key);
            self.navigate_to(View::ResourceDetail);
        }
    }

    /// `f`: toggle the selected resource as a favorite.
    pub(super) fn toggle_selected_favorite(&mut self) {
        let resources = self.get_filtered_resources();
        if let Some(resource) = resources.get(self.view_state.selected_index) {
            let key = crate::watcher::resource_key(
                &resource.namespace,
                &resource.name,
                &resource.resource_type,
            );
            self.toggle_favorite(&key);
            self.set_status_message((
                if self.is_favorite(&key) {
                    format!("Added {} to favorites", resource.name)
                } else {
                    format!("Removed {} from favorites", resource.name)
                },
                false,
            ));
        }
    }

    /// `h`: reconciliation history of the current resource.
    pub(super) fn open_history(&mut self) {
        // View reconciliation history - works from list, favorites, and detail view
        if let Some(resource) = self.get_current_resource() {
            use crate::models::FluxResourceKind;

            let key = crate::watcher::resource_key(
                &resource.namespace,
                &resource.name,
                &resource.resource_type,
            );

            // Check if resource object exists and has status.history
            let obj = self.resource_objects.get(&key);
            let has_history = obj
                .and_then(|obj| obj.get("status"))
                .and_then(|s| s.get("history"))
                .and_then(|h| h.as_array())
                .map(|arr| !arr.is_empty())
                .unwrap_or(false);
            let is_kustomization = matches!(
                FluxResourceKind::parse_optional(&resource.resource_type),
                Some(FluxResourceKind::Kustomization)
            );

            if has_history {
                self.selection_state.selected_resource_key = Some(key);
                self.navigate_to(View::ResourceHistory);
                self.view_state.history_scroll_offset = 0;
            } else {
                // Show error message immediately
                let error_msg = if is_kustomization {
                    format!(
                        "Reconciliation history is not supported for Kustomization '{}' in this version of Flux. History requires Flux v2.3.0 or later.",
                        resource.name
                    )
                } else {
                    let supported_types: Vec<String> = FluxResourceKind::history_supported_types()
                        .iter()
                        .map(|k| k.as_str().to_string())
                        .collect();
                    format!(
                        "Resource '{}' does not have reconciliation history. History is only available for: {}",
                        resource.name,
                        supported_types.join(", ")
                    )
                };
                self.set_status_message((error_msg, true));
            }
        } else {
            self.set_status_message(("No resource selected".to_string(), true));
        }
    }

    /// `g`: graph of the current resource.
    pub(super) fn open_graph(&mut self) {
        // View resource graph - works from list, favorites, and detail view
        if let Some(resource) = self.get_current_resource() {
            // Check if resource type supports graph view
            if !crate::trace::is_resource_type_with_graph(&resource.resource_type) {
                self.set_status_message((
                    format!(
                        "Graph view not supported for {} resources",
                        resource.resource_type
                    ),
                    true,
                ));
                return;
            }

            // Trigger graph building
            let key = crate::watcher::resource_key(
                &resource.namespace,
                &resource.name,
                &resource.resource_type,
            );

            self.selection_state.selected_resource_key = Some(key.clone());
            self.async_state.graph.request(ResourceKey {
                resource_type: resource.resource_type.clone(),
                namespace: resource.namespace.clone(),
                name: resource.name.clone(),
            });
            self.view_state.graph_scroll_offset = 0; // Reset scroll
            self.view_state.graph_focus_index = None; // Reset focus (set when graph loads)
            self.navigate_to(View::ResourceGraph);
        } else {
            self.set_status_message(("No resource selected".to_string(), true));
        }
    }

    /// Enter / `l` in the workload list: open the selected workload's detail
    /// (with `then_logs`, continue into its pod logs).
    pub(super) fn open_selected_workload(&mut self, then_logs: bool) {
        if let Some(row) = self
            .view_state
            .workload_rows
            .get(self.view_state.selected_index)
            .cloned()
        {
            self.open_workload_detail(row.kind, row.namespace, row.name, then_logs);
        }
    }
}
