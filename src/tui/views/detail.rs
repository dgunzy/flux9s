//! Resource detail view rendering

use crate::models::FluxResourceKind;
use crate::models::flux_resource_kind::field_names;
use crate::tui::theme::Theme;
use crate::watcher::ResourceState;
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};
use std::collections::HashMap;

/// Capitalize the first letter of a field key for display
/// Special case: "URL" stays all-caps
fn capitalize_first(key: &str) -> String {
    if key == "URL" {
        "URL".to_string()
    } else if let Some(first) = key.chars().next() {
        format!(
            "{}{}",
            first.to_uppercase(),
            key[first.len_utf8()..].to_lowercase()
        )
    } else {
        key.to_string()
    }
}

/// Display phase of one ResourceSet step, derived from spec order and the
/// status conditions. `None` phases mean the conditions carry no step
/// information (e.g. suspended or never reconciled) — names still render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StepPhase {
    Done,
    Applying,
    Failed,
    Pending,
}

impl StepPhase {
    fn label(self) -> &'static str {
        match self {
            StepPhase::Done => "done",
            StepPhase::Applying => "applying",
            StepPhase::Failed => "failed",
            StepPhase::Pending => "pending",
        }
    }
}

/// One `spec.steps` entry reduced to what the detail view shows.
#[derive(Debug)]
struct StepInfo {
    name: String,
    /// Number of inline `resources` entries.
    resources: usize,
    /// Whether the step also renders a `resourcesTemplate`.
    has_template: bool,
    /// The step's own timeout, verbatim (e.g. "5m").
    timeout: Option<String>,
    phase: Option<StepPhase>,
}

/// Find a condition of the given type in `status.conditions`.
fn find_condition<'a>(
    obj: &'a serde_json::Value,
    cond_type: &str,
) -> Option<&'a serde_json::Value> {
    obj.pointer("/status/conditions")?
        .as_array()?
        .iter()
        .find(|c| c["type"].as_str() == Some(cond_type))
}

/// Extract the ResourceSet steps and derive each step's phase.
///
/// The operator reports step progress only through condition messages:
/// while running, the Reconciling condition says `Applying step <i>/<n> "<name>"`;
/// on failure, the Ready condition message contains `step "<name>"`. Steps
/// before the referenced one have applied; later ones are pending. A Ready
/// ResourceSet has completed every step.
fn extract_resource_set_steps(obj: &serde_json::Value) -> Vec<StepInfo> {
    let Some(steps) = obj.pointer("/spec/steps").and_then(|s| s.as_array()) else {
        return Vec::new();
    };

    let mut infos: Vec<StepInfo> = steps
        .iter()
        .map(|step| StepInfo {
            name: step["name"].as_str().unwrap_or_default().to_string(),
            resources: step["resources"].as_array().map_or(0, |r| r.len()),
            has_template: step["resourcesTemplate"]
                .as_str()
                .is_some_and(|t| !t.is_empty()),
            timeout: step["timeout"].as_str().map(|t| t.to_string()),
            phase: None,
        })
        .collect();

    // A step named in a condition message appears as `step "<name>"` /
    // `Applying step <i>/<n> "<name>"` — match on the quoted name.
    let named_step_position = |message: &str| {
        infos
            .iter()
            .position(|info| message.contains(&format!("\"{}\"", info.name)))
    };

    let reconciling = find_condition(obj, "Reconciling")
        .filter(|c| c["status"].as_str() == Some("True"))
        .and_then(|c| c["message"].as_str())
        .filter(|m| m.contains("Applying step"))
        .and_then(named_step_position);
    let ready = find_condition(obj, "Ready");
    let failed = ready
        .filter(|c| c["status"].as_str() == Some("False"))
        .and_then(|c| c["message"].as_str())
        .and_then(named_step_position);

    let marker = if let Some(pos) = reconciling {
        Some((pos, StepPhase::Applying))
    } else if let Some(pos) = failed {
        Some((pos, StepPhase::Failed))
    } else if ready.is_some_and(|c| c["status"].as_str() == Some("True")) {
        // Fully reconciled: every step applied.
        for info in &mut infos {
            info.phase = Some(StepPhase::Done);
        }
        None
    } else {
        None
    };

    if let Some((pos, phase)) = marker {
        for (idx, info) in infos.iter_mut().enumerate() {
            info.phase = Some(match idx.cmp(&pos) {
                std::cmp::Ordering::Less => StepPhase::Done,
                std::cmp::Ordering::Equal => phase,
                std::cmp::Ordering::Greater => StepPhase::Pending,
            });
        }
    }

    infos
}

/// Append the Steps section for step-based ResourceSets. All pushed content
/// is owned, so this works with any line lifetime.
fn push_steps_section<'a>(lines: &mut Vec<Line<'a>>, steps: &[StepInfo], theme: &Theme) {
    if steps.is_empty() {
        return;
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        format!("Steps ({}): ", steps.len()),
        Style::default().fg(theme.text_label),
    )]));

    let name_width = steps.iter().map(|s| s.name.len()).max().unwrap_or(0);
    for (idx, step) in steps.iter().enumerate() {
        let phase_style = match step.phase {
            Some(StepPhase::Done) => theme.status_ready_style(),
            Some(StepPhase::Applying) => Style::default().fg(theme.text_primary),
            Some(StepPhase::Failed) => theme.status_error_style(),
            Some(StepPhase::Pending) | None => Style::default().fg(theme.text_secondary),
        };

        let mut contents = Vec::new();
        if step.resources > 0 {
            contents.push(format!(
                "{} resource{}",
                step.resources,
                if step.resources == 1 { "" } else { "s" }
            ));
        }
        if step.has_template {
            contents.push("template".to_string());
        }
        if let Some(ref timeout) = step.timeout {
            contents.push(format!("timeout {}", timeout));
        }

        lines.push(Line::from(vec![
            Span::raw(format!("  {}. ", idx + 1)),
            Span::styled(
                format!("{:<width$}  ", step.name, width = name_width),
                Style::default().fg(theme.text_primary),
            ),
            Span::styled(
                format!(
                    "{:<9}",
                    step.phase.map(StepPhase::label).unwrap_or_default()
                ),
                phase_style,
            ),
            Span::styled(
                contents.join(", "),
                Style::default().fg(theme.text_secondary),
            ),
        ]));
    }
}

/// Render the resource detail view
pub fn render_resource_detail(
    f: &mut Frame,
    area: Rect,
    selected_resource_key: &Option<String>,
    state: &ResourceState,
    resource_objects: &HashMap<String, serde_json::Value>,
    theme: &Theme,
) {
    let key = match selected_resource_key {
        Some(k) => k,
        None => {
            let text = vec![Line::from("No resource selected")];
            let block = crate::tui::views::helpers::create_themed_block("Detail", theme);
            let paragraph = Paragraph::new(text).block(block);
            f.render_widget(paragraph, area);
            return;
        }
    };

    let resource = match state.get(key) {
        Some(r) => r,
        None => {
            let text = vec![Line::from("Resource not found")];
            let block = crate::tui::views::helpers::create_themed_block("Detail", theme);
            let paragraph = Paragraph::new(text).block(block);
            f.render_widget(paragraph, area);
            return;
        }
    };

    let obj_json = resource_objects.get(key);

    let mut lines = vec![
        Line::from(vec![
            Span::styled("Name: ", Style::default().fg(theme.text_label)),
            Span::raw(&resource.name),
        ]),
        Line::from(vec![
            Span::styled("Namespace: ", Style::default().fg(theme.text_label)),
            Span::raw(&resource.namespace),
        ]),
        Line::from(vec![
            Span::styled("Type: ", Style::default().fg(theme.text_label)),
            Span::raw(&resource.resource_type),
        ]),
        Line::from(""),
    ];

    // Status fields
    if let Some(suspended) = resource.suspended {
        lines.push(Line::from(vec![
            Span::styled("Suspended: ", Style::default().fg(theme.text_label)),
            Span::styled(
                if suspended { "True" } else { "False" },
                if suspended {
                    theme.status_suspended_style()
                } else {
                    theme.status_ready_style()
                },
            ),
        ]));
    }

    if let Some(ready) = resource.ready {
        lines.push(Line::from(vec![
            Span::styled("Ready: ", Style::default().fg(theme.text_label)),
            Span::styled(
                if ready { "True" } else { "False" },
                if ready {
                    theme.status_ready_style()
                } else {
                    theme.status_error_style()
                },
            ),
        ]));
    }

    if let Some(ref revision) = resource.revision {
        lines.push(Line::from(vec![
            Span::styled("Revision: ", Style::default().fg(theme.text_label)),
            Span::raw(revision),
        ]));
    }

    if let Some(ref message) = resource.message {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            "Message: ",
            Style::default().fg(theme.text_label),
        )]));
        // Split long messages into multiple lines
        for line in message.lines() {
            lines.push(Line::from(line));
        }
    }

    // Show JSON spec if available
    if let Some(obj) = obj_json {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            "Spec: ",
            Style::default().fg(theme.text_label),
        )]));

        // Extract fields using FluxResourceKind method
        if let Some(kind) = FluxResourceKind::parse_optional(&resource.resource_type) {
            let fields = kind.extract_fields(obj);

            // Display order: URL, BRANCH, PATH, CHART, VERSION, SOURCE, IMAGE, SEMVER, TAG, PRUNE, INTERVAL, DIGEST
            let display_order = [
                field_names::TYPE,
                field_names::URL,
                field_names::SECRET,
                field_names::BRANCH,
                field_names::PATH,
                field_names::CHART,
                field_names::VERSION,
                field_names::SOURCE,
                field_names::IMAGE,
                field_names::SEMVER,
                field_names::TAG,
                field_names::ENDPOINT,
                field_names::PROVIDER,
                field_names::ADDRESS,
                field_names::CHANNEL,
                field_names::WEBHOOK,
                field_names::INPUTS,
                field_names::PRUNE,
                field_names::INTERVAL,
                field_names::DIGEST,
            ];

            for &field_key in display_order.iter() {
                if let Some(value) = fields.get(field_key) {
                    let label = capitalize_first(field_key);
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("{}: ", label),
                            Style::default().fg(theme.text_label),
                        ),
                        Span::raw(value.clone()),
                    ]));
                }
            }

            // Step-based ResourceSets: show the ordered steps with each
            // step's phase derived from the status conditions.
            if kind == FluxResourceKind::ResourceSet {
                push_steps_section(&mut lines, &extract_resource_set_steps(obj), theme);
            }
        }
    }

    let title = format!("Detail - {}", resource.name);
    let block = crate::tui::views::helpers::create_themed_block(&title, theme);
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(ratatui::widgets::Wrap { trim: true });
    f.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rset_with(steps: serde_json::Value, conditions: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "apiVersion": "fluxcd.controlplane.io/v1",
            "kind": "ResourceSet",
            "metadata": {"name": "app", "namespace": "flux-system"},
            "spec": {"steps": steps},
            "status": {"conditions": conditions}
        })
    }

    fn three_steps() -> serde_json::Value {
        serde_json::json!([
            {"name": "pre-deploy", "resources": [{}, {}]},
            {"name": "deploy", "resources": [{}], "resourcesTemplate": "apiVersion: v1\n"},
            {"name": "post-deploy", "resourcesTemplate": "apiVersion: v1\n", "timeout": "5m"}
        ])
    }

    fn phases(infos: &[StepInfo]) -> Vec<Option<StepPhase>> {
        infos.iter().map(|i| i.phase).collect()
    }

    #[test]
    fn steps_parse_names_resources_template_timeout() {
        let obj = rset_with(three_steps(), serde_json::json!([]));
        let infos = extract_resource_set_steps(&obj);
        assert_eq!(infos.len(), 3);
        assert_eq!(infos[0].name, "pre-deploy");
        assert_eq!(infos[0].resources, 2);
        assert!(!infos[0].has_template);
        assert!(infos[1].has_template);
        assert_eq!(infos[2].timeout.as_deref(), Some("5m"));
        // No usable conditions: no phases, names still listed
        assert_eq!(phases(&infos), vec![None, None, None]);
    }

    #[test]
    fn reconciling_condition_marks_applying_step() {
        // Mirrors the operator's message: Applying step <i>/<n> "<name>"
        let obj = rset_with(
            three_steps(),
            serde_json::json!([
                {"type": "Reconciling", "status": "True",
                 "message": "Applying step 2/3 \"deploy\""},
                {"type": "Ready", "status": "False", "reason": "Progressing",
                 "message": "Reconciliation in progress"}
            ]),
        );
        let infos = extract_resource_set_steps(&obj);
        assert_eq!(
            phases(&infos),
            vec![
                Some(StepPhase::Done),
                Some(StepPhase::Applying),
                Some(StepPhase::Pending)
            ]
        );
    }

    #[test]
    fn failed_ready_condition_marks_failed_step() {
        // Mirrors the operator's failure wrapping: step "<name>" <action>: <err>
        let obj = rset_with(
            three_steps(),
            serde_json::json!([
                {"type": "Ready", "status": "False", "reason": "ReconciliationFailed",
                 "message": "step \"deploy\" apply: dry-run failed"}
            ]),
        );
        let infos = extract_resource_set_steps(&obj);
        assert_eq!(
            phases(&infos),
            vec![
                Some(StepPhase::Done),
                Some(StepPhase::Failed),
                Some(StepPhase::Pending)
            ]
        );
    }

    #[test]
    fn ready_resource_set_marks_all_steps_done() {
        let obj = rset_with(
            three_steps(),
            serde_json::json!([
                {"type": "Ready", "status": "True", "reason": "ReconciliationSucceeded",
                 "message": "Reconciliation finished in 2s"}
            ]),
        );
        let infos = extract_resource_set_steps(&obj);
        assert!(phases(&infos).iter().all(|p| *p == Some(StepPhase::Done)));
    }

    #[test]
    fn flat_resource_set_has_no_steps_section() {
        let obj = serde_json::json!({
            "spec": {"resources": [{}]},
            "status": {}
        });
        assert!(extract_resource_set_steps(&obj).is_empty());

        let mut lines = Vec::new();
        push_steps_section(&mut lines, &[], &Theme::default());
        assert!(lines.is_empty(), "no section rendered without steps");
    }

    #[test]
    fn steps_section_renders_phase_and_contents() {
        let obj = rset_with(
            three_steps(),
            serde_json::json!([
                {"type": "Reconciling", "status": "True",
                 "message": "Applying step 2/3 \"deploy\""}
            ]),
        );
        let mut lines = Vec::new();
        push_steps_section(
            &mut lines,
            &extract_resource_set_steps(&obj),
            &Theme::default(),
        );
        let text: Vec<String> = lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();

        assert!(text[1].contains("Steps (3)") || text[0].contains("Steps (3)"));
        let deploy_row = text.iter().find(|l| l.contains("2. deploy")).unwrap();
        assert!(deploy_row.contains("applying"));
        assert!(deploy_row.contains("1 resource, template"));
        let post_row = text.iter().find(|l| l.contains("3. post-deploy")).unwrap();
        assert!(post_row.contains("pending"));
        assert!(post_row.contains("timeout 5m"));
    }
}
