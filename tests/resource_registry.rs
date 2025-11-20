//! Resource registry tests
//!
//! Tests to ensure resource type detection and command mapping work correctly
//! when CRDs are updated.

use flux9s::get_all_commands;

#[test]
fn test_get_all_commands() {
    let commands = get_all_commands();

    // Should have entries for all Flux resource types
    assert!(commands.len() > 0);

    // Check that common resource types are present
    let has_kustomization = commands.iter().any(|(name, _)| *name == "Kustomization");
    let has_gitrepository = commands.iter().any(|(name, _)| *name == "GitRepository");
    let has_helmrelease = commands.iter().any(|(name, _)| *name == "HelmRelease");

    assert!(
        has_kustomization,
        "Kustomization should be in command registry"
    );
    assert!(
        has_gitrepository,
        "GitRepository should be in command registry"
    );
    assert!(has_helmrelease, "HelmRelease should be in command registry");
}

#[test]
fn test_command_aliases() {
    let commands = get_all_commands();

    // Check that aliases work
    if let Some((_, aliases)) = commands.iter().find(|(name, _)| *name == "Kustomization") {
        // Should have common aliases
        assert!(
            aliases
                .iter()
                .any(|a| a == &"kustomization" || a == &"ks" || a == &"kustomizations"),
            "Kustomization should have aliases"
        );
    }
}

#[test]
fn test_all_resource_types_have_commands() {
    let commands = get_all_commands();

    // Expected Flux resource types
    let expected_types = vec![
        "Kustomization",
        "GitRepository",
        "OCIRepository",
        "HelmRepository",
        "HelmChart",
        "HelmRelease",
        "ImageRepository",
        "ImagePolicy",
        "ImageUpdateAutomation",
        "Alert",
        "Provider",
        "Receiver",
        "Bucket",
        "ExternalArtifact",
        // Flux Operator resources
        "ResourceSet",
        "ResourceSetInputProvider",
        "FluxReport",
        "FluxInstance",
    ];

    for resource_type in expected_types {
        let found = commands.iter().any(|(name, _)| *name == resource_type);
        assert!(
            found,
            "Resource type {} should be in command registry",
            resource_type
        );
    }
}
