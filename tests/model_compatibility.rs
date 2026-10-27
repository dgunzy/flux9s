//! CRD model compilation tests
//!
//! These tests ensure that generated models compile correctly and can be used.
//! When CRDs are updated, these tests will catch compilation errors.

use flux9s::models::FluxResourceKind;
use flux9s::watcher::{GitRepository, HelmRelease, Kustomization, ResourceSetInputProvider};
use serde_json::json;

#[test]
fn test_generated_models_compile() {
    // This test ensures all generated models can be imported and used

    // Test that types can be used (even if just for compilation)
    let _gitrepo: Option<GitRepository> = None;
    let _kustomization: Option<Kustomization> = None;
    let _helmrelease: Option<HelmRelease> = None;

    // If we get here, the models compiled successfully - the test passes
}

/// Every Flux kind's group / current version / plural, read from the
/// generated model. A CRD update that moves a kind's version fails here:
/// update this table and add the old version to `older_api_versions`, so
/// clusters still serving it keep working.
#[test]
fn every_kind_reads_its_api_from_the_generated_model() {
    let expected = [
        (
            FluxResourceKind::GitRepository,
            "source.toolkit.fluxcd.io",
            "v1",
            "gitrepositories",
        ),
        (
            FluxResourceKind::OCIRepository,
            "source.toolkit.fluxcd.io",
            "v1",
            "ocirepositories",
        ),
        (
            FluxResourceKind::HelmRepository,
            "source.toolkit.fluxcd.io",
            "v1",
            "helmrepositories",
        ),
        (
            FluxResourceKind::Bucket,
            "source.toolkit.fluxcd.io",
            "v1",
            "buckets",
        ),
        (
            FluxResourceKind::HelmChart,
            "source.toolkit.fluxcd.io",
            "v1",
            "helmcharts",
        ),
        (
            FluxResourceKind::ExternalArtifact,
            "source.toolkit.fluxcd.io",
            "v1",
            "externalartifacts",
        ),
        (
            FluxResourceKind::ArtifactGenerator,
            "source.extensions.fluxcd.io",
            "v1beta1",
            "artifactgenerators",
        ),
        (
            FluxResourceKind::Kustomization,
            "kustomize.toolkit.fluxcd.io",
            "v1",
            "kustomizations",
        ),
        (
            FluxResourceKind::HelmRelease,
            "helm.toolkit.fluxcd.io",
            "v2",
            "helmreleases",
        ),
        (
            FluxResourceKind::ImageRepository,
            "image.toolkit.fluxcd.io",
            "v1",
            "imagerepositories",
        ),
        (
            FluxResourceKind::ImagePolicy,
            "image.toolkit.fluxcd.io",
            "v1",
            "imagepolicies",
        ),
        (
            FluxResourceKind::ImageUpdateAutomation,
            "image.toolkit.fluxcd.io",
            "v1",
            "imageupdateautomations",
        ),
        (
            FluxResourceKind::Alert,
            "notification.toolkit.fluxcd.io",
            "v1",
            "alerts",
        ),
        (
            FluxResourceKind::Provider,
            "notification.toolkit.fluxcd.io",
            "v1",
            "providers",
        ),
        (
            FluxResourceKind::Receiver,
            "notification.toolkit.fluxcd.io",
            "v1",
            "receivers",
        ),
        (
            FluxResourceKind::ResourceSet,
            "fluxcd.controlplane.io",
            "v1",
            "resourcesets",
        ),
        (
            FluxResourceKind::ResourceSetInputProvider,
            "fluxcd.controlplane.io",
            "v1",
            "resourcesetinputproviders",
        ),
        (
            FluxResourceKind::FluxReport,
            "fluxcd.controlplane.io",
            "v1",
            "fluxreports",
        ),
        (
            FluxResourceKind::FluxInstance,
            "fluxcd.controlplane.io",
            "v1",
            "fluxinstances",
        ),
    ];
    assert_eq!(
        expected.len(),
        FluxResourceKind::ALL.len(),
        "a kind is missing here"
    );
    for (kind, group, version, plural) in expected {
        let resource = kind.api_resource();
        assert_eq!(
            (
                resource.group.as_str(),
                resource.version.as_str(),
                resource.plural.as_str()
            ),
            (group, version, plural),
            "{kind}"
        );
        assert_eq!(resource.kind, kind.as_str(), "variant names the CRD kind");
        assert_eq!(resource.api_version, format!("{group}/{version}"));
    }
}

#[test]
fn test_resourcesetinputprovider_externalservice_deserializes() {
    let obj = json!({
        "apiVersion": "fluxcd.controlplane.io/v1",
        "kind": "ResourceSetInputProvider",
        "metadata": {
            "name": "namespaces",
            "namespace": "flux-system"
        },
        "spec": {
            "type": "ExternalService",
            "url": "http://flux-api.flux-system.svc.cluster.local:8080/api/v2/input"
        }
    });

    let parsed: Result<ResourceSetInputProvider, _> = serde_json::from_value(obj);
    assert!(
        parsed.is_ok(),
        "ResourceSetInputProvider with type=ExternalService should deserialize"
    );
}
