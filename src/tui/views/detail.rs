//! Resource detail view rendering

use crate::models::FluxResourceKind;
use crate::models::flux_resource_kind::field_names;
use crate::tui::theme::Theme;
use crate::watcher::ResourceState;
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};
use std::collections::HashMap;

/// Capitalize the first letter of a field key for display
/// Special case: "URL" stays all-caps
fn capitalize_first(key: &str) -> String {
    if key == "URL" {
        "URL".to_string()
    } else if let Some(first) = key.chars().next() {
        format!(
            "{}{}",
            first.to_uppercase(),
            &key[first.len_utf8()..].to_lowercase()
        )
    } else {
        key.to_string()
    }
}

/// Render the resource detail view
pub fn render_resource_detail(
    f: &mut Frame,
    area: Rect,
    selected_resource_key: &Option<String>,
    state: &ResourceState,
    resource_objects: &HashMap<String, serde_json::Value>,
    theme: &Theme,
) {
    let key = match selected_resource_key {
        Some(k) => k,
        None => {
            let text = vec![Line::from("No resource selected")];
            let block = crate::tui::views::helpers::create_themed_block("Detail", theme);
            let paragraph = Paragraph::new(text).block(block);
            f.render_widget(paragraph, area);
            return;
        }
    };

    let resource = match state.get(key) {
        Some(r) => r,
        None => {
            let text = vec![Line::from("Resource not found")];
            let block = crate::tui::views::helpers::create_themed_block("Detail", theme);
            let paragraph = Paragraph::new(text).block(block);
            f.render_widget(paragraph, area);
            return;
        }
    };

    let obj_json = resource_objects.get(key);

    let mut lines = vec![
        Line::from(vec![
            Span::styled("Name: ", Style::default().fg(theme.text_label)),
            Span::raw(&resource.name),
        ]),
        Line::from(vec![
            Span::styled("Namespace: ", Style::default().fg(theme.text_label)),
            Span::raw(&resource.namespace),
        ]),
        Line::from(vec![
            Span::styled("Type: ", Style::default().fg(theme.text_label)),
            Span::raw(&resource.resource_type),
        ]),
        Line::from(""),
    ];

    // Status fields
    if let Some(suspended) = resource.suspended {
        lines.push(Line::from(vec![
            Span::styled("Suspended: ", Style::default().fg(theme.text_label)),
            Span::styled(
                if suspended { "True" } else { "False" },
                if suspended {
                    theme.status_suspended_style()
                } else {
                    theme.status_ready_style()
                },
            ),
        ]));
    }

    if let Some(ready) = resource.ready {
        lines.push(Line::from(vec![
            Span::styled("Ready: ", Style::default().fg(theme.text_label)),
            Span::styled(
                if ready { "True" } else { "False" },
                if ready {
                    theme.status_ready_style()
                } else {
                    theme.status_error_style()
                },
            ),
        ]));
    }

    if let Some(ref revision) = resource.revision {
        lines.push(Line::from(vec![
            Span::styled("Revision: ", Style::default().fg(theme.text_label)),
            Span::raw(revision),
        ]));
    }

    if let Some(ref message) = resource.message {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            "Message: ",
            Style::default().fg(theme.text_label),
        )]));
        // Split long messages into multiple lines
        for line in message.lines() {
            lines.push(Line::from(line));
        }
    }

    // Show JSON spec if available
    if let Some(obj) = obj_json {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            "Spec: ",
            Style::default().fg(theme.text_label),
        )]));

        // Extract fields using FluxResourceKind method
        if let Some(kind) = FluxResourceKind::parse_optional(&resource.resource_type) {
            let fields = kind.extract_fields(obj);

            // Display order: URL, BRANCH, PATH, CHART, VERSION, SOURCE, IMAGE, SEMVER, TAG, PRUNE, INTERVAL, DIGEST
            let display_order = [
                field_names::TYPE,
                field_names::URL,
                field_names::SECRET,
                field_names::BRANCH,
                field_names::PATH,
                field_names::CHART,
                field_names::VERSION,
                field_names::SOURCE,
                field_names::IMAGE,
                field_names::SEMVER,
                field_names::TAG,
                field_names::PRUNE,
                field_names::INTERVAL,
                field_names::DIGEST,
            ];

            for &field_key in display_order.iter() {
                if let Some(value) = fields.get(field_key) {
                    let label = capitalize_first(field_key);
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("{}: ", label),
                            Style::default().fg(theme.text_label),
                        ),
                        Span::raw(value.clone()),
                    ]));
                }
            }
        }
    }

    let title = format!("Detail - {}", resource.name);
    let block = crate::tui::views::helpers::create_themed_block(&title, theme);
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(ratatui::widgets::Wrap { trim: true });
    f.render_widget(paragraph, area);
}
