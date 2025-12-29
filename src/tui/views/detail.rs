//! Resource detail view rendering

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
use std::sync::Arc;
use std::sync::RwLock;

/// Render the resource detail view
pub fn render_resource_detail(
    f: &mut Frame,
    area: Rect,
    selected_resource_key: &Option<String>,
    state: &ResourceState,
    resource_objects: &Arc<RwLock<HashMap<String, serde_json::Value>>>,
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

    let objects = resource_objects.read().unwrap();
    let obj_json = objects.get(key);

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

        if let Some(spec) = obj.get("spec").and_then(|s| s.as_object()) {
            // Show key spec fields
            if let Some(url) = spec.get("url").and_then(|u| u.as_str()) {
                lines.push(Line::from(vec![
                    Span::styled("URL: ", Style::default().fg(theme.text_label)),
                    Span::raw(url),
                ]));
            }
            if let Some(branch) = spec.get("branch").and_then(|b| b.as_str()) {
                lines.push(Line::from(vec![
                    Span::styled("Branch: ", Style::default().fg(theme.text_label)),
                    Span::raw(branch),
                ]));
            }
            if let Some(path) = spec.get("path").and_then(|p| p.as_str()) {
                lines.push(Line::from(vec![
                    Span::styled("Path: ", Style::default().fg(theme.text_label)),
                    Span::raw(path),
                ]));
            }
            if let Some(interval) = spec.get("interval").and_then(|i| i.as_str()) {
                lines.push(Line::from(vec![
                    Span::styled("Interval: ", Style::default().fg(theme.text_label)),
                    Span::raw(interval),
                ]));
            }
            // OCIRepository specific: semver from spec.ref.semver
            if let Some(semver) = spec
                .get("ref")
                .and_then(|r| r.get("semver"))
                .and_then(|s| s.as_str())
            {
                lines.push(Line::from(vec![
                    Span::styled("Semver: ", Style::default().fg(theme.text_label)),
                    Span::raw(semver),
                ]));
            }
            // OCIRepository specific: tag from spec.ref.tag
            if let Some(tag) = spec
                .get("ref")
                .and_then(|r| r.get("tag"))
                .and_then(|t| t.as_str())
            {
                lines.push(Line::from(vec![
                    Span::styled("Tag: ", Style::default().fg(theme.text_label)),
                    Span::raw(tag),
                ]));
            }
        }

        // Show status artifact info (digest for OCIRepository)
        if let Some(status) = obj.get("status").and_then(|s| s.as_object()) {
            if let Some(artifact) = status.get("artifact").and_then(|a| a.as_object()) {
                if let Some(digest) = artifact.get("digest").and_then(|d| d.as_str()) {
                    lines.push(Line::from(vec![
                        Span::styled("Digest: ", Style::default().fg(theme.text_label)),
                        Span::raw(digest),
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
