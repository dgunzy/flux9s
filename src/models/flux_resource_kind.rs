//! Flux Resource Kind definitions
//!
//! This module provides a centralized enum for all Flux CRD resource kinds.
//! This eliminates hardcoded strings throughout the codebase and provides
//! type safety for resource kind references.

use std::fmt;
use std::str::FromStr;

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
}
