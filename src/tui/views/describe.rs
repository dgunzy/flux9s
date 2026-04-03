//! Describe view rendering

use crate::tui::theme::Theme;
use crate::watcher::ResourceState;
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};
use serde_json::Value;
use std::collections::HashMap;

fn scalar_to_string(value: &Value) -> String {
    match value {
        Value::Null => "<none>".to_string(),
        Value::Bool(v) => {
            if *v {
                "True".to_string()
            } else {
                "False".to_string()
            }
        }
        Value::Number(v) => v.to_string(),
        Value::String(v) => v.clone(),
        _ => value.to_string(),
    }
}

fn push_section_header(lines: &mut Vec<Line<'static>>, title: &str, theme: &Theme) {
    if !lines.is_empty() {
        lines.push(Line::from(""));
    }
    lines.push(Line::from(vec![Span::styled(
        title.to_string(),
        Style::default().fg(theme.text_label),
    )]));
}

fn push_scalar_field(lines: &mut Vec<Line<'static>>, label: &str, value: &str, theme: &Theme) {
    lines.push(Line::from(vec![
        Span::styled(
            format!("{}: ", label),
            Style::default().fg(theme.text_label),
        ),
        Span::raw(value.to_string()),
    ]));
}

fn push_value_lines(lines: &mut Vec<Line<'static>>, value: &Value, indent: usize, theme: &Theme) {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for key in keys {
                let child = &map[key];
                let prefix = " ".repeat(indent);
                match child {
                    Value::Object(_) | Value::Array(_) => {
                        lines.push(Line::from(vec![Span::styled(
                            format!("{}{}:", prefix, key),
                            Style::default().fg(theme.text_label),
                        )]));
                        push_value_lines(lines, child, indent + 2, theme);
                    }
                    _ => {
                        lines.push(Line::from(vec![
                            Span::styled(
                                format!("{}{}: ", prefix, key),
                                Style::default().fg(theme.text_label),
                            ),
                            Span::raw(scalar_to_string(child)),
                        ]));
                    }
                }
            }
        }
        Value::Array(arr) => {
            if arr.is_empty() {
                lines.push(Line::from(format!("{}[]", " ".repeat(indent))));
                return;
            }

            for item in arr {
                let prefix = " ".repeat(indent);
                match item {
                    Value::Object(_) | Value::Array(_) => {
                        lines.push(Line::from(format!("{}-", prefix)));
                        push_value_lines(lines, item, indent + 2, theme);
                    }
                    _ => {
                        lines.push(Line::from(format!(
                            "{}- {}",
                            prefix,
                            scalar_to_string(item)
                        )));
                    }
                }
            }
        }
        _ => lines.push(Line::from(format!(
            "{}{}",
            " ".repeat(indent),
            scalar_to_string(value)
        ))),
    }
}

fn build_describe_lines(
    resource: Option<&crate::watcher::ResourceInfo>,
    obj_json: &Value,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let metadata = obj_json.get("metadata");

    push_section_header(&mut lines, "Summary", theme);

    if let Some(resource) = resource {
        push_scalar_field(&mut lines, "Name", &resource.name, theme);
        push_scalar_field(&mut lines, "Namespace", &resource.namespace, theme);
        push_scalar_field(&mut lines, "Kind", &resource.resource_type, theme);
        if let Some(ready) = resource.ready {
            push_scalar_field(
                &mut lines,
                "Ready",
                if ready { "True" } else { "False" },
                theme,
            );
        }
        if let Some(suspended) = resource.suspended {
            push_scalar_field(
                &mut lines,
                "Suspended",
                if suspended { "True" } else { "False" },
                theme,
            );
        }
        if let Some(revision) = &resource.revision {
            push_scalar_field(&mut lines, "Revision", revision, theme);
        }
        if let Some(message) = &resource.message {
            push_section_header(&mut lines, "Message", theme);
            for message_line in message.lines() {
                lines.push(Line::from(message_line.to_string()));
            }
        }
    } else {
        if let Some(name) = metadata
            .and_then(|meta| meta.get("name"))
            .and_then(|value| value.as_str())
        {
            push_scalar_field(&mut lines, "Name", name, theme);
        }
        if let Some(namespace) = metadata
            .and_then(|meta| meta.get("namespace"))
            .and_then(|value| value.as_str())
        {
            push_scalar_field(&mut lines, "Namespace", namespace, theme);
        }
        if let Some(kind) = obj_json.get("kind").and_then(|value| value.as_str()) {
            push_scalar_field(&mut lines, "Kind", kind, theme);
        }
    }

    if let Some(api_version) = obj_json.get("apiVersion").and_then(|value| value.as_str()) {
        push_scalar_field(&mut lines, "API Version", api_version, theme);
    }

    if let Some(created_at) = metadata
        .and_then(|meta| meta.get("creationTimestamp"))
        .and_then(|value| value.as_str())
    {
        push_scalar_field(&mut lines, "Created", created_at, theme);
    }
    if let Some(generation) = metadata.and_then(|meta| meta.get("generation")) {
        push_scalar_field(
            &mut lines,
            "Generation",
            &scalar_to_string(generation),
            theme,
        );
    }
    if let Some(resource_version) = metadata
        .and_then(|meta| meta.get("resourceVersion"))
        .and_then(|value| value.as_str())
    {
        push_scalar_field(&mut lines, "Resource Version", resource_version, theme);
    }

    if let Some(labels) = metadata.and_then(|meta| meta.get("labels")) {
        push_section_header(&mut lines, "Labels", theme);
        push_value_lines(&mut lines, labels, 2, theme);
    }

    if let Some(annotations) = metadata.and_then(|meta| meta.get("annotations")) {
        push_section_header(&mut lines, "Annotations", theme);
        push_value_lines(&mut lines, annotations, 2, theme);
    }

    if let Some(spec) = obj_json.get("spec") {
        push_section_header(&mut lines, "Spec", theme);
        push_value_lines(&mut lines, spec, 2, theme);
    }

    if let Some(status) = obj_json.get("status") {
        push_section_header(&mut lines, "Status", theme);
        push_value_lines(&mut lines, status, 2, theme);
    }

    lines
}

/// Render the describe view.
pub fn render_resource_describe(
    f: &mut Frame,
    area: Rect,
    selected_resource_key: &Option<String>,
    state: &ResourceState,
    resource_objects: &HashMap<String, serde_json::Value>,
    describe_fetched: &Option<serde_json::Value>,
    describe_fetch_pending: &Option<String>,
    describe_scroll_offset: &mut usize,
    theme: &Theme,
) {
    let key = match selected_resource_key {
        Some(k) => k,
        None => {
            crate::tui::views::helpers::render_empty_state(
                f,
                area,
                "Describe",
                "No resource selected",
                "Select a resource to view its description",
                theme,
            );
            return;
        }
    };

    let obj_json = if let Some(fetched) = describe_fetched {
        fetched.clone()
    } else if describe_fetch_pending.is_some() {
        crate::tui::views::helpers::render_loading_state(
            f,
            area,
            "Describe",
            "Loading resource description from API...",
            theme,
        );
        return;
    } else {
        match resource_objects.get(key).cloned() {
            Some(obj) => obj,
            None => {
                crate::tui::views::helpers::render_empty_state(
                    f,
                    area,
                    "Describe",
                    "Resource description not available",
                    "Resource may have been deleted",
                    theme,
                );
                return;
            }
        }
    };

    let cleaned_json = crate::tui::views::helpers::clean_resource_json(&obj_json);
    let resource = state.get(key);
    let title = if let Some(ref resource) = resource {
        format!("Describe - {} - {}", resource.resource_type, resource.name)
    } else {
        "Describe".to_string()
    };

    let all_lines = build_describe_lines(resource.as_ref(), &cleaned_json, theme);
    let visible_height = area.height.saturating_sub(2) as usize;
    let max_scroll = all_lines.len().saturating_sub(visible_height);
    *describe_scroll_offset = (*describe_scroll_offset).min(max_scroll);

    let visible_lines: Vec<Line> = all_lines
        .iter()
        .skip(*describe_scroll_offset)
        .take(visible_height)
        .cloned()
        .collect();

    let block = crate::tui::views::helpers::create_themed_block(&title, theme);
    let paragraph = Paragraph::new(visible_lines)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}
