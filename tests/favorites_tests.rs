//! Tests for favorites functionality

use flux9s::config::Config;

#[test]
fn test_config_favorites_default() {
    let config = Config::default();
    assert!(config.favorites.is_empty());
}

#[test]
fn test_config_favorites_serialization() {
    let config = Config {
        favorites: vec![
            "Kustomization:flux-system:my-app".to_string(),
            "HelmRelease:production:nginx".to_string(),
        ],
        ..Config::default()
    };

    let yaml = flux9s::yaml::to_string(&config).unwrap();
    assert!(yaml.contains("favorites"));
    assert!(yaml.contains("Kustomization:flux-system:my-app"));
    assert!(yaml.contains("HelmRelease:production:nginx"));
}

#[test]
fn test_config_favorites_deserialization() {
    let yaml = r#"
readOnly: false
defaultNamespace: flux-system
favorites:
  - "Kustomization:flux-system:my-app"
  - "HelmRelease:production:nginx"
"#;

    let config: Config = flux9s::yaml::from_str(yaml).unwrap();
    assert_eq!(config.favorites.len(), 2);
    assert!(
        config
            .favorites
            .contains(&"Kustomization:flux-system:my-app".to_string())
    );
    assert!(
        config
            .favorites
            .contains(&"HelmRelease:production:nginx".to_string())
    );
}

#[test]
fn test_config_favorites_empty_skipped() {
    let config = Config::default();
    let yaml = flux9s::yaml::to_string(&config).unwrap();
    // Empty favorites should be skipped in serialization
    assert!(!yaml.contains("favorites"));
}

// Note: App methods are tested through integration tests
// These unit tests focus on config serialization/deserialization

#[test]
fn test_resource_key_format() {
    let key = flux9s::ResourceKey::new("Kustomization", "flux-system", "my-app").to_key_string();
    assert_eq!(key, "Kustomization:flux-system:my-app");
}

// Note: Navigation and filtering tests require access to private fields/methods
// These are tested through integration tests and manual testing
