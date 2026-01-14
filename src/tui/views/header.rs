//! Header view rendering

use crate::tui::app::state::ControllerPodState;
use crate::tui::theme::Theme;
use crate::watcher::ResourceState;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use std::collections::HashMap;

/// Render the main header with context, namespace, totals, and resource counts
pub fn render_header(
    f: &mut Frame,
    area: Rect,
    state: &ResourceState,
    controller_pods: &ControllerPodState,
    context: &str,
    namespace: &Option<String>,
    filter: &str,
    selected_resource_type: &Option<String>,
    filtered_count: usize,       // Count of resources after filtering
    health_percentage: f64,      // Health percentage (0-100)
    health_filter: Option<&str>, // Health filter status ("healthy", "unhealthy", or None)
    read_only: bool,             // Readonly mode status
    theme: &Theme,
    no_icons: bool,
    namespace_hotkeys: &[String],
) {
    // Split header into left (info), middle (controller status), and right (ASCII art)
    let header_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(60), // Left: context/namespace/resources
            Constraint::Percentage(15), // Middle: controller status
            Constraint::Percentage(25), // Right: ASCII logo
        ])
        .split(area);

    let left_area = header_chunks[0];
    let middle_area = header_chunks[1];
    let right_area = header_chunks[2];

    let counts = state.count_by_type();
    let total: usize = counts.values().sum();

    // Sort resource types alphabetically for stable display
    let mut type_counts: Vec<_> = counts.iter().collect();
    type_counts.sort_by_key(|(resource_type, _)| *resource_type);

    // Create multi-line header similar to k9s
    let namespace_display = namespace.as_ref().map(|ns| ns.as_str()).unwrap_or("all");

    // Build filter status - show what's being filtered and how to clear
    let mut filter_parts = Vec::new();

    if !filter.is_empty() {
        filter_parts.push(format!("name='{}'", filter));
    }

    if let Some(resource_type) = selected_resource_type {
        filter_parts.push(format!("type={}", resource_type));
    }

    if let Some(health_filter) = health_filter {
        filter_parts.push(format!("health={}", health_filter));
    }

    // Calculate available width for resource types (accounting for "Resources: " prefix and borders)
    let available_width = left_area.width.saturating_sub(12); // "Resources: " = 11 chars + 1 padding

    // When filtering by type, show only that type's count; otherwise show all types
    let type_summary_parts: Vec<String> = if let Some(resource_type) = selected_resource_type {
        // Show only the filtered resource type with its filtered count
        vec![format!("{}:{}", resource_type, filtered_count)]
    } else {
        // Show all resource types with their counts
        type_counts
            .iter()
            .map(|(resource_type, count)| format!("{}:{}", resource_type, count))
            .collect()
    };

    // Wrap resource types into lines
    // Show "Resources:" only on the first line, then wrap the list
    let mut resource_lines = Vec::new();
    let mut current_line = String::new();
    let mut is_first_line = true;

    for part in &type_summary_parts {
        let part_with_space = format!("{} ", part);
        let prefix = if is_first_line {
            "Resources: "
        } else {
            // Indent continuation lines to align with content
            "           " // Same width as "Resources: "
        };

        if current_line.len() + part_with_space.len() > available_width as usize
            && !current_line.is_empty()
        {
            // Start new line
            resource_lines.push(Line::from(vec![
                Span::styled(prefix, Style::default().fg(theme.header_resources)),
                Span::raw(current_line.clone()),
            ]));
            current_line = part_with_space;
            is_first_line = false;
        } else {
            if current_line.is_empty() && is_first_line {
                // First line - don't add prefix yet, will add when flushing
            }
            current_line.push_str(&part_with_space);
        }
    }
    // Add the last line
    if !current_line.is_empty() {
        let prefix = if is_first_line {
            "Resources: "
        } else {
            "           "
        };
        resource_lines.push(Line::from(vec![
            Span::styled(prefix, Style::default().fg(theme.header_resources)),
            Span::raw(current_line),
        ]));
    }

    // Build header lines with clean structure
    let mut context_line_spans = vec![
        Span::styled("Context: ", Style::default().fg(theme.header_resources)),
        Span::styled(context, theme.header_context_style()),
        Span::raw("  "),
        Span::styled("Namespace: ", Style::default().fg(theme.header_resources)),
        Span::styled(
            namespace_display,
            theme.header_namespace_style(namespace_display == "all"),
        ),
    ];

    // Add health percentage indicator
    let health_color = if health_percentage >= 90.0 {
        theme.status_ready
    } else if health_percentage >= 70.0 {
        theme.status_unknown // Yellow for warning state
    } else {
        theme.status_error
    };
    let health_icon = if no_icons {
        "[HEALTH]"
    } else if health_percentage >= 90.0 {
        "●"
    } else if health_percentage >= 70.0 {
        "⚠"
    } else {
        "✗"
    };
    context_line_spans.push(Span::raw("  "));
    context_line_spans.push(Span::styled(
        format!("{} {:.1}%", health_icon, health_percentage),
        Style::default()
            .fg(health_color)
            .add_modifier(Modifier::BOLD),
    ));

    // Add readonly indicator if enabled
    if read_only {
        context_line_spans.push(Span::raw("  "));
        let readonly_text = if no_icons {
            "[READONLY]"
        } else {
            "🔒 READONLY"
        };
        context_line_spans.push(Span::styled(
            readonly_text,
            Style::default()
                .fg(theme.status_error)
                .add_modifier(Modifier::BOLD),
        ));
    }

    // Add namespace hotkeys after namespace
    if !namespace_hotkeys.is_empty() {
        context_line_spans.push(Span::raw("  "));
        for (idx, ns) in namespace_hotkeys.iter().enumerate() {
            if idx > 0 {
                context_line_spans.push(Span::raw(" "));
            }
            let display_ns = if ns == "all" {
                "all"
            } else if ns.len() > 6 {
                &ns[..6]
            } else {
                ns
            };
            // Highlight current namespace
            let is_current = if ns == "all" {
                namespace.is_none()
            } else {
                namespace.as_ref() == Some(ns)
            };
            let hotkey_style = if is_current {
                Style::default()
                    .fg(theme
                        .header_namespace_style(false)
                        .fg
                        .unwrap_or(theme.text_primary))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text_secondary)
            };
            context_line_spans.push(Span::styled(
                format!("{}:{}", idx, display_ns),
                hotkey_style,
            ));
        }
    }

    let mut header_lines = vec![Line::from(context_line_spans)];

    // Add filter status line if filtering is active - make it prominent and informative
    if !filter_parts.is_empty() {
        let filter_display = filter_parts.join(" + ");
        let clear_hint = if !filter.is_empty() {
            "Clear: /"
        } else if selected_resource_type.is_some() || health_filter.is_some() {
            "Clear: :all"
        } else {
            ""
        };

        let filter_icon = if no_icons { "[FILTER]" } else { "⚠ Filter: " };
        header_lines.push(Line::from(vec![
            Span::styled(
                filter_icon,
                Style::default()
                    .fg(theme.header_filter)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                filter_display.clone(),
                Style::default().fg(theme.header_filter),
            ),
            Span::raw("  |  "),
            Span::styled(
                format!("Showing {} of {}", filtered_count, total),
                Style::default().fg(theme.header_filter),
            ),
            Span::raw("  |  "),
            Span::styled(clear_hint, Style::default().fg(theme.text_secondary)),
        ]));
    }

    // Add Flux9s and Total line - always show total, and filtered count when filtering
    let total_display = if !filter_parts.is_empty() {
        format!("{} (filtered: {})", total, filtered_count)
    } else {
        format!("{}", total)
    };

    header_lines.push(Line::from(vec![
        Span::styled(
            "Total Resources: ",
            Style::default().fg(theme.header_resources),
        ),
        Span::styled(
            &total_display,
            Style::default()
                .fg(theme.header_total)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    // Add resource type lines
    header_lines.extend(resource_lines);

    // Ensure we have at least 5 content lines to match ASCII art height
    // This prevents ratatui from adding extra blank lines at the bottom
    while header_lines.len() < 5 {
        header_lines.push(Line::from(""));
    }

    let header = Paragraph::new(header_lines).block(Block::default().borders(Borders::ALL));
    f.render_widget(header, left_area);

    // Render controller status in the middle
    render_controller_status(f, middle_area, controller_pods, theme, no_icons);

    // Render ASCII art on the right
    render_header_ascii(f, right_area, controller_pods, theme);
}

/// Render the ASCII art logo
pub fn render_header_ascii(
    f: &mut Frame,
    area: Rect,
    controller_pods: &ControllerPodState,
    theme: &Theme,
) {
    // Simple ASCII art - one line per character row
    let ascii_lines = [
        " _____ _             ___      ",
        "|  ___| |_   ___  __/ _ \\ ___ ",
        "| |_  | | | | \\ \\/ / (_) / __|",
        "|  _| | | |_| |>  < \\__, \\__ \\",
        "|_|   |_|\\__,_/_/\\_\\  /_/|___/",
    ];

    let mut lines: Vec<Line> = ascii_lines
        .iter()
        .map(|line| {
            Line::from(vec![Span::styled(
                *line,
                Style::default()
                    .fg(theme.header_ascii)
                    .add_modifier(Modifier::BOLD),
            )])
        })
        .collect();

    // Add Flux version if available
    if let Some(version) = controller_pods.get_flux_version() {
        lines.push(Line::from("")); // Empty line for spacing
        lines.push(Line::from(vec![Span::styled(
            format!("Flux {}", version),
            Style::default().fg(theme.header_ascii),
        )]));
    }

    // Center the ASCII art vertically and horizontally
    let paragraph = Paragraph::new(lines).alignment(ratatui::layout::Alignment::Center);
    f.render_widget(paragraph, area);
}

/// Render controller pod status in the middle header column
fn render_controller_status(
    f: &mut Frame,
    area: Rect,
    controller_pods: &ControllerPodState,
    theme: &Theme,
    no_icons: bool,
) {
    use crate::tui::app::state::ControllerPodInfo;

    let all_pods = controller_pods.get_all_pods();

    if all_pods.is_empty() {
        // No controllers found - show message
        let block = Block::default()
            .borders(Borders::ALL)
            .title("Controllers")
            .style(Style::default().fg(theme.header_resources));
        let text = Paragraph::new("N/A")
            .block(block)
            .style(Style::default().fg(theme.status_unknown));
        f.render_widget(text, area);
        return;
    }

    // Group pods by controller type
    let mut controller_groups: HashMap<&str, Vec<&ControllerPodInfo>> = HashMap::new();
    for pod in &all_pods {
        if let Some(controller_name) = extract_controller_name(&pod.name) {
            controller_groups
                .entry(controller_name)
                .or_default()
                .push(pod);
        }
    }

    let mut lines = Vec::new();
    let status_letters = [' ', 'S', 'T', 'A', 'T', 'U', 'S', ' '];
    let mut line_index = 0;

    for controller_name in crate::tui::constants::FLUX_CONTROLLER_NAMES {
        if let Some(pods) = controller_groups.get(controller_name) {
            let short_name = abbreviate_controller_name(controller_name);

            let total_replicas = pods.len();
            let ready_replicas = pods.iter().filter(|p| p.ready).count();
            let all_ready = ready_replicas == total_replicas;

            let (status_icon, status_color) = if all_ready {
                (if no_icons { "[OK]" } else { "✓" }, theme.status_ready)
            } else if ready_replicas > 0 {
                (if no_icons { "[WARN]" } else { "⚠" }, theme.status_unknown)
            } else {
                (if no_icons { "[ERR]" } else { "✗" }, theme.status_error)
            };

            let version = pods
                .iter()
                .find_map(|p| p.version.as_ref())
                .map(|v| v.as_str())
                .unwrap_or("?");

            let status_char = status_letters.get(line_index).unwrap_or(&' ');
            let line_spans = vec![
                Span::styled(
                    format!(" {}  ", status_char),
                    Style::default()
                        .fg(theme.header_resources)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{:<5} ", short_name),
                    Style::default().fg(theme.header_resources),
                ),
                Span::styled(
                    format!("{}/{} ", ready_replicas, total_replicas),
                    Style::default().fg(theme.header_total),
                ),
                Span::styled(
                    format!("{} ", status_icon),
                    Style::default()
                        .fg(status_color)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(version, Style::default().fg(theme.header_context)),
            ];
            lines.push(Line::from(line_spans));
            line_index += 1;
        }
    }

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, area);
}

/// Match pod name prefix against known Flux controllers
fn extract_controller_name(pod_name: &str) -> Option<&str> {
    crate::tui::constants::FLUX_CONTROLLER_NAMES
        .iter()
        .find(|&&controller| pod_name.starts_with(controller))
        .copied()
}

/// Abbreviate controller names for compact display
fn abbreviate_controller_name(name: &str) -> &str {
    match name {
        "source-controller" => "Src",
        "kustomize-controller" => "Kstz",
        "helm-controller" => "Helm",
        "notification-controller" => "Notif",
        "image-reflector-controller" => "ImgR",
        "image-automation-controller" => "ImgA",
        "source-watcher" => "SrcW",
        "flux-operator" => "Oper",
        _ => "?",
    }
}
