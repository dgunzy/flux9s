//! Resource list view rendering

use crate::tui::app::state::SortField;
use crate::tui::theme::Theme;
use crate::tui::views::props::ListCursor;
use crate::tui::views::{extract_resource_specific_fields, get_resource_type_columns};
use crate::watcher::ResourceInfo;
use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, Borders, Cell, Row, Table},
};
use std::cmp;
use std::collections::{HashMap, HashSet};
use unicode_width::UnicodeWidthStr;

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

/// What the Flux list (or favorites) shows. `access_notice` is the contextual RBAC notice shown in place of the default empty-state message when the list is empty because a kind is restricted (see `App::access_notice`).
pub struct ResourceListProps<'a> {
    pub resources: &'a [ResourceInfo],
    pub kind_filter: &'a crate::models::kind_filter::KindFilter,
    pub resource_objects: &'a HashMap<String, serde_json::Value>,
    pub no_icons: bool,
    pub favorites: &'a HashSet<String>,
    pub sort_field: SortField,
    pub sort_reverse: bool,
    pub access_notice: Option<&'a str>,
}

/// Render the resource list table
pub fn render_resource_list(
    f: &mut Frame,
    area: Rect,
    props: ResourceListProps<'_>,
    cursor: ListCursor<'_>,
    theme: &Theme,
) {
    let ResourceListProps {
        resources,
        kind_filter,
        resource_objects,
        no_icons,
        favorites,
        sort_field,
        sort_reverse,
        access_notice,
    } = props;
    let ListCursor {
        selected: selected_index,
        offset: scroll_offset,
    } = cursor;
    let visible_height = (area.height as usize).saturating_sub(2);
    const SCROLL_BUFFER: usize = 2; // Keep 2 rows buffer before scrolling

    // Adjust scroll offset based on selected index with buffer
    let viewport = crate::tui::views::scroll::Viewport::list_with_margin(
        selected_index,
        scroll_offset,
        resources.len(),
        visible_height,
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

    // One kind gets its own columns; several or all share the unified ones.
    // A kind set whose kinds all have revisions (`:ks,hr`, #309) shows
    // REVISION too, so revisions compare across kinds; the all-kinds list
    // stays as it was.
    let resource_type = kind_filter.single();
    let columns: Vec<&str> = match resource_type {
        Some(kind) => {
            let mut columns = get_resource_type_columns(kind);
            columns.push("AGE");
            columns
        }
        None => {
            let show_revision = !kind_filter.is_all()
                && kind_filter
                    .kinds()
                    .iter()
                    .all(|kind| get_resource_type_columns(kind).contains(&"REVISION"));
            let mut columns = vec!["STATUS", "NAMESPACE", "NAME", "TYPE"];
            if show_revision {
                columns.push("REVISION");
            }
            columns.extend(["SUSPENDED", "READY", "AGE", "MESSAGE"]);
            columns
        }
    };

    // Every cell's text, for every row of the list — not just the page on
    // screen — so the column widths don't move as you scroll (#309).
    // Kind-specific fields are read for single-kind lists only.
    let texts: Vec<Vec<String>> = resources
        .iter()
        .map(|r| {
            let fields = resource_type
                .and_then(|kind| {
                    let key =
                        crate::watcher::ResourceKey::new(&r.resource_type, &r.namespace, &r.name)
                            .to_key_string();
                    resource_objects
                        .get(&key)
                        .map(|obj| extract_resource_specific_fields(kind, obj))
                })
                .unwrap_or_default();
            columns
                .iter()
                // Cluster text made safe for one table row: newlines
                // flattened (a multi-line message keeps its `…`), control
                // characters and escapes stripped.
                .map(|col| {
                    crate::kube::kind_list::sanitize_cell(&cell_text(
                        col, r, &fields, favorites, theme, no_icons,
                    ))
                })
                .collect()
        })
        .collect();

    // Each column as wide as its widest value (and its header), but at most
    // a share of the list's width — so one very long URL or name can't push
    // MESSAGE off the row; MESSAGE takes the rest. On a narrow terminal the
    // layout shrinks the widest columns first, so short ones (READY, AGE)
    // stay readable.
    let column_cap = area.width.saturating_sub(2) / MAX_COLUMN_SHARE;
    let constraints: Vec<Constraint> = columns
        .iter()
        .enumerate()
        .map(|(i, col)| {
            if *col == "MESSAGE" {
                return Constraint::Fill(1);
            }
            let widest = texts
                .iter()
                .map(|row| display_width(&row[i]))
                .max()
                .unwrap_or(0);
            Constraint::Length(widest.min(column_cap).max(header_width(col)))
        })
        .collect();

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
    let title = match (kind_filter.view(), kind_filter.kinds_label()) {
        (Some(view), Some(kinds)) => format!("{view}: {kinds} ({total}){range_suffix}"),
        (None, Some(kinds)) => format!("{kinds} ({total}){range_suffix}"),
        (_, None) => format!("All Resources ({total}){range_suffix}"),
    };
    let block = Block::default().title(title).borders(Borders::ALL);

    // The widths the table will draw — the same constraints, spacing, flex,
    // and area it is given — so each cell's text can be fitted to its column.
    let widths: Vec<u16> = Layout::horizontal(constraints.iter().copied())
        .flex(Flex::Start)
        .spacing(COLUMN_SPACING)
        .split(block.inner(area))
        .iter()
        .map(|rect| rect.width)
        .collect();

    let header = Row::new(
        columns
            .iter()
            .map(|col| decorate_header(col, sort_field, sort_reverse, no_icons))
            .collect::<Vec<_>>(),
    )
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
            let row_texts = &texts[actual_idx];
            let favorite = favorites.contains(
                &crate::watcher::ResourceKey::new(&r.resource_type, &r.namespace, &r.name)
                    .to_key_string(),
            );
            let cells: Vec<Cell> = columns
                .iter()
                .zip(row_texts)
                .zip(&widths)
                .map(|((col, text), width)| {
                    let text = fit(text, *width);
                    match *col {
                        "STATUS" => {
                            let (_, color) =
                                get_status_indicator(r.ready, r.suspended, theme, no_icons);
                            Cell::from(Span::styled(text, Style::default().fg(color)))
                        }
                        "NAME" if favorite => Cell::from(Span::styled(
                            text,
                            Style::default()
                                .fg(theme.text_primary)
                                .add_modifier(Modifier::BOLD),
                        )),
                        _ => Cell::from(text),
                    }
                })
                .collect();
            let style = if actual_idx == valid_selected {
                theme.table_selected_style()
            } else {
                Style::default()
            };
            Row::new(cells).style(style)
        })
        .collect();

    // No highlight symbol: the widths measured above assume the table has
    // no selection column (selection is the row style). Adding one means
    // measuring that column too.
    let table = Table::new(rows, constraints)
        .header(header)
        .block(block)
        .column_spacing(COLUMN_SPACING)
        .flex(Flex::Start);

    f.render_widget(table, area);
    viewport.render_scrollbar(f, area, theme);
}

/// A cell's text: the same for the unified and single-kind lists.
/// `fields` are the kind-specific ones (empty in the unified list).
fn cell_text(
    col: &str,
    r: &ResourceInfo,
    fields: &HashMap<String, String>,
    favorites: &HashSet<String>,
    theme: &Theme,
    no_icons: bool,
) -> String {
    let yes_no = |value: Option<bool>| match value {
        Some(true) => "True",
        Some(false) => "False",
        None => "?",
    };
    match col {
        "STATUS" => get_status_indicator(r.ready, r.suspended, theme, no_icons)
            .0
            .to_string(),
        "NAMESPACE" => r.namespace.clone(),
        "NAME" => {
            let key = crate::watcher::ResourceKey::new(&r.resource_type, &r.namespace, &r.name)
                .to_key_string();
            if favorites.contains(&key) {
                format!("★ {}", r.name)
            } else {
                r.name.clone()
            }
        }
        "TYPE" => fields
            .get("TYPE")
            .cloned()
            .unwrap_or_else(|| r.resource_type.clone()),
        "SUSPENDED" => yes_no(r.suspended).to_string(),
        "READY" => yes_no(r.ready).to_string(),
        "REVISION" => r
            .revision
            .as_deref()
            .map(short_revision)
            .unwrap_or_else(|| "-".to_string()),
        "AGE" => crate::tui::views::helpers::format_age(r.age),
        "MESSAGE" => r.message.clone().unwrap_or_else(|| "-".to_string()),
        _ => fields.get(col).cloned().unwrap_or_else(|| "-".to_string()),
    }
}

/// A measured column takes at most 1/`MAX_COLUMN_SHARE` of the list's
/// width: it scales with the terminal, unlike a fixed cap.
const MAX_COLUMN_SHARE: u16 = 3;

/// Space between columns — given to the table and used to measure it, so
/// fitted text and drawn columns agree.
const COLUMN_SPACING: u16 = 2;

/// Display width of `text` in terminal columns (wide characters count 2).
fn display_width(text: &str) -> u16 {
    u16::try_from(UnicodeWidthStr::width(text)).unwrap_or(u16::MAX)
}

/// A header's width plus room for its sort arrow, so sorting by a column
/// never shifts the columns.
fn header_width(name: &str) -> u16 {
    display_width(name).saturating_add(1)
}

/// `text` cut to `width` terminal columns, ending in `…` only when it
/// doesn't fit. Measured by display width, so wide characters (CJK, emoji)
/// never overflow the column and lose the ellipsis (#309).
pub(crate) fn fit(text: &str, width: u16) -> String {
    use unicode_width::UnicodeWidthChar;
    if display_width(text) <= width {
        return text.to_string();
    }
    let room = usize::from(width.saturating_sub(1)); // one column for `…`
    if width == 0 {
        return String::new();
    }
    let mut used = 0;
    let mut cut = String::new();
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if used + w > room {
            break;
        }
        used += w;
        cut.push(c);
    }
    cut.push('…');
    cut
}

/// A revision with its commit or digest hash cut to 12 hex characters —
/// `main@sha1:4f1a5b1051b4` — like `git log --oneline`. The full revision
/// is in describe / YAML; versions (`6.15.0`) pass through (#309).
pub(crate) fn short_revision(revision: &str) -> String {
    const SHORT: usize = 12;
    for marker in ["sha1:", "sha256:", "sha512:", "blake3:"] {
        if let Some(at) = revision.find(marker) {
            let hash_start = at + marker.len();
            let hash = &revision[hash_start..];
            if hash.len() > SHORT && hash.chars().all(|c| c.is_ascii_hexdigit()) {
                return format!("{}{}", &revision[..hash_start], &hash[..SHORT]);
            }
        }
    }
    revision.to_string()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::kind_filter::KindFilter;
    use ratatui::{Terminal, backend::TestBackend};

    const REVISION: &str = "main@sha1:4f1a5b1051b469108d1bdf0ebec6c814427e3834";

    fn resource(kind: &str, name: &str, message: &str) -> ResourceInfo {
        ResourceInfo {
            name: name.into(),
            namespace: "flux-system".into(),
            resource_type: kind.into(),
            age: None,
            suspended: Some(false),
            ready: Some(true),
            message: Some(message.into()),
            revision: Some(REVISION.into()),
            labels: HashMap::new(),
            annotations: HashMap::new(),
            last_reconciled: None,
            reconciliation_history: vec![],
        }
    }

    /// Rows of `resources` rendered at `width` columns.
    fn render(resources: &[ResourceInfo], filter: &KindFilter, width: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, 8)).unwrap();
        terminal
            .draw(|f| {
                render_resource_list(
                    f,
                    f.area(),
                    ResourceListProps {
                        resources,
                        kind_filter: filter,
                        resource_objects: &HashMap::new(),
                        no_icons: false,
                        favorites: &HashSet::new(),
                        sort_field: SortField::Default,
                        sort_reverse: false,
                        access_notice: None,
                    },
                    ListCursor::new(0, &mut 0),
                    &Theme::default(),
                )
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..8)
            .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect())
            .collect()
    }

    fn applied() -> String {
        format!("Applied revision: {REVISION}")
    }

    #[test]
    fn the_message_uses_the_room_it_has() {
        let rows = render(
            &[resource("Kustomization", "apps", &applied())],
            &KindFilter::all(),
            180,
        );
        // Wide enough: the whole message, commit hash included (#309).
        assert!(rows[2].contains(&applied()), "{rows:#?}");
        // Narrow: cut to the column with an ellipsis, not a fixed length.
        let rows = render(
            &[resource("Kustomization", "apps", &applied())],
            &KindFilter::all(),
            100,
        );
        assert!(rows[2].contains("Applied revision: main@"), "{rows:#?}");
        assert!(rows[2].contains('…'), "{rows:#?}");
    }

    #[test]
    fn a_kind_set_with_revisions_shows_a_revision_column() {
        let both = KindFilter::parse("ks,hr").unwrap();
        let rows = render(
            &[
                resource("Kustomization", "apps", "Applied"),
                resource("HelmRelease", "podinfo", "Upgraded"),
            ],
            &both,
            160,
        );
        assert!(rows[1].contains("REVISION"), "{rows:#?}");
        assert!(
            rows[2].contains("main@sha1:4f1a5b1051b4 "),
            "short hash: {rows:#?}"
        );

        // Kinds without revisions (and the all-kinds list) stay as they were.
        let alerts = KindFilter::parse("ks,alert").unwrap();
        let rows = render(
            &[resource("Kustomization", "apps", "Applied")],
            &alerts,
            160,
        );
        assert!(!rows[1].contains("REVISION"), "{rows:#?}");
        let rows = render(
            &[resource("Kustomization", "apps", "Applied")],
            &KindFilter::all(),
            160,
        );
        assert!(!rows[1].contains("REVISION"), "{rows:#?}");
    }

    #[test]
    fn long_names_grow_their_column() {
        let name = "infrastructure-controllers-production";
        let rows = render(
            &[resource("Kustomization", name, "Applied")],
            &KindFilter::all(),
            180,
        );
        assert!(rows[2].contains(name), "{rows:#?}");
    }

    #[test]
    fn multibyte_messages_fit_without_panicking() {
        let message = "Reconciliación fallida — ".repeat(10);
        for width in [100, 130, 200] {
            let rows = render(
                &[resource("Kustomization", "apps", &message)],
                &KindFilter::all(),
                width,
            );
            assert!(rows[2].contains("Reconciliación"), "{rows:#?}");
        }
        // A single-kind list with a MESSAGE column (Kustomization and
        // HelmRelease show REVISION instead).
        let rows = render(
            &[resource("Provider", "slack", &message)],
            &KindFilter::kind("Provider"),
            200,
        );
        assert!(rows[2].contains("Reconciliación"), "{rows:#?}");
    }

    /// Rows of `resources` at `width`×`height`, scrolled so `selected` shows.
    fn render_page(
        resources: &[ResourceInfo],
        selected: usize,
        width: u16,
        height: u16,
    ) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let mut offset = 0;
        terminal
            .draw(|f| {
                render_resource_list(
                    f,
                    f.area(),
                    ResourceListProps {
                        resources,
                        kind_filter: &KindFilter::all(),
                        resource_objects: &HashMap::new(),
                        no_icons: false,
                        favorites: &HashSet::new(),
                        sort_field: SortField::Default,
                        sort_reverse: false,
                        access_notice: None,
                    },
                    ListCursor::new(selected, &mut offset),
                    &Theme::default(),
                )
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect())
            .collect()
    }

    /// Scrolling must never move the columns: widths come from every row,
    /// not the page on screen (#309 review).
    #[test]
    fn columns_stay_put_while_scrolling() {
        let mut resources: Vec<ResourceInfo> = (0..30)
            .map(|i| resource("Kustomization", &format!("app-{i:02}"), "Applied"))
            .collect();
        resources[25].name = "a-much-longer-kustomization-name-far-below".into();
        let top = render_page(&resources, 0, 160, 12);
        let bottom = render_page(&resources, 25, 160, 12);
        assert!(
            bottom.iter().any(|row| row.contains("a-much-longer")),
            "{bottom:#?}"
        );
        // Compare inside the borders: the scrollbar thumb on the right
        // border moves, as it should.
        let inner = |row: &str| row.chars().skip(1).take(158).collect::<String>();
        assert_eq!(
            inner(&top[1]),
            inner(&bottom[1]),
            "the header (and so every column) didn't move"
        );
    }

    /// When the row doesn't fit, the widest columns give way first; short
    /// ones stay readable (#309 review).
    #[test]
    fn narrow_terminals_shrink_the_widest_columns_first() {
        let rows = render(
            &[resource(
                "Kustomization",
                "an-extremely-long-kustomization-name-for-a-narrow-terminal",
                &applied(),
            )],
            &KindFilter::all(),
            90,
        );
        let header = &rows[1];
        for column in ["SUSPENDED", "READY", "AGE"] {
            assert!(header.contains(column), "{column} intact: {rows:#?}");
        }
        assert!(
            rows[2].contains("an-extremely-long-") && rows[2].contains('…'),
            "{rows:#?}"
        );
    }

    #[test]
    fn one_very_long_value_cant_push_the_message_out() {
        let mut long = resource("Kustomization", &"x".repeat(150), &applied());
        long.namespace = "flux-system".into();
        let rows = render(
            &[long, resource("Kustomization", "apps", &applied())],
            &KindFilter::all(),
            175,
        );
        // NAME is capped at a third of the width; the message still shows.
        assert!(rows[1].contains("MESSAGE"), "{rows:#?}");
        assert!(
            rows[3].contains("Applied revision: main@sha1:"),
            "{rows:#?}"
        );
    }

    #[test]
    fn multi_line_messages_keep_their_ellipsis() {
        let message = format!(
            "health check failed after 2m0s:\n{}",
            "timeout waiting for Deployment/apps/web ".repeat(4)
        );
        let rows = render(
            &[resource("Kustomization", "apps", &message)],
            &KindFilter::all(),
            120,
        );
        assert!(
            rows[2].contains("health check failed after 2m0s: timeout"),
            "flattened: {rows:#?}"
        );
        assert!(rows[2].contains('…'), "{rows:#?}");
    }

    #[test]
    fn wide_characters_fit_by_display_width() {
        let message = "同步失败：仓库不可用，请检查凭据与网络连接配置".repeat(3);
        let rows = render(
            &[resource("Kustomization", "apps", &message)],
            &KindFilter::all(),
            100,
        );
        // The ellipsis survives: the cut is measured in columns, not chars.
        assert!(rows[2].contains('…'), "{rows:#?}");
        assert!(
            rows[2].ends_with("│"),
            "nothing spills over the border: {rows:#?}"
        );
    }

    #[test]
    fn fit_and_short_revision() {
        assert_eq!(fit("abcdef", 6), "abcdef");
        assert_eq!(fit("abcdef", 4), "abc…");
        assert_eq!(fit("ééééé", 3), "éé…");
        assert_eq!(fit("同步失败", 5), "同步…", "wide characters count 2");
        assert_eq!(fit("abc", 0), "");
        assert_eq!(short_revision(REVISION), "main@sha1:4f1a5b1051b4");
        assert_eq!(
            short_revision("latest@sha256:0123456789abcdef0123456789abcdef"),
            "latest@sha256:0123456789ab"
        );
        assert_eq!(short_revision("6.15.0"), "6.15.0");
        assert_eq!(short_revision("main@sha1:abc123"), "main@sha1:abc123");
    }
}
