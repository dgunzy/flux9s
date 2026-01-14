//! Reconciliation history view rendering

use crate::tui::theme::Theme;
use crate::watcher::ResourceInfo;
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use std::collections::HashMap;

/// Render the reconciliation history view for a resource
/// Extracts history directly from status.history field in the resource object
pub fn render_reconciliation_history(
    f: &mut Frame,
    area: Rect,
    resource: &ResourceInfo,
    resource_objects: &HashMap<String, serde_json::Value>,
    scroll_offset: &mut usize,
    theme: &Theme,
) -> Result<(), String> {
    let block = Block::default()
        .title(format!(
            "Reconciliation History: {}/{}",
            resource.resource_type, resource.name
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.text_label));

    let inner_area = block.inner(area);
    f.render_widget(block, area);

    // Get the full resource object to extract status.history
    let key =
        crate::watcher::resource_key(&resource.namespace, &resource.name, &resource.resource_type);

    let obj = match resource_objects.get(&key) {
        Some(obj) => obj,
        None => {
            return Err("Resource object not found".to_string());
        }
    };

    // Extract history from status.history
    let history_array = match obj
        .get("status")
        .and_then(|s| s.get("history"))
        .and_then(|h| h.as_array())
    {
        Some(arr) if !arr.is_empty() => arr,
        _ => {
            // No history field or empty - show error message
            use crate::models::FluxResourceKind;
            let mut text = vec![
                Line::from("No reconciliation history available for this resource"),
                Line::from(""),
                Line::from(format!(
                    "Resource type '{}' does not have a status.history field",
                    resource.resource_type
                )),
                Line::from(""),
                Line::from("History is only available for:"),
            ];
            for kind in FluxResourceKind::history_supported_types() {
                text.push(Line::from(format!("  - {}", kind.as_str())));
            }
            text.push(Line::from(""));
            text.push(Line::from("Press Esc to go back"));
            let paragraph = Paragraph::new(text)
                .style(Style::default().fg(theme.text_secondary))
                .wrap(ratatui::widgets::Wrap { trim: true });
            f.render_widget(paragraph, inner_area);
            return Err("No history field".to_string());
        }
    };

    // Convert history array to YAML format
    // Create a wrapper object with just the history field for cleaner output
    let history_obj = serde_json::json!({
        "history": history_array
    });

    let yaml_str = match serde_yaml::to_string(&history_obj) {
        Ok(yaml) => yaml,
        Err(e) => {
            let text = vec![
                Line::from(format!("Failed to serialize history: {}", e)),
                Line::from(""),
                Line::from("Press Esc to go back"),
            ];
            let paragraph = Paragraph::new(text).style(Style::default().fg(theme.text_secondary));
            f.render_widget(paragraph, inner_area);
            return Err("Serialization failed".to_string());
        }
    };

    // Split into lines and format for display
    let lines: Vec<String> = yaml_str.lines().map(|s| s.to_string()).collect();

    // Handle scrolling
    let visible_height = inner_area.height as usize;
    let max_scroll = lines.len().saturating_sub(visible_height);
    *scroll_offset = (*scroll_offset).min(max_scroll);

    // Get visible lines with syntax highlighting
    let visible_lines: Vec<Line> = lines
        .iter()
        .skip(*scroll_offset)
        .take(visible_height)
        .map(|line| {
            let mut spans = Vec::new();

            // Handle indentation
            let indent_len = line.len() - line.trim_start().len();
            let indent = " ".repeat(indent_len);
            let trimmed = line.trim_start();

            if trimmed.is_empty() {
                spans.push(Span::raw(line.clone()));
            }
            // Array items (lines starting with -)
            else if trimmed.starts_with('-') {
                spans.push(Span::raw(indent));
                spans.push(Span::styled("-", Style::default().fg(theme.text_label)));
                let rest = trimmed.trim_start_matches('-').trim_start();
                spans.push(Span::styled(rest, Style::default().fg(theme.text_primary)));
            }
            // Key-value pairs (key: value)
            else if trimmed.contains(':') {
                if let Some((key, value)) = trimmed.split_once(':') {
                    spans.push(Span::raw(indent));
                    spans.push(Span::styled(
                        format!("{}:", key),
                        Style::default().fg(theme.text_label),
                    ));
                    let value_trimmed = value.trim_start();
                    if !value_trimmed.is_empty() {
                        spans.push(Span::styled(
                            format!(" {}", value_trimmed),
                            Style::default().fg(theme.text_value),
                        ));
                    }
                } else {
                    spans.push(Span::raw(line.clone()));
                }
            }
            // Plain text
            else {
                spans.push(Span::styled(
                    line.clone(),
                    Style::default().fg(theme.text_primary),
                ));
            }

            Line::from(spans)
        })
        .collect();

    // Render the history as formatted text
    let paragraph = Paragraph::new(visible_lines)
        .style(Style::default().fg(theme.text_primary))
        .wrap(ratatui::widgets::Wrap { trim: false });

    f.render_widget(paragraph, inner_area);

    // Show scroll indicator if needed
    if lines.len() > visible_height {
        let scroll_info = format!(
            "Scroll: {}/{} (j/k to navigate, Esc to close)",
            *scroll_offset + 1,
            max_scroll + 1
        );
        let scroll_line = Line::from(Span::styled(
            scroll_info,
            Style::default().fg(theme.text_label),
        ));
        let scroll_area = Rect {
            x: area.x + 1,
            y: area.y + area.height - 1,
            width: area.width.saturating_sub(2),
            height: 1,
        };
        f.render_widget(Paragraph::new(scroll_line), scroll_area);
    }

    Ok(())
}
