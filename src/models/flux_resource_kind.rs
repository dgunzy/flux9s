//! Flux Resource Kind definitions
//!
//! This module provides a centralized enum for all Flux CRD resource kinds.
//! This eliminates hardcoded strings throughout the codebase and provides
//! type safety for resource kind references.

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use serde_json::Value;

// Column and field name constants
pub mod field_names {
    // Common columns
    pub const STATUS: &str = "STATUS";
    pub const NAMESPACE: &str = "NAMESPACE";
    pub const NAME: &str = "NAME";
    pub const TYPE: &str = "TYPE";
    pub const SUSPENDED: &str = "SUSPENDED";
    pub const READY: &str = "READY";
    pub const MESSAGE: &str = "MESSAGE";

    // Resource-specific fields
    pub const URL: &str = "URL";
    pub const BRANCH: &str = "BRANCH";
    pub const REVISION: &str = "REVISION";
    pub const SEMVER: &str = "SEMVER";
    pub const DIGEST: &str = "DIGEST";
    pub const PATH: &str = "PATH";
    pub const PRUNE: &str = "PRUNE";
    pub const CHART: &str = "CHART";
    pub const VERSION: &str = "VERSION";
    pub const SOURCE: &str = "SOURCE";
    pub const IMAGE: &str = "IMAGE";
    pub const TAG: &str = "TAG";
    pub const INTERVAL: &str = "INTERVAL";
    pub const SECRET: &str = "SECRET";
    pub const ENDPOINT: &str = "ENDPOINT";
    pub const PROVIDER: &str = "PROVIDER";
    pub const CHANNEL: &str = "CHANNEL";
    pub const ADDRESS: &str = "ADDRESS";
    pub const WEBHOOK: &str = "WEBHOOK";
    pub const INPUTS: &str = "INPUTS";
}

/// Enumeration of all Flux CRD resource kinds
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FluxResourceKind {
    // Source Controller resources
    GitRepository,
    OCIRepository,
    HelmRepository,
    Bucket,
    HelmChart,
    ExternalArtifact,
    ArtifactGenerator,
    // Kustomize Controller resources
    Kustomization,
    // Helm Controller resources
    HelmRelease,
    // Image Reflector Controller resources
    ImageRepository,
    ImagePolicy,
    // Image Automation Controller resources
    ImageUpdateAutomation,
    // Notification Controller resources
    Alert,
    Provider,
    Receiver,
    // Flux Operator resources
    ResourceSet,
    ResourceSetInputProvider,
    FluxReport,
    FluxInstance,
}

impl FluxResourceKind {
    /// Get the display name as a string
    pub fn as_str(&self) -> &'static str {
        match self {
            FluxResourceKind::GitRepository => "GitRepository",
            FluxResourceKind::OCIRepository => "OCIRepository",
            FluxResourceKind::HelmRepository => "HelmRepository",
            FluxResourceKind::Bucket => "Bucket",
            FluxResourceKind::HelmChart => "HelmChart",
            FluxResourceKind::ExternalArtifact => "ExternalArtifact",
            FluxResourceKind::ArtifactGenerator => "ArtifactGenerator",
            FluxResourceKind::Kustomization => "Kustomization",
            FluxResourceKind::HelmRelease => "HelmRelease",
            FluxResourceKind::ImageRepository => "ImageRepository",
            FluxResourceKind::ImagePolicy => "ImagePolicy",
            FluxResourceKind::ImageUpdateAutomation => "ImageUpdateAutomation",
            FluxResourceKind::Alert => "Alert",
            FluxResourceKind::Provider => "Provider",
            FluxResourceKind::Receiver => "Receiver",
            FluxResourceKind::ResourceSet => "ResourceSet",
            FluxResourceKind::ResourceSetInputProvider => "ResourceSetInputProvider",
            FluxResourceKind::FluxReport => "FluxReport",
            FluxResourceKind::FluxInstance => "FluxInstance",
        }
    }

    /// Try to parse a string into a FluxResourceKind, returning None if invalid
    /// Use this when you want Option<Self> instead of Result<Self, String>
    pub fn parse_optional(s: &str) -> Option<Self> {
        s.parse().ok()
    }

    /// Get all Flux resource kinds
    ///
    /// Returns an array of all FluxResourceKind variants.
    /// This is useful for iterating over all resource types dynamically.
    pub fn all() -> &'static [Self] {
        &[
            FluxResourceKind::GitRepository,
            FluxResourceKind::OCIRepository,
            FluxResourceKind::HelmRepository,
            FluxResourceKind::Bucket,
            FluxResourceKind::HelmChart,
            FluxResourceKind::ExternalArtifact,
            FluxResourceKind::ArtifactGenerator,
            FluxResourceKind::Kustomization,
            FluxResourceKind::HelmRelease,
            FluxResourceKind::ImageRepository,
            FluxResourceKind::ImagePolicy,
            FluxResourceKind::ImageUpdateAutomation,
            FluxResourceKind::Alert,
            FluxResourceKind::Provider,
            FluxResourceKind::Receiver,
            FluxResourceKind::ResourceSet,
            FluxResourceKind::ResourceSetInputProvider,
            FluxResourceKind::FluxReport,
            FluxResourceKind::FluxInstance,
        ]
    }

    /// Try to parse a string (case-insensitive) into a FluxResourceKind
    pub fn from_str_case_insensitive(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "gitrepository" | "gitrepo" | "gitrepositories" => {
                Some(FluxResourceKind::GitRepository)
            }
            "ocirepository" | "oci" | "ocirepositories" => Some(FluxResourceKind::OCIRepository),
            "helmrepository" | "helmrepo" | "helmrepositories" => {
                Some(FluxResourceKind::HelmRepository)
            }
            "bucket" | "buckets" => Some(FluxResourceKind::Bucket),
            "helmchart" | "helmcharts" => Some(FluxResourceKind::HelmChart),
            "externalartifact" | "externalartifacts" | "ea" => {
                Some(FluxResourceKind::ExternalArtifact)
            }
            "artifactgenerator" | "artifactgenerators" | "ag" => {
                Some(FluxResourceKind::ArtifactGenerator)
            }
            "kustomization" | "ks" | "kustomizations" => Some(FluxResourceKind::Kustomization),
            "helmrelease" | "hr" | "helmreleases" => Some(FluxResourceKind::HelmRelease),
            "imagerepository" | "imagerepositories" => Some(FluxResourceKind::ImageRepository),
            "imagepolicy" | "imagepolicies" => Some(FluxResourceKind::ImagePolicy),
            "imageupdateautomation" | "imageupdateautomations" => {
                Some(FluxResourceKind::ImageUpdateAutomation)
            }
            "alert" | "alerts" => Some(FluxResourceKind::Alert),
            "provider" | "providers" => Some(FluxResourceKind::Provider),
            "receiver" | "receivers" => Some(FluxResourceKind::Receiver),
            "resourceset" | "resourcesets" | "rset" => Some(FluxResourceKind::ResourceSet),
            "resourcesetinputprovider" | "resourcesetinputproviders" | "rsip" => {
                Some(FluxResourceKind::ResourceSetInputProvider)
            }
            "fluxreport" | "fluxreports" | "fr" => Some(FluxResourceKind::FluxReport),
            "fluxinstance" | "fluxinstances" | "fi" => Some(FluxResourceKind::FluxInstance),
            _ => None,
        }
    }

    /// Check if this resource type supports the graph view
    ///
    /// Only resources with inventory tracking capabilities support graphs:
    /// - Kustomization
    /// - HelmRelease
    /// - ArtifactGenerator
    /// - FluxInstance
    /// - ResourceSet
    pub fn supports_graph(&self) -> bool {
        matches!(
            self,
            FluxResourceKind::Kustomization
                | FluxResourceKind::HelmRelease
                | FluxResourceKind::ArtifactGenerator
                | FluxResourceKind::FluxInstance
                | FluxResourceKind::ResourceSet
        )
    }

    /// Check if this resource type supports reconciliation history
    ///
    /// Only resources with status.history field support history:
    /// - FluxInstance
    /// - ResourceSet
    /// - Kustomization
    /// - HelmRelease
    pub fn supports_history(&self) -> bool {
        matches!(
            self,
            FluxResourceKind::FluxInstance
                | FluxResourceKind::ResourceSet
                | FluxResourceKind::Kustomization
                | FluxResourceKind::HelmRelease
        )
    }

    /// Check if this resource type is stateless (has no status.conditions in its CRD)
    ///
    /// Stateless resources are treated as always ready since they are
    /// configuration-only resources with no reconciliation status.
    pub fn is_stateless(&self) -> bool {
        matches!(self, FluxResourceKind::Alert | FluxResourceKind::Provider)
    }

    /// Get all resource types that support graph view
    pub fn graph_supported_types() -> &'static [Self] {
        &[
            FluxResourceKind::Kustomization,
            FluxResourceKind::HelmRelease,
            FluxResourceKind::ArtifactGenerator,
            FluxResourceKind::FluxInstance,
            FluxResourceKind::ResourceSet,
        ]
    }

    /// Get all resource types that support reconciliation history
    pub fn history_supported_types() -> &'static [Self] {
        &[
            FluxResourceKind::FluxInstance,
            FluxResourceKind::ResourceSet,
            FluxResourceKind::Kustomization,
            FluxResourceKind::HelmRelease,
        ]
    }

    /// Get column headers for this resource type
    pub fn columns(&self) -> Vec<&'static str> {
        use field_names::*;
        match self {
            FluxResourceKind::GitRepository => vec![
                STATUS, NAMESPACE, NAME, URL, BRANCH, REVISION, SUSPENDED, READY,
            ],
            FluxResourceKind::OCIRepository => {
                vec![STATUS, NAMESPACE, NAME, SEMVER, DIGEST, REVISION]
            }
            FluxResourceKind::HelmRepository => {
                vec![STATUS, NAMESPACE, NAME, URL, REVISION, SUSPENDED, READY]
            }
            FluxResourceKind::Kustomization => {
                vec![
                    STATUS, NAMESPACE, NAME, PATH, REVISION, PRUNE, SUSPENDED, READY,
                ]
            }
            FluxResourceKind::HelmRelease => {
                vec![
                    STATUS, NAMESPACE, NAME, CHART, VERSION, REVISION, SUSPENDED, READY,
                ]
            }
            FluxResourceKind::HelmChart => {
                vec![
                    STATUS, NAMESPACE, NAME, CHART, VERSION, SOURCE, SUSPENDED, READY,
                ]
            }
            FluxResourceKind::ImageRepository => {
                vec![STATUS, NAMESPACE, NAME, IMAGE, SUSPENDED, READY]
            }
            FluxResourceKind::ImagePolicy => {
                vec![STATUS, NAMESPACE, NAME, IMAGE, SUSPENDED, READY]
            }
            FluxResourceKind::ImageUpdateAutomation => {
                vec![STATUS, NAMESPACE, NAME, IMAGE, BRANCH, SUSPENDED, READY]
            }
            FluxResourceKind::Bucket => {
                vec![
                    STATUS, NAMESPACE, NAME, ENDPOINT, PROVIDER, INTERVAL, SUSPENDED, READY,
                ]
            }
            FluxResourceKind::ExternalArtifact => {
                vec![STATUS, NAMESPACE, NAME, SOURCE, TYPE, READY, MESSAGE]
            }
            FluxResourceKind::Alert => {
                vec![STATUS, NAMESPACE, NAME, PROVIDER, SUSPENDED, READY, MESSAGE]
            }
            FluxResourceKind::Provider => {
                vec![
                    STATUS, NAMESPACE, NAME, TYPE, ADDRESS, CHANNEL, SUSPENDED, READY, MESSAGE,
                ]
            }
            FluxResourceKind::Receiver => {
                vec![
                    STATUS, NAMESPACE, NAME, TYPE, WEBHOOK, SUSPENDED, READY, MESSAGE,
                ]
            }
            FluxResourceKind::ResourceSet => {
                vec![STATUS, NAMESPACE, NAME, INPUTS, READY, MESSAGE]
            }
            FluxResourceKind::ResourceSetInputProvider => vec![
                STATUS, NAMESPACE, NAME, TYPE, URL, SECRET, INTERVAL, READY, MESSAGE,
            ],
            FluxResourceKind::FluxInstance => {
                vec![STATUS, NAMESPACE, NAME, VERSION, READY, MESSAGE]
            }
            _ => vec![STATUS, NAMESPACE, NAME, TYPE, SUSPENDED, READY, MESSAGE],
        }
    }

    /// Extract resource-specific display fields from a JSON object
    pub fn extract_fields(&self, obj: &Value) -> HashMap<String, String> {
        use field_names::*;
        let mut fields = HashMap::new();

        if let Some(spec) = obj.get("spec").and_then(|s| s.as_object()) {
            match self {
                FluxResourceKind::GitRepository | FluxResourceKind::HelmRepository => {
                    if let Some(url) = spec.get("url").and_then(|u| u.as_str()) {
                        fields.insert(URL.to_string(), url.to_string());
                    }
                    if let Some(branch) = spec.get("branch").and_then(|b| b.as_str()) {
                        fields.insert(BRANCH.to_string(), branch.to_string());
                    }
                    // Extract INTERVAL (common across types)
                    if let Some(interval) = spec.get("interval").and_then(|i| i.as_str()) {
                        fields.insert(INTERVAL.to_string(), interval.to_string());
                    }
                }
                FluxResourceKind::OCIRepository => {
                    if let Some(semver) = spec
                        .get("ref")
                        .and_then(|s| s.get("semver"))
                        .and_then(|se| se.as_str())
                    {
                        fields.insert(SEMVER.to_string(), semver.to_string());
                    }
                    // Extract TAG (was only in detail.rs)
                    if let Some(tag) = spec
                        .get("ref")
                        .and_then(|r| r.get("tag"))
                        .and_then(|t| t.as_str())
                    {
                        fields.insert(TAG.to_string(), tag.to_string());
                    }
                    // Extract INTERVAL (common across types)
                    if let Some(interval) = spec.get("interval").and_then(|i| i.as_str()) {
                        fields.insert(INTERVAL.to_string(), interval.to_string());
                    }
                }
                FluxResourceKind::Kustomization => {
                    if let Some(path) = spec.get("path").and_then(|p| p.as_str()) {
                        fields.insert(PATH.to_string(), path.to_string());
                    }
                    if let Some(prune) = spec.get("prune").and_then(|p| p.as_bool()) {
                        fields.insert(
                            PRUNE.to_string(),
                            if prune { "True" } else { "False" }.to_string(),
                        );
                    }
                    // Extract INTERVAL (common across types)
                    if let Some(interval) = spec.get("interval").and_then(|i| i.as_str()) {
                        fields.insert(INTERVAL.to_string(), interval.to_string());
                    }
                }
                FluxResourceKind::HelmRelease => {
                    if let Some(chart) = spec
                        .get("chart")
                        .and_then(|c| c.get("spec"))
                        .and_then(|cs| cs.get("chart"))
                        .and_then(|ch| ch.as_str())
                    {
                        fields.insert(CHART.to_string(), chart.to_string());
                    }
                    if let Some(version) = spec
                        .get("chart")
                        .and_then(|c| c.get("spec"))
                        .and_then(|cs| cs.get("version"))
                        .and_then(|v| v.as_str())
                    {
                        fields.insert(VERSION.to_string(), version.to_string());
                    }
                    // Extract INTERVAL (common across types)
                    if let Some(interval) = spec.get("interval").and_then(|i| i.as_str()) {
                        fields.insert(INTERVAL.to_string(), interval.to_string());
                    }
                }
                FluxResourceKind::HelmChart => {
                    if let Some(chart) = spec.get("chart").and_then(|c| c.as_str()) {
                        fields.insert(CHART.to_string(), chart.to_string());
                    }
                    if let Some(version) = spec.get("version").and_then(|v| v.as_str()) {
                        fields.insert(VERSION.to_string(), version.to_string());
                    }
                    if let Some(source_ref) = spec.get("sourceRef")
                        && let Some(name) = source_ref.get("name").and_then(|n| n.as_str())
                    {
                        fields.insert(SOURCE.to_string(), name.to_string());
                    }
                    // Extract INTERVAL (common across types)
                    if let Some(interval) = spec.get("interval").and_then(|i| i.as_str()) {
                        fields.insert(INTERVAL.to_string(), interval.to_string());
                    }
                }
                FluxResourceKind::ImageRepository => {
                    if let Some(image) = spec.get("image").and_then(|i| i.as_str()) {
                        fields.insert(IMAGE.to_string(), image.to_string());
                    }
                    // Extract INTERVAL (common across types)
                    if let Some(interval) = spec.get("interval").and_then(|i| i.as_str()) {
                        fields.insert(INTERVAL.to_string(), interval.to_string());
                    }
                }
                FluxResourceKind::ImagePolicy => {
                    if let Some(image_ref) = spec
                        .get("imageRepositoryRef")
                        .and_then(|ir| ir.get("name"))
                        .and_then(|n| n.as_str())
                    {
                        fields.insert(IMAGE.to_string(), image_ref.to_string());
                    }
                    // Extract INTERVAL (common across types)
                    if let Some(interval) = spec.get("interval").and_then(|i| i.as_str()) {
                        fields.insert(INTERVAL.to_string(), interval.to_string());
                    }
                }
                FluxResourceKind::ImageUpdateAutomation => {
                    if let Some(image_ref) = spec
                        .get("sourceRef")
                        .and_then(|sr| sr.get("name"))
                        .and_then(|n| n.as_str())
                    {
                        fields.insert(IMAGE.to_string(), image_ref.to_string());
                    }
                    if let Some(branch) = spec
                        .get("git")
                        .and_then(|g| g.get("checkout"))
                        .and_then(|c| c.get("ref"))
                        .and_then(|r| r.get("branch"))
                        .and_then(|b| b.as_str())
                    {
                        fields.insert(BRANCH.to_string(), branch.to_string());
                    }
                    // Extract INTERVAL (common across types)
                    if let Some(interval) = spec.get("interval").and_then(|i| i.as_str()) {
                        fields.insert(INTERVAL.to_string(), interval.to_string());
                    }
                }
                FluxResourceKind::Bucket => {
                    if let Some(endpoint) = spec.get("endpoint").and_then(|e| e.as_str()) {
                        fields.insert(ENDPOINT.to_string(), endpoint.to_string());
                    }
                    if let Some(provider) = spec.get("provider").and_then(|p| p.as_str()) {
                        fields.insert(PROVIDER.to_string(), provider.to_string());
                    }
                    if let Some(secret_name) = spec
                        .get("secretRef")
                        .and_then(|s| s.get("name"))
                        .and_then(|n| n.as_str())
                    {
                        fields.insert(SECRET.to_string(), secret_name.to_string());
                    }
                    if let Some(interval) = spec.get("interval").and_then(|i| i.as_str()) {
                        fields.insert(INTERVAL.to_string(), interval.to_string());
                    }
                }
                FluxResourceKind::ExternalArtifact => {
                    if let Some(source_ref) = spec.get("sourceRef") {
                        if let Some(name) = source_ref.get("name").and_then(|n| n.as_str()) {
                            fields.insert(SOURCE.to_string(), name.to_string());
                        }
                        if let Some(kind) = source_ref.get("kind").and_then(|k| k.as_str()) {
                            fields.insert(TYPE.to_string(), kind.to_string());
                        }
                    }
                }
                FluxResourceKind::Alert => {
                    if let Some(provider_name) = spec
                        .get("providerRef")
                        .and_then(|p| p.get("name"))
                        .and_then(|n| n.as_str())
                    {
                        fields.insert(PROVIDER.to_string(), provider_name.to_string());
                    }
                }
                FluxResourceKind::Provider => {
                    if let Some(provider_type) = spec.get("type").and_then(|t| t.as_str()) {
                        fields.insert(TYPE.to_string(), provider_type.to_string());
                    }
                    if let Some(address) = spec.get("address").and_then(|a| a.as_str()) {
                        fields.insert(ADDRESS.to_string(), address.to_string());
                    }
                    if let Some(channel) = spec.get("channel").and_then(|c| c.as_str()) {
                        fields.insert(CHANNEL.to_string(), channel.to_string());
                    }
                    if let Some(secret_name) = spec
                        .get("secretRef")
                        .and_then(|s| s.get("name"))
                        .and_then(|n| n.as_str())
                    {
                        fields.insert(SECRET.to_string(), secret_name.to_string());
                    }
                }
                FluxResourceKind::Receiver => {
                    if let Some(receiver_type) = spec.get("type").and_then(|t| t.as_str()) {
                        fields.insert(TYPE.to_string(), receiver_type.to_string());
                    }
                    if let Some(secret_name) = spec
                        .get("secretRef")
                        .and_then(|s| s.get("name"))
                        .and_then(|n| n.as_str())
                    {
                        fields.insert(SECRET.to_string(), secret_name.to_string());
                    }
                    if let Some(interval) = spec.get("interval").and_then(|i| i.as_str()) {
                        fields.insert(INTERVAL.to_string(), interval.to_string());
                    }
                }
                FluxResourceKind::ResourceSet => {
                    if let Some(inputs_from) = spec.get("inputsFrom").and_then(|i| i.as_array()) {
                        let names: Vec<&str> = inputs_from
                            .iter()
                            .filter_map(|item| item.get("name").and_then(|n| n.as_str()))
                            .collect();
                        if !names.is_empty() {
                            fields.insert(INPUTS.to_string(), names.join(", "));
                        }
                    }
                }
                FluxResourceKind::ResourceSetInputProvider => {
                    if let Some(input_type) = spec.get("type").and_then(|t| t.as_str()) {
                        fields.insert(TYPE.to_string(), input_type.to_string());
                    }
                    if let Some(url) = spec.get("url").and_then(|u| u.as_str()) {
                        fields.insert(URL.to_string(), url.to_string());
                    }
                    if let Some(secret_name) = spec
                        .get("secretRef")
                        .and_then(|s| s.get("name"))
                        .and_then(|n| n.as_str())
                    {
                        fields.insert(SECRET.to_string(), secret_name.to_string());
                    }
                    if let Some(reconcile_every) = obj
                        .get("metadata")
                        .and_then(|m| m.get("annotations"))
                        .and_then(|a| a.get("fluxcd.controlplane.io/reconcileEvery"))
                        .and_then(|v| v.as_str())
                    {
                        fields.insert(INTERVAL.to_string(), reconcile_every.to_string());
                    }
                }
                FluxResourceKind::FluxInstance => {
                    if let Some(version) = spec
                        .get("distribution")
                        .and_then(|d| d.get("version"))
                        .and_then(|v| v.as_str())
                    {
                        fields.insert(VERSION.to_string(), version.to_string());
                    }
                }
                _ => {
                    // Extract INTERVAL for other types that might have it
                    if let Some(interval) = spec.get("interval").and_then(|i| i.as_str()) {
                        fields.insert(INTERVAL.to_string(), interval.to_string());
                    }
                }
            }
        }

        // Extract status fields
        if let Some(status) = obj.get("status").and_then(|s| s.as_object()) {
            if matches!(self, FluxResourceKind::HelmRelease) {
                if let Some(helm_chart) = status.get("helmChart").and_then(|hc| hc.as_str()) {
                    fields.insert(CHART.to_string(), helm_chart.to_string());
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
                    fields.insert(STATUS.to_string(), release_status.to_string());
                }
            }
            if matches!(self, FluxResourceKind::Receiver) {
                if let Some(webhook) = status.get("webhookPath").and_then(|w| w.as_str()) {
                    fields.insert(WEBHOOK.to_string(), webhook.to_string());
                }
            }
            if matches!(self, FluxResourceKind::OCIRepository) {
                if let Some(digest) = status
                    .get("artifact")
                    .and_then(|a| a.get("digest"))
                    .and_then(|d| d.as_str())
                {
                    fields.insert(DIGEST.to_string(), digest.to_string());
                }
                if let Some(revision) = status
                    .get("artifact")
                    .and_then(|a| a.get("revision"))
                    .and_then(|r| r.as_str())
                {
                    fields.insert(REVISION.to_string(), revision.to_string());
                }
            }
        }

        fields
    }
}

impl fmt::Display for FluxResourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl From<FluxResourceKind> for String {
    fn from(kind: FluxResourceKind) -> Self {
        kind.as_str().to_string()
    }
}

impl FromStr for FluxResourceKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "GitRepository" => Ok(FluxResourceKind::GitRepository),
            "OCIRepository" => Ok(FluxResourceKind::OCIRepository),
            "HelmRepository" => Ok(FluxResourceKind::HelmRepository),
            "Bucket" => Ok(FluxResourceKind::Bucket),
            "HelmChart" => Ok(FluxResourceKind::HelmChart),
            "ExternalArtifact" => Ok(FluxResourceKind::ExternalArtifact),
            "ArtifactGenerator" => Ok(FluxResourceKind::ArtifactGenerator),
            "Kustomization" => Ok(FluxResourceKind::Kustomization),
            "HelmRelease" => Ok(FluxResourceKind::HelmRelease),
            "ImageRepository" => Ok(FluxResourceKind::ImageRepository),
            "ImagePolicy" => Ok(FluxResourceKind::ImagePolicy),
            "ImageUpdateAutomation" => Ok(FluxResourceKind::ImageUpdateAutomation),
            "Alert" => Ok(FluxResourceKind::Alert),
            "Provider" => Ok(FluxResourceKind::Provider),
            "Receiver" => Ok(FluxResourceKind::Receiver),
            "ResourceSet" => Ok(FluxResourceKind::ResourceSet),
            "ResourceSetInputProvider" => Ok(FluxResourceKind::ResourceSetInputProvider),
            "FluxReport" => Ok(FluxResourceKind::FluxReport),
            "FluxInstance" => Ok(FluxResourceKind::FluxInstance),
            _ => Err(format!("Unknown Flux resource kind: {}", s)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_as_str() {
        assert_eq!(FluxResourceKind::GitRepository.as_str(), "GitRepository");
        assert_eq!(FluxResourceKind::OCIRepository.as_str(), "OCIRepository");
        assert_eq!(FluxResourceKind::Kustomization.as_str(), "Kustomization");
    }

    #[test]
    fn test_from_str() {
        assert_eq!(
            FluxResourceKind::parse_optional("GitRepository"),
            Some(FluxResourceKind::GitRepository)
        );
        assert_eq!(
            FluxResourceKind::parse_optional("OCIRepository"),
            Some(FluxResourceKind::OCIRepository)
        );
        assert_eq!(FluxResourceKind::parse_optional("Unknown"), None);
    }

    #[test]
    fn test_from_str_case_insensitive() {
        assert_eq!(
            FluxResourceKind::from_str_case_insensitive("gitrepository"),
            Some(FluxResourceKind::GitRepository)
        );
        assert_eq!(
            FluxResourceKind::from_str_case_insensitive("GitRepository"),
            Some(FluxResourceKind::GitRepository)
        );
        assert_eq!(
            FluxResourceKind::from_str_case_insensitive("ks"),
            Some(FluxResourceKind::Kustomization)
        );
        assert_eq!(
            FluxResourceKind::from_str_case_insensitive("oci"),
            Some(FluxResourceKind::OCIRepository)
        );
    }

    #[test]
    fn test_display() {
        assert_eq!(
            format!("{}", FluxResourceKind::GitRepository),
            "GitRepository"
        );
        assert_eq!(
            format!("{}", FluxResourceKind::Kustomization),
            "Kustomization"
        );
    }

    #[test]
    fn test_into_string() {
        let s: String = FluxResourceKind::HelmRelease.into();
        assert_eq!(s, "HelmRelease");
    }

    #[test]
    fn test_is_stateless() {
        assert!(FluxResourceKind::Alert.is_stateless());
        assert!(FluxResourceKind::Provider.is_stateless());
        assert!(!FluxResourceKind::Receiver.is_stateless());
        assert!(!FluxResourceKind::Kustomization.is_stateless());
        assert!(!FluxResourceKind::HelmRelease.is_stateless());
        assert!(!FluxResourceKind::GitRepository.is_stateless());
    }

    #[test]
    fn test_columns() {
        let gitrepo_cols = FluxResourceKind::GitRepository.columns();
        assert!(gitrepo_cols.contains(&"URL"));
        assert!(gitrepo_cols.contains(&"BRANCH"));

        let kustomization_cols = FluxResourceKind::Kustomization.columns();
        assert!(kustomization_cols.contains(&"PATH"));
        assert!(kustomization_cols.contains(&"PRUNE"));

        let helmrelease_cols = FluxResourceKind::HelmRelease.columns();
        assert!(helmrelease_cols.contains(&"CHART"));
        assert!(helmrelease_cols.contains(&"VERSION"));

        let alert_cols = FluxResourceKind::Alert.columns();
        assert!(alert_cols.contains(&"STATUS"));
        assert!(alert_cols.contains(&"PROVIDER"));

        let provider_cols = FluxResourceKind::Provider.columns();
        assert!(provider_cols.contains(&"TYPE"));
        assert!(provider_cols.contains(&"ADDRESS"));
        assert!(provider_cols.contains(&"CHANNEL"));

        let bucket_cols = FluxResourceKind::Bucket.columns();
        assert!(bucket_cols.contains(&"ENDPOINT"));
        assert!(bucket_cols.contains(&"PROVIDER"));

        let receiver_cols = FluxResourceKind::Receiver.columns();
        assert!(receiver_cols.contains(&"TYPE"));
        assert!(receiver_cols.contains(&"WEBHOOK"));

        let resourceset_cols = FluxResourceKind::ResourceSet.columns();
        assert!(resourceset_cols.contains(&"STATUS"));
        assert!(resourceset_cols.contains(&"INPUTS"));

        // Default columns for types without custom columns
        let default_cols = FluxResourceKind::ArtifactGenerator.columns();
        assert!(default_cols.contains(&"STATUS"));
        assert!(default_cols.contains(&"TYPE"));
    }

    #[test]
    fn test_extract_fields_gitrepository() {
        let obj = json!({
            "spec": {
                "url": "https://github.com/fluxcd/flux2",
                "branch": "main",
                "interval": "5m"
            }
        });

        let fields = FluxResourceKind::GitRepository.extract_fields(&obj);
        assert_eq!(
            fields.get("URL"),
            Some(&"https://github.com/fluxcd/flux2".to_string())
        );
        assert_eq!(fields.get("BRANCH"), Some(&"main".to_string()));
        assert_eq!(fields.get("INTERVAL"), Some(&"5m".to_string()));
    }

    #[test]
    fn test_extract_fields_kustomization() {
        let obj = json!({
            "spec": {
                "path": "./clusters/prod",
                "prune": true,
                "interval": "10m"
            }
        });

        let fields = FluxResourceKind::Kustomization.extract_fields(&obj);
        assert_eq!(fields.get("PATH"), Some(&"./clusters/prod".to_string()));
        assert_eq!(fields.get("PRUNE"), Some(&"True".to_string()));
        assert_eq!(fields.get("INTERVAL"), Some(&"10m".to_string()));
    }

    #[test]
    fn test_extract_fields_helmrelease() {
        let obj = json!({
            "spec": {
                "chart": {
                    "spec": {
                        "chart": "cert-manager",
                        "version": "v1.13.6"
                    }
                },
                "interval": "15m"
            }
        });

        let fields = FluxResourceKind::HelmRelease.extract_fields(&obj);
        assert_eq!(fields.get("CHART"), Some(&"cert-manager".to_string()));
        assert_eq!(fields.get("VERSION"), Some(&"v1.13.6".to_string()));
        assert_eq!(fields.get("INTERVAL"), Some(&"15m".to_string()));
    }

    #[test]
    fn test_extract_fields_ocirepository() {
        let obj = json!({
            "spec": {
                "ref": {
                    "semver": ">=1.0.0",
                    "tag": "latest"
                },
                "interval": "5m"
            },
            "status": {
                "artifact": {
                    "digest": "sha256:abc123",
                    "revision": "v1.0.0"
                }
            }
        });

        let fields = FluxResourceKind::OCIRepository.extract_fields(&obj);
        assert_eq!(fields.get("SEMVER"), Some(&">=1.0.0".to_string()));
        assert_eq!(fields.get("TAG"), Some(&"latest".to_string()));
        assert_eq!(fields.get("INTERVAL"), Some(&"5m".to_string()));
        assert_eq!(fields.get("DIGEST"), Some(&"sha256:abc123".to_string()));
        assert_eq!(fields.get("REVISION"), Some(&"v1.0.0".to_string()));
    }

    #[test]
    fn test_extract_fields_resourcesetinputprovider() {
        let obj = json!({
            "metadata": {
                "annotations": {
                    "fluxcd.controlplane.io/reconcileEvery": "30s"
                }
            },
            "spec": {
                "type": "ExternalService",
                "url": "http://flux-api.flux-system.svc.cluster.local:8080/api/v2/flux/clusters/demo-cluster-01.k8s.example.com/platform-components",
                "secretRef": {
                    "name": "internal-api-token"
                }
            }
        });

        let fields = FluxResourceKind::ResourceSetInputProvider.extract_fields(&obj);
        assert_eq!(fields.get("TYPE"), Some(&"ExternalService".to_string()));
        assert_eq!(
            fields.get("URL"),
            Some(&"http://flux-api.flux-system.svc.cluster.local:8080/api/v2/flux/clusters/demo-cluster-01.k8s.example.com/platform-components".to_string())
        );
        assert_eq!(
            fields.get("SECRET"),
            Some(&"internal-api-token".to_string())
        );
        assert_eq!(fields.get("INTERVAL"), Some(&"30s".to_string()));
    }

    #[test]
    fn test_extract_fields_missing_spec() {
        let obj = json!({});

        let fields = FluxResourceKind::Kustomization.extract_fields(&obj);
        assert!(fields.is_empty());
    }

    #[test]
    fn test_extract_fields_default_type() {
        let obj = json!({
            "spec": {
                "someField": "value",
                "interval": "5m"
            }
        });

        // ArtifactGenerator falls through to the default arm
        let fields = FluxResourceKind::ArtifactGenerator.extract_fields(&obj);
        // Should only extract INTERVAL for default types
        assert_eq!(fields.get("INTERVAL"), Some(&"5m".to_string()));
        assert_eq!(fields.len(), 1);
    }

    #[test]
    fn test_extract_fields_alert() {
        let obj = json!({
            "spec": {
                "providerRef": {
                    "name": "slack-bot"
                },
                "eventSeverity": "error"
            }
        });

        let fields = FluxResourceKind::Alert.extract_fields(&obj);
        assert_eq!(fields.get("PROVIDER"), Some(&"slack-bot".to_string()));
    }

    #[test]
    fn test_extract_fields_provider() {
        let obj = json!({
            "spec": {
                "type": "slack",
                "address": "https://hooks.slack.com/services/xxx",
                "channel": "#alerts",
                "secretRef": {
                    "name": "slack-webhook"
                }
            }
        });

        let fields = FluxResourceKind::Provider.extract_fields(&obj);
        assert_eq!(fields.get("TYPE"), Some(&"slack".to_string()));
        assert_eq!(
            fields.get("ADDRESS"),
            Some(&"https://hooks.slack.com/services/xxx".to_string())
        );
        assert_eq!(fields.get("CHANNEL"), Some(&"#alerts".to_string()));
        assert_eq!(fields.get("SECRET"), Some(&"slack-webhook".to_string()));
    }

    #[test]
    fn test_extract_fields_bucket() {
        let obj = json!({
            "spec": {
                "endpoint": "minio.example.com",
                "provider": "generic",
                "bucketName": "my-bucket",
                "interval": "5m",
                "secretRef": {
                    "name": "minio-credentials"
                }
            }
        });

        let fields = FluxResourceKind::Bucket.extract_fields(&obj);
        assert_eq!(
            fields.get("ENDPOINT"),
            Some(&"minio.example.com".to_string())
        );
        assert_eq!(fields.get("PROVIDER"), Some(&"generic".to_string()));
        assert_eq!(fields.get("INTERVAL"), Some(&"5m".to_string()));
        assert_eq!(fields.get("SECRET"), Some(&"minio-credentials".to_string()));
    }

    #[test]
    fn test_extract_fields_receiver() {
        let obj = json!({
            "spec": {
                "type": "github",
                "secretRef": {
                    "name": "webhook-token"
                },
                "interval": "10m"
            },
            "status": {
                "webhookPath": "/hook/abc123"
            }
        });

        let fields = FluxResourceKind::Receiver.extract_fields(&obj);
        assert_eq!(fields.get("TYPE"), Some(&"github".to_string()));
        assert_eq!(fields.get("SECRET"), Some(&"webhook-token".to_string()));
        assert_eq!(fields.get("INTERVAL"), Some(&"10m".to_string()));
        assert_eq!(fields.get("WEBHOOK"), Some(&"/hook/abc123".to_string()));
    }

    #[test]
    fn test_extract_fields_flux_instance() {
        let obj = json!({
            "spec": {
                "distribution": {
                    "version": "2.3.0",
                    "registry": "ghcr.io/fluxcd"
                }
            }
        });

        let fields = FluxResourceKind::FluxInstance.extract_fields(&obj);
        assert_eq!(fields.get("VERSION"), Some(&"2.3.0".to_string()));
    }

    #[test]
    fn test_extract_fields_external_artifact() {
        let obj = json!({
            "spec": {
                "sourceRef": {
                    "kind": "ConfigMap",
                    "name": "my-config"
                }
            }
        });

        let fields = FluxResourceKind::ExternalArtifact.extract_fields(&obj);
        assert_eq!(fields.get("SOURCE"), Some(&"my-config".to_string()));
        assert_eq!(fields.get("TYPE"), Some(&"ConfigMap".to_string()));
    }

    #[test]
    fn test_extract_fields_resource_set() {
        let obj = json!({
            "spec": {
                "inputsFrom": [
                    {"name": "namespaces"},
                    {"name": "clusters"}
                ],
                "resources": [{}]
            }
        });

        let fields = FluxResourceKind::ResourceSet.extract_fields(&obj);
        assert_eq!(
            fields.get("INPUTS"),
            Some(&"namespaces, clusters".to_string())
        );
    }

    #[test]
    fn test_extract_fields_resource_set_no_inputs_from() {
        let obj = json!({
            "spec": {
                "inputs": [{"name": "test"}],
                "resources": [{}]
            }
        });

        let fields = FluxResourceKind::ResourceSet.extract_fields(&obj);
        assert!(!fields.contains_key("INPUTS"));
    }
}
