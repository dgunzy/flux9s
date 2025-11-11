//! Header view rendering

use crate::tui::theme::Theme;
use crate::watcher::ResourceState;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Render the main header with context, namespace, totals, and resource counts
pub fn render_header(
    f: &mut Frame,
    area: Rect,
    state: &ResourceState,
    context: &str,
    namespace: &Option<String>,
    filter: &str,
    selected_resource_type: &Option<String>,
    filtered_count: usize, // Count of resources after filtering
    theme: &Theme,
) {
    // Split header into left (info) and right (ASCII art)
    let header_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(area);

    let left_area = header_chunks[0];
    let right_area = header_chunks[1];

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

    if let Some(ref resource_type) = selected_resource_type {
        filter_parts.push(format!("type={}", resource_type));
    }

    // Calculate available width for resource types (accounting for "Resources: " prefix and borders)
    let available_width = left_area.width.saturating_sub(12); // "Resources: " = 11 chars + 1 padding

    // When filtering by type, show only that type's count; otherwise show all types
    let type_summary_parts: Vec<String> = if let Some(ref resource_type) = selected_resource_type {
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
    let mut resource_lines = Vec::new();
    let mut current_line = String::from("Resources: ");
    for part in &type_summary_parts {
        let part_with_space = format!("{} ", part);
        if current_line.len() + part_with_space.len() > available_width as usize
            && current_line.len() > 11
        {
            // Start new line - extract the content after "Resources: "
            let content = current_line.trim_start_matches("Resources: ").to_string();
            resource_lines.push(Line::from(vec![
                Span::styled("Resources: ", Style::default().fg(theme.header_resources)),
                Span::raw(content),
            ]));
            current_line = format!("{} ", part);
        } else {
            current_line.push_str(&part_with_space);
        }
    }
    // Add the last line
    if current_line.len() > 11 {
        let content = current_line.trim_start_matches("Resources: ").to_string();
        resource_lines.push(Line::from(vec![
            Span::styled("Resources: ", Style::default().fg(theme.header_resources)),
            Span::raw(content),
        ]));
    }

    // Build header lines with clean structure
    let context_line_spans = vec![
        Span::styled(" Context: ", Style::default().fg(theme.header_resources)),
        Span::styled(context, theme.header_context_style()),
        Span::raw("  "),
        Span::styled("Namespace: ", Style::default().fg(theme.header_resources)),
        Span::styled(
            namespace_display,
            theme.header_namespace_style(namespace_display == "all"),
        ),
        Span::raw(" ("),
        Span::styled(":ns <name>", Style::default().fg(theme.text_secondary)),
        Span::raw(" to switch)"),
    ];

    let mut header_lines = vec![Line::from(context_line_spans)];

    // Add filter status line if filtering is active - make it prominent and informative
    if !filter_parts.is_empty() {
        let filter_display = filter_parts.join(" + ");
        let clear_hint = if !filter.is_empty() {
            "Clear: /"
        } else if selected_resource_type.is_some() {
            "Clear: :all"
        } else {
            ""
        };

        header_lines.push(Line::from(vec![
            Span::styled(
                "⚠ Filter: ",
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
        Span::styled(" Flux9s ", theme.header_context_style()),
        Span::raw(" | "),
        Span::styled("Total: ", Style::default().fg(theme.header_resources)),
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

    // Render ASCII art on the right
    render_header_ascii(f, right_area, theme);
}

/// Render the ASCII art logo
pub fn render_header_ascii(f: &mut Frame, area: Rect, theme: &Theme) {
    // Simple ASCII art - one line per character row
    let ascii_lines = [
        " _____ _             ___      ",
        "|  ___| |_   ___  __/ _ \\ ___ ",
        "| |_  | | | | \\ \\/ / (_) / __|",
        "|  _| | | |_| |>  < \\__, \\__ \\",
        "|_|   |_|\\__,_/_/\\_\\  /_/|___/",
    ];

    let lines: Vec<Line> = ascii_lines
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

    // Center the ASCII art vertically and horizontally
    let paragraph = Paragraph::new(lines).alignment(ratatui::layout::Alignment::Center);
    f.render_widget(paragraph, area);
}
