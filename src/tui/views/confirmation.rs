//! Confirmation dialog rendering

use crate::tui::theme::Theme;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph},
};

/// Render the confirmation dialog
pub fn render_confirmation(f: &mut Frame, area: Rect, message: &str, theme: &Theme) {
    let msg = message.to_string();
    let lines = vec![
        Line::from(""),
        Line::from(vec![
            ratatui::text::Span::styled("⚠ ", theme.operation_warning_style()),
            ratatui::text::Span::styled("CONFIRMATION REQUIRED", theme.operation_warning_style()),
        ]),
        Line::from(""),
        Line::from(msg.clone()),
        Line::from(""),
        Line::from(vec![
            ratatui::text::Span::raw("Press "),
            ratatui::text::Span::styled(
                "y",
                Style::default()
                    .fg(theme.operation_confirm)
                    .add_modifier(Modifier::BOLD),
            ),
            ratatui::text::Span::raw(" or "),
            ratatui::text::Span::styled(
                "Y",
                Style::default()
                    .fg(theme.operation_confirm)
                    .add_modifier(Modifier::BOLD),
            ),
            ratatui::text::Span::raw(" to confirm"),
        ]),
        Line::from(vec![
            ratatui::text::Span::raw("Press "),
            ratatui::text::Span::styled(
                "n",
                Style::default()
                    .fg(theme.operation_cancel)
                    .add_modifier(Modifier::BOLD),
            ),
            ratatui::text::Span::raw(", "),
            ratatui::text::Span::styled(
                "N",
                Style::default()
                    .fg(theme.operation_cancel)
                    .add_modifier(Modifier::BOLD),
            ),
            ratatui::text::Span::raw(", or "),
            ratatui::text::Span::styled(
                "Esc",
                Style::default()
                    .fg(theme.operation_cancel)
                    .add_modifier(Modifier::BOLD),
            ),
            ratatui::text::Span::raw(" to cancel"),
        ]),
    ];

    let block = Block::default()
        .title("Confirm Operation")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.text_label))
        .border_style(Style::default().fg(theme.operation_warning));
    let paragraph = Paragraph::new(lines)
        .block(block)
        .alignment(ratatui::layout::Alignment::Center);
    f.render_widget(paragraph, area);
}
