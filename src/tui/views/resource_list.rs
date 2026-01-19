//! Resource list view rendering

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
    plugin_registry: Option<&crate::plugins::PluginRegistry>,
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
        crate::tui::views::helpers::render_empty_state(
            f,
            area,
            &format!("Resources ({})", resources.len()),
            "No resources found",
            "Waiting for resources to appear...",
            theme,
        );
        return;
    }

    // Determine if we're in unified view or resource-type-specific view
    let is_unified = selected_resource_type.is_none();

    // Store header column names outside the if/else to keep them alive
    let header_cols: Option<Vec<String>> = if !is_unified {
        Some(get_resource_type_columns(
            selected_resource_type.as_ref().unwrap(),
            plugin_registry,
        ))
    } else {
        None
    };

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
                    Cell::from(message_display),
                ])
                .style(style)
            })
            .collect();

        // Status column width: "STATUS" header needs 6 chars (icon is only 1 char, so fits fine)
        let status_width = 6;
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
        let column_names = header_cols.as_ref().unwrap();

        // Get plugin column config if this is a plugin resource
        let plugin_column_config: Option<(
            &crate::plugins::manifest::PluginManifest,
            &crate::plugins::manifest::WatchedResourceConfig,
        )> = plugin_registry
            .and_then(|reg| reg.get_watched_resource_for_display_name(resource_type));

        // Create header from column names (header_cols is kept alive in outer scope)
        let header = Row::new(column_names.iter().map(|s| s.as_str()).collect::<Vec<_>>()).style(
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
                    .map(|obj| {
                        extract_resource_specific_fields(resource_type, obj, plugin_registry)
                    })
                    .unwrap_or_default();

                // Build row cells based on column names
                let mut cells = Vec::new();
                for col in column_names.iter() {
                    let cell = match col.as_str() {
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
                        _ => {
                            // Check if this is a plugin column with a renderer
                            if let Some((_, watched_config)) = plugin_column_config {
                                if let Some(column_config) =
                                    watched_config.columns.iter().find(|c| c.name == *col)
                                {
                                    let value = specific_fields
                                        .get(col)
                                        .cloned()
                                        .unwrap_or("-".to_string());
                                    let span = crate::plugins::render_column_value(
                                        &value,
                                        &column_config.renderer,
                                        theme,
                                    );
                                    Cell::from(span)
                                } else {
                                    Cell::from(
                                        specific_fields
                                            .get(col)
                                            .cloned()
                                            .unwrap_or("-".to_string()),
                                    )
                                }
                            } else {
                                Cell::from(
                                    specific_fields.get(col).cloned().unwrap_or("-".to_string()),
                                )
                            }
                        }
                    };
                    cells.push(cell);
                }

                Row::new(cells).style(style)
            })
            .collect();

        // Build constraints based on column names
        let constraints: Vec<Constraint> = column_names
            .iter()
            .map(|col| {
                // Check if this is a plugin column with width defined
                if let Some((_, watched_config)) = plugin_column_config {
                    if let Some(column_config) =
                        watched_config.columns.iter().find(|c| c.name == *col)
                    {
                        return Constraint::Length(column_config.width);
                    }
                }
                // Default constraints for known columns
                match col.as_str() {
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
                }
            })
            .collect();

        (rows, header, constraints)
    };

    let title = if let Some(rt) = selected_resource_type {
        format!("{} ({})", rt, resources.len())
    } else {
        format!("All Resources ({})", resources.len())
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
