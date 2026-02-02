//! CRD compatibility tests
//!
//! These tests ensure that when Flux CRDs are updated, the code remains compatible.
//! They test status field extraction, resource type detection, and model compatibility.

use flux9s::models::FluxResourceKind;
use flux9s::{extract_status_fields, resource_key};
use serde_json::json;

#[test]
fn test_extract_status_fields_suspended_true() {
    let obj = json!({
        "spec": {
            "suspend": true
        },
        "status": {
            "conditions": [
                {
                    "type": "Ready",
                    "status": "True",
                    "message": "Reconciliation succeeded"
                }
            ]
        }
    });

    let (suspended, ready, message, revision) = extract_status_fields(&obj);
    assert_eq!(suspended, Some(true));
    assert_eq!(ready, Some(true));
    assert_eq!(message, Some("Reconciliation succeeded".to_string()));
    assert_eq!(revision, None);
}

#[test]
fn test_extract_status_fields_suspended_false() {
    let obj = json!({
        "spec": {
            "suspend": false
        },
        "status": {
            "conditions": [
                {
                    "type": "Ready",
                    "status": "False",
                    "message": "Reconciliation failed"
                }
            ]
        }
    });

    let (suspended, ready, message, revision) = extract_status_fields(&obj);
    assert_eq!(suspended, Some(false));
    assert_eq!(ready, Some(false));
    assert_eq!(message, Some("Reconciliation failed".to_string()));
    assert_eq!(revision, None);
}

#[test]
fn test_extract_status_fields_no_suspend_field() {
    // When suspend field doesn't exist, should default to false
    let obj = json!({
        "spec": {},
        "status": {
            "conditions": [
                {
                    "type": "Ready",
                    "status": "True",
                    "message": "Ready"
                }
            ]
        }
    });

    let (suspended, ready, message, revision) = extract_status_fields(&obj);
    assert_eq!(suspended, Some(false)); // Should default to false
    assert_eq!(ready, Some(true));
    assert_eq!(message, Some("Ready".to_string()));
    assert_eq!(revision, None);
}

#[test]
fn test_extract_status_fields_no_spec() {
    // When spec doesn't exist, should default to false
    let obj = json!({
        "status": {
            "conditions": [
                {
                    "type": "Ready",
                    "status": "True",
                    "message": "Ready"
                }
            ]
        }
    });

    let (suspended, ready, message, revision) = extract_status_fields(&obj);
    assert_eq!(suspended, Some(false)); // Should default to false
    assert_eq!(ready, Some(true));
    assert_eq!(message, Some("Ready".to_string()));
    assert_eq!(revision, None);
}

#[test]
fn test_extract_status_fields_with_revision() {
    let obj = json!({
        "spec": {
            "suspend": false
        },
        "status": {
            "lastAppliedRevision": "main@sha1:abc123",
            "conditions": [
                {
                    "type": "Ready",
                    "status": "True",
                    "message": "Applied revision"
                }
            ]
        }
    });

    let (suspended, ready, message, revision) = extract_status_fields(&obj);
    assert_eq!(suspended, Some(false));
    assert_eq!(ready, Some(true));
    assert_eq!(message, Some("Applied revision".to_string()));
    assert_eq!(revision, Some("main@sha1:abc123".to_string()));
}

#[test]
fn test_extract_status_fields_with_last_handled_reconcile_at() {
    // Some resources use lastHandledReconcileAt instead of lastAppliedRevision
    let obj = json!({
        "spec": {
            "suspend": false
        },
        "status": {
            "lastHandledReconcileAt": "2024-01-01T00:00:00Z",
            "conditions": [
                {
                    "type": "Ready",
                    "status": "True",
                    "message": "Reconciled"
                }
            ]
        }
    });

    let (suspended, ready, message, revision) = extract_status_fields(&obj);
    assert_eq!(suspended, Some(false));
    assert_eq!(ready, Some(true));
    assert_eq!(message, Some("Reconciled".to_string()));
    assert_eq!(revision, Some("2024-01-01T00:00:00Z".to_string()));
}

#[test]
fn test_extract_status_fields_no_ready_condition() {
    let obj = json!({
        "spec": {
            "suspend": false
        },
        "status": {
            "conditions": [
                {
                    "type": "Reconciling",
                    "status": "True",
                    "message": "Reconciling"
                }
            ]
        }
    });

    let (suspended, ready, message, revision) = extract_status_fields(&obj);
    assert_eq!(suspended, Some(false));
    assert_eq!(ready, None); // No Ready condition
    assert_eq!(message, None); // No message from Ready condition
    assert_eq!(revision, None);
}

#[test]
fn test_extract_status_fields_empty_status() {
    let obj = json!({
        "spec": {
            "suspend": false
        },
        "status": {}
    });

    let (suspended, ready, message, revision) = extract_status_fields(&obj);
    assert_eq!(suspended, Some(false));
    assert_eq!(ready, None);
    assert_eq!(message, None);
    assert_eq!(revision, None);
}

#[test]
fn test_extract_status_fields_no_status() {
    let obj = json!({
        "spec": {
            "suspend": false
        }
    });

    let (suspended, ready, message, revision) = extract_status_fields(&obj);
    assert_eq!(suspended, Some(false));
    assert_eq!(ready, None);
    assert_eq!(message, None);
    assert_eq!(revision, None);
}

#[test]
fn test_extract_status_fields_kustomization_example() {
    // Real-world Kustomization example
    let obj = json!({
        "apiVersion": "kustomize.toolkit.fluxcd.io/v1",
        "kind": "Kustomization",
        "metadata": {
            "name": "test-ks",
            "namespace": "flux-system"
        },
        "spec": {
            "suspend": false,
            "path": "./clusters/prod",
            "sourceRef": {
                "kind": "GitRepository",
                "name": "flux-system"
            }
        },
        "status": {
            "lastAppliedRevision": "main@sha1:bffdf10f",
            "conditions": [
                {
                    "type": "Ready",
                    "status": "True",
                    "message": "Applied revision: main@sha1:bffdf10f",
                    "reason": "ReconciliationSucceeded"
                }
            ]
        }
    });

    let (suspended, ready, message, revision) = extract_status_fields(&obj);
    assert_eq!(suspended, Some(false));
    assert_eq!(ready, Some(true));
    assert_eq!(
        message,
        Some("Applied revision: main@sha1:bffdf10f".to_string())
    );
    assert_eq!(revision, Some("main@sha1:bffdf10f".to_string()));
}

#[test]
fn test_extract_status_fields_gitrepository_example() {
    // Real-world GitRepository example
    let obj = json!({
        "apiVersion": "source.toolkit.fluxcd.io/v1",
        "kind": "GitRepository",
        "metadata": {
            "name": "flux-system",
            "namespace": "flux-system"
        },
        "spec": {
            "suspend": false,
            "url": "https://github.com/fluxcd/flux2",
            "ref": {
                "branch": "main"
            }
        },
        "status": {
            "lastHandledReconcileAt": "2024-01-01T00:00:00Z",
            "conditions": [
                {
                    "type": "Ready",
                    "status": "True",
                    "message": "stored artifact for revision 'main@sha1:bffdf10f'"
                }
            ]
        }
    });

    let (suspended, ready, message, revision) = extract_status_fields(&obj);
    assert_eq!(suspended, Some(false));
    assert_eq!(ready, Some(true));
    assert_eq!(
        message,
        Some("stored artifact for revision 'main@sha1:bffdf10f'".to_string())
    );
    assert_eq!(revision, Some("2024-01-01T00:00:00Z".to_string()));
}

#[test]
fn test_extract_status_fields_helmrelease_example() {
    // Real-world HelmRelease example
    let obj = json!({
        "apiVersion": "helm.toolkit.fluxcd.io/v2beta2",
        "kind": "HelmRelease",
        "metadata": {
            "name": "cert-manager",
            "namespace": "cert-manager"
        },
        "spec": {
            "suspend": false,
            "chart": {
                "spec": {
                    "chart": "cert-manager",
                    "version": "v1.13.6"
                }
            }
        },
        "status": {
            "lastAppliedRevision": "v1.13.6",
            "conditions": [
                {
                    "type": "Ready",
                    "status": "True",
                    "message": "Helm install succeeded for release cert-manager/cert-manager.v1"
                }
            ]
        }
    });

    let (suspended, ready, message, revision) = extract_status_fields(&obj);
    assert_eq!(suspended, Some(false));
    assert_eq!(ready, Some(true));
    assert_eq!(
        message,
        Some("Helm install succeeded for release cert-manager/cert-manager.v1".to_string())
    );
    assert_eq!(revision, Some("v1.13.6".to_string()));
}

#[test]
fn test_resource_key_format() {
    // Ensure resource key format is consistent
    let key = resource_key("default", "my-resource", "Kustomization");
    assert_eq!(key, "Kustomization:default:my-resource");

    // Test with different formats
    let key2 = resource_key("flux-system", "flux-system", "GitRepository");
    assert_eq!(key2, "GitRepository:flux-system:flux-system");

    // Test that keys are unique
    let key3 = resource_key("default", "my-resource", "GitRepository");
    assert_ne!(key, key3); // Different resource types should produce different keys
}

#[test]
fn test_stateless_resources_identified() {
    // Alert and Provider are stateless (no status.conditions in CRD)
    assert!(FluxResourceKind::Alert.is_stateless());
    assert!(FluxResourceKind::Provider.is_stateless());

    // All other resources should not be stateless
    for kind in FluxResourceKind::all() {
        if !matches!(kind, FluxResourceKind::Alert | FluxResourceKind::Provider) {
            assert!(!kind.is_stateless(), "{} should not be stateless", kind);
        }
    }
}

#[test]
fn test_stateless_resource_override_logic() {
    // Simulate what happens for a stateless resource with no status
    let obj = json!({
        "spec": {
            "eventSources": [{"kind": "GitRepository", "name": "flux-system"}]
        }
    });
    let (_suspended, ready, _message, _revision) = extract_status_fields(&obj);
    assert_eq!(ready, None); // extract_status_fields returns None

    // The override in the WatchEvent handler would set ready = Some(true)
    let kind = FluxResourceKind::parse_optional("Alert").unwrap();
    assert!(kind.is_stateless());
    let overridden_ready = if ready.is_none() && kind.is_stateless() {
        Some(true)
    } else {
        ready
    };
    assert_eq!(overridden_ready, Some(true));
}
