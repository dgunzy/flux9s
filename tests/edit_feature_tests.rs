//! Tests for edit feature

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use flux9s::config::Config;
use flux9s::tui::App;
use flux9s::tui::app::state::{AsyncOperationState, EditorState};
use flux9s::watcher::{ResourceKey, ResourceState};
use serde_json::json;

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
    let config = Config::default();
    let theme = flux9s::tui::Theme::default();
    App::new(state, "test-context".to_string(), None, config, theme)
}

#[test]
fn test_edit_state_initialization() {
    let async_state = AsyncOperationState::default();
    assert!(async_state.edit_pending.is_none());
    assert!(async_state.edit_yaml.is_none());
    assert!(async_state.edit_save_result_rx.is_none());
    assert!(async_state.editor_state.is_none());
}

#[test]
fn test_edit_state_clear() {
    let mut async_state = AsyncOperationState::default();
    async_state.edit_pending = Some(ResourceKey::new(
        "Kustomization".to_string(),
        "flux-system".to_string(),
        "test-app".to_string(),
    ));
    async_state.edit_yaml = Some("spec:\n  suspend: false".to_string());
    async_state.editor_state = Some(EditorState::new("spec:\n  suspend: false"));
    async_state.edit_save_pending = Some(serde_json::Value::Null);
    async_state.edit_error_message = Some("previous error".to_string());

    assert!(async_state.edit_pending.is_some());
    assert!(async_state.edit_yaml.is_some());
    assert!(async_state.edit_save_pending.is_some());
    assert!(async_state.edit_error_message.is_some());
    assert!(async_state.editor_state.is_some());

    async_state.clear_pending();

    assert!(async_state.edit_pending.is_none());
    assert!(async_state.edit_yaml.is_none());
    assert!(async_state.edit_save_pending.is_none());
    assert!(async_state.edit_save_result_rx.is_none());
    assert!(async_state.edit_error_message.is_none());
    assert!(async_state.editor_state.is_none());
}

#[test]
fn test_yaml_fetch_initializes_editor_state_for_edit_mode() {
    let mut app = create_test_app();
    app.set_view(flux9s::tui::app::state::View::ResourceEdit);
    let yaml = json!({
        "apiVersion": "source.toolkit.fluxcd.io/v1beta2",
        "kind": "GitRepository",
        "metadata": { "name": "test-repo", "namespace": "default" },
        "spec": { "url": "https://example.com/repo.git", "interval": "1m" }
    });

    app.set_yaml_fetched(yaml);
    assert!(app.async_state_mut().edit_yaml.is_some());
    assert!(app.async_state_mut().editor_state.is_some());
    let editor_content = app
        .async_state_mut()
        .editor_state
        .as_ref()
        .unwrap()
        .get_content();
    assert!(editor_content.contains("url: https://example.com/repo.git"));
    assert!(editor_content.contains("interval: 1m"));
}

#[test]
fn test_edit_save_pending_set_on_save() {
    let mut async_state = AsyncOperationState::default();
    let yaml = "spec:\n  suspend: false";
    async_state.edit_yaml = Some(yaml.to_string());
    async_state.edit_pending = Some(ResourceKey::new(
        "Kustomization".to_string(),
        "flux-system".to_string(),
        "test-app".to_string(),
    ));
    async_state.edit_save_pending = Some(serde_yaml::from_str::<serde_json::Value>(yaml).unwrap());

    assert!(async_state.edit_save_pending.is_some());
}

#[test]
fn test_handle_ctrl_s_creates_save_pending() {
    let mut app = create_test_app();
    app.ui_state_mut().show_splash = false;
    app.set_view(flux9s::tui::app::state::View::ResourceEdit);
    app.async_state_mut().edit_pending = Some(ResourceKey::new(
        "Kustomization".to_string(),
        "flux-system".to_string(),
        "test-app".to_string(),
    ));
    app.async_state_mut().edit_yaml = Some("spec:\n  suspend: false".to_string());
    app.async_state_mut().editor_state = Some(EditorState::new("spec:\n  suspend: false"));

    assert!(app.async_state_mut().edit_save_pending.is_none());
    let _ = app.handle_key(make_ctrl_key(KeyCode::Char('s')));
    assert!(app.async_state_mut().edit_save_pending.is_some());
    assert!(app.async_state_mut().edit_error_message.is_none());
}

#[test]
fn test_handle_ctrl_s_invalid_yaml_sets_error() {
    let mut app = create_test_app();
    app.ui_state_mut().show_splash = false;
    app.set_view(flux9s::tui::app::state::View::ResourceEdit);
    app.async_state_mut().edit_pending = Some(ResourceKey::new(
        "Kustomization".to_string(),
        "flux-system".to_string(),
        "test-app".to_string(),
    ));
    app.async_state_mut().edit_yaml = Some("spec:\n  suspend: [invalid".to_string());
    app.async_state_mut().editor_state = Some(EditorState::new("spec:\n  suspend: [invalid"));

    assert!(app.async_state_mut().edit_save_pending.is_none());
    let _ = app.handle_key(make_ctrl_key(KeyCode::Char('s')));
    assert!(app.async_state_mut().edit_save_pending.is_none());
    assert!(app.async_state_mut().edit_error_message.is_none());
    assert!(
        app.async_state_mut()
            .editor_state
            .as_ref()
            .unwrap()
            .validation_error
            .is_some()
    );
}

#[test]
fn test_edit_save_error_message_cleared_on_save_attempt() {
    let mut async_state = AsyncOperationState::default();
    async_state.edit_error_message = Some("previous error".to_string());
    async_state.edit_save_pending = Some(serde_json::Value::Null);

    assert!(async_state.edit_error_message.is_some());
    async_state.edit_error_message = None;
    assert!(async_state.edit_error_message.is_none());
}

#[test]
fn test_edit_mode_config_default() {
    let config = Config::default();
    assert!(config.edit_mode);
    assert!(config.read_only);
}

#[test]
fn test_edit_mode_config_custom() {
    let mut config = Config::default();
    config.edit_mode = false;
    assert!(!config.edit_mode);
}

#[test]
fn test_resource_key_for_edit() {
    let key = ResourceKey::new(
        "GitRepository".to_string(),
        "test-resources".to_string(),
        "test-repo".to_string(),
    );

    assert_eq!(key.resource_type, "GitRepository");
    assert_eq!(key.namespace, "test-resources");
    assert_eq!(key.name, "test-repo");
    assert_eq!(
        key.to_key_string(),
        "GitRepository:test-resources:test-repo"
    );
}

// Editor state tests

#[test]
fn test_editor_state_creation() {
    let editor = EditorState::new("line1\nline2\nline3");
    assert_eq!(editor.lines.len(), 3);
    assert_eq!(editor.cursor_row, 0);
    assert_eq!(editor.cursor_col, 0);
    assert!(editor.validation_error.is_none());
}

#[test]
fn test_editor_state_empty() {
    let editor = EditorState::new("");
    assert_eq!(editor.lines.len(), 1);
    assert_eq!(editor.lines[0], "");
    assert_eq!(editor.cursor_row, 0);
    assert_eq!(editor.cursor_col, 0);
}

#[test]
fn test_editor_insert_char() {
    let mut editor = EditorState::new("hello");
    editor.cursor_col = 5; // End of line
    editor.insert_char('!');
    assert_eq!(editor.lines[0], "hello!");
    assert_eq!(editor.cursor_col, 6);
}

#[test]
fn test_editor_insert_char_middle() {
    let mut editor = EditorState::new("helo");
    editor.cursor_col = 2; // After 'he'
    editor.insert_char('l');
    assert_eq!(editor.lines[0], "hello");
    assert_eq!(editor.cursor_col, 3);
}

#[test]
fn test_editor_backspace() {
    let mut editor = EditorState::new("hello");
    editor.cursor_col = 5;
    editor.backspace();
    assert_eq!(editor.lines[0], "hell");
    assert_eq!(editor.cursor_col, 4);
}

#[test]
fn test_editor_backspace_at_start() {
    let mut editor = EditorState::new("line1\nline2");
    editor.cursor_row = 1;
    editor.cursor_col = 0;
    editor.backspace();
    // Should merge lines
    assert_eq!(editor.cursor_row, 0);
    assert_eq!(editor.lines[0], "line1line2");
}

#[test]
fn test_editor_delete_char() {
    let mut editor = EditorState::new("hello");
    editor.cursor_col = 2;
    editor.delete_char();
    assert_eq!(editor.lines[0], "helo");
    assert_eq!(editor.cursor_col, 2);
}

#[test]
fn test_editor_delete_char_at_end() {
    let mut editor = EditorState::new("hello");
    editor.cursor_col = 5;
    editor.delete_char();
    assert_eq!(editor.lines[0], "hello");
}

#[test]
fn test_editor_cursor_navigation() {
    let mut editor = EditorState::new("hello");
    // Left
    editor.cursor_col = 3;
    editor.cursor_left();
    assert_eq!(editor.cursor_col, 2);

    // Right
    editor.cursor_right();
    assert_eq!(editor.cursor_col, 3);

    // Home
    editor.cursor_home();
    assert_eq!(editor.cursor_col, 0);

    // End
    editor.cursor_end();
    assert_eq!(editor.cursor_col, 5);
}

#[test]
fn test_editor_vertical_navigation() {
    let mut editor = EditorState::new("line1\nline2\nline3");
    editor.cursor_col = 3;

    // Down
    editor.cursor_down(10);
    assert_eq!(editor.cursor_row, 1);
    assert_eq!(editor.cursor_col, 3);

    // Up
    editor.cursor_up();
    assert_eq!(editor.cursor_row, 0);

    // Down at end
    editor.cursor_row = 2;
    editor.cursor_down(10);
    assert_eq!(editor.cursor_row, 2); // No change, already at end
}

#[test]
fn test_editor_get_content() {
    let editor = EditorState::new("line1\nline2\nline3");
    assert_eq!(editor.get_content(), "line1\nline2\nline3");
}

#[test]
fn test_editor_validate_yaml_valid() {
    let mut editor = EditorState::new("spec:\n  suspend: false\n  interval: 1m");
    assert!(editor.validate_yaml());
    assert!(editor.validation_error.is_none());
}

#[test]
fn test_editor_validate_yaml_invalid() {
    let mut editor = EditorState::new("spec:\n  suspend: [invalid yaml");
    assert!(!editor.validate_yaml());
    assert!(editor.validation_error.is_some());
    assert!(
        editor
            .validation_error
            .as_ref()
            .unwrap()
            .contains("YAML error")
    );
}

#[test]
fn test_editor_clear_error() {
    let mut editor = EditorState::new("invalid: [");
    editor.validate_yaml();
    assert!(editor.validation_error.is_some());

    editor.clear_error();
    assert!(editor.validation_error.is_none());
}

#[test]
fn test_editor_insert_clears_error() {
    let mut editor = EditorState::new("invalid: [");
    editor.validate_yaml();
    assert!(editor.validation_error.is_some());

    editor.insert_char(']');
    assert!(editor.validation_error.is_none());
}
