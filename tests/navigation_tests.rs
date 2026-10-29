//! Tests for navigation flow consistency
//!
//! Ensures that navigation between views is consistent and users never get stuck.
//! Tests that Esc always returns to the correct list view, regardless of navigation path.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use flux9s::tui::App;
use flux9s::tui::app::state::View;
use flux9s::watcher::{ResourceInfo, ResourceState};
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

fn create_test_app() -> App {
    let state = ResourceState::new();
    let config = flux9s::config::Config::default();
    let theme = flux9s::tui::Theme::default();
    App::new(state, "test-context".to_string(), None, config, theme)
}

/// Creates a test app with the splash screen disabled.
///
/// Required for tests that call `handle_key` directly: the default app starts
/// with `show_splash = true`, so the first key press would be consumed by the
/// splash dismissal logic rather than reaching the handler under test.
fn create_test_app_no_splash() -> App {
    let state = ResourceState::new();
    let mut config = flux9s::config::Config::default();
    config.ui.splashless = true;
    let theme = flux9s::tui::Theme::default();
    App::new(state, "test-context".to_string(), None, config, theme)
}

fn add_test_resource(app: &mut App) {
    let resource = ResourceInfo {
        api_group: None,
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
    app.state().upsert(
        flux9s::ResourceKey::new(&resource.resource_type, &resource.namespace, &resource.name)
            .to_key_string(),
        resource,
    );
}

#[test]
fn test_navigation_from_list_to_trace_to_graph_to_esc() {
    // List -> Trace -> Graph -> Esc returns to the list: sibling detail
    // views replace each other instead of stacking.
    let mut app = create_test_app_no_splash();
    app.set_view(View::ResourceList);

    app.set_view_trace();
    assert_eq!(app.back_target(), Some(View::ResourceList));
    app.set_view_graph();
    assert_eq!(
        app.back_target(),
        Some(View::ResourceList),
        "trace -> graph doesn't stack"
    );

    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(app.current_view(), View::ResourceList);
    assert_eq!(app.back_target(), None);
}

#[test]
fn test_navigation_from_list_to_yaml_to_trace_to_esc() {
    let mut app = create_test_app_no_splash();
    add_test_resource(&mut app);
    app.set_view(View::ResourceList);

    app.handle_key(make_key(KeyCode::Char('y')));
    assert_eq!(app.current_view(), View::ResourceYAML);
    app.set_view_trace();

    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(app.current_view(), View::ResourceList);
}

#[test]
fn test_navigation_from_favorites_to_trace_to_graph_to_esc() {
    let mut app = create_test_app_no_splash();
    app.set_view(View::ResourceFavorites);

    app.set_view_trace();
    app.set_view_graph();

    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(
        app.current_view(),
        View::ResourceFavorites,
        "Back returns to the favorites it was opened from"
    );
}

#[test]
fn test_set_view_does_not_touch_back_history() {
    let mut app = create_test_app();
    app.set_view(View::ResourceList);
    app.set_view(View::ResourceDetail);
    app.set_view(View::ResourceTrace);
    app.set_view(View::ResourceGraph);
    assert_eq!(app.back_target(), None, "set_view is a raw setter");
}

#[test]
fn test_navigation_from_list_to_detail_to_yaml_to_esc() {
    let mut app = create_test_app_no_splash();
    add_test_resource(&mut app);
    app.set_view(View::ResourceList);

    app.handle_key(make_key(KeyCode::Enter));
    assert_eq!(app.current_view(), View::ResourceDetail);
    app.handle_key(make_key(KeyCode::Char('y')));
    assert_eq!(app.current_view(), View::ResourceYAML);

    app.handle_key(make_key(KeyCode::Esc));
    assert_eq!(
        app.current_view(),
        View::ResourceList,
        "detail -> YAML doesn't stack"
    );
}

#[test]
fn test_d_from_list_opens_describe_and_esc_returns_to_list() {
    let mut app = create_test_app_no_splash();
    add_test_resource(&mut app);
    app.set_view(View::ResourceList);

    let result = app.handle_key(make_key(KeyCode::Char('d')));

    assert_eq!(result, None);
    assert_eq!(app.current_view(), View::ResourceDescribe);

    let result = app.handle_key(make_key(KeyCode::Esc));

    assert_eq!(result, None);
    assert_eq!(app.current_view(), View::ResourceList);
}

// ── q / Esc / quit-confirm key handling ──────────────────────────────────────

#[test]
fn test_q_at_top_level_shows_quit_confirm() {
    let mut app = create_test_app_no_splash();
    app.set_view(View::ResourceList);

    let result = app.handle_key(make_key(KeyCode::Char('q')));

    assert_eq!(result, None, "q at top level should not quit the app");
    assert!(
        app.show_quit_confirm(),
        "q at top level should open the quit confirm dialog"
    );
}

#[test]
fn test_esc_at_top_level_shows_quit_confirm() {
    let mut app = create_test_app_no_splash();
    app.set_view(View::ResourceList);

    let result = app.handle_key(make_key(KeyCode::Esc));

    assert_eq!(result, None, "Esc at top level should not quit the app");
    assert!(
        app.show_quit_confirm(),
        "Esc at top level should open the quit confirm dialog"
    );
}

#[test]
fn test_q_at_nested_view_navigates_back() {
    let mut app = create_test_app_no_splash();
    app.set_view(View::ResourceDetail);

    let result = app.handle_key(make_key(KeyCode::Char('q')));

    assert_eq!(result, None, "q at a nested view should not quit");
    assert_eq!(
        app.current_view(),
        View::ResourceList,
        "q should navigate back to the list"
    );
    assert!(
        !app.show_quit_confirm(),
        "quit confirm should not appear when navigating back"
    );
}

#[test]
fn test_quit_confirm_y_confirms_quit() {
    let mut app = create_test_app_no_splash();
    app.set_view(View::ResourceList);
    app.handle_key(make_key(KeyCode::Char('q'))); // open dialog

    let result = app.handle_key(make_key(KeyCode::Char('y')));

    assert_eq!(result, Some(true), "y in the quit dialog should quit");
}

#[test]
fn test_quit_confirm_n_cancels() {
    let mut app = create_test_app_no_splash();
    app.set_view(View::ResourceList);
    app.handle_key(make_key(KeyCode::Char('q'))); // open dialog

    let result = app.handle_key(make_key(KeyCode::Char('n')));

    assert_eq!(result, None, "n in the quit dialog should not quit");
    assert!(
        !app.show_quit_confirm(),
        "n should dismiss the quit confirm dialog"
    );
}

#[test]
fn test_quit_confirm_q_cancels() {
    let mut app = create_test_app_no_splash();
    app.set_view(View::ResourceList);
    app.handle_key(make_key(KeyCode::Char('q'))); // open dialog

    let result = app.handle_key(make_key(KeyCode::Char('q')));

    assert_eq!(result, None, "q inside the dialog should cancel, not quit");
    assert!(
        !app.show_quit_confirm(),
        "q inside the dialog should dismiss it"
    );
}

#[test]
fn test_quit_confirm_esc_cancels() {
    let mut app = create_test_app_no_splash();
    app.set_view(View::ResourceList);
    app.handle_key(make_key(KeyCode::Char('q'))); // open dialog

    let result = app.handle_key(make_key(KeyCode::Esc));

    assert_eq!(result, None, "Esc inside the dialog should not quit");
    assert!(
        !app.show_quit_confirm(),
        "Esc inside the dialog should dismiss it"
    );
}

#[test]
fn test_uppercase_q_quits_immediately() {
    let mut app = create_test_app_no_splash();
    app.set_view(View::ResourceList);

    let result = app.handle_key(make_key(KeyCode::Char('Q')));

    assert_eq!(
        result,
        Some(true),
        "Q should quit immediately without a dialog"
    );
    assert!(
        !app.show_quit_confirm(),
        "Q should not open the quit confirm dialog"
    );
}

#[test]
fn test_ctrl_c_quits_immediately() {
    let mut app = create_test_app_no_splash();
    app.set_view(View::ResourceList);

    let result = app.handle_key(make_ctrl_key(KeyCode::Char('c')));

    assert_eq!(result, Some(true), "Ctrl+C should quit immediately");
}

// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_backspace_mirrors_esc_without_the_quit_prompt() {
    let mut app = create_test_app_no_splash();
    add_test_resource(&mut app);
    app.set_view(View::ResourceList);

    app.handle_key(make_key(KeyCode::Enter));
    app.handle_key(make_key(KeyCode::Backspace));
    assert_eq!(app.current_view(), View::ResourceList);

    // At the top Backspace does nothing (Esc would ask to quit).
    app.handle_key(make_key(KeyCode::Backspace));
    assert_eq!(app.current_view(), View::ResourceList);
}
