//! Trace, graph, and jump to what manages a native object (#266).
//!
//! The native views — kind lists, describe / YAML / trace of a native
//! object, and workload detail — know which Flux object manages what they
//! show: from the kind list's resolved owner chains, the describe and YAML
//! Managed By resolution, the workload's Managed By, or the trace itself.
//! `t`, `g`, and `m` act on that manager, and the action table offers them
//! only once it is known ([`App::target_flux_manager`]), so the footer
//! never shows them for objects Flux doesn't manage.

use super::core::App;
use super::state::{GraphRequest, TraceRequest, TraceResponse, View};
use crate::kube::objects::ObjectRef;
use crate::watcher::ResourceKey;

impl App {
    /// The native object the current view shows (what `t` traces and `g`
    /// marks in its manager's graph). Exhaustive on purpose: a new view
    /// must decide whether it shows one.
    pub(crate) fn native_subject(&self) -> Option<ObjectRef> {
        match self.view_state.current_view {
            View::KindList => self.native_view_target(),
            View::WorkloadDetail => self.async_state.workload.result().map(|w| ObjectRef {
                kind: w.kind.clone(),
                namespace: w.namespace.clone(),
                name: w.name.clone(),
                api_version: w.api_version.clone(),
            }),
            View::ResourceDescribe | View::ResourceYAML | View::ResourceTrace => {
                self.selection_state.native_object.clone()
            }
            View::ResourceList
            | View::ResourceDetail
            | View::ResourceGraph
            | View::ResourceFavorites
            | View::ResourceHistory
            | View::EventList
            | View::Logs
            | View::WorkloadList
            | View::InventoryList
            | View::Pulse
            | View::HelmValues
            | View::ResourceEdit
            | View::Help => None,
        }
    }

    /// The Flux object managing the current native subject, directly or
    /// through its owner chain — `None` until that is known, and for
    /// objects something else (or nothing) manages. Exhaustive on purpose,
    /// like [`Self::native_subject`].
    pub(crate) fn target_flux_manager(&self) -> Option<ResourceKey> {
        let native = self.selection_state.native_object.is_some();
        match self.view_state.current_view {
            View::KindList => self.selected_kind_row()?.flux_manager(),
            View::WorkloadDetail => self
                .async_state
                .workload
                .result()?
                .managed_by
                .as_ref()?
                .flux
                .clone(),
            View::ResourceDescribe => {
                if !native {
                    return None;
                }
                self.async_state
                    .describe
                    .result()?
                    .managed_by
                    .as_ref()?
                    .flux
                    .clone()
            }
            View::ResourceYAML => {
                if !native {
                    return None;
                }
                let object = self.async_state.yaml.result()?;
                match crate::kube::ownership::immediate_manager(object) {
                    Some(managed_by) => managed_by.flux,
                    None => self
                        .view_state
                        .yaml_manager
                        .as_ref()
                        .filter(|r| Some(r.uid.as_str()) == object_uid(object))?
                        .managed_by
                        .flux
                        .clone(),
                }
            }
            View::ResourceTrace => {
                if !native {
                    return None;
                }
                self.trace_result()?.flux_manager().map(|n| n.key())
            }
            View::ResourceList
            | View::ResourceDetail
            | View::ResourceGraph
            | View::ResourceFavorites
            | View::ResourceHistory
            | View::EventList
            | View::Logs
            | View::WorkloadList
            | View::InventoryList
            | View::Pulse
            | View::HelmValues
            | View::ResourceEdit
            | View::Help => None,
        }
    }

    /// The trace on screen, if one was found.
    pub(crate) fn trace_result(&self) -> Option<&crate::trace::TraceResult> {
        self.async_state
            .trace
            .result()
            .and_then(TraceResponse::traced)
    }

    /// `t` on a native object: trace it up to its Flux manager and source.
    pub(super) fn trace_native_subject(&mut self) {
        let Some(target) = self.native_subject() else {
            return;
        };
        self.request_trace_of(target);
    }

    /// Trace `target` (a native object, or a Flux resource without an
    /// `apiVersion`). Nothing on screen changes until the trace succeeds:
    /// the trace view and its subject arrive together
    /// ([`Self::set_trace_response`]).
    pub(super) fn request_trace_of(&mut self, target: ObjectRef) {
        self.async_state.trace.request(TraceRequest::Object(target));
    }

    /// `g` on a native object: its Flux manager's graph, focused on the
    /// node that lists it (the object, or the owner the manager applied).
    pub(super) fn open_manager_graph(&mut self) {
        let Some(manager) = self.target_flux_manager() else {
            return;
        };
        if !crate::trace::is_resource_type_with_graph(&manager.resource_type) {
            self.set_status_message((
                format!(
                    "Graph view not supported for {} resources",
                    manager.resource_type
                ),
                true,
            ));
            return;
        }
        let highlight = self.native_subject();
        // The graph is the manager's: Flux keys apply there.
        self.selection_state.native_object = None;
        self.selection_state.selected_resource_key = Some(manager.to_key_string());
        self.async_state.graph.request(GraphRequest {
            root: manager,
            highlight,
        });
        self.view_state.graph_scroll_offset = 0;
        self.view_state.graph_focus_index = None;
        self.view_state.graph_highlight = None;
        self.navigate_to(View::ResourceGraph);
    }

    /// `m`: open the Flux object managing the current native object — its
    /// detail view when flux9s watches it, else its describe (fetched, so a
    /// manager outside the watched namespaces still opens).
    pub(super) fn jump_to_manager(&mut self) {
        let Some(manager) = self.target_flux_manager() else {
            return;
        };
        let key = manager.to_key_string();
        self.selection_state.native_object = None;
        self.selection_state.selected_resource_key = Some(key.clone());
        if self.state.get(&key).is_some() {
            self.navigate_to(View::ResourceDetail);
        } else {
            self.async_state.describe.request(manager.into());
            self.view_state.describe_scroll_offset = 0;
            self.view_state.text_search.clear();
            self.navigate_to(View::ResourceDescribe);
        }
    }

    /// Store a trace answer: open the trace, or report what went wrong
    /// (several matches for a search, not found, unreadable).
    pub fn set_trace_response(&mut self, result: anyhow::Result<TraceResponse>) {
        match result {
            Ok(TraceResponse::Traced { target, trace }) => {
                // The subject: a native object keeps its apiVersion (so y / d
                // / g / m act on it); a Flux resource is a plain key.
                self.selection_state.selected_resource_key =
                    Some(target.to_resource_key().to_key_string());
                self.selection_state.native_object =
                    target.api_version.is_some().then(|| target.clone());
                self.async_state
                    .trace
                    .set_result(TraceResponse::Traced { target, trace });
                self.set_view_trace();
            }
            Ok(TraceResponse::Ambiguous { target, namespaces }) => {
                self.async_state.trace.set_error();
                self.set_status_message((
                    format!(
                        "{target} is in {} namespaces ({}) — use :trace {target} -n <namespace>",
                        namespaces.len(),
                        namespaces.join(", ")
                    ),
                    true,
                ));
            }
            Err(e) => {
                self.async_state.trace.set_error();
                // `{:#}`: the cause too ("… deployments.apps \"x\" not found").
                self.set_status_message((format!("Trace failed: {e:#}"), true));
            }
        }
    }
}

fn object_uid(object: &serde_json::Value) -> Option<&str> {
    object
        .pointer("/metadata/uid")
        .and_then(serde_json::Value::as_str)
}
