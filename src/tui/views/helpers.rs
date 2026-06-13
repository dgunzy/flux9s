//! Common helper functions for view rendering
//!
//! This module provides reusable functions to reduce duplication across views.

use crate::tui::theme::Theme;
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::Line,
    widgets::{Block, Borders, Paragraph},
};

/// Update scroll offset based on selected index and visible area
///
/// This handles the common pattern of scrolling to keep the selected item visible
/// with a buffer zone.
pub fn update_scroll_offset(
    selected_index: usize,
    visible_height: usize,
    scroll_offset: &mut usize,
    scroll_buffer: usize,
) {
    // When selected row is near bottom, scroll to keep buffer
    if selected_index >= *scroll_offset + visible_height.saturating_sub(scroll_buffer) {
        *scroll_offset =
            selected_index.saturating_sub(visible_height.saturating_sub(scroll_buffer + 1));
    }
    // When selected row is above visible area, scroll to show it with buffer
    if selected_index < *scroll_offset + scroll_buffer {
        *scroll_offset = selected_index.saturating_sub(scroll_buffer);
    }
}

/// Render a loading state message
///
/// Shows a consistent loading message across all views.
pub fn render_loading_state(f: &mut Frame, area: Rect, title: &str, message: &str, theme: &Theme) {
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.text_label));
    let text = vec![
        Line::from(message),
        Line::from(""),
        Line::from("Please wait..."),
    ];
    let paragraph = Paragraph::new(text)
        .block(block)
        .style(Style::default().fg(theme.text_secondary));
    f.render_widget(paragraph, area);
}

/// Clean a JSON object by removing Kubernetes-managed fields that add noise in text views.
pub fn clean_resource_json(obj: &serde_json::Value) -> serde_json::Value {
    match obj {
        serde_json::Value::Object(map) => {
            let mut cleaned = serde_json::Map::new();
            for (key, value) in map {
                if key == "managedFields" {
                    continue;
                }
                cleaned.insert(key.clone(), clean_resource_json(value));
            }
            serde_json::Value::Object(cleaned)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(clean_resource_json).collect())
        }
        other => other.clone(),
    }
}

/// Render an empty state message
///
/// Shows a consistent empty state message across all views.
pub fn render_empty_state(
    f: &mut Frame,
    area: Rect,
    title: &str,
    message: &str,
    instructions: &str,
    theme: &Theme,
) {
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.text_label));
    let text = vec![
        Line::from(message),
        Line::from(""),
        Line::from(instructions),
    ];
    let paragraph = Paragraph::new(text)
        .block(block)
        .style(Style::default().fg(theme.text_secondary));
    f.render_widget(paragraph, area);
}

/// Format a boolean option as a string
///
/// Converts `Option<bool>` to "True", "False", or "?".
#[allow(dead_code)] // Reserved for future use
pub fn format_bool_option(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "True",
        Some(false) => "False",
        None => "?",
    }
}

/// Truncate a message to a maximum length
///
/// If the message exceeds max_len, truncates and adds "...".
pub fn truncate_message(message: &str, max_len: usize) -> String {
    if message.len() > max_len {
        format!("{}...", &message[..max_len.saturating_sub(3)])
    } else {
        message.to_string()
    }
}

/// Create a block with title and borders using theme
///
/// Reduces boilerplate when creating blocks in views.
pub fn create_themed_block<'a>(title: &'a str, theme: &Theme) -> Block<'a> {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.text_label))
}

/// Format a resource age (creation timestamp) as a compact k9s-style duration.
///
/// Uses the largest applicable unit: `42s`, `5m`, `3h`, `12d`, `2y`.
/// Returns `-` when the timestamp is unknown.
pub fn format_age(age: Option<chrono::DateTime<chrono::Utc>>) -> String {
    let Some(created) = age else {
        return "-".to_string();
    };
    let seconds = (chrono::Utc::now() - created).num_seconds().max(0);
    match seconds {
        s if s < 60 => format!("{}s", s),
        s if s < 60 * 60 => format!("{}m", s / 60),
        s if s < 24 * 60 * 60 => format!("{}h", s / (60 * 60)),
        s if s < 365 * 24 * 60 * 60 => format!("{}d", s / (24 * 60 * 60)),
        s => format!("{}y", s / (365 * 24 * 60 * 60)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    #[test]
    fn test_format_age_units() {
        assert_eq!(format_age(None), "-");
        assert_eq!(format_age(Some(Utc::now())), "0s");
        assert_eq!(format_age(Some(Utc::now() - Duration::seconds(42))), "42s");
        assert_eq!(format_age(Some(Utc::now() - Duration::minutes(5))), "5m");
        assert_eq!(format_age(Some(Utc::now() - Duration::hours(3))), "3h");
        assert_eq!(format_age(Some(Utc::now() - Duration::days(12))), "12d");
        assert_eq!(format_age(Some(Utc::now() - Duration::days(800))), "2y");
    }

    #[test]
    fn test_format_age_future_timestamp_clamps_to_zero() {
        // Clock skew between client and API server should not render negative ages
        assert_eq!(format_age(Some(Utc::now() + Duration::seconds(30))), "0s");
    }
}
