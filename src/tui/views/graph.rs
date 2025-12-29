//! Graph view rendering
//!
//! Renders a visual graph of resource relationships using Ratatui layouts and widgets.
//! Based on Flux Operator Web UI graph visualization patterns.

use crate::trace::{NodeType, RelationshipType, ResourceGraph};
use crate::tui::theme::Theme;
use crate::watcher::ResourceKey;
use ratatui::{
    Frame,
    layout::{Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

/// Render the resource graph view
pub fn render_resource_graph(
    f: &mut Frame,
    area: Rect,
    _selected_resource_key: &Option<String>,
    graph_result: &Option<ResourceGraph>,
    graph_pending: &Option<ResourceKey>,
    scroll_offset: &mut usize, // Line-based scroll offset (like YAML view)
    theme: &Theme,
) {
    let outer_block = crate::tui::views::helpers::create_themed_block("Resource Graph", theme);

    // Show loading if pending OR if no result yet (prevents flashing "no data" message)
    if graph_pending.is_some() || graph_result.is_none() {
        crate::tui::views::helpers::render_loading_state(
            f,
            area,
            "Resource Graph",
            "Building graph... Discovering resource relationships...",
            theme,
        );
        return;
    }

    let mut graph = match graph_result {
        Some(result) => result.clone(),
        None => {
            // This should not happen due to check above, but handle it anyway
            let text = vec![
                Line::from("No graph data available"),
                Line::from(""),
                Line::from("Select a resource and press 'g' to view graph"),
            ];
            let paragraph = Paragraph::new(text)
                .block(outer_block)
                .style(Style::default().fg(theme.text_secondary));
            f.render_widget(paragraph, area);
            return;
        }
    };

    // Calculate layout
    let inner_area = outer_block.inner(area);
    f.render_widget(outer_block, area);

    // Check if graph has nodes
    if graph.nodes.is_empty() {
        let text = vec![
            Line::from("No graph data available"),
            Line::from(""),
            Line::from("Graph is empty - no resources found"),
        ];
        let paragraph = Paragraph::new(text).style(Style::default().fg(theme.text_secondary));
        f.render_widget(paragraph, inner_area);
        return;
    }

    let (_layout_width, layout_height) =
        graph.calculate_layout(inner_area.width, inner_area.height);

    // Calculate visible height and clamp scroll offset (like YAML view)
    let visible_height = inner_area.height as usize;
    let max_scroll = if layout_height > visible_height as u16 {
        (layout_height as usize).saturating_sub(visible_height)
    } else {
        0
    };
    *scroll_offset = (*scroll_offset).min(max_scroll);

    // Render graph using improved layout with line-based scrolling
    render_graph_nodes_and_edges(f, inner_area, &graph, *scroll_offset, theme);
}

/// Render graph nodes and edges with improved layout
fn render_graph_nodes_and_edges(
    f: &mut Frame,
    area: Rect,
    graph: &ResourceGraph,
    scroll_offset: usize, // Line-based scroll offset
    theme: &Theme,
) {
    // Render edges first (so they appear behind nodes) using Unicode box drawing
    // Group edges by parent node to render T-junctions properly
    use std::collections::HashMap;
    let mut edges_by_parent: HashMap<String, Vec<(&crate::trace::GraphEdge, usize, usize)>> =
        HashMap::new();

    for edge in &graph.edges {
        if let (Some(&from_idx), Some(&to_idx)) = (
            graph.node_index.get(&edge.from),
            graph.node_index.get(&edge.to),
        ) {
            if let (Some(from_node), Some(to_node)) =
                (graph.nodes.get(from_idx), graph.nodes.get(to_idx))
            {
                if from_node.position.is_some() && to_node.position.is_some() {
                    edges_by_parent
                        .entry(edge.from.clone())
                        .or_default()
                        .push((edge, from_idx, to_idx));
                }
            }
        }
    }

    // Render grouped edges (for T-junctions) and individual edges
    let scroll_offset_u16 = scroll_offset as u16;
    for (parent_id, child_edges) in &edges_by_parent {
        if let Some(&parent_idx) = graph.node_index.get(parent_id) {
            if let Some(parent_node) = graph.nodes.get(parent_idx) {
                if let Some((parent_x, parent_y)) = parent_node.position {
                    // Check if children are side-by-side (need T-junction)
                    let child_nodes: Vec<(
                        &crate::trace::GraphEdge,
                        &crate::trace::GraphNode,
                        (u16, u16),
                    )> = child_edges
                        .iter()
                        .filter_map(|(edge, _, to_idx)| {
                            graph
                                .nodes
                                .get(*to_idx)
                                .and_then(|n| n.position.map(|pos| (*edge, n, pos)))
                        })
                        .collect();

                    if child_nodes.len() > 1 {
                        // Check if children are side-by-side
                        let (parent_w, _parent_h) = calculate_node_size(parent_node, area.width);
                        let _parent_center_x = parent_x + parent_w / 2;

                        let child_centers: Vec<(u16, u16, u16)> = child_nodes
                            .iter()
                            .map(|(_, child_node, (cx, _cy))| {
                                let (child_w, _child_h) =
                                    calculate_node_size(child_node, area.width);
                                let child_center_x = *cx + child_w / 2;
                                (child_center_x, *cx, child_w)
                            })
                            .collect();

                        // Check if children are side-by-side
                        let min_x = child_centers
                            .iter()
                            .map(|(cx, _, _)| *cx)
                            .min()
                            .unwrap_or(0);
                        let max_x = child_centers
                            .iter()
                            .map(|(cx, _, _)| *cx)
                            .max()
                            .unwrap_or(0);
                        let avg_width: u16 = child_centers.iter().map(|(_, _, w)| *w).sum::<u16>()
                            / child_centers.len() as u16;
                        let is_side_by_side = max_x.saturating_sub(min_x) > avg_width;

                        if is_side_by_side {
                            // Render T-junction: vertical down, horizontal branch, vertical up to each child
                            render_t_junction(
                                f,
                                area,
                                parent_x,
                                parent_y,
                                parent_node,
                                &child_nodes,
                                scroll_offset_u16,
                                theme,
                            );
                        } else {
                            // Render individual edges (vertically aligned)
                            for (edge, child_node, (child_x, child_y)) in &child_nodes {
                                render_edge_improved(
                                    f,
                                    area,
                                    parent_x,
                                    parent_y,
                                    *child_x,
                                    *child_y,
                                    parent_node,
                                    child_node,
                                    edge.relationship,
                                    scroll_offset_u16,
                                    theme,
                                );
                            }
                        }
                    } else {
                        // Single child - render normally
                        for (edge, _, to_idx) in child_edges {
                            if let (Some(child_node), Some((child_x, child_y))) =
                                (graph.nodes.get(*to_idx), graph.nodes[*to_idx].position)
                            {
                                render_edge_improved(
                                    f,
                                    area,
                                    parent_x,
                                    parent_y,
                                    child_x,
                                    child_y,
                                    parent_node,
                                    child_node,
                                    edge.relationship,
                                    scroll_offset_u16,
                                    theme,
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    // Render nodes - only those within visible range (like YAML view)
    let visible_height = area.height as usize;
    let scroll_offset_u16 = scroll_offset as u16;

    for node in &graph.nodes {
        if let Some((x, y)) = node.position {
            // Only render if node's Y position is within visible range
            if y >= scroll_offset_u16 && y < scroll_offset_u16 + visible_height as u16 {
                render_node_text(f, area, x, y, node, scroll_offset_u16, theme);
            }
        }
    }
}

/// Calculate node dimensions based on content
fn calculate_node_size(node: &crate::trace::GraphNode, area_width: u16) -> (u16, u16) {
    let name_len = node.name.len() as u16;
    let kind_len = node.kind.len() as u16;
    let desc_len = node
        .description
        .as_ref()
        .map(|d| d.len() as u16)
        .unwrap_or(0);
    let max_content = name_len.max(kind_len).max(desc_len);
    let width = max_content.clamp(30, 60).min(area_width.saturating_sub(4));

    // Calculate height: title section + content + borders
    // For WorkloadGroup/ResourceGroup: name (1) + separator (1) + borders (2) = 4 base
    // For other nodes: kind label (1) + name (1) + separator (1) + borders (2) = 5 base
    let mut height = if matches!(
        node.node_type,
        NodeType::WorkloadGroup | NodeType::ResourceGroup
    ) {
        4u16 // name + separator + borders
    } else {
        5u16 // kind label + name + separator + borders
    };

    // Workload nodes have: kind label + name + separator + description (replica count) + namespace subtitle
    if matches!(node.node_type, NodeType::Workload) {
        if node.description.is_some() {
            height += 2; // description line + namespace subtitle
        }
    } else if matches!(node.node_type, NodeType::WorkloadGroup) {
        // Workload group nodes show each workload as 3-4 lines (kind, name, status, optional namespace) + blank lines between
        if let Some(ref desc) = node.description {
            let workload_lines: Vec<&str> = desc.lines().collect();
            let workload_count = workload_lines.len();

            if workload_count > 0 {
                // Check if namespaces differ
                let show_namespace = if workload_count > 1 {
                    let first_namespace = workload_lines[0].split('|').nth(2).unwrap_or("");
                    !workload_lines
                        .iter()
                        .all(|line| line.split('|').nth(2).unwrap_or("") == first_namespace)
                } else {
                    false
                };

                // Each workload: kind (1) + name (1) + status (1) + namespace (0-1) = 3-4 lines
                // Plus 1 blank line between workloads (workload_count - 1 blank lines)
                let lines_per_workload = if show_namespace { 4 } else { 3 };
                let blank_lines = workload_count.saturating_sub(1);
                height += (workload_count * lines_per_workload + blank_lines) as u16;
            }
        }
    } else if matches!(node.node_type, NodeType::ResourceGroup) {
        // Resource group nodes show each resource kind on a separate line
        if let Some(ref desc) = node.description {
            // Count the number of resource items (separated by ", ")
            let item_count = desc.matches(", ").count() + 1;
            height += item_count as u16;
        }
    } else {
        if node.description.is_some() {
            height += 1; // description line
        }
        if node.ready.is_some() {
            height += 1; // status line
        }
    }

    (width, height)
}

/// Render a T-junction for side-by-side child nodes
fn render_t_junction(
    f: &mut Frame,
    area: Rect,
    parent_x: u16,
    parent_y: u16,
    parent_node: &crate::trace::GraphNode,
    child_nodes: &[(
        &crate::trace::GraphEdge,
        &crate::trace::GraphNode,
        (u16, u16),
    )],
    scroll_offset: u16,
    theme: &Theme,
) {
    let (parent_w, parent_h) = calculate_node_size(parent_node, area.width);

    // Adjust parent position for scroll
    let fx = parent_x;
    let fy = parent_y.saturating_sub(scroll_offset);

    let parent_center_x = fx + parent_w / 2;
    let parent_bottom_y = fy + parent_h;

    // Get child positions and centers
    let mut child_info: Vec<(u16, u16, u16, u16, RelationshipType)> = Vec::new();
    for (edge, child_node, (child_x, child_y)) in child_nodes {
        let (child_w, child_h) = calculate_node_size(child_node, area.width);
        let tx = child_x;
        let ty = child_y.saturating_sub(scroll_offset);
        let child_center_x = tx + child_w / 2;
        let child_top_y = ty;
        child_info.push((
            child_center_x,
            child_top_y,
            child_w,
            child_h,
            edge.relationship,
        ));
    }

    // Determine edge color (use first edge's relationship)
    let edge_color = match child_info.first().map(|(_, _, _, _, r)| r) {
        Some(RelationshipType::SourcedFrom) => theme.status_ready,
        Some(RelationshipType::ManagedBy) => theme.text_primary,
        Some(RelationshipType::Owns) => theme.text_label,
        None => theme.text_label,
    };

    // Find leftmost and rightmost children
    let leftmost_x = child_info
        .iter()
        .map(|(cx, _, _, _, _)| *cx)
        .min()
        .unwrap_or(0);
    let rightmost_x = child_info
        .iter()
        .map(|(cx, _, _, _, _)| *cx)
        .max()
        .unwrap_or(0);

    // Branch Y is 1 line below parent
    let branch_y = parent_bottom_y + 1;

    // Draw vertical line from parent bottom to branch point
    if parent_center_x < area.width && branch_y > parent_bottom_y {
        let vertical_start_y = parent_bottom_y;
        let vertical_end_y = branch_y.min(area.height);
        if vertical_end_y > vertical_start_y {
            let vertical_height = vertical_end_y.saturating_sub(vertical_start_y);
            if vertical_height > 0 {
                let vertical_area = Rect {
                    x: area.x + parent_center_x,
                    y: area.y + vertical_start_y,
                    width: 1,
                    height: vertical_height,
                };
                let mut lines = Vec::new();
                for _ in 0..vertical_area.height {
                    lines.push(Line::from("│"));
                }
                let paragraph = Paragraph::new(lines).style(Style::default().fg(edge_color));
                f.render_widget(paragraph, vertical_area);
            }
        }
    }

    // Draw horizontal branch from leftmost to rightmost child
    if branch_y < area.height {
        let horizontal_start_x = leftmost_x.min(parent_center_x);
        let horizontal_end_x = rightmost_x.max(parent_center_x).min(area.width);
        let horizontal_width = horizontal_end_x.saturating_sub(horizontal_start_x);

        if horizontal_width > 0 && horizontal_start_x < area.width {
            let horizontal_area = Rect {
                x: area.x + horizontal_start_x,
                y: area.y + branch_y,
                width: horizontal_width,
                height: 1,
            };

            let horizontal_line = "─".repeat(horizontal_area.width as usize);
            let paragraph = Paragraph::new(horizontal_line).style(Style::default().fg(edge_color));
            f.render_widget(paragraph, horizontal_area);
        }
    }

    // Draw vertical lines from branch to each child
    for (child_center_x, child_top_y, _child_w, _child_h, _relationship) in &child_info {
        let vertical_start_y = branch_y + 1;
        let vertical_end_y = (*child_top_y).min(area.height);

        if vertical_start_y < vertical_end_y && *child_center_x < area.width {
            let vertical_height = vertical_end_y.saturating_sub(vertical_start_y);
            if vertical_height > 0 {
                let vertical_area = Rect {
                    x: area.x + *child_center_x,
                    y: area.y + vertical_start_y,
                    width: 1,
                    height: vertical_height,
                };
                let mut lines = Vec::new();
                for _ in 0..vertical_area.height {
                    lines.push(Line::from("│"));
                }
                let paragraph = Paragraph::new(lines).style(Style::default().fg(edge_color));
                f.render_widget(paragraph, vertical_area);
            }
        }
    }
}

/// Render an edge between two nodes with improved Unicode box drawing
fn render_edge_improved(
    f: &mut Frame,
    area: Rect,
    from_x: u16,
    from_y: u16,
    to_x: u16,
    to_y: u16,
    from_node: &crate::trace::GraphNode,
    to_node: &crate::trace::GraphNode,
    relationship: RelationshipType,
    scroll_offset: u16,
    theme: &Theme,
) {
    let (from_w, from_h) = calculate_node_size(from_node, area.width);
    let (to_w, _to_h) = calculate_node_size(to_node, area.width);

    // Adjust positions for scroll offset
    let fx = from_x;
    let fy = from_y.saturating_sub(scroll_offset);
    let tx = to_x;
    let ty = to_y.saturating_sub(scroll_offset);

    // Calculate connection points (center bottom of from, center top of to)
    let from_center_x = fx + from_w / 2;
    let from_bottom_y = fy + from_h;
    let to_center_x = tx + to_w / 2;
    let to_top_y = ty;

    // Only skip if completely off-screen (allow partial visibility for edges)
    // Edges can extend beyond visible area - we'll clip them during rendering
    if from_center_x >= area.width && to_center_x >= area.width {
        return; // Both nodes completely off-screen horizontally
    }
    if from_bottom_y >= area.height && to_top_y >= area.height {
        return; // Both nodes completely off-screen vertically
    }

    let edge_color = match relationship {
        RelationshipType::SourcedFrom => theme.status_ready,
        RelationshipType::ManagedBy => theme.text_primary,
        RelationshipType::Owns => theme.text_label,
    };

    // Check if nodes are side-by-side (horizontal connection needed)
    let horizontal_distance = from_center_x.abs_diff(to_center_x);

    let is_side_by_side = horizontal_distance > from_w / 2 + to_w / 2;

    if is_side_by_side {
        // Nodes are side-by-side: draw vertical line down, then horizontal branch, then vertical to target
        let branch_y = from_bottom_y + 1; // Start branch 1 line below from node

        // Draw vertical line from bottom of from_node to branch point
        // Clip to visible area
        let vertical_start_y = from_bottom_y;
        let vertical_end_y = branch_y.min(area.height);
        if vertical_end_y > vertical_start_y && from_center_x < area.width {
            let vertical_height = vertical_end_y.saturating_sub(vertical_start_y);
            if vertical_height > 0 {
                let vertical_area = Rect {
                    x: area.x + from_center_x,
                    y: area.y + vertical_start_y,
                    width: 1,
                    height: vertical_height,
                };
                let mut lines = Vec::new();
                for _ in 0..vertical_area.height {
                    lines.push(Line::from("│"));
                }
                let paragraph = Paragraph::new(lines).style(Style::default().fg(edge_color));
                f.render_widget(paragraph, vertical_area);
            }
        }

        // Draw horizontal line from from_center_x to to_center_x at branch_y
        // This forms the horizontal branch of the T-junction
        // Clip to visible area but ensure we draw if any part is visible
        if branch_y < area.height {
            let horizontal_start_x = from_center_x.min(to_center_x);
            let horizontal_end_x = from_center_x.max(to_center_x).min(area.width);
            let horizontal_width = horizontal_end_x.saturating_sub(horizontal_start_x);

            if horizontal_width > 0 && horizontal_start_x < area.width {
                let horizontal_area = Rect {
                    x: area.x + horizontal_start_x,
                    y: area.y + branch_y,
                    width: horizontal_width,
                    height: 1,
                };

                // Create horizontal line with ─ characters
                let horizontal_line = "─".repeat(horizontal_area.width as usize);
                let paragraph =
                    Paragraph::new(horizontal_line).style(Style::default().fg(edge_color));
                f.render_widget(paragraph, horizontal_area);
            }
        }

        // Draw vertical line from branch_y to top of to_node for each target
        // For T-junction, we need to draw vertical lines to both left and right nodes
        let vertical_start_y = branch_y + 1;
        let vertical_end_y = to_top_y.min(area.height);

        if vertical_start_y < vertical_end_y && to_center_x < area.width {
            let vertical_height = vertical_end_y.saturating_sub(vertical_start_y);
            if vertical_height > 0 {
                let vertical_area = Rect {
                    x: area.x + to_center_x,
                    y: area.y + vertical_start_y,
                    width: 1,
                    height: vertical_height,
                };
                let mut lines = Vec::new();
                for _ in 0..vertical_area.height {
                    lines.push(Line::from("│"));
                }
                let paragraph = Paragraph::new(lines).style(Style::default().fg(edge_color));
                f.render_widget(paragraph, vertical_area);
            }
        }
    } else {
        // Nodes are vertically aligned: draw straight vertical line
        let start_y = from_bottom_y;
        let end_y = to_top_y;

        // Only draw if there's space between nodes
        if start_y < end_y && start_y < area.height && end_y > 0 {
            let line_x = from_center_x;

            if line_x < area.width {
                let line_height = end_y.saturating_sub(start_y);

                // Skip if no space (nodes are directly adjacent)
                if line_height > 0 {
                    let edge_area = Rect {
                        x: area.x + line_x,
                        y: area.y + start_y,
                        width: 1,
                        height: line_height.min(area.height.saturating_sub(start_y)),
                    };

                    // Create vertical line with repeated │ characters
                    let mut lines = Vec::new();
                    for _ in 0..edge_area.height {
                        lines.push(Line::from("│"));
                    }

                    let paragraph = Paragraph::new(lines).style(Style::default().fg(edge_color));
                    f.render_widget(paragraph, edge_area);
                }
            }
        }
    }
}

/// Render node text content
fn render_node_text(
    f: &mut Frame,
    area: Rect,
    x: u16,
    y: u16,
    node: &crate::trace::GraphNode,
    scroll_offset: u16, // Line-based scroll offset
    theme: &Theme,
) {
    // Adjust Y position for scroll offset (like YAML view)
    // scroll_offset is already u16 (converted at call site)
    let adjusted_y = y.saturating_sub(scroll_offset);
    let adjusted_x = x; // No horizontal scrolling

    // Calculate node area with dynamic sizing
    let (node_width, node_height) =
        calculate_node_size(node, area.width.saturating_sub(adjusted_x));

    // Simplified scrolling: skip nodes that would be above or below visible area
    // When scrolling up, nodes above are completely removed (not clipped)
    if adjusted_x >= area.width {
        // Completely off-screen horizontally
        return;
    }
    if adjusted_y >= area.height {
        // Completely below visible area
        return;
    }
    // Note: adjusted_y is u16 (from y.saturating_sub(scroll_offset)), so it can't be negative
    // Nodes that would be above the visible area are already handled by adjusted_y being 0

    // Calculate node area - render fully if it starts within visible area
    let node_x = area.x + adjusted_x.min(area.width.saturating_sub(node_width));
    let node_y = area.y + adjusted_y;

    // Clip height only if node extends below visible area
    let node_area = Rect {
        x: node_x,
        y: node_y,
        width: node_width.min(area.width.saturating_sub(adjusted_x)),
        height: node_height.min(area.height.saturating_sub(adjusted_y)),
    };

    // Basic validation
    if node_area.width == 0 || node_area.height == 0 {
        return;
    }

    // Determine node style based on type and status
    let (border_style, title_style) = match node.node_type {
        NodeType::Object => (
            Style::default()
                .fg(theme.text_label)
                .add_modifier(Modifier::BOLD),
            Style::default()
                .fg(theme.text_label)
                .add_modifier(Modifier::BOLD),
        ),
        NodeType::Chain => (
            Style::default().fg(theme.text_primary),
            Style::default().fg(theme.text_primary),
        ),
        NodeType::Source => (
            Style::default().fg(theme.status_ready),
            Style::default().fg(theme.status_ready),
        ),
        NodeType::Upstream => (
            Style::default().fg(theme.status_ready),
            Style::default().fg(theme.status_ready),
        ),
        NodeType::FluxResource => (
            Style::default().fg(theme.text_primary),
            Style::default().fg(theme.text_primary),
        ),
        NodeType::Workload => (
            Style::default().fg(theme.status_ready),
            Style::default().fg(theme.status_ready),
        ),
        NodeType::WorkloadGroup => (
            Style::default().fg(theme.text_secondary),
            Style::default().fg(theme.text_secondary),
        ),
        NodeType::ResourceGroup => (
            Style::default().fg(theme.text_secondary),
            Style::default().fg(theme.text_secondary),
        ),
    };

    // Status indicator
    let status_indicator = match node.ready {
        Some(true) => "✓",
        Some(false) => "✗",
        None => "?",
    };

    let status_color = match node.ready {
        Some(true) => theme.status_ready,
        Some(false) => theme.status_error,
        None => theme.status_unknown,
    };

    // Build node content with type label (like Web UI)
    let mut content = vec![];

    // For WorkloadGroup and ResourceGroup, we don't add type label/name upfront
    // since their content is already formatted
    if !matches!(
        node.node_type,
        NodeType::WorkloadGroup | NodeType::ResourceGroup
    ) {
        // Add type label based on node type (similar to Web UI)
        // Web UI shows kind in uppercase
        let type_label = match node.node_type {
            NodeType::Upstream => "UPSTREAM".to_string(),
            NodeType::Source => node.kind.to_uppercase(),
            NodeType::Chain => node.kind.to_uppercase(),
            NodeType::Object => node.kind.to_uppercase(),
            NodeType::FluxResource => format!("{}  →", node.kind.to_uppercase()),
            NodeType::Workload => node.kind.to_uppercase(),
            NodeType::WorkloadGroup => format!("{}  →", node.kind.to_uppercase()),
            NodeType::ResourceGroup => format!("{}  →", node.kind.to_uppercase()),
        };

        // Type label uses text_secondary without DIM modifier (matching Web UI muted text)
        content.push(Line::from(vec![Span::styled(
            type_label, // Moved ownership instead of borrowing
            Style::default().fg(theme.text_secondary),
        )]));

        // Add resource name - Web UI shows namespace/name format when namespace is present
        let display_name = if !node.namespace.is_empty() {
            format!("{}/{}", node.namespace, node.name)
        } else {
            node.name.clone()
        };
        content.push(Line::from(vec![Span::styled(
            display_name, // Clone to avoid borrow issues
            title_style,
        )]));
    }

    // For workload nodes, show replica status prominently with status dot
    if matches!(node.node_type, NodeType::Workload) {
        // Add replica/status description with colored dot (like Web UI)
        if let Some(ref desc) = node.description {
            let max_desc_len = (node_area.width.saturating_sub(4)) as usize; // Reserve space for dot
            let desc_display: String = if desc.len() > max_desc_len {
                format!("{}...", &desc[..max_desc_len.saturating_sub(3)])
            } else {
                desc.clone()
            };

            // Add status indicator dot (● or ○) based on ready status
            let dot = if node.ready == Some(true) {
                "●"
            } else {
                "○"
            };

            let status_line = format!("{} {}", dot, desc_display);

            content.push(Line::from(vec![Span::styled(
                status_line,
                Style::default().fg(if node.ready == Some(true) {
                    theme.status_ready
                } else {
                    theme.text_secondary
                }),
            )]));
        }

        // Add the namespace as a subtitle (matching Web UI pattern)
        content.push(Line::from(vec![Span::styled(
            &node.namespace,
            Style::default().fg(theme.text_secondary),
        )]));
    } else if matches!(node.node_type, NodeType::WorkloadGroup) {
        // For workload group nodes, show each workload on multiple lines
        // Format: "Kind|name|namespace|status_indicator|status_text"
        // Web UI shows: kind (muted), name (bold), status with dot, namespace only if namespaces differ
        if let Some(ref desc) = node.description {
            // First pass: collect all workloads to check if namespaces differ
            let mut workloads: Vec<(String, String, String, String, String)> = Vec::new();
            for workload_line in desc.lines() {
                let parts: Vec<&str> = workload_line.split('|').collect();
                if parts.len() == 5 {
                    workloads.push((
                        parts[0].to_string(), // kind
                        parts[1].to_string(), // name
                        parts[2].to_string(), // namespace
                        parts[3].to_string(), // status_indicator
                        parts[4].to_string(), // status_text
                    ));
                }
            }

            // Check if all workloads share the same namespace
            let show_namespace = if workloads.len() > 1 {
                let first_namespace = &workloads[0].2;
                !workloads.iter().all(|w| &w.2 == first_namespace)
            } else {
                false // Single workload, don't show namespace
            };

            // Render each workload
            for (kind, name, namespace, status_indicator, status_text) in &workloads {
                // Line 1: Kind (muted, uppercase like Web UI)
                content.push(Line::from(vec![Span::styled(
                    kind.to_uppercase(),
                    Style::default().fg(theme.text_secondary),
                )]));

                // Line 2: Name (bold) - Web UI shows just name, not namespace/name
                content.push(Line::from(vec![Span::styled(
                    name.clone(),
                    Style::default()
                        .fg(theme.text_primary)
                        .add_modifier(Modifier::BOLD),
                )]));

                // Line 3: Status with colored dot
                let status_color = if status_indicator == "●" {
                    theme.status_ready
                } else {
                    theme.text_secondary
                };
                let status_line = format!("{} {}", status_indicator, status_text);
                content.push(Line::from(vec![Span::styled(
                    status_line,
                    Style::default().fg(status_color),
                )]));

                // Line 4: Namespace (muted) - only if namespaces differ
                if show_namespace {
                    content.push(Line::from(vec![Span::styled(
                        namespace.clone(),
                        Style::default().fg(theme.text_secondary),
                    )]));
                }

                // Add spacing between workloads
                content.push(Line::from(vec![Span::raw("")]));
            }
        }
    } else if matches!(node.node_type, NodeType::ResourceGroup) {
        // For resource group nodes, show each resource kind on its own line
        if let Some(ref desc) = node.description {
            // Description format: "Kind1: count1, Kind2: count2, ..."
            let max_width = (node_area.width.saturating_sub(2)) as usize;
            for resource_item in desc.split(", ") {
                let item_display: String = if resource_item.len() > max_width {
                    format!("{}...", &resource_item[..max_width.saturating_sub(3)])
                } else {
                    resource_item.to_string()
                };
                content.push(Line::from(vec![Span::styled(
                    item_display,
                    Style::default().fg(theme.text_secondary),
                )]));
            }
        }
    } else {
        // For other non-workload nodes, show description if available
        if let Some(ref desc) = node.description {
            let max_desc_len = (node_area.width.saturating_sub(2)) as usize;
            let desc_display: String = if desc.len() > max_desc_len {
                format!("{}...", &desc[..max_desc_len.saturating_sub(3)])
            } else {
                desc.clone()
            };
            content.push(Line::from(vec![Span::styled(
                desc_display,
                Style::default()
                    .fg(theme.text_secondary)
                    .add_modifier(Modifier::ITALIC),
            )]));
        }

        // Add status line for nodes with status
        let status_text = match node.ready {
            Some(true) => format!("{} Ready", status_indicator),
            Some(false) => format!("{} Not Ready", status_indicator),
            None => "".to_string(),
        };

        if !status_text.is_empty() {
            content.push(Line::from(vec![Span::styled(
                status_text,
                Style::default().fg(status_color),
            )]));
        }
    }

    // Create the outer block with borders
    // Use Reset background to ensure transparent background (matches terminal default)
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .style(Style::default().bg(ratatui::style::Color::Reset));

    // Render the outer block
    f.render_widget(block, node_area);

    // Get inner area (inside the border)
    let inner = node_area.inner(&Margin {
        horizontal: 1,
        vertical: 1,
    });

    // Simplified: ensure inner area is within bounds
    // Since we skip nodes that start above visible area, inner.y should already be >= area.y
    if inner.height < 2 || inner.y + inner.height > area.y + area.height {
        return; // Not enough space or extends beyond visible area
    }

    // Clip inner height only if it extends below visible area
    let inner = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: inner
            .height
            .min((area.y + area.height).saturating_sub(inner.y)),
    };

    // Split inner area: title section + content section
    // For WorkloadGroup/ResourceGroup: name (1) + separator (1) = 2
    // For other nodes: kind label (1) + name (1) + separator (1) = 3
    let is_group_node = matches!(
        node.node_type,
        NodeType::WorkloadGroup | NodeType::ResourceGroup
    );
    let kind_label_height = if is_group_node { 0 } else { 1 };
    let name_height = 1;
    let separator_height = 1;
    let title_section_height = kind_label_height + name_height + separator_height;

    let kind_label_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: kind_label_height,
    };
    let name_area = Rect {
        x: inner.x,
        y: inner.y + kind_label_height,
        width: inner.width,
        height: name_height,
    };
    let separator_area = Rect {
        x: inner.x,
        y: inner.y + kind_label_height + name_height,
        width: inner.width,
        height: separator_height,
    };
    let content_area = Rect {
        x: inner.x,
        y: inner.y + title_section_height,
        width: inner.width,
        height: inner.height.saturating_sub(title_section_height),
    };

    // Extract kind label and name from content for title section
    let (kind_label_line, name_line) = if is_group_node {
        // For group nodes, show name as title, content is already formatted
        (
            None,
            Line::from(Span::styled(
                &node.name,
                title_style.add_modifier(Modifier::BOLD),
            )),
        )
    } else if content.len() >= 2 {
        // Extract kind label and name from content
        let kind_label = content.remove(0); // Remove and get type label
        let name = content.remove(0); // Remove and get name
        (Some(kind_label), name)
    } else if !content.is_empty() {
        // Only one line, use it as name
        (None, content.remove(0))
    } else {
        // No content, create name from node name
        (
            None,
            Line::from(Span::styled(
                &node.name,
                title_style.add_modifier(Modifier::BOLD),
            )),
        )
    };

    // Render kind label (uppercase, muted) - matching Web UI
    if let Some(ref kind_line) = kind_label_line {
        f.render_widget(
            Paragraph::new(vec![kind_line.clone()]).style(
                Style::default()
                    .fg(theme.text_secondary)
                    .bg(ratatui::style::Color::Reset),
            ),
            kind_label_area,
        );
    }

    // Render name (bold)
    f.render_widget(
        Paragraph::new(vec![name_line]).style(
            title_style
                .add_modifier(Modifier::BOLD)
                .bg(ratatui::style::Color::Reset),
        ),
        name_area,
    );

    // Render horizontal separator line below name
    let separator = "─".repeat(inner.width as usize);
    f.render_widget(
        Paragraph::new(separator).style(
            Style::default()
                .fg(theme.text_secondary)
                .bg(ratatui::style::Color::Reset),
        ),
        separator_area,
    );

    // Render remaining content below separator
    // Limit content to what fits in the clipped content_area to prevent buffer overflow
    if !content.is_empty() && content_area.height > 0 {
        // Ensure content_area is within bounds
        let clipped_content_area = Rect {
            x: content_area.x,
            y: content_area.y,
            width: content_area.width,
            height: content_area
                .height
                .min((area.y + area.height).saturating_sub(content_area.y)),
        };

        if clipped_content_area.height > 0 {
            // Limit content lines to what fits
            let max_lines = clipped_content_area.height as usize;
            let visible_content: Vec<_> = content.iter().take(max_lines).cloned().collect();

            let paragraph = Paragraph::new(visible_content)
                .style(
                    Style::default()
                        .fg(theme.text_primary)
                        .bg(ratatui::style::Color::Reset),
                )
                .wrap(ratatui::widgets::Wrap { trim: true });
            f.render_widget(paragraph, clipped_content_area);
        }
    }
}
