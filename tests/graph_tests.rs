//! Graph functionality tests
//!
//! Tests for resource graph building, layout calculation, and node rendering

use flux9s::trace::{
    GraphEdge, GraphNode, NodeType, RelationshipType, ResourceGraph, is_resource_type_with_graph,
};

/// A workload group grows by 3 rows per workload (kind, name, status), 4 when
/// the workloads span namespaces (each shows its own), plus a blank between
/// them — and the layout reserves all of it.
#[test]
fn test_workload_group_height_calculation() {
    let group = |lines: &str| GraphNode {
        id: "workloadgroup:test".to_string(),
        kind: "Workloads".to_string(),
        name: "Workloads".to_string(),
        namespace: "test".to_string(),
        node_type: NodeType::WorkloadGroup,
        ready: None,
        position: None,
        description: Some(lines.to_string()),
    };
    let one_namespace = group(
        "Deployment|app1|ns1|●|Replicas: 1/1|apps/v1\nDeployment|app2|ns1|●|Replicas: 2/2|apps/v1",
    );
    let two_namespaces = group(
        "Deployment|app1|ns1|●|Replicas: 1/1|apps/v1\nDeployment|app2|ns2|●|Replicas: 2/2|apps/v1",
    );
    // 4 rows of chrome + 2 workloads + 1 blank between them.
    assert_eq!(one_namespace.render_height(), 4 + 2 * 3 + 1);
    assert_eq!(two_namespaces.render_height(), 4 + 2 * 4 + 1);

    let mut graph = ResourceGraph::new();
    graph.add_node(two_namespaces);
    let (_width, height) = graph.calculate_layout(100, 50);
    let (_, y) = graph.nodes[0].position.unwrap();
    assert!(height >= y + graph.nodes[0].render_height());
}

#[test]
fn test_resource_group_height_calculation() {
    let mut graph = ResourceGraph::new();

    // Add ResourceGroup with multiple resource types
    let resource_group = GraphNode {
        id: "resourcegroup:test".to_string(),
        kind: "Resources".to_string(),
        name: "Resources (7)".to_string(),
        namespace: "test".to_string(),
        node_type: NodeType::ResourceGroup,
        ready: None,
        position: None,
        description: Some(
            "ConfigMap|test|cm-1|v1\nSecret|test|sec-1|v1\nSecret|test|sec-2|v1\n\
             Service|test|svc-1|v1\nService|test|svc-2|v1\nService|test|svc-3|v1\n\
             Service|test|svc-4|v1"
                .to_string(),
        ),
    };
    graph.add_node(resource_group);

    let (_width, height) = graph.calculate_layout(100, 50);

    // Find the resource group node
    let rg_node = graph
        .nodes
        .iter()
        .find(|n| n.id == "resourcegroup:test")
        .unwrap();

    // Verify it has a position
    assert!(rg_node.position.is_some());

    // Height accounts for the 3 distinct kinds the 7 entries collapse into
    // (one "Kind: count" line each), plus 4 rows of chrome:
    // name (1) + separator (1) + borders (2).
    assert_eq!(rg_node.render_height(), 7);
    assert!(height >= rg_node.render_height());
}

#[test]
fn test_side_by_side_layout() {
    let mut graph = ResourceGraph::new();

    // Add Kustomization
    let ks = GraphNode {
        id: "ks-1".to_string(),
        kind: "Kustomization".to_string(),
        name: "test-app".to_string(),
        namespace: "default".to_string(),
        node_type: NodeType::Object,
        ready: Some(true),
        position: None,
        description: Some("./apps".to_string()),
    };
    graph.add_node(ks);

    // Add WorkloadGroup
    let wg = GraphNode {
        id: "workloadgroup:default".to_string(),
        kind: "Workloads".to_string(),
        name: "Workloads (1)".to_string(),
        namespace: "default".to_string(),
        node_type: NodeType::WorkloadGroup,
        ready: None,
        position: None,
        description: Some("Deployment|app|default|●|Replicas: 1/1".to_string()),
    };
    graph.add_node(wg);

    // Add ResourceGroup
    let rg = GraphNode {
        id: "resourcegroup:default".to_string(),
        kind: "Resources".to_string(),
        name: "Resources (2)".to_string(),
        namespace: "default".to_string(),
        node_type: NodeType::ResourceGroup,
        ready: None,
        position: None,
        description: Some("ConfigMap|default|cm-1|v1\nSecret|default|sec-1|v1".to_string()),
    };
    graph.add_node(rg);

    // Add edges
    graph.add_edge(GraphEdge {
        from: "ks-1".to_string(),
        to: "workloadgroup:default".to_string(),
        relationship: RelationshipType::Owns,
    });
    graph.add_edge(GraphEdge {
        from: "ks-1".to_string(),
        to: "resourcegroup:default".to_string(),
        relationship: RelationshipType::Owns,
    });

    // Calculate layout
    let (_width, height) = graph.calculate_layout(100, 50);

    // Verify layout succeeded
    assert!(height > 0);

    // Verify all nodes have positions
    for node in &graph.nodes {
        assert!(
            node.position.is_some(),
            "Node {} should have a position",
            node.id
        );
    }

    // Verify WorkloadGroup and ResourceGroup are positioned side-by-side
    let wg_node = graph
        .nodes
        .iter()
        .find(|n| n.id == "workloadgroup:default")
        .unwrap();
    let rg_node = graph
        .nodes
        .iter()
        .find(|n| n.id == "resourcegroup:default")
        .unwrap();

    let (wg_x, wg_y) = wg_node.position.unwrap();
    let (rg_x, rg_y) = rg_node.position.unwrap();

    // They should be at the same Y level (side-by-side)
    assert_eq!(
        wg_y, rg_y,
        "WorkloadGroup and ResourceGroup should be at the same Y level"
    );

    // Side by side without overlapping.
    let (left, right) = if wg_x < rg_x {
        (wg_node, rg_x)
    } else {
        (rg_node, wg_x)
    };
    assert!(
        left.position.unwrap().0 + left.render_width(100) <= right,
        "the groups overlap"
    );

    // Both below the object they belong to, clear of it.
    let ks_node = graph.nodes.iter().find(|n| n.id == "ks-1").unwrap();
    let (_, ks_y) = ks_node.position.unwrap();
    assert!(wg_y >= ks_y + ks_node.render_height());
}

#[test]
fn test_is_resource_type_with_graph() {
    // Test supported resource types
    assert!(is_resource_type_with_graph("Kustomization"));
    assert!(is_resource_type_with_graph("HelmRelease"));
    assert!(is_resource_type_with_graph("ArtifactGenerator"));
    assert!(is_resource_type_with_graph("FluxInstance"));
    assert!(is_resource_type_with_graph("ResourceSet"));

    // Test unsupported resource types
    assert!(!is_resource_type_with_graph("GitRepository"));
    assert!(!is_resource_type_with_graph("OCIRepository"));
    assert!(!is_resource_type_with_graph("HelmRepository"));
    assert!(!is_resource_type_with_graph("Bucket"));
    assert!(!is_resource_type_with_graph("Deployment"));
    assert!(!is_resource_type_with_graph("Service"));
    assert!(!is_resource_type_with_graph("ConfigMap"));
    assert!(!is_resource_type_with_graph(""));
}
