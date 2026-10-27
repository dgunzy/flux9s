//! The generated Flux models, re-exported for typed reads
//! (`Api::<Kustomization>`).
//!
//! Group, version, plural, and command aliases are not declared here: each
//! kind's row in the `flux_kinds!` table (`src/models/flux_resource_kind.rs`)
//! names its model, and [`crate::models::FluxResourceKind::api_resource`]
//! reads them from the model's `#[kube(...)]` attributes. To add a kind,
//! regenerate the models and add its row there; the watcher, `:` commands,
//! and help pick it up.

use crate::models::_generated::*;

// Re-export the generated types for convenience
pub use helm_controller::HelmRelease;
pub use image_automation_controller::ImageUpdateAutomation;
pub use image_reflector_controller::{ImagePolicy, ImageRepository};
pub use kustomize_controller::Kustomization;
pub use notification_controller::{Alert, Provider, Receiver};
pub use source_controller::{
    Bucket, ExternalArtifact, GitRepository, HelmChart, HelmRepository, OciRepository,
};
pub use source_watcher::ArtifactGenerator;
// Flux Operator resources
pub use flux_operator_fluxinstances::FluxInstance;
pub use flux_operator_fluxreports::FluxReport;
pub use flux_operator_resourcesetinputproviders::ResourceSetInputProvider;
pub use flux_operator_resourcesets::ResourceSet;
