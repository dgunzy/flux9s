//! Quit confirmation dialog rendering
//!
//! Displayed as a centered popup overlay when the user presses `q` or `Esc`
//! at the top-level view. This is inspired by k9s, where neither key exits
//! the application directly — only `Q`, `:q`, or `Ctrl+C` do that.

use crate::tui::theme::Theme;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph},
};

/// Render the quit confirmation dialog as a centered popup overlay.
///
/// Renders on top of the current view (background remains visible).
/// Confirm with `y`/`Y`; cancel with `n`/`N`/`q`/`Esc`.
pub fn render_quit_confirm(f: &mut Frame, area: Rect, theme: &Theme) {
    let popup_width = 52u16.min(area.width);
    let popup_height = 7u16.min(area.height);
    let popup_area = centered_rect(popup_width, popup_height, area);

    // Clear the background area so the popup is opaque
    f.render_widget(Clear, popup_area);

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            ratatui::text::Span::styled("Press ", Style::default().fg(theme.text_primary)),
            ratatui::text::Span::styled(
                "y",
                Style::default()
                    .fg(theme.operation_confirm)
                    .add_modifier(Modifier::BOLD),
            ),
            ratatui::text::Span::styled(" or ", Style::default().fg(theme.text_primary)),
            ratatui::text::Span::styled(
                "Y",
                Style::default()
                    .fg(theme.operation_confirm)
                    .add_modifier(Modifier::BOLD),
            ),
            ratatui::text::Span::styled(" to quit", Style::default().fg(theme.text_primary)),
        ]),
        Line::from(vec![
            ratatui::text::Span::styled("Press ", Style::default().fg(theme.text_primary)),
            ratatui::text::Span::styled(
                "n",
                Style::default()
                    .fg(theme.operation_cancel)
                    .add_modifier(Modifier::BOLD),
            ),
            ratatui::text::Span::styled(", ", Style::default().fg(theme.text_primary)),
            ratatui::text::Span::styled(
                "N",
                Style::default()
                    .fg(theme.operation_cancel)
                    .add_modifier(Modifier::BOLD),
            ),
            ratatui::text::Span::styled(", ", Style::default().fg(theme.text_primary)),
            ratatui::text::Span::styled(
                "q",
                Style::default()
                    .fg(theme.operation_cancel)
                    .add_modifier(Modifier::BOLD),
            ),
            ratatui::text::Span::styled(", or ", Style::default().fg(theme.text_primary)),
            ratatui::text::Span::styled(
                "Esc",
                Style::default()
                    .fg(theme.operation_cancel)
                    .add_modifier(Modifier::BOLD),
            ),
            ratatui::text::Span::styled(" to cancel", Style::default().fg(theme.text_primary)),
        ]),
        Line::from(""),
        Line::from(vec![ratatui::text::Span::styled(
            "Tip: Q, :q, or Ctrl+C skips this dialog",
            Style::default()
                .fg(theme.text_secondary)
                .add_modifier(Modifier::ITALIC),
        )]),
    ];

    let block = Block::default()
        .title("Quit flux9s?")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.operation_warning))
        .style(Style::default().fg(theme.text_primary).bg(Color::Black));
    let paragraph = Paragraph::new(lines)
        .block(block)
        .alignment(ratatui::layout::Alignment::Center);
    f.render_widget(paragraph, popup_area);
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let popup_x = area.x.saturating_add(area.width.saturating_sub(width) / 2);
    let popup_y = area
        .y
        .saturating_add(area.height.saturating_sub(height) / 2);
    Rect {
        x: popup_x,
        y: popup_y,
        width: width.min(area.width),
        height: height.min(area.height),
    }
}
