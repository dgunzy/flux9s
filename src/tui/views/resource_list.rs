//! Resource list view rendering

use crate::tui::app::state::SortField;
use crate::tui::theme::Theme;
use crate::tui::views::{extract_resource_specific_fields, get_resource_type_columns};
use crate::watcher::ResourceInfo;
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, Borders, Cell, Row, Table},
};
use std::cmp;
use std::collections::{HashMap, HashSet};

/// Decorate a header column name with a sort arrow when it is the active sort.
fn decorate_header(col: &str, sort_field: SortField, sort_reverse: bool, no_icons: bool) -> String {
    if sort_field.column_name() == Some(col) {
        let arrow = match (sort_reverse, no_icons) {
            (false, false) => "↑",
            (true, false) => "↓",
            (false, true) => "^",
            (true, true) => "v",
        };
        format!("{}{}", col, arrow)
    } else {
        col.to_string()
    }
}

/// Render the resource list table
pub fn render_resource_list(
    f: &mut Frame,
    area: Rect,
    resources: &[ResourceInfo],
    selected_index: usize,
    scroll_offset: &mut usize,
    selected_resource_type: &Option<String>,
    resource_objects: &HashMap<String, serde_json::Value>,
    theme: &Theme,
    no_icons: bool,
    favorites: &HashSet<String>,
    sort_field: SortField,
    sort_reverse: bool,
    // Contextual RBAC notice shown in place of the default empty-state message
    // when the list is empty because a kind is restricted (see App::access_notice).
    access_notice: Option<&str>,
) {
    let visible_height = (area.height as usize).saturating_sub(2);
    const SCROLL_BUFFER: usize = 2; // Keep 2 rows buffer before scrolling

    // Adjust scroll offset based on selected index with buffer
    crate::tui::views::helpers::update_scroll_offset(
        selected_index,
        visible_height,
        scroll_offset,
        SCROLL_BUFFER,
    );

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
        // An RBAC restriction explains the emptiness precisely; otherwise fall
        // back to the neutral "waiting" message (a missing CRD stays silent).
        let (message, instructions) = match access_notice {
            Some(notice) => (
                notice,
                "Grant RBAC access, switch namespace/context, or set ui.rbacWarnings=false to hide this.",
            ),
            None => ("No resources found", "Waiting for resources to appear..."),
        };
        crate::tui::views::helpers::render_empty_state(
            f,
            area,
            &format!("Resources ({})", resources.len()),
            message,
            instructions,
            theme,
        );
        return;
    }

    // Determine if we're in unified view or resource-type-specific view
    let is_unified = selected_resource_type.is_none();

    let (rows, header, constraints): (Vec<Row>, Row, Vec<Constraint>) = if is_unified {
        // Unified view: show common fields with status indicator
        let header_cells: Vec<String> = [
            "STATUS",
            "NAMESPACE",
            "NAME",
            "TYPE",
            "SUSPENDED",
            "READY",
            "AGE",
            "MESSAGE",
        ]
        .iter()
        .map(|col| decorate_header(col, sort_field, sort_reverse, no_icons))
        .collect();
        let header = Row::new(header_cells).style(
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
                let message_display = crate::tui::views::helpers::truncate_message(message, 40);

                // Check if resource is favorited
                let resource_key =
                    crate::watcher::resource_key(&r.namespace, &r.name, &r.resource_type);
                let is_favorite = favorites.contains(&resource_key);
                let name_display = if is_favorite {
                    format!("★ {}", r.name)
                } else {
                    r.name.clone()
                };
                let name_cell = if is_favorite {
                    Cell::from(Span::styled(
                        name_display,
                        Style::default()
                            .fg(theme.text_primary)
                            .add_modifier(Modifier::BOLD),
                    ))
                } else {
                    Cell::from(name_display)
                };

                Row::new(vec![
                    Cell::from(Span::styled(
                        status_indicator,
                        Style::default().fg(status_color),
                    )),
                    Cell::from(r.namespace.clone()),
                    name_cell,
                    Cell::from(r.resource_type.clone()),
                    Cell::from(suspended_str),
                    Cell::from(ready_str),
                    Cell::from(crate::tui::views::helpers::format_age(r.age)),
                    Cell::from(message_display),
                ])
                .style(style)
            })
            .collect();

        // Status column width: 7 chars fits "STATUS" plus a sort arrow
        let status_width = 7;
        let constraints: Vec<Constraint> = vec![
            Constraint::Length(status_width), // STATUS
            Constraint::Min(15),              // NAMESPACE
            Constraint::Min(30),              // NAME
            Constraint::Min(20),              // TYPE
            Constraint::Length(10),           // SUSPENDED
            Constraint::Length(6),            // READY
            Constraint::Length(7),            // AGE
            Constraint::Percentage(40),       // MESSAGE
        ];

        (rows, header, constraints)
    } else {
        // Resource-type-specific view: show type-specific fields (plus AGE)
        let resource_type = selected_resource_type.as_ref().unwrap();
        let mut column_names = get_resource_type_columns(resource_type);
        column_names.push("AGE");
        let header_cells: Vec<String> = column_names
            .iter()
            .map(|col| decorate_header(col, sort_field, sort_reverse, no_icons))
            .collect();
        let header = Row::new(header_cells).style(
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

                let (status_indicator, status_color) =
                    get_status_indicator(r.ready, r.suspended, theme, no_icons);

                // Get resource-specific fields from stored object
                let key = crate::watcher::resource_key(&r.namespace, &r.name, &r.resource_type);
                let specific_fields = resource_objects
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
                        "NAME" => {
                            let resource_key = crate::watcher::resource_key(
                                &r.namespace,
                                &r.name,
                                &r.resource_type,
                            );
                            let is_favorite = favorites.contains(&resource_key);
                            let name_display = if is_favorite {
                                format!("★ {}", r.name)
                            } else {
                                r.name.clone()
                            };
                            if is_favorite {
                                Cell::from(Span::styled(
                                    name_display,
                                    Style::default()
                                        .fg(theme.text_primary)
                                        .add_modifier(Modifier::BOLD),
                                ))
                            } else {
                                Cell::from(name_display)
                            }
                        }
                        "TYPE" => Cell::from(
                            specific_fields
                                .get("TYPE")
                                .cloned()
                                .unwrap_or_else(|| r.resource_type.clone()),
                        ),
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
                        "AGE" => Cell::from(crate::tui::views::helpers::format_age(r.age)),
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
                // 7 chars fits "STATUS" plus a sort arrow (and "PAUSED" in no-icons mode)
                "STATUS" => Constraint::Length(7),
                "NAMESPACE" => Constraint::Min(15),
                "NAME" => Constraint::Min(30),
                "TYPE" => Constraint::Min(20),
                "AGE" => Constraint::Length(7),
                "SUSPENDED" | "READY" => Constraint::Length(10),
                "REVISION" => Constraint::Min(20),
                "URL" | "PATH" | "CHART" | "IMAGE" | "SOURCE" | "ENDPOINT" | "ADDRESS"
                | "WEBHOOK" | "INPUTS" => Constraint::Min(30),
                "BRANCH" | "VERSION" | "PROVIDER" | "CHANNEL" => Constraint::Min(15),
                "PRUNE" => Constraint::Length(8),
                "MESSAGE" => Constraint::Percentage(40),
                _ => Constraint::Min(15),
            })
            .collect();

        (rows, header, constraints)
    };

    let total = resources.len();
    // Show visible row range in the title when the list is larger than one page.
    // scroll_offset is already updated by update_scroll_offset above.
    let range_suffix = if total > visible_height {
        let first = *scroll_offset + 1;
        let last = (*scroll_offset + visible_height).min(total);
        format!(" ─── {}-{}", first, last)
    } else {
        String::new()
    };

    let title = if let Some(rt) = selected_resource_type {
        format!("{} ({}){}", rt, total, range_suffix)
    } else {
        format!("All Resources ({}){}", total, range_suffix)
    };

    let table = Table::new(rows, constraints)
        .header(header)
        .block(Block::default().title(title).borders(Borders::ALL));

    f.render_widget(table, area);
}

pub fn get_status_indicator(
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
