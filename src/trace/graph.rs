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
/// The left and right border of a node box, in columns.
const NODE_BORDERS: u16 = 2;

/// Columns `text` occupies. Names, kinds, and URLs are ASCII and the status
/// glyphs (`✓`, `●`) are one column each, so this is the character count.
fn columns(text: &str) -> u16 {
    u16::try_from(text.chars().count()).unwrap_or(u16::MAX)
}

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
    /// Whether this workload or resource group node lists `key` among its
    /// entries (decoded with the same encoders that wrote them).
    pub fn group_lists(&self, key: &crate::watcher::ResourceKey) -> bool {
        let Some(description) = &self.description else {
            return false;
        };
        let matches = |kind: &str, namespace: &str, name: &str| {
            kind == key.resource_type && namespace == key.namespace && name == key.name
        };
        match self.node_type {
            NodeType::WorkloadGroup => description
                .lines()
                .filter_map(crate::kube::workloads::WorkloadRef::parse_graph_line)
                .any(|w| matches(&w.kind, &w.namespace, &w.name)),
            NodeType::ResourceGroup => description
                .lines()
                .filter_map(crate::kube::inventory::InventoryEntry::parse_graph_line)
                .any(|e| matches(&e.kind, &e.namespace, &e.name)),
            _ => false,
        }
    }

    /// Whether this node draws only its title and the group's rows (no type label).
    pub fn is_group(&self) -> bool {
        matches!(
            self.node_type,
            NodeType::WorkloadGroup | NodeType::ResourceGroup
        )
    }

    /// The muted label above a non-group node's title: its kind in capitals,
    /// with `→` on nodes that lead somewhere.
    pub fn type_label(&self) -> String {
        match self.node_type {
            NodeType::Upstream => "UPSTREAM".to_string(),
            NodeType::FluxResource | NodeType::WorkloadGroup | NodeType::ResourceGroup => {
                format!("{}  →", self.kind.to_uppercase())
            }
            NodeType::Source | NodeType::Chain | NodeType::Object | NodeType::Workload => {
                self.kind.to_uppercase()
            }
        }
    }

    /// The node's title: `namespace/name` (as the Flux Operator Web UI shows
    /// it), the bare name for cluster-scoped objects and group nodes.
    pub fn title(&self) -> String {
        if self.namespace.is_empty() || self.is_group() {
            self.name.clone()
        } else {
            format!("{}/{}", self.namespace, self.name)
        }
    }

    /// A workload group's workloads, and whether each shows its namespace
    /// (only when they don't all share one).
    pub fn group_workloads(&self) -> (Vec<crate::kube::workloads::WorkloadRef>, bool) {
        let workloads: Vec<_> = self
            .description
            .iter()
            .flat_map(|desc| desc.lines())
            .filter_map(crate::kube::workloads::WorkloadRef::parse_graph_line)
            .collect();
        let show_namespace = workloads
            .first()
            .is_some_and(|first| workloads.iter().any(|w| w.namespace != first.namespace));
        (workloads, show_namespace)
    }

    /// Width of the widest line this node draws inside its borders, in columns:
    /// the same lines `render_node_text` draws, so none of them is clipped.
    fn content_width(&self) -> u16 {
        let mut lines = vec![self.title()];
        if !self.is_group() {
            lines.push(self.type_label());
        }
        match (self.node_type, &self.description) {
            (NodeType::ResourceGroup, Some(desc)) => lines.extend(
                crate::kube::inventory::kind_counts_from_description(desc)
                    .iter()
                    .map(|(kind, count)| format!("{kind}: {count}")),
            ),
            (NodeType::WorkloadGroup, _) => {
                let (workloads, show_namespace) = self.group_workloads();
                for w in workloads {
                    lines.push(w.kind.to_uppercase());
                    lines.push(format!("{} {}", w.indicator, w.status));
                    if show_namespace {
                        lines.push(w.namespace.clone());
                    }
                    lines.push(w.name);
                }
            }
            (NodeType::Workload, desc) => {
                // "● <status>" over the namespace subtitle.
                lines.extend(desc.iter().map(|d| format!("● {d}")));
                lines.push(self.namespace.clone());
            }
            (_, desc) => {
                lines.extend(desc.iter().flat_map(|d| d.lines()).map(str::to_string));
                if self.ready == Some(false) {
                    lines.push("✗ Not Ready".to_string());
                }
            }
        }
        lines.iter().map(|l| columns(l)).max().unwrap_or(0)
    }

    /// Rendered width in columns, borders included, clamped to sane bounds and
    /// the available width. Single source of truth for node width across
    /// layout and drawing.
    pub fn render_width(&self, available_width: u16) -> u16 {
        self.content_width()
            .saturating_add(NODE_BORDERS)
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
                let (workloads, show_namespace) = self.group_workloads();
                let count = workloads.len();
                if count > 0 {
                    let per_workload = if show_namespace { 4 } else { 3 };
                    let blanks = count.saturating_sub(1);
                    height += (count * per_workload + blanks) as u16;
                }
            }
            NodeType::ResourceGroup => {
                if let Some(desc) = &self.description {
                    // The description carries one encoded entry per line; the
                    // node displays one "Kind: count" line per distinct kind.
                    height +=
                        crate::kube::inventory::kind_counts_from_description(desc).len() as u16;
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

    /// Add an edge to the graph,
    /// once: the same relationship found two ways (a manager's label and its
    /// owner reference) draws one connector.
    pub fn add_edge(&mut self, edge: GraphEdge) {
        let duplicate = self
            .edges
            .iter()
            .any(|e| e.from == edge.from && e.to == edge.to && e.relationship == edge.relationship);
        if !duplicate {
            self.edges.push(edge);
        }
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
    /// navigation. Upstream nodes follow their rows ([`Self::upstream_ranks`]);
    /// within a layer or row, insertion order is preserved (stable sort).
    pub fn focus_order(&self) -> Vec<usize> {
        let ranks = self.upstream_ranks();
        let mut order: Vec<usize> = (0..self.nodes.len()).collect();
        order.sort_by_key(|&i| {
            let layer = Self::node_layer(self.nodes[i].node_type);
            match ranks.get(&i) {
                // Upstream rows top-down; within a row, sources before managers.
                Some(&rank) => (0, std::cmp::Reverse(rank), layer, i),
                None => (layer, std::cmp::Reverse(0), 0, i),
            }
        });
        order
    }

    /// Whether a node sits above the object: its source lineage (upstream,
    /// sources, intermediates like a HelmChart) and its managers.
    fn is_upstream(node_type: NodeType) -> bool {
        matches!(
            node_type,
            NodeType::Upstream | NodeType::Source | NodeType::Chain
        )
    }

    /// The row of every upstream node, counted up from the object: 1 for what
    /// points at the object directly (its source, its manager), one more for
    /// each hop further up (ExternalArtifact → ArtifactGenerator → its
    /// sources → their Git origin). Ranking by edges rather than node type
    /// keeps a parent above its child, and siblings (a manager beside the
    /// source, an ArtifactGenerator's several sources) side by side, so no
    /// connector runs behind an unrelated node.
    fn upstream_ranks(&self) -> HashMap<usize, u16> {
        fn rank_of(
            graph: &ResourceGraph,
            idx: usize,
            ranks: &mut HashMap<usize, u16>,
            visiting: &mut Vec<usize>,
        ) -> u16 {
            if let Some(&rank) = ranks.get(&idx) {
                return rank;
            }
            if visiting.contains(&idx) {
                return 1; // A cycle: cut it here.
            }
            visiting.push(idx);
            let id = &graph.nodes[idx].id;
            let mut rank = 1;
            for edge in graph.edges.iter().filter(|e| &e.from == id) {
                if let Some(&child) = graph.node_index.get(&edge.to)
                    && ResourceGraph::is_upstream(graph.nodes[child].node_type)
                {
                    rank = rank.max(1 + rank_of(graph, child, ranks, visiting));
                }
            }
            visiting.pop();
            ranks.insert(idx, rank);
            rank
        }

        let mut ranks = HashMap::new();
        for idx in 0..self.nodes.len() {
            if Self::is_upstream(self.nodes[idx].node_type) {
                rank_of(self, idx, &mut ranks, &mut Vec::new());
            }
        }
        ranks
    }

    /// Lay `indices` out as one centered row starting at `*current_y`, or as a
    /// centered stack when the row doesn't fit the width.
    fn row_centered(
        &mut self,
        indices: &[usize],
        available_width: u16,
        center_x: u16,
        current_y: &mut u16,
    ) {
        let widths: Vec<u16> = indices
            .iter()
            .filter_map(|&idx| self.nodes.get(idx))
            .map(|node| node.render_width(available_width))
            .collect();
        let gaps = INVENTORY_GROUP_GAP * (widths.len().saturating_sub(1) as u16);
        let total = widths.iter().sum::<u16>() + gaps;
        if indices.len() < 2 || total > available_width {
            self.stack_centered(indices, available_width, center_x, current_y);
            return;
        }
        let mut x = center_x.saturating_sub(total / 2);
        let mut height = 0;
        for (&idx, width) in indices.iter().zip(widths) {
            if let Some(node) = self.nodes.get_mut(idx) {
                node.position = Some((x, *current_y));
                height = height.max(node.render_height());
            }
            x += width + INVENTORY_GROUP_GAP;
        }
        *current_y += height + NODE_VERTICAL_SPACING;
    }

    /// The node showing `key` (#266): its own node, else the workload or
    /// resource group that lists it.
    pub fn node_containing(&self, key: &crate::watcher::ResourceKey) -> Option<usize> {
        let id = format!("{}:{}:{}", key.resource_type, key.namespace, key.name);
        if let Some(&index) = self.node_index.get(&id) {
            return Some(index);
        }
        self.nodes.iter().position(|node| node.group_lists(key))
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
        let mut object = Vec::new();
        let mut flux = Vec::new();
        let mut workload_groups = Vec::new();
        let mut resource_groups = Vec::new();
        for (idx, node) in self.nodes.iter().enumerate() {
            match node.node_type {
                // Placed by rank below.
                NodeType::Upstream | NodeType::Source | NodeType::Chain => {}
                NodeType::Object => object.push(idx),
                NodeType::FluxResource => flux.push(idx),
                NodeType::WorkloadGroup => workload_groups.push(idx),
                NodeType::ResourceGroup => resource_groups.push(idx),
                NodeType::Workload => {}
            }
        }

        let center_x = available_width / 2;
        let mut current_y = 1u16;

        // Upstream rows, furthest from the object first.
        let ranks = self.upstream_ranks();
        let top_rank = ranks.values().copied().max().unwrap_or(0);
        for rank in (1..=top_rank).rev() {
            let mut row: Vec<usize> = ranks
                .iter()
                .filter(|&(_, &r)| r == rank)
                .map(|(&idx, _)| idx)
                .collect();
            // Sources on the left, managers beside them (as focus orders them).
            row.sort_unstable_by_key(|&idx| (Self::node_layer(self.nodes[idx].node_type), idx));
            self.row_centered(&row, available_width, center_x, &mut current_y);
        }

        // Centered single-column layers, top to bottom.
        for layer in [&object, &flux] {
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

    #[test]
    fn node_containing_finds_the_node_or_the_group_listing_it() {
        use crate::watcher::ResourceKey;
        let node =
            |id: &str, kind: &str, node_type: NodeType, description: Option<String>| GraphNode {
                id: id.into(),
                kind: kind.into(),
                name: id.into(),
                namespace: "apps".into(),
                node_type,
                ready: None,
                position: None,
                description,
            };
        let workloads = crate::kube::workloads::WorkloadRef {
            kind: "Deployment".into(),
            name: "web".into(),
            namespace: "apps".into(),
            indicator: "●".into(),
            status: "Replicas: 1/1".into(),
            api_version: "apps/v1".into(),
        }
        .to_graph_line();
        let resources = crate::kube::inventory::InventoryEntry {
            kind: "ConfigMap".into(),
            namespace: "apps".into(),
            name: "settings".into(),
            api_version: "v1".into(),
        }
        .to_graph_line();
        let mut graph = ResourceGraph::new();
        graph.add_node(node(
            "Kustomization:flux-system:apps",
            "Kustomization",
            NodeType::Object,
            None,
        ));
        graph.add_node(node(
            "HelmRelease:apps:web",
            "HelmRelease",
            NodeType::FluxResource,
            None,
        ));
        graph.add_node(node(
            "workloadgroup:apps",
            "Workloads",
            NodeType::WorkloadGroup,
            Some(workloads),
        ));
        graph.add_node(node(
            "resourcegroup:apps",
            "Resources",
            NodeType::ResourceGroup,
            Some(resources),
        ));

        let at = |kind: &str, namespace: &str, name: &str| {
            graph.node_containing(&ResourceKey::new(kind, namespace, name))
        };
        assert_eq!(at("HelmRelease", "apps", "web"), Some(1), "its own node");
        assert_eq!(
            at("Deployment", "apps", "web"),
            Some(2),
            "the workload group"
        );
        assert_eq!(
            at("ConfigMap", "apps", "settings"),
            Some(3),
            "the resource group"
        );
        assert_eq!(at("Deployment", "other", "web"), None, "namespace matters");
        assert_eq!(at("Pod", "apps", "web-abc"), None);
    }

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

    fn edge(from: &str, to: &str, relationship: RelationshipType) -> GraphEdge {
        GraphEdge {
            from: from.into(),
            to: to.into(),
            relationship,
        }
    }

    /// The user-reported shape: a Kustomization sourced from an
    /// ExternalArtifact, applied by a parent Kustomization. Each lineage hop
    /// gets its own row above the object, the manager shares the source's
    /// row, and focus walks the rows top-down.
    #[test]
    fn upstream_lineage_is_ranked_into_rows_above_the_object() {
        let mut graph = ResourceGraph::new();
        graph.add_node(node("ks", NodeType::Object)); // 0
        graph.add_node(node("artifact", NodeType::Source)); // 1
        graph.add_node(node("generator", NodeType::Source)); // 2
        graph.add_node(node("repo", NodeType::Source)); // 3
        graph.add_node(node("origin", NodeType::Upstream)); // 4
        graph.add_node(node("parent", NodeType::Chain)); // 5
        graph.add_edge(edge("artifact", "ks", RelationshipType::SourcedFrom));
        graph.add_edge(edge("generator", "artifact", RelationshipType::SourcedFrom));
        graph.add_edge(edge("repo", "generator", RelationshipType::SourcedFrom));
        graph.add_edge(edge("origin", "repo", RelationshipType::SourcedFrom));
        graph.add_edge(edge("parent", "ks", RelationshipType::ManagedBy));
        graph.calculate_layout(120, 60);

        let pos = |i: usize| graph.nodes[i].position.unwrap();
        let y = |i: usize| pos(i).1;
        assert!(y(4) < y(3) && y(3) < y(2) && y(2) < y(1) && y(1) < y(0));
        assert_eq!(y(5), y(1), "the manager sits beside the source");
        assert!(pos(1).0 < pos(5).0, "source left, manager right");
        assert_eq!(graph.focus_order(), vec![4, 3, 2, 1, 5, 0]);
    }

    #[test]
    fn an_edge_found_twice_is_drawn_once() {
        let mut graph = ResourceGraph::new();
        graph.add_edge(edge("parent", "ks", RelationshipType::ManagedBy));
        graph.add_edge(edge("parent", "ks", RelationshipType::ManagedBy));
        graph.add_edge(edge("parent", "ks", RelationshipType::Owns));
        assert_eq!(graph.edges.len(), 2);
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
        // Resource group: 4 chrome + one row per *distinct kind* in the encoded
        // entries (two Services collapse into a single "Service: 2" row).
        assert_eq!(
            node_with(
                NodeType::ResourceGroup,
                Some("ConfigMap|ns|cm|v1\nService|ns|svc-a|v1\nService|ns|svc-b|v1"),
                None
            )
            .render_height(),
            6
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

        // Multi-line descriptions are sized by their widest rendered line, not
        // by the whole encoded string: a resource group is sized by its
        // "Kind: count" summary, so a long entry list still renders compactly.
        let group = node_with(
            NodeType::ResourceGroup,
            Some(
                "ConfigMap|a-very-long-namespace-name|a-very-long-resource-name|v1\n\
                 ConfigMap|a-very-long-namespace-name|another-long-resource-name|v1",
            ),
            None,
        );
        assert_eq!(group.render_width(100), MIN_NODE_WIDTH);
    }

    /// Every line a node draws fits inside its borders (#340).
    #[test]
    fn render_width_fits_every_drawn_line_inside_the_borders() {
        let inner = |node: &GraphNode| node.render_width(200) - NODE_BORDERS;

        // The title is `namespace/name`: the namespace counts too.
        let mut source = node_with(NodeType::Source, None, Some(true));
        source.namespace = "flux-resources".to_string();
        source.name = "flux-resources-podinfo-oci-chart".to_string();
        assert_eq!(
            source.title(),
            "flux-resources/flux-resources-podinfo-oci-chart"
        );
        assert_eq!(inner(&source), columns(&source.title()));

        // A description as wide as the box used to lose its end to the borders.
        let url = "chart: oci://ghcr.io/stefanprodan/charts/podinfo";
        let release = node_with(NodeType::FluxResource, Some(url), Some(true));
        assert_eq!(inner(&release), columns(url));

        // Cluster-scoped objects and groups are titled by name alone.
        let mut cluster_scoped = node_with(NodeType::Object, None, None);
        cluster_scoped.namespace = String::new();
        assert_eq!(cluster_scoped.title(), "name");
        let group = node_with(NodeType::ResourceGroup, None, None);
        assert_eq!(group.title(), "name");

        // Workload groups are sized by the rows they draw, not the encoded lines.
        let name = "a-deployment-with-a-name-longer-than-the-minimum";
        let workloads = node_with(
            NodeType::WorkloadGroup,
            Some(&format!("Deployment|{name}|apps|●|Replicas: 1/1|apps/v1")),
            None,
        );
        assert_eq!(inner(&workloads), columns(name));
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
