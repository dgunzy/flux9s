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
