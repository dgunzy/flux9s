//! Graph data structures for visualizing resource relationships
//!
//! This module provides structures to represent resources and their relationships
//! as a graph, suitable for visualization in the TUI.

use std::collections::HashMap;

/// Minimum rendered width of a graph node, in terminal columns.
pub const MIN_NODE_WIDTH: u16 = 30;
/// Maximum rendered width of a graph node, in terminal columns.
pub const MAX_NODE_WIDTH: u16 = 60;
/// Vertical gap (rows) between stacked nodes. Two is the minimum the fan-out edge
/// routing needs: one row for the drop line and one for the horizontal branch.
pub const NODE_VERTICAL_SPACING: u16 = 2;
/// Horizontal gap (columns) between side-by-side inventory groups.
pub const INVENTORY_GROUP_GAP: u16 = 4;
/// Border + padding overhead reserved around a node's content, in columns.
const NODE_HORIZONTAL_CHROME: u16 = 4;

/// A node in the resource graph
#[derive(Debug, Clone)]
pub struct GraphNode {
    /// Unique identifier for the node
    pub id: String,
    /// Resource kind
    pub kind: String,
    /// Resource name
    pub name: String,
    /// Resource namespace
    pub namespace: String,
    /// Node type (Object, Chain, Source)
    pub node_type: NodeType,
    /// Status information
    pub ready: Option<bool>,
    /// Position for rendering (calculated during layout)
    pub position: Option<(u16, u16)>,
    /// Optional description/snippet about the resource (e.g., URL, path, etc.)
    pub description: Option<String>,
}

impl GraphNode {
    /// Width of the widest line of content this node will render, in columns.
    fn content_width(&self) -> u16 {
        let desc = self
            .description
            .as_ref()
            .map(|d| d.len() as u16)
            .unwrap_or(0);
        (self.name.len() as u16)
            .max(self.kind.len() as u16)
            .max(desc)
    }

    /// Rendered width in columns, clamped to sane bounds and the available width.
    /// Single source of truth for node width across layout and drawing.
    pub fn render_width(&self, available_width: u16) -> u16 {
        self.content_width()
            .clamp(MIN_NODE_WIDTH, MAX_NODE_WIDTH)
            .min(available_width.saturating_sub(NODE_HORIZONTAL_CHROME))
    }

    /// Rendered height in rows: title section + content + borders. Single source
    /// of truth for node height across layout and drawing.
    pub fn render_height(&self) -> u16 {
        // Group nodes show only a name + separator inside the borders (4 rows of
        // chrome); other nodes also carry a kind label (5 rows).
        let mut height = if matches!(
            self.node_type,
            NodeType::WorkloadGroup | NodeType::ResourceGroup
        ) {
            4
        } else {
            5
        };

        match self.node_type {
            NodeType::Workload => {
                if self.description.is_some() {
                    height += 2; // description line + namespace subtitle
                }
            }
            NodeType::WorkloadGroup => {
                if let Some(desc) = &self.description {
                    let lines: Vec<&str> = desc.lines().collect();
                    let count = lines.len();
                    if count > 0 {
                        // A namespace line is shown per workload only when they differ.
                        let show_namespace = count > 1 && {
                            let first = lines[0].split('|').nth(2).unwrap_or("");
                            !lines
                                .iter()
                                .all(|l| l.split('|').nth(2).unwrap_or("") == first)
                        };
                        let per_workload = if show_namespace { 4 } else { 3 };
                        let blanks = count.saturating_sub(1);
                        height += (count * per_workload + blanks) as u16;
                    }
                }
            }
            NodeType::ResourceGroup => {
                if let Some(desc) = &self.description {
                    // One line per resource kind ("Kind: count", joined by ", ").
                    height += (desc.matches(", ").count() + 1) as u16;
                }
            }
            _ => {
                if self.description.is_some() {
                    height += 1; // description line
                }
                if self.ready.is_some() {
                    height += 1; // status line
                }
            }
        }

        height
    }
}

/// Type of node in the graph
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NodeType {
    /// The original object being traced
    Object,
    /// An intermediate resource in the chain
    Chain,
    /// The source resource
    Source,
    /// Upstream external source (e.g., GitHub URL)
    Upstream,
    /// A Flux resource managed by this resource
    FluxResource,
    /// A workload resource (Deployment, StatefulSet, etc.) - individual
    #[allow(dead_code)] // Used in pattern matching, not directly constructed
    Workload,
    /// An aggregate node for grouped workloads (e.g., "Workloads (2)")
    WorkloadGroup,
    /// An aggregate node for grouped resources (e.g., "Resources (7)")
    ResourceGroup,
}

/// An edge representing a relationship between nodes
#[derive(Debug, Clone)]
pub struct GraphEdge {
    /// Source node ID
    pub from: String,
    /// Target node ID
    pub to: String,
    /// Relationship type
    pub relationship: RelationshipType,
}

/// Type of relationship between nodes
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RelationshipType {
    /// Managed by (e.g., Deployment managed by Kustomization)
    ManagedBy,
    /// Sourced from (e.g., Kustomization sourced from GitRepository)
    SourcedFrom,
    /// Owns (e.g., Kustomization owns a Deployment)
    Owns,
}

/// A graph representing resource relationships
#[derive(Debug, Clone)]
pub struct ResourceGraph {
    /// All nodes in the graph
    pub nodes: Vec<GraphNode>,
    /// All edges in the graph
    pub edges: Vec<GraphEdge>,
    /// Map from node ID to index in nodes vector
    pub node_index: HashMap<String, usize>,
}

impl ResourceGraph {
    /// Create a new empty graph
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            node_index: HashMap::new(),
        }
    }

    /// Add a node to the graph
    pub fn add_node(&mut self, node: GraphNode) {
        let id = node.id.clone();
        let index = self.nodes.len();
        self.node_index.insert(id, index);
        self.nodes.push(node);
    }

    /// Add an edge to the graph
    pub fn add_edge(&mut self, edge: GraphEdge) {
        self.edges.push(edge);
    }

    /// Vertical layer a node type occupies in the layout, used to order keyboard
    /// focus from top to bottom (sources at the top, inventory at the bottom).
    /// Mirrors the ordering applied in [`Self::calculate_layout`].
    fn node_layer(node_type: NodeType) -> u8 {
        match node_type {
            NodeType::Upstream => 0,
            NodeType::Source => 1,
            NodeType::Chain => 2,
            NodeType::Object => 3,
            NodeType::FluxResource => 4,
            NodeType::Workload | NodeType::WorkloadGroup => 5,
            NodeType::ResourceGroup => 6,
        }
    }

    /// Node indices in visual top-to-bottom order, used for keyboard focus
    /// navigation. Within a layer, insertion order is preserved (stable sort).
    pub fn focus_order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.nodes.len()).collect();
        order.sort_by_key(|&i| (Self::node_layer(self.nodes[i].node_type), i));
        order
    }

    /// Index of the primary "object" node (the resource being viewed), if any.
    /// Used as the initial focus target so the graph is navigable immediately.
    pub fn object_node_index(&self) -> Option<usize> {
        self.nodes
            .iter()
            .position(|n| n.node_type == NodeType::Object)
    }

    /// Position `indices` as a vertical, horizontally-centered stack starting at
    /// `*current_y`, advancing `*current_y` past the placed nodes.
    fn stack_centered(
        &mut self,
        indices: &[usize],
        available_width: u16,
        center_x: u16,
        current_y: &mut u16,
    ) {
        for &idx in indices {
            if let Some(node) = self.nodes.get_mut(idx) {
                let x = center_x.saturating_sub(node.render_width(available_width) / 2);
                node.position = Some((x, *current_y));
                *current_y += node.render_height() + NODE_VERTICAL_SPACING;
            }
        }
    }

    /// Position `indices` as a vertical stack at a fixed `x`, returning the `y`
    /// just past the last placed node.
    fn stack_at(&mut self, indices: &[usize], x: u16, start_y: u16) -> u16 {
        let mut y = start_y;
        for &idx in indices {
            if let Some(node) = self.nodes.get_mut(idx) {
                node.position = Some((x, y));
                y += node.render_height() + NODE_VERTICAL_SPACING;
            }
        }
        y
    }

    /// Widest rendered width among `indices`, falling back to the minimum width.
    fn max_render_width(&self, indices: &[usize], available_width: u16) -> u16 {
        indices
            .iter()
            .filter_map(|&idx| self.nodes.get(idx))
            .map(|node| node.render_width(available_width))
            .max()
            .unwrap_or(MIN_NODE_WIDTH)
    }

    /// Calculate a simple hierarchical layout for the graph.
    /// Sources sit at the top, the object in the middle, inventory at the bottom.
    /// Returns the (width, height) the layout occupies.
    pub fn calculate_layout(&mut self, available_width: u16, _available_height: u16) -> (u16, u16) {
        if self.nodes.is_empty() {
            return (0, 0);
        }

        // Bucket node indices by type so each layer can be positioned in turn.
        // Individual Workload nodes are never placed directly (they're aggregated
        // into a WorkloadGroup by the builder), so they're intentionally skipped.
        let mut upstream = Vec::new();
        let mut sources = Vec::new();
        let mut chain = Vec::new();
        let mut object = Vec::new();
        let mut flux = Vec::new();
        let mut workload_groups = Vec::new();
        let mut resource_groups = Vec::new();
        for (idx, node) in self.nodes.iter().enumerate() {
            match node.node_type {
                NodeType::Upstream => upstream.push(idx),
                NodeType::Source => sources.push(idx),
                NodeType::Chain => chain.push(idx),
                NodeType::Object => object.push(idx),
                NodeType::FluxResource => flux.push(idx),
                NodeType::WorkloadGroup => workload_groups.push(idx),
                NodeType::ResourceGroup => resource_groups.push(idx),
                NodeType::Workload => {}
            }
        }

        let center_x = available_width / 2;
        let mut current_y = 1u16;

        // Centered single-column layers, top to bottom.
        for layer in [&upstream, &sources, &chain, &object, &flux] {
            self.stack_centered(layer, available_width, center_x, &mut current_y);
        }

        // Inventory groups at the bottom.
        let inventory_y = current_y;
        match (!workload_groups.is_empty(), !resource_groups.is_empty()) {
            (true, true) => {
                // Side by side, centered as a pair with a small gap so the fan-out
                // from the parent stays tight rather than spanning the whole width.
                let left_w = self.max_render_width(&workload_groups, available_width);
                let right_w = self.max_render_width(&resource_groups, available_width);
                let total = left_w + INVENTORY_GROUP_GAP + right_w;
                let left_x = center_x.saturating_sub(total / 2);
                let right_x = left_x + left_w + INVENTORY_GROUP_GAP;

                let left_end = self.stack_at(&workload_groups, left_x, inventory_y);
                let right_end = self.stack_at(&resource_groups, right_x, inventory_y);
                current_y = left_end.max(right_end);
            }
            (true, false) => {
                self.stack_centered(&workload_groups, available_width, center_x, &mut current_y)
            }
            (false, true) => {
                self.stack_centered(&resource_groups, available_width, center_x, &mut current_y)
            }
            (false, false) => {}
        }

        (available_width, current_y)
    }
}

impl Default for ResourceGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, node_type: NodeType) -> GraphNode {
        GraphNode {
            id: id.to_string(),
            kind: "Test".to_string(),
            name: id.to_string(),
            namespace: "default".to_string(),
            node_type,
            ready: None,
            position: None,
            description: None,
        }
    }

    #[test]
    fn focus_order_sorts_top_to_bottom_by_layer() {
        // Insertion order deliberately scrambles the visual layering.
        let mut graph = ResourceGraph::new();
        graph.add_node(node("object", NodeType::Object)); // idx 0, layer 3
        graph.add_node(node("source", NodeType::Source)); // idx 1, layer 1
        graph.add_node(node("wg", NodeType::WorkloadGroup)); // idx 2, layer 5
        graph.add_node(node("upstream", NodeType::Upstream)); // idx 3, layer 0

        // Expected visual order: upstream, source, object, workload group.
        assert_eq!(graph.focus_order(), vec![3, 1, 0, 2]);
    }

    #[test]
    fn focus_order_preserves_insertion_within_a_layer() {
        let mut graph = ResourceGraph::new();
        graph.add_node(node("flux-a", NodeType::FluxResource));
        graph.add_node(node("flux-b", NodeType::FluxResource));
        graph.add_node(node("flux-c", NodeType::FluxResource));

        assert_eq!(graph.focus_order(), vec![0, 1, 2]);
    }

    fn node_with(node_type: NodeType, description: Option<&str>, ready: Option<bool>) -> GraphNode {
        GraphNode {
            id: "id".to_string(),
            kind: "GitRepository".to_string(),
            name: "name".to_string(),
            namespace: "default".to_string(),
            node_type,
            ready,
            position: None,
            description: description.map(|d| d.to_string()),
        }
    }

    #[test]
    fn render_height_matches_rendered_rows() {
        // Plain node: 5 rows of chrome only.
        assert_eq!(node_with(NodeType::Object, None, None).render_height(), 5);
        // Plain node with a description and a status line: 5 + 1 + 1.
        assert_eq!(
            node_with(NodeType::Source, Some("https://x"), Some(true)).render_height(),
            7
        );
        // Resource group: 4 chrome + one row per kind ("A: 1, B: 2, C: 3" => 3).
        assert_eq!(
            node_with(NodeType::ResourceGroup, Some("A: 1, B: 2, C: 3"), None).render_height(),
            7
        );
        // Workload group, single workload (no namespace row): 4 + 3.
        assert_eq!(
            node_with(
                NodeType::WorkloadGroup,
                Some("Deployment|a|ns1|●|1/1"),
                None
            )
            .render_height(),
            7
        );
        // Workload group, two workloads in differing namespaces: 4 + (2*4 + 1).
        assert_eq!(
            node_with(
                NodeType::WorkloadGroup,
                Some("Deployment|a|ns1|●|1/1\nDeployment|b|ns2|●|2/2"),
                None,
            )
            .render_height(),
            13
        );
    }

    #[test]
    fn render_width_clamps_to_bounds_and_available() {
        let short = node_with(NodeType::Object, None, None); // content < MIN
        assert_eq!(short.render_width(100), MIN_NODE_WIDTH);

        // Long description pushes content past MAX, so it clamps to MAX.
        let long = node_with(NodeType::Object, Some(&"x".repeat(120)), None);
        assert_eq!(long.render_width(100), MAX_NODE_WIDTH);

        // A narrow viewport wins over the content-based width.
        assert_eq!(long.render_width(20), 20 - NODE_HORIZONTAL_CHROME);
    }

    #[test]
    fn object_node_index_finds_the_object() {
        let mut graph = ResourceGraph::new();
        graph.add_node(node("source", NodeType::Source));
        graph.add_node(node("object", NodeType::Object));
        assert_eq!(graph.object_node_index(), Some(1));

        let empty = ResourceGraph::new();
        assert_eq!(empty.object_node_index(), None);
    }
}
