//! Connection error screen rendering.
//!
//! Shown full-screen when flux9s cannot reach the Kubernetes API server at
//! startup. Gives the user a classified reason, the offending context/server,
//! an actionable hint, and where to find logs. The app stays alive so the
//! message remains readable until the user quits.

use std::path::Path;

use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::kube::health::ConnectionError;
use crate::tui::Theme;

/// Render the full-screen connection error view.
pub fn render_connection_error(
    f: &mut Frame,
    area: Rect,
    theme: &Theme,
    error: &ConnectionError,
    log_path: Option<&Path>,
) {
    let label = theme.header_context_style();
    let dim = Style::default().fg(theme.text_secondary);

    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(Span::styled(
        format!("✖ {}", error.kind.summary()),
        theme.status_error_style().add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    lines.push(Line::from(vec![
        Span::styled("Context:  ", label),
        Span::styled(
            error.context.clone().unwrap_or_else(|| "(unknown)".into()),
            label,
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Server:   ", label),
        Span::styled(
            error
                .server_url
                .clone()
                .unwrap_or_else(|| "(unknown)".into()),
            label,
        ),
    ]));
    lines.push(Line::from(""));

    lines.push(Line::from(Span::styled("Details:", label)));
    lines.push(Line::from(Span::styled(error.detail(), dim)));
    lines.push(Line::from(""));

    lines.push(Line::from(Span::styled("What to try:", label)));
    lines.push(Line::from(Span::styled(
        error.kind.hint().to_string(),
        label,
    )));
    lines.push(Line::from(""));

    let log_line = match log_path {
        Some(p) => format!("Logs: {}", p.display()),
        None => "Logs: run flux9s with --debug to capture detailed logs".to_string(),
    };
    lines.push(Line::from(Span::styled(log_line, dim)));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("Press q or Ctrl+C to quit", dim)));

    let block = Block::default()
        .title(" Connection Error ")
        .borders(Borders::ALL)
        .border_style(theme.status_error_style());

    let paragraph = Paragraph::new(lines)
        .block(block)
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: true });

    f.render_widget(paragraph, area);
}
