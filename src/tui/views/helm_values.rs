//! HelmRelease effective values view (#264)
//!
//! A searchable text view: the `valuesFrom` sources in merge order with what
//! happened to each, then the merged values as YAML. The data is kept live
//! by the values watch (see [`crate::kube::live::watch_helm_values`]).

use crate::kube::helm_values::{HelmValues, SourceOutcome};
use crate::tui::app::state::TextSearchState;
use crate::tui::theme::Theme;
use crate::tui::views::yaml::{
    apply_text_search, decorate_title_with_search, find_match_lines, highlight_yaml_line,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

/// Build the view's lines: sources summary, separator, values YAML.
fn build_lines(values: &HelmValues, theme: &Theme) -> Vec<Line<'static>> {
    let label = Style::default().fg(theme.text_label);
    let mut lines = vec![Line::from(Span::styled("Sources (merge order):", label))];

    for (index, source) in values.sources.iter().enumerate() {
        let (text, style) = match &source.outcome {
            SourceOutcome::Applied => ("applied".to_string(), theme.status_ready_style()),
            SourceOutcome::AppliedRedacted => (
                "applied, values redacted (x to reveal)".to_string(),
                theme.status_ready_style(),
            ),
            SourceOutcome::Skipped(why) => (
                format!("skipped: {why}"),
                Style::default().fg(theme.text_secondary),
            ),
            SourceOutcome::Failed(why) => (format!("FAILED: {why}"), theme.status_error_style()),
        };
        lines.push(Line::from(vec![
            Span::raw(format!("  {}. {}  ", index + 1, source.reference.label())),
            Span::styled(text, style),
        ]));
    }
    let inline = if values.has_inline {
        Span::styled("applied last", theme.status_ready_style())
    } else {
        Span::styled("none", Style::default().fg(theme.text_secondary))
    };
    lines.push(Line::from(vec![
        Span::raw(format!(
            "  {}. spec.values (inline)  ",
            values.sources.len() + 1
        )),
        inline,
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("Values:", label)));

    let is_empty = values.values.as_object().is_none_or(|m| m.is_empty());
    if is_empty {
        lines.push(Line::from(Span::styled(
            "  <none> — the chart's defaults apply",
            Style::default().fg(theme.text_secondary),
        )));
    } else {
        let yaml = serde_yaml::to_string(&values.values)
            .unwrap_or_else(|e| format!("Error converting values to YAML: {e}"));
        lines.extend(yaml.lines().map(|line| highlight_yaml_line(line, theme)));
    }
    lines
}

/// Render the HelmRelease values view.
pub fn render_helm_values(
    f: &mut Frame,
    area: Rect,
    selected_resource_key: &Option<String>,
    values: Option<&HelmValues>,
    loading: bool,
    scroll_offset: &mut usize,
    search: &mut TextSearchState,
    theme: &Theme,
) {
    let name = selected_resource_key
        .as_deref()
        .and_then(crate::watcher::ResourceKey::parse)
        .map(|rk| format!("{}/{}", rk.namespace, rk.name))
        .unwrap_or_default();
    let Some(values) = values else {
        if loading {
            crate::tui::views::helpers::render_loading_state(
                f,
                area,
                "Values",
                "Resolving values from valuesFrom and spec.values...",
                theme,
            );
        } else {
            crate::tui::views::helpers::render_empty_state(
                f,
                area,
                "Values",
                "Values not available",
                "Press v on a HelmRelease to view its effective values",
                theme,
            );
        }
        return;
    };

    let mut title = format!("Values - HelmRelease - {name}");
    if values.secrets_revealed {
        title.push_str(" [secrets shown]");
    }

    let lines = build_lines(values, theme);
    let plain: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    let visible_height = (area.height as usize).saturating_sub(2);

    let match_lines = find_match_lines(&plain, &search.query);
    let current_match_line = apply_text_search(search, &match_lines, scroll_offset, visible_height);
    decorate_title_with_search(&mut title, search);

    let max_scroll = lines.len().saturating_sub(visible_height);
    *scroll_offset = (*scroll_offset).min(max_scroll);

    let visible: Vec<Line> = lines
        .into_iter()
        .enumerate()
        .skip(*scroll_offset)
        .take(visible_height)
        .map(|(idx, line)| {
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
    let paragraph = Paragraph::new(visible)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kube::helm_values::{ValuesRef, ValuesSource};
    use ratatui::{Terminal, backend::TestBackend};
    use serde_json::json;

    fn render(values: Option<&HelmValues>, loading: bool) -> String {
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
        let mut scroll = 0;
        let mut search = TextSearchState::default();
        terminal
            .draw(|f| {
                render_helm_values(
                    f,
                    f.area(),
                    &Some("HelmRelease:apps:podinfo".to_string()),
                    values,
                    loading,
                    &mut scroll,
                    &mut search,
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
            .chunks(100)
            .map(|r| r.concat())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn shows_sources_and_values() {
        let values = HelmValues {
            values: json!({"replicaCount": 2, "db": {"password": "<redacted>"}}),
            sources: vec![
                ValuesSource {
                    reference: ValuesRef {
                        kind: "Secret".into(),
                        name: "creds".into(),
                        values_key: "values.yaml".into(),
                        target_path: None,
                        optional: false,
                    },
                    outcome: SourceOutcome::AppliedRedacted,
                },
                ValuesSource {
                    reference: ValuesRef {
                        kind: "ConfigMap".into(),
                        name: "extra".into(),
                        values_key: "values.yaml".into(),
                        target_path: None,
                        optional: true,
                    },
                    outcome: SourceOutcome::Skipped("optional, not found".into()),
                },
            ],
            has_inline: true,
            secrets_revealed: false,
        };
        let out = render(Some(&values), false);
        assert!(out.contains("Values - HelmRelease - apps/podinfo"));
        assert!(out.contains("1. Secret/creds [values.yaml]"));
        assert!(out.contains("x to reveal"));
        assert!(out.contains("skipped: optional, not found"));
        assert!(out.contains("3. spec.values (inline)"));
        assert!(out.contains("replicaCount: 2"));
        assert!(out.contains("password: <redacted>"));
    }

    #[test]
    fn empty_values_mention_chart_defaults() {
        let values = HelmValues {
            values: json!({}),
            sources: vec![],
            has_inline: false,
            secrets_revealed: false,
        };
        assert!(render(Some(&values), false).contains("chart's defaults apply"));
    }

    #[test]
    fn loading_state() {
        assert!(render(None, true).contains("Resolving values"));
    }
}
