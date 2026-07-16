//! Workload drill-down views (#194)
//!
//! `render_workload_list` shows the workloads of a graph WorkloadGroup node;
//! `render_workload_detail` shows one workload's rollout summary, containers,
//! pods, and events (read-only).

use crate::kube::workloads::{WorkloadData, WorkloadRef};
use crate::tui::app::state::TextSearchState;
use crate::tui::theme::Theme;
use crate::tui::views::yaml::{apply_text_search, decorate_title_with_search, find_match_lines};
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Row, Table, Wrap},
};
use std::cmp;

/// Render the workload list (the drilled-into WorkloadGroup's members).
pub fn render_workload_list(
    f: &mut Frame,
    area: Rect,
    rows: &[WorkloadRef],
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

    let title = format!("Workloads ({})", rows.len());
    if rows.is_empty() {
        crate::tui::views::helpers::render_empty_state(
            f,
            area,
            &title,
            "No workloads",
            "Open a graph workload group to populate this view",
            theme,
        );
        return;
    }

    let valid_selected = cmp::min(selected_index, rows.len().saturating_sub(1));
    let header = Row::new(["KIND", "NAME", "NAMESPACE", "READY", "STATUS"]).style(
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
                row.name.clone(),
                row.namespace.clone(),
                row.indicator.clone(),
                row.status.clone(),
            ])
            .style(style)
        })
        .collect();

    let constraints = [
        Constraint::Length(12), // KIND
        Constraint::Length(36), // NAME
        Constraint::Length(20), // NAMESPACE
        Constraint::Length(6),  // READY
        Constraint::Min(16),    // STATUS
    ];

    let block = crate::tui::views::helpers::create_themed_block(&title, theme);
    let table = Table::new(table_rows, constraints)
        .header(header)
        .block(block);
    f.render_widget(table, area);
}

/// Build the workload detail's text lines (separate from rendering so the
/// content is unit testable).
fn build_workload_lines(workload: &WorkloadData, theme: &Theme) -> Vec<Line<'static>> {
    let label =
        |text: &str| Span::styled(format!("{}: ", text), Style::default().fg(theme.text_label));
    let mut lines = vec![
        Line::from(vec![label("Kind"), Span::raw(workload.kind.clone())]),
        Line::from(vec![label("Name"), Span::raw(workload.name.clone())]),
        Line::from(vec![
            label("Namespace"),
            Span::raw(workload.namespace.clone()),
        ]),
    ];

    if let Some(ready) = workload.ready {
        lines.push(Line::from(vec![
            label("Ready"),
            Span::styled(
                if ready { "True" } else { "False" }.to_string(),
                if ready {
                    theme.status_ready_style()
                } else {
                    theme.status_error_style()
                },
            ),
        ]));
    }
    for (key, value) in &workload.summary {
        lines.push(Line::from(vec![label(key), Span::raw(value.clone())]));
    }

    if !workload.containers.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("Containers ({}):", workload.containers.len()),
            Style::default().fg(theme.text_label),
        )));
        for container in &workload.containers {
            lines.push(Line::from(format!(
                "  {}  {}",
                container.name, container.image
            )));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!("Pods ({}):", workload.pods.len()),
        Style::default().fg(theme.text_label),
    )));
    if workload.pods.is_empty() {
        lines.push(Line::from(Span::styled(
            "  <none>".to_string(),
            Style::default().fg(theme.text_secondary),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            format!(
                "  {:<44} {:<12} {:>6} {:>9} {:>7}",
                "NAME", "PHASE", "READY", "RESTARTS", "AGE"
            ),
            Style::default().fg(theme.text_label),
        )));
        for pod in &workload.pods {
            let style = if pod.phase == "Running" {
                Style::default()
            } else {
                Style::default().fg(theme.status_error)
            };
            lines.push(Line::from(Span::styled(
                format!(
                    "  {:<44} {:<12} {:>6} {:>9} {:>7}",
                    pod.name,
                    pod.phase,
                    pod.ready,
                    pod.restarts,
                    crate::tui::views::helpers::format_age(pod.age),
                ),
                style,
            )));
        }
    }

    // Events section, kubectl-style (same shape as the describe view)
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Events".to_string(),
        Style::default().fg(theme.text_label),
    )));
    if let Some(ref error) = workload.events_error {
        lines.push(Line::from(Span::styled(
            format!("  Events unavailable: {}", error),
            Style::default().fg(theme.text_secondary),
        )));
    } else if workload.events.is_empty() {
        lines.push(Line::from(Span::styled(
            "  <none>".to_string(),
            Style::default().fg(theme.text_secondary),
        )));
    } else {
        for event in &workload.events {
            let style = if event.is_warning() {
                Style::default().fg(theme.status_error)
            } else {
                Style::default()
            };
            lines.push(Line::from(Span::styled(
                format!(
                    "  {:<8} {:<24} {:>8}  {}",
                    event.event_type,
                    event.reason,
                    crate::tui::views::helpers::format_age(event.last_seen),
                    event.message.replace('\n', " "),
                ),
                style,
            )));
        }
    }

    lines
}

/// Render the workload detail view (scrollable, searchable text).
pub fn render_workload_detail(
    f: &mut Frame,
    area: Rect,
    workload: Option<&WorkloadData>,
    loading: bool,
    scroll_offset: &mut usize,
    search: &mut TextSearchState,
    theme: &Theme,
) {
    let Some(workload) = workload else {
        if loading {
            crate::tui::views::helpers::render_loading_state(
                f,
                area,
                "Workload",
                "Fetching workload, pods, and events...",
                theme,
            );
        } else {
            crate::tui::views::helpers::render_empty_state(
                f,
                area,
                "Workload",
                "No workload selected",
                "Open one from a graph workload group",
                theme,
            );
        }
        return;
    };

    let mut title = format!("Workload - {} - {}", workload.kind, workload.name);
    let all_lines = build_workload_lines(workload, theme);
    let visible_height = (area.height as usize).saturating_sub(2);

    let line_texts: Vec<String> = all_lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect();
    let match_lines = find_match_lines(&line_texts, &search.query);
    let current_match_line = apply_text_search(search, &match_lines, scroll_offset, visible_height);
    decorate_title_with_search(&mut title, search);

    let max_scroll = all_lines.len().saturating_sub(visible_height);
    *scroll_offset = (*scroll_offset).min(max_scroll);

    let visible_lines: Vec<Line> = all_lines
        .iter()
        .enumerate()
        .skip(*scroll_offset)
        .take(visible_height)
        .map(|(idx, line)| {
            let line = line.clone();
            if Some(idx) == current_match_line {
                line.style(Style::default().add_modifier(Modifier::REVERSED))
            } else if match_lines.binary_search(&idx).is_ok() {
                line.style(Style::default().add_modifier(Modifier::UNDERLINED))
            } else {
                line
            }
        })
        .collect();

    let block = crate::tui::views::helpers::create_themed_block(&title, theme);
    let paragraph = Paragraph::new(visible_lines)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kube::workloads::{ContainerInfo, PodRow};

    fn workload() -> WorkloadData {
        WorkloadData {
            kind: "Deployment".to_string(),
            name: "source-controller".to_string(),
            namespace: "flux-system".to_string(),
            ready: Some(false),
            summary: vec![(
                "Replicas".to_string(),
                "1/2 ready, 2 updated, 1 available".to_string(),
            )],
            containers: vec![ContainerInfo {
                name: "manager".to_string(),
                image: "ghcr.io/fluxcd/source-controller:v1.9.3".to_string(),
            }],
            pods: vec![PodRow {
                name: "source-controller-abc".to_string(),
                phase: "CrashLoopBackOff".to_string(),
                ready: "0/1".to_string(),
                restarts: 7,
                age: None,
            }],
            events: Vec::new(),
            events_error: Some("forbidden".to_string()),
        }
    }

    fn texts(lines: &[Line<'_>]) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    #[test]
    fn workload_lines_cover_summary_containers_pods_events() {
        let lines = texts(&build_workload_lines(&workload(), &Theme::default()));
        let all = lines.join("\n");
        assert!(all.contains("Ready: False"));
        assert!(all.contains("1/2 ready, 2 updated, 1 available"));
        assert!(all.contains("manager  ghcr.io/fluxcd/source-controller:v1.9.3"));
        assert!(all.contains("source-controller-abc"));
        assert!(all.contains("CrashLoopBackOff"));
        assert!(all.contains("Events unavailable: forbidden"));
    }

    #[test]
    fn workload_lines_show_empty_pod_and_event_states() {
        let mut wl = workload();
        wl.pods.clear();
        wl.events_error = None;
        let all = texts(&build_workload_lines(&wl, &Theme::default())).join("\n");
        assert!(all.contains("Pods (0):"));
        assert!(
            all.matches("<none>").count() >= 2,
            "pods and events both empty"
        );
    }
}
