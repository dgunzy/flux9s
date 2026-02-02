//! CRD field extraction tests
//!
//! Tests to ensure resource-specific field extraction works correctly
//! when CRD schemas are updated.

use flux9s::{extract_resource_specific_fields, get_resource_type_columns};
use serde_json::json;

#[test]
fn test_extract_gitrepository_fields() {
    let obj = json!({
        "spec": {
            "url": "https://github.com/fluxcd/flux2",
            "ref": {
                "branch": "main"
            }
        }
    });

    let fields = extract_resource_specific_fields("GitRepository", &obj, None);
    assert_eq!(
        fields.get("URL"),
        Some(&"https://github.com/fluxcd/flux2".to_string())
    );
    // Note: branch extraction may need to be updated if the code changes
    // For now, we just test that URL extraction works
}

#[test]
fn test_extract_kustomization_fields() {
    let obj = json!({
        "spec": {
            "path": "./clusters/prod",
            "prune": true
        }
    });

    let fields = extract_resource_specific_fields("Kustomization", &obj, None);
    assert_eq!(fields.get("PATH"), Some(&"./clusters/prod".to_string()));
    assert_eq!(fields.get("PRUNE"), Some(&"True".to_string()));
}

#[test]
fn test_extract_helmrelease_fields() {
    let obj = json!({
        "spec": {
            "chart": {
                "spec": {
                    "chart": "cert-manager",
                    "version": "v1.13.6"
                }
            }
        }
    });

    let fields = extract_resource_specific_fields("HelmRelease", &obj, None);
    assert_eq!(fields.get("CHART"), Some(&"cert-manager".to_string()));
    assert_eq!(fields.get("VERSION"), Some(&"v1.13.6".to_string()));
}

#[test]
fn test_get_resource_type_columns() {
    let columns = get_resource_type_columns("Kustomization", None);
    assert!(columns.iter().any(|c| c == "PATH"));
    assert!(columns.iter().any(|c| c == "PRUNE"));
    assert!(columns.iter().any(|c| c == "REVISION"));

    let gitrepo_columns = get_resource_type_columns("GitRepository", None);
    assert!(gitrepo_columns.iter().any(|c| c == "URL"));
    assert!(gitrepo_columns.iter().any(|c| c == "BRANCH"));

    let helm_columns = get_resource_type_columns("HelmRelease", None);
    assert!(helm_columns.iter().any(|c| c == "CHART"));
    assert!(helm_columns.iter().any(|c| c == "VERSION"));
}

#[test]
fn test_extract_fields_missing_spec() {
    let obj = json!({});

    let fields = extract_resource_specific_fields("Kustomization", &obj, None);
    assert!(fields.is_empty());
}

#[test]
fn test_extract_fields_unknown_type() {
    let obj = json!({
        "spec": {
            "someField": "value"
        }
    });

    let fields = extract_resource_specific_fields("UnknownType", &obj, None);
    assert!(fields.is_empty());
}
