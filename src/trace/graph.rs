//! Graph data structures for visualizing resource relationships
//!
//! This module provides structures to represent resources and their relationships
//! as a graph, suitable for visualization in the TUI.

use std::collections::HashMap;

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

    /// Calculate a simple hierarchical layout for the graph
    /// Returns (width, height) needed for the layout
    pub fn calculate_layout(&mut self, available_width: u16, _available_height: u16) -> (u16, u16) {
        if self.nodes.is_empty() {
            return (0, 0);
        }

        // Flexible layout with dynamic sizing
        // UPSTREAM FIRST (sources at top), then object, then workloads/resources at bottom
        let min_node_width = 30u16; // Minimum width per node
        let max_node_width = available_width.saturating_sub(4).min(60); // Max width, leave margins
        let vertical_spacing = 3u16; // Space between nodes (increased from 2)

        // Group nodes by type
        let mut object_nodes = Vec::new();
        let mut chain_nodes = Vec::new();
        let mut source_nodes = Vec::new();
        let mut upstream_nodes = Vec::new();
        let mut flux_resource_nodes = Vec::new();
        let mut workload_nodes = Vec::new();
        let mut workload_group_nodes = Vec::new();
        let mut resource_group_nodes = Vec::new();

        for (idx, node) in self.nodes.iter().enumerate() {
            match node.node_type {
                NodeType::Object => object_nodes.push(idx),
                NodeType::Chain => chain_nodes.push(idx),
                NodeType::Source => source_nodes.push(idx),
                NodeType::Upstream => upstream_nodes.push(idx),
                NodeType::FluxResource => flux_resource_nodes.push(idx),
                NodeType::Workload => workload_nodes.push(idx),
                NodeType::WorkloadGroup => workload_group_nodes.push(idx),
                NodeType::ResourceGroup => resource_group_nodes.push(idx),
            }
        }

        // Calculate positions with flexible layout
        // UPSTREAM FIRST (sources at top), then object, then workloads/resources at bottom
        let center_x = available_width / 2;
        let mut current_y = 1u16;

        // Calculate node dimensions based on content
        let calculate_node_width = |node: &GraphNode| -> u16 {
            let name_len = node.name.len() as u16;
            let kind_len = node.kind.len() as u16;
            let desc_len = node
                .description
                .as_ref()
                .map(|d| d.len() as u16)
                .unwrap_or(0);
            let max_content = name_len.max(kind_len).max(desc_len);
            max_content.max(min_node_width).min(max_node_width)
        };

        let calculate_node_height = |node: &GraphNode| -> u16 {
            // Calculate height: title section + content + borders
            // For WorkloadGroup/ResourceGroup: name (1) + separator (1) + borders (2) = 4
            // For other nodes: kind label (1) + name (1) + separator (1) + borders (2) = 5
            let mut height = if matches!(
                node.node_type,
                NodeType::WorkloadGroup | NodeType::ResourceGroup
            ) {
                4u16 // name + separator + borders
            } else {
                5u16 // kind label + name + separator + borders
            };

            // Workload nodes have: type label + name + description (replica count) + resource name subtitle
            if matches!(node.node_type, NodeType::Workload) {
                if node.description.is_some() {
                    height += 2; // description line + resource name subtitle
                }
            } else if matches!(node.node_type, NodeType::WorkloadGroup) {
                // Workload group nodes show each workload as 4-5 lines (kind, name, status, optional namespace) + 1 blank line between
                if let Some(ref desc) = node.description {
                    // Count the number of workloads (separated by newlines)
                    let workload_lines: Vec<&str> = desc.lines().collect();
                    let workload_count = workload_lines.len();

                    if workload_count > 0 {
                        // Check if namespaces differ (if so, namespace line will be shown for each workload)
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
            height
        };

        // Position upstream nodes at VERY TOP (external sources like GitHub URLs)
        for &idx in &upstream_nodes {
            if let Some(node) = self.nodes.get_mut(idx) {
                let node_width = calculate_node_width(node);
                let node_height = calculate_node_height(node);
                node.position = Some((center_x.saturating_sub(node_width / 2), current_y));
                current_y += node_height + vertical_spacing;
            }
        }

        // Position source nodes (Flux source resources)
        for &idx in &source_nodes {
            if let Some(node) = self.nodes.get_mut(idx) {
                let node_width = calculate_node_width(node);
                let node_height = calculate_node_height(node);
                node.position = Some((center_x.saturating_sub(node_width / 2), current_y));
                current_y += node_height + vertical_spacing;
            }
        }

        // Position chain nodes in middle (intermediate resources like HelmChart)
        for &idx in &chain_nodes {
            if let Some(node) = self.nodes.get_mut(idx) {
                let node_width = calculate_node_width(node);
                let node_height = calculate_node_height(node);
                node.position = Some((center_x.saturating_sub(node_width / 2), current_y));
                current_y += node_height + vertical_spacing;
            }
        }

        // Position object node (the Kustomization/HelmRelease being viewed)
        for &idx in &object_nodes {
            if let Some(node) = self.nodes.get_mut(idx) {
                let node_width = calculate_node_width(node);
                let node_height = calculate_node_height(node);
                node.position = Some((center_x.saturating_sub(node_width / 2), current_y));
                current_y += node_height + vertical_spacing;
            }
        }

        // Position Flux resources managed by this resource (individual items)
        for &idx in &flux_resource_nodes {
            if let Some(node) = self.nodes.get_mut(idx) {
                let node_width = calculate_node_width(node);
                let node_height = calculate_node_height(node);
                node.position = Some((center_x.saturating_sub(node_width / 2), current_y));
                current_y += node_height + vertical_spacing;
            }
        }

        // Position inventory nodes at BOTTOM (side by side if multiple categories)
        let inventory_y = current_y;
        let has_workload_groups = !workload_group_nodes.is_empty();
        let has_resource_groups = !resource_group_nodes.is_empty();

        if has_workload_groups && has_resource_groups {
            // Both workload groups and resource groups exist - place side by side
            let left_x = center_x.saturating_sub(max_node_width + 5);
            let right_x = center_x + 5;

            // Workload groups on left
            let mut workloads_y = inventory_y;
            for &idx in &workload_group_nodes {
                if let Some(node) = self.nodes.get_mut(idx) {
                    let node_height = calculate_node_height(node);
                    node.position = Some((left_x, workloads_y));
                    workloads_y += node_height + vertical_spacing;
                }
            }

            // Resource groups on right
            let mut resources_y = inventory_y;
            for &idx in &resource_group_nodes {
                if let Some(node) = self.nodes.get_mut(idx) {
                    let node_height = calculate_node_height(node);
                    node.position = Some((right_x, resources_y));
                    resources_y += node_height + vertical_spacing;
                }
            }

            current_y = workloads_y.max(resources_y);
        } else if has_workload_groups {
            // Only workload groups - center them
            for &idx in &workload_group_nodes {
                if let Some(node) = self.nodes.get_mut(idx) {
                    let node_width = calculate_node_width(node);
                    let node_height = calculate_node_height(node);
                    node.position = Some((center_x.saturating_sub(node_width / 2), current_y));
                    current_y += node_height + vertical_spacing;
                }
            }
        } else if has_resource_groups {
            // Only resource groups - center them
            for &idx in &resource_group_nodes {
                if let Some(node) = self.nodes.get_mut(idx) {
                    let node_width = calculate_node_width(node);
                    let node_height = calculate_node_height(node);
                    node.position = Some((center_x.saturating_sub(node_width / 2), current_y));
                    current_y += node_height + vertical_spacing;
                }
            }
        }

        (available_width, current_y)
    }
}

impl Default for ResourceGraph {
    fn default() -> Self {
        Self::new()
    }
}
