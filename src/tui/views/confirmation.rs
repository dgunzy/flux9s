//! Confirmation dialog rendering

use crate::tui::app::PendingOperation;
use crate::tui::operations::OperationRegistry;
use crate::tui::theme::Theme;
use crate::watcher::{resource_key, ResourceState};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Render the confirmation dialog
pub fn render_confirmation(
    f: &mut Frame,
    area: Rect,
    confirmation_pending: &PendingOperation,
    operation_registry: &OperationRegistry,
    state: &ResourceState,
    theme: &Theme,
) {
    let resource_type = &confirmation_pending.resource_type;
    let namespace = &confirmation_pending.namespace;
    let name = &confirmation_pending.name;
    let op_key = confirmation_pending.operation_key;

    if let Some(operation) = operation_registry.get_by_keybinding(op_key) {
        if let Some(resource) = state.get(&resource_key(namespace, name, resource_type)) {
            let msg = operation.confirmation_message(&resource);
            let lines = vec![
                Line::from(""),
                Line::from(vec![
                    ratatui::text::Span::styled("⚠ ", theme.operation_warning_style()),
                    ratatui::text::Span::styled(
                        "CONFIRMATION REQUIRED",
                        theme.operation_warning_style(),
                    ),
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
                .border_style(Style::default().fg(theme.operation_warning));
            let paragraph = Paragraph::new(lines)
                .block(block)
                .alignment(ratatui::layout::Alignment::Center);
            f.render_widget(paragraph, area);
            return;
        }
    }

    // Fallback if confirmation state is invalid
    let text = vec![Line::from("Invalid confirmation state")];
    let block = Block::default().title("Error").borders(Borders::ALL);
    let paragraph = Paragraph::new(text).block(block);
    f.render_widget(paragraph, area);
}
