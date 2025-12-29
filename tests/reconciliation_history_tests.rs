//! Tests for reconciliation history functionality

use serde_json::json;

#[test]
fn test_extract_reconciliation_info_with_history() {
    // Test extracting reconciliation info from a resource with status.history
    let obj = json!({
        "status": {
            "lastReconciledAt": "2024-01-01T12:00:00Z",
            "lastAppliedRevision": "abc123",
            "conditions": [
                {
                    "type": "Ready",
                    "status": "True",
                    "lastTransitionTime": "2024-01-01T12:00:00Z",
                    "message": "Reconciliation succeeded"
                }
            ],
            "history": [
                {
                    "digest": "sha256:abc123",
                    "firstReconciled": "2024-01-01T10:00:00Z",
                    "lastReconciled": "2024-01-01T12:00:00Z",
                    "lastReconciledDuration": "5s",
                    "lastReconciledStatus": "ReconciliationSucceeded",
                    "totalReconciliations": 10
                }
            ]
        }
    });

    // Check that history exists
    let history = obj
        .get("status")
        .and_then(|s| s.get("history"))
        .and_then(|h| h.as_array());

    assert!(history.is_some());
    assert_eq!(history.unwrap().len(), 1);
}

#[test]
fn test_extract_reconciliation_info_without_history() {
    // Test resource without history field
    let obj = json!({
        "status": {
            "lastReconciledAt": "2024-01-01T12:00:00Z",
            "conditions": [
                {
                    "type": "Ready",
                    "status": "True",
                    "lastTransitionTime": "2024-01-01T12:00:00Z"
                }
            ]
        }
    });

    let history = obj
        .get("status")
        .and_then(|s| s.get("history"))
        .and_then(|h| h.as_array());

    assert!(history.is_none());
}

#[test]
fn test_extract_reconciliation_info_empty_history() {
    // Test resource with empty history array
    let obj = json!({
        "status": {
            "history": []
        }
    });

    let history = obj
        .get("status")
        .and_then(|s| s.get("history"))
        .and_then(|h| h.as_array())
        .map(|arr| !arr.is_empty())
        .unwrap_or(false);

    assert!(!history);
}

// Note: Internal state structures are tested through integration tests
// These unit tests focus on JSON extraction and parsing

#[test]
fn test_history_extraction_from_fluxinstance() {
    // Test extracting history from FluxInstance status
    let obj = json!({
        "status": {
            "history": [
                {
                    "digest": "sha256:abc123",
                    "firstReconciled": "2024-01-01T10:00:00Z",
                    "lastReconciled": "2024-01-01T12:00:00Z",
                    "lastReconciledDuration": "5.2s",
                    "lastReconciledStatus": "ReconciliationSucceeded",
                    "metadata": {
                        "flux": "v2.7.5"
                    },
                    "totalReconciliations": 10
                },
                {
                    "digest": "sha256:def456",
                    "firstReconciled": "2024-01-01T08:00:00Z",
                    "lastReconciled": "2024-01-01T10:00:00Z",
                    "lastReconciledDuration": "4.8s",
                    "lastReconciledStatus": "ReconciliationSucceeded",
                    "totalReconciliations": 5
                }
            ]
        }
    });

    let history = obj
        .get("status")
        .and_then(|s| s.get("history"))
        .and_then(|h| h.as_array());

    assert!(history.is_some());
    let history_arr = history.unwrap();
    assert_eq!(history_arr.len(), 2);

    // Check first entry
    let first = &history_arr[0];
    assert_eq!(
        first.get("digest").and_then(|v| v.as_str()),
        Some("sha256:abc123")
    );
    assert_eq!(
        first.get("totalReconciliations").and_then(|v| v.as_i64()),
        Some(10)
    );
}

#[test]
fn test_history_extraction_from_resourceset() {
    // Test extracting history from ResourceSet status
    let obj = json!({
        "status": {
            "history": [
                {
                    "digest": "sha256:xyz789",
                    "firstReconciled": "2024-01-01T14:00:00Z",
                    "lastReconciled": "2024-01-01T15:00:00Z",
                    "lastReconciledDuration": "3.5s",
                    "lastReconciledStatus": "ReconciliationSucceeded",
                    "totalReconciliations": 1
                }
            ]
        }
    });

    let history = obj
        .get("status")
        .and_then(|s| s.get("history"))
        .and_then(|h| h.as_array());

    assert!(history.is_some());
    let history_arr = history.unwrap();
    assert_eq!(history_arr.len(), 1);
}

#[test]
fn test_history_extraction_from_kustomization() {
    // Test extracting history from Kustomization status
    let obj = json!({
        "status": {
            "history": [
                {
                    "digest": "sha256:kustom123",
                    "firstReconciled": "2024-01-01T16:00:00Z",
                    "lastReconciled": "2024-01-01T17:00:00Z",
                    "lastReconciledDuration": "2.1s",
                    "lastReconciledStatus": "ReconciliationSucceeded",
                    "totalReconciliations": 3
                }
            ]
        }
    });

    let history = obj
        .get("status")
        .and_then(|s| s.get("history"))
        .and_then(|h| h.as_array());

    assert!(history.is_some());
}

#[test]
fn test_history_extraction_from_helmrelease() {
    // Test extracting history from HelmRelease status
    let obj = json!({
        "status": {
            "history": [
                {
                    "digest": "sha256:helm456",
                    "firstReconciled": "2024-01-01T18:00:00Z",
                    "lastReconciled": "2024-01-01T19:00:00Z",
                    "lastReconciledDuration": "6.3s",
                    "lastReconciledStatus": "ReconciliationSucceeded",
                    "totalReconciliations": 7
                }
            ]
        }
    });

    let history = obj
        .get("status")
        .and_then(|s| s.get("history"))
        .and_then(|h| h.as_array());

    assert!(history.is_some());
}
