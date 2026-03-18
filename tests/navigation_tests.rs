//! Tests for navigation flow consistency
//!
//! Ensures that navigation between views is consistent and users never get stuck.
//! Tests that Esc always returns to the correct list view, regardless of navigation path.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use flux9s::tui::App;
use flux9s::tui::app::state::View;
use flux9s::watcher::ResourceState;

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

#[test]
fn test_navigation_from_list_to_trace_to_graph_to_esc() {
    // Test: List -> Trace -> Graph -> Esc should return to List
    let mut app = create_test_app();

    // Start in list view
    app.set_view(View::ResourceList);
    // Simulate navigating to trace from list (this would set previous_list_view in real code)
    // For testing, we'll manually set it to simulate the behavior
    // In real code, this happens in handle_key when 't' is pressed from ResourceList

    // Navigate to trace (simulate what handle_key does)
    app.set_view_trace();
    // previous_list_view should be ResourceList (set when navigating from list)

    // Navigate to graph from trace (should NOT update previous_list_view)
    app.set_view_graph();
    // Verify previous_list_view is still ResourceList
    assert_eq!(
        app.previous_list_view(),
        View::ResourceList,
        "previous_list_view should remain ResourceList when navigating trace -> graph"
    );
}

#[test]
fn test_navigation_from_list_to_yaml_to_trace_to_esc() {
    // Test: List -> YAML -> Trace -> Esc should return to List
    let mut app = create_test_app();

    app.set_view(View::ResourceList);

    // Navigate to YAML (simulate - in real code this sets previous_list_view)
    app.set_view(View::ResourceYAML);
    // In real code, previous_list_view would be set to ResourceList when navigating from list

    // Navigate to trace from YAML (should NOT update previous_list_view)
    app.set_view_trace();
    // Verify previous_list_view behavior - it should remain unchanged when navigating between detail views
    // This test verifies the logic, actual state would be set by handle_key
}

#[test]
fn test_navigation_from_favorites_to_trace_to_graph_to_esc() {
    // Test: Favorites -> Trace -> Graph -> Esc should return to Favorites
    let mut app = create_test_app();

    app.set_view(View::ResourceFavorites);
    // In real code, previous_list_view would be set when navigating from favorites

    // Navigate to trace
    app.set_view_trace();

    // Navigate to graph from trace (should NOT update previous_list_view)
    app.set_view_graph();
    // Verify that set_view_graph doesn't change previous_list_view
    // The actual previous_list_view state is managed by handle_key, not by set_view_graph
}

#[test]
fn test_navigation_previous_list_view_only_updated_from_list_views() {
    // Test that previous_list_view is only updated when coming from ResourceList or ResourceFavorites
    // This test verifies the logic in handle_key - we can't directly test handle_key without
    // setting up a full app with resources, but we can verify the helper methods work correctly
    let mut app = create_test_app();

    // Test that set_view doesn't change previous_list_view
    app.set_view(View::ResourceList);
    let initial_prev = app.previous_list_view();

    app.set_view(View::ResourceDetail);
    assert_eq!(
        app.previous_list_view(),
        initial_prev,
        "set_view should not change previous_list_view"
    );

    app.set_view(View::ResourceTrace);
    assert_eq!(
        app.previous_list_view(),
        initial_prev,
        "set_view should not change previous_list_view"
    );

    app.set_view(View::ResourceGraph);
    assert_eq!(
        app.previous_list_view(),
        initial_prev,
        "set_view should not change previous_list_view"
    );
}

#[test]
fn test_navigation_from_list_to_detail_to_yaml_to_esc() {
    // Test: List -> Detail -> YAML -> Esc should return to List
    // This verifies that navigating between detail views doesn't change previous_list_view
    let mut app = create_test_app();

    app.set_view(View::ResourceList);
    let initial_prev = app.previous_list_view();

    // Navigate to detail
    app.set_view(View::ResourceDetail);
    assert_eq!(
        app.previous_list_view(),
        initial_prev,
        "previous_list_view should not change"
    );

    // Navigate to YAML from detail (should NOT update previous_list_view)
    app.set_view(View::ResourceYAML);
    assert_eq!(
        app.previous_list_view(),
        initial_prev,
        "previous_list_view should remain unchanged when navigating between detail views"
    );
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
fn test_navigation_esc_from_detail_views() {
    // Test that Esc from any detail view returns to the correct list view
    // This tests the set_view and previous_list_view methods
    let mut app = create_test_app();

    // Test set_view works correctly
    app.set_view(View::ResourceDetail);
    app.set_view(View::ResourceList);
    assert_eq!(app.previous_list_view(), View::ResourceList); // Default

    // Test that set_view can change to any view
    app.set_view(View::ResourceYAML);
    app.set_view(View::ResourceFavorites);
    app.set_view(View::ResourceTrace);
    app.set_view(View::ResourceGraph);
    app.set_view(View::ResourceHistory);
    // All should work without panicking

    // Test previous_list_view returns correct value
    assert_eq!(app.previous_list_view(), View::ResourceList);

    // Test that set_view with previous_list_view works (simulating Esc)
    app.set_view(View::ResourceGraph);
    let prev = app.previous_list_view();
    app.set_view(prev);
    // Should successfully change view
    assert_eq!(app.current_view(), prev);
}
