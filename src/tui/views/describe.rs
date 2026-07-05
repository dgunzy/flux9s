//! Describe view rendering

use crate::tui::app::state::TextSearchState;
use crate::tui::theme::Theme;
use crate::tui::views::yaml::{apply_text_search, decorate_title_with_search, find_match_lines};
use crate::watcher::ResourceState;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
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

/// Append the kubectl-style Events section.
///
/// `events` is `None` when rendering from the locally cached object (no
/// events were fetched) — the section is omitted entirely rather than
/// claiming the resource has none.
fn push_events_section(
    lines: &mut Vec<Line<'static>>,
    events: &[crate::kube::events::KubeEventInfo],
    events_error: Option<&str>,
    theme: &Theme,
) {
    push_section_header(lines, "Events", theme);

    if let Some(error) = events_error {
        lines.push(Line::from(Span::styled(
            format!("  Events unavailable: {}", error),
            Style::default().fg(theme.text_secondary),
        )));
        return;
    }
    if events.is_empty() {
        lines.push(Line::from(Span::styled(
            "  <none>".to_string(),
            Style::default().fg(theme.text_secondary),
        )));
        return;
    }

    lines.push(Line::from(Span::styled(
        format!(
            "  {:<8} {:<26} {:>8} {:>6}  {:<24} {}",
            "TYPE", "REASON", "AGE", "COUNT", "FROM", "MESSAGE"
        ),
        Style::default().fg(theme.text_label),
    )));
    for event in events {
        let row_style = if event.is_warning() {
            Style::default().fg(theme.status_error)
        } else {
            Style::default()
        };
        let age = crate::tui::views::helpers::format_age(event.last_seen);
        // Multi-line messages (common for apply errors) continue on
        // indented follow-up lines so nothing is lost.
        let mut message_lines = event.message.lines();
        let first_message = message_lines.next().unwrap_or_default();
        lines.push(Line::from(Span::styled(
            format!(
                "  {:<8} {:<26} {:>8} {:>6}  {:<24} {}",
                event.event_type,
                event.reason,
                age,
                format!("x{}", event.count),
                event.source,
                first_message,
            ),
            row_style,
        )));
        for continuation in message_lines {
            lines.push(Line::from(Span::styled(
                format!("  {:<78} {}", "", continuation),
                row_style,
            )));
        }
    }
}

fn build_describe_lines(
    resource: Option<&crate::watcher::ResourceInfo>,
    obj_json: &Value,
    events: Option<(&[crate::kube::events::KubeEventInfo], Option<&str>)>,
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

    if let Some((events, events_error)) = events {
        push_events_section(&mut lines, events, events_error, theme);
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
    describe_fetched: Option<&crate::kube::fetch::DescribeData>,
    describe_loading: bool,
    describe_scroll_offset: &mut usize,
    search: &mut TextSearchState,
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

    // Events only exist on the fetched path; the locally cached fallback
    // object omits the section rather than claiming there are none.
    let (obj_json, events) = if let Some(fetched) = describe_fetched {
        (
            fetched.object.clone(),
            Some((fetched.events.as_slice(), fetched.events_error.as_deref())),
        )
    } else if describe_loading {
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
            Some(obj) => (obj, None),
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
    let mut title = if let Some(ref resource) = resource {
        format!("Describe - {} - {}", resource.resource_type, resource.name)
    } else {
        "Describe".to_string()
    };

    let all_lines = build_describe_lines(resource.as_ref(), &cleaned_json, events, theme);
    let visible_height = area.height.saturating_sub(2) as usize;

    // Text search: match against the plain-text content of each line
    let line_texts: Vec<String> = all_lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect();
    let match_lines = find_match_lines(&line_texts, &search.query);
    let current_match_line =
        apply_text_search(search, &match_lines, describe_scroll_offset, visible_height);
    decorate_title_with_search(&mut title, search);

    let max_scroll = all_lines.len().saturating_sub(visible_height);
    *describe_scroll_offset = (*describe_scroll_offset).min(max_scroll);

    let visible_lines: Vec<Line> = all_lines
        .iter()
        .enumerate()
        .skip(*describe_scroll_offset)
        .take(visible_height)
        .map(|(idx, line)| {
            let line = line.clone();
            if Some(idx) == current_match_line {
                line.style(Style::default().add_modifier(Modifier::REVERSED))
            } else if match_lines.binary_search(&idx).is_ok() {
                line.style(Style::default().add_modifier(Modifier::UNDERLINED))
            } else {
                line
            }
        })
        .collect();

    let block = crate::tui::views::helpers::create_themed_block(&title, theme);
    let paragraph = Paragraph::new(visible_lines)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kube::events::KubeEventInfo;

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    fn section_lines(events: &[KubeEventInfo], error: Option<&str>) -> Vec<String> {
        let mut lines = Vec::new();
        push_events_section(&mut lines, events, error, &Theme::default());
        lines.iter().map(line_text).collect()
    }

    fn sample_event(event_type: &str, message: &str) -> KubeEventInfo {
        KubeEventInfo::from_json(&serde_json::json!({
            "metadata": {"uid": "uid-1", "namespace": "flux-system"},
            "involvedObject": {
                "kind": "Kustomization",
                "namespace": "flux-system",
                "name": "podinfo"
            },
            "type": event_type,
            "reason": "TestReason",
            "message": message,
            "count": 2,
            "source": {"component": "kustomize-controller"}
        }))
        .unwrap()
    }

    #[test]
    fn events_section_renders_header_and_rows() {
        let lines = section_lines(&[sample_event("Warning", "something failed")], None);
        // Section title, column header, one event row
        assert!(lines.iter().any(|l| l.trim() == "Events"));
        let header = lines.iter().find(|l| l.contains("REASON")).unwrap();
        assert!(header.contains("TYPE") && header.contains("MESSAGE"));
        let row = lines.iter().find(|l| l.contains("TestReason")).unwrap();
        assert!(row.contains("Warning"));
        assert!(row.contains("x2"));
        assert!(row.contains("kustomize-controller"));
        assert!(row.contains("something failed"));
    }

    #[test]
    fn events_section_multiline_message_continues_indented() {
        let lines = section_lines(&[sample_event("Normal", "line one\nline two")], None);
        let row_idx = lines.iter().position(|l| l.contains("line one")).unwrap();
        assert!(lines[row_idx + 1].contains("line two"));
        assert!(!lines[row_idx + 1].contains("TestReason"));
    }

    #[test]
    fn events_section_empty_and_error_states() {
        assert!(
            section_lines(&[], None)
                .iter()
                .any(|l| l.contains("<none>"))
        );
        let errored = section_lines(&[], Some("forbidden"));
        assert!(
            errored
                .iter()
                .any(|l| l.contains("Events unavailable: forbidden"))
        );
    }

    #[test]
    fn describe_lines_include_events_only_when_fetched() {
        let obj = serde_json::json!({
            "apiVersion": "kustomize.toolkit.fluxcd.io/v1",
            "kind": "Kustomization",
            "metadata": {"name": "podinfo", "namespace": "flux-system"}
        });
        let theme = Theme::default();

        let without = build_describe_lines(None, &obj, None, &theme);
        assert!(!without.iter().map(line_text).any(|l| l.trim() == "Events"));

        let events = [sample_event("Normal", "ok")];
        let with = build_describe_lines(None, &obj, Some((&events, None)), &theme);
        assert!(with.iter().map(line_text).any(|l| l.trim() == "Events"));
    }
}
