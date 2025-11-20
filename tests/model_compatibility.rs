//! CRD model compilation tests
//!
//! These tests ensure that generated models compile correctly and can be used.
//! When CRDs are updated, these tests will catch compilation errors.

use flux9s::watcher::{
    Alert, Bucket, ExternalArtifact, GitRepository, HelmChart, HelmRelease, HelmRepository,
    ImagePolicy, ImageRepository, ImageUpdateAutomation, Kustomization, OCIRepository, Provider,
    Receiver, WatchableResource,
};

#[test]
fn test_generated_models_compile() {
    // This test ensures all generated models can be imported and used

    // Test that types can be used (even if just for compilation)
    let _gitrepo: Option<GitRepository> = None;
    let _kustomization: Option<Kustomization> = None;
    let _helmrelease: Option<HelmRelease> = None;

    // If we get here, the models compiled successfully - the test passes
}

#[test]
fn test_watchable_resource_trait_implementations() {
    // Test that all resource types implement WatchableResource trait

    // Test API group/version/plural methods exist and return strings
    assert_eq!(GitRepository::api_group(), "source.toolkit.fluxcd.io");
    assert_eq!(GitRepository::api_version(), "v1");
    assert_eq!(GitRepository::plural(), "gitrepositories");
    assert_eq!(GitRepository::display_name(), "GitRepository");

    assert_eq!(Kustomization::api_group(), "kustomize.toolkit.fluxcd.io");
    assert_eq!(Kustomization::api_version(), "v1");
    assert_eq!(Kustomization::plural(), "kustomizations");
    assert_eq!(Kustomization::display_name(), "Kustomization");

    assert_eq!(HelmRelease::api_group(), "helm.toolkit.fluxcd.io");
    assert_eq!(HelmRelease::api_version(), "v2beta2");
    assert_eq!(HelmRelease::plural(), "helmreleases");
    assert_eq!(HelmRelease::display_name(), "HelmRelease");
}

#[test]
fn test_resource_type_api_consistency() {
    // Test that API groups/versions match expected Flux patterns

    // Source controller resources should all use source.toolkit.fluxcd.io/v1
    assert_eq!(GitRepository::api_group(), "source.toolkit.fluxcd.io");
    assert_eq!(GitRepository::api_version(), "v1");
    assert_eq!(OCIRepository::api_group(), "source.toolkit.fluxcd.io");
    assert_eq!(OCIRepository::api_version(), "v1");
    assert_eq!(HelmRepository::api_group(), "source.toolkit.fluxcd.io");
    assert_eq!(HelmRepository::api_version(), "v1");
    assert_eq!(Bucket::api_group(), "source.toolkit.fluxcd.io");
    assert_eq!(Bucket::api_version(), "v1");
    assert_eq!(HelmChart::api_group(), "source.toolkit.fluxcd.io");
    assert_eq!(HelmChart::api_version(), "v1");
    assert_eq!(ExternalArtifact::api_group(), "source.toolkit.fluxcd.io");
    assert_eq!(ExternalArtifact::api_version(), "v1");

    // Kustomize controller
    assert_eq!(Kustomization::api_group(), "kustomize.toolkit.fluxcd.io");
    assert_eq!(Kustomization::api_version(), "v1");

    // Helm controller
    assert_eq!(HelmRelease::api_group(), "helm.toolkit.fluxcd.io");
    assert_eq!(HelmRelease::api_version(), "v2beta2");

    // Image reflector/automation controllers
    assert_eq!(ImageRepository::api_group(), "image.toolkit.fluxcd.io");
    assert_eq!(ImageRepository::api_version(), "v1");
    assert_eq!(ImagePolicy::api_group(), "image.toolkit.fluxcd.io");
    assert_eq!(ImagePolicy::api_version(), "v1");
    assert_eq!(
        ImageUpdateAutomation::api_group(),
        "image.toolkit.fluxcd.io"
    );
    assert_eq!(ImageUpdateAutomation::api_version(), "v1");

    // Notification controller
    assert_eq!(Alert::api_group(), "notification.toolkit.fluxcd.io");
    assert_eq!(Alert::api_version(), "v1beta3");
    assert_eq!(Provider::api_group(), "notification.toolkit.fluxcd.io");
    assert_eq!(Provider::api_version(), "v1beta3");
    assert_eq!(Receiver::api_group(), "notification.toolkit.fluxcd.io");
    assert_eq!(Receiver::api_version(), "v1beta3");
}
