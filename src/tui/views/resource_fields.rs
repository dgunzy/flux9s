//! Helper functions for extracting resource-specific fields from JSON objects

use crate::models::FluxResourceKind;
use serde_json::Value;

/// Extract resource-specific display fields from a JSON object
pub fn extract_resource_specific_fields(
    resource_type: &str,
    obj: &Value,
) -> HashMap<String, String> {
    let mut fields = HashMap::new();

    if let Some(spec) = obj.get("spec").and_then(|s| s.as_object()) {
        match FluxResourceKind::parse_optional(resource_type) {
            Some(FluxResourceKind::GitRepository) | Some(FluxResourceKind::HelmRepository) => {
                if let Some(url) = spec.get("url").and_then(|u| u.as_str()) {
                    fields.insert("URL".to_string(), url.to_string());
                }
                if let Some(branch) = spec.get("branch").and_then(|b| b.as_str()) {
                    fields.insert("BRANCH".to_string(), branch.to_string());
                }
            }
            Some(FluxResourceKind::OCIRepository) => {
                if let Some(semver) = spec
                    .get("ref")
                    .and_then(|s| s.get("semver"))
                    .and_then(|se| se.as_str())
                {
                    fields.insert("SEMVER".to_string(), semver.to_string());
                }
            }
            Some(FluxResourceKind::Kustomization) => {
                if let Some(path) = spec.get("path").and_then(|p| p.as_str()) {
                    fields.insert("PATH".to_string(), path.to_string());
                }
                if let Some(prune) = spec.get("prune").and_then(|p| p.as_bool()) {
                    fields.insert(
                        "PRUNE".to_string(),
                        if prune { "True" } else { "False" }.to_string(),
                    );
                }
            }
            Some(FluxResourceKind::HelmRelease) => {
                if let Some(chart) = spec
                    .get("chart")
                    .and_then(|c| c.get("spec"))
                    .and_then(|cs| cs.get("chart"))
                    .and_then(|ch| ch.as_str())
                {
                    fields.insert("CHART".to_string(), chart.to_string());
                }
                if let Some(version) = spec
                    .get("chart")
                    .and_then(|c| c.get("spec"))
                    .and_then(|cs| cs.get("version"))
                    .and_then(|v| v.as_str())
                {
                    fields.insert("VERSION".to_string(), version.to_string());
                }
            }
            Some(FluxResourceKind::HelmChart) => {
                if let Some(chart) = spec.get("chart").and_then(|c| c.as_str()) {
                    fields.insert("CHART".to_string(), chart.to_string());
                }
                if let Some(version) = spec.get("version").and_then(|v| v.as_str()) {
                    fields.insert("VERSION".to_string(), version.to_string());
                }
                if let Some(source_ref) = spec.get("sourceRef") {
                    if let Some(name) = source_ref.get("name").and_then(|n| n.as_str()) {
                        fields.insert("SOURCE".to_string(), name.to_string());
                    }
                }
            }
            Some(FluxResourceKind::ImageRepository) => {
                if let Some(image) = spec.get("image").and_then(|i| i.as_str()) {
                    fields.insert("IMAGE".to_string(), image.to_string());
                }
            }
            Some(FluxResourceKind::ImagePolicy) => {
                if let Some(image_ref) = spec
                    .get("imageRepositoryRef")
                    .and_then(|ir| ir.get("name"))
                    .and_then(|n| n.as_str())
                {
                    fields.insert("IMAGE".to_string(), image_ref.to_string());
                }
            }
            Some(FluxResourceKind::ImageUpdateAutomation) => {
                if let Some(image_ref) = spec
                    .get("sourceRef")
                    .and_then(|sr| sr.get("name"))
                    .and_then(|n| n.as_str())
                {
                    fields.insert("IMAGE".to_string(), image_ref.to_string());
                }
                if let Some(branch) = spec
                    .get("git")
                    .and_then(|g| g.get("checkout"))
                    .and_then(|c| c.get("ref"))
                    .and_then(|r| r.get("branch"))
                    .and_then(|b| b.as_str())
                {
                    fields.insert("BRANCH".to_string(), branch.to_string());
                }
            }
            _ => {}
        }
    }

    // Extract status fields
    if let Some(status) = obj.get("status").and_then(|s| s.as_object()) {
        if FluxResourceKind::parse_optional(resource_type) == Some(FluxResourceKind::HelmRelease) {
            if let Some(helm_chart) = status.get("helmChart").and_then(|hc| hc.as_str()) {
                fields.insert("CHART".to_string(), helm_chart.to_string());
            }
            if let Some(release_status) = status
                .get("conditions")
                .and_then(|c| c.as_array())
                .and_then(|arr| {
                    arr.iter()
                        .find(|c| c.get("type").and_then(|t| t.as_str()) == Some("Ready"))
                })
                .and_then(|c| c.get("status"))
                .and_then(|s| s.as_str())
            {
                fields.insert("STATUS".to_string(), release_status.to_string());
            }
        }
        if FluxResourceKind::parse_optional(resource_type) == Some(FluxResourceKind::OCIRepository)
        {
            if let Some(digest) = status
                .get("artifact")
                .and_then(|a| a.get("digest"))
                .and_then(|d| d.as_str())
            {
                fields.insert("DIGEST".to_string(), digest.to_string());
            }
            if let Some(revision) = status
                .get("artifact")
                .and_then(|a| a.get("revision"))
                .and_then(|r| r.as_str())
            {
                fields.insert("REVISION".to_string(), revision.to_string());
            }
        }
    }

    fields
}

/// Get column headers for a resource type
pub fn get_resource_type_columns(resource_type: &str) -> Vec<&'static str> {
    match FluxResourceKind::parse_optional(resource_type) {
        Some(FluxResourceKind::GitRepository) => vec![
            "STATUS",
            "NAMESPACE",
            "NAME",
            "URL",
            "BRANCH",
            "REVISION",
            "SUSPENDED",
            "READY",
        ],
        Some(FluxResourceKind::OCIRepository) => {
            vec![
                "STATUS",
                "NAMESPACE",
                "NAME",
                "SEMVER",
                "DIGEST",
                "REVISION",
            ]
        }
        Some(FluxResourceKind::HelmRepository) => vec![
            "STATUS",
            "NAMESPACE",
            "NAME",
            "URL",
            "REVISION",
            "SUSPENDED",
            "READY",
        ],
        Some(FluxResourceKind::Kustomization) => vec![
            "STATUS",
            "NAMESPACE",
            "NAME",
            "PATH",
            "REVISION",
            "PRUNE",
            "SUSPENDED",
            "READY",
        ],
        Some(FluxResourceKind::HelmRelease) => vec![
            "STATUS",
            "NAMESPACE",
            "NAME",
            "CHART",
            "VERSION",
            "REVISION",
            "SUSPENDED",
            "READY",
        ],
        Some(FluxResourceKind::HelmChart) => vec![
            "STATUS",
            "NAMESPACE",
            "NAME",
            "CHART",
            "VERSION",
            "SOURCE",
            "SUSPENDED",
            "READY",
        ],
        Some(FluxResourceKind::ImageRepository) => {
            vec!["STATUS", "NAMESPACE", "NAME", "IMAGE", "SUSPENDED", "READY"]
        }
        Some(FluxResourceKind::ImagePolicy) => {
            vec!["STATUS", "NAMESPACE", "NAME", "IMAGE", "SUSPENDED", "READY"]
        }
        Some(FluxResourceKind::ImageUpdateAutomation) => vec![
            "STATUS",
            "NAMESPACE",
            "NAME",
            "IMAGE",
            "BRANCH",
            "SUSPENDED",
            "READY",
        ],
        _ => vec![
            "STATUS",
            "NAMESPACE",
            "NAME",
            "TYPE",
            "SUSPENDED",
            "READY",
            "MESSAGE",
        ],
    }
}

use std::collections::HashMap;
