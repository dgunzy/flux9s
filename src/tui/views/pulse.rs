//! Cluster pulse dashboard (#195)
//!
//! An at-a-glance answer to "is my GitOps pipeline healthy?": totals and
//! per-kind health counts from the live watch state, the most recent
//! failures for fast triage, and the FluxReport's distribution info.
//! Everything renders from data flux9s already holds — no fetch needed.

use crate::tui::app::state::ControllerPodInfo;
use crate::tui::app::state::TextSearchState;
use crate::tui::theme::Theme;
use crate::tui::views::yaml::{apply_text_search, decorate_title_with_search, find_match_lines};
use crate::watcher::ResourceInfo;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use std::collections::BTreeMap;

/// How many recent failures the triage section lists.
const MAX_PULSE_FAILURES: usize = 10;

/// Per-kind health counters.
#[derive(Debug, Default, Clone, Copy)]
struct KindCounts {
    total: usize,
    ready: usize,
    failed: usize,
    suspended: usize,
}

/// Aggregate the watch state into per-kind counters (sorted by kind).
fn count_by_kind(resources: &[ResourceInfo]) -> BTreeMap<String, KindCounts> {
    let mut counts: BTreeMap<String, KindCounts> = BTreeMap::new();
    for resource in resources {
        let entry = counts.entry(resource.resource_type.clone()).or_default();
        entry.total += 1;
        if resource.effective_suspended() {
            entry.suspended += 1;
        } else if resource.effective_ready() {
            entry.ready += 1;
        } else {
            entry.failed += 1;
        }
    }
    counts
}

/// Build the dashboard's text lines (pure, so the content is testable).
fn build_pulse_lines(
    resources: &[ResourceInfo],
    flux_report: Option<&serde_json::Value>,
    controller_pods: &[ControllerPodInfo],
    namespace: Option<&str>,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let label = |t: &str| Span::styled(format!("{}: ", t), Style::default().fg(theme.text_label));
    let header = |t: &str| {
        Line::from(Span::styled(
            t.to_string(),
            Style::default()
                .fg(theme.text_label)
                .add_modifier(Modifier::BOLD),
        ))
    };
    let mut lines = Vec::new();

    // ── Totals ──────────────────────────────────────────────────────────
    let counts = count_by_kind(resources);
    let totals = counts.values().fold(KindCounts::default(), |mut acc, c| {
        acc.total += c.total;
        acc.ready += c.ready;
        acc.failed += c.failed;
        acc.suspended += c.suspended;
        acc
    });
    let percent = if totals.total == 0 {
        100.0
    } else {
        (totals.ready as f64 / totals.total as f64) * 100.0
    };

    lines.push(Line::from(vec![
        label("Scope"),
        Span::raw(namespace.unwrap_or("all namespaces").to_string()),
    ]));
    lines.push(Line::from(vec![
        label("Resources"),
        Span::styled(
            format!("{} ready", totals.ready),
            theme.status_ready_style(),
        ),
        Span::raw(" / "),
        Span::styled(
            format!("{} failed", totals.failed),
            if totals.failed > 0 {
                theme.status_error_style()
            } else {
                Style::default().fg(theme.text_secondary)
            },
        ),
        Span::raw(" / "),
        Span::styled(
            format!("{} suspended", totals.suspended),
            Style::default().fg(theme.status_suspended),
        ),
        Span::raw(format!("  ({} total, {:.1}% ready)", totals.total, percent)),
    ]));

    // ── Per-kind table ───────────────────────────────────────────────────
    lines.push(Line::from(""));
    lines.push(header("Kinds"));
    lines.push(Line::from(Span::styled(
        format!(
            "  {:<26} {:>6} {:>6} {:>7} {:>10}",
            "KIND", "TOTAL", "READY", "FAILED", "SUSPENDED"
        ),
        Style::default().fg(theme.text_label),
    )));
    for (kind, c) in &counts {
        let row_style = if c.failed > 0 {
            theme.status_error_style()
        } else {
            Style::default().fg(theme.text_primary)
        };
        lines.push(Line::from(Span::styled(
            format!(
                "  {:<26} {:>6} {:>6} {:>7} {:>10}",
                kind, c.total, c.ready, c.failed, c.suspended
            ),
            row_style,
        )));
    }
    if counts.is_empty() {
        lines.push(Line::from(Span::styled(
            "  <no resources in scope>".to_string(),
            Style::default().fg(theme.text_secondary),
        )));
    }

    // ── Failures (most recent reconcile activity first) ─────────────────
    let mut failures: Vec<&ResourceInfo> = resources
        .iter()
        .filter(|r| !r.effective_ready() && !r.effective_suspended())
        .collect();
    failures.sort_by_key(|r| std::cmp::Reverse(r.last_reconciled));

    lines.push(Line::from(""));
    lines.push(header(&format!("Failures ({})", failures.len())));
    if failures.is_empty() {
        lines.push(Line::from(Span::styled(
            "  <none>".to_string(),
            Style::default().fg(theme.text_secondary),
        )));
    }
    for resource in failures.iter().take(MAX_PULSE_FAILURES) {
        let message = resource
            .message
            .as_deref()
            .unwrap_or("-")
            .replace('\n', " ");
        lines.push(Line::from(Span::styled(
            format!(
                "  {:<22} {}/{} — {}",
                resource.resource_type, resource.namespace, resource.name, message
            ),
            theme.status_error_style(),
        )));
    }
    if failures.len() > MAX_PULSE_FAILURES {
        lines.push(Line::from(Span::styled(
            format!(
                "  … and {} more (use :unhealthy)",
                failures.len() - MAX_PULSE_FAILURES
            ),
            Style::default().fg(theme.text_secondary),
        )));
    }

    // ── Flux distribution (from the FluxReport) ─────────────────────────
    lines.push(Line::from(""));
    lines.push(header("Flux"));
    if let Some(report) = flux_report {
        let spec = &report["spec"];
        if let Some(version) = spec
            .pointer("/distribution/version")
            .and_then(|v| v.as_str())
        {
            let status = spec
                .pointer("/distribution/status")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown");
            lines.push(Line::from(vec![
                label("  Distribution"),
                Span::raw(format!("{} ({})", version, status)),
            ]));
        }
        if let Some(entitlement) = spec
            .pointer("/distribution/entitlement")
            .and_then(|v| v.as_str())
        {
            lines.push(Line::from(vec![
                label("  Entitlement"),
                Span::raw(entitlement.to_string()),
            ]));
        }
        if let Some(version) = spec.pointer("/operator/version").and_then(|v| v.as_str()) {
            let platform = spec
                .pointer("/operator/platform")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            lines.push(Line::from(vec![
                label("  Operator"),
                Span::raw(format!("{} {}", version, platform)),
            ]));
        }
        // Sync is only present when the FluxInstance defines spec.sync
        if let Some(source) = spec.pointer("/sync/source").and_then(|v| v.as_str()) {
            let status = spec
                .pointer("/sync/status")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown");
            lines.push(Line::from(vec![
                label("  Sync"),
                Span::raw(format!("{} ({})", source, status)),
            ]));
        }
    } else {
        lines.push(Line::from(Span::styled(
            "  FluxReport not available (is the Flux Operator installed?)".to_string(),
            Style::default().fg(theme.text_secondary),
        )));
    }

    // ── Controllers (live pod state, same source as the header) ─────────
    if controller_pods.is_empty() {
        // Distinct from "0/0 ready": no pods discovered is not healthy-green.
        lines.push(Line::from(vec![
            label("  Controllers"),
            Span::styled(
                "none discovered".to_string(),
                Style::default().fg(theme.text_secondary),
            ),
        ]));
    } else {
        let ready_pods = controller_pods.iter().filter(|p| p.ready).count();
        lines.push(Line::from(vec![
            label("  Controllers"),
            Span::styled(
                format!("{}/{} ready", ready_pods, controller_pods.len()),
                if ready_pods == controller_pods.len() {
                    theme.status_ready_style()
                } else {
                    theme.status_error_style()
                },
            ),
        ]));
    }
    for pod in controller_pods.iter().filter(|p| !p.ready) {
        lines.push(Line::from(Span::styled(
            format!("    {} not ready", pod.name),
            theme.status_error_style(),
        )));
    }

    lines
}

/// Render the pulse dashboard (scrollable, searchable text view).
#[allow(clippy::too_many_arguments)]
pub fn render_pulse(
    f: &mut Frame,
    area: Rect,
    resources: &[ResourceInfo],
    flux_report: Option<&serde_json::Value>,
    controller_pods: &[ControllerPodInfo],
    namespace: Option<&str>,
    scroll_offset: &mut usize,
    search: &mut TextSearchState,
    theme: &Theme,
) {
    let mut title = "Pulse".to_string();
    let all_lines = build_pulse_lines(resources, flux_report, controller_pods, namespace, theme);
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
    let paragraph = Paragraph::new(visible_lines).block(block);
    f.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn resource(
        kind: &str,
        name: &str,
        ready: Option<bool>,
        suspended: Option<bool>,
    ) -> ResourceInfo {
        ResourceInfo {
            name: name.to_string(),
            namespace: "flux-system".to_string(),
            resource_type: kind.to_string(),
            age: None,
            suspended,
            ready,
            message: Some(format!("{name} message")),
            revision: None,
            labels: HashMap::new(),
            annotations: HashMap::new(),
            last_reconciled: None,
            reconciliation_history: vec![],
        }
    }

    fn texts(lines: &[Line<'_>]) -> String {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn pulse_counts_ready_failed_suspended_per_kind() {
        let resources = [
            resource("Kustomization", "ok", Some(true), Some(false)),
            resource("Kustomization", "broken", Some(false), Some(false)),
            resource("Kustomization", "paused", Some(true), Some(true)),
            resource("GitRepository", "repo", Some(true), Some(false)),
            // Unknown readiness counts as ready (stateless kinds)
            resource("Alert", "alert", None, None),
        ];
        let all = texts(&build_pulse_lines(
            &resources,
            None,
            &[],
            None,
            &Theme::default(),
        ));

        assert!(all.contains("3 ready / 1 failed / 1 suspended  (5 total, 60.0% ready)"));
        // Kind rows: Kustomization 3 total, 1 ready, 1 failed, 1 suspended
        let ks_row = all
            .lines()
            .find(|l| l.trim_start().starts_with("Kustomization"))
            .unwrap();
        assert!(ks_row.contains(" 3 ") && ks_row.contains(" 1 "));
        assert!(all.contains("Failures (1)"));
        assert!(all.contains("flux-system/broken — broken message"));
        assert!(all.contains("FluxReport not available"));
        assert!(
            all.contains("Controllers: none discovered"),
            "no pods must not render as a green 0/0"
        );
        assert!(all.contains("Scope: all namespaces"));
    }

    #[test]
    fn pulse_shows_flux_report_and_controller_health() {
        let report = serde_json::json!({
            "spec": {
                "distribution": {
                    "version": "v2.9.2", "status": "Installed",
                    "entitlement": "Issued by controlplane"
                },
                "operator": {"version": "v0.55.0", "platform": "linux/arm64"},
                "sync": {"source": "https://github.com/org/repo.git", "status": "Applied rev main@sha1:abc"}
            }
        });
        let pods = [
            ControllerPodInfo {
                name: "source-controller-abc".to_string(),
                ready: true,
                version: None,
            },
            ControllerPodInfo {
                name: "helm-controller-def".to_string(),
                ready: false,
                version: None,
            },
        ];
        let all = texts(&build_pulse_lines(
            &[],
            Some(&report),
            &pods,
            Some("flux-system"),
            &Theme::default(),
        ));

        assert!(all.contains("Distribution: v2.9.2 (Installed)"));
        assert!(all.contains("Operator: v0.55.0 linux/arm64"));
        assert!(all.contains("Sync: https://github.com/org/repo.git"));
        assert!(all.contains("Controllers: 1/2 ready"));
        assert!(all.contains("helm-controller-def not ready"));
        assert!(all.contains("Scope: flux-system"));
        assert!(all.contains("<no resources in scope>"));
    }

    #[test]
    fn pulse_truncates_long_failure_lists() {
        let resources: Vec<ResourceInfo> = (0..15)
            .map(|i| {
                resource(
                    "Kustomization",
                    &format!("broken-{i}"),
                    Some(false),
                    Some(false),
                )
            })
            .collect();
        let all = texts(&build_pulse_lines(
            &resources,
            None,
            &[],
            None,
            &Theme::default(),
        ));
        assert!(all.contains("Failures (15)"));
        assert!(all.contains("… and 5 more (use :unhealthy)"));
    }
}
