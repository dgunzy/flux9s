//! The unmanaged / orphaned list (#274): objects of any kind with where
//! they stand with Flux and why. Enter/d describes the selected one, y shows
//! its YAML, and `m` / `t` go to the Flux object an orphan names.

use crate::kube::kind_list::sanitize_cell;
use crate::kube::standing::Standing;
use crate::kube::sweep::{
    SweepRequest, SweepResult, SweepRow, SweepScope, SweepShow, describe_request,
};
use crate::tui::theme::Theme;
use crate::tui::views::props::ListCursor;
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    widgets::{Cell, Row, Table},
};

/// What the unmanaged / orphaned list shows: the sweep asked for, its
/// result, which standings are shown, and the rows after that and the `/`
/// filter.
pub struct OwnershipListProps<'a> {
    pub request: Option<&'a SweepRequest>,
    pub result: Option<&'a SweepResult>,
    pub show: SweepShow,
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
        show,
        rows,
        loading,
        filter,
    } = props;
    let ListCursor {
        selected: selected_index,
        offset: scroll_offset,
    } = cursor;
    let what = request.map_or_else(
        || "Ownership".to_string(),
        |request| describe_request(request, show),
    );
    // Only a whole-cluster sweep has standings to switch between.
    let switchable = request.is_some_and(|r| r.scope == SweepScope::NotFlux);
    let Some(result) = result else {
        let message = if loading {
            format!("{what}: listing every kind…")
        } else {
            format!("{what}: the sweep didn't finish")
        };
        crate::tui::views::helpers::render_loading_state(f, area, &what, &message, theme);
        return;
    };

    let shown_by_show = result
        .rows
        .iter()
        .filter(|row| show.shows(&row.standing))
        .count();
    let title = title(
        &what,
        result,
        switchable.then_some(show),
        rows.len(),
        shown_by_show,
        filter,
    );
    if rows.is_empty() {
        let hidden = result.rows.len() - shown_by_show;
        let message = match (filter.is_empty(), show) {
            (false, _) => format!("Nothing matches '{filter}'"),
            (true, SweepShow::Orphaned) if switchable => "No orphans".to_string(),
            (true, SweepShow::Unmanaged) if switchable => "Nothing unmanaged".to_string(),
            _ => "Nothing found".to_string(),
        };
        // An incomplete scan can't promise "none": say so where the eye is.
        let message = match incomplete_warning(result) {
            Some(warning) => format!("{message} (scan incomplete: {warning})"),
            None => message,
        };
        let hint = if switchable && filter.is_empty() && hidden > 0 {
            format!("{hidden} more hidden: o orphans · u unmanaged · a everything")
        } else {
            "Ctrl-r rescans · ownership.ignore in the config hides expected objects".to_string()
        };
        crate::tui::views::helpers::render_empty_state(f, area, &title, &message, &hint, theme);
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

/// `Not managed by Flux in apps (44) · 2 unreadable: secrets, … · 3
/// orphaned · 41 unmanaged · 12 other · 41 kinds`. Warnings that the scan
/// is incomplete come right after the count, so a narrow terminal cuts the
/// counts before them; the standing counts come with a switchable sweep
/// (`show`), hidden ones marked.
fn title(
    what: &str,
    result: &SweepResult,
    show: Option<SweepShow>,
    shown: usize,
    shown_by_show: usize,
    filter: &str,
) -> String {
    let count = if filter.is_empty() {
        format!("({shown})")
    } else {
        format!("({shown} of {shown_by_show})")
    };
    let mut title = format!("{what} {count}");
    if let Some(warning) = incomplete_warning(result) {
        title.push_str(&format!(" · {warning}"));
    }
    if let Some(show) = show {
        let counts = result.counts();
        for (n, label, visible) in [
            (counts.orphaned, "orphaned", show != SweepShow::Unmanaged),
            (counts.unmanaged, "unmanaged", show != SweepShow::Orphaned),
            (counts.other, "other", show == SweepShow::All),
        ] {
            let hidden = if visible { "" } else { " hidden" };
            title.push_str(&format!(" · {n} {label}{hidden}"));
        }
    }
    title.push_str(&format!(" · {} kinds", result.kinds_scanned));
    title
}

/// Why the result may be missing objects: kinds that couldn't be listed
/// (`2 unreadable: secrets, …`) and the row cap (`truncated`).
fn incomplete_warning(result: &SweepResult) -> Option<String> {
    let mut parts = Vec::new();
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
        parts.push(format!(
            "{} unreadable: {names}{more}",
            result.unreadable.len()
        ));
    }
    if result.truncated {
        parts.push("truncated".to_string());
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
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
        render_showing(result, loading, SweepShow::All)
    }

    /// The list as the app draws it: rows filtered by `show`.
    fn render_showing(result: Option<&SweepResult>, loading: bool, show: SweepShow) -> String {
        let request = SweepRequest {
            scope: SweepScope::NotFlux,
            namespace: Some("apps".into()),
            config: Default::default(),
        };
        let backend = TestBackend::new(140, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let rows: Vec<&SweepRow> = result
            .map(|r| {
                r.rows
                    .iter()
                    .filter(|row| show.shows(&row.standing))
                    .collect()
            })
            .unwrap_or_default();
        terminal
            .draw(|f| {
                let mut offset = 0;
                render_ownership_list(
                    f,
                    f.area(),
                    OwnershipListProps {
                        request: Some(&request),
                        result,
                        show,
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
            screen.contains(
                "Not managed by Flux in apps (2) · 1 unreadable: secrets · 1 orphaned · 1 unmanaged · 0 other · 41 kinds"
            ),
            "{screen}"
        );
        assert!(
            screen.contains("Orphaned")
                && screen.contains("Flux Kustomization flux-system/apps no longer exists")
        );
        assert!(screen.contains("Unmanaged") && screen.contains("created by kubectl"));

        let unmanaged = render_showing(Some(&result), false, SweepShow::Unmanaged);
        assert!(
            unmanaged.contains(
                "Managed by nothing in apps (1) · 1 unreadable: secrets · 1 orphaned hidden · 1 unmanaged · 0 other hidden"
            ),
            "{unmanaged}"
        );
        assert!(
            !unmanaged.contains("no longer exists"),
            "orphans are hidden"
        );
    }

    /// Render at `width` columns.
    fn render_at(result: &SweepResult, show: SweepShow, width: u16) -> String {
        let request = SweepRequest {
            scope: SweepScope::NotFlux,
            namespace: None,
            config: Default::default(),
        };
        let rows: Vec<&SweepRow> = result
            .rows
            .iter()
            .filter(|r| show.shows(&r.standing))
            .collect();
        let mut terminal = Terminal::new(TestBackend::new(width, 10)).unwrap();
        terminal
            .draw(|f| {
                let mut offset = 0;
                render_ownership_list(
                    f,
                    f.area(),
                    OwnershipListProps {
                        request: Some(&request),
                        result: Some(result),
                        show,
                        rows: &rows,
                        loading: false,
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
            .chunks(width as usize)
            .map(|line| line.iter().map(|c| c.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// At 80 columns the incomplete-scan warning stays visible — the counts
    /// are what gets cut — and an empty list doesn't claim there's nothing.
    #[test]
    fn incomplete_scans_stay_visible_at_80_columns() {
        let result = SweepResult {
            rows: vec![row("notes", Standing::Unmanaged { created_by: None })],
            kinds_scanned: 139,
            unreadable: vec!["secrets".into(), "certificates".into()],
            truncated: true,
        };
        let list = render_at(&result, SweepShow::All, 80);
        let title = list.lines().next().unwrap();
        assert!(title.contains("2 unreadable: secrets"), "{list}");

        let empty = render_at(&result, SweepShow::Orphaned, 80);
        assert!(
            empty.lines().next().unwrap().contains("2 unreadable"),
            "{empty}"
        );
        assert!(empty.contains("No orphans (scan incomplete"), "{empty}");
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

    #[test]
    fn showing_orphans_only_counts_and_hints_at_the_rest() {
        let result = SweepResult {
            rows: vec![
                row("notes", Standing::Unmanaged { created_by: None }),
                row(
                    "app",
                    Standing::Other(Manager::ArgoCd {
                        app: "guestbook".into(),
                    }),
                ),
            ],
            kinds_scanned: 3,
            ..SweepResult::default()
        };
        let screen = render_showing(Some(&result), false, SweepShow::Orphaned);
        assert!(
            screen.contains(
                "Left behind by Flux in apps (0) · 0 orphaned · 1 unmanaged hidden · 1 other hidden"
            ),
            "{screen}"
        );
        assert!(screen.contains("No orphans") && screen.contains("2 more hidden: o orphans"));

        let all = render_showing(Some(&result), false, SweepShow::All);
        assert!(
            all.contains("Not managed by Flux in apps (2) · 0 orphaned · 1 unmanaged · 1 other ·"),
            "{all}"
        );
        assert!(all.contains("guestbook"));
    }
}
