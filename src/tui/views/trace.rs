//! Trace view rendering

use crate::tui::app::state::TextSearchState;
use crate::tui::theme::Theme;
use crate::tui::trace::{TraceNode, TraceResult};
use crate::tui::views::yaml::decorate_title_with_search;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

/// Whether any of a trace node's displayed text matches the search query.
fn node_matches(node: &TraceNode, query_lower: &str) -> bool {
    let mut haystacks: Vec<&str> = vec![&node.kind, &node.name, &node.namespace];
    if let Some(ref spec) = node.spec {
        haystacks.extend(spec.path.as_deref());
        haystacks.extend(spec.url.as_deref());
        haystacks.extend(spec.branch.as_deref());
    }
    if let Some(ref status) = node.status {
        haystacks.extend(status.revision.as_deref());
        haystacks.extend(status.message.as_deref());
    }
    haystacks
        .iter()
        .any(|text| text.to_lowercase().contains(query_lower))
}

/// Render the trace view
pub fn render_resource_trace(
    f: &mut Frame,
    area: Rect,
    _selected_resource_key: &Option<String>,
    trace_result: Option<&TraceResult>,
    trace_loading: bool,
    scroll_offset: &mut usize,
    search: &mut TextSearchState,
    theme: &Theme,
) {
    if trace_loading {
        crate::tui::views::helpers::render_loading_state(
            f,
            area,
            "Trace",
            "Tracing resource... Walking ownership chain...",
            theme,
        );
        return;
    }

    let trace_result = match trace_result {
        Some(result) => result,
        None => {
            crate::tui::views::helpers::render_empty_state(
                f,
                area,
                "Trace",
                "No trace data available",
                "Select a resource and press 't' to trace",
                theme,
            );
            return;
        }
    };

    // Build list of nodes to display
    let mut nodes: Vec<(&TraceNode, &str)> = Vec::new();

    // Add the main object
    nodes.push((&trace_result.object, "Object"));

    // Add chain nodes (Kustomization/HelmRelease/HelmChart)
    // Skip nodes that match the object being traced (avoid duplicates)
    for node in &trace_result.chain {
        use crate::models::FluxResourceKind;
        if matches!(
            FluxResourceKind::parse_optional(&node.kind),
            Some(FluxResourceKind::Kustomization)
                | Some(FluxResourceKind::HelmRelease)
                | Some(FluxResourceKind::HelmChart)
        ) {
            // Skip if this node matches the object being traced (already shown as "Object")
            if node.kind == trace_result.object.kind
                && node.name == trace_result.object.name
                && node.namespace == trace_result.object.namespace
            {
                continue;
            }
            nodes.push((node, "managed by"));
        }
    }

    // Add source node
    if let Some(source) = &trace_result.source {
        nodes.push((source, "sourced from"));
    }

    // Text search: match whole nodes (the trace view is block-based, not line-based)
    let arrow_height = 3; // Space for arrow between blocks
    let query_lower = search.query.to_lowercase();
    let match_indexes: Vec<usize> = if query_lower.is_empty() {
        Vec::new()
    } else {
        nodes
            .iter()
            .enumerate()
            .filter(|(_, (node, _))| node_matches(node, &query_lower))
            .map(|(idx, _)| idx)
            .collect()
    };
    search.total_matches = match_indexes.len();
    if !match_indexes.is_empty() {
        search.current_match = search.current_match.min(match_indexes.len() - 1);
    }
    let current_match_node = match_indexes.get(search.current_match).copied();

    let mut title = "Trace".to_string();
    decorate_title_with_search(&mut title, search);
    let outer_block = crate::tui::views::helpers::create_themed_block(&title, theme);

    // Inner area (excluding border)
    let inner_area = outer_block.inner(area);
    f.render_widget(outer_block, area);

    // Jump the scroll offset to the start of the current match's block
    if search.pending_jump {
        search.pending_jump = false;
        if let Some(node_idx) = current_match_node {
            let mut y_offset: u16 = 0;
            for (node, _) in nodes.iter().take(node_idx) {
                y_offset += calculate_block_height(node, inner_area.width) + arrow_height;
            }
            *scroll_offset = y_offset as usize;
        }
    }

    // Calculate required height for all nodes
    let block_min_height = 4; // Minimum height for a block
    let total_height: u16 = nodes
        .iter()
        .map(|_| block_min_height + arrow_height)
        .sum::<u16>()
        - arrow_height; // Last node doesn't need arrow after it

    // Calculate scroll
    let available_height = inner_area.height;
    let max_scroll = total_height.saturating_sub(available_height) as usize;
    *scroll_offset = (*scroll_offset).min(max_scroll);
    let scroll_offset_u16 = *scroll_offset as u16;

    // Render nodes with scrolling
    let mut current_y = inner_area.y.saturating_sub(scroll_offset_u16);

    for (i, (node, relationship)) in nodes.iter().enumerate() {
        let is_first = i == 0;
        let is_current_match = Some(i) == current_match_node;

        // Draw arrow before this node (except for the first)
        if !is_first {
            let arrow_y_start = current_y;
            let arrow_y_end = (arrow_y_start + arrow_height).min(inner_area.y + inner_area.height);

            // Only render if arrow is visible
            if arrow_y_end > inner_area.y {
                let arrow_area = Rect {
                    x: inner_area.x,
                    y: arrow_y_start.max(inner_area.y),
                    width: inner_area.width,
                    height: arrow_y_end.saturating_sub(arrow_y_start.max(inner_area.y)),
                };

                render_arrow(f, arrow_area, relationship, theme);
            }

            current_y += arrow_height;
        }

        // Calculate block height based on content
        let block_height = calculate_block_height(node, inner_area.width);
        let block_y_start = current_y;
        let block_y_end = (block_y_start + block_height).min(inner_area.y + inner_area.height);

        // Only render if block is visible
        if block_y_end > inner_area.y && block_y_start < inner_area.y + inner_area.height {
            let block_area = Rect {
                x: inner_area.x,
                y: block_y_start.max(inner_area.y),
                width: inner_area.width,
                height: block_y_end.saturating_sub(block_y_start.max(inner_area.y)),
            };

            render_trace_node(f, block_area, node, is_first, is_current_match, theme);
        }

        current_y += block_height;

        // Stop if we've scrolled past the visible area
        if current_y >= inner_area.y + inner_area.height {
            break;
        }
    }

    // Show scroll indicator if needed
    if total_height > available_height {
        let scroll_info = format!(
            "Scroll: {}/{} (j/k to navigate, Esc to close)",
            *scroll_offset + 1,
            max_scroll + 1
        );
        let scroll_line = Line::from(Span::styled(
            scroll_info,
            Style::default().fg(theme.text_label),
        ));
        let scroll_area = Rect {
            x: area.x + 1,
            y: area.y + area.height - 1,
            width: area.width.saturating_sub(2),
            height: 1,
        };
        f.render_widget(Paragraph::new(scroll_line), scroll_area);
    }
}

/// Calculate the height needed for a trace node block
fn calculate_block_height(node: &TraceNode, _width: u16) -> u16 {
    let mut lines = 2; // Kind/Name and Namespace

    if let Some(ref spec) = node.spec {
        if spec.path.is_some() {
            lines += 1;
        }
        if spec.url.is_some() {
            lines += 1;
        }
        if spec.branch.is_some() {
            lines += 1;
        }
    }

    if let Some(ref status) = node.status {
        if status.revision.is_some() {
            lines += 1;
        }
        if status.last_reconciled.is_some() {
            lines += 1;
        }
        if status.message.is_some() {
            lines += 1;
        }
    }

    // Add padding and borders (top + bottom borders = 2)
    (lines + 2).max(4) as u16
}

/// Render a trace node as a block
fn render_trace_node(
    f: &mut Frame,
    area: Rect,
    node: &TraceNode,
    is_primary: bool,
    is_current_match: bool,
    theme: &Theme,
) {
    let mut content = Vec::new();

    // Title line: Kind/Name
    let title = format!("{}: {}", node.kind, node.name);
    content.push(Line::from(vec![Span::styled(
        title,
        Style::default()
            .fg(if is_primary {
                theme.text_label
            } else {
                theme.text_primary
            })
            .add_modifier(Modifier::BOLD),
    )]));

    // Namespace
    content.push(Line::from(vec![
        Span::styled("Namespace: ", Style::default().fg(theme.text_label)),
        Span::styled(&node.namespace, Style::default().fg(theme.text_value)),
    ]));

    // Spec information
    if let Some(ref spec) = node.spec {
        if let Some(ref path) = spec.path {
            content.push(Line::from(vec![
                Span::styled("Path: ", Style::default().fg(theme.text_label)),
                Span::styled(path, Style::default().fg(theme.text_value)),
            ]));
        }
        if let Some(ref url) = spec.url {
            content.push(Line::from(vec![
                Span::styled("URL: ", Style::default().fg(theme.text_label)),
                Span::styled(url, Style::default().fg(theme.text_value)),
            ]));
        }
        if let Some(ref branch) = spec.branch {
            content.push(Line::from(vec![
                Span::styled("Branch: ", Style::default().fg(theme.text_label)),
                Span::styled(branch, Style::default().fg(theme.text_value)),
            ]));
        }
    }

    // Status information
    if let Some(ref status) = node.status {
        if let Some(ref revision) = status.revision {
            content.push(Line::from(vec![
                Span::styled("Revision: ", Style::default().fg(theme.text_label)),
                Span::styled(revision, Style::default().fg(theme.text_value)),
            ]));
        }
        if let Some(ref last_reconciled) = status.last_reconciled {
            content.push(Line::from(vec![
                Span::styled("Last reconciled: ", Style::default().fg(theme.text_label)),
                Span::styled(last_reconciled, Style::default().fg(theme.text_value)),
            ]));
        }
        if let Some(ref message) = status.message {
            // Truncate long messages
            let max_len = (area.width.saturating_sub(15)) as usize;
            let display_message: String = if message.len() > max_len {
                format!("{}...", &message[..max_len])
            } else {
                message.clone()
            };
            content.push(Line::from(vec![
                Span::styled("Message: ", Style::default().fg(theme.text_label)),
                Span::styled(display_message, Style::default().fg(theme.text_value)),
            ]));
        }
    }

    // Create block with appropriate styling; the current search match is emphasized
    let block_style = if is_current_match {
        Style::default()
            .fg(theme.text_value)
            .add_modifier(Modifier::BOLD)
    } else if is_primary {
        Style::default().fg(theme.text_label)
    } else {
        Style::default().fg(theme.text_secondary)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(block_style);

    let paragraph = Paragraph::new(content)
        .block(block)
        .wrap(Wrap { trim: true });

    f.render_widget(paragraph, area);
}

/// Render an arrow between blocks
fn render_arrow(f: &mut Frame, area: Rect, relationship: &str, theme: &Theme) {
    if area.height == 0 {
        return;
    }

    let center_x = area.x + area.width / 2;
    let mut arrow_lines = Vec::new();

    // Create arrow lines - vertical line(s) + arrow head
    if area.height >= 3 {
        // Multiple lines: vertical line, arrow head, label
        for _i in 0..(area.height - 1) {
            let mut line_chars = vec![' '; area.width as usize];
            if center_x >= area.x && center_x < area.x + area.width {
                let idx = (center_x - area.x) as usize;
                line_chars[idx] = '│';
            }
            arrow_lines.push(Line::from(vec![Span::styled(
                line_chars.iter().collect::<String>(),
                Style::default().fg(theme.text_label),
            )]));
        }
        // Arrow head line with label
        let mut arrow_line_chars = vec![' '; area.width as usize];
        if center_x >= area.x && center_x < area.x + area.width {
            let idx = (center_x - area.x) as usize;
            arrow_line_chars[idx] = '▼';
        }
        let arrow_line_str: String = arrow_line_chars.iter().collect();

        // Create line with arrow and label as separate spans for better styling
        let mut spans = vec![Span::styled(
            arrow_line_str,
            Style::default().fg(theme.text_label),
        )];

        // Add relationship label to the right of arrow
        if center_x + 3 < area.x + area.width {
            let label_x = center_x + 3;
            if label_x < area.x + area.width
                && relationship.len() as u16 <= area.width.saturating_sub(label_x - area.x)
            {
                spans.push(Span::styled(
                    format!(" {}", relationship),
                    Style::default()
                        .fg(theme.text_label)
                        .add_modifier(Modifier::ITALIC),
                ));
            }
        }

        arrow_lines.push(Line::from(spans));
    } else if area.height >= 2 {
        // Just arrow head and label
        let mut arrow_line_chars = vec![' '; area.width as usize];
        if center_x >= area.x && center_x < area.x + area.width {
            let idx = (center_x - area.x) as usize;
            arrow_line_chars[idx] = '▼';
        }
        let arrow_line_str: String = arrow_line_chars.iter().collect();

        let mut spans = vec![Span::styled(
            arrow_line_str,
            Style::default().fg(theme.text_label),
        )];

        // Add relationship label
        if center_x + 3 < area.x + area.width {
            let label_x = center_x + 3;
            if label_x < area.x + area.width
                && relationship.len() as u16 <= area.width.saturating_sub(label_x - area.x)
            {
                spans.push(Span::styled(
                    format!(" {}", relationship),
                    Style::default()
                        .fg(theme.text_label)
                        .add_modifier(Modifier::ITALIC),
                ));
            }
        }

        arrow_lines.push(Line::from(spans));
    } else {
        // Single line - just arrow
        let mut arrow_line_chars = vec![' '; area.width as usize];
        if center_x >= area.x && center_x < area.x + area.width {
            let idx = (center_x - area.x) as usize;
            arrow_line_chars[idx] = '▼';
        }
        arrow_lines.push(Line::from(vec![Span::styled(
            arrow_line_chars.iter().collect::<String>(),
            Style::default().fg(theme.text_label),
        )]));
    }

    // Render the arrow
    let arrow_para = Paragraph::new(arrow_lines).style(Style::default().fg(theme.text_label));
    f.render_widget(arrow_para, area);
}
