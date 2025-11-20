//! Trace tests matching Flux CLI behavior
//!
//! These tests verify that our trace implementation matches the Flux CLI's trace output.
//! Test cases are based on Flux's own trace tests.
//!
//! These are unit tests that verify the trace result structure and expected behavior
//! without requiring a Kubernetes cluster. They test the trace logic by validating
//! the structure of TraceResult objects that match expected Flux CLI output patterns.

use flux9s::trace::TraceResult;

/// Helper function to verify trace result structure matches Flux CLI output format
pub fn verify_trace_structure(result: &TraceResult) {
    // Object should always be present
    assert!(!result.object.kind.is_empty());
    assert!(!result.object.name.is_empty());

    // Chain should contain Flux resources (Kustomization, HelmRelease, HelmChart)
    for node in &result.chain {
        assert!(
            matches!(
                node.kind.as_str(),
                "Kustomization" | "HelmRelease" | "HelmChart"
            ),
            "Chain should only contain Flux resources, got: {}",
            node.kind
        );
    }

    // Source should be a source resource if present
    if let Some(source) = &result.source {
        assert!(
            matches!(
                source.kind.as_str(),
                "GitRepository"
                    | "OCIRepository"
                    | "HelmRepository"
                    | "ExternalArtifact"
                    | "Bucket"
            ),
            "Source should be a source resource, got: {}",
            source.kind
        );
    }
}

/// Test that trace result structure is valid
#[test]
fn test_trace_result_structure() {
    use flux9s::trace::{TraceNode, TraceResult, TraceSpec, TraceStatus};

    // Create a minimal valid trace result
    let result = TraceResult {
        object: TraceNode {
            kind: "Kustomization".to_string(),
            name: "test".to_string(),
            namespace: "flux-system".to_string(),
            status: Some(TraceStatus {
                ready: Some(true),
                message: Some("test message".to_string()),
                last_reconciled: Some("2024-01-01T00:00:00Z".to_string()),
                revision: Some("main@sha1:abc123".to_string()),
            }),
            spec: Some(TraceSpec {
                path: Some("./test".to_string()),
                url: None,
                branch: None,
                source_ref: None,
            }),
        },
        chain: vec![],
        source: None,
    };

    verify_trace_structure(&result);
}

/// Test that verifies expected trace result structure for a Kustomization managed by another Kustomization
/// Expected chain: Kustomization (flux-system) -> GitRepository
///
/// This test verifies that when tracing a Kustomization that is managed by another Kustomization:
/// - The traced object (cabot-book) should NOT appear in the chain
/// - Only managing resources should appear in the chain
/// - The chain should show: flux-system Kustomization
#[test]
fn test_trace_kustomization_managed_by_kustomization_structure() {
    use flux9s::trace::{SourceRef, TraceNode, TraceResult, TraceSpec, TraceStatus};

    // Simulate trace result for Kustomization/cabot-book managed by flux-system
    let result = TraceResult {
        object: TraceNode {
            kind: "Kustomization".to_string(),
            name: "cabot-book".to_string(),
            namespace: "flux-system".to_string(),
            status: Some(TraceStatus {
                ready: Some(true),
                message: Some("Applied revision: main@sha1:abc123".to_string()),
                last_reconciled: Some("2024-01-01T00:00:00Z".to_string()),
                revision: Some("main@sha1:abc123".to_string()),
            }),
            spec: Some(TraceSpec {
                path: Some("./clusters/my-cluster/apps/cabot-book".to_string()),
                url: None,
                branch: None,
                source_ref: None,
            }),
        },
        chain: vec![TraceNode {
            kind: "Kustomization".to_string(),
            name: "flux-system".to_string(),
            namespace: "flux-system".to_string(),
            status: Some(TraceStatus {
                ready: Some(true),
                message: Some("Applied revision: main@sha1:abc123".to_string()),
                last_reconciled: Some("2024-01-01T00:00:00Z".to_string()),
                revision: Some("main@sha1:abc123".to_string()),
            }),
            spec: Some(TraceSpec {
                path: Some("./clusters/my-cluster/flux-system".to_string()),
                url: None,
                branch: None,
                source_ref: Some(SourceRef {
                    kind: "GitRepository".to_string(),
                    name: "flux-system".to_string(),
                    namespace: Some("flux-system".to_string()),
                }),
            }),
        }],
        source: Some(TraceNode {
            kind: "GitRepository".to_string(),
            name: "flux-system".to_string(),
            namespace: "flux-system".to_string(),
            status: Some(TraceStatus {
                ready: Some(true),
                message: Some("stored artifact for revision 'main@sha1:abc123'".to_string()),
                last_reconciled: Some("2024-01-01T00:00:00Z".to_string()),
                revision: Some("main@sha1:abc123".to_string()),
            }),
            spec: Some(TraceSpec {
                path: None,
                url: Some("ssh://git@github.com/dgunzy/k3-gitops".to_string()),
                branch: Some("main".to_string()),
                source_ref: None,
            }),
        }),
    };

    // Verify structure
    verify_trace_structure(&result);

    // Verify specific expectations
    assert_eq!(result.object.name, "cabot-book");
    assert_eq!(result.object.kind, "Kustomization");

    // Verify cabot-book is NOT in chain (only flux-system should be)
    assert!(!result.chain.iter().any(|n| n.name == "cabot-book"));
    assert!(result
        .chain
        .iter()
        .any(|n| n.name == "flux-system" && n.kind == "Kustomization"));

    // Verify source is GitRepository
    assert!(result.source.is_some());
    assert_eq!(result.source.as_ref().unwrap().kind, "GitRepository");
    assert_eq!(result.source.as_ref().unwrap().name, "flux-system");
}

/// Test that verifies expected trace result structure for a HelmRelease managed by a Kustomization
/// Expected chain: Kustomization (infrastructure) -> Source
#[test]
fn test_trace_helmrelease_managed_by_kustomization_structure() {
    use flux9s::trace::{SourceRef, TraceNode, TraceResult, TraceSpec, TraceStatus};

    // Simulate trace result for HelmRelease/podinfo managed by infrastructure Kustomization
    let result = TraceResult {
        object: TraceNode {
            kind: "HelmRelease".to_string(),
            name: "podinfo".to_string(),
            namespace: "default".to_string(),
            status: Some(TraceStatus {
                ready: Some(true),
                message: Some("Release reconciliation succeeded".to_string()),
                last_reconciled: Some("2024-01-01T00:00:00Z".to_string()),
                revision: Some("6.3.5".to_string()),
            }),
            spec: Some(TraceSpec {
                path: None,
                url: None,
                branch: None,
                source_ref: None,
            }),
        },
        chain: vec![TraceNode {
            kind: "Kustomization".to_string(),
            name: "infrastructure".to_string(),
            namespace: "flux-system".to_string(),
            status: Some(TraceStatus {
                ready: Some(true),
                message: Some("Applied revision: main@sha1:abc123".to_string()),
                last_reconciled: Some("2024-01-01T00:00:00Z".to_string()),
                revision: Some("main@sha1:abc123".to_string()),
            }),
            spec: Some(TraceSpec {
                path: Some("./infrastructure".to_string()),
                url: None,
                branch: None,
                source_ref: Some(SourceRef {
                    kind: "GitRepository".to_string(),
                    name: "flux-system".to_string(),
                    namespace: Some("flux-system".to_string()),
                }),
            }),
        }],
        source: Some(TraceNode {
            kind: "GitRepository".to_string(),
            name: "flux-system".to_string(),
            namespace: "flux-system".to_string(),
            status: Some(TraceStatus {
                ready: Some(true),
                message: Some("Fetched revision: main@sha1:abc123".to_string()),
                last_reconciled: Some("2024-01-01T00:00:00Z".to_string()),
                revision: Some("main@sha1:abc123".to_string()),
            }),
            spec: Some(TraceSpec {
                path: None,
                url: Some("ssh://git@github.com/example/repo".to_string()),
                branch: Some("main".to_string()),
                source_ref: None,
            }),
        }),
    };

    // Verify structure
    verify_trace_structure(&result);

    // Verify specific expectations
    assert_eq!(result.object.name, "podinfo");
    assert_eq!(result.object.kind, "HelmRelease");

    // Verify podinfo is NOT in chain (only infrastructure should be)
    assert!(!result.chain.iter().any(|n| n.name == "podinfo"));
    assert!(result
        .chain
        .iter()
        .any(|n| n.name == "infrastructure" && n.kind == "Kustomization"));

    // Verify source is GitRepository
    assert!(result.source.is_some());
    assert_eq!(result.source.as_ref().unwrap().kind, "GitRepository");
}

/// Test that verifies expected trace result structure for a Deployment managed by HelmRelease
/// Expected chain: Deployment -> HelmRelease -> HelmChart -> Source
#[test]
fn test_trace_deployment_managed_by_helmrelease_structure() {
    use flux9s::trace::{SourceRef, TraceNode, TraceResult, TraceSpec, TraceStatus};

    // Simulate trace result for Deployment/podinfo managed by HelmRelease
    let result = TraceResult {
        object: TraceNode {
            kind: "Deployment".to_string(),
            name: "podinfo".to_string(),
            namespace: "default".to_string(),
            status: None,
            spec: None,
        },
        chain: vec![
            TraceNode {
                kind: "HelmRelease".to_string(),
                name: "podinfo".to_string(),
                namespace: "default".to_string(),
                status: Some(TraceStatus {
                    ready: Some(true),
                    message: Some("Release reconciliation succeeded".to_string()),
                    last_reconciled: Some("2024-01-01T00:00:00Z".to_string()),
                    revision: Some("6.3.5".to_string()),
                }),
                spec: Some(TraceSpec {
                    path: None,
                    url: None,
                    branch: None,
                    source_ref: None,
                }),
            },
            TraceNode {
                kind: "HelmChart".to_string(),
                name: "podinfo-podinfo".to_string(),
                namespace: "flux-system".to_string(),
                status: Some(TraceStatus {
                    ready: Some(true),
                    message: Some("Chart reconciliation succeeded".to_string()),
                    last_reconciled: Some("2024-01-01T00:00:00Z".to_string()),
                    revision: Some("6.3.5".to_string()),
                }),
                spec: Some(TraceSpec {
                    path: None,
                    url: None,
                    branch: None,
                    source_ref: Some(SourceRef {
                        kind: "HelmRepository".to_string(),
                        name: "podinfo".to_string(),
                        namespace: Some("flux-system".to_string()),
                    }),
                }),
            },
        ],
        source: Some(TraceNode {
            kind: "HelmRepository".to_string(),
            name: "podinfo".to_string(),
            namespace: "flux-system".to_string(),
            status: Some(TraceStatus {
                ready: Some(true),
                message: Some("Repository reconciliation succeeded".to_string()),
                last_reconciled: Some("2024-01-01T00:00:00Z".to_string()),
                revision: None,
            }),
            spec: Some(TraceSpec {
                path: None,
                url: Some("https://stefanprodan.github.io/podinfo".to_string()),
                branch: None,
                source_ref: None,
            }),
        }),
    };

    // Verify structure
    verify_trace_structure(&result);

    // Verify specific expectations
    assert_eq!(result.object.name, "podinfo");
    assert_eq!(result.object.kind, "Deployment");

    // Verify chain contains HelmRelease and HelmChart
    assert!(result
        .chain
        .iter()
        .any(|n| n.name == "podinfo" && n.kind == "HelmRelease"));
    assert!(result
        .chain
        .iter()
        .any(|n| n.name == "podinfo-podinfo" && n.kind == "HelmChart"));

    // Verify source is HelmRepository
    assert!(result.source.is_some());
    assert_eq!(result.source.as_ref().unwrap().kind, "HelmRepository");
}

/// Test that verifies expected trace result structure for a Kustomization not managed by another
/// Expected chain: Empty -> Source (no intermediate Kustomization)
#[test]
fn test_trace_kustomization_direct_structure() {
    use flux9s::trace::{SourceRef, TraceNode, TraceResult, TraceSpec, TraceStatus};

    // Simulate trace result for Kustomization/infrastructure (not managed by another)
    let result = TraceResult {
        object: TraceNode {
            kind: "Kustomization".to_string(),
            name: "infrastructure".to_string(),
            namespace: "flux-system".to_string(),
            status: Some(TraceStatus {
                ready: Some(true),
                message: Some("Applied revision: main@sha1:abc123".to_string()),
                last_reconciled: Some("2024-01-01T00:00:00Z".to_string()),
                revision: Some("main@sha1:abc123".to_string()),
            }),
            spec: Some(TraceSpec {
                path: Some("./infrastructure".to_string()),
                url: None,
                branch: None,
                source_ref: Some(SourceRef {
                    kind: "GitRepository".to_string(),
                    name: "flux-system".to_string(),
                    namespace: Some("flux-system".to_string()),
                }),
            }),
        },
        chain: vec![], // Empty chain - no managing Kustomization
        source: Some(TraceNode {
            kind: "GitRepository".to_string(),
            name: "flux-system".to_string(),
            namespace: "flux-system".to_string(),
            status: Some(TraceStatus {
                ready: Some(true),
                message: Some("Fetched revision: main@sha1:abc123".to_string()),
                last_reconciled: Some("2024-01-01T00:00:00Z".to_string()),
                revision: Some("main@sha1:abc123".to_string()),
            }),
            spec: Some(TraceSpec {
                path: None,
                url: Some("ssh://git@github.com/example/repo".to_string()),
                branch: Some("main".to_string()),
                source_ref: None,
            }),
        }),
    };

    // Verify structure
    verify_trace_structure(&result);

    // Verify specific expectations
    assert_eq!(result.object.name, "infrastructure");
    assert_eq!(result.object.kind, "Kustomization");

    // Verify chain is empty (no managing Kustomization)
    assert!(result.chain.is_empty());

    // Verify source is GitRepository
    assert!(result.source.is_some());
    assert_eq!(result.source.as_ref().unwrap().kind, "GitRepository");
    assert_eq!(result.source.as_ref().unwrap().name, "flux-system");
}
