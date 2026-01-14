//! Snapshot tests for TUI rendering
//!
//! These tests use insta to capture and compare the rendered output of the TUI views.
//! Run `cargo insta review` to review and accept snapshot changes.

use flux9s::config::{Config, LoggerConfig, UiConfig};
use flux9s::tui::Theme;
use flux9s::tui::app::state::ControllerPodState;
use flux9s::tui::views::{
    render_footer, render_header, render_resource_detail, render_resource_list,
    render_resource_yaml,
};
use flux9s::watcher::{ResourceInfo, ResourceState};
use insta::assert_snapshot;
use ratatui::{Terminal, backend::TestBackend};
use std::collections::{HashMap, HashSet};

/// Create a test theme
fn create_test_theme() -> Theme {
    Theme::default()
}

/// Create a test config
fn create_test_config() -> Config {
    Config {
        read_only: false,
        default_namespace: "".to_string(),
        namespace_hotkeys: vec![],
        ui: UiConfig {
            enable_mouse: false,
            headless: false,
            no_icons: false,
            skin: "default".to_string(),
            skin_read_only: None,
            splashless: true, // Skip splash for tests
        },
        logger: LoggerConfig {
            tail: 100,
            buffer: 1000,
            since_seconds: 3600,
            text_wrap: false,
        },
        context_skins: HashMap::new(),
        cluster: HashMap::new(),
        favorites: vec![],
    }
}

/// Create test resource state with sample data
fn create_test_resource_state() -> ResourceState {
    let state = ResourceState::new();

    // Add some test resources
    let resources = vec![
        ResourceInfo {
            name: "my-kustomization".to_string(),
            namespace: "flux-system".to_string(),
            resource_type: "Kustomization".to_string(),
            age: Some(chrono::Utc::now() - chrono::Duration::hours(2)),
            suspended: Some(false),
            ready: Some(true),
            message: Some("Applied revision: main@sha1:abc123".to_string()),
            revision: Some("main@sha1:abc123".to_string()),
            labels: HashMap::new(),
            annotations: HashMap::new(),
            last_reconciled: Some(chrono::Utc::now() - chrono::Duration::minutes(5)),
            reconciliation_history: vec![],
        },
        ResourceInfo {
            name: "my-gitrepo".to_string(),
            namespace: "flux-system".to_string(),
            resource_type: "GitRepository".to_string(),
            age: Some(chrono::Utc::now() - chrono::Duration::hours(1)),
            suspended: Some(false),
            ready: Some(true),
            message: Some("Fetched revision: main@sha1:def456".to_string()),
            revision: Some("main@sha1:def456".to_string()),
            labels: HashMap::new(),
            annotations: HashMap::new(),
            last_reconciled: Some(chrono::Utc::now() - chrono::Duration::minutes(2)),
            reconciliation_history: vec![],
        },
        ResourceInfo {
            name: "my-helmrelease".to_string(),
            namespace: "default".to_string(),
            resource_type: "HelmRelease".to_string(),
            age: Some(chrono::Utc::now() - chrono::Duration::hours(3)),
            suspended: Some(true),
            ready: Some(false),
            message: Some("Suspended".to_string()),
            revision: None,
            labels: HashMap::new(),
            annotations: HashMap::new(),
            last_reconciled: None,
            reconciliation_history: vec![],
        },
    ];

    for resource in resources {
        let key = format!(
            "{}:{}:{}",
            resource.resource_type, resource.namespace, resource.name
        );
        state.upsert(key, resource);
    }

    state
}

#[test]
fn test_render_header() {
    let state = create_test_resource_state();
    let theme = create_test_theme();
    let config = create_test_config();

    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    let controller_pods = ControllerPodState::default();

    terminal
        .draw(|frame| {
            let area = frame.size();
            render_header(
                frame,
                area,
                &state,
                &controller_pods,
                "test-context",
                &None,
                "",
                &None,
                3,
                66.67,
                None,
                false,
                &theme,
                config.ui.no_icons,
                &[],
            );
        })
        .unwrap();

    assert_snapshot!(terminal.backend());
}

#[test]
fn test_render_header_with_namespace() {
    let state = create_test_resource_state();
    let theme = create_test_theme();
    let config = create_test_config();

    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    let controller_pods = ControllerPodState::default();

    terminal
        .draw(|frame| {
            let area = frame.size();
            render_header(
                frame,
                area,
                &state,
                &controller_pods,
                "test-context",
                &Some("flux-system".to_string()),
                "",
                &None,
                2,
                100.0,
                None,
                false,
                &theme,
                config.ui.no_icons,
                &[],
            );
        })
        .unwrap();

    assert_snapshot!(terminal.backend());
}

#[test]
fn test_render_header_with_filter() {
    let state = create_test_resource_state();
    let theme = create_test_theme();
    let config = create_test_config();

    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    let controller_pods = ControllerPodState::default();

    terminal
        .draw(|frame| {
            let area = frame.size();
            render_header(
                frame,
                area,
                &state,
                &controller_pods,
                "test-context",
                &None,
                "kustomization",
                &Some("Kustomization".to_string()),
                1,
                100.0,
                Some("healthy"),
                false,
                &theme,
                config.ui.no_icons,
                &[],
            );
        })
        .unwrap();

    assert_snapshot!(terminal.backend());
}

#[test]
fn test_render_footer_navigation() {
    let state = create_test_resource_state();
    let theme = create_test_theme();
    let _config = create_test_config();

    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();

    terminal
        .draw(|frame| {
            let area = frame.size();
            let operation_registry = flux9s::tui::operations::OperationRegistry::new();
            render_footer(
                frame,
                area,
                false,
                "",
                false,
                "",
                false,
                &None,
                &None,
                &operation_registry,
                &state,
                &theme,
                &[],
                &None,
            );
        })
        .unwrap();

    assert_snapshot!(terminal.backend());
}

#[test]
fn test_render_footer_command_mode() {
    let state = create_test_resource_state();
    let theme = create_test_theme();

    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();

    terminal
        .draw(|frame| {
            let area = frame.size();
            let operation_registry = flux9s::tui::operations::OperationRegistry::new();
            render_footer(
                frame,
                area,
                true,
                ":kustomization",
                false,
                "",
                false,
                &None,
                &None,
                &operation_registry,
                &state,
                &theme,
                &[],
                &None,
            );
        })
        .unwrap();

    assert_snapshot!(terminal.backend());
}

#[test]
fn test_render_footer_filter_mode() {
    let state = create_test_resource_state();
    let theme = create_test_theme();

    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();

    terminal
        .draw(|frame| {
            let area = frame.size();
            let operation_registry = flux9s::tui::operations::OperationRegistry::new();
            render_footer(
                frame,
                area,
                false,
                "",
                true,
                "my-",
                false,
                &None,
                &None,
                &operation_registry,
                &state,
                &theme,
                &[],
                &None,
            );
        })
        .unwrap();

    assert_snapshot!(terminal.backend());
}

#[test]
fn test_render_resource_list() {
    let state = create_test_resource_state();
    let theme = create_test_theme();
    let config = create_test_config();

    let mut resources: Vec<ResourceInfo> = state.all();
    // Sort for deterministic ordering: namespace, resource_type, name
    resources.sort_by(|a, b| {
        a.namespace
            .cmp(&b.namespace)
            .then_with(|| a.resource_type.cmp(&b.resource_type))
            .then_with(|| a.name.cmp(&b.name))
    });

    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    let mut scroll_offset = 0;

    terminal
        .draw(|frame| {
            let area = frame.size();
            let resource_objects = HashMap::new();
            render_resource_list(
                frame,
                area,
                &resources,
                0,
                &mut scroll_offset,
                &None,
                &resource_objects,
                &theme,
                config.ui.no_icons,
                &HashSet::new(),
            );
        })
        .unwrap();

    assert_snapshot!(terminal.backend());
}

#[test]
fn test_render_resource_list_with_selection() {
    let state = create_test_resource_state();
    let theme = create_test_theme();
    let config = create_test_config();

    let mut resources: Vec<ResourceInfo> = state.all();
    // Sort for deterministic ordering: namespace, resource_type, name
    resources.sort_by(|a, b| {
        a.namespace
            .cmp(&b.namespace)
            .then_with(|| a.resource_type.cmp(&b.resource_type))
            .then_with(|| a.name.cmp(&b.name))
    });

    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    let mut scroll_offset = 0;

    terminal
        .draw(|frame| {
            let area = frame.size();
            let resource_objects = HashMap::new();
            render_resource_list(
                frame,
                area,
                &resources,
                1, // Select second item
                &mut scroll_offset,
                &None,
                &resource_objects,
                &theme,
                config.ui.no_icons,
                &HashSet::new(),
            );
        })
        .unwrap();

    assert_snapshot!(terminal.backend());
}

#[test]
fn test_render_resource_list_with_resource_type_filter() {
    let state = create_test_resource_state();
    let theme = create_test_theme();
    let config = create_test_config();

    let resources: Vec<ResourceInfo> = state
        .all()
        .into_iter()
        .filter(|info| info.resource_type == "Kustomization")
        .collect();

    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    let mut scroll_offset = 0;

    terminal
        .draw(|frame| {
            let area = frame.size();
            let resource_objects = HashMap::new();
            render_resource_list(
                frame,
                area,
                &resources,
                0,
                &mut scroll_offset,
                &Some("Kustomization".to_string()),
                &resource_objects,
                &theme,
                config.ui.no_icons,
                &HashSet::new(),
            );
        })
        .unwrap();

    assert_snapshot!(terminal.backend());
}

#[test]
fn test_render_resource_list_empty() {
    let theme = create_test_theme();
    let config = create_test_config();

    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    let mut scroll_offset = 0;

    terminal
        .draw(|frame| {
            let area = frame.size();
            let resource_objects = HashMap::new();
            render_resource_list(
                frame,
                area,
                &[],
                0,
                &mut scroll_offset,
                &None,
                &resource_objects,
                &theme,
                config.ui.no_icons,
                &HashSet::new(),
            );
        })
        .unwrap();

    assert_snapshot!(terminal.backend());
}

#[test]
fn test_render_resource_detail() {
    let state = create_test_resource_state();
    let theme = create_test_theme();

    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();

    terminal
        .draw(|frame| {
            let area = frame.size();
            let resource_objects = HashMap::new();
            render_resource_detail(
                frame,
                area,
                &Some("Kustomization:flux-system:my-kustomization".to_string()),
                &state,
                &resource_objects,
                &theme,
            );
        })
        .unwrap();

    assert_snapshot!(terminal.backend());
}

#[test]
fn test_render_resource_detail_no_selection() {
    let state = create_test_resource_state();
    let theme = create_test_theme();

    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();

    terminal
        .draw(|frame| {
            let area = frame.size();
            let resource_objects = HashMap::new();
            render_resource_detail(frame, area, &None, &state, &resource_objects, &theme);
        })
        .unwrap();

    assert_snapshot!(terminal.backend());
}

#[test]
fn test_render_resource_yaml_no_selection() {
    let state = create_test_resource_state();
    let theme = create_test_theme();

    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    let mut yaml_scroll_offset = 0;

    terminal
        .draw(|frame| {
            let area = frame.size();
            let resource_objects = HashMap::new();
            render_resource_yaml(
                frame,
                area,
                &None,
                &state,
                &resource_objects,
                &None,
                &None,
                &mut yaml_scroll_offset,
                &theme,
            );
        })
        .unwrap();

    assert_snapshot!(terminal.backend());
}

#[test]
fn test_render_resource_yaml_pending() {
    let state = create_test_resource_state();
    let theme = create_test_theme();

    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    let mut yaml_scroll_offset = 0;

    terminal
        .draw(|frame| {
            let area = frame.size();
            let resource_objects = HashMap::new();
            render_resource_yaml(
                frame,
                area,
                &Some("Kustomization:flux-system:my-kustomization".to_string()),
                &state,
                &resource_objects,
                &None,
                &Some("Kustomization:flux-system:my-kustomization".to_string()),
                &mut yaml_scroll_offset,
                &theme,
            );
        })
        .unwrap();

    assert_snapshot!(terminal.backend());
}

#[test]
fn test_render_resource_yaml_with_data() {
    let state = create_test_resource_state();
    let theme = create_test_theme();

    // Create a sample resource JSON
    let resource_json = serde_json::json!({
        "apiVersion": "kustomize.toolkit.fluxcd.io/v1",
        "kind": "Kustomization",
        "metadata": {
            "name": "my-kustomization",
            "namespace": "flux-system"
        },
        "spec": {
            "path": "./kustomize",
            "sourceRef": {
                "kind": "GitRepository",
                "name": "my-gitrepo"
            }
        },
        "status": {
            "conditions": [
                {
                    "type": "Ready",
                    "status": "True",
                    "message": "Applied revision: main@sha1:abc123"
                }
            ]
        }
    });

    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    let mut yaml_scroll_offset = 0;

    terminal
        .draw(|frame| {
            let area = frame.size();
            let resource_objects = HashMap::new();
            render_resource_yaml(
                frame,
                area,
                &Some("Kustomization:flux-system:my-kustomization".to_string()),
                &state,
                &resource_objects,
                &Some(resource_json),
                &None,
                &mut yaml_scroll_offset,
                &theme,
            );
        })
        .unwrap();

    assert_snapshot!(terminal.backend());
}
