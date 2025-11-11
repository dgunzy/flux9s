//! Resource type definitions and implementations
//!
//! Wraps generated CRD types with WatchableResource trait implementations.
//!
//! ## Adding a New Resource Type
//!
//! To add a new Flux CRD resource type:
//!
//! 1. **Re-export the type** (if not already exported above)
//!    ```text
//!    pub use source_controller::YourNewResource;
//!    ```
//!
//! 2. **Add impl_watchable! macro** with correct API details:
//!    ```text
//!    impl_watchable!(
//!        YourNewResource,
//!        "source.toolkit.fluxcd.io",  // API group
//!        "v1",                        // API version
//!        "yournewresources",          // Plural name
//!        "YourNewResource"            // Display name
//!    );
//!    ```
//!
//! 3. **Add to registry** in `src/watcher/registry.rs`:
//!    ```text
//!    ResourceEntry {
//!        display_name: "YourNewResource",
//!        command_aliases: &["yournewresource", "ynr"],
//!    },
//!    ```
//!
//! 4. **Add watch call** in `src/watcher/mod.rs` `watch_all()`:
//!    ```text
//!    self.watch::<resource::YourNewResource>()?;
//!    ```
//!
//! That's it! The resource will automatically:
//! - Be watched for changes
//! - Appear in the unified view
//! - Support command mode (`:yournewresource`)
//! - Show up in help text

use crate::models::_generated::*;

// Re-export the generated types for convenience
pub use helm_controller::HelmRelease;
pub use image_automation_controller::ImageUpdateAutomation;
pub use image_reflector_controller::{ImagePolicy, ImageRepository};
pub use kustomize_controller::Kustomization;
pub use notification_controller::{Alert, Provider, Receiver};
pub use source_controller::{
    Bucket, ExternalArtifact, GitRepository, HelmChart, HelmRepository, OCIRepository,
};

// Implement WatchableResource for each Flux resource type

macro_rules! impl_watchable {
    ($type:ty, $group:expr, $version:expr, $plural:expr, $display:expr) => {
        impl crate::watcher::WatchableResource for $type {
            fn api_group() -> &'static str {
                $group
            }

            fn api_version() -> &'static str {
                $version
            }

            fn plural() -> &'static str {
                $plural
            }

            fn display_name() -> &'static str {
                $display
            }
        }
    };
}

// Source Controller resources
impl_watchable!(
    GitRepository,
    "source.toolkit.fluxcd.io",
    "v1",
    "gitrepositories",
    "GitRepository"
);
impl_watchable!(
    OCIRepository,
    "source.toolkit.fluxcd.io",
    "v1",
    "ocirepositories",
    "OCIRepository"
);
impl_watchable!(
    HelmRepository,
    "source.toolkit.fluxcd.io",
    "v1",
    "helmrepositories",
    "HelmRepository"
);
impl_watchable!(
    Bucket,
    "source.toolkit.fluxcd.io",
    "v1",
    "buckets",
    "Bucket"
);
impl_watchable!(
    HelmChart,
    "source.toolkit.fluxcd.io",
    "v1",
    "helmcharts",
    "HelmChart"
);
impl_watchable!(
    ExternalArtifact,
    "source.toolkit.fluxcd.io",
    "v1",
    "externalartifacts",
    "ExternalArtifact"
);

// Kustomize Controller resources
impl_watchable!(
    Kustomization,
    "kustomize.toolkit.fluxcd.io",
    "v1",
    "kustomizations",
    "Kustomization"
);

// Helm Controller resources
impl_watchable!(
    HelmRelease,
    "helm.toolkit.fluxcd.io",
    "v2beta2",
    "helmreleases",
    "HelmRelease"
);

// Image Reflector Controller resources
impl_watchable!(
    ImageRepository,
    "image.toolkit.fluxcd.io",
    "v1",
    "imagerepositories",
    "ImageRepository"
);
impl_watchable!(
    ImagePolicy,
    "image.toolkit.fluxcd.io",
    "v1",
    "imagepolicies",
    "ImagePolicy"
);

// Image Automation Controller resources
impl_watchable!(
    ImageUpdateAutomation,
    "image.toolkit.fluxcd.io",
    "v1",
    "imageupdateautomations",
    "ImageUpdateAutomation"
);

// Notification Controller resources
impl_watchable!(
    Alert,
    "notification.toolkit.fluxcd.io",
    "v1beta3",
    "alerts",
    "Alert"
);
impl_watchable!(
    Provider,
    "notification.toolkit.fluxcd.io",
    "v1beta3",
    "providers",
    "Provider"
);
impl_watchable!(
    Receiver,
    "notification.toolkit.fluxcd.io",
    "v1beta3",
    "receivers",
    "Receiver"
);
