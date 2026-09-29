//! Workload drill-down views (#194)
//!
//! `render_workload_list` shows the workloads of a graph WorkloadGroup node;
//! `render_workload_detail` shows one workload's rollout summary, containers,
//! pods, and events (read-only).

use crate::kube::metrics::{MetricsSnapshot, MetricsSource, format_memory};
use crate::kube::workloads::{PodRow, WorkloadData, WorkloadRef};
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

/// Usage at or above this share of the limit is highlighted as a risk
/// (CPU throttling / OOM kill); above the warn share it's flagged softer.
const LIMIT_DANGER: f64 = 0.9;
const LIMIT_WARN: f64 = 0.75;
/// Width of the usage bars, in cells.
const BAR_WIDTH: usize = 20;

/// CPU in millicores always, so use/request/limit compare at a glance.
/// Nonzero use under 1m reads `<1m`, never a misleading `0m`.
fn millicores(value: f64) -> String {
    if value > 0.0 && value < 0.5 {
        "<1m".to_string()
    } else {
        format!("{}m", value.round())
    }
}

/// Cells of a usage bar. Used cells are drawn in the status colour, free
/// cells dim, so the fill is readable in any font.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BarCell {
    Used,
    Free,
    /// Where the request sits.
    Request,
}

/// The bar for `used` against `scale`, with the request marked. Any
/// nonzero use fills at least one cell so light load stays visible.
fn usage_bar(used: f64, request: Option<f64>, scale: f64) -> Vec<BarCell> {
    let cell = |v: f64| {
        ((v / scale) * BAR_WIDTH as f64)
            .round()
            .clamp(0.0, BAR_WIDTH as f64) as usize
    };
    let mut filled = cell(used);
    if used > 0.0 {
        filled = filled.max(1);
    }
    // A request under half a cell can't be placed meaningfully, and would
    // hide the first used cell — leave it off the bar (it's still printed).
    let marker = request
        .filter(|r| *r > 0.0 && *r < scale)
        .map(cell)
        .filter(|m| *m >= 1);
    (0..BAR_WIDTH)
        .map(|i| match (Some(i) == marker, i < filled) {
            (true, _) => BarCell::Request,
            (false, true) => BarCell::Used,
            (false, false) => BarCell::Free,
        })
        .collect()
}

/// Bar cells as styled spans (runs of the same kind share one span).
fn bar_spans(cells: &[BarCell], used_style: Style, theme: &Theme) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut run = String::new();
    let mut current: Option<BarCell> = None;
    let style_of = |kind: BarCell| match kind {
        BarCell::Used => used_style,
        BarCell::Free => Style::default().fg(theme.text_secondary),
        BarCell::Request => Style::default().fg(theme.text_label),
    };
    for &kind in cells {
        if current.is_some_and(|c| c != kind) {
            if let Some(c) = current {
                spans.push(Span::styled(std::mem::take(&mut run), style_of(c)));
            }
        }
        current = Some(kind);
        run.push(match kind {
            BarCell::Used => '█',
            BarCell::Free => '░',
            BarCell::Request => '│',
        });
    }
    if let Some(c) = current {
        spans.push(Span::styled(run, style_of(c)));
    }
    spans
}

/// Share of the limit, never rounded down to a misleading `0%`: nonzero
/// use under 1% reads `<1%`.
fn percent_of_limit(used: f64, limit: f64) -> String {
    let percent = used / limit * 100.0;
    if used > 0.0 && percent < 1.0 {
        "<1%".to_string()
    } else {
        format!("{percent:.0}%")
    }
}

/// One CPU or MEM line of the Resources section.
fn resource_line(
    label: &str,
    used: Option<f64>,
    request: Option<f64>,
    limit: Option<f64>,
    source: Option<MetricsSource>,
    format: fn(f64) -> String,
    theme: &Theme,
) -> Line<'static> {
    let secondary = Style::default().fg(theme.text_secondary);
    let fmt = |v: Option<f64>| v.map_or_else(|| "-".to_string(), format);
    let mut spans = vec![Span::styled(
        format!("    {label:<4}"),
        Style::default().fg(theme.text_label),
    )];

    // The bar scales to the limit, else the request; without either there
    // is nothing meaningful to fill against.
    let scale = limit.or(request).filter(|s| *s > 0.0);
    match (source, used) {
        (Some(MetricsSource::None), _) => spans.push(Span::styled(
            format!("{:<width$}", "(no metrics)", width = BAR_WIDTH + 2),
            secondary,
        )),
        (_, Some(used)) => {
            let ratio = limit.filter(|l| *l > 0.0).map(|l| used / l);
            let style = match ratio {
                Some(r) if r >= LIMIT_DANGER => theme.status_error_style(),
                Some(r) if r >= LIMIT_WARN => Style::default().fg(theme.status_pending),
                Some(_) => theme.status_ready_style(),
                None => Style::default().fg(theme.text_primary),
            };
            match scale {
                Some(scale) => {
                    spans.extend(bar_spans(&usage_bar(used, request, scale), style, theme))
                }
                None => spans.push(Span::raw(" ".repeat(BAR_WIDTH))),
            }
            spans.push(Span::styled(format!("  {:>7}", format(used)), style));
        }
        (Some(_), None) => spans.push(Span::styled(
            format!("{:<width$}", "measuring…", width = BAR_WIDTH + 9),
            secondary,
        )),
        (None, _) => spans.push(Span::styled(
            format!("{:<width$}", "…", width = BAR_WIDTH + 9),
            secondary,
        )),
    }
    if request.is_none() && limit.is_none() {
        spans.push(Span::styled("   no request/limit set", secondary));
    } else {
        spans.push(Span::raw(format!(
            "   req {:<8} lim {:<8}",
            fmt(request),
            fmt(limit)
        )));
    }
    if let (Some(used), Some(limit)) = (used, limit.filter(|l| *l > 0.0))
        && source != Some(MetricsSource::None)
    {
        spans.push(Span::styled(
            format!("  {} of limit", percent_of_limit(used, limit)),
            secondary,
        ));
    }
    Line::from(spans)
}

/// The Resources section: per pod, CPU and memory use against request and
/// limit (#265).
fn resource_lines(
    pods: &[PodRow],
    metrics: Option<&MetricsSnapshot>,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let header = match metrics.map(|m| m.source) {
        None => "Resources (detecting metrics source…):".to_string(),
        Some(MetricsSource::None) => {
            "Resources (no metrics source — requests/limits only):".to_string()
        }
        Some(source) => format!("Resources (metrics: {source}, refreshed every 15s):"),
    };
    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(header, Style::default().fg(theme.text_label))),
    ];
    let source = metrics.map(|m| m.source);
    for pod in pods {
        let usage = metrics.and_then(|m| m.pods.get(&pod.name));
        lines.push(Line::from(format!("  {}", pod.name)));
        lines.push(resource_line(
            "CPU",
            usage.and_then(|u| u.cpu_millicores),
            pod.resources.cpu_request,
            pod.resources.cpu_limit,
            source,
            millicores,
            theme,
        ));
        lines.push(resource_line(
            "MEM",
            usage.and_then(|u| u.memory_bytes),
            pod.resources.memory_request,
            pod.resources.memory_limit,
            source,
            format_memory,
            theme,
        ));
    }
    lines
}

/// Build the workload detail's text lines (separate from rendering so the
/// content is unit testable).
fn build_workload_lines(
    workload: &WorkloadData,
    metrics: Option<&MetricsSnapshot>,
    theme: &Theme,
) -> Vec<Line<'static>> {
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

    if let Some(managed_by) = &workload.managed_by {
        lines.push(Line::from(vec![
            label("Managed By"),
            Span::raw(managed_by.clone()),
        ]));
    }
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
        lines.extend(resource_lines(&workload.pods, metrics, theme));
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
    metrics: Option<&MetricsSnapshot>,
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
    if let Some(metrics) = metrics {
        title.push_str(&match metrics.source {
            MetricsSource::None => " · metrics: none (requests/limits)".to_string(),
            source => format!(" · metrics: {source}"),
        });
    }
    let all_lines = build_workload_lines(workload, metrics, theme);
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
                resources: Default::default(),
            }],
            events: Vec::new(),
            events_error: Some("forbidden".to_string()),
            pod_selector: None,
            managed_by: None,
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
        let lines = texts(&build_workload_lines(&workload(), None, &Theme::default()));
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
        let all = texts(&build_workload_lines(&wl, None, &Theme::default())).join("\n");
        assert!(all.contains("Pods (0):"));
        assert!(
            all.matches("<none>").count() >= 2,
            "pods and events both empty"
        );
    }

    fn pod_with_limits() -> PodRow {
        PodRow {
            name: "web-1".to_string(),
            phase: "Running".to_string(),
            ready: "1/1".to_string(),
            restarts: 0,
            age: None,
            resources: crate::kube::metrics::PodResources {
                cpu_request: Some(100.0),
                cpu_limit: Some(200.0),
                memory_request: Some(64.0 * 1024.0 * 1024.0),
                memory_limit: Some(128.0 * 1024.0 * 1024.0),
            },
        }
    }

    fn snapshot(source: MetricsSource, cpu: f64, mem_mi: f64) -> MetricsSnapshot {
        MetricsSnapshot {
            source,
            pods: std::collections::HashMap::from([(
                "web-1".to_string(),
                crate::kube::metrics::PodUsage {
                    cpu_millicores: Some(cpu),
                    memory_bytes: Some(mem_mi * 1024.0 * 1024.0),
                },
            )]),
        }
    }

    fn resources_text(metrics: Option<&MetricsSnapshot>) -> String {
        texts(&resource_lines(
            &[pod_with_limits()],
            metrics,
            &Theme::default(),
        ))
        .join("\n")
    }

    #[test]
    fn resources_show_use_request_limit_and_bar() {
        let metrics = snapshot(MetricsSource::Kubelet, 50.0, 120.0);
        let out = resources_text(Some(&metrics));
        assert!(out.contains("metrics: kubelet, refreshed every 15s"));
        assert!(out.contains("50m"));
        assert!(out.contains("req 100m"));
        assert!(out.contains("lim 200m"));
        assert!(out.contains("25% of limit"));
        assert!(out.contains("120Mi"));
        assert!(out.contains("94% of limit"));
        assert!(
            out.contains('█') && out.contains('│'),
            "bar with request marker"
        );
    }

    #[test]
    fn memory_near_limit_is_flagged_red() {
        let theme = Theme::default();
        let line = resource_line(
            "MEM",
            Some(120.0),
            Some(64.0),
            Some(128.0),
            Some(MetricsSource::Kubelet),
            format_memory,
            &theme,
        );
        assert!(
            line.spans
                .iter()
                .any(|s| s.style == theme.status_error_style()),
            "94% of the limit is an OOM risk"
        );
    }

    #[test]
    fn usage_bar_marks_request_and_fills_to_use() {
        // 50 of 200 = 5 cells; request 100 = cell 10
        let bar = usage_bar(50.0, Some(100.0), 200.0);
        assert_eq!(bar.len(), BAR_WIDTH);
        assert_eq!(bar.iter().filter(|c| **c == BarCell::Used).count(), 5);
        assert_eq!(bar[10], BarCell::Request);
        // Light but nonzero use still shows one used cell.
        let light = usage_bar(6.0, None, 2000.0);
        assert_eq!(light.iter().filter(|c| **c == BarCell::Used).count(), 1);
        assert!(
            usage_bar(0.0, None, 2000.0)
                .iter()
                .all(|c| *c == BarCell::Free)
        );
        // flux-operator: 6m used, 10m request, 2000m limit — the tiny request
        // must not hide the used cell.
        let tiny = usage_bar(6.0, Some(10.0), 2000.0);
        assert_eq!(tiny[0], BarCell::Used);
        assert!(!tiny.contains(&BarCell::Request));
    }

    #[test]
    fn idle_cpu_never_reads_zero() {
        assert_eq!(millicores(0.005), "<1m");
        assert_eq!(millicores(0.0), "0m");
        assert_eq!(millicores(6.4), "6m");
    }

    #[test]
    fn percent_never_rounds_running_use_to_zero() {
        assert_eq!(percent_of_limit(6.0, 2000.0), "<1%");
        assert_eq!(percent_of_limit(0.0, 2000.0), "0%");
        assert_eq!(percent_of_limit(111.0, 1024.0), "11%");
    }

    #[test]
    fn free_cells_are_dim_and_used_cells_carry_status_colour() {
        let theme = Theme::default();
        let used_style = theme.status_ready_style();
        let spans = bar_spans(&usage_bar(50.0, Some(100.0), 200.0), used_style, &theme);
        assert_eq!(spans[0].content, "█████");
        assert_eq!(spans[0].style, used_style);
        assert!(
            spans
                .iter()
                .filter(|s| s.content.contains('░'))
                .all(|s| s.style == Style::default().fg(theme.text_secondary))
        );
    }

    #[test]
    fn without_a_source_resources_show_requests_and_limits() {
        let metrics = MetricsSnapshot {
            source: MetricsSource::None,
            pods: Default::default(),
        };
        let out = resources_text(Some(&metrics));
        assert!(out.contains("requests/limits only"));
        assert!(out.contains("(no metrics)"));
        assert!(out.contains("req 100m"));
        assert!(!out.contains("% of limit"));
    }

    #[test]
    fn resources_wait_for_source_and_first_cpu_sample() {
        assert!(resources_text(None).contains("detecting metrics source"));
        let mut metrics = snapshot(MetricsSource::Kubelet, 0.0, 10.0);
        if let Some(usage) = metrics.pods.get_mut("web-1") {
            usage.cpu_millicores = None;
        }
        assert!(resources_text(Some(&metrics)).contains("measuring…"));
    }
}
