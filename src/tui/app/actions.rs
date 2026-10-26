//! The action table (#276): one source for key dispatch, footer hints, and
//! help.
//!
//! Every key the normal (non-modal) key handler understands is an
//! [`Action`] in [`ACTIONS`]: its keys, when it applies, what it runs, its
//! footer hint, and its help line. Dispatch takes the first action whose key
//! matches and whose `when` holds, so table order is precedence (view-specific
//! entries sit above the general ones they override).
//!
//! Footers are per-context *layouts* of action ids ([`footer_layout`]),
//! filtered by `when` — a footer can only advertise a key that actually works
//! in the current view, and it always shows what dispatch would run. Help is
//! generated from the same table, plus the `:` command help in
//! `command_mode.rs`.
//!
//! [`ActionSpec`] is data so later config-defined actions (#277) can reuse the
//! same dispatch, confirmation, and read-only gate.

use super::core::App;
use super::state::View;
use crate::models::kinds::Capabilities;
use crate::tui::keybindings::NavigationCommand;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// A key binding. Only the Control modifier is significant: terminals report
/// Shift on uppercase letters inconsistently, so `R` matches with or without
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Key {
    pub code: KeyCode,
    pub ctrl: bool,
}

const fn key(c: char) -> Key {
    Key {
        code: KeyCode::Char(c),
        ctrl: false,
    }
}

const fn ctrl(c: char) -> Key {
    Key {
        code: KeyCode::Char(c),
        ctrl: true,
    }
}

const fn code(code: KeyCode) -> Key {
    Key { code, ctrl: false }
}

impl Key {
    fn matches(&self, event: &KeyEvent) -> bool {
        self.code == event.code && self.ctrl == event.modifiers.contains(KeyModifiers::CONTROL)
    }
}

/// What an action does. The write variants are plain data so config-defined
/// actions (#277) can reuse the same confirmation, read-only gate, footer,
/// and help; `&'static str` fields become owned strings when config can
/// supply them.
pub(crate) enum ActionSpec {
    /// App code. Returns `Some(true)` to quit, like `handle_key`.
    Builtin(fn(&mut App, KeyEvent) -> Option<bool>),
    /// A registered [`crate::operations::FluxOperation`], by its keybinding:
    /// validity check, read-only gate, and confirmation are shared.
    Operation(char),
    /// A JSON merge patch on the action's target. `{{now}}` is replaced with
    /// the current time (RFC 3339, UTC).
    Patch { patch: &'static str, write: Write },
    /// Set one annotation on the target (`{{now}}` allowed in the value).
    Annotate {
        key: &'static str,
        value: &'static str,
        write: Write,
    },
}

/// How a write action is confirmed, reported, and limited.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Write {
    /// Imperative for the confirmation ("Restart").
    pub verb: &'static str,
    /// Past tense for the status line ("Restarted").
    pub done: &'static str,
    /// Extra sentence in the confirmation ("All pods will be rolled.").
    pub detail: &'static str,
    /// Kinds it applies to; empty means any.
    pub kinds: &'static [&'static str],
}

impl ActionSpec {
    /// The merge patch a write action applies at `now`, or `None` for
    /// non-write actions.
    pub(crate) fn patch_document(&self, now: &str) -> Option<anyhow::Result<serde_json::Value>> {
        match self {
            ActionSpec::Patch { patch, .. } => Some(
                serde_json::from_str(&patch.replace("{{now}}", now))
                    .map_err(|e| anyhow::anyhow!("Invalid patch in action: {e}")),
            ),
            ActionSpec::Annotate { key, value, .. } => Some(Ok(serde_json::json!({
                "metadata": {"annotations": {*key: value.replace("{{now}}", now)}}
            }))),
            ActionSpec::Builtin(_) | ActionSpec::Operation(_) => None,
        }
    }

    /// Confirmation / status wording and kind filter of a write action.
    pub(crate) fn write_info(&self) -> Option<&Write> {
        match self {
            ActionSpec::Patch { write, .. } | ActionSpec::Annotate { write, .. } => Some(write),
            ActionSpec::Builtin(_) | ActionSpec::Operation(_) => None,
        }
    }
}

/// Help overlay column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HelpColumn {
    Resource,
    General,
    Navigation,
}

/// Stable identity of an action, used by footer layouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActionId {
    KindLogs,
    Restart,
    KindDeletePod,
    KindEnter,
    KindDescribe,
    NamespaceHotkey,
    Quit,
    Page,
    WorkloadDeletePod,
    Delete,
    Back,
    QuitNow,
    Help,
    Suspend,
    Resume,
    Reconcile,
    ReconcileWithSource,
    NativeTrace,
    Orphans,
    Rescan,
    NativeGraph,
    JumpToManager,
    Trace,
    CommandMode,
    LogsFollow,
    Move,
    FilterOrSearch,
    NextMatch,
    Sort,
    InventoryDescribe,
    Yaml,
    Describe,
    Edit,
    EditHint,
    GraphEnter,
    EventEnter,
    WorkloadListEnter,
    Values,
    RevealSecrets,
    LogContainer,
    WorkloadLogs,
    WorkloadListLogs,
    ListEnter,
    Favorite,
    History,
    Graph,
    Backspace,
}

pub(crate) struct Action {
    pub id: ActionId,
    pub keys: &'static [Key],
    /// When the action applies (view, capabilities). Selection-dependent
    /// checks ("nothing selected") stay in the action and report a status.
    pub when: fn(&App) -> bool,
    pub spec: ActionSpec,
    /// Default footer hint: key label and label.
    pub footer: Option<(&'static str, &'static str)>,
    /// Help line: column, key label, description.
    pub help: Option<(HelpColumn, &'static str, &'static str)>,
    /// Keeps the status line when pressed (the action sets its own message).
    pub sets_status: bool,
}

fn always(_: &App) -> bool {
    true
}

fn in_view(app: &App, views: &[View]) -> bool {
    views.contains(&app.view_state.current_view)
}

fn kind_caps(app: &App, has: fn(&Capabilities) -> bool) -> bool {
    app.view_state.current_view == View::KindList && has(&app.kind_list_caps())
}

/// Views that act on a Flux resource: Flux operations, `t`, `h`, `g`, `v`.
fn on_flux_resource(app: &App) -> bool {
    app.selection_state.native_object.is_none()
        && in_view(
            app,
            &[
                View::ResourceList,
                View::ResourceFavorites,
                View::ResourceDetail,
                View::ResourceDescribe,
                View::ResourceYAML,
                View::ResourceTrace,
                View::ResourceGraph,
                View::ResourceHistory,
                View::HelmValues,
                View::EventList,
            ],
        )
}

/// A native object whose Flux manager is known (#266): `t`, `g`, and `m`
/// act on that manager, so they show only once it has resolved.
fn has_flux_manager(app: &App) -> bool {
    app.target_flux_manager().is_some()
}

/// Whether `e` would edit the current target: always for Flux resources;
/// for native objects when the kind is patchable and isn't a Secret.
fn can_edit(app: &App) -> bool {
    match app.native_view_target() {
        Some(target) => app.native_editable(&target),
        None => true,
    }
}

/// Views where `y` / `d` have a target.
fn has_view_target(app: &App) -> bool {
    !in_view(
        app,
        &[
            View::Logs,
            View::WorkloadList,
            View::WorkloadDetail,
            View::Pulse,
            View::ResourceEdit,
            View::Help,
        ],
    )
}

const fn op(c: char) -> ActionSpec {
    ActionSpec::Operation(c)
}

/// Every key of the normal key handler, in precedence order.
pub(crate) const ACTIONS: &[Action] = &[
    // Kind-list actions, gated by the kind's capabilities (deny by default).
    Action {
        id: ActionId::KindLogs,
        keys: &[key('l')],
        when: |app| kind_caps(app, |c| c.pod_logs),
        spec: ActionSpec::Builtin(|app, _| {
            app.kind_logs_selected();
            None
        }),
        footer: Some(("l", "Logs")),
        help: Some((HelpColumn::Navigation, "<l>", "Pod logs (workloads, :po)")),
        sets_status: false,
    },
    Action {
        id: ActionId::KindDeletePod,
        keys: &[ctrl('d')],
        when: |app| kind_caps(app, |c| c.pod_delete),
        spec: ActionSpec::Builtin(|app, _| {
            app.kind_delete_pod_selected();
            None
        }),
        footer: Some(("^d", "Delete pod")),
        help: None,
        sets_status: true,
    },
    Action {
        id: ActionId::NamespaceHotkey,
        keys: &[
            key('0'),
            key('1'),
            key('2'),
            key('3'),
            key('4'),
            key('5'),
            key('6'),
            key('7'),
            key('8'),
            key('9'),
        ],
        when: always,
        spec: ActionSpec::Builtin(|app, event| {
            if let KeyCode::Char(c) = event.code {
                app.namespace_hotkey(c);
            }
            None
        }),
        footer: None,
        help: None,
        sets_status: false,
    },
    // Ctrl+C arrives as a key in raw mode: unconditional quit.
    Action {
        id: ActionId::Quit,
        keys: &[ctrl('c')],
        when: always,
        spec: ActionSpec::Builtin(|_, _| Some(true)),
        footer: None,
        help: None,
        sets_status: false,
    },
    Action {
        id: ActionId::Page,
        keys: &[
            ctrl('f'),
            code(KeyCode::PageDown),
            ctrl('b'),
            code(KeyCode::PageUp),
        ],
        when: always,
        spec: ActionSpec::Builtin(|app, event| {
            let page_size = app.view_state.page_size;
            if matches!(event.code, KeyCode::PageDown | KeyCode::Char('f')) {
                app.scroll_down(page_size);
            } else {
                app.scroll_up(page_size);
            }
            None
        }),
        footer: Some(("^f/^b", "PgDn/Up")),
        help: Some((
            HelpColumn::Navigation,
            "<Ctrl+f/b>/<PgDn/Up>",
            "Page down / up",
        )),
        sets_status: false,
    },
    // In workload detail, Ctrl+d deletes a pod (#263); the workload itself
    // is owned by Flux.
    Action {
        id: ActionId::WorkloadDeletePod,
        keys: &[ctrl('d')],
        when: |app| in_view(app, &[View::WorkloadDetail]),
        spec: ActionSpec::Builtin(|app, _| {
            app.request_pod_delete();
            None
        }),
        footer: Some(("^d", "Delete pod")),
        help: Some((
            HelpColumn::Navigation,
            "<Ctrl+d>",
            "Delete pod (workload detail, :po)",
        )),
        sets_status: true,
    },
    Action {
        id: ActionId::Delete,
        keys: &[ctrl('d')],
        when: on_flux_resource,
        spec: op('d'),
        footer: Some(("^d", "Delete")),
        help: Some((HelpColumn::Resource, "<Ctrl+d>", "Delete resource")),
        sets_status: true,
    },
    // q / Esc go back a level (k9s: never exit directly); at the top a
    // confirmation is shown. Esc first clears an active text search.
    Action {
        id: ActionId::Back,
        keys: &[code(KeyCode::Esc), key('q')],
        when: always,
        spec: ActionSpec::Builtin(|app, event| {
            if event.code == KeyCode::Esc
                && app.view_state.current_view.is_text_search_view()
                && app.view_state.text_search.is_active()
            {
                app.view_state.text_search.clear();
                return None;
            }
            app.navigate_back_or_confirm_quit()
        }),
        footer: Some(("Esc/q", "Back")),
        help: Some((
            HelpColumn::General,
            "<q>/<Esc>",
            "Back (confirm quit at root)",
        )),
        sets_status: false,
    },
    Action {
        id: ActionId::QuitNow,
        keys: &[key('Q')],
        when: always,
        spec: ActionSpec::Builtin(|_, _| Some(true)),
        footer: None,
        help: Some((HelpColumn::General, "<Q>", "Quit immediately")),
        sets_status: false,
    },
    Action {
        id: ActionId::Help,
        keys: &[key('?')],
        when: always,
        spec: ActionSpec::Builtin(|app, _| {
            app.toggle_help();
            None
        }),
        footer: Some(("?", "Help")),
        help: Some((HelpColumn::General, "<?>", "Show/hide help")),
        sets_status: false,
    },
    // Rollout restart (#263), k9s-style — a data-driven patch: the same
    // shape a config-defined action will have (#277).
    Action {
        id: ActionId::Restart,
        keys: &[key('r')],
        when: |app| {
            in_view(app, &[View::WorkloadList, View::WorkloadDetail])
                || kind_caps(app, |c| c.restart)
        },
        spec: ActionSpec::Patch {
            patch: crate::kube::workloads::RESTART_PATCH,
            write: Write {
                verb: "Restart",
                done: "Restarted",
                detail: "All pods will be rolled.",
                kinds: crate::kube::workloads::RESTART_KINDS,
            },
        },
        footer: Some(("r", "Restart")),
        help: Some((
            HelpColumn::Navigation,
            "<r>",
            "Restart workload (workloads, :deploy)",
        )),
        sets_status: true,
    },
    Action {
        id: ActionId::Suspend,
        keys: &[key('s')],
        when: on_flux_resource,
        spec: op('s'),
        footer: Some(("s", "Suspend")),
        help: Some((HelpColumn::Resource, "<s>", "Suspend reconciliation")),
        sets_status: true,
    },
    Action {
        id: ActionId::Resume,
        keys: &[key('r')],
        when: on_flux_resource,
        spec: op('r'),
        footer: Some(("r", "Resume")),
        help: Some((HelpColumn::Resource, "<r>", "Resume reconciliation")),
        sets_status: true,
    },
    Action {
        id: ActionId::Reconcile,
        keys: &[key('R')],
        when: on_flux_resource,
        spec: op('R'),
        footer: Some(("R", "Reconcile")),
        help: Some((HelpColumn::Resource, "<R>", "Reconcile resource")),
        sets_status: true,
    },
    Action {
        id: ActionId::ReconcileWithSource,
        keys: &[key('W')],
        when: on_flux_resource,
        spec: op('W'),
        footer: Some(("W", "Reconcile+Source")),
        help: Some((HelpColumn::Resource, "<W>", "Reconcile with source")),
        sets_status: true,
    },
    // Native objects (#266): trace / graph / open what manages them.
    Action {
        id: ActionId::NativeTrace,
        keys: &[key('t')],
        // Already on its trace: nothing more to trace.
        when: |app| app.view_state.current_view != View::ResourceTrace && has_flux_manager(app),
        spec: ActionSpec::Builtin(|app, _| {
            app.trace_native_subject();
            None
        }),
        footer: Some(("t", "Trace")),
        help: None,
        sets_status: false,
    },
    Action {
        id: ActionId::NativeGraph,
        keys: &[key('g')],
        when: has_flux_manager,
        spec: ActionSpec::Builtin(|app, _| {
            app.open_manager_graph();
            None
        }),
        footer: Some(("g", "Graph")),
        help: None,
        sets_status: false,
    },
    Action {
        id: ActionId::JumpToManager,
        keys: &[key('m')],
        when: has_flux_manager,
        spec: ActionSpec::Builtin(|app, _| {
            app.jump_to_manager();
            None
        }),
        footer: Some(("m", "Manager")),
        help: Some((
            HelpColumn::Resource,
            "<m>",
            "Open the Flux object managing it (Kubernetes objects)",
        )),
        sets_status: false,
    },
    Action {
        id: ActionId::Trace,
        keys: &[key('t')],
        when: on_flux_resource,
        spec: ActionSpec::Builtin(|app, _| {
            app.trace_selected();
            None
        }),
        footer: Some(("t", "Trace")),
        help: Some((
            HelpColumn::Resource,
            "<t>",
            "Trace ownership chain (any object Flux manages)",
        )),
        sets_status: false,
    },
    Action {
        id: ActionId::CommandMode,
        keys: &[key(':')],
        when: always,
        spec: ActionSpec::Builtin(|app, _| {
            app.ui_state.command_mode = true;
            app.ui_state.command_buffer.clear();
            None
        }),
        footer: Some((":", "Command")),
        help: Some((HelpColumn::General, "<:>", "Command mode")),
        sets_status: false,
    },
    // Jump to the newest log line and resume following.
    Action {
        id: ActionId::LogsFollow,
        keys: &[key('G')],
        when: |app| in_view(app, &[View::Logs]),
        spec: ActionSpec::Builtin(|app, _| {
            app.logs.follow = true;
            None
        }),
        footer: Some(("G", "Follow")),
        help: Some((
            HelpColumn::Navigation,
            "<G>",
            "Follow newest line (logs view)",
        )),
        sets_status: false,
    },
    Action {
        id: ActionId::Move,
        keys: &[key('j'), code(KeyCode::Down), key('k'), code(KeyCode::Up)],
        when: always,
        spec: ActionSpec::Builtin(|app, event| {
            // Max scroll in scrollable views is clamped during render.
            if matches!(event.code, KeyCode::Char('j') | KeyCode::Down) {
                app.scroll_down(1);
            } else {
                app.scroll_up(1);
            }
            None
        }),
        footer: Some(("j/k ", "Navigate")),
        help: Some((
            HelpColumn::Navigation,
            "<j/k>/<Down/Up>",
            "Navigate / scroll",
        )),
        sets_status: false,
    },
    // `/`: search inside text views, filter the rest.
    Action {
        id: ActionId::FilterOrSearch,
        keys: &[key('/')],
        when: always,
        spec: ActionSpec::Builtin(|app, _| {
            app.start_filter_or_search();
            None
        }),
        footer: Some(("/", "Filter/Search")),
        help: Some((
            HelpColumn::General,
            "</>",
            "Filter list / search text views",
        )),
        sets_status: false,
    },
    // Cycle search matches in text views (vim-style n/N).
    Action {
        id: ActionId::NextMatch,
        keys: &[key('n'), key('N')],
        when: |app| {
            app.view_state.current_view.is_text_search_view()
                && app.view_state.text_search.is_active()
        },
        spec: ActionSpec::Builtin(|app, event| {
            let delta = if event.code == KeyCode::Char('n') {
                1
            } else {
                -1
            };
            app.advance_text_search(delta);
            None
        }),
        footer: Some(("n/N", "Next/Prev")),
        help: Some((HelpColumn::Navigation, "<n>/<N>", "Next/prev search match")),
        sets_status: false,
    },
    // Column sorting in list views (k9s-style shift-key sort).
    Action {
        id: ActionId::Sort,
        keys: &[key('N'), key('A'), key('T'), key('S')],
        when: |app| app.view_state.current_view.is_list_view(),
        spec: ActionSpec::Builtin(|app, event| {
            if let KeyCode::Char(c) = event.code {
                app.sort_by_key(c);
            }
            None
        }),
        footer: Some(("N/A/T/S", "Sort")),
        help: Some((
            HelpColumn::Navigation,
            "<N>/<A>/<T>/<S>",
            "Sort name/age/type/status",
        )),
        sets_status: false,
    },
    // What a Kustomization / HelmRelease / ResourceSet / FluxInstance left
    // behind (#274).
    Action {
        id: ActionId::Orphans,
        keys: &[key('O')],
        // Sweeps list every kind: only with the native preview on.
        when: |app| on_flux_resource(app) && app.config.native_resources,
        spec: ActionSpec::Builtin(|app, _| {
            app.orphans_of_selected();
            None
        }),
        footer: Some(("O", "Orphans")),
        help: Some((
            HelpColumn::Resource,
            "<O>",
            "Objects this Flux object left behind",
        )),
        sets_status: false,
    },
    // Sweeps aren't live: run the current one again.
    Action {
        id: ActionId::Rescan,
        keys: &[ctrl('r')],
        when: |app| in_view(app, &[View::OwnershipList]),
        spec: ActionSpec::Builtin(|app, _| {
            app.rescan_sweep();
            None
        }),
        footer: Some(("Ctrl-r", "Rescan")),
        help: Some((
            HelpColumn::Navigation,
            "<Ctrl-r>",
            "Rescan (unmanaged / orphaned)",
        )),
        sets_status: false,
    },
    // Inventory objects aren't watched: Enter describes them.
    Action {
        id: ActionId::InventoryDescribe,
        keys: &[code(KeyCode::Enter), key('d')],
        when: |app| in_view(app, &[View::InventoryList, View::OwnershipList]),
        spec: ActionSpec::Builtin(|app, _| {
            app.open_describe_view();
            None
        }),
        footer: Some(("Enter/d", "Describe")),
        help: Some((
            HelpColumn::Navigation,
            "<Enter>/<y>",
            "Describe/YAML (inventory)",
        )),
        sets_status: false,
    },
    // Kind lists: workload kinds open the workload detail, others describe.
    Action {
        id: ActionId::KindEnter,
        keys: &[code(KeyCode::Enter)],
        when: |app| kind_caps(app, |c| c.workload_detail),
        spec: ActionSpec::Builtin(|app, _| {
            app.kind_enter_selected();
            None
        }),
        footer: Some(("Enter", "Details")),
        help: None,
        sets_status: false,
    },
    Action {
        id: ActionId::KindDescribe,
        keys: &[code(KeyCode::Enter)],
        when: |app| kind_caps(app, |c| !c.workload_detail),
        spec: ActionSpec::Builtin(|app, _| {
            app.kind_enter_selected();
            None
        }),
        footer: Some(("Enter", "Describe")),
        help: None,
        sets_status: false,
    },
    Action {
        id: ActionId::Yaml,
        keys: &[key('y')],
        when: has_view_target,
        spec: ActionSpec::Builtin(|app, _| {
            app.open_yaml_view();
            None
        }),
        footer: Some(("y", "YAML")),
        help: Some((HelpColumn::Resource, "<y>", "View YAML manifest")),
        sets_status: false,
    },
    Action {
        id: ActionId::Describe,
        keys: &[key('d')],
        when: has_view_target,
        spec: ActionSpec::Builtin(|app, _| {
            app.open_describe_view();
            None
        }),
        footer: Some(("d", "Describe")),
        help: Some((HelpColumn::Resource, "<d>", "Describe resource")),
        sets_status: false,
    },
    Action {
        id: ActionId::Edit,
        keys: &[key('e')],
        when: |app| has_view_target(app) && can_edit(app),
        spec: ActionSpec::Builtin(|app, _| {
            app.edit_selected();
            None
        }),
        footer: Some(("e", "Edit")),
        help: Some((
            HelpColumn::Resource,
            "<e>",
            "Edit resource in system editor",
        )),
        sets_status: true,
    },
    // `e` on something that can't be edited here: say why (Secrets, kinds
    // the API won't patch) and point at kubectl. No footer hint.
    Action {
        id: ActionId::EditHint,
        keys: &[key('e')],
        when: has_view_target,
        spec: ActionSpec::Builtin(|app, _| {
            app.edit_selected();
            None
        }),
        footer: None,
        help: None,
        sets_status: true,
    },
    // Drill into the focused graph node's resource.
    Action {
        id: ActionId::GraphEnter,
        keys: &[code(KeyCode::Enter)],
        when: |app| in_view(app, &[View::ResourceGraph]),
        spec: ActionSpec::Builtin(|app, _| {
            app.navigate_to_focused_graph_node();
            None
        }),
        footer: Some(("Enter", "Open")),
        help: Some((
            HelpColumn::Navigation,
            "<Enter>",
            "Open details / graph node / group",
        )),
        sets_status: false,
    },
    // Jump to the event's involved resource when flux9s watches it.
    Action {
        id: ActionId::EventEnter,
        keys: &[code(KeyCode::Enter)],
        when: |app| in_view(app, &[View::EventList]),
        spec: ActionSpec::Builtin(|app, _| {
            app.navigate_to_selected_event_resource();
            None
        }),
        footer: Some(("Enter", "Open")),
        help: None,
        sets_status: false,
    },
    Action {
        id: ActionId::WorkloadListEnter,
        keys: &[code(KeyCode::Enter)],
        when: |app| in_view(app, &[View::WorkloadList]),
        spec: ActionSpec::Builtin(|app, _| {
            app.open_selected_workload(false);
            None
        }),
        footer: Some(("Enter", "Details")),
        help: None,
        sets_status: false,
    },
    // HelmRelease effective values (#264).
    Action {
        id: ActionId::Values,
        keys: &[key('v')],
        when: on_flux_resource,
        spec: ActionSpec::Builtin(|app, _| {
            app.open_helm_values();
            None
        }),
        footer: Some(("v", "Values")),
        help: Some((HelpColumn::Resource, "<v>", "HelmRelease effective values")),
        sets_status: false,
    },
    Action {
        id: ActionId::RevealSecrets,
        keys: &[key('x')],
        when: |app| in_view(app, &[View::HelmValues]),
        spec: ActionSpec::Builtin(|app, _| {
            app.toggle_helm_values_secrets();
            None
        }),
        footer: Some(("x", "Reveal secrets")),
        help: Some((
            HelpColumn::Navigation,
            "<x>",
            "Reveal secrets (values view)",
        )),
        sets_status: false,
    },
    // Switch container in a multi-container pod's log stream.
    Action {
        id: ActionId::LogContainer,
        keys: &[key('c')],
        when: |app| in_view(app, &[View::Logs]),
        spec: ActionSpec::Builtin(|app, _| {
            app.open_log_container_picker();
            None
        }),
        footer: Some(("c", "Container")),
        help: Some((
            HelpColumn::Navigation,
            "<c>",
            "Switch container (logs view)",
        )),
        sets_status: false,
    },
    // Stream logs for the detailed workload's pods (#194).
    Action {
        id: ActionId::WorkloadLogs,
        keys: &[key('l')],
        when: |app| in_view(app, &[View::WorkloadDetail]),
        spec: ActionSpec::Builtin(|app, _| {
            app.open_workload_pod_logs();
            None
        }),
        footer: Some(("l", "Logs")),
        help: None,
        sets_status: false,
    },
    // `l` straight from the workload list: fetch the workload and open its
    // pod logs as soon as the data arrives.
    Action {
        id: ActionId::WorkloadListLogs,
        keys: &[key('l')],
        when: |app| in_view(app, &[View::WorkloadList]),
        spec: ActionSpec::Builtin(|app, _| {
            app.open_selected_workload(true);
            None
        }),
        footer: Some(("l", "Logs")),
        help: None,
        sets_status: false,
    },
    Action {
        id: ActionId::ListEnter,
        keys: &[code(KeyCode::Enter)],
        when: |app| app.view_state.current_view.is_list_view(),
        spec: ActionSpec::Builtin(|app, _| {
            app.open_selected_detail();
            None
        }),
        footer: Some(("Enter", "Details")),
        help: Some((HelpColumn::Resource, "<Enter>", "View resource details")),
        sets_status: false,
    },
    Action {
        id: ActionId::Favorite,
        keys: &[key('f')],
        when: |app| app.view_state.current_view.is_list_view(),
        spec: ActionSpec::Builtin(|app, _| {
            app.toggle_selected_favorite();
            None
        }),
        footer: Some(("f", "Favorite")),
        help: Some((HelpColumn::Resource, "<f>", "Toggle favorite")),
        sets_status: false,
    },
    Action {
        id: ActionId::History,
        keys: &[key('h')],
        when: on_flux_resource,
        spec: ActionSpec::Builtin(|app, _| {
            app.open_history();
            None
        }),
        footer: Some(("h", "History")),
        help: Some((HelpColumn::Resource, "<h>", "View reconciliation history")),
        sets_status: false,
    },
    Action {
        id: ActionId::Graph,
        keys: &[key('g')],
        when: on_flux_resource,
        spec: ActionSpec::Builtin(|app, _| {
            app.open_graph();
            None
        }),
        footer: Some(("g", "Graph")),
        help: Some((
            HelpColumn::Resource,
            "<g>",
            "View resource graph (of the managing Flux object)",
        )),
        sets_status: false,
    },
    // Backspace mirrors Esc, minus the quit prompt at the top.
    Action {
        id: ActionId::Backspace,
        keys: &[code(KeyCode::Backspace)],
        when: always,
        spec: ActionSpec::Builtin(|app, _| {
            if app.view_state.current_view == View::KindList {
                app.kind_list_escape();
            } else {
                app.navigate_back();
            }
            None
        }),
        footer: None,
        help: None,
        sets_status: false,
    },
];

/// The action with `id`.
pub(crate) fn action(id: ActionId) -> &'static Action {
    ACTIONS
        .iter()
        .find(|a| a.id == id)
        .unwrap_or(&ACTIONS[ACTIONS.len() - 1])
}

/// One footer hint.
#[derive(Debug, Clone, Copy)]
pub(crate) enum FooterItem {
    /// An action, with its default hint.
    Action(ActionId),
    /// An action under a label that fits the view ("Scroll" for `j/k` in a
    /// text view). Same key, same behaviour.
    Relabel(ActionId, &'static str),
    /// A `:` command worth surfacing (checked against the command table).
    Command(&'static str, &'static str),
}

use ActionId as A;
use FooterItem::{Action as Act, Command as Cmd, Relabel};

/// The Flux resource views' footer (the main list and the views opened
/// from it).
pub(crate) const FLUX_LAYOUT: &[FooterItem] = &[
    Act(A::Move),
    Act(A::Page),
    Act(A::CommandMode),
    Act(A::ListEnter),
    Act(A::FilterOrSearch),
    Act(A::Sort),
    Act(A::Suspend),
    Act(A::Resume),
    Act(A::Reconcile),
    Act(A::Yaml),
    Act(A::Describe),
    Act(A::Edit),
    Act(A::Favorite),
    Act(A::Graph),
    Act(A::History),
    Act(A::Values),
    Act(A::Trace),
    Act(A::Orphans),
    Act(A::ReconcileWithSource),
    Act(A::Delete),
    Act(A::Help),
    Act(A::Back),
];

/// Shared tail of the non-Flux views.
const TAIL: [FooterItem; 3] = [Act(A::CommandMode), Act(A::Help), Act(A::Back)];

const WORKLOAD_LIST_LAYOUT: &[FooterItem] = &[
    Act(A::Move),
    Act(A::Page),
    Act(A::WorkloadListEnter),
    Act(A::WorkloadListLogs),
    Act(A::Restart),
    TAIL[0],
    TAIL[1],
    TAIL[2],
];

const WORKLOAD_DETAIL_LAYOUT: &[FooterItem] = &[
    Relabel(A::Move, "Scroll"),
    Act(A::Page),
    Relabel(A::FilterOrSearch, "Search"),
    Act(A::WorkloadLogs),
    Act(A::Restart),
    Act(A::WorkloadDeletePod),
    Act(A::NativeTrace),
    Act(A::NativeGraph),
    Act(A::JumpToManager),
    TAIL[0],
    TAIL[1],
    TAIL[2],
];

const INVENTORY_LAYOUT: &[FooterItem] = &[
    Act(A::Move),
    Act(A::Page),
    Act(A::InventoryDescribe),
    Act(A::Yaml),
    Act(A::Edit),
    TAIL[0],
    TAIL[1],
    TAIL[2],
];

/// YAML / describe / trace of a native (non-Flux) object (#262): viewing
/// keys, and the way to its Flux manager once known (#266).
const NATIVE_OBJECT_LAYOUT: &[FooterItem] = &[
    Relabel(A::Move, "Scroll"),
    Act(A::Page),
    Relabel(A::FilterOrSearch, "Search"),
    Act(A::NextMatch),
    Act(A::Yaml),
    Act(A::Describe),
    Act(A::Edit),
    Act(A::NativeTrace),
    Act(A::NativeGraph),
    Act(A::JumpToManager),
    TAIL[0],
    TAIL[1],
    TAIL[2],
];

const LOGS_LAYOUT: &[FooterItem] = &[
    Relabel(A::Move, "Scroll"),
    Act(A::Page),
    Relabel(A::FilterOrSearch, "Search"),
    Act(A::NextMatch),
    Act(A::LogsFollow),
    Act(A::LogContainer),
    TAIL[0],
    TAIL[1],
    TAIL[2],
];

const EVENTS_LAYOUT: &[FooterItem] = &[
    Act(A::Move),
    Act(A::Page),
    Act(A::EventEnter),
    Relabel(A::FilterOrSearch, "Filter"),
    Act(A::Yaml),
    Act(A::Describe),
    Act(A::Edit),
    TAIL[0],
    TAIL[1],
    TAIL[2],
];

/// `:<kind>` lists (#267): the kind's capabilities decide which actions show.
const KIND_LIST_LAYOUT: &[FooterItem] = &[
    Act(A::Move),
    Act(A::Page),
    Act(A::KindEnter),
    Act(A::KindDescribe),
    Act(A::Describe),
    Act(A::Yaml),
    Act(A::Edit),
    Act(A::NativeTrace),
    Act(A::NativeGraph),
    Act(A::JumpToManager),
    Act(A::KindLogs),
    Act(A::Restart),
    Act(A::KindDeletePod),
    Relabel(A::FilterOrSearch, "Filter"),
    Cmd(":flux", "Flux view"),
    TAIL[0],
    TAIL[1],
    TAIL[2],
];

/// The unmanaged / orphaned sweep (#274): open a row, or go to the Flux
/// object an orphan names.
const OWNERSHIP_LAYOUT: &[FooterItem] = &[
    Act(A::Move),
    Act(A::Page),
    Act(A::InventoryDescribe),
    Act(A::Yaml),
    Act(A::Edit),
    Act(A::NativeTrace),
    Act(A::JumpToManager),
    Relabel(A::FilterOrSearch, "Filter"),
    Act(A::Rescan),
    TAIL[0],
    TAIL[1],
    TAIL[2],
];

const HELM_VALUES_LAYOUT: &[FooterItem] = &[
    Relabel(A::Move, "Scroll"),
    Act(A::Page),
    Relabel(A::FilterOrSearch, "Search"),
    Act(A::NextMatch),
    Act(A::RevealSecrets),
    Act(A::Reconcile),
    TAIL[0],
    TAIL[1],
    TAIL[2],
];

const PULSE_LAYOUT: &[FooterItem] = &[
    Relabel(A::Move, "Scroll"),
    Act(A::Page),
    Relabel(A::FilterOrSearch, "Search"),
    TAIL[0],
    TAIL[1],
    TAIL[2],
];

pub(crate) const CONNECTION_ERROR_LAYOUT: &[FooterItem] =
    &[Act(A::CommandMode), Act(A::Help), Relabel(A::Back, "Quit")];

/// The footer layout for a view (before filtering by what applies).
pub(crate) fn view_layout(view: View) -> &'static [FooterItem] {
    match view {
        View::WorkloadList => WORKLOAD_LIST_LAYOUT,
        View::WorkloadDetail => WORKLOAD_DETAIL_LAYOUT,
        View::InventoryList => INVENTORY_LAYOUT,
        View::Logs => LOGS_LAYOUT,
        View::EventList => EVENTS_LAYOUT,
        View::Pulse => PULSE_LAYOUT,
        View::HelmValues => HELM_VALUES_LAYOUT,
        View::KindList => KIND_LIST_LAYOUT,
        View::OwnershipList => OWNERSHIP_LAYOUT,
        View::ResourceList
        | View::ResourceDetail
        | View::ResourceDescribe
        | View::ResourceYAML
        | View::ResourceTrace
        | View::ResourceGraph
        | View::ResourceFavorites
        | View::ResourceHistory
        | View::ResourceEdit
        | View::Help => FLUX_LAYOUT,
    }
}

/// The hint an item shows.
fn hint(item: &FooterItem) -> NavigationCommand {
    match *item {
        Act(id) => {
            let (key, label) = action(id).footer.unwrap_or(("", ""));
            NavigationCommand::new(key, label)
        }
        Relabel(id, label) => {
            NavigationCommand::new(action(id).footer.unwrap_or(("", "")).0, label)
        }
        Cmd(key, label) => NavigationCommand::new(key, label),
    }
}

/// Every hint of a layout, unfiltered (for contexts where all apply).
pub(crate) fn layout_hints(layout: &[FooterItem]) -> Vec<NavigationCommand> {
    layout.iter().map(hint).collect()
}

/// Help lines for one column, in table order.
pub(crate) fn help_lines(column: HelpColumn) -> Vec<(&'static str, &'static str)> {
    ACTIONS
        .iter()
        .filter_map(|a| a.help)
        .filter(|(c, _, _)| *c == column)
        .map(|(_, key, text)| (key, text))
        .collect()
}

impl App {
    /// The footer layout for the current state.
    pub(crate) fn footer_layout(&self) -> &'static [FooterItem] {
        if self.has_connection_error() {
            return CONNECTION_ERROR_LAYOUT;
        }
        let view = self.view_state.current_view;
        if self.selection_state.native_object.is_some()
            && matches!(
                view,
                View::ResourceYAML | View::ResourceDescribe | View::ResourceTrace
            )
        {
            return NATIVE_OBJECT_LAYOUT;
        }
        view_layout(view)
    }

    /// Footer hints for the current state: the layout, minus actions that
    /// don't apply here — so the footer never advertises a dead key.
    pub(crate) fn footer_commands(&self) -> Vec<NavigationCommand> {
        if self.has_connection_error() {
            // Only `:`, `?`, and quit work before connecting.
            return layout_hints(CONNECTION_ERROR_LAYOUT);
        }
        self.footer_layout()
            .iter()
            .filter(|item| match item {
                Act(id) | Relabel(id, _) => (action(*id).when)(self),
                Cmd(..) => true,
            })
            .map(hint)
            .collect()
    }

    /// The action `key` runs here, if any.
    pub(crate) fn action_for(&self, key: &KeyEvent) -> Option<&'static Action> {
        ACTIONS
            .iter()
            .find(|a| a.keys.iter().any(|k| k.matches(key)) && (a.when)(self))
    }

    /// Run the action bound to `key`. `None` when no action handles it;
    /// otherwise the quit signal (`Some(true)` quits).
    pub(crate) fn dispatch_action(&mut self, key: KeyEvent) -> Option<Option<bool>> {
        let action = self.action_for(&key)?;
        Some(match action.spec {
            ActionSpec::Builtin(run) => run(self, key),
            ActionSpec::Operation(op_key) => {
                self.handle_operation_key(op_key);
                None
            }
            ActionSpec::Patch { .. } | ActionSpec::Annotate { .. } => {
                self.request_write(&action.spec);
                None
            }
        })
    }
}
