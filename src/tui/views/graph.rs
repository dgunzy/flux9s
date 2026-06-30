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
    style::{Color, Modifier, Style},
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
    scroll_offset: &mut usize,  // Line-based scroll offset (like YAML view)
    focus_index: Option<usize>, // Index of the keyboard-focused node, if any
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

    // Auto-scroll so the focused node stays fully on screen. Positions are only
    // known after layout, so this runs here rather than in the event handler.
    if let Some((node_y, node_height)) = focus_index.and_then(|idx| {
        graph.nodes.get(idx).and_then(|node| {
            node.position.map(|(_, y)| {
                (
                    y as usize,
                    calculate_node_size(node, inner_area.width).1 as usize,
                )
            })
        })
    }) {
        if node_y < *scroll_offset {
            *scroll_offset = node_y;
        } else if node_y + node_height > *scroll_offset + visible_height {
            *scroll_offset = (node_y + node_height).saturating_sub(visible_height);
        }
    }

    *scroll_offset = (*scroll_offset).min(max_scroll);

    // Render graph using improved layout with line-based scrolling
    render_graph_nodes_and_edges(f, inner_area, &graph, *scroll_offset, focus_index, theme);
}

/// Pure geometry for one parent→children connector, in scroll-adjusted,
/// area-relative cell coordinates. Separated from drawing so it can be unit
/// tested without a `Frame`.
struct FanoutRoute {
    parent_center_x: u16,
    parent_bottom_y: u16,
    /// Each child's connection point: `(center_x, top_y)`.
    children: Vec<(u16, u16)>,
    relationship: RelationshipType,
}

/// Compute the connector route for every parent that has placed children.
///
/// Pure function of the (already laid-out) graph, the available width and the
/// scroll offset — no theme or `Frame` involved — so the geometry is testable in
/// isolation. Parents and children are visited in node/edge insertion order, so
/// the result is deterministic.
fn fanout_routes(graph: &ResourceGraph, area_width: u16, scroll_offset: u16) -> Vec<FanoutRoute> {
    let mut routes = Vec::new();

    for parent in &graph.nodes {
        let Some((px, py)) = parent.position else {
            continue;
        };

        let mut children = Vec::new();
        let mut relationship = None;
        for edge in &graph.edges {
            if edge.from != parent.id {
                continue;
            }
            if let Some(child) = graph
                .node_index
                .get(&edge.to)
                .and_then(|&i| graph.nodes.get(i))
            {
                if let Some((cx, cy)) = child.position {
                    let center = cx + child.render_width(area_width) / 2;
                    children.push((center, cy.saturating_sub(scroll_offset)));
                    relationship.get_or_insert(edge.relationship);
                }
            }
        }

        let Some(relationship) = relationship else {
            continue; // No placed children for this parent.
        };

        routes.push(FanoutRoute {
            parent_center_x: px + parent.render_width(area_width) / 2,
            parent_bottom_y: (py + parent.render_height()).saturating_sub(scroll_offset),
            children,
            relationship,
        });
    }

    routes
}

/// Map an edge relationship to its connector color.
fn edge_color(relationship: RelationshipType, theme: &Theme) -> Color {
    match relationship {
        RelationshipType::SourcedFrom => theme.status_ready,
        RelationshipType::ManagedBy => theme.text_primary,
        RelationshipType::Owns => theme.text_label,
    }
}

/// Render graph nodes and edges with improved layout
fn render_graph_nodes_and_edges(
    f: &mut Frame,
    area: Rect,
    graph: &ResourceGraph,
    scroll_offset: usize,       // Line-based scroll offset
    focus_index: Option<usize>, // Index of the keyboard-focused node, if any
    theme: &Theme,
) {
    let scroll_offset_u16 = scroll_offset as u16;

    // Render edges first (behind the nodes) as fan-outs: a trunk down from the
    // parent, a horizontal branch just above its children, then a short drop into
    // each child. Routing every parent (one child or many) through the same
    // renderer keeps the connector lines consistent and unambiguous.
    for route in fanout_routes(graph, area.width, scroll_offset_u16) {
        render_fanout(
            f,
            area,
            route.parent_center_x,
            route.parent_bottom_y,
            &route.children,
            edge_color(route.relationship, theme),
        );
    }

    // Render nodes - only those within visible range (like YAML view)
    let visible_height = area.height as usize;

    for (idx, node) in graph.nodes.iter().enumerate() {
        if let Some((x, y)) = node.position {
            // Only render if node's Y position is within visible range
            if y >= scroll_offset_u16 && y < scroll_offset_u16 + visible_height as u16 {
                let is_focused = focus_index == Some(idx);
                render_node_text(f, area, x, y, node, scroll_offset_u16, is_focused, theme);
            }
        }
    }
}

/// Calculate node dimensions based on content
/// Rendered (width, height) of a node. Thin adapter over the single source of
/// truth on [`crate::trace::GraphNode`] so layout and drawing always agree.
fn calculate_node_size(node: &crate::trace::GraphNode, area_width: u16) -> (u16, u16) {
    (node.render_width(area_width), node.render_height())
}

/// Draw a vertical run of `height` cells at column `x`, clipped to `area`.
fn draw_vline(f: &mut Frame, area: Rect, x: u16, y: u16, height: u16, color: Color) {
    if height == 0 || x >= area.width || y >= area.height {
        return;
    }
    let h = height.min(area.height - y);
    let rect = Rect {
        x: area.x + x,
        y: area.y + y,
        width: 1,
        height: h,
    };
    let lines: Vec<Line> = (0..h).map(|_| Line::from("│")).collect();
    f.render_widget(
        Paragraph::new(lines).style(Style::default().fg(color)),
        rect,
    );
}

/// Draw a pre-built run of box-drawing glyphs starting at column `x`, clipped to
/// `area`. The caller composes the glyphs (with junctions) so overlapping cells
/// don't fight each other.
fn draw_hline(f: &mut Frame, area: Rect, x: u16, y: u16, glyphs: &str, color: Color) {
    let width = glyphs.chars().count() as u16;
    if width == 0 || x >= area.width || y >= area.height {
        return;
    }
    let w = width.min(area.width - x);
    let text: String = glyphs.chars().take(w as usize).collect();
    let rect = Rect {
        x: area.x + x,
        y: area.y + y,
        width: w,
        height: 1,
    };
    f.render_widget(Paragraph::new(text).style(Style::default().fg(color)), rect);
}

/// Pick the box-drawing glyph for a cell from which sides connect.
fn box_glyph(up: bool, down: bool, left: bool, right: bool) -> char {
    match (up, down, left, right) {
        (true, true, true, true) => '┼',
        (false, true, true, true) => '┬',
        (true, false, true, true) => '┴',
        (true, true, true, false) => '┤',
        (true, true, false, true) => '├',
        (false, true, false, true) => '┌',
        (false, true, true, false) => '┐',
        (true, false, false, true) => '└',
        (true, false, true, false) => '┘',
        (true, true, false, false) | (true, false, false, false) | (false, true, false, false) => {
            '│'
        }
        _ => '─',
    }
}

/// Render a parent→children fan-out: a trunk down from the parent, a horizontal
/// branch sitting just above the topmost child (with proper junction glyphs),
/// then a short drop into each child. The single-child case collapses to a plain
/// vertical line, so every connector in the graph is drawn the same way.
fn render_fanout(
    f: &mut Frame,
    area: Rect,
    parent_center_x: u16,
    parent_bottom_y: u16,
    children: &[(u16, u16)], // (center_x, top_y), already scroll-adjusted
    color: Color,
) {
    if children.is_empty() {
        return;
    }

    let min_top = children.iter().map(|(_, t)| *t).min().unwrap_or(0);

    // No room for a branch row between parent and children: connect each child
    // straight up to the parent's bottom edge instead.
    if min_top <= parent_bottom_y {
        for &(cx, top) in children {
            if top > parent_bottom_y {
                draw_vline(f, area, cx, parent_bottom_y, top - parent_bottom_y, color);
            }
        }
        return;
    }

    let branch_y = min_top - 1;

    // Horizontal extent spans the trunk and every child center.
    let mut left = parent_center_x;
    let mut right = parent_center_x;
    for &(cx, _) in children {
        left = left.min(cx);
        right = right.max(cx);
    }
    if right >= area.width {
        right = area.width.saturating_sub(1);
    }
    if left > right {
        return;
    }

    // 1. Trunk from the parent's bottom down to the branch row.
    if branch_y > parent_bottom_y {
        draw_vline(
            f,
            area,
            parent_center_x,
            parent_bottom_y,
            branch_y - parent_bottom_y,
            color,
        );
    }

    // 2. Branch row, composed cell-by-cell so junctions render correctly.
    let mut glyphs = String::new();
    for c in left..=right {
        let up = c == parent_center_x;
        let down = children.iter().any(|&(cx, _)| cx == c);
        let lft = c > left;
        let rgt = c < right;
        glyphs.push(box_glyph(up, down, lft, rgt));
    }
    draw_hline(f, area, left, branch_y, &glyphs, color);

    // 3. Short drop from the branch into each child top (zero-length when the
    //    child sits directly below the branch row).
    let drop_start = branch_y + 1;
    for &(cx, top) in children {
        if top > drop_start {
            draw_vline(f, area, cx, drop_start, top - drop_start, color);
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
    is_focused: bool,   // Whether this node has keyboard focus
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

    // Create the outer block with borders.
    // Use Reset background to ensure transparent background (matches terminal default).
    // The focused node gets a bright, bold double border in the accent color
    // (distinct from the status/text colors every other border uses) plus a "▸"
    // title marker, so the selection is unmistakable at a glance.
    let focus_style = Style::default()
        .fg(theme.footer_key)
        .add_modifier(Modifier::BOLD);

    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_style(if is_focused {
            focus_style
        } else {
            border_style
        })
        .style(Style::default().bg(ratatui::style::Color::Reset));

    if is_focused {
        block = block
            .border_type(ratatui::widgets::BorderType::Double)
            .title(Span::styled(" ▸ ", focus_style));
    }

    // Render the outer block
    f.render_widget(block, node_area);

    // Get inner area (inside the border)
    let inner = node_area.inner(Margin {
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

    // Render name (bold). The focused node's name takes the accent color too,
    // reinforcing the highlighted border.
    let name_style = if is_focused {
        focus_style.bg(ratatui::style::Color::Reset)
    } else {
        title_style
            .add_modifier(Modifier::BOLD)
            .bg(ratatui::style::Color::Reset)
    };
    f.render_widget(Paragraph::new(vec![name_line]).style(name_style), name_area);

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

#[cfg(test)]
mod tests {
    use super::{box_glyph, fanout_routes};
    use crate::trace::{GraphEdge, GraphNode, NodeType, RelationshipType, ResourceGraph};

    #[test]
    fn box_glyph_renders_expected_junctions() {
        // up, down, left, right
        assert_eq!(box_glyph(true, true, true, true), '┼'); // cross
        assert_eq!(box_glyph(false, true, true, true), '┬'); // child drop on the branch
        assert_eq!(box_glyph(true, false, true, true), '┴'); // trunk meets the branch
        assert_eq!(box_glyph(false, true, false, true), '┌'); // left end with a drop
        assert_eq!(box_glyph(false, true, true, false), '┐'); // right end with a drop
        assert_eq!(box_glyph(true, false, false, true), '└'); // left end, trunk above
        assert_eq!(box_glyph(true, false, true, false), '┘'); // right end, trunk above
        assert_eq!(box_glyph(false, false, true, true), '─'); // plain horizontal
        assert_eq!(box_glyph(true, true, false, false), '│'); // plain vertical (aligned child)
    }

    fn node(id: &str, node_type: NodeType, desc: Option<&str>) -> GraphNode {
        GraphNode {
            id: id.to_string(),
            kind: "Kind".to_string(),
            name: id.to_string(),
            namespace: "ns".to_string(),
            node_type,
            ready: None,
            position: None,
            description: desc.map(|d| d.to_string()),
        }
    }

    #[test]
    fn fanout_routes_connects_object_to_its_inventory_groups() {
        let mut graph = ResourceGraph::new();
        graph.add_node(node("obj", NodeType::Object, None));
        graph.add_node(node(
            "wg",
            NodeType::WorkloadGroup,
            Some("Deployment|a|ns|●|1/1"),
        ));
        graph.add_node(node("rg", NodeType::ResourceGroup, Some("ConfigMap: 1")));
        for child in ["wg", "rg"] {
            graph.add_edge(GraphEdge {
                from: "obj".to_string(),
                to: child.to_string(),
                relationship: RelationshipType::Owns,
            });
        }

        graph.calculate_layout(100, 50);
        let routes = fanout_routes(&graph, 100, 0);

        // One parent (the object) with both inventory groups as children.
        assert_eq!(routes.len(), 1);
        let route = &routes[0];
        assert_eq!(route.relationship, RelationshipType::Owns);
        assert_eq!(route.children.len(), 2);

        // Children sit below the parent's bottom edge.
        assert!(
            route
                .children
                .iter()
                .all(|&(_, top)| top > route.parent_bottom_y)
        );

        // The trunk falls between the child centers (a centered fan-out).
        let min_x = route.children.iter().map(|&(x, _)| x).min().unwrap();
        let max_x = route.children.iter().map(|&(x, _)| x).max().unwrap();
        assert!(min_x <= route.parent_center_x && route.parent_center_x <= max_x);
    }

    #[test]
    fn fanout_routes_skips_parents_without_placed_children() {
        // A lone node with no edges produces no routes.
        let mut graph = ResourceGraph::new();
        graph.add_node(node("solo", NodeType::Object, None));
        graph.calculate_layout(100, 50);
        assert!(fanout_routes(&graph, 100, 0).is_empty());
    }
}
