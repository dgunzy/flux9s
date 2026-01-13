//! Embedded themes/skins from k9s
//!
//! This module provides built-in themes that are embedded in the binary.
//! These themes are available by default without requiring users to install
//! theme files manually. Users can still add custom themes to their config directory.

use crate::config::theme_loader::ThemeLoader;
use crate::tui::Theme;
use anyhow::{Context, Result};

/// List of embedded theme names
pub const EMBEDDED_THEMES: &[&str] = &[
    "dracula",
    "nord",
    "solarized-dark",
    "monokai",
    "gruvbox-dark",
    "catppuccin-mocha",
    "rose-pine-moon",
    "inthenavy",
    "kiss",
    "default-light",
    "one-dark",
    "tokyo-night",
    "ayu-dark",
    "snazzy",
    "tomorrow-night",
    "papercolor-dark",
    "base16-dark",
];

/// Get embedded theme YAML content by name
pub fn get_embedded_theme(name: &str) -> Option<&'static str> {
    match name {
        "dracula" => Some(include_str!("embedded_themes/dracula.yaml")),
        "nord" => Some(include_str!("embedded_themes/nord.yaml")),
        "solarized-dark" => Some(include_str!("embedded_themes/solarized-dark.yaml")),
        "monokai" => Some(include_str!("embedded_themes/monokai.yaml")),
        "gruvbox-dark" => Some(include_str!("embedded_themes/gruvbox-dark.yaml")),
        "catppuccin-mocha" => Some(include_str!("embedded_themes/catppuccin-mocha.yaml")),
        "rose-pine-moon" => Some(include_str!("embedded_themes/rose-pine-moon.yaml")),
        "inthenavy" => Some(include_str!("embedded_themes/inthenavy.yaml")),
        "kiss" => Some(include_str!("embedded_themes/kiss.yaml")),
        "default-light" => Some(include_str!("embedded_themes/default-light.yaml")),
        "one-dark" => Some(include_str!("embedded_themes/one-dark.yaml")),
        "tokyo-night" => Some(include_str!("embedded_themes/tokyo-night.yaml")),
        "ayu-dark" => Some(include_str!("embedded_themes/ayu-dark.yaml")),
        "snazzy" => Some(include_str!("embedded_themes/snazzy.yaml")),
        "tomorrow-night" => Some(include_str!("embedded_themes/tomorrow-night.yaml")),
        "papercolor-dark" => Some(include_str!("embedded_themes/papercolor-dark.yaml")),
        "base16-dark" => Some(include_str!("embedded_themes/base16-dark.yaml")),
        _ => None,
    }
}

/// Load an embedded theme by name
pub fn load_embedded_theme(name: &str) -> Result<Theme> {
    let yaml_content = get_embedded_theme(name)
        .ok_or_else(|| anyhow::anyhow!("Embedded theme '{}' not found", name))?;

    ThemeLoader::load_from_yaml(yaml_content)
        .with_context(|| format!("Failed to load embedded theme '{}'", name))
}

/// Check if a theme name is an embedded theme
pub fn is_embedded_theme(name: &str) -> bool {
    EMBEDDED_THEMES.contains(&name)
}

/// Get all embedded theme names
pub fn list_embedded_themes() -> Vec<String> {
    EMBEDDED_THEMES.iter().map(|s| s.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_embedded_themes() {
        let themes = list_embedded_themes();
        assert!(
            !themes.is_empty(),
            "Should have at least one embedded theme"
        );
        assert_eq!(themes.len(), EMBEDDED_THEMES.len());
        assert!(themes.contains(&"dracula".to_string()));
        assert!(themes.contains(&"nord".to_string()));
    }

    #[test]
    fn test_is_embedded_theme() {
        assert!(is_embedded_theme("dracula"));
        assert!(is_embedded_theme("nord"));
        assert!(is_embedded_theme("solarized-dark"));
        assert!(!is_embedded_theme("nonexistent"));
        assert!(!is_embedded_theme("default"));
        assert!(!is_embedded_theme(""));
    }

    #[test]
    fn test_get_embedded_theme() {
        // Test that we can get YAML content for known themes
        assert!(get_embedded_theme("dracula").is_some());
        assert!(get_embedded_theme("nord").is_some());
        assert!(get_embedded_theme("solarized-dark").is_some());

        // Test that YAML content is not empty
        let dracula_yaml = get_embedded_theme("dracula").unwrap();
        assert!(!dracula_yaml.is_empty());
        assert!(dracula_yaml.contains("k9s:"));

        // Test that unknown themes return None
        assert!(get_embedded_theme("nonexistent").is_none());
        assert!(get_embedded_theme("default").is_none());
    }

    #[test]
    fn test_load_embedded_theme() {
        // Test loading a known embedded theme
        let theme = load_embedded_theme("dracula").expect("Should load dracula theme");
        // Verify theme has been loaded (has some color properties)
        // The exact colors depend on the theme file, but we can check it's a valid Theme
        let _ = theme.header_context; // Access to ensure it's loaded

        // Test loading another theme
        let nord_theme = load_embedded_theme("nord").expect("Should load nord theme");
        let _ = nord_theme.header_context;

        // Test that themes are different (they should have different colors)
        // This is a basic sanity check
        assert!(
            theme.header_context != nord_theme.header_context
                || theme.text_primary != nord_theme.text_primary
        );
    }

    #[test]
    fn test_load_embedded_theme_nonexistent() {
        // Test that loading a non-existent theme fails
        let result = load_embedded_theme("nonexistent");
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("not found") || error_msg.contains("nonexistent"));
    }

    #[test]
    fn test_all_embedded_themes_loadable() {
        // Test that all themes in EMBEDDED_THEMES can be loaded
        for theme_name in EMBEDDED_THEMES {
            let result = load_embedded_theme(theme_name);
            assert!(
                result.is_ok(),
                "Failed to load embedded theme '{}': {:?}",
                theme_name,
                result.err()
            );
            let theme = result.unwrap();
            // Verify theme is valid by accessing a property
            let _ = theme.header_context;
        }
    }

    #[test]
    fn test_embedded_themes_yaml_valid() {
        // Test that all embedded theme YAML files are valid and parseable
        for theme_name in EMBEDDED_THEMES {
            let yaml_content = get_embedded_theme(theme_name)
                .unwrap_or_else(|| panic!("Should have YAML content for theme '{}'", theme_name));

            // Verify YAML contains expected structure
            assert!(
                yaml_content.contains("k9s:"),
                "Theme '{}' should contain 'k9s:' key",
                theme_name
            );

            // Verify we can parse it as a theme
            let result = ThemeLoader::load_from_yaml(yaml_content);
            assert!(
                result.is_ok(),
                "Failed to parse YAML for theme '{}': {:?}",
                theme_name,
                result.err()
            );
        }
    }
}
