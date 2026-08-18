//! Inventory drill-down view (#245)
//!
//! Shows the members of a graph ResourceGroup node — the resources a Flux
//! object owns that aren't workloads or Flux resources — broken down by kind,
//! namespace, and name. Read-only: these resources aren't watched by flux9s,
//! so the view exists to answer "what does this own, and where does it live?".

use crate::kube::inventory::InventoryEntry;
use crate::tui::theme::Theme;
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    widgets::{Row, Table},
};
use std::cmp;

/// Render the inventory list (the drilled-into ResourceGroup's members).
pub fn render_inventory_list(
    f: &mut Frame,
    area: Rect,
    rows: &[InventoryEntry],
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
    let header = Row::new(["KIND", "NAMESPACE", "NAME", "API VERSION"]).style(
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
            let style = if *scroll_offset + idx == valid_selected {
                theme.table_selected_style()
            } else {
                Style::default().fg(theme.text_primary)
            };
            Row::new(vec![
                row.kind.clone(),
                // Cluster-scoped resources have no namespace of their own.
                if row.namespace.is_empty() {
                    "<cluster>".to_string()
                } else {
                    row.namespace.clone()
                },
                row.name.clone(),
                row.api_version.clone(),
            ])
            .style(style)
        })
        .collect();

    let constraints = [
        Constraint::Length(28), // KIND
        Constraint::Length(24), // NAMESPACE
        Constraint::Min(24),    // NAME
        Constraint::Length(28), // API VERSION
    ];

    let block = crate::tui::views::helpers::create_themed_block(&title, theme);
    let table = Table::new(table_rows, constraints)
        .header(header)
        .block(block);
    f.render_widget(table, area);
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
        let backend = TestBackend::new(100, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut scroll = 0usize;
        terminal
            .draw(|f| {
                render_inventory_list(f, f.area(), rows, 0, &mut scroll, &Theme::default());
            })
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<Vec<_>>()
            .chunks(100)
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
}
