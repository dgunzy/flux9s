//! The app view: what Flux objects deploy, as a tree — each Flux object,
//! its sources, what its inventory lists (nested Flux objects expanded),
//! and its workloads' pods, with each object's live health.

use crate::kube::app::{AppRow, AppSnapshot};
use crate::kube::kind_list::sanitize_cell;
use crate::kube::object_status::ObjectHealth;
use crate::tui::theme::Theme;
use crate::tui::views::columns::fit;
use crate::tui::views::inventory::health_style;
use crate::tui::views::props::{Fetched, ListCursor, held_widths};
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    widgets::{Cell, Row, Table},
};

/// What the app view shows: its title (a view's name, or the objects asked
/// for), the tree, and the rows left after the `/` filter.
pub struct AppTreeProps<'a> {
    pub title: &'a str,
    pub snapshot: Fetched<'a, AppSnapshot>,
    pub rows: &'a [&'a AppRow],
    pub filter: &'a str,
}

/// Render the app view.
pub fn render_app_tree(
    f: &mut Frame,
    area: Rect,
    props: AppTreeProps<'_>,
    cursor: ListCursor<'_>,
    theme: &Theme,
) {
    let AppTreeProps {
        title: what,
        snapshot: Fetched {
            data: snapshot,
            loading,
        },
        rows,
        filter,
    } = props;
    let ListCursor {
        selected: selected_index,
        offset: scroll_offset,
        widths,
    } = cursor;
    let Some(snapshot) = snapshot else {
        let message = if loading {
            format!("Reading what {what} deploys…")
        } else {
            format!("{what}: couldn't be read")
        };
        crate::tui::views::helpers::render_loading_state(f, area, what, &message, theme);
        return;
    };
    let title = title(what, snapshot, rows.len(), filter);
    if rows.is_empty() {
        let message = if filter.is_empty() {
            "Nothing found".to_string()
        } else {
            format!("Nothing matches '{filter}'")
        };
        crate::tui::views::helpers::render_empty_state(
            f,
            area,
            &title,
            &message,
            "The tree comes from each Flux object's inventory and sources",
            theme,
        );
        return;
    }

    let visible_height = (area.height as usize).saturating_sub(3);
    let viewport = crate::tui::views::scroll::Viewport::list_with_margin(
        selected_index,
        scroll_offset,
        rows.len(),
        visible_height,
        2,
    );
    let valid_selected = selected_index.min(rows.len().saturating_sub(1));
    let header = Row::new([
        "STATUS",
        "KIND",
        "NAMESPACE",
        "NAME",
        "ROLE",
        "READY",
        "MESSAGE",
    ])
    .style(
        Style::default()
            .fg(theme.table_header)
            .add_modifier(Modifier::BOLD),
    );
    // A filtered list loses the tree's shape: show kinds without indent.
    let indent = |row: &AppRow| {
        if !filter.is_empty() || row.depth == 0 {
            row.object.kind.clone()
        } else {
            format!("{}└ {}", "  ".repeat(row.depth - 1), row.object.kind)
        }
    };
    let table_rows: Vec<Row> = rows
        .iter()
        .enumerate()
        .skip(*scroll_offset)
        .take(visible_height)
        .map(|(index, row)| {
            let selected = index == valid_selected;
            // The kind's own word (Suspended, Available, CrashLoopBackOff),
            // coloured by its health.
            let status = Cell::from(sanitize_cell(row.status.label()));
            let status = if selected {
                status
            } else {
                status.style(health_style(row.status.health, theme))
            };
            Row::new(vec![
                status,
                Cell::from(indent(row)),
                Cell::from(if row.object.namespace.is_empty() {
                    "<cluster>".to_string()
                } else {
                    sanitize_cell(&row.object.namespace)
                }),
                Cell::from(sanitize_cell(&row.object.name)),
                Cell::from(row.role.label()),
                Cell::from(
                    row.status
                        .ready
                        .as_deref()
                        .map_or_else(|| "-".to_string(), sanitize_cell),
                ),
                Cell::from(sanitize_cell(&row.status.message)),
            ])
            .style(if selected {
                theme.table_selected_style()
            } else {
                Style::default().fg(theme.text_primary)
            })
        })
        .collect();

    let chars = |text: &str| text.chars().count();
    let natural = vec![
        fit("STATUS", rows.iter().map(|r| chars(r.status.label())), 18),
        fit("KIND", rows.iter().map(|r| chars(&indent(r))), 36),
        fit(
            "NAMESPACE",
            rows.iter().map(|r| chars(&r.object.namespace).max(9)),
            24,
        ),
        fit("NAME", rows.iter().map(|r| chars(&r.object.name)), 48),
        fit("ROLE", rows.iter().map(|r| chars(r.role.label())), 7),
        fit(
            "READY",
            rows.iter()
                .map(|r| r.status.ready.as_deref().map_or(1, chars)),
            9,
        ),
    ];
    // Held while the view is open, so a status or name changing doesn't
    // move the columns.
    let widths = held_widths(widths, &format!("app:{what}|{}", area.width), natural);
    let constraints: Vec<Constraint> = widths
        .into_iter()
        .map(Constraint::Length)
        .chain(std::iter::once(Constraint::Fill(1)))
        .collect();
    let block = crate::tui::views::helpers::create_themed_block(&title, theme);
    let table = Table::new(table_rows, constraints)
        .header(header)
        .block(block)
        .column_spacing(2);
    f.render_widget(table, area);
    viewport.render_scrollbar(f, area, theme);
}

/// `shop (24) · 2 failing · 1 in progress · 1 suspended · 1 note: … ·
/// truncated`: problems
/// right after the count, so a narrow terminal cuts the rest first.
fn title(what: &str, snapshot: &AppSnapshot, shown: usize, filter: &str) -> String {
    let count = if filter.is_empty() {
        format!("({shown})")
    } else {
        format!("({shown} of {})", snapshot.rows.len())
    };
    let mut title = format!("{what} {count}");
    let count_of = |health: &[ObjectHealth]| {
        snapshot
            .rows
            .iter()
            .filter(|row| health.contains(&row.status.health))
            .count()
    };
    for (n, label) in [
        (
            count_of(&[ObjectHealth::Failed, ObjectHealth::NotFound]),
            "failing",
        ),
        (
            count_of(&[ObjectHealth::InProgress, ObjectHealth::Terminating]),
            "in progress",
        ),
        (count_of(&[ObjectHealth::Suspended]), "suspended"),
    ] {
        if n > 0 {
            title.push_str(&format!(" · {n} {label}"));
        }
    }
    if let Some(note) = snapshot.notes.first() {
        let more = match snapshot.notes.len() {
            1 => String::new(),
            n => format!(" (+{} more)", n - 1),
        };
        title.push_str(&format!(" · {}{more}", sanitize_cell(note)));
    }
    if snapshot.truncated {
        title.push_str(" · truncated");
    }
    title
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kube::app::AppRole;
    use crate::kube::inventory::InventoryEntry;
    use crate::kube::object_status::ObjectStatus;
    use ratatui::{Terminal, backend::TestBackend};

    fn row(kind: &str, name: &str, depth: usize, role: AppRole, health: ObjectHealth) -> AppRow {
        AppRow {
            object: InventoryEntry {
                kind: kind.into(),
                namespace: "apps".into(),
                name: name.into(),
                api_version: "v1".into(),
            },
            depth,
            role,
            status: ObjectStatus::new(
                health,
                if health == ObjectHealth::Failed {
                    "Ready: 0/1"
                } else {
                    ""
                },
            ),
        }
    }

    fn render(snapshot: Option<&AppSnapshot>, loading: bool, filter: &str) -> String {
        let rows: Vec<&AppRow> = snapshot
            .map(|s| {
                s.rows
                    .iter()
                    .filter(|r| filter.is_empty() || r.object.name.contains(filter))
                    .collect()
            })
            .unwrap_or_default();
        let mut terminal = Terminal::new(TestBackend::new(130, 12)).unwrap();
        terminal
            .draw(|f| {
                let mut offset = 0;
                render_app_tree(
                    f,
                    f.area(),
                    AppTreeProps {
                        title: "shop",
                        snapshot: Fetched::new(snapshot, loading),
                        rows: &rows,
                        filter,
                    },
                    ListCursor::new(0, &mut offset),
                    &Theme::default(),
                );
            })
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .chunks(130)
            .map(|line| line.iter().map(|c| c.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn snapshot() -> AppSnapshot {
        AppSnapshot {
            rows: vec![
                row(
                    "Kustomization",
                    "shop",
                    0,
                    AppRole::App,
                    ObjectHealth::Current,
                ),
                row(
                    "GitRepository",
                    "shop",
                    1,
                    AppRole::Source,
                    ObjectHealth::Current,
                ),
                row(
                    "Deployment",
                    "web",
                    1,
                    AppRole::Applied,
                    ObjectHealth::Failed,
                ),
                row("Pod", "web-abc", 2, AppRole::Pod, ObjectHealth::InProgress),
            ],
            notes: vec!["HelmRelease apps/db: Helm storage forbidden".into()],
            truncated: false,
        }
    }

    #[test]
    fn the_tree_shows_each_object_under_the_one_that_brought_it() {
        let screen = render(Some(&snapshot()), false, "");
        assert!(
            screen.contains(
                "shop (4) · 1 failing · 1 in progress · HelmRelease apps/db: Helm storage forbidden"
            ),
            "{screen}"
        );
        assert!(
            screen.contains("└ GitRepository") && screen.contains("source"),
            "{screen}"
        );
        assert!(
            screen.contains("  └ Pod") && screen.contains("web-abc"),
            "{screen}"
        );
        assert!(screen.contains("Ready: 0/1"));
    }

    /// Where KIND starts on the Kustomization row, drawn with `widths`.
    fn kind_column(
        snapshot: &AppSnapshot,
        widths: &mut crate::tui::views::columns::StableWidths,
    ) -> usize {
        let rows: Vec<&AppRow> = snapshot.rows.iter().collect();
        let mut terminal = Terminal::new(TestBackend::new(130, 12)).unwrap();
        terminal
            .draw(|f| {
                let mut offset = 0;
                render_app_tree(
                    f,
                    f.area(),
                    AppTreeProps {
                        title: "shop",
                        snapshot: Fetched::new(Some(snapshot), false),
                        rows: &rows,
                        filter: "",
                    },
                    ListCursor::new(0, &mut offset).with_widths(widths),
                    &Theme::default(),
                );
            })
            .unwrap();
        let screen: Vec<String> = terminal
            .backend()
            .buffer()
            .content()
            .chunks(130)
            .map(|line| line.iter().map(|c| c.symbol()).collect())
            .collect();
        let line = screen.iter().find(|l| l.contains("Kustomization")).unwrap();
        line.find("Kustomization").unwrap()
    }

    #[test]
    fn columns_hold_still_while_values_change() {
        let mut widths = crate::tui::views::columns::StableWidths::default();
        let mut snapshot = snapshot();
        for row in &mut snapshot.rows {
            row.status = ObjectStatus::new(ObjectHealth::InProgress, "").with_state("Reconciling");
        }
        let before = kind_column(&snapshot, &mut widths);
        for row in &mut snapshot.rows {
            row.status = ObjectStatus::new(ObjectHealth::Current, "").with_state("Ready");
        }
        assert_eq!(
            kind_column(&snapshot, &mut widths),
            before,
            "STATUS didn't shrink"
        );
        // Sized to this frame alone, the column would have moved.
        let mut fresh = crate::tui::views::columns::StableWidths::default();
        assert!(kind_column(&snapshot, &mut fresh) < before);
    }

    #[test]
    fn rows_show_their_kinds_own_state_and_readiness() {
        let mut snapshot = snapshot();
        snapshot.rows[0].status = ObjectStatus::new(ObjectHealth::Suspended, "Applied revision")
            .with_state("Suspended")
            .with_ready("True");
        snapshot.rows[2].status = ObjectStatus::new(ObjectHealth::Current, "Replicas: 2/2")
            .with_state("Available")
            .with_ready("2/2");
        let screen = render(Some(&snapshot), false, "");
        assert!(screen.contains("· 1 suspended"), "{screen}");
        let lines: Vec<&str> = screen.lines().collect();
        let line = |name: &str| *lines.iter().find(|l| l.contains(name)).unwrap();
        assert!(
            line("Kustomization").contains("Suspended") && line("Kustomization").contains("True")
        );
        assert!(line("Deployment").contains("Available") && line("Deployment").contains("2/2"));
        assert!(
            line("GitRepository").contains("Current"),
            "a kind with no word of its own"
        );
    }

    #[test]
    fn filtering_flattens_the_tree_and_loading_says_so() {
        let screen = render(Some(&snapshot()), false, "web");
        assert!(screen.contains("shop (2 of 4)"), "{screen}");
        assert!(
            !screen.contains("└ Deployment"),
            "no tree lines once filtered"
        );
        assert!(render(None, true, "").contains("Reading what shop deploys"));
    }
}
