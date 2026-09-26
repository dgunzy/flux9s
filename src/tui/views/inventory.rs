//! Inventory drill-down view (#245)
//!
//! Shows the members of a graph ResourceGroup node — the resources a Flux
//! object owns that aren't workloads or Flux resources — broken down by kind,
//! namespace, and name, with each object's health (#262) fetched once on
//! entry. These objects aren't watched; Enter/d describes and y shows the
//! YAML of the selected one.

use crate::kube::inventory::InventoryEntry;
use crate::kube::object_status::{ObjectHealth, ObjectStatus};
use crate::tui::theme::Theme;
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    widgets::{Cell, Row, Table},
};
use std::cmp;

/// Render the inventory list (the drilled-into ResourceGroup's members).
pub fn render_inventory_list(
    f: &mut Frame,
    area: Rect,
    rows: &[InventoryEntry],
    statuses: Option<&[ObjectStatus]>,
    statuses_loading: bool,
    selected_index: usize,
    scroll_offset: &mut usize,
    theme: &Theme,
) {
    let visible_height = (area.height as usize).saturating_sub(2);
    const SCROLL_BUFFER: usize = 2;
    crate::tui::views::helpers::update_scroll_offset(
        selected_index,
        visible_height,
        scroll_offset,
        SCROLL_BUFFER,
    );

    let title = inventory_title(rows);
    if rows.is_empty() {
        crate::tui::views::helpers::render_empty_state(
            f,
            area,
            &title,
            "No resources",
            "Open a graph resource group to populate this view",
            theme,
        );
        return;
    }

    let valid_selected = cmp::min(selected_index, rows.len().saturating_sub(1));
    let header = Row::new([
        "KIND",
        "NAMESPACE",
        "NAME",
        "STATUS",
        "API VERSION",
        "MESSAGE",
    ])
    .style(
        Style::default()
            .fg(theme.table_header)
            .add_modifier(Modifier::BOLD),
    );

    let table_rows: Vec<Row> = rows
        .iter()
        .skip(*scroll_offset)
        .take(visible_height)
        .enumerate()
        .map(|(idx, row)| {
            let index = *scroll_offset + idx;
            let selected = index == valid_selected;
            let style = if selected {
                theme.table_selected_style()
            } else {
                Style::default().fg(theme.text_primary)
            };
            let status = statuses.and_then(|s| s.get(index));
            let (status_text, message) = match status {
                Some(status) => (status.health.label(), status.message.as_str()),
                None if statuses_loading => ("…", ""),
                None => ("-", ""),
            };
            // The selected row keeps one uniform highlight; otherwise the
            // status cell carries the health colour.
            let status_cell = match status {
                Some(status) if !selected => {
                    Cell::from(status_text).style(health_style(status.health, theme))
                }
                _ => Cell::from(status_text),
            };
            Row::new(vec![
                Cell::from(row.kind.clone()),
                // Cluster-scoped resources have no namespace of their own.
                Cell::from(if row.namespace.is_empty() {
                    "<cluster>".to_string()
                } else {
                    row.namespace.clone()
                }),
                Cell::from(row.name.clone()),
                status_cell,
                Cell::from(row.api_version.clone()),
                Cell::from(message.lines().next().unwrap_or_default().to_string()),
            ])
            .style(style)
        })
        .collect();

    // KIND and API VERSION size to their longest value so CRD groups
    // (`gateway.networking.k8s.io/v1`) aren't truncated, capped so one long
    // group can't squeeze NAME and MESSAGE off screen.
    let kind_width = column_width("KIND", rows.iter().map(|r| r.kind.as_str()), 28);
    let api_width = column_width(
        "API VERSION",
        rows.iter().map(|r| r.api_version.as_str()),
        36,
    );
    let constraints = [
        Constraint::Length(kind_width), // KIND
        Constraint::Length(18),         // NAMESPACE
        Constraint::Min(20),            // NAME
        Constraint::Length(12),         // STATUS
        Constraint::Length(api_width),  // API VERSION
        Constraint::Min(20),            // MESSAGE
    ];

    let block = crate::tui::views::helpers::create_themed_block(&title, theme);
    let table = Table::new(table_rows, constraints)
        .header(header)
        .block(block);
    f.render_widget(table, area);
}

/// Width of a column holding `values`: the longest value or the header,
/// whichever is wider, capped at `max`.
fn column_width<'a>(header: &str, values: impl Iterator<Item = &'a str>, max: u16) -> u16 {
    let widest = values
        .map(|v| v.chars().count())
        .chain(std::iter::once(header.chars().count()))
        .max()
        .unwrap_or_default();
    u16::try_from(widest).unwrap_or(max).min(max)
}

/// Theme style for a health value.
fn health_style(health: ObjectHealth, theme: &Theme) -> Style {
    match health {
        ObjectHealth::Current => theme.status_ready_style(),
        ObjectHealth::InProgress | ObjectHealth::Terminating => {
            Style::default().fg(theme.status_pending)
        }
        ObjectHealth::Failed | ObjectHealth::NotFound => theme.status_error_style(),
        ObjectHealth::Forbidden | ObjectHealth::Unknown => {
            Style::default().fg(theme.status_unknown)
        }
    }
}

/// Title summarising the breakdown: total, then the distinct kinds and counts
/// (the same summary the graph node itself shows).
fn inventory_title(rows: &[InventoryEntry]) -> String {
    let counts = crate::kube::inventory::kind_counts(rows);
    if counts.is_empty() {
        return "Resources (0)".to_string();
    }
    let summary = counts
        .iter()
        .map(|(kind, count)| format!("{}: {}", kind, count))
        .collect::<Vec<_>>()
        .join(", ");
    format!("Resources ({}) - {}", rows.len(), summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn entry(kind: &str, namespace: &str, name: &str) -> InventoryEntry {
        InventoryEntry {
            kind: kind.to_string(),
            name: name.to_string(),
            namespace: namespace.to_string(),
            api_version: "v1".to_string(),
        }
    }

    fn render(rows: &[InventoryEntry]) -> String {
        render_with(rows, None, false)
    }

    fn render_with(
        rows: &[InventoryEntry],
        statuses: Option<&[ObjectStatus]>,
        loading: bool,
    ) -> String {
        let backend = TestBackend::new(140, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut scroll = 0usize;
        terminal
            .draw(|f| {
                render_inventory_list(
                    f,
                    f.area(),
                    rows,
                    statuses,
                    loading,
                    0,
                    &mut scroll,
                    &Theme::default(),
                );
            })
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<Vec<_>>()
            .chunks(140)
            .map(|row| row.concat())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn title_summarises_totals_by_kind() {
        let rows = vec![
            entry("ConfigMap", "app", "cm"),
            entry("Service", "app", "svc-a"),
            entry("Service", "app", "svc-b"),
        ];
        assert_eq!(
            inventory_title(&rows),
            "Resources (3) - ConfigMap: 1, Service: 2"
        );
        assert_eq!(inventory_title(&[]), "Resources (0)");
    }

    #[test]
    fn rows_show_kind_namespace_and_name() {
        let output = render(&[
            entry("ConfigMap", "cabot-book", "cabot-book-config"),
            entry("Namespace", "", "cabot-book"),
        ]);
        assert!(output.contains("KIND"));
        assert!(output.contains("NAMESPACE"));
        assert!(output.contains("cabot-book-config"));
        assert!(output.contains("cabot-book"));
        // Cluster-scoped entries are labelled rather than left blank.
        assert!(output.contains("<cluster>"));
    }

    #[test]
    fn empty_inventory_renders_an_empty_state() {
        let output = render(&[]);
        assert!(output.contains("No resources"));
    }

    #[test]
    fn status_column_shows_health_and_message() {
        let rows = vec![
            entry("ConfigMap", "app", "cm"),
            entry("Service", "app", "web"),
        ];
        let statuses = vec![
            ObjectStatus::new(ObjectHealth::Current, ""),
            ObjectStatus::new(
                ObjectHealth::InProgress,
                "Waiting for load balancer address",
            ),
        ];
        let output = render_with(&rows, Some(&statuses), false);
        assert!(output.contains("STATUS"));
        assert!(output.contains("Current"));
        assert!(output.contains("InProgress"));
        assert!(output.contains("Waiting for load balancer"));
    }

    #[test]
    fn status_column_shows_loading_placeholder() {
        let output = render_with(&[entry("ConfigMap", "app", "cm")], None, true);
        assert!(output.contains("…"));
    }

    #[test]
    fn long_api_versions_are_not_truncated() {
        let mut route = entry("HTTPRoute", "app", "web");
        route.api_version = "gateway.networking.k8s.io/v1".to_string();
        let output = render(&[route]);
        assert!(output.contains("gateway.networking.k8s.io/v1"));
    }

    #[test]
    fn column_width_fits_content_within_cap() {
        assert_eq!(column_width("KIND", ["Service"].into_iter(), 28), 7);
        assert_eq!(column_width("API VERSION", ["v1"].into_iter(), 36), 11);
        assert_eq!(
            column_width("KIND", ["x".repeat(50).as_str()].into_iter(), 28),
            28
        );
    }
}
