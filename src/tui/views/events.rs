//! Live Kubernetes events view rendering
//!
//! Renders the `:events` feed: a table of core/v1 Events in the current
//! namespace scope, newest first, with Warnings highlighted. The feed is
//! populated by the lazily started events watcher.

use crate::kube::events::KubeEventInfo;
use crate::tui::theme::Theme;
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    widgets::{Row, Table},
};
use std::cmp;

/// Render the Kubernetes events table.
///
/// `events` is pre-filtered and sorted newest-first; `total_count` is the
/// unfiltered feed size (shown in the title so an active filter is visible).
pub fn render_kube_events(
    f: &mut Frame,
    area: Rect,
    events: &[KubeEventInfo],
    total_count: usize,
    selected_index: usize,
    scroll_offset: &mut usize,
    filter: &str,
    all_namespaces: bool,
    theme: &Theme,
) {
    let visible_height = (area.height as usize).saturating_sub(2);
    const SCROLL_BUFFER: usize = 2; // Keep 2 rows buffer before scrolling

    crate::tui::views::helpers::update_scroll_offset(
        selected_index,
        visible_height,
        scroll_offset,
        SCROLL_BUFFER,
    );

    let mut title = if filter.is_empty() {
        format!("Events ({})", total_count)
    } else {
        format!("Events ({}/{}) /{}", events.len(), total_count, filter)
    };

    if events.is_empty() {
        let (message, hint) = if filter.is_empty() {
            (
                "No events yet",
                "Waiting for events in the current namespace scope...",
            )
        } else {
            ("No events match the filter", "Press / to change the filter")
        };
        crate::tui::views::helpers::render_empty_state(f, area, &title, message, hint, theme);
        return;
    }

    let valid_selected = cmp::min(selected_index, events.len().saturating_sub(1));

    let mut header_cells = vec!["LAST SEEN", "TYPE", "REASON"];
    if all_namespaces {
        header_cells.push("NAMESPACE");
    }
    header_cells.extend(["OBJECT", "COUNT", "FROM", "MESSAGE"]);
    let header = Row::new(header_cells).style(
        Style::default()
            .fg(theme.table_header)
            .add_modifier(Modifier::BOLD),
    );

    let rows: Vec<Row> = events
        .iter()
        .skip(*scroll_offset)
        .take(visible_height)
        .enumerate()
        .map(|(idx, event)| {
            let actual_idx = *scroll_offset + idx;
            let style = if actual_idx == valid_selected {
                theme.table_selected_style()
            } else if event.is_warning() {
                Style::default().fg(theme.status_error)
            } else {
                Style::default().fg(theme.text_primary)
            };

            let mut cells = vec![
                crate::tui::views::helpers::format_age(event.last_seen),
                event.event_type.clone(),
                event.reason.clone(),
            ];
            if all_namespaces {
                cells.push(event.involved_namespace.clone());
            }
            cells.extend([
                event.object_label(),
                format!("x{}", event.count),
                event.source.clone(),
                // Single row per event: collapse multi-line messages
                event.message.replace('\n', " "),
            ]);
            Row::new(cells).style(style)
        })
        .collect();

    let mut constraints = vec![
        Constraint::Length(9),  // LAST SEEN
        Constraint::Length(8),  // TYPE
        Constraint::Length(24), // REASON
    ];
    if all_namespaces {
        constraints.push(Constraint::Length(16)); // NAMESPACE
    }
    constraints.extend([
        Constraint::Length(32), // OBJECT
        Constraint::Length(6),  // COUNT
        Constraint::Length(22), // FROM
        Constraint::Min(20),    // MESSAGE
    ]);

    // Scroll position indicator, matching the resource list style
    if events.len() > visible_height {
        let first = *scroll_offset + 1;
        let last = cmp::min(*scroll_offset + visible_height, events.len());
        title = format!("{} [{}-{}]", title, first, last);
    }

    let block = crate::tui::views::helpers::create_themed_block(&title, theme);
    let table = Table::new(rows, constraints).header(header).block(block);
    f.render_widget(table, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn make_event(uid: &str, event_type: &str, reason: &str) -> KubeEventInfo {
        KubeEventInfo::from_json(&serde_json::json!({
            "metadata": {"uid": uid, "namespace": "flux-system"},
            "involvedObject": {
                "kind": "Kustomization",
                "namespace": "flux-system",
                "name": "podinfo"
            },
            "type": event_type,
            "reason": reason,
            "message": "line one\nline two",
            "count": 2,
            "lastTimestamp": "2026-07-01T12:30:00Z",
            "source": {"component": "kustomize-controller"}
        }))
        .unwrap()
    }

    fn render_to_text(events: &[KubeEventInfo], all_namespaces: bool) -> String {
        let mut terminal = Terminal::new(TestBackend::new(160, 12)).unwrap();
        let mut scroll_offset = 0;
        terminal
            .draw(|frame| {
                render_kube_events(
                    frame,
                    frame.area(),
                    events,
                    events.len(),
                    0,
                    &mut scroll_offset,
                    "",
                    all_namespaces,
                    &Theme::default(),
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut text = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                text.push_str(buffer[(x, y)].symbol());
            }
            text.push('\n');
        }
        text
    }

    #[test]
    fn renders_event_rows_and_columns() {
        let events = [
            make_event("a", "Warning", "ReconciliationFailed"),
            make_event("b", "Normal", "ReconciliationSucceeded"),
        ];
        let text = render_to_text(&events, false);
        assert!(text.contains("REASON"));
        assert!(!text.contains("NAMESPACE"), "namespaced scope hides column");
        assert!(text.contains("ReconciliationFailed"));
        assert!(text.contains("Kustomization/podinfo"));
        assert!(text.contains("x2"));
        assert!(text.contains("line one line two"), "message is one row");
        assert!(text.contains("Events (2)"));
    }

    #[test]
    fn all_namespaces_scope_adds_namespace_column() {
        let events = [make_event("a", "Normal", "Sync")];
        let text = render_to_text(&events, true);
        assert!(text.contains("NAMESPACE"));
        assert!(text.contains("flux-system"));
    }

    #[test]
    fn empty_feed_renders_waiting_state() {
        let text = render_to_text(&[], false);
        assert!(text.contains("No events yet"));
    }
}
