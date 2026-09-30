//! `:<kind>` list view (#267)
//!
//! A table of any kind's objects: NAMESPACE (when showing all namespaces),
//! NAME, then the kind's columns — STATUS/HEALTH coloured by kstatus health.
//! Stateless: the app passes the spec, snapshot, and filtered rows.

use crate::kube::kind_list::{KindListSnapshot, KindRow};
use crate::kube::object_status::ObjectHealth;
use crate::models::kinds::{BuiltinColumn, Column, KindSpec};
use crate::tui::theme::Theme;
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    widgets::{Cell, Row, Table},
};

/// Theme style for a health value (shared palette with the inventory view).
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

fn is_health_column(column: &Column) -> bool {
    matches!(
        column,
        Column::Builtin(BuiltinColumn::Status | BuiltinColumn::Health)
    )
}

/// Width for a column: its widest value or header, capped.
fn width(header: &str, values: impl Iterator<Item = usize>, cap: usize) -> u16 {
    let widest = values
        .chain(std::iter::once(header.len()))
        .max()
        .unwrap_or(0);
    u16::try_from(widest.min(cap)).unwrap_or(u16::MAX)
}

/// Render the kind list.
pub fn render_kind_list(
    f: &mut Frame,
    area: Rect,
    spec: Option<&KindSpec>,
    namespace: &Option<String>,
    snapshot: Option<&KindListSnapshot>,
    rows: &[&KindRow],
    error: Option<&str>,
    loading: bool,
    selected_index: usize,
    scroll_offset: &mut usize,
    theme: &Theme,
) {
    let Some(spec) = spec else {
        crate::tui::views::helpers::render_empty_state(
            f,
            area,
            "Kinds",
            "No kind selected",
            "Type :<kind> (e.g. :deploy, :pods, :svc) to browse any resource",
            theme,
        );
        return;
    };
    let scope = match (spec.is_namespaced(), namespace) {
        (false, _) => "cluster-scoped".to_string(),
        (true, Some(ns)) => format!("ns: {ns}"),
        (true, None) => "all namespaces".to_string(),
    };
    let mut title = format!("{} ({}) [{}]", spec.names.plural, rows.len(), scope);
    if let Some(snapshot) = snapshot.filter(|s| s.truncated()) {
        title.push_str(&format!(
            " — first {} of {}; narrow with :ns or /",
            snapshot.rows.len(),
            snapshot.total
        ));
    }

    if let Some(error) = error {
        crate::tui::views::helpers::render_empty_state(
            f,
            area,
            &title,
            error,
            ":flux returns to the Flux view",
            theme,
        );
        return;
    }
    let Some(snapshot) = snapshot else {
        let message = if loading {
            format!("Listing {}…", spec.names.plural)
        } else {
            "Not loaded".to_string()
        };
        crate::tui::views::helpers::render_loading_state(f, area, &title, &message, theme);
        return;
    };
    if rows.is_empty() {
        crate::tui::views::helpers::render_empty_state(
            f,
            area,
            &title,
            &format!("No {} found", spec.names.plural),
            ":ns switches namespace · :flux returns to the Flux view",
            theme,
        );
        return;
    }

    let visible_height = (area.height as usize).saturating_sub(3);
    crate::tui::views::helpers::update_scroll_offset(
        selected_index,
        visible_height,
        scroll_offset,
        2,
    );
    let selected = selected_index.min(rows.len().saturating_sub(1));
    let show_namespace = spec.is_namespaced() && namespace.is_none();

    let mut headers: Vec<String> = Vec::new();
    let mut constraints: Vec<Constraint> = Vec::new();
    if show_namespace {
        headers.push("NAMESPACE".into());
        constraints.push(Constraint::Length(width(
            "NAMESPACE",
            rows.iter().map(|r| r.namespace.len()),
            28,
        )));
    }
    headers.push("NAME".into());
    constraints.push(Constraint::Length(width(
        "NAME",
        rows.iter().map(|r| r.name.len()),
        48,
    )));
    let last = snapshot.columns.len().saturating_sub(1);
    for (i, column) in snapshot.columns.iter().enumerate() {
        let header = column.header();
        constraints.push(if i == last {
            Constraint::Min(10)
        } else {
            Constraint::Length(width(
                &header,
                rows.iter()
                    .map(|r| r.cells.get(i).map_or(0, |c| c.chars().count())),
                32,
            ))
        });
        headers.push(header);
    }

    let table_rows: Vec<Row> = rows
        .iter()
        .enumerate()
        .skip(*scroll_offset)
        .take(visible_height)
        .map(|(index, row)| {
            let is_selected = index == selected;
            let mut cells: Vec<Cell> = Vec::new();
            if show_namespace {
                cells.push(Cell::from(row.namespace.clone()));
            }
            cells.push(Cell::from(row.name.clone()));
            for (column, value) in snapshot.columns.iter().zip(&row.cells) {
                let cell = Cell::from(value.clone());
                cells.push(if is_health_column(column) && !is_selected {
                    cell.style(health_style(row.health, theme))
                } else {
                    cell
                });
            }
            let style = if is_selected {
                theme.table_selected_style()
            } else {
                Style::default().fg(theme.text_primary)
            };
            Row::new(cells).style(style)
        })
        .collect();

    let header = Row::new(headers).style(
        Style::default()
            .fg(theme.table_header)
            .add_modifier(Modifier::BOLD),
    );
    let block = crate::tui::views::helpers::create_themed_block(&title, theme);
    let table = Table::new(table_rows, constraints)
        .header(header)
        .block(block);
    f.render_widget(table, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::kinds::{Gvk, KindScope};
    use ratatui::{Terminal, backend::TestBackend};

    fn spec(scope: KindScope) -> KindSpec {
        KindSpec::generic(
            Gvk {
                group: "apps".into(),
                version: "v1".into(),
                kind: "Deployment".into(),
            },
            scope,
            "deployments".into(),
            vec![],
        )
    }

    fn row(ns: &str, name: &str, health: ObjectHealth) -> KindRow {
        KindRow {
            namespace: ns.into(),
            name: name.into(),
            health,
            cells: vec![health.label().into(), "3d".into(), "Replicas: 1/1".into()],
            created: None,
            ownership: crate::kube::ownership::Ownership::Unmanaged,
        }
    }

    fn render(
        spec: &KindSpec,
        namespace: Option<&str>,
        rows: &[KindRow],
        error: Option<&str>,
    ) -> String {
        let snapshot = KindListSnapshot {
            columns: spec.columns.clone(),
            rows: rows.to_vec(),
            total: rows.len(),
        };
        let refs: Vec<&KindRow> = snapshot.rows.iter().collect();
        let mut terminal = Terminal::new(TestBackend::new(110, 10)).unwrap();
        let mut scroll = 0;
        terminal
            .draw(|f| {
                render_kind_list(
                    f,
                    f.area(),
                    Some(spec),
                    &namespace.map(String::from),
                    Some(&snapshot),
                    &refs,
                    error,
                    false,
                    0,
                    &mut scroll,
                    &Theme::default(),
                )
            })
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect::<Vec<_>>()
            .chunks(110)
            .map(|r| r.concat())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn all_namespaces_list_shows_namespace_column() {
        let out = render(
            &spec(KindScope::Namespaced),
            None,
            &[row("apps", "web", ObjectHealth::Current)],
            None,
        );
        assert!(out.contains("deployments (1) [all namespaces]"));
        assert!(out.contains("NAMESPACE"));
        assert!(out.contains("web"));
        assert!(out.contains("Current"));
        assert!(out.contains("Replicas: 1/1"));
    }

    #[test]
    fn namespaced_and_cluster_scoped_titles_drop_the_namespace_column() {
        let out = render(
            &spec(KindScope::Namespaced),
            Some("apps"),
            &[row("apps", "web", ObjectHealth::Current)],
            None,
        );
        assert!(out.contains("[ns: apps]"));
        assert!(!out.contains("NAMESPACE"));
        let out = render(
            &spec(KindScope::Cluster),
            Some("apps"),
            &[row("", "n1", ObjectHealth::Current)],
            None,
        );
        assert!(out.contains("[cluster-scoped]"));
    }

    #[test]
    fn errors_and_empty_lists_explain_the_way_back() {
        let out = render(
            &spec(KindScope::Namespaced),
            None,
            &[],
            Some("Forbidden: cannot list deployments"),
        );
        assert!(out.contains("Forbidden"));
        assert!(out.contains(":flux"));
        let out = render(&spec(KindScope::Namespaced), Some("x"), &[], None);
        assert!(out.contains("No deployments found"));
    }

    #[test]
    fn truncated_lists_say_so_in_the_title() {
        let s = spec(KindScope::Namespaced);
        let snapshot = KindListSnapshot {
            columns: s.columns.clone(),
            rows: vec![row("apps", "web", ObjectHealth::Current)],
            total: 9_000,
        };
        let refs: Vec<&KindRow> = snapshot.rows.iter().collect();
        let mut terminal = Terminal::new(TestBackend::new(140, 6)).unwrap();
        let mut scroll = 0;
        terminal
            .draw(|f| {
                render_kind_list(
                    f,
                    f.area(),
                    Some(&s),
                    &None,
                    Some(&snapshot),
                    &refs,
                    None,
                    false,
                    0,
                    &mut scroll,
                    &Theme::default(),
                )
            })
            .unwrap();
        let out: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(out.contains("first 1 of 9000; narrow with :ns or /"));
    }
}
