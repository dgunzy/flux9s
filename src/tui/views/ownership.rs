//! The unmanaged / orphaned list (#274): objects of any kind with where
//! they stand with Flux and why. Enter/d describes the selected one, y shows
//! its YAML, and `m` / `t` go to the Flux object an orphan names.

use crate::kube::kind_list::sanitize_cell;
use crate::kube::standing::Standing;
use crate::kube::sweep::{SweepRequest, SweepResult, SweepRow, describe_request};
use crate::tui::theme::Theme;
use crate::tui::views::props::ListCursor;
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    widgets::{Cell, Row, Table},
};

/// What the unmanaged / orphaned list shows: the sweep asked for, its result and filtered rows.
pub struct OwnershipListProps<'a> {
    pub request: Option<&'a SweepRequest>,
    pub result: Option<&'a SweepResult>,
    pub rows: &'a [&'a SweepRow],
    pub loading: bool,
    pub filter: &'a str,
}

/// Render the ownership list.
pub fn render_ownership_list(
    f: &mut Frame,
    area: Rect,
    props: OwnershipListProps<'_>,
    cursor: ListCursor<'_>,
    theme: &Theme,
) {
    let OwnershipListProps {
        request,
        result,
        rows,
        loading,
        filter,
    } = props;
    let ListCursor {
        selected: selected_index,
        offset: scroll_offset,
    } = cursor;
    let what = request.map_or_else(|| "Ownership".to_string(), describe_request);
    let Some(result) = result else {
        let message = if loading {
            format!("{what}: listing every kind…")
        } else {
            format!("{what}: the sweep didn't finish")
        };
        crate::tui::views::helpers::render_loading_state(f, area, &what, &message, theme);
        return;
    };

    let title = title(&what, result, rows.len(), filter);
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
            "Ctrl-r rescans · ownership.ignore in the config hides expected objects",
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
    let header = Row::new(["STANDING", "KIND", "NAMESPACE", "NAME", "AGE", "DETAIL"]).style(
        Style::default()
            .fg(theme.table_header)
            .add_modifier(Modifier::BOLD),
    );
    let now = chrono::Utc::now();
    let table_rows: Vec<Row> = rows
        .iter()
        .enumerate()
        .skip(*scroll_offset)
        .take(visible_height)
        .map(|(index, row)| {
            let selected = index == valid_selected;
            let standing = Cell::from(row.standing.label());
            let standing = if selected {
                standing
            } else {
                standing.style(standing_style(&row.standing, theme))
            };
            Row::new(vec![
                standing,
                Cell::from(row.kind.clone()),
                Cell::from(if row.namespace.is_empty() {
                    "<cluster>".to_string()
                } else {
                    sanitize_cell(&row.namespace)
                }),
                Cell::from(sanitize_cell(&row.name)),
                Cell::from(
                    row.created
                        .map(|t| crate::age::compact(now - t))
                        .unwrap_or_default(),
                ),
                Cell::from(sanitize_cell(&row.standing.detail())),
            ])
            .style(if selected {
                theme.table_selected_style()
            } else {
                Style::default().fg(theme.text_primary)
            })
        })
        .collect();

    let width = |header: &str, values: &mut dyn Iterator<Item = usize>, max: u16| -> u16 {
        let widest = values
            .chain(std::iter::once(header.len()))
            .max()
            .unwrap_or(0);
        u16::try_from(widest).unwrap_or(max).min(max)
    };
    let constraints = [
        Constraint::Length(9),
        Constraint::Length(width(
            "KIND",
            &mut rows.iter().map(|r| r.kind.chars().count()),
            28,
        )),
        Constraint::Length(width(
            "NAMESPACE",
            &mut rows.iter().map(|r| r.namespace.chars().count().max(9)),
            24,
        )),
        Constraint::Length(width(
            "NAME",
            &mut rows.iter().map(|r| r.name.chars().count()),
            48,
        )),
        Constraint::Length(4),
        Constraint::Fill(1),
    ];
    let block = crate::tui::views::helpers::create_themed_block(&title, theme);
    let table = Table::new(table_rows, constraints)
        .header(header)
        .block(block)
        .column_spacing(2);
    f.render_widget(table, area);
    viewport.render_scrollbar(f, area, theme);
}

/// `Not managed by Flux in apps (12) · 41 kinds · 2 unreadable: secrets, …`.
fn title(what: &str, result: &SweepResult, shown: usize, filter: &str) -> String {
    let count = if filter.is_empty() {
        format!("({})", result.rows.len())
    } else {
        format!("({shown} of {})", result.rows.len())
    };
    let mut title = format!("{what} {count} · {} kinds", result.kinds_scanned);
    if !result.unreadable.is_empty() {
        let names = result
            .unreadable
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        let more = if result.unreadable.len() > 3 {
            ", …"
        } else {
            ""
        };
        title.push_str(&format!(
            " · {} unreadable: {names}{more}",
            result.unreadable.len()
        ));
    }
    if result.truncated {
        title.push_str(" · truncated");
    }
    title
}

fn standing_style(standing: &Standing, theme: &Theme) -> Style {
    match standing {
        Standing::Orphaned(..) => theme.status_error_style(),
        Standing::Unmanaged { .. } => Style::default().fg(theme.status_pending),
        Standing::Flux(_) => theme.status_ready_style(),
        Standing::Other(_) | Standing::System(_) => Style::default().fg(theme.text_secondary),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kube::ownership::Manager;
    use crate::kube::standing::OrphanReason;
    use crate::kube::sweep::SweepScope;
    use ratatui::{Terminal, backend::TestBackend};

    fn row(name: &str, standing: Standing) -> SweepRow {
        SweepRow {
            api_version: "v1".into(),
            kind: "ConfigMap".into(),
            namespace: "apps".into(),
            name: name.into(),
            standing,
            created: None,
        }
    }

    fn render(result: Option<&SweepResult>, loading: bool) -> String {
        let request = SweepRequest {
            scope: SweepScope::Unmanaged {
                include_other: false,
            },
            namespace: Some("apps".into()),
            config: Default::default(),
        };
        let backend = TestBackend::new(140, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let rows: Vec<&SweepRow> = result.map(|r| r.rows.iter().collect()).unwrap_or_default();
        terminal
            .draw(|f| {
                let mut offset = 0;
                render_ownership_list(
                    f,
                    f.area(),
                    OwnershipListProps {
                        request: Some(&request),
                        result,
                        rows: &rows,
                        loading,
                        filter: "",
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
            .chunks(140)
            .map(|line| line.iter().map(|c| c.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn rows_show_standing_and_why() {
        let ks = Manager::Flux {
            kind: crate::models::FluxResourceKind::Kustomization,
            namespace: "flux-system".into(),
            name: "apps".into(),
        };
        let result = SweepResult {
            rows: vec![
                row("left", Standing::Orphaned(ks, OrphanReason::OwnerGone)),
                row(
                    "notes",
                    Standing::Unmanaged {
                        created_by: Some("kubectl".into()),
                    },
                ),
            ],
            kinds_scanned: 41,
            unreadable: vec!["secrets".into()],
            truncated: false,
        };
        let screen = render(Some(&result), false);
        assert!(
            screen.contains("Not managed by Flux in apps (2) · 41 kinds · 1 unreadable: secrets"),
            "{screen}"
        );
        assert!(
            screen.contains("Orphaned")
                && screen.contains("Flux Kustomization flux-system/apps no longer exists")
        );
        assert!(screen.contains("Unmanaged") && screen.contains("created by kubectl"));
    }

    #[test]
    fn loading_and_empty_states() {
        assert!(render(None, true).contains("listing every kind"));
        let empty = SweepResult {
            kinds_scanned: 3,
            ..SweepResult::default()
        };
        assert!(render(Some(&empty), false).contains("Nothing found"));
    }
}
