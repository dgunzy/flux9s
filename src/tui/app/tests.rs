//! App-level behaviour tests driven through `handle_key` / `execute_command`.

use super::core::App;
use super::events::native_edit_message;
use super::state::{Confirmation, PendingOperation, View};
use crate::config::{Config, UiConfig};
use crate::kube::objects::ObjectRef;
use crate::tui::Theme;
use crate::tui::app::state::{HealthFilter, NavEntry};
use crate::watcher::ResourceKey;
use crate::watcher::{ResourceInfo, ResourceState, resource_key};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use std::collections::HashMap;

fn make_key(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn make_ctrl_key(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::CONTROL,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn create_test_app(read_only: bool) -> App {
    let state = ResourceState::new();
    let config = Config {
        read_only,
        default_namespace: "".to_string(),
        default_controller_namespace: "".to_string(),
        discover_flux_resources: false,
        native_resources: true,
        metrics_source: crate::kube::metrics::MetricsSourceSetting::Auto,
        namespace_hotkeys: vec![],
        ui: UiConfig {
            enable_mouse: false,
            headless: false,
            no_icons: false,
            skin: "default".to_string(),
            skin_read_only: None,
            splashless: true,
            rbac_warnings: true,
        },
        context_skins: HashMap::new(),
        cluster: HashMap::new(),
        favorites: vec![],
        default_resource_filter: None,
        connect_timeout_seconds: crate::kube::health::DEFAULT_CONNECT_TIMEOUT_SECS,
        editor: None,
    };
    let theme = Theme::default();
    App::new(state, "test-context".to_string(), None, config, theme)
}

fn add_resource(app: &mut App) {
    let resource = ResourceInfo {
        name: "my-kustomization".to_string(),
        namespace: "flux-system".to_string(),
        resource_type: "Kustomization".to_string(),
        age: None,
        suspended: Some(false),
        ready: Some(true),
        message: Some("Applied revision".to_string()),
        revision: Some("main@sha1:abc123".to_string()),
        labels: HashMap::new(),
        annotations: HashMap::new(),
        last_reconciled: None,
        reconciliation_history: vec![],
    };
    app.state.upsert(
        resource_key(&resource.namespace, &resource.name, &resource.resource_type),
        resource,
    );
}

fn add_helm_release(app: &mut App) {
    let resource = ResourceInfo {
        name: "podinfo".to_string(),
        namespace: "apps".to_string(),
        resource_type: "HelmRelease".to_string(),
        age: None,
        suspended: Some(false),
        ready: Some(true),
        message: None,
        revision: None,
        labels: HashMap::new(),
        annotations: HashMap::new(),
        last_reconciled: None,
        reconciliation_history: vec![],
    };
    app.state.upsert(
        resource_key(&resource.namespace, &resource.name, &resource.resource_type),
        resource,
    );
}

#[test]
fn v_opens_live_values_for_helm_release_and_x_toggles_secrets() {
    let mut app = create_test_app(false);
    add_helm_release(&mut app);
    app.view_state.current_view = View::ResourceList;

    app.handle_key(make_key(KeyCode::Char('v')));

    assert_eq!(app.view_state.current_view, View::HelmValues);
    let request = app.async_state.helm_values.pending().cloned().unwrap();
    assert_eq!(request.key.to_key_string(), "HelmRelease:apps:podinfo");
    assert!(!request.reveal_secrets);
    // Flux operations still target the release from the values view.
    assert_eq!(
        app.view_target().map(|rk| rk.to_key_string()),
        Some("HelmRelease:apps:podinfo".to_string())
    );

    app.handle_key(make_key(KeyCode::Char('x')));
    assert!(
        app.async_state
            .helm_values
            .pending()
            .is_some_and(|r| r.reveal_secrets)
    );

    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(app.view_state.current_view, View::ResourceList);
    assert!(!app.async_state.helm_values.is_loading(), "watch stopped");
}

#[test]
fn v_on_non_helm_release_explains() {
    let mut app = create_test_app(false);
    add_resource(&mut app); // a Kustomization
    app.view_state.current_view = View::ResourceList;

    app.handle_key(make_key(KeyCode::Char('v')));

    assert_eq!(app.view_state.current_view, View::ResourceList);
    assert!(
        app.ui_state
            .status_message
            .as_ref()
            .is_some_and(|(msg, err)| *err && msg.contains("only available for HelmReleases"))
    );
}

#[test]
fn test_d_opens_describe_view() {
    let mut app = create_test_app(false);
    add_resource(&mut app);
    app.view_state.current_view = View::ResourceList;

    let result = app.handle_key(make_key(KeyCode::Char('d')));

    assert_eq!(result, None);
    assert_eq!(app.view_state.current_view, View::ResourceDescribe);
    assert_eq!(
        app.async_state
            .describe
            .pending()
            .map(|t| t.to_resource_key().to_key_string())
            .as_deref(),
        Some("Kustomization:flux-system:my-kustomization")
    );
    assert!(app.async_state.confirmation_pending.is_none());
}

#[test]
fn test_ctrl_d_still_requires_delete_confirmation() {
    let mut app = create_test_app(false);
    add_resource(&mut app);
    app.view_state.current_view = View::ResourceList;

    let result = app.handle_key(make_ctrl_key(KeyCode::Char('d')));

    assert_eq!(result, None);
    assert!(app.async_state.confirmation_pending.is_some());
    assert_eq!(app.view_state.current_view, View::ResourceList);
    assert!(app.async_state.operation.pending().is_none());
}

#[test]
fn test_ctrl_d_is_blocked_in_readonly_mode() {
    let mut app = create_test_app(true);
    add_resource(&mut app);
    app.view_state.current_view = View::ResourceList;

    let result = app.handle_key(make_ctrl_key(KeyCode::Char('d')));

    assert_eq!(result, None);
    assert!(app.async_state.confirmation_pending.is_none());
    assert_eq!(
        app.ui_state.status_message,
        Some((
            crate::constants::READ_ONLY_WRITE_ACTION_MESSAGE.to_string(),
            true,
        ))
    );
}

#[test]
fn test_delete_confirmation_still_blocks_execution_in_readonly_mode() {
    let mut app = create_test_app(true);
    app.async_state.confirmation_pending = Some(Confirmation::Flux(PendingOperation::new(
        "Kustomization".to_string(),
        "flux-system".to_string(),
        "my-kustomization".to_string(),
        'd',
    )));

    let result = app.handle_key(make_key(KeyCode::Char('y')));

    assert_eq!(result, None);
    assert!(app.async_state.confirmation_pending.is_none());
    assert!(app.async_state.operation.pending().is_none());
    assert_eq!(
        app.ui_state.status_message,
        Some((
            crate::constants::READ_ONLY_WRITE_ACTION_MESSAGE.to_string(),
            true,
        ))
    );
}

#[test]
fn test_ctx_command_sets_pending_context_switch() {
    let mut app = create_test_app(false);
    app.ui_state.command_buffer = "ctx new-context-xyz".to_string();

    let result = app.execute_command();

    assert_eq!(result, None);
    assert_eq!(
        app.pending_context_switch,
        Some("new-context-xyz".to_string())
    );
    assert_eq!(
        app.ui_state.status_message,
        Some((
            "Switching to context 'new-context-xyz'...".to_string(),
            false
        ))
    );
}

#[test]
fn table_command_healthy_sets_filter() {
    let mut app = create_test_app(false);
    app.ui_state.command_buffer = "healthy".to_string();

    assert_eq!(app.execute_command(), None);
    assert_eq!(app.view_state.health_filter, HealthFilter::Healthy);
}

#[test]
fn table_command_suspended_filters_to_suspended_resources() {
    let mut app = create_test_app(false);
    add_resource(&mut app);
    let suspended = ResourceInfo {
        name: "paused".to_string(),
        namespace: "flux-system".to_string(),
        resource_type: "Kustomization".to_string(),
        age: None,
        suspended: Some(true),
        ready: Some(true),
        message: None,
        revision: None,
        labels: HashMap::new(),
        annotations: HashMap::new(),
        last_reconciled: None,
        reconciliation_history: vec![],
    };
    app.state.upsert(
        resource_key(
            &suspended.namespace,
            &suspended.name,
            &suspended.resource_type,
        ),
        suspended,
    );
    app.ui_state.command_buffer = "suspended".to_string();

    assert_eq!(app.execute_command(), None);
    assert_eq!(app.view_state.health_filter, HealthFilter::Suspended);
    let names: Vec<String> = app
        .get_filtered_resources()
        .into_iter()
        .map(|r| r.name)
        .collect();
    assert_eq!(names, vec!["paused".to_string()]);

    app.ui_state.command_buffer = "all".to_string();
    assert_eq!(app.execute_command(), None);
    assert_eq!(app.view_state.health_filter, HealthFilter::All);
    assert_eq!(app.get_filtered_resources().len(), 2);
}

#[test]
fn table_command_all_clears_filters() {
    let mut app = create_test_app(false);
    app.view_state.selected_resource_type = Some("Kustomization".to_string());
    app.view_state.health_filter = HealthFilter::Unhealthy;
    app.ui_state.command_buffer = "all".to_string();

    assert_eq!(app.execute_command(), None);
    assert_eq!(app.view_state.selected_resource_type, None);
    assert_eq!(app.view_state.health_filter, HealthFilter::All);
}

#[test]
fn table_command_favorites_switches_view() {
    let mut app = create_test_app(false);
    app.ui_state.command_buffer = "favorites".to_string();

    assert_eq!(app.execute_command(), None);
    assert_eq!(app.view_state.current_view, View::ResourceFavorites);
}

#[test]
fn quit_command_returns_true() {
    let mut app = create_test_app(false);
    app.ui_state.command_buffer = "q".to_string();
    assert_eq!(app.execute_command(), Some(true));
}

#[test]
fn unknown_command_reports_error() {
    let mut app = create_test_app(false);
    app.ui_state.command_buffer = "definitely-not-a-command".to_string();

    assert_eq!(app.execute_command(), None);
    let (msg, is_error) = app.ui_state.status_message.clone().unwrap();
    assert!(is_error);
    assert!(msg.contains("Unknown command"));
}

#[test]
fn readonly_command_toggles_mode() {
    let mut app = create_test_app(false);
    assert!(!app.config.read_only);

    app.ui_state.command_buffer = "readonly".to_string();
    assert_eq!(app.execute_command(), None);
    assert!(app.config.read_only);

    app.ui_state.command_buffer = "readonly".to_string();
    assert_eq!(app.execute_command(), None);
    assert!(!app.config.read_only);
}

#[test]
fn test_sort_keys_in_list_view() {
    use crate::tui::app::state::SortField;
    let mut app = create_test_app(false);
    assert_eq!(app.view_state.current_view, View::ResourceList);

    app.handle_key(make_key(KeyCode::Char('N')));
    assert_eq!(app.view_state.sort_field, SortField::Name);
    assert!(!app.view_state.sort_reverse);

    app.handle_key(make_key(KeyCode::Char('N')));
    assert!(app.view_state.sort_reverse);

    app.handle_key(make_key(KeyCode::Char('A')));
    assert_eq!(app.view_state.sort_field, SortField::Age);
    assert!(!app.view_state.sort_reverse);

    app.handle_key(make_key(KeyCode::Char('S')));
    assert_eq!(app.view_state.sort_field, SortField::Status);
    app.handle_key(make_key(KeyCode::Char('T')));
    assert_eq!(app.view_state.sort_field, SortField::Type);
}

#[test]
fn test_sort_keys_ignored_outside_list_views() {
    use crate::tui::app::state::SortField;
    let mut app = create_test_app(false);
    app.view_state.current_view = View::ResourceYAML;

    app.handle_key(make_key(KeyCode::Char('A')));
    assert_eq!(app.view_state.sort_field, SortField::Default);
}

#[test]
fn test_text_search_input_flow_in_yaml_view() {
    let mut app = create_test_app(false);
    app.view_state.current_view = View::ResourceYAML;

    // '/' opens search input
    app.handle_key(make_key(KeyCode::Char('/')));
    assert!(app.view_state.text_search.input_mode);
    // Filter mode must NOT be entered in text views
    assert!(!app.view_state.filter_mode);

    // Type a query and apply it
    app.handle_key(make_key(KeyCode::Char('a')));
    app.handle_key(make_key(KeyCode::Char('b')));
    app.handle_key(make_key(KeyCode::Backspace));
    app.handle_key(make_key(KeyCode::Enter));
    assert!(!app.view_state.text_search.input_mode);
    assert_eq!(app.view_state.text_search.query, "a");
    assert!(app.view_state.text_search.pending_jump);

    // Esc clears the active search before navigating back
    app.handle_key(make_key(KeyCode::Esc));
    assert!(app.view_state.text_search.query.is_empty());
    assert_eq!(app.view_state.current_view, View::ResourceYAML);

    // Second Esc navigates back to the list
    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(app.view_state.current_view, View::ResourceList);
}

#[test]
fn test_text_search_next_prev_match_keys() {
    let mut app = create_test_app(false);
    app.view_state.current_view = View::ResourceDescribe;
    app.view_state.text_search.query = "spec".to_string();
    app.view_state.text_search.total_matches = 3;
    app.view_state.text_search.current_match = 0;

    app.handle_key(make_key(KeyCode::Char('n')));
    assert_eq!(app.view_state.text_search.current_match, 1);
    assert!(app.view_state.text_search.pending_jump);

    // 'N' wraps backwards from 1 -> 0 -> 2
    app.handle_key(make_key(KeyCode::Char('N')));
    app.handle_key(make_key(KeyCode::Char('N')));
    assert_eq!(app.view_state.text_search.current_match, 2);
}

#[test]
fn test_slash_in_list_view_still_enters_filter_mode() {
    let mut app = create_test_app(false);
    assert_eq!(app.view_state.current_view, View::ResourceList);

    app.handle_key(make_key(KeyCode::Char('/')));
    assert!(app.view_state.filter_mode);
    assert!(!app.view_state.text_search.input_mode);
}

// --- Graph view focus navigation -------------------------------------

fn graph_node(
    kind: &str,
    name: &str,
    node_type: crate::trace::NodeType,
) -> crate::trace::GraphNode {
    crate::trace::GraphNode {
        id: format!("{}:flux-system:{}", kind, name),
        kind: kind.to_string(),
        name: name.to_string(),
        namespace: "flux-system".to_string(),
        node_type,
        ready: None,
        position: None,
        description: None,
    }
}

/// Build a small graph: a source (watched), the object (watched), and a
/// workload group (aggregate). Returns the app already on the graph view.
fn app_on_graph() -> App {
    use crate::trace::{NodeType, ResourceGraph};

    let mut app = create_test_app(false);
    add_resource(&mut app); // Kustomization:flux-system:my-kustomization (watched)

    let mut graph = ResourceGraph::new();
    // idx 0: object (layer 3) — matches the watched resource
    graph.add_node(graph_node(
        "Kustomization",
        "my-kustomization",
        NodeType::Object,
    ));
    // idx 1: source (layer 1) — not in the watched state
    graph.add_node(graph_node("GitRepository", "my-repo", NodeType::Source));
    // idx 2: workload group (layer 5) — aggregate, not navigable
    graph.add_node(graph_node(
        "Workloads",
        "Workloads (1)",
        NodeType::WorkloadGroup,
    ));

    app.set_graph_result(graph);
    app.view_state.current_view = View::ResourceGraph;
    app
}

#[test]
fn graph_focus_starts_on_object_node() {
    let app = app_on_graph();
    // object_node_index() == 0 in app_on_graph's graph
    assert_eq!(app.view_state.graph_focus_index, Some(0));
}

#[test]
fn graph_j_k_move_focus_in_visual_order() {
    let mut app = app_on_graph();
    // Visual order by layer: source(1), object(0), workload group(2).
    // Focus starts on the object (pos 1 in that order).
    app.handle_key(make_key(KeyCode::Char('j'))); // down -> workload group
    assert_eq!(app.view_state.graph_focus_index, Some(2));

    app.handle_key(make_key(KeyCode::Char('j'))); // clamp at the bottom
    assert_eq!(app.view_state.graph_focus_index, Some(2));

    app.handle_key(make_key(KeyCode::Char('k'))); // up -> object
    assert_eq!(app.view_state.graph_focus_index, Some(0));
    app.handle_key(make_key(KeyCode::Char('k'))); // up -> source
    assert_eq!(app.view_state.graph_focus_index, Some(1));
    app.handle_key(make_key(KeyCode::Char('k'))); // clamp at the top
    assert_eq!(app.view_state.graph_focus_index, Some(1));
}

#[test]
fn graph_enter_navigates_into_watched_node_and_esc_returns_to_graph() {
    let mut app = app_on_graph();
    // Focus is on the object node, which is in the watched state.
    app.handle_key(make_key(KeyCode::Enter));

    assert_eq!(app.view_state.current_view, View::ResourceDetail);
    assert_eq!(
        app.selection_state.selected_resource_key.as_deref(),
        Some("Kustomization:flux-system:my-kustomization")
    );
    assert_eq!(app.back_target(), Some(View::ResourceGraph));

    // Esc from the detail view returns to the graph, not the list.
    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(app.view_state.current_view, View::ResourceGraph);
    // Focus is preserved so the user lands back where they were.
    assert_eq!(app.view_state.graph_focus_index, Some(0));
}

#[test]
fn graph_enter_on_unwatched_node_shows_message_and_stays() {
    let mut app = app_on_graph();
    app.view_state.graph_focus_index = Some(1); // the source, not in state

    app.handle_key(make_key(KeyCode::Enter));

    assert_eq!(app.view_state.current_view, View::ResourceGraph);
    assert!(app.ui_state.status_message.is_some());
}

#[test]
fn graph_enter_on_aggregate_node_shows_message_and_stays() {
    let mut app = app_on_graph();
    app.view_state.graph_focus_index = Some(2); // the workload group aggregate

    app.handle_key(make_key(KeyCode::Enter));

    assert_eq!(app.view_state.current_view, View::ResourceGraph);
    assert!(app.ui_state.status_message.is_some());
}

#[test]
fn list_enter_returns_to_the_list_after_a_graph_drill_down() {
    let mut app = app_on_graph();
    // Drill in from the graph and back out to the list.
    app.handle_key(make_key(KeyCode::Enter));
    app.handle_key(make_key(KeyCode::Esc));
    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(app.view_state.current_view, View::ResourceList);

    // Now open a resource the normal way: Esc returns to the list.
    app.view_state.selected_index = 0;
    app.handle_key(make_key(KeyCode::Enter));
    assert_eq!(app.view_state.current_view, View::ResourceDetail);
    assert_eq!(app.back_target(), Some(View::ResourceList));
    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(app.view_state.current_view, View::ResourceList);
}

fn add_kube_event(app: &mut App, uid: &str, kind: &str, name: &str) {
    let info = crate::kube::events::KubeEventInfo::from_json(&serde_json::json!({
        "metadata": {"uid": uid, "namespace": "flux-system"},
        "involvedObject": {
            "kind": kind,
            "namespace": "flux-system",
            "name": name
        },
        "type": "Normal",
        "reason": "Test",
        "message": "test",
        "lastTimestamp": "2026-07-01T12:30:00Z"
    }))
    .unwrap();
    app.kube_events.upsert(info);
}

#[test]
fn events_command_opens_event_list_view() {
    let mut app = create_test_app(false);
    app.ui_state.command_buffer = "events".to_string();

    let result = app.execute_command();

    assert_eq!(result, None);
    assert_eq!(app.view_state.current_view, View::EventList);
}

#[test]
fn esc_from_event_list_returns_to_resource_list_and_clears_feed() {
    let mut app = create_test_app(false);
    add_kube_event(&mut app, "uid-1", "Kustomization", "my-kustomization");
    app.view_state.current_view = View::EventList;

    let result = app.handle_key(make_key(KeyCode::Esc));

    assert_eq!(result, None);
    assert_eq!(app.view_state.current_view, View::ResourceList);
    assert!(
        app.kube_events.is_empty(),
        "leaving the events view drops the collected feed"
    );
}

#[test]
fn enter_on_event_opens_watched_resource_detail() {
    let mut app = create_test_app(false);
    add_resource(&mut app);
    add_kube_event(&mut app, "uid-1", "Kustomization", "my-kustomization");
    app.view_state.current_view = View::EventList;
    app.view_state.selected_index = 0;

    app.handle_key(make_key(KeyCode::Enter));

    assert_eq!(app.view_state.current_view, View::ResourceDetail);
    assert_eq!(
        app.selection_state.selected_resource_key.as_deref(),
        Some("Kustomization:flux-system:my-kustomization")
    );
    // Back returns to the events feed, not the resource list
    assert_eq!(app.back_target(), Some(View::EventList));
    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(app.view_state.current_view, View::EventList);
}

#[test]
fn enter_on_event_for_unwatched_object_stays_in_event_list() {
    let mut app = create_test_app(false);
    add_kube_event(&mut app, "uid-1", "Pod", "some-pod");
    app.view_state.current_view = View::EventList;
    app.view_state.selected_index = 0;

    app.handle_key(make_key(KeyCode::Enter));

    assert_eq!(app.view_state.current_view, View::EventList);
    assert!(
        app.ui_state
            .status_message
            .as_ref()
            .is_some_and(|(msg, _)| msg.contains("Pod/some-pod")
                && msg.contains("ns: flux-system")
                && msg.contains("y/d")),
        "names the object + namespace and points at y/d"
    );
}

#[test]
fn all_command_returns_from_event_list_and_clears_filters() {
    let mut app = create_test_app(false);
    add_kube_event(&mut app, "uid-1", "Kustomization", "my-kustomization");
    app.view_state.current_view = View::EventList;
    app.view_state.selected_resource_type = Some("HelmChart".to_string());
    app.view_state.health_filter = HealthFilter::Unhealthy;

    app.ui_state.command_buffer = "all".to_string();
    let result = app.execute_command();

    assert_eq!(result, None);
    assert_eq!(app.view_state.current_view, View::ResourceList);
    assert_eq!(app.view_state.selected_resource_type, None);
    assert_eq!(app.view_state.health_filter, HealthFilter::All);
    assert!(
        app.kube_events.is_empty(),
        "leaving the events view via :all drops the feed"
    );
}

#[test]
fn y_and_d_work_from_event_list_on_involved_object() {
    let mut app = create_test_app(false);
    add_kube_event(&mut app, "uid-1", "Kustomization", "my-kustomization");
    app.view_state.current_view = View::EventList;
    app.view_state.selected_index = 0;

    app.handle_key(make_key(KeyCode::Char('y')));

    assert_eq!(app.view_state.current_view, View::ResourceYAML);
    assert_eq!(
        app.async_state
            .yaml
            .pending()
            .map(|t| t.to_resource_key().to_key_string())
            .as_deref(),
        Some("Kustomization:flux-system:my-kustomization")
    );
    // Back returns to the events feed, not the resource list
    assert_eq!(app.back_target(), Some(View::EventList));
    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(app.view_state.current_view, View::EventList);

    app.handle_key(make_key(KeyCode::Char('d')));
    assert_eq!(app.view_state.current_view, View::ResourceDescribe);
    assert_eq!(
        app.async_state
            .describe
            .pending()
            .map(|t| t.to_resource_key().to_key_string())
            .as_deref(),
        Some("Kustomization:flux-system:my-kustomization")
    );
}

#[test]
fn view_target_resolves_current_resource_from_every_view() {
    let mut app = create_test_app(false);
    add_resource(&mut app);
    add_kube_event(&mut app, "uid-1", "Kustomization", "my-kustomization");

    // Events feed: the selected event's involved object — including for
    // operations/trace/graph/history, which resolve via get_current_resource
    app.view_state.current_view = View::EventList;
    app.view_state.selected_index = 0;
    assert_eq!(
        app.view_target().map(|rk| rk.to_key_string()).as_deref(),
        Some("Kustomization:flux-system:my-kustomization")
    );
    assert!(
        app.get_current_resource()
            .is_some_and(|r| r.name == "my-kustomization"),
        "watched involved objects resolve for operations from the events feed"
    );

    // Resource list: the selected row
    app.view_state.current_view = View::ResourceList;
    assert_eq!(
        app.view_target().map(|rk| rk.to_key_string()).as_deref(),
        Some("Kustomization:flux-system:my-kustomization")
    );

    // Nested views: the stored selection
    app.selection_state.selected_resource_key =
        Some("Kustomization:flux-system:my-kustomization".to_string());
    for view in [
        View::ResourceDetail,
        View::ResourceDescribe,
        View::ResourceYAML,
        View::ResourceTrace,
        View::ResourceHistory,
    ] {
        app.view_state.current_view = view;
        assert!(app.view_target().is_some(), "{view:?} should resolve");
    }
}

#[test]
fn yaml_from_focused_graph_node_returns_to_graph_on_esc() {
    let mut app = app_on_graph();
    // Focus starts on the object node (a real resource)
    app.handle_key(make_key(KeyCode::Char('y')));

    assert_eq!(app.view_state.current_view, View::ResourceYAML);
    assert_eq!(
        app.async_state
            .yaml
            .pending()
            .map(|t| t.to_resource_key().to_key_string())
            .as_deref(),
        Some("Kustomization:flux-system:my-kustomization")
    );

    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(
        app.view_state.current_view,
        View::ResourceGraph,
        "Back from a view opened off a graph node returns to the graph"
    );
}

#[test]
fn yaml_on_aggregate_graph_node_does_nothing() {
    let mut app = app_on_graph();
    // Move focus to the workload group (aggregate) node
    app.view_state.graph_focus_index = Some(2);

    app.handle_key(make_key(KeyCode::Char('y')));

    assert_eq!(app.view_state.current_view, View::ResourceGraph);
    assert!(app.async_state.yaml.pending().is_none());
}

#[test]
fn event_list_filter_narrows_feed() {
    let mut app = create_test_app(false);
    add_kube_event(&mut app, "uid-1", "Kustomization", "podinfo");
    add_kube_event(&mut app, "uid-2", "HelmRelease", "cert-manager");
    app.view_state.current_view = View::EventList;

    app.view_state.filter = "helmrelease".to_string();
    let filtered = app.filtered_kube_events();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].involved_name, "cert-manager");

    app.view_state.filter.clear();
    assert_eq!(app.filtered_kube_events().len(), 2);
}

fn add_controller_pod(app: &mut App, name: &str) {
    app.controller_pods.upsert_pod(
        name.to_string(),
        crate::tui::app::state::ControllerPodInfo {
            name: name.to_string(),
            ready: true,
            version: Some("v1.0.0".to_string()),
        },
    );
}

#[test]
fn logs_command_matches_pod_by_exact_name_and_prefix() {
    let mut app = create_test_app(false);
    add_controller_pod(&mut app, "source-controller-abc123");
    add_controller_pod(&mut app, "source-watcher-def456");

    // Unique prefix opens the log view
    app.ui_state.command_buffer = "logs source-c".to_string();
    app.execute_command();
    assert_eq!(app.view_state.current_view, View::Logs);
    assert!(app.logs.follow, "streams start in follow mode");

    // Back stops the session and returns to the list
    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(app.view_state.current_view, View::ResourceList);
    assert!(app.logs.session.is_none(), "Esc stops the stream");

    // Ambiguous prefix ("source" matches both pods) does not open a stream
    app.ui_state.command_buffer = "logs source".to_string();
    app.execute_command();
    assert_eq!(app.view_state.current_view, View::ResourceList);

    // No match reports an error
    app.ui_state.command_buffer = "logs nonexistent".to_string();
    app.execute_command();
    assert!(
        app.ui_state
            .status_message
            .as_ref()
            .is_some_and(|(msg, is_err)| *is_err && msg.contains("nonexistent"))
    );
}

#[test]
fn logs_command_without_arg_opens_pod_submenu() {
    let mut app = create_test_app(false);

    // No pods discovered yet: error message, no submenu
    app.ui_state.command_buffer = "logs".to_string();
    app.execute_command();
    assert!(app.view_state.submenu_state.is_none());
    assert!(app.ui_state.status_message.is_some());

    add_controller_pod(&mut app, "helm-controller-xyz");
    app.ui_state.command_buffer = "logs".to_string();
    app.execute_command();
    let submenu = app
        .view_state
        .submenu_state
        .as_ref()
        .expect("submenu opens");
    assert_eq!(submenu.command, "logs");
    assert_eq!(submenu.items.len(), 1);

    // Selecting a pod opens the log view
    app.handle_key(make_key(KeyCode::Enter));
    assert_eq!(app.view_state.current_view, View::Logs);
    assert!(app.view_state.submenu_state.is_none());
}

#[test]
fn log_view_scrolling_pauses_follow_and_g_resumes() {
    let mut app = create_test_app(false);
    add_controller_pod(&mut app, "source-controller-abc");
    app.ui_state.command_buffer = "logs source-controller-abc".to_string();
    app.execute_command();
    assert!(app.logs.follow);

    app.handle_key(make_key(KeyCode::Char('k')));
    assert!(!app.logs.follow, "scrolling up pauses following");

    app.handle_key(make_key(KeyCode::Char('G')));
    assert!(app.logs.follow, "G resumes following");
}

#[test]
fn logs_from_event_list_returns_to_event_list() {
    let mut app = create_test_app(false);
    add_controller_pod(&mut app, "source-controller-abc");
    app.view_state.current_view = View::EventList;

    app.ui_state.command_buffer = "logs source-controller-abc".to_string();
    app.execute_command();
    assert_eq!(app.view_state.current_view, View::Logs);

    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(
        app.view_state.current_view,
        View::EventList,
        "Back returns to the events feed logs were opened from"
    );
}

/// Build an app on a graph whose workload group carries two encoded
/// workload rows (the #194 drill-down entry point).
fn app_on_graph_with_workloads() -> App {
    use crate::kube::workloads::WorkloadRef;
    use crate::trace::{NodeType, ResourceGraph};

    let mut app = create_test_app(false);
    add_resource(&mut app);

    let lines = [
        WorkloadRef {
            kind: "Deployment".to_string(),
            name: "podinfo".to_string(),
            namespace: "flux-system".to_string(),
            indicator: "\u{25cf}".to_string(),
            status: "Replicas: 1/1".to_string(),
            api_version: "apps/v1".to_string(),
        },
        WorkloadRef {
            kind: "StatefulSet".to_string(),
            name: "redis".to_string(),
            namespace: "flux-system".to_string(),
            indicator: "\u{25cb}".to_string(),
            status: "Replicas: 0/1".to_string(),
            api_version: "apps/v1".to_string(),
        },
    ]
    .iter()
    .map(WorkloadRef::to_graph_line)
    .collect::<Vec<_>>()
    .join("\n");

    let mut graph = ResourceGraph::new();
    graph.add_node(graph_node(
        "Kustomization",
        "my-kustomization",
        NodeType::Object,
    ));
    let mut group = graph_node("Workloads", "Workloads (2)", NodeType::WorkloadGroup);
    group.description = Some(lines);
    graph.add_node(group);
    app.set_graph_result(graph);
    app.view_state.current_view = View::ResourceGraph;
    app.view_state.graph_focus_index = Some(1);
    app
}

#[test]
fn workload_group_enter_opens_workload_list() {
    let mut app = app_on_graph_with_workloads();

    app.handle_key(make_key(KeyCode::Enter));

    assert_eq!(app.view_state.current_view, View::WorkloadList);
    assert_eq!(app.view_state.workload_rows.len(), 2);
    assert_eq!(app.view_state.workload_rows[0].name, "podinfo");
    assert_eq!(app.view_state.selected_index, 0);

    // Back returns to the graph
    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(app.view_state.current_view, View::ResourceGraph);
}

/// Build an app on a graph whose resource group carries three encoded
/// inventory entries (the #245 drill-down entry point).
fn app_on_graph_with_resource_group() -> App {
    use crate::kube::inventory::InventoryEntry;
    use crate::trace::{NodeType, ResourceGraph};

    let mut app = create_test_app(false);
    add_resource(&mut app);

    let lines = [
        InventoryEntry {
            kind: "ConfigMap".to_string(),
            name: "podinfo-config".to_string(),
            namespace: "flux-system".to_string(),
            api_version: "v1".to_string(),
        },
        InventoryEntry {
            kind: "Namespace".to_string(),
            name: "podinfo".to_string(),
            namespace: String::new(),
            api_version: "v1".to_string(),
        },
        InventoryEntry {
            kind: "Service".to_string(),
            name: "podinfo".to_string(),
            namespace: "flux-system".to_string(),
            api_version: "v1".to_string(),
        },
    ]
    .iter()
    .map(InventoryEntry::to_graph_line)
    .collect::<Vec<_>>()
    .join("\n");

    let mut graph = ResourceGraph::new();
    graph.add_node(graph_node(
        "Kustomization",
        "my-kustomization",
        NodeType::Object,
    ));
    let mut group = graph_node("Resources", "Resources (3)", NodeType::ResourceGroup);
    group.description = Some(lines);
    graph.add_node(group);
    app.set_graph_result(graph);
    app.view_state.current_view = View::ResourceGraph;
    app.view_state.graph_focus_index = Some(1);
    app
}

#[test]
fn resource_group_enter_opens_inventory_list() {
    let mut app = app_on_graph_with_resource_group();

    app.handle_key(make_key(KeyCode::Enter));

    assert_eq!(app.view_state.current_view, View::InventoryList);
    assert_eq!(app.view_state.inventory_rows.len(), 3);
    // Each row carries the namespace and name the summary node hides.
    assert_eq!(app.view_state.inventory_rows[0].kind, "ConfigMap");
    assert_eq!(app.view_state.inventory_rows[0].namespace, "flux-system");
    assert_eq!(app.view_state.inventory_rows[0].name, "podinfo-config");
    // Cluster-scoped entries keep their empty namespace.
    assert_eq!(app.view_state.inventory_rows[1].namespace, "");
    assert_eq!(app.view_state.selected_index, 0);

    // j/k move the selection within the breakdown
    app.handle_key(make_key(KeyCode::Char('j')));
    assert_eq!(app.view_state.selected_index, 1);

    // Back returns to the graph
    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(app.view_state.current_view, View::ResourceGraph);
}

#[test]
fn opening_inventory_requests_object_statuses() {
    let mut app = app_on_graph_with_resource_group();
    app.handle_key(make_key(KeyCode::Enter));

    assert!(app.async_state.inventory_status.is_loading());
    assert_eq!(
        app.async_state.inventory_status.pending().map(Vec::len),
        Some(3)
    );

    // Leaving the inventory drops the status lookup.
    app.handle_key(make_key(KeyCode::Esc));
    assert!(!app.async_state.inventory_status.is_loading());
}

#[test]
fn inventory_enter_describes_native_object_and_back_returns() {
    let mut app = app_on_graph_with_resource_group();
    app.handle_key(make_key(KeyCode::Enter)); // graph → inventory
    app.handle_key(make_key(KeyCode::Char('j'))); // Namespace row

    app.handle_key(make_key(KeyCode::Enter));

    assert_eq!(app.view_state.current_view, View::ResourceDescribe);
    assert_eq!(
        app.async_state.describe.pending().cloned(),
        Some(ObjectRef::native("v1", "Namespace", "", "podinfo"))
    );
    assert!(app.selection_state.native_object.is_some());
    // Flux-only actions have no target on a native object.
    assert!(app.view_target().is_none());

    // y toggles to YAML for the same native object.
    app.handle_key(make_key(KeyCode::Char('y')));
    assert_eq!(app.view_state.current_view, View::ResourceYAML);
    assert_eq!(
        app.async_state
            .yaml
            .pending()
            .and_then(|t| t.api_version.clone()),
        Some("v1".to_string())
    );

    // Back returns to the inventory with the selection intact.
    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(app.view_state.current_view, View::InventoryList);
    assert_eq!(app.view_state.selected_index, 1);
    assert!(app.selection_state.native_object.is_none());
}

#[test]
fn event_for_unwatched_native_object_opens_describe() {
    let mut app = create_test_app(false);
    let event = crate::kube::events::KubeEventInfo::from_json(&serde_json::json!({
        "metadata": {"uid": "uid-native", "namespace": "apps"},
        "involvedObject": {
            "apiVersion": "apps/v1",
            "kind": "Deployment",
            "namespace": "apps",
            "name": "web"
        },
        "type": "Normal",
        "reason": "ScalingReplicaSet",
        "message": "Scaled up"
    }))
    .unwrap();
    app.kube_events.upsert(event);
    app.view_state.current_view = View::EventList;
    app.view_state.selected_index = 0;

    app.handle_key(make_key(KeyCode::Enter));

    assert_eq!(app.view_state.current_view, View::ResourceDescribe);
    assert_eq!(
        app.async_state.describe.pending().cloned(),
        Some(ObjectRef::native("apps/v1", "Deployment", "apps", "web"))
    );
    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(app.view_state.current_view, View::EventList);
}

#[test]
fn resource_group_without_rows_shows_hint() {
    use crate::trace::{NodeType, ResourceGraph};

    let mut app = create_test_app(false);
    let mut graph = ResourceGraph::new();
    graph.add_node(graph_node(
        "Kustomization",
        "my-kustomization",
        NodeType::Object,
    ));
    // An undescribed resource group has nothing to drill into.
    graph.add_node(graph_node(
        "Resources",
        "Resources (0)",
        NodeType::ResourceGroup,
    ));
    app.set_graph_result(graph);
    app.view_state.current_view = View::ResourceGraph;
    app.view_state.graph_focus_index = Some(1);

    app.handle_key(make_key(KeyCode::Enter));

    assert_eq!(app.view_state.current_view, View::ResourceGraph);
    assert!(app.ui_state.status_message.is_some());
}

#[test]
fn workload_group_without_rows_shows_hint() {
    let mut app = app_on_graph();
    app.view_state.graph_focus_index = Some(2); // undescribed workload group

    app.handle_key(make_key(KeyCode::Enter));

    assert_eq!(app.view_state.current_view, View::ResourceGraph);
    assert!(app.ui_state.status_message.is_some());
}

#[test]
fn workload_list_enter_requests_detail_and_back_chain_holds() {
    let mut app = app_on_graph_with_workloads();
    app.handle_key(make_key(KeyCode::Enter)); // -> WorkloadList
    app.handle_key(make_key(KeyCode::Char('j'))); // select redis
    app.handle_key(make_key(KeyCode::Enter)); // -> WorkloadDetail

    assert_eq!(app.view_state.current_view, View::WorkloadDetail);
    assert_eq!(
        app.async_state
            .workload
            .pending()
            .map(crate::watcher::ResourceKey::to_key_string)
            .as_deref(),
        Some("StatefulSet:flux-system:redis")
    );

    // Full back chain: detail -> list -> graph
    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(app.view_state.current_view, View::WorkloadList);
    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(app.view_state.current_view, View::ResourceGraph);
}

fn workload_data(pods: &[&str]) -> crate::kube::workloads::WorkloadData {
    crate::kube::workloads::WorkloadData {
        kind: "Deployment".to_string(),
        name: "podinfo".to_string(),
        namespace: "flux-system".to_string(),
        ready: Some(true),
        summary: Vec::new(),
        containers: Vec::new(),
        pods: pods
            .iter()
            .map(|name| crate::kube::workloads::PodRow {
                name: name.to_string(),
                phase: "Running".to_string(),
                ready: "1/1".to_string(),
                restarts: 0,
                age: None,
                resources: Default::default(),
            })
            .collect(),
        events: Vec::new(),
        events_error: None,
        pod_selector: None,
        managed_by: None,
    }
}

#[test]
fn workload_logs_single_pod_streams_and_returns_to_detail() {
    let mut app = app_on_graph_with_workloads();
    app.view_state.current_view = View::WorkloadDetail;
    app.async_state
        .workload
        .set_result(workload_data(&["podinfo-abc"]));

    app.handle_key(make_key(KeyCode::Char('l')));

    assert_eq!(app.view_state.current_view, View::Logs);
    assert_eq!(
        app.back_target(),
        Some(View::WorkloadDetail),
        "Back from pod logs returns to the workload detail"
    );

    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(app.view_state.current_view, View::WorkloadDetail);
    assert!(app.logs.session.is_none(), "Esc stops the stream");
}

#[test]
fn workload_logs_multiple_pods_open_submenu() {
    let mut app = app_on_graph_with_workloads();
    app.view_state.current_view = View::WorkloadDetail;
    app.async_state
        .workload
        .set_result(workload_data(&["podinfo-abc", "podinfo-def"]));

    app.handle_key(make_key(KeyCode::Char('l')));

    let submenu = app
        .view_state
        .submenu_state
        .as_ref()
        .expect("multiple pods open a submenu");
    assert_eq!(submenu.command, "pod-logs");
    assert_eq!(submenu.items.len(), 2);
    assert_eq!(submenu.items[0].value, "flux-system/podinfo-abc");

    // Selecting a pod opens its log stream
    app.handle_key(make_key(KeyCode::Enter));
    assert_eq!(app.view_state.current_view, View::Logs);
    assert_eq!(app.back_target(), Some(View::WorkloadDetail));
}

fn footer_labels(app: &App) -> Vec<(&'static str, &'static str)> {
    app.footer_commands()
        .into_iter()
        .map(|c| (c.key, c.label))
        .collect()
}

#[test]
fn footer_follows_the_current_view() {
    let mut app = app_on_graph_with_workloads();

    // Resource views keep the full Flux set.
    app.view_state.current_view = View::ResourceList;
    assert!(footer_labels(&app).contains(&("r", "Resume")));

    // Workload detail advertises its own meaning of r and ^d.
    app.view_state.current_view = View::WorkloadDetail;
    let labels = footer_labels(&app);
    assert!(labels.contains(&("r", "Restart")));
    assert!(labels.contains(&("^d", "Delete pod")));
    assert!(!labels.contains(&("r", "Resume")));
    assert!(!labels.contains(&("s", "Suspend")));
}

#[test]
fn native_object_footer_offers_only_viewing_keys() {
    let mut app = app_on_graph_with_resource_group();
    app.handle_key(make_key(KeyCode::Enter)); // graph → inventory
    assert!(footer_labels(&app).contains(&("Enter/d", "Describe")));

    app.handle_key(make_key(KeyCode::Char('y'))); // native YAML
    let labels = footer_labels(&app);
    assert!(labels.contains(&("d", "Describe")));
    assert!(!labels.iter().any(|(_, label)| *label == "Suspend"));
}

#[tokio::test]
async fn c_in_logs_switches_container() {
    use crate::tui::app::logs::LogEvent;
    let mut app = create_test_app(false);
    app.open_pod_logs("apps", "web-abc");
    let (_, tx) = app.logs.dispatch().unwrap();
    tx.send(LogEvent::Containers {
        selected: "app".to_string(),
        available: vec!["app".to_string(), "istio-proxy".to_string()],
    })
    .unwrap();
    app.logs.drain();

    app.handle_key(make_key(KeyCode::Char('c')));
    let submenu = app.view_state.submenu_state.as_ref().expect("picker opens");
    assert_eq!(submenu.command, "log-container");
    assert_eq!(submenu.items[0].display_text, "app (current)");

    app.handle_key(make_key(KeyCode::Char('j')));
    app.handle_key(make_key(KeyCode::Enter));

    // A new stream is queued for the chosen container of the same pod.
    let (request, _) = app.logs.dispatch().expect("switch queues a stream");
    assert_eq!(request.pod, "web-abc");
    assert_eq!(request.container.as_deref(), Some("istio-proxy"));
    assert_eq!(app.view_state.current_view, View::Logs);
}

#[test]
fn workload_load_starts_metrics_once_and_back_stops_them() {
    let mut app = app_on_graph_with_workloads();
    app.view_state.current_view = View::WorkloadList;
    app.navigate_to(View::WorkloadDetail);
    let mut data = workload_data(&["podinfo-abc"]);
    data.pod_selector = Some("app=podinfo".to_string());

    app.on_workload_loaded(data.clone());
    let request = app.async_state.workload_metrics.pending().cloned().unwrap();
    assert_eq!(request.selector, "app=podinfo");
    assert_eq!(request.namespace, "flux-system");
    let (_, _tx) = app.async_state.workload_metrics.dispatch().unwrap();

    // A live refresh with the same selector keeps the running task (and
    // its CPU counters) instead of restarting it.
    app.on_workload_loaded(data);
    assert!(app.async_state.workload_metrics.pending().is_none());
    assert!(app.async_state.workload_metrics.key().is_some());

    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(app.view_state.current_view, View::WorkloadList);
    assert!(
        app.async_state.workload_metrics.key().is_none(),
        "stopped on leave"
    );
}

fn kind(group: &str, kind: &str, plural: &str) -> crate::models::kinds::KindSpec {
    crate::models::kinds::KindSpec::generic(
        crate::models::kinds::Gvk {
            group: group.into(),
            version: "v1".into(),
            kind: kind.into(),
        },
        crate::models::kinds::KindScope::Namespaced,
        plural.into(),
        vec![],
    )
}

fn with_rows(app: &mut App, rows: &[(&str, &str)]) {
    let columns = app
        .async_state
        .kind_list
        .key()
        .map(|r| r.spec.columns.clone())
        .unwrap_or_default();
    app.async_state
        .kind_list
        .set_result(crate::kube::kind_list::KindListSnapshot {
            columns,
            rows: rows
                .iter()
                .map(|(ns, name)| crate::kube::kind_list::KindRow {
                    namespace: ns.to_string(),
                    name: name.to_string(),
                    health: crate::kube::object_status::ObjectHealth::Current,
                    cells: vec![],
                    created: None,
                    ownership: crate::kube::ownership::Ownership::Unmanaged,
                })
                .collect(),
            total: rows.len(),
        });
}

#[test]
fn kind_lists_stack_history_and_esc_walks_back_home() {
    let mut app = create_test_app(false);
    app.view_state.current_view = View::ResourceList;

    app.open_kind_list(kind("apps", "Deployment", "deployments"));
    assert_eq!(app.view_state.current_view, View::KindList);
    let request = app.async_state.kind_list.pending().cloned().unwrap();
    assert_eq!(request.spec.gvk.kind, "Deployment");
    assert_eq!(
        request.namespace, app.namespace,
        "follows the current namespace"
    );

    app.open_kind_list(kind("", "Pod", "pods"));
    assert_eq!(
        app.view_state.nav_stack,
        [
            NavEntry::View(View::ResourceList),
            NavEntry::Kind(kind("apps", "Deployment", "deployments"))
        ],
        "Flux, then deployments"
    );

    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(app.view_state.current_view, View::KindList);
    assert_eq!(
        app.async_state
            .kind_list
            .key()
            .map(|r| r.spec.gvk.kind.clone()),
        Some("Deployment".to_string())
    );

    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(app.view_state.current_view, View::ResourceList);
    assert!(
        app.async_state.kind_list.key().is_none(),
        "watch stopped at home"
    );
}

#[test]
fn flux_command_goes_home_from_any_depth() {
    let mut app = create_test_app(false);
    app.view_state.current_view = View::ResourceList;
    app.open_kind_list(kind("apps", "Deployment", "deployments"));
    app.open_kind_list(kind("", "Pod", "pods"));

    app.ui_state.command_buffer = "flux".to_string();
    app.execute_command();

    assert_eq!(app.view_state.current_view, View::ResourceList);
    assert!(app.view_state.nav_stack.is_empty());
    assert!(app.async_state.kind_list.key().is_none());
}

#[test]
fn d_on_a_kind_row_describes_it_and_back_returns_to_the_list() {
    let mut app = create_test_app(false);
    app.view_state.current_view = View::ResourceList;
    app.open_kind_list(kind("apps", "Deployment", "deployments"));
    with_rows(&mut app, &[("apps", "api"), ("apps", "web")]);

    app.handle_key(make_key(KeyCode::Char('j')));
    // `d` always describes (Enter opens the workload detail for
    // workload kinds — covered separately).
    app.handle_key(make_key(KeyCode::Char('d')));

    assert_eq!(app.view_state.current_view, View::ResourceDescribe);
    assert_eq!(
        app.async_state.describe.pending().cloned(),
        Some(ObjectRef::native("apps/v1", "Deployment", "apps", "web"))
    );
    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(app.view_state.current_view, View::KindList);
    assert_eq!(app.view_state.selected_index, 1);
}

#[test]
fn filter_narrows_kind_rows() {
    let mut app = create_test_app(false);
    app.open_kind_list(kind("apps", "Deployment", "deployments"));
    with_rows(
        &mut app,
        &[("apps", "api"), ("apps", "web"), ("ops", "webhook")],
    );
    app.view_state.filter = "web".to_string();
    let names: Vec<_> = app
        .filtered_kind_rows()
        .iter()
        .map(|r| r.name.clone())
        .collect();
    assert_eq!(names, ["web", "webhook"]);
}

#[test]
fn namespace_switch_restarts_the_kind_list() {
    let mut app = create_test_app(false);
    app.open_kind_list(kind("apps", "Deployment", "deployments"));
    app.switch_namespace(Some("ops".to_string()));
    let request = app.async_state.kind_list.pending().cloned().unwrap();
    assert_eq!(request.namespace.as_deref(), Some("ops"));
    assert_eq!(app.view_state.current_view, View::KindList);
}

#[test]
fn flux_aliases_still_filter_the_flux_list() {
    let mut app = create_test_app(false);
    app.open_kind_list(kind("apps", "Deployment", "deployments"));
    app.ui_state.command_buffer = "ks".to_string();
    app.execute_command();
    assert_eq!(app.view_state.current_view, View::ResourceList);
    assert_eq!(
        app.view_state.selected_resource_type.as_deref(),
        Some("Kustomization")
    );
}

#[test]
fn native_resources_opt_out_keeps_flux_only() {
    let mut app = create_test_app(false);
    app.config.native_resources = false;
    app.view_state.current_view = View::ResourceList;
    app.open_kind_list(kind("apps", "Deployment", "deployments"));
    assert_eq!(app.view_state.current_view, View::ResourceList);
    assert!(
        app.ui_state
            .status_message
            .as_ref()
            .is_some_and(|(m, _)| m.contains(":native turns it on"))
    );
}

#[test]
fn unknown_kind_rechecks_discovery_at_most_every_30s() {
    let mut app = create_test_app(false);
    assert!(app.refresh_kind_discovery(), "first unknown kind re-checks");
    assert!(app.async_state.kind_discovery.is_loading());
    assert!(!app.refresh_kind_discovery(), "throttled while recent");

    app.config.native_resources = false;
    app.async_state.kind_discovery.clear();
    app.async_state.kind_discovery_at = None;
    assert!(!app.refresh_kind_discovery(), "never when opted out");
}

#[test]
fn edit_on_native_objects_explains_and_suggests_kubectl() {
    let mut app = create_test_app(false);
    app.view_state.current_view = View::ResourceList;
    app.open_kind_list(kind("metallb.io", "IPAddressPool", "ipaddresspools"));
    with_rows(&mut app, &[("metallb-system", "main-pool")]);

    app.handle_key(make_key(KeyCode::Char('e')));

    assert_eq!(app.view_state.current_view, View::KindList, "no edit view");
    let (message, is_error) = app.ui_state.status_message.clone().unwrap();
    assert!(is_error);
    assert!(
        message.contains("IPAddressPool can't be edited here"),
        "{message}"
    );
    assert!(message.contains("kubectl edit ipaddresspool.metallb.io/main-pool -n metallb-system"));
}

#[test]
fn edit_on_unwatched_flux_object_points_at_namespace_switch() {
    let target = ObjectRef::native(
        "kustomize.toolkit.fluxcd.io/v1",
        "Kustomization",
        "team-a",
        "apps",
    );
    assert_eq!(
        native_edit_message(&target),
        "Kustomization team-a/apps isn't in the watched namespace — :ns team-a to edit it"
    );
}

#[test]
fn live_updates_keep_the_cursor_on_the_same_object() {
    let mut app = create_test_app(false);
    app.open_kind_list(kind("apps", "Deployment", "deployments"));
    with_rows(&mut app, &[("apps", "api"), ("apps", "web")]);
    app.handle_key(make_key(KeyCode::Char('j'))); // on "web"

    // A new object sorts above the cursor.
    let row = |name: &str| crate::kube::kind_list::KindRow {
        namespace: "apps".into(),
        name: name.into(),
        health: crate::kube::object_status::ObjectHealth::Current,
        cells: vec![],
        created: None,
        ownership: crate::kube::ownership::Ownership::Unmanaged,
    };
    app.on_kind_list_snapshot(crate::kube::kind_list::KindListSnapshot {
        columns: vec![],
        rows: vec![row("aaa"), row("api"), row("web")],
        total: 3,
    });
    assert_eq!(app.view_state.selected_index, 2, "still on web");

    // The selected object disappears: clamp instead of pointing past the end.
    app.on_kind_list_snapshot(crate::kube::kind_list::KindListSnapshot {
        columns: vec![],
        rows: vec![row("aaa")],
        total: 1,
    });
    assert_eq!(app.view_state.selected_index, 0);
}

#[test]
fn esc_clears_an_applied_kind_filter_before_leaving() {
    let mut app = create_test_app(false);
    app.view_state.current_view = View::ResourceList;
    app.open_kind_list(kind("apps", "Deployment", "deployments"));
    with_rows(&mut app, &[("apps", "api"), ("apps", "web")]);

    app.handle_key(make_key(KeyCode::Char('/')));
    for c in "web".chars() {
        app.handle_key(make_key(KeyCode::Char(c)));
    }
    app.handle_key(make_key(KeyCode::Enter));
    assert_eq!(app.view_state.filter, "web");
    assert_eq!(app.filtered_kind_rows().len(), 1);

    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(
        app.view_state.current_view,
        View::KindList,
        "first Esc clears"
    );
    assert!(app.view_state.filter.is_empty());

    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(app.view_state.current_view, View::ResourceList);
}

#[test]
fn native_edit_message_handles_core_and_cluster_scoped_kinds() {
    let pod = ObjectRef::native("v1", "Pod", "apps", "web-1");
    assert!(native_edit_message(&pod).ends_with("kubectl edit pod/web-1 -n apps"));
    let node = ObjectRef::native("v1", "Node", "", "worker-1");
    assert!(native_edit_message(&node).ends_with("kubectl edit node/worker-1"));
}

#[test]
fn ns_picker_lists_every_namespace_with_flux_ones_first_and_marked() {
    let mut app = create_test_app(false);
    app.update_namespace_hotkeys(vec!["apps".into(), "infra".into()]);
    app.all_namespaces = vec![
        "zeta".into(),
        "apps".into(),
        "default".into(),
        "flux-system".into(),
        "infra".into(),
        "kube-system".into(),
    ];
    let options = app.namespace_picker_options();
    assert_eq!(
        options,
        [
            "all",
            "flux-system",
            "apps",
            "infra",
            "default",
            "kube-system",
            "zeta"
        ],
        "hotkeys/Flux namespaces first, the rest alphabetically, no duplicates"
    );

    app.ui_state.command_buffer = "ns".to_string();
    app.execute_command();
    let submenu = app.view_state.submenu_state.as_ref().unwrap();
    let apps = submenu.items.iter().find(|i| i.value == "apps").unwrap();
    assert!(
        apps.display_text.contains("flux"),
        "Flux namespaces are marked"
    );
    let zeta = submenu.items.iter().find(|i| i.value == "zeta").unwrap();
    assert_eq!(zeta.display_text, "zeta");
}

#[test]
fn ns_picker_is_unchanged_when_native_resources_are_off() {
    let mut app = create_test_app(false);
    app.config.native_resources = false;
    app.update_namespace_hotkeys(vec!["apps".into()]);
    app.request_namespace_list();
    assert!(
        !app.async_state.namespace_list.is_loading(),
        "no extra call"
    );
    assert_eq!(
        app.namespace_picker_options(),
        ["all", "flux-system", "apps"]
    );
}

#[test]
fn ns_picker_opened_before_the_list_arrives_fills_in_place() {
    // The reported bug: :deploy then :ns showed only the Flux namespaces
    // until the picker was reopened, because it was built before the
    // cluster-wide list arrived.
    let mut app = create_test_app(false);
    app.update_namespace_hotkeys(vec!["apps".into()]);
    app.ui_state.command_buffer = "ns".to_string();
    app.execute_command();
    let picker = app.view_state.submenu_state.as_ref().unwrap();
    assert!(picker.title.as_deref().unwrap().contains("loading"));
    let values = |app: &App| -> Vec<String> {
        let picker = app.view_state.submenu_state.as_ref().unwrap();
        picker.items.iter().map(|i| i.value.clone()).collect()
    };
    assert_eq!(values(&app), ["all", "flux-system", "apps"]);

    // Pick "apps" and type a filter while it loads.
    let picker = app.view_state.submenu_state.as_mut().unwrap();
    picker.selected_index = 2;
    picker.filter_mode = true;

    let _ = app.async_state.namespace_list.dispatch();
    let _ = app.async_state.namespace_list.try_recv();
    app.set_all_namespaces(vec![
        "apps".into(),
        "default".into(),
        "flux-system".into(),
        "kube-system".into(),
    ]);
    assert_eq!(
        values(&app),
        ["all", "flux-system", "apps", "default", "kube-system"],
        "the open picker now lists every namespace, Flux ones first"
    );
    let picker = app.view_state.submenu_state.as_ref().unwrap();
    assert_eq!(
        picker.selected_value().as_deref(),
        Some("apps"),
        "selection kept"
    );
    assert!(picker.filter_mode, "filter state kept");
    assert_eq!(picker.title.as_deref(), Some("Select Namespace"));
}

#[test]
fn ns_picker_marks_and_lifts_namespaces_seen_with_flux_resources() {
    let mut app = create_test_app(false);
    // Discovery at connect time only found flux-system...
    app.update_namespace_hotkeys(vec![]);
    app.all_namespaces = vec!["apps".into(), "default".into(), "flux-system".into()];
    // ...but the watchers have since seen a HelmRelease in "apps".
    add_helm_release(&mut app);
    assert_eq!(
        app.namespace_picker_options(),
        ["all", "flux-system", "apps", "default"]
    );
    let picker = app.build_namespace_picker().unwrap();
    let apps = picker.items.iter().find(|i| i.value == "apps").unwrap();
    assert!(apps.display_text.contains("flux"));
    let default = picker.items.iter().find(|i| i.value == "default").unwrap();
    assert_eq!(default.display_text, "default");
}

#[test]
fn ns_picker_puts_non_flux_hotkeys_after_flux_namespaces() {
    let mut app = create_test_app(false);
    app.config.namespace_hotkeys = vec!["all".into(), "scratch".into(), "apps".into()];
    app.update_namespace_hotkeys(vec!["apps".into()]);
    app.all_namespaces = vec!["apps".into(), "scratch".into(), "zeta".into()];
    assert_eq!(
        app.namespace_picker_options(),
        ["all", "apps", "scratch", "zeta"]
    );
}

#[test]
fn context_switch_rediscovers_flux_namespaces_and_hotkeys() {
    let mut app = create_test_app(false);
    app.update_namespace_hotkeys(vec!["old-apps".into(), "old-infra".into()]);
    app.all_namespaces = vec!["old-apps".into(), "old-infra".into(), "default".into()];

    app.complete_context_switch("other".into(), Some("flux-system".into()));
    assert_eq!(
        app.namespace_hotkeys(),
        ["all", "flux-system"],
        "the old cluster's hotkeys are gone at once"
    );
    assert!(app.flux_namespaces.is_empty());
    assert!(app.all_namespaces.is_empty());
    assert!(app.async_state.flux_namespace_discovery.is_loading());
    assert_eq!(app.namespace_picker_options(), ["all", "flux-system"]);

    // The new cluster's discovery lands while the picker is open.
    app.ui_state.command_buffer = "ns".to_string();
    app.execute_command();
    let _ = app.async_state.flux_namespace_discovery.dispatch();
    let _ = app.async_state.flux_namespace_discovery.try_recv();
    app.set_flux_namespaces(vec!["new-apps".into()]);
    assert_eq!(app.namespace_hotkeys(), ["all", "flux-system", "new-apps"]);
    let picker = app.view_state.submenu_state.as_ref().unwrap();
    let new_apps = picker.items.iter().find(|i| i.value == "new-apps").unwrap();
    assert!(new_apps.display_text.contains("flux"));
    assert!(!picker.items.iter().any(|i| i.value.starts_with("old-")));
}

#[test]
fn context_switch_with_configured_hotkeys_skips_rediscovery() {
    let mut app = create_test_app(false);
    app.config.namespace_hotkeys = vec!["all".into(), "team".into()];
    app.complete_context_switch("other".into(), None);
    assert_eq!(app.namespace_hotkeys(), ["all", "team"]);
    assert!(!app.async_state.flux_namespace_discovery.is_loading());
}

#[test]
fn context_switch_leaves_views_tied_to_the_old_cluster() {
    for (view, stays) in [
        (View::WorkloadDetail, false),
        (View::ResourceDescribe, false),
        (View::Logs, false),
        (View::EventList, false),
        (View::ResourceGraph, false),
        (View::KindList, false),
        (View::ResourceFavorites, true),
        (View::Pulse, true),
    ] {
        let mut app = create_test_app(false);
        app.view_state.current_view = view;
        app.selection_state.selected_resource_key = Some("Kustomization:flux-system:apps".into());
        app.selection_state.native_object = Some(ObjectRef::native("v1", "Pod", "a", "b"));
        app.complete_context_switch("other".into(), None);
        let expected = if stays { view } else { View::ResourceList };
        assert_eq!(app.view_state.current_view, expected, "{view:?}");
        assert!(app.selection_state.selected_resource_key.is_none());
        assert!(app.selection_state.native_object.is_none());
    }
}

#[test]
fn back_from_a_kind_row_keeps_the_live_list_and_cursor() {
    let mut app = create_test_app(false);
    open_list(
        &mut app,
        "apps",
        "Deployment",
        "deployments",
        &[("apps", "a"), ("apps", "b")],
    );
    app.view_state.selected_index = 1;

    app.handle_key(make_key(KeyCode::Char('d')));
    assert_eq!(app.view_state.current_view, View::ResourceDescribe);
    app.handle_key(make_key(KeyCode::Esc));

    assert_eq!(app.view_state.current_view, View::KindList);
    assert!(
        app.async_state.kind_list.result().is_some(),
        "not restarted: the snapshot is still there"
    );
    assert_eq!(app.view_state.selected_index, 1, "cursor kept");
}

#[test]
fn back_reopens_a_kind_list_left_behind_by_a_namespace_switch() {
    let mut app = create_test_app(false);
    open_list(
        &mut app,
        "apps",
        "Deployment",
        "deployments",
        &[("apps", "a")],
    );
    app.handle_key(make_key(KeyCode::Char('d')));
    app.switch_namespace(Some("other".to_string()));

    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(app.view_state.current_view, View::KindList);
    let request = app.async_state.kind_list.pending().cloned().unwrap();
    assert_eq!(request.namespace.as_deref(), Some("other"));
}

#[test]
fn list_logs_replace_the_fetched_workload_so_back_returns_to_the_list() {
    let mut app = app_on_graph_with_workloads();
    app.handle_key(make_key(KeyCode::Enter)); // workload list
    app.handle_key(make_key(KeyCode::Char('l')));
    assert_eq!(app.view_state.current_view, View::WorkloadDetail);

    app.on_workload_loaded(workload_data(&["podinfo-abc"]));
    assert_eq!(app.view_state.current_view, View::Logs);
    assert_eq!(app.back_target(), Some(View::WorkloadList));
    assert!(
        app.async_state.workload.result().is_none(),
        "detail released"
    );

    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(app.view_state.current_view, View::WorkloadList);
    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(app.view_state.current_view, View::ResourceGraph);
}

#[test]
fn top_level_commands_reset_history_to_the_flux_list() {
    let mut app = create_test_app(false);
    open_list(
        &mut app,
        "apps",
        "Deployment",
        "deployments",
        &[("apps", "a")],
    );
    app.handle_key(make_key(KeyCode::Char('d')));

    app.ui_state.command_buffer = "events".to_string();
    app.execute_command();
    assert_eq!(app.view_state.current_view, View::EventList);
    assert_eq!(
        app.view_state.nav_stack,
        [NavEntry::View(View::ResourceList)]
    );
    assert!(
        app.async_state.kind_list.key().is_none(),
        "kind list closed"
    );

    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(app.view_state.current_view, View::ResourceList);
}

#[test]
fn native_off_drops_kind_lists_from_the_back_history() {
    let mut app = create_test_app(false);
    open_list(
        &mut app,
        "apps",
        "Deployment",
        "deployments",
        &[("apps", "a")],
    );
    app.handle_key(make_key(KeyCode::Char('d')));

    app.ui_state.command_buffer = "native".to_string();
    app.execute_command();
    assert!(!app.config.native_resources);
    assert_eq!(app.view_state.current_view, View::ResourceList);
    assert!(app.view_state.nav_stack.is_empty());
}

#[test]
fn back_history_is_bounded() {
    let mut app = create_test_app(false);
    for i in 0..100 {
        app.view_state.current_view = if i % 2 == 0 {
            View::ResourceList
        } else {
            View::Pulse
        };
        app.push_current_view();
    }
    assert!(app.view_state.nav_stack.len() <= 32);
}

#[test]
fn late_async_results_dont_navigate_away_from_where_the_user_went() {
    let mut app = create_test_app(false);
    app.view_state.current_view = View::ResourceList;
    app.view_state.nav_stack = vec![NavEntry::View(View::ResourceFavorites)];

    app.back_from(View::ResourceGraph);
    assert_eq!(app.view_state.current_view, View::ResourceList);

    app.async_state.edit_pending = Some(ResourceKey::new("Kustomization", "a", "b").into());
    app.leave_edit();
    assert_eq!(app.view_state.current_view, View::ResourceList);
    assert!(app.async_state.edit_pending.is_none(), "edit state cleared");
    assert_eq!(app.view_state.nav_stack.len(), 1, "history untouched");
}

#[test]
fn edit_returns_to_the_exact_view_it_was_opened_from() {
    let mut app = create_test_app(false);
    add_resource(&mut app);
    app.view_state.current_view = View::ResourceList;
    app.handle_key(make_key(KeyCode::Char('y')));
    app.handle_key(make_key(KeyCode::Char('e')));
    assert_eq!(app.view_state.current_view, View::ResourceEdit);

    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(app.view_state.current_view, View::ResourceYAML);
    assert!(app.async_state.edit_pending.is_none());
    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(app.view_state.current_view, View::ResourceList);
}

fn trace_command(app: &mut App, cmd: &str) -> Option<ResourceKey> {
    app.async_state.trace.clear();
    app.ui_state.command_buffer = cmd.to_string();
    app.execute_command();
    app.async_state.trace.pending().cloned()
}

#[test]
fn trace_command_resolves_kinds_through_the_registry() {
    let mut app = create_test_app(false);
    app.namespace = Some("apps".into());
    assert_eq!(
        trace_command(&mut app, "trace ks/podinfo"),
        Some(ResourceKey::new("Kustomization", "apps", "podinfo"))
    );
    assert_eq!(
        trace_command(&mut app, "trace helmrelease/podinfo"),
        Some(ResourceKey::new("HelmRelease", "apps", "podinfo"))
    );
    // Exact kind names pass through (resolved when the trace runs).
    assert_eq!(
        trace_command(&mut app, "trace Deployment/web"),
        Some(ResourceKey::new("Deployment", "apps", "web"))
    );
    assert_eq!(trace_command(&mut app, "trace ks"), None, "usage");
    assert_eq!(trace_command(&mut app, "trace ks/a/b"), None, "usage");
}

#[test]
fn trace_aliases_for_kubernetes_kinds_need_native_resources() {
    let mut app = create_test_app(false);
    app.config.native_resources = false;
    assert_eq!(trace_command(&mut app, "trace deploy/web"), None);
    let (message, is_error) = app.ui_state.status_message.clone().unwrap();
    assert!(is_error && message.contains(":native"), "{message}");
    // The exact kind name still works without discovery.
    assert!(trace_command(&mut app, "trace Deployment/web").is_some());
}

fn open_list(app: &mut App, group: &str, kind_name: &str, plural: &str, rows: &[(&str, &str)]) {
    app.view_state.current_view = View::ResourceList;
    app.open_kind_list(kind(group, kind_name, plural));
    with_rows(app, rows);
}

fn footer_keys(app: &App) -> Vec<(&'static str, &'static str)> {
    app.footer_commands()
        .into_iter()
        .map(|c| (c.key, c.label))
        .collect()
}

#[test]
fn deploy_list_enter_opens_the_shared_workload_detail_and_back_returns() {
    let mut app = create_test_app(false);
    open_list(
        &mut app,
        "apps",
        "Deployment",
        "deployments",
        &[("apps", "web")],
    );

    app.handle_key(make_key(KeyCode::Enter));

    assert_eq!(app.view_state.current_view, View::WorkloadDetail);
    assert_eq!(
        app.async_state
            .workload
            .pending()
            .map(|k| k.to_key_string()),
        Some("Deployment:apps:web".to_string())
    );
    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(
        app.view_state.current_view,
        View::KindList,
        "back to :deploy, not the graph"
    );
    assert!(
        app.async_state.kind_list.key().is_some(),
        "list kept live meanwhile"
    );
}

#[test]
fn deploy_list_r_confirms_a_restart_like_the_workload_views() {
    let mut app = create_test_app(false);
    open_list(
        &mut app,
        "apps",
        "Deployment",
        "deployments",
        &[("apps", "web")],
    );
    assert!(footer_keys(&app).contains(&("r", "Restart")));

    app.handle_key(make_key(KeyCode::Char('r')));

    assert_eq!(
        pending_restart(&app),
        Some(("Deployment".into(), "apps".into(), "web".into()))
    );
}

#[test]
fn pod_list_offers_logs_and_delete_but_not_restart() {
    use crate::kube::workloads::ObjectAction;
    let mut app = create_test_app(false);
    open_list(&mut app, "", "Pod", "pods", &[("apps", "web-abc")]);
    let footer = footer_keys(&app);
    assert!(footer.contains(&("l", "Logs")));
    assert!(footer.contains(&("^d", "Delete pod")));
    assert!(!footer.iter().any(|(k, _)| *k == "r"));

    app.handle_key(make_ctrl_key(KeyCode::Char('d')));
    assert_eq!(
        pending_workload_action(&app),
        Some(ObjectAction::DeletePod {
            namespace: "apps".into(),
            name: "web-abc".into(),
        })
    );
    app.handle_key(make_key(KeyCode::Char('n'))); // cancel

    app.handle_key(make_key(KeyCode::Char('l')));
    assert_eq!(app.view_state.current_view, View::Logs);
    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(
        app.view_state.current_view,
        View::KindList,
        "logs return to :po"
    );
}

#[test]
fn kind_list_actions_respect_read_only() {
    let mut app = create_test_app(false);
    app.config.read_only = true;
    open_list(&mut app, "", "Pod", "pods", &[("apps", "web-abc")]);
    app.handle_key(make_ctrl_key(KeyCode::Char('d')));
    assert!(app.async_state.confirmation_pending.is_none());
}

#[test]
fn view_only_kinds_offer_no_actions() {
    let mut app = create_test_app(false);
    open_list(&mut app, "", "ConfigMap", "configmaps", &[("apps", "cfg")]);
    let footer = footer_keys(&app);
    assert!(!footer.iter().any(|(k, _)| ["r", "l", "^d"].contains(k)));
    app.handle_key(make_key(KeyCode::Char('r')));
    app.handle_key(make_ctrl_key(KeyCode::Char('d')));
    assert!(app.async_state.confirmation_pending.is_none());
    app.handle_key(make_key(KeyCode::Enter));
    assert_eq!(
        app.view_state.current_view,
        View::ResourceDescribe,
        "Enter describes"
    );
}

#[test]
fn namespace_hotkeys_follow_into_kind_lists() {
    let mut app = create_test_app(false);
    app.update_namespace_hotkeys(vec!["apps".into()]);
    open_list(&mut app, "apps", "Deployment", "deployments", &[]);
    app.handle_key(make_key(KeyCode::Char('2'))); // hotkeys: all, flux-system, apps
    assert_eq!(app.namespace.as_deref(), Some("apps"));
    let request = app.async_state.kind_list.pending().cloned().unwrap();
    assert_eq!(
        request.namespace.as_deref(),
        Some("apps"),
        "list restarted in the new namespace"
    );
}

#[test]
fn kind_typed_during_discovery_is_held_not_rejected() {
    let mut app = create_test_app(false);
    app.view_state.current_view = View::ResourceList;
    app.request_kind_discovery();
    assert!(app.async_state.kind_discovery.is_loading());

    app.ui_state.command_buffer = "zzznotakind".to_string();
    app.execute_command();

    assert_eq!(app.pending_kind_command.as_deref(), Some("zzznotakind"));
    let (message, is_error) = app.ui_state.status_message.clone().unwrap();
    assert!(!is_error && message.contains("opens when ready"));

    // Discovery lands without it: now it's reported as unknown.
    app.on_kind_discovery_complete();
    assert!(app.pending_kind_command.is_none());
    assert!(
        app.ui_state
            .status_message
            .as_ref()
            .is_some_and(|(m, err)| *err && m.contains("Unknown command: 'zzznotakind'"))
    );
}

#[test]
fn native_resources_opt_out_blocks_flux_crd_discovery_everywhere() {
    let mut app = create_test_app(false);
    app.config.native_resources = false;
    app.config.discover_flux_resources = true;
    assert!(!app.config.flux_crd_discovery_enabled());

    // :discover can't turn it back on.
    app.ui_state.command_buffer = "discover".to_string();
    app.execute_command();
    assert!(app.config.discover_flux_resources, "flag untouched");
    assert!(!app.config.flux_crd_discovery_enabled());
    assert!(
        app.ui_state
            .status_message
            .as_ref()
            .is_some_and(|(m, err)| *err && m.contains("turn them on with :native"))
    );

    // Queued discovery events are ignored: no kind registered.
    let extra = crate::models::extra_kinds::ExtraKind {
        kind: "OptOutWidget".into(),
        group: "optout.example.com".into(),
        version: "v1".into(),
        plural: "optoutwidgets".into(),
        short_names: vec![],
    };
    app.apply_extra_kind_event(crate::watcher::WatchEvent::ExtraKindDiscovered(extra));
    assert!(
        crate::models::extra_kinds::global()
            .get("OptOutWidget")
            .is_none()
    );
}

#[test]
fn crd_discovery_events_apply_when_both_flags_are_on() {
    let mut app = create_test_app(false);
    app.config.native_resources = true;
    app.config.discover_flux_resources = true;
    let extra = crate::models::extra_kinds::ExtraKind {
        kind: "OptInWidget".into(),
        group: "optin.example.com".into(),
        version: "v1".into(),
        plural: "optinwidgets".into(),
        short_names: vec![],
    };
    app.apply_extra_kind_event(crate::watcher::WatchEvent::ExtraKindDiscovered(extra));
    assert!(
        crate::models::extra_kinds::global()
            .get("OptInWidget")
            .is_some()
    );
    app.apply_extra_kind_event(crate::watcher::WatchEvent::ExtraKindRemoved(
        "OptInWidget".into(),
    ));
    assert!(
        crate::models::extra_kinds::global()
            .get("OptInWidget")
            .is_none()
    );
}

#[test]
fn native_toggle_turns_the_preview_on_and_off_for_the_session() {
    let mut app = create_test_app(false);
    app.config.native_resources = false;
    app.view_state.current_view = View::ResourceList;

    app.ui_state.command_buffer = "native".to_string();
    app.execute_command();
    assert!(app.config.native_resources);
    assert!(
        app.async_state.kind_discovery.is_loading(),
        "discovers kinds"
    );
    assert!(
        app.async_state.namespace_list.is_loading(),
        "lists namespaces"
    );
    assert!(
        app.ui_state
            .status_message
            .as_ref()
            .is_some_and(|(m, err)| !err && m.contains("preview"))
    );

    app.open_kind_list(kind("apps", "Deployment", "deployments"));
    assert_eq!(app.view_state.current_view, View::KindList);

    app.ui_state.command_buffer = "native".to_string();
    app.execute_command();
    assert!(!app.config.native_resources);
    assert_eq!(
        app.view_state.current_view,
        View::ResourceList,
        "back to Flux"
    );
    assert!(
        app.async_state.kind_list.key().is_none(),
        "kind watch stopped"
    );
    assert!(!app.async_state.kind_discovery.is_loading());
}

#[test]
fn native_toggle_carries_flux_crd_discovery_with_it() {
    let mut app = create_test_app(false);
    app.config.native_resources = false;
    app.config.discover_flux_resources = true;
    assert!(!app.config.flux_crd_discovery_enabled());
    app.ui_state.command_buffer = "native".to_string();
    app.execute_command();
    assert!(app.config.flux_crd_discovery_enabled(), "both flags on now");
    app.ui_state.command_buffer = "native".to_string();
    app.execute_command();
    assert!(!app.config.flux_crd_discovery_enabled());
    assert!(
        app.config.discover_flux_resources,
        "the discovery preference is kept"
    );
}

#[test]
fn kind_commands_while_off_point_at_native() {
    let mut app = create_test_app(false);
    app.config.native_resources = false;
    for (command, expect) in [
        ("deploy", ":deploy is a Kubernetes kind"),
        ("ipaddresspools.metallb.io", "is a Kubernetes kind"),
        ("nosuchthing", ":native turns it on"),
    ] {
        app.ui_state.command_buffer = command.to_string();
        app.execute_command();
        let (message, _) = app.ui_state.status_message.clone().unwrap();
        assert!(message.contains(expect), "{command}: {message}");
        assert_eq!(app.view_state.current_view, View::ResourceList);
    }
}

/// The pending restart's (kind, namespace, name), checking it is the
/// action table's restart patch.
fn pending_restart(app: &App) -> Option<(String, String, String)> {
    match pending_workload_action(app)? {
        crate::kube::workloads::ObjectAction::Patch {
            target,
            verb,
            patch,
            ..
        } if verb == "Restart" => {
            assert!(
                patch
                    .pointer(
                        "/spec/template/metadata/annotations/kubectl.kubernetes.io~1restartedAt"
                    )
                    .is_some(),
                "restart patches the pod template: {patch}"
            );
            Some((target.kind, target.namespace, target.name))
        }
        _ => None,
    }
}

fn pending_workload_action(app: &App) -> Option<crate::kube::workloads::ObjectAction> {
    match app.async_state.confirmation_pending.as_ref()? {
        Confirmation::Object(action) => Some(action.clone()),
        Confirmation::Flux(_) | Confirmation::Edit(_) => None,
    }
}

fn open_workload_list(app: &mut App) {
    app.handle_key(make_key(KeyCode::Enter)); // graph → workload list
    assert_eq!(app.view_state.current_view, View::WorkloadList);
}

#[test]
fn r_in_workload_list_confirms_then_runs_restart() {
    let mut app = app_on_graph_with_workloads();
    open_workload_list(&mut app);

    app.handle_key(make_key(KeyCode::Char('r')));
    assert_eq!(
        pending_restart(&app),
        Some(("Deployment".into(), "flux-system".into(), "podinfo".into()))
    );
    let expected = pending_workload_action(&app).unwrap();
    assert!(
        app.confirmation_message()
            .is_some_and(|m| m.contains("Restart Deployment 'podinfo'"))
    );

    app.handle_key(make_key(KeyCode::Char('y')));
    assert!(app.async_state.confirmation_pending.is_none());
    assert_eq!(app.async_state.object_action.pending(), Some(&expected));
}

#[test]
fn restart_can_be_cancelled() {
    let mut app = app_on_graph_with_workloads();
    open_workload_list(&mut app);

    app.handle_key(make_key(KeyCode::Char('r')));
    app.handle_key(make_key(KeyCode::Char('n')));

    assert!(app.async_state.confirmation_pending.is_none());
    assert!(!app.async_state.object_action.is_loading());
    assert_eq!(app.view_state.current_view, View::WorkloadList);
}

#[test]
fn restart_is_blocked_in_read_only_mode() {
    let mut app = app_on_graph_with_workloads();
    app.config.read_only = true;
    open_workload_list(&mut app);

    app.handle_key(make_key(KeyCode::Char('r')));

    assert!(app.async_state.confirmation_pending.is_none());
    assert!(
        app.ui_state
            .status_message
            .as_ref()
            .is_some_and(|(_, is_error)| *is_error)
    );
}

#[test]
fn restart_rejects_cronjobs() {
    let mut app = app_on_graph_with_workloads();
    app.view_state.current_view = View::WorkloadDetail;
    let mut data = workload_data(&[]);
    data.kind = "CronJob".to_string();
    app.async_state.workload.set_result(data);

    app.handle_key(make_key(KeyCode::Char('r')));

    assert!(app.async_state.confirmation_pending.is_none());
}

#[test]
fn ctrl_d_in_workload_detail_deletes_the_only_pod() {
    use crate::kube::workloads::ObjectAction;
    let mut app = app_on_graph_with_workloads();
    app.view_state.current_view = View::WorkloadDetail;
    app.async_state
        .workload
        .set_result(workload_data(&["podinfo-abc"]));

    app.handle_key(make_ctrl_key(KeyCode::Char('d')));

    assert_eq!(
        pending_workload_action(&app),
        Some(ObjectAction::DeletePod {
            namespace: "flux-system".into(),
            name: "podinfo-abc".into(),
        })
    );
}

#[test]
fn ctrl_d_with_several_pods_picks_one_then_confirms() {
    use crate::kube::workloads::ObjectAction;
    let mut app = app_on_graph_with_workloads();
    app.view_state.current_view = View::WorkloadDetail;
    app.async_state
        .workload
        .set_result(workload_data(&["podinfo-abc", "podinfo-def"]));

    app.handle_key(make_ctrl_key(KeyCode::Char('d')));
    let submenu = app
        .view_state
        .submenu_state
        .as_ref()
        .expect("several pods open a picker");
    assert_eq!(submenu.command, "pod-delete");
    assert!(app.async_state.confirmation_pending.is_none());

    app.handle_key(make_key(KeyCode::Char('j')));
    app.handle_key(make_key(KeyCode::Enter));

    assert_eq!(
        pending_workload_action(&app),
        Some(ObjectAction::DeletePod {
            namespace: "flux-system".into(),
            name: "podinfo-def".into(),
        })
    );
}

#[test]
fn l_from_workload_list_continues_into_pod_logs_on_load() {
    let mut app = app_on_graph_with_workloads();
    app.handle_key(make_key(KeyCode::Enter)); // -> WorkloadList

    app.handle_key(make_key(KeyCode::Char('l')));
    assert_eq!(app.view_state.current_view, View::WorkloadDetail);
    assert!(app.logs_after_workload_load, "auto-logs intent recorded");
    assert!(app.async_state.workload.pending().is_some());

    // Simulate the fetch completing (what the main loop does)
    let _ = app.async_state.workload.dispatch();
    app.on_workload_loaded(workload_data(&["podinfo-abc"]));

    assert_eq!(
        app.view_state.current_view,
        View::Logs,
        "single-pod workload streams immediately after loading"
    );
    assert!(!app.logs_after_workload_load, "intent consumed");

    // Back returns to where `l` was pressed — the workload LIST
    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(app.view_state.current_view, View::WorkloadList);
}

#[test]
fn plain_enter_does_not_auto_open_logs_on_load() {
    let mut app = app_on_graph_with_workloads();
    app.handle_key(make_key(KeyCode::Enter)); // -> WorkloadList
    app.handle_key(make_key(KeyCode::Enter)); // -> WorkloadDetail (plain)

    let _ = app.async_state.workload.dispatch();
    app.on_workload_loaded(workload_data(&["podinfo-abc"]));

    assert_eq!(
        app.view_state.current_view,
        View::WorkloadDetail,
        "Enter shows the detail; it must not jump into logs"
    );
}

#[test]
fn ns_command_without_arg_opens_picker_and_selects() {
    let mut app = create_test_app(false);
    // Default hotkeys (no config, no discovery) are ["all", "flux-system"].
    app.ui_state.command_buffer = "ns".to_string();
    app.execute_command();

    let submenu = app
        .view_state
        .submenu_state
        .as_ref()
        .expect(":ns with no argument opens the namespace picker");
    assert_eq!(submenu.command, "ns");
    assert_eq!(submenu.items[0].value, "all");
    assert!(
        submenu.items.iter().any(|i| i.value == "flux-system"),
        "picker offers the discovered/hotkey namespaces"
    );

    // Selecting a namespace switches to it and closes the submenu.
    app.handle_key(make_key(KeyCode::Char('j'))); // -> flux-system
    app.handle_key(make_key(KeyCode::Enter));
    assert!(app.view_state.submenu_state.is_none());
    assert_eq!(app.namespace, Some("flux-system".to_string()));
}

#[test]
fn ns_command_with_arg_switches_directly_without_picker() {
    let mut app = create_test_app(false);
    app.ui_state.command_buffer = "ns kube-system".to_string();
    app.execute_command();

    assert!(
        app.view_state.submenu_state.is_none(),
        ":ns <name> switches directly, no picker"
    );
    assert_eq!(app.namespace, Some("kube-system".to_string()));

    // `:ns all` returns to the cluster-wide scope.
    app.ui_state.command_buffer = "ns all".to_string();
    app.execute_command();
    assert_eq!(app.namespace, None);
}

#[test]
fn forbidden_watcher_state_tracks_and_clears() {
    let mut app = create_test_app(false);
    assert!(app.forbidden_watchers.is_empty());

    app.watch_forbidden("HelmRelease".to_string());
    assert!(!app.forbidden_watchers.is_empty());
    assert!(app.forbidden_watchers.contains("HelmRelease"));

    // A recovered watch for the same kind clears the restriction.
    app.watch_recovered("HelmRelease");
    assert!(app.forbidden_watchers.is_empty());

    // Marking forbidden also drops any prior degraded flag for that kind.
    app.watch_degraded("HelmRelease".to_string());
    app.watch_forbidden("HelmRelease".to_string());
    assert!(!app.degraded_watchers.contains("HelmRelease"));
    assert!(app.forbidden_watchers.contains("HelmRelease"));
}

#[test]
fn access_notice_is_contextual_and_respects_config() {
    let mut app = create_test_app(false);
    // Nothing restricted -> no notice.
    assert_eq!(app.access_notice(), None);

    app.watch_forbidden("HelmRelease".to_string());

    // Unified list (no selected type) -> summary mentioning the kind.
    app.view_state.selected_resource_type = None;
    let notice = app.access_notice().expect("unified notice");
    assert!(
        notice.contains("HelmRelease"),
        "notice names the kind: {notice}"
    );

    // Selected the restricted kind -> specific notice.
    app.view_state.selected_resource_type = Some("HelmRelease".to_string());
    assert!(app.access_notice().unwrap().contains("HelmRelease"));

    // Selected a DIFFERENT, non-restricted kind -> no false alarm.
    app.view_state.selected_resource_type = Some("Kustomization".to_string());
    assert_eq!(app.access_notice(), None);

    // Config off -> silent, even when restricted and viewing the kind.
    app.view_state.selected_resource_type = Some("HelmRelease".to_string());
    app.config.ui.rbac_warnings = false;
    assert_eq!(app.access_notice(), None);
}

#[test]
fn switching_namespace_clears_forbidden_state() {
    let mut app = create_test_app(false);
    app.watch_forbidden("HelmRelease".to_string());
    assert!(!app.forbidden_watchers.is_empty());

    // A namespace switch restarts watchers, so stale RBAC state must clear.
    app.ui_state.command_buffer = "ns kube-system".to_string();
    app.execute_command();
    assert!(app.forbidden_watchers.is_empty());
}

#[test]
fn submenu_filter_follows_resource_filter_convention() {
    let mut app = create_test_app(false);
    add_controller_pod(&mut app, "source-controller-abc");
    add_controller_pod(&mut app, "helm-controller-def");
    app.ui_state.command_buffer = "logs".to_string();
    app.execute_command();

    // Keys don't filter until `/` enters filter mode
    app.handle_key(make_key(KeyCode::Char('x')));
    let submenu = app.view_state.submenu_state.as_ref().unwrap();
    assert!(
        submenu.filter.is_empty(),
        "typing without / must not filter"
    );
    assert!(!submenu.filter_mode);

    // `/` enters filter mode; typing narrows; Enter applies (keeps filter)
    app.handle_key(make_key(KeyCode::Char('/')));
    assert!(app.view_state.submenu_state.as_ref().unwrap().filter_mode);
    app.handle_key(make_key(KeyCode::Char('h')));
    let submenu = app.view_state.submenu_state.as_ref().unwrap();
    assert_eq!(submenu.filter, "h");
    assert_eq!(submenu.filtered_items().len(), 1);
    app.handle_key(make_key(KeyCode::Enter));
    let submenu = app.view_state.submenu_state.as_ref().unwrap();
    assert!(!submenu.filter_mode, "Enter applies the filter");
    assert_eq!(submenu.filter, "h", "filter survives applying");

    // Enter now selects the filtered item (streams its logs)
    app.handle_key(make_key(KeyCode::Enter));
    assert_eq!(app.view_state.current_view, View::Logs);
    let (request, _tx) = app.logs.dispatch().expect("log stream queued");
    assert_eq!(request.pod, "helm-controller-def");
}

#[test]
fn esc_in_submenu_filter_mode_cancels_like_resource_filter() {
    let mut app = create_test_app(false);
    add_controller_pod(&mut app, "source-controller-abc");
    app.ui_state.command_buffer = "logs".to_string();
    app.execute_command();

    app.handle_key(make_key(KeyCode::Char('/')));
    app.handle_key(make_key(KeyCode::Char('z')));
    // Esc exits filter mode and clears the filter — submenu stays open
    app.handle_key(make_key(KeyCode::Esc));
    let submenu = app.view_state.submenu_state.as_ref().unwrap();
    assert!(!submenu.filter_mode);
    assert!(submenu.filter.is_empty());
    assert_eq!(submenu.filtered_items().len(), 1);

    // A second Esc closes the submenu
    app.handle_key(make_key(KeyCode::Esc));
    assert!(app.view_state.submenu_state.is_none());
}

#[test]
fn colon_in_submenu_opens_command_mode() {
    let mut app = create_test_app(false);
    add_controller_pod(&mut app, "source-controller-abc");
    app.ui_state.command_buffer = "logs".to_string();
    app.execute_command();
    assert!(app.view_state.submenu_state.is_some());

    app.handle_key(make_key(KeyCode::Char(':')));

    assert!(app.view_state.submenu_state.is_none(), "submenu closes");
    assert!(app.ui_state.command_mode, "command mode opens");
}

#[test]
fn pulse_command_opens_dashboard_and_esc_returns() {
    let mut app = create_test_app(false);
    add_resource(&mut app);

    app.ui_state.command_buffer = "pulse".to_string();
    let result = app.execute_command();

    assert_eq!(result, None);
    assert_eq!(app.view_state.current_view, View::Pulse);

    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(app.view_state.current_view, View::ResourceList);
}

/// The discovered-kind registry is process-global, and `:discover` off
/// clears it wholesale, so the tests that register kinds must not run
/// concurrently with each other.
static REGISTRY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock_registry() -> std::sync::MutexGuard<'static, ()> {
    REGISTRY_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// #245: `:discover` flips the setting for the session and, when turning
/// discovery off, takes the discovered kinds and their resources with it.
#[test]
fn discover_command_toggles_discovery_and_purges_kinds() {
    use crate::models::extra_kinds::{ExtraKind, global};

    let _guard = lock_registry();
    let mut app = create_test_app(false);
    assert!(!app.config.discover_flux_resources);

    app.ui_state.command_buffer = "discover".to_string();
    app.execute_command();
    assert!(
        app.config.discover_flux_resources,
        "first :discover enables"
    );

    // A kind the discovery watcher would have registered, plus one of its
    // resources in the list state and the type filter pointing at it.
    global().insert(ExtraKind {
        kind: "DiscoverToggleTest".to_string(),
        group: "example.com".to_string(),
        version: "v1".to_string(),
        plural: "discovertoggletests".to_string(),
        short_names: vec![],
    });
    let key = resource_key("default", "widget", "DiscoverToggleTest");
    app.state.upsert(
        key.clone(),
        ResourceInfo {
            name: "widget".to_string(),
            namespace: "default".to_string(),
            resource_type: "DiscoverToggleTest".to_string(),
            age: None,
            suspended: None,
            ready: Some(true),
            message: None,
            revision: None,
            labels: HashMap::new(),
            annotations: HashMap::new(),
            last_reconciled: None,
            reconciliation_history: vec![],
        },
    );
    app.view_state.selected_resource_type = Some("DiscoverToggleTest".to_string());

    app.ui_state.command_buffer = "discovery".to_string();
    app.execute_command();

    assert!(
        !app.config.discover_flux_resources,
        "second :discover disables"
    );
    assert!(global().is_empty(), "discovered kinds are deregistered");
    assert!(
        app.state.get(&key).is_none(),
        "discovered resources are purged from the list"
    );
    assert_eq!(
        app.view_state.selected_resource_type, None,
        "a filter on a gone kind is cleared"
    );
}

/// #197: without discovery, unknown kinds stay unknown (zero impact);
/// once a kind is registered, its CRD-derived aliases resolve like
/// built-in resource commands. Uses a unique kind name because the
/// registry is process-global.
#[test]
fn discovered_kind_commands_resolve_only_when_registered() {
    use crate::models::extra_kinds::{ExtraKind, global};

    let _guard = lock_registry();
    let mut app = create_test_app(false);

    // Zero impact: unregistered kind is an unknown command
    app.ui_state.command_buffer = "zephyrtest".to_string();
    app.execute_command();
    assert!(app.view_state.selected_resource_type.is_none());
    assert!(
        app.ui_state
            .status_message
            .as_ref()
            .is_some_and(|(msg, is_err)| *is_err && msg.contains("Unknown command"))
    );

    global().insert(ExtraKind {
        kind: "ZephyrTest".to_string(),
        group: "example.com".to_string(),
        version: "v1".to_string(),
        plural: "zephyrtests".to_string(),
        short_names: vec!["zt9".to_string()],
    });

    // Kind, plural, and short name now resolve to the type filter
    for token in ["zephyrtest", "zephyrtests", "zt9"] {
        app.view_state.selected_resource_type = None;
        app.ui_state.command_buffer = token.to_string();
        app.execute_command();
        assert_eq!(
            app.view_state.selected_resource_type.as_deref(),
            Some("ZephyrTest"),
            "{token} should resolve"
        );
    }

    // Autocomplete offers the discovered aliases
    let matches = crate::tui::commands::find_matching_commands("zephyr");
    assert!(matches.contains(&"zephyrtest".to_string()));
    assert!(matches.contains(&"zephyrtests".to_string()));

    // GVK resolution works for read-only fetches (y/d)
    let gvk = crate::kube::get_gvk_for_resource_type("ZephyrTest").unwrap();
    assert_eq!(
        gvk,
        (
            "example.com".to_string(),
            "v1".to_string(),
            "zephyrtests".to_string()
        )
    );

    // Discovered kinds stay view-only: no operation considers them valid
    let registry = crate::tui::OperationRegistry::new();
    for key in ['s', 'r', 'R', 'W'] {
        if let Some(op) = registry.get_by_keybinding(key) {
            assert!(
                !op.is_valid_for("ZephyrTest"),
                "operation '{key}' must not apply to discovered kinds"
            );
        }
    }

    // Cleanup the process-global registry
    global().remove("ZephyrTest");
    assert!(
        crate::kube::get_gvk_for_resource_type("ZephyrTest").is_err(),
        "removal restores the unknown-kind error"
    );
}

#[test]
fn submenu_table_covers_every_popup() {
    use crate::tui::commands::submenu_ids;
    let table: Vec<&str> = super::submenus::SUBMENU_TABLE
        .iter()
        .map(|(id, _)| *id)
        .collect();
    for id in submenu_ids::ALL {
        assert!(table.contains(id), "popup {id} has no selection handler");
    }
    assert_eq!(
        table.len(),
        submenu_ids::ALL.len(),
        "no duplicate or unknown ids"
    );
}

#[test]
fn namespace_popup_selection_goes_through_the_table() {
    let mut app = create_test_app(false);
    app.update_namespace_hotkeys(vec!["apps".into()]);
    app.ui_state.command_buffer = "ns".to_string();
    app.execute_command();
    // all, flux-system, apps → select "apps"
    app.handle_key(make_key(KeyCode::Char('j')));
    app.handle_key(make_key(KeyCode::Char('j')));
    app.handle_key(make_key(KeyCode::Enter));
    assert!(app.view_state.submenu_state.is_none());
    assert_eq!(app.namespace().as_deref(), Some("apps"));
}

/// Apps in every footer context: each view, a native object, and kind lists
/// of kinds with different capabilities.
fn footer_contexts() -> Vec<(String, App)> {
    let mut contexts = Vec::new();
    for view in [
        View::ResourceList,
        View::ResourceFavorites,
        View::ResourceDetail,
        View::ResourceDescribe,
        View::ResourceYAML,
        View::ResourceTrace,
        View::ResourceGraph,
        View::ResourceHistory,
        View::ResourceEdit,
        View::HelmValues,
        View::EventList,
        View::Logs,
        View::WorkloadList,
        View::WorkloadDetail,
        View::InventoryList,
        View::Pulse,
        View::Help,
    ] {
        let mut app = create_test_app(false);
        app.view_state.current_view = view;
        contexts.push((format!("{view:?}"), app));
    }
    let mut native = create_test_app(false);
    native.view_state.current_view = View::ResourceYAML;
    native.selection_state.native_object = Some(ObjectRef::native("v1", "Pod", "a", "b"));
    contexts.push(("native YAML".into(), native));
    for (group, kind_name, plural) in [
        ("apps", "Deployment", "deployments"),
        ("", "Pod", "pods"),
        ("", "ConfigMap", "configmaps"),
    ] {
        let mut app = create_test_app(false);
        open_list(&mut app, group, kind_name, plural, &[("apps", "x")]);
        contexts.push((format!(":{plural}"), app));
    }
    contexts
}

#[test]
fn every_footer_hint_runs_the_action_it_advertises() {
    use super::actions::{FooterItem, action};
    for (context, app) in footer_contexts() {
        for item in app.footer_layout() {
            let id = match item {
                FooterItem::Action(id) | FooterItem::Relabel(id, _) => *id,
                FooterItem::Command(..) => continue,
            };
            let advertised = action(id);
            assert!(advertised.footer.is_some(), "{id:?} has no footer hint");
            if !(advertised.when)(&app) {
                continue; // filtered out of this footer
            }
            for key in advertised.keys {
                let event = KeyEvent::new(
                    key.code,
                    if key.ctrl {
                        crossterm::event::KeyModifiers::CONTROL
                    } else {
                        crossterm::event::KeyModifiers::NONE
                    },
                );
                let runs = app.action_for(&event).map(|a| a.id);
                assert_eq!(
                    runs,
                    Some(id),
                    "{context}: footer shows {id:?} for {:?} but the key runs {runs:?}",
                    key.code
                );
            }
        }
    }
}

#[test]
fn footers_hide_keys_that_do_nothing_in_the_view() {
    let mut app = create_test_app(false);
    app.view_state.current_view = View::ResourceList;
    let list = footer_keys(&app);
    assert!(list.contains(&("Enter", "Details")));
    assert!(list.contains(&("N/A/T/S", "Sort")));

    // YAML of a Flux resource: operations still apply, list-only keys don't.
    app.view_state.current_view = View::ResourceYAML;
    let yaml = footer_keys(&app);
    assert!(yaml.contains(&("s", "Suspend")));
    assert!(!yaml.contains(&("Enter", "Details")));
    assert!(!yaml.contains(&("N/A/T/S", "Sort")));
    assert!(!yaml.contains(&("f", "Favorite")));
}

#[test]
fn flux_list_footer_is_the_full_layout() {
    // The snapshot-tested hint list and the live footer agree in the list.
    let mut app = create_test_app(false);
    app.view_state.current_view = View::ResourceList;
    let live: Vec<_> = app
        .footer_commands()
        .into_iter()
        .map(|c| (c.key, c.label))
        .collect();
    let canonical: Vec<_> = crate::tui::keybindings::get_navigation_commands()
        .into_iter()
        .map(|c| (c.key, c.label))
        .collect();
    assert_eq!(live, canonical);
}

#[test]
fn footer_command_hints_are_real_commands() {
    use super::actions::{FooterItem, view_layout};
    for view in [View::KindList, View::ResourceList, View::Logs] {
        for item in view_layout(view) {
            if let FooterItem::Command(command, _) = item {
                let cmd = command.trim_start_matches(':');
                assert!(
                    super::command_mode::COMMAND_TABLE
                        .iter()
                        .any(|spec| (spec.matches)(cmd)),
                    "{command} isn't a table command"
                );
            }
        }
    }
}

#[test]
fn help_is_generated_from_the_tables() {
    use super::actions::{ACTIONS, HelpColumn, help_lines};
    let resource = help_lines(HelpColumn::Resource);
    for key in ["<s>", "<R>", "<y>", "<e>", "<Ctrl+d>", "<t>"] {
        assert!(
            resource.iter().any(|(k, _)| *k == key),
            "{key} missing from help"
        );
    }
    // Every action that has help lands in exactly one column.
    let total: usize = [
        HelpColumn::Resource,
        HelpColumn::General,
        HelpColumn::Navigation,
    ]
    .iter()
    .map(|c| help_lines(*c).len())
    .sum();
    assert_eq!(total, ACTIONS.iter().filter(|a| a.help.is_some()).count());

    let commands = super::command_mode::command_help();
    for spec in super::command_mode::COMMAND_TABLE {
        assert!(!spec.help.is_empty(), "a table command has no help");
        for line in spec.help {
            assert!(commands.contains(line));
        }
    }
    assert!(commands.contains(&(":q", "Quit application")));
}

#[test]
fn operation_keys_keep_the_status_line_they_set() {
    let mut app = create_test_app(true); // read-only: `s` reports why it can't
    add_resource(&mut app);
    app.view_state.current_view = View::ResourceList;
    app.handle_key(make_key(KeyCode::Char('s')));
    let (message, is_error) = app.ui_state.status_message.clone().unwrap();
    assert!(is_error, "{message}");
    // A second operation key doesn't clear it before setting its own.
    app.handle_key(make_key(KeyCode::Char('s')));
    assert!(app.ui_state.status_message.is_some());
    // A navigation key does.
    app.handle_key(make_key(KeyCode::Char('j')));
    assert!(app.ui_state.status_message.is_none());
}

#[test]
fn control_modified_letters_dont_trigger_plain_actions() {
    let mut app = create_test_app(false);
    add_resource(&mut app);
    app.view_state.current_view = View::ResourceList;
    app.handle_key(make_ctrl_key(KeyCode::Char('y')));
    assert_eq!(app.view_state.current_view, View::ResourceList);
    app.handle_key(make_key(KeyCode::Char('y')));
    assert_eq!(app.view_state.current_view, View::ResourceYAML);
}

#[test]
fn workload_list_watches_its_workloads_live() {
    use crate::kube::object_status::{ObjectHealth, ObjectStatus};
    let mut app = app_on_graph_with_workloads();
    app.handle_key(make_key(KeyCode::Enter)); // workload group → list
    assert_eq!(app.view_state.current_view, View::WorkloadList);
    let watched = app.async_state.inventory_status.pending().cloned().unwrap();
    assert_eq!(watched.len(), 2);
    assert_eq!(watched[0].kind, "Deployment");
    assert_eq!(watched[0].api_version, "apps/v1");

    // A live update replaces the graph's snapshot (e.g. after a restart).
    app.async_state.inventory_status.set_result(vec![
        ObjectStatus::new(ObjectHealth::InProgress, "Ready: 0/1"),
        ObjectStatus::new(ObjectHealth::Current, "Replicas: 1/1"),
    ]);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 8)).unwrap();
    terminal
        .draw(|f| {
            crate::tui::views::render_workload_list(
                f,
                f.area(),
                &app.view_state.workload_rows,
                app.async_state.inventory_status.result().map(Vec::as_slice),
                0,
                &mut 0,
                &app.theme,
            )
        })
        .unwrap();
    let out: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(out.contains("Ready: 0/1"), "{out}");

    // Back to the graph stops the watch.
    app.handle_key(make_key(KeyCode::Esc));
    assert!(app.async_state.inventory_status.key().is_none());
}

#[test]
fn workload_graph_lines_round_trip_and_stay_backward_compatible() {
    use crate::kube::workloads::WorkloadRef;
    let row = WorkloadRef {
        kind: "StatefulSet".into(),
        name: "db".into(),
        namespace: "data".into(),
        indicator: "●".into(),
        status: "Replicas: 1/1".into(),
        api_version: "apps/v1".into(),
    };
    assert_eq!(
        WorkloadRef::parse_graph_line(&row.to_graph_line()),
        Some(row)
    );
    let old = WorkloadRef::parse_graph_line("Deployment|web|apps|●|Replicas: 1/1").unwrap();
    assert_eq!(old.api_version, "apps/v1");
    assert_eq!(old.status, "Replicas: 1/1");
}

#[test]
fn help_overlay_scrolls_to_reach_everything() {
    let mut app = create_test_app(false);
    app.view_state.current_view = View::ResourceList;
    app.handle_key(make_key(KeyCode::Char('?')));
    assert!(app.ui_state.show_help);
    assert_eq!(app.ui_state.help_scroll, 0);

    // j/k scroll the overlay, not the list underneath.
    app.handle_key(make_key(KeyCode::Char('j')));
    app.handle_key(make_key(KeyCode::Char('j')));
    assert_eq!(app.ui_state.help_scroll, 2);
    assert_eq!(app.view_state.selected_index, 0);
    app.handle_key(make_key(KeyCode::Char('k')));
    assert_eq!(app.ui_state.help_scroll, 1);

    // G jumps to the end; rendering clamps it so the last command shows.
    app.handle_key(make_key(KeyCode::Char('G')));
    let render = |scroll: &mut usize| -> String {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(200, 15)).unwrap();
        terminal
            .draw(|f| crate::tui::views::render_help(f, f.area(), &Theme::default(), &[], scroll))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect()
    };
    let mut top = 0;
    assert!(
        !render(&mut top).contains(":q "),
        "too short to show it all"
    );
    let bottom = render(&mut app.ui_state.help_scroll);
    assert!(bottom.contains(":q "), "the last line is reachable");
    assert!(app.ui_state.help_scroll < usize::MAX, "clamped");

    // ? closes; reopening starts at the top.
    app.handle_key(make_key(KeyCode::Char('?')));
    assert!(!app.ui_state.show_help);
    app.handle_key(make_key(KeyCode::Char('?')));
    assert_eq!(app.ui_state.help_scroll, 0);
    app.handle_key(make_key(KeyCode::Esc));
    assert!(!app.ui_state.show_help);
}

/// A kind list of a patchable kind (as discovery would build it).
fn open_editable_list(app: &mut App, group: &str, kind_name: &str, plural: &str) {
    app.view_state.current_view = View::ResourceList;
    app.open_kind_list(kind(group, kind_name, plural).with_patch_verb(true));
    with_rows(app, &[("apps", "web")]);
}

fn preflight(manager: crate::kube::ownership::ManagerLookup) -> super::state::EditPreflight {
    super::state::EditPreflight {
        object: serde_json::json!({"apiVersion": "v1", "kind": "ConfigMap",
            "metadata": {"name": "web", "namespace": "apps"}}),
        manager,
        manager_suspended: None,
    }
}

fn flux_ks(name: &str) -> crate::kube::ownership::Manager {
    crate::kube::ownership::Manager::Flux {
        kind: "Kustomization".into(),
        namespace: "flux-system".into(),
        name: name.into(),
    }
}

#[test]
fn e_on_a_patchable_native_kind_checks_ownership_before_editing() {
    let mut app = create_test_app(false);
    open_editable_list(&mut app, "", "ConfigMap", "configmaps");
    assert!(footer_keys(&app).contains(&("e", "Edit")));

    app.handle_key(make_key(KeyCode::Char('e')));
    assert_eq!(app.view_state.current_view, View::ResourceEdit);
    let target = app.async_state.edit_preflight.pending().cloned().unwrap();
    assert_eq!(target, ObjectRef::native("v1", "ConfigMap", "apps", "web"));
    assert_eq!(app.async_state.edit_pending.as_ref(), Some(&target));

    // Unmanaged: straight to the editor.
    let _ = app.async_state.edit_preflight.dispatch();
    app.on_edit_preflight(preflight(crate::kube::ownership::ManagerLookup::Unmanaged));
    assert!(app.async_state.confirmation_pending.is_none());
    assert!(app.async_state.edit_full_yaml.is_some());
}

#[test]
fn editing_a_flux_managed_object_warns_and_respects_the_answer() {
    use crate::kube::ownership::ManagerLookup;
    let mut app = create_test_app(false);
    add_resource(&mut app); // Kustomization flux-system/my-kustomization, not suspended
    open_editable_list(&mut app, "", "ConfigMap", "configmaps");
    app.handle_key(make_key(KeyCode::Char('e')));
    let _ = app.async_state.edit_preflight.dispatch();
    app.on_edit_preflight(preflight(ManagerLookup::Managed(flux_ks(
        "my-kustomization",
    ))));

    let message = app.confirmation_message().unwrap();
    assert!(
        message.contains("managed by Flux Kustomization flux-system/my-kustomization"),
        "{message}"
    );
    assert!(message.contains("next reconcile will revert"), "{message}");
    assert!(
        app.async_state.edit_full_yaml.is_none(),
        "editor waits for y"
    );

    // n backs out of the edit entirely.
    app.handle_key(make_key(KeyCode::Char('n')));
    assert_eq!(app.view_state.current_view, View::KindList);
    assert!(app.async_state.edit_pending.is_none());

    // y opens the editor.
    app.handle_key(make_key(KeyCode::Char('e')));
    let _ = app.async_state.edit_preflight.dispatch();
    app.on_edit_preflight(preflight(ManagerLookup::Managed(flux_ks(
        "my-kustomization",
    ))));
    app.handle_key(make_key(KeyCode::Char('y')));
    assert!(app.async_state.edit_full_yaml.is_some());
    assert_eq!(app.view_state.current_view, View::ResourceEdit);
}

#[test]
fn a_suspended_flux_manager_gets_its_own_warning() {
    use crate::kube::ownership::{ManagerLookup, OwnerRef, Resolved};
    let mut app = create_test_app(false);
    add_resource(&mut app);
    let key = "Kustomization:flux-system:my-kustomization";
    let mut suspended = app.state.get(key).unwrap();
    suspended.suspended = Some(true);
    app.state.upsert(key.to_string(), suspended);
    let target = ObjectRef::native("v1", "Pod", "apps", "web-abc");

    // Through an owner chain: the pod's Deployment is Flux-managed.
    let owned = ManagerLookup::Owned {
        owner: OwnerRef {
            api_version: "apps/v1".into(),
            kind: "ReplicaSet".into(),
            name: "web-7b5".into(),
            uid: "rs-1".into(),
        },
        resolved: Some(Resolved::Manager(flux_ks("my-kustomization"))),
        timed_out: false,
    };
    let message = app.edit_warning(&target, &owned, None).unwrap();
    assert!(message.contains("(via ReplicaSet/web-7b5)"), "{message}");
    assert!(message.contains("which is suspended"), "{message}");
    assert!(
        message.contains("stays until Kustomization my-kustomization is resumed"),
        "{message}"
    );

    // A Flux manager outside the watched namespaces: can't know.
    let unknown = app
        .edit_warning(&target, &ManagerLookup::Managed(flux_ks("elsewhere")), None)
        .unwrap();
    assert!(unknown.contains("unless it's suspended"), "{unknown}");

    // Other managers.
    let helm = app
        .edit_warning(
            &target,
            &ManagerLookup::Managed(crate::kube::ownership::Manager::Helm {
                namespace: "apps".into(),
                release: "web".into(),
            }),
            None,
        )
        .unwrap();
    assert!(helm.contains("helm upgrade"), "{helm}");
    let top_owner = app
        .edit_warning(
            &target,
            &ManagerLookup::Owned {
                owner: OwnerRef {
                    api_version: "apps/v1".into(),
                    kind: "ReplicaSet".into(),
                    name: "web-7b5".into(),
                    uid: "rs-1".into(),
                },
                resolved: Some(Resolved::Owner {
                    kind: "Deployment".into(),
                    name: "web".into(),
                }),
                timed_out: false,
            },
            None,
        )
        .unwrap();
    assert!(top_owner.contains("owned by Deployment/web"), "{top_owner}");
    assert_eq!(
        app.edit_warning(&target, &ManagerLookup::Unmanaged, None),
        None
    );
}

#[test]
fn secrets_and_read_only_never_reach_the_editor() {
    let mut app = create_test_app(false);
    open_editable_list(&mut app, "", "Secret", "secrets");
    assert!(!footer_keys(&app).contains(&("e", "Edit")), "no Edit hint");
    app.handle_key(make_key(KeyCode::Char('e')));
    assert_eq!(app.view_state.current_view, View::KindList);
    let (message, _) = app.ui_state.status_message.clone().unwrap();
    assert!(message.contains("Secrets can't be edited"), "{message}");

    let mut app = create_test_app(true);
    open_editable_list(&mut app, "", "ConfigMap", "configmaps");
    app.handle_key(make_key(KeyCode::Char('e')));
    assert_eq!(app.view_state.current_view, View::KindList);
    assert!(app.async_state.edit_preflight.pending().is_none());
}

#[test]
fn a_late_preflight_after_leaving_the_edit_is_ignored() {
    let mut app = create_test_app(false);
    open_editable_list(&mut app, "", "ConfigMap", "configmaps");
    app.handle_key(make_key(KeyCode::Char('e')));
    app.handle_key(make_key(KeyCode::Esc)); // leave before it lands
    app.on_edit_preflight(preflight(crate::kube::ownership::ManagerLookup::Managed(
        flux_ks("x"),
    )));
    assert!(app.async_state.confirmation_pending.is_none());
    assert_eq!(app.view_state.current_view, View::KindList);
}

#[test]
fn annotate_actions_build_a_metadata_patch() {
    use super::actions::{ActionSpec, Write};
    let spec = ActionSpec::Annotate {
        key: "example.com/reconcile-at",
        value: "{{now}}",
        write: Write {
            verb: "Reconcile",
            done: "Requested reconcile of",
            detail: "",
            kinds: &[],
        },
    };
    let patch = spec
        .patch_document("2026-10-12T00:00:00Z")
        .unwrap()
        .unwrap();
    assert_eq!(
        patch,
        serde_json::json!({"metadata": {"annotations": {"example.com/reconcile-at": "2026-10-12T00:00:00Z"}}})
    );
    let bad = ActionSpec::Patch {
        patch: "{not json",
        write: Write {
            verb: "X",
            done: "X",
            detail: "",
            kinds: &[],
        },
    };
    assert!(bad.patch_document("now").unwrap().is_err());
}

#[test]
fn restart_on_a_cronjob_explains_it_does_not_apply() {
    use crate::kube::workloads::WorkloadRef;
    let mut app = create_test_app(false);
    app.view_state.workload_rows = vec![WorkloadRef {
        kind: "CronJob".into(),
        name: "nightly".into(),
        namespace: "apps".into(),
        indicator: "●".into(),
        status: String::new(),
        api_version: "batch/v1".into(),
    }];
    app.view_state.current_view = View::WorkloadList;
    app.handle_key(make_key(KeyCode::Char('r')));
    assert!(app.async_state.confirmation_pending.is_none());
    let (message, is_error) = app.ui_state.status_message.clone().unwrap();
    assert!(
        is_error && message.contains("CronJob does not support restart"),
        "{message}"
    );
}

#[test]
fn a_suspended_manager_outside_the_watched_namespaces_is_still_recognised() {
    use crate::kube::ownership::ManagerLookup;
    // flux9s scoped to the app's namespace: the Kustomization isn't watched,
    // but the pre-check read it from the cluster.
    let app = create_test_app(false);
    let target = ObjectRef::native("apps/v1", "Deployment", "cabot-cup", "cabot-cup");
    let lookup = ManagerLookup::Managed(flux_ks("apps"));
    let suspended = app.edit_warning(&target, &lookup, Some(true)).unwrap();
    assert!(suspended.contains("which is suspended"), "{suspended}");
    let active = app.edit_warning(&target, &lookup, Some(false)).unwrap();
    assert!(
        active.contains(" — its next reconcile will revert"),
        "{active}"
    );
    let unknown = app.edit_warning(&target, &lookup, None).unwrap();
    assert!(unknown.contains("unless it's suspended"), "{unknown}");
}

#[test]
fn the_watched_state_wins_over_the_fetched_suspend_flag() {
    use crate::kube::ownership::ManagerLookup;
    let mut app = create_test_app(false);
    add_resource(&mut app); // watched, not suspended
    let target = ObjectRef::native("v1", "ConfigMap", "apps", "web");
    let message = app
        .edit_warning(
            &target,
            &ManagerLookup::Managed(flux_ks("my-kustomization")),
            Some(true),
        )
        .unwrap();
    assert!(!message.contains("suspended"), "{message}");
}
