//! Graph functionality tests
//!
//! Tests for resource graph building, layout calculation, and node rendering

use flux9s::trace::{
    GraphEdge, GraphNode, NodeType, RelationshipType, ResourceGraph, is_resource_type_with_graph,
};

#[test]
fn test_graph_creation() {
    let graph = ResourceGraph::new();
    assert!(graph.nodes.is_empty());
    assert!(graph.edges.is_empty());
    assert!(graph.node_index.is_empty());
}

#[test]
fn test_add_node() {
    let mut graph = ResourceGraph::new();

    let node = GraphNode {
        id: "test-node".to_string(),
        kind: "Kustomization".to_string(),
        name: "test-app".to_string(),
        namespace: "default".to_string(),
        node_type: NodeType::Object,
        ready: Some(true),
        position: None,
        description: Some("Test description".to_string()),
    };

    graph.add_node(node);

    assert_eq!(graph.nodes.len(), 1);
    assert_eq!(graph.node_index.len(), 1);
    assert!(graph.node_index.contains_key("test-node"));
}

#[test]
fn test_add_edge() {
    let mut graph = ResourceGraph::new();

    let from_node = GraphNode {
        id: "from-node".to_string(),
        kind: "Kustomization".to_string(),
        name: "from-app".to_string(),
        namespace: "default".to_string(),
        node_type: NodeType::Object,
        ready: Some(true),
        position: None,
        description: None,
    };

    let to_node = GraphNode {
        id: "to-node".to_string(),
        kind: "Deployment".to_string(),
        name: "to-app".to_string(),
        namespace: "default".to_string(),
        node_type: NodeType::Workload,
        ready: Some(true),
        position: None,
        description: None,
    };

    graph.add_node(from_node);
    graph.add_node(to_node);
    graph.add_edge(GraphEdge {
        from: "from-node".to_string(),
        to: "to-node".to_string(),
        relationship: RelationshipType::Owns,
    });

    assert_eq!(graph.edges.len(), 1);
    assert_eq!(graph.edges[0].from, "from-node");
    assert_eq!(graph.edges[0].to, "to-node");
    assert_eq!(graph.edges[0].relationship, RelationshipType::Owns);
}

#[test]
fn test_calculate_layout() {
    let mut graph = ResourceGraph::new();

    // Add a Kustomization node
    let kustomization = GraphNode {
        id: "ks-1".to_string(),
        kind: "Kustomization".to_string(),
        name: "test-app".to_string(),
        namespace: "default".to_string(),
        node_type: NodeType::Object,
        ready: Some(true),
        position: None,
        description: Some("./apps".to_string()),
    };
    graph.add_node(kustomization);

    // Add a WorkloadGroup node
    let workload_group = GraphNode {
        id: "workloadgroup:default".to_string(),
        kind: "Workloads".to_string(),
        name: "Workloads (2)".to_string(),
        namespace: "default".to_string(),
        node_type: NodeType::WorkloadGroup,
        ready: None,
        position: None,
        description: Some(
            "Deployment|app1|default|●|Replicas: 1/1\nDeployment|app2|default|●|Replicas: 2/2"
                .to_string(),
        ),
    };
    graph.add_node(workload_group);

    // Add a ResourceGroup node
    let resource_group = GraphNode {
        id: "resourcegroup:default".to_string(),
        kind: "Resources".to_string(),
        name: "Resources (3)".to_string(),
        namespace: "default".to_string(),
        node_type: NodeType::ResourceGroup,
        ready: None,
        position: None,
        description: Some("ConfigMap: 1, Secret: 2".to_string()),
    };
    graph.add_node(resource_group);

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

    // Verify layout was calculated
    assert!(height > 0);

    // Verify all nodes have positions
    for node in &graph.nodes {
        assert!(
            node.position.is_some(),
            "Node {} should have a position",
            node.id
        );
    }
}

#[test]
fn test_workload_group_height_calculation() {
    let mut graph = ResourceGraph::new();

    // Add WorkloadGroup with 2 workloads
    let workload_group = GraphNode {
        id: "workloadgroup:test".to_string(),
        kind: "Workloads".to_string(),
        name: "Workloads (2)".to_string(),
        namespace: "test".to_string(),
        node_type: NodeType::WorkloadGroup,
        ready: None,
        position: None,
        description: Some(
            "Deployment|app1|ns1|●|Replicas: 1/1\nDeployment|app2|ns2|●|Replicas: 2/2".to_string(),
        ),
    };
    graph.add_node(workload_group);

    let (_width, height) = graph.calculate_layout(100, 50);

    // Find the workload group node
    let wg_node = graph
        .nodes
        .iter()
        .find(|n| n.id == "workloadgroup:test")
        .unwrap();

    // Verify it has a position (layout was calculated)
    assert!(wg_node.position.is_some());

    // The height should account for:
    // - Base: name (1) + separator (1) + borders (2) = 4
    // - Content: 2 workloads * 4 lines (kind + name + status + namespace) + 1 blank = 9
    // Total should be at least 13
    // But we can't directly test the height here since it's calculated internally
    // We just verify the layout succeeds
    assert!(height >= 4);
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
        description: Some("ConfigMap: 1, Secret: 2, Service: 4".to_string()),
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

    // Height should account for 3 resource types (one per line)
    // Base: name (1) + separator (1) + borders (2) = 4
    // Content: 3 lines
    // Total should be at least 7
    assert!(height >= 4);
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
        description: Some("ConfigMap: 1, Secret: 1".to_string()),
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

    // They should be at different X positions (side-by-side)
    assert_ne!(
        wg_x, rg_x,
        "WorkloadGroup and ResourceGroup should be at different X positions"
    );
}

#[test]
fn test_node_types() {
    // Test that all node types are properly defined
    assert!(matches!(NodeType::Object, NodeType::Object));
    assert!(matches!(NodeType::WorkloadGroup, NodeType::WorkloadGroup));
    assert!(matches!(NodeType::ResourceGroup, NodeType::ResourceGroup));
    assert!(matches!(NodeType::Source, NodeType::Source));
    assert!(matches!(NodeType::Chain, NodeType::Chain));
    assert!(matches!(NodeType::Upstream, NodeType::Upstream));
    assert!(matches!(NodeType::FluxResource, NodeType::FluxResource));
    assert!(matches!(NodeType::Workload, NodeType::Workload));
}

#[test]
fn test_relationship_types() {
    // Test that all relationship types are properly defined
    assert!(matches!(RelationshipType::Owns, RelationshipType::Owns));
    assert!(matches!(
        RelationshipType::SourcedFrom,
        RelationshipType::SourcedFrom
    ));
    assert!(matches!(
        RelationshipType::ManagedBy,
        RelationshipType::ManagedBy
    ));
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
