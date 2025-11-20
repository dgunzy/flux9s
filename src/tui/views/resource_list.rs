//! Resource list view rendering

use crate::tui::theme::Theme;
use crate::tui::views::{extract_resource_specific_fields, get_resource_type_columns};
use crate::watcher::ResourceInfo;
use ratatui::{
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
    Frame,
};
use std::cmp;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Render the resource list table
pub fn render_resource_list(
    f: &mut Frame,
    area: Rect,
    resources: &[ResourceInfo],
    selected_index: usize,
    scroll_offset: &mut usize,
    selected_resource_type: &Option<String>,
    resource_objects: &Arc<RwLock<HashMap<String, serde_json::Value>>>,
    theme: &Theme,
    no_icons: bool,
) {
    let visible_height = (area.height as usize).saturating_sub(2);

    // Adjust scroll offset based on selected index
    if selected_index >= *scroll_offset + visible_height {
        *scroll_offset = selected_index.saturating_sub(visible_height - 1);
    }
    if selected_index < *scroll_offset {
        *scroll_offset = selected_index;
    }

    // Ensure selected_index is valid
    let valid_selected = if !resources.is_empty() {
        cmp::min(selected_index, resources.len().saturating_sub(1))
    } else {
        0
    };

    let visible_resources: Vec<_> = resources
        .iter()
        .skip(*scroll_offset)
        .take(visible_height)
        .collect();

    if visible_resources.is_empty() {
        let text = vec![
            ratatui::text::Line::from("No resources found"),
            ratatui::text::Line::from(""),
            ratatui::text::Line::from("Waiting for resources to appear..."),
        ];
        let block = Block::default()
            .title(format!("Resources ({})", resources.len()))
            .borders(Borders::ALL);
        let paragraph = Paragraph::new(text).block(block);
        f.render_widget(paragraph, area);
        return;
    }

    // Determine if we're in unified view or resource-type-specific view
    let is_unified = selected_resource_type.is_none();

    let (rows, header, constraints): (Vec<Row>, Row, Vec<Constraint>) = if is_unified {
        // Unified view: show common fields with status indicator
        let header = Row::new(vec![
            "STATUS",
            "NAMESPACE",
            "NAME",
            "TYPE",
            "SUSPENDED",
            "READY",
            "MESSAGE",
        ])
        .style(
            Style::default()
                .fg(theme.table_header)
                .add_modifier(Modifier::BOLD),
        );

        let rows: Vec<Row> = visible_resources
            .iter()
            .enumerate()
            .map(|(idx, r)| {
                let actual_idx = *scroll_offset + idx;
                let is_selected = actual_idx == valid_selected;

                let style = if is_selected {
                    theme.table_selected_style()
                } else {
                    Style::default()
                };

                let suspended_str = r
                    .suspended
                    .map(|s| if s { "True" } else { "False" })
                    .unwrap_or("?")
                    .to_string();

                let ready_str = r
                    .ready
                    .map(|r| if r { "True" } else { "False" })
                    .unwrap_or("?")
                    .to_string();

                // Status indicator
                let (status_indicator, status_color) =
                    get_status_indicator(r.ready, r.suspended, theme, no_icons);

                let message = r.message.as_deref().unwrap_or("-");
                let message_display = if message.len() > 40 {
                    format!("{}...", &message[..37])
                } else {
                    message.to_string()
                };

                Row::new(vec![
                    Cell::from(Span::styled(
                        status_indicator,
                        Style::default().fg(status_color),
                    )),
                    Cell::from(r.namespace.clone()),
                    Cell::from(r.name.clone()),
                    Cell::from(r.resource_type.clone()),
                    Cell::from(suspended_str),
                    Cell::from(ready_str),
                    Cell::from(message_display),
                ])
                .style(style)
            })
            .collect();

        let status_width = if no_icons { 6 } else { 3 }; // "PAUSED" vs "●"
        let constraints: Vec<Constraint> = vec![
            Constraint::Length(status_width), // STATUS
            Constraint::Min(15),              // NAMESPACE
            Constraint::Min(30),              // NAME
            Constraint::Min(20),              // TYPE
            Constraint::Length(10),           // SUSPENDED
            Constraint::Length(6),            // READY
            Constraint::Percentage(40),       // MESSAGE
        ];

        (rows, header, constraints)
    } else {
        // Resource-type-specific view: show type-specific fields
        let resource_type = selected_resource_type.as_ref().unwrap();
        let column_names = get_resource_type_columns(resource_type);
        let header = Row::new(column_names.clone()).style(
            Style::default()
                .fg(theme.table_header)
                .add_modifier(Modifier::BOLD),
        );

        let objects = resource_objects.read().unwrap();

        let rows: Vec<Row> = visible_resources
            .iter()
            .enumerate()
            .map(|(idx, r)| {
                let actual_idx = *scroll_offset + idx;
                let is_selected = actual_idx == valid_selected;

                let style = if is_selected {
                    theme.table_selected_style()
                } else {
                    Style::default()
                };

                let (status_indicator, status_color) =
                    get_status_indicator(r.ready, r.suspended, theme, no_icons);

                // Get resource-specific fields from stored object
                let key = crate::watcher::resource_key(&r.namespace, &r.name, &r.resource_type);
                let specific_fields = objects
                    .get(&key)
                    .map(|obj| extract_resource_specific_fields(resource_type, obj))
                    .unwrap_or_default();

                // Build row cells based on column names
                let mut cells = Vec::new();
                for col in &column_names {
                    let cell = match *col {
                        "STATUS" => Cell::from(Span::styled(
                            status_indicator,
                            Style::default().fg(status_color),
                        )),
                        "NAMESPACE" => Cell::from(r.namespace.clone()),
                        "NAME" => Cell::from(r.name.clone()),
                        "TYPE" => Cell::from(r.resource_type.clone()),
                        "SUSPENDED" => Cell::from(
                            r.suspended
                                .map(|s| {
                                    if s {
                                        "True".to_string()
                                    } else {
                                        "False".to_string()
                                    }
                                })
                                .unwrap_or_else(|| "?".to_string()),
                        ),
                        "READY" => Cell::from(
                            r.ready
                                .map(|r| {
                                    if r {
                                        "True".to_string()
                                    } else {
                                        "False".to_string()
                                    }
                                })
                                .unwrap_or_else(|| "?".to_string()),
                        ),
                        "REVISION" => Cell::from(r.revision.clone().unwrap_or("-".to_string())),
                        "MESSAGE" => {
                            let msg = r.message.as_deref().unwrap_or("-");
                            let display = if msg.len() > 50 {
                                format!("{}...", &msg[..47])
                            } else {
                                msg.to_string()
                            };
                            Cell::from(display)
                        }
                        _ => Cell::from(
                            specific_fields
                                .get(*col)
                                .cloned()
                                .unwrap_or("-".to_string()),
                        ),
                    };
                    cells.push(cell);
                }

                Row::new(cells).style(style)
            })
            .collect();

        // Build constraints based on column names
        let constraints: Vec<Constraint> = column_names
            .iter()
            .map(|col| match *col {
                "STATUS" => Constraint::Length(if no_icons { 6 } else { 3 }),
                "NAMESPACE" => Constraint::Min(15),
                "NAME" => Constraint::Min(30),
                "TYPE" => Constraint::Min(20),
                "SUSPENDED" | "READY" => Constraint::Length(10),
                "REVISION" => Constraint::Min(20),
                "URL" | "PATH" | "CHART" | "IMAGE" | "SOURCE" => Constraint::Min(30),
                "BRANCH" | "VERSION" => Constraint::Min(15),
                "PRUNE" => Constraint::Length(8),
                "MESSAGE" => Constraint::Percentage(40),
                _ => Constraint::Min(15),
            })
            .collect();

        (rows, header, constraints)
    };

    let title = if let Some(ref rt) = selected_resource_type {
        format!("{} ({})", rt, resources.len())
    } else {
        format!("All Resources ({})", resources.len())
    };

    let table = Table::new(rows, constraints)
        .header(header)
        .block(Block::default().title(title).borders(Borders::ALL));

    f.render_widget(table, area);
}

fn get_status_indicator(
    ready: Option<bool>,
    suspended: Option<bool>,
    theme: &Theme,
    no_icons: bool,
) -> (&'static str, Color) {
    if no_icons {
        // Use text alternatives when icons are disabled
        match (ready, suspended) {
            (Some(true), Some(false)) => ("OK", theme.status_ready),
            (Some(true), Some(true)) => ("PAUSED", theme.status_suspended),
            (Some(false), _) => ("ERR", theme.status_error),
            (None, Some(true)) => ("PAUSED", theme.status_suspended),
            _ => ("?", theme.status_unknown),
        }
    } else {
        // Use Unicode icons when enabled
        match (ready, suspended) {
            (Some(true), Some(false)) => ("●", theme.status_ready),
            (Some(true), Some(true)) => ("⏸", theme.status_suspended),
            (Some(false), _) => ("✗", theme.status_error),
            (None, Some(true)) => ("⏸", theme.status_suspended),
            _ => ("○", theme.status_unknown),
        }
    }
}
