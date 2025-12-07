//! Theme loading and management
//!
//! Handles loading themes from YAML files following k9s-style skin format.

use super::paths;
use crate::tui::Theme;
use anyhow::{Context, Result};
use csscolorparser::Color as CssColor;
use ratatui::style::Color;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::str::FromStr;

/// k9s-style skin file structure
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SkinFile {
    k9s: Option<K9sSkin>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct K9sSkin {
    body: Option<BodyColors>,
    frame: Option<FrameColors>,
    views: Option<ViewColors>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BodyColors {
    #[serde(alias = "fgColor")]
    fg: Option<String>,
    #[serde(alias = "bgColor")]
    bg: Option<String>,
    #[serde(alias = "logoColor")]
    logo: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FrameColors {
    border: Option<BorderColors>,
    menu: Option<MenuColors>,
    crumbs: Option<CrumbsColors>,
    status: Option<StatusColors>,
    title: Option<TitleColors>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BorderColors {
    #[serde(alias = "fgColor")]
    fg: Option<String>,
    #[serde(alias = "focusColor")]
    focus: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MenuColors {
    #[serde(alias = "fgColor")]
    fg: Option<String>,
    #[serde(alias = "bgColor")]
    bg: Option<String>,
    #[serde(alias = "keyColor")]
    key: Option<String>,
    #[serde(alias = "numKeyColor")]
    num_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CrumbsColors {
    #[serde(alias = "fgColor")]
    fg: Option<String>,
    #[serde(alias = "activeColor")]
    active: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StatusColors {
    #[serde(alias = "newColor")]
    new: Option<String>,
    #[serde(alias = "modifyColor")]
    modify: Option<String>,
    #[serde(alias = "addColor")]
    add: Option<String>,
    #[serde(alias = "errorColor")]
    error: Option<String>,
    highlightcolor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TitleColors {
    #[serde(alias = "fgColor")]
    fg: Option<String>,
    #[serde(alias = "bgColor")]
    bg: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ViewColors {
    table: Option<TableColors>,
    yaml: Option<YamlColors>,
    logs: Option<LogsColors>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TableColors {
    #[serde(alias = "fgColor")]
    fg: Option<String>,
    #[serde(alias = "bgColor")]
    bg: Option<String>,
    cursor: Option<CursorColors>,
    header: Option<HeaderColors>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CursorColors {
    #[serde(alias = "fgColor")]
    fg: Option<String>,
    #[serde(alias = "bgColor")]
    bg: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HeaderColors {
    #[serde(alias = "fgColor")]
    fg: Option<String>,
    #[serde(alias = "bgColor")]
    bg: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct YamlColors {
    #[serde(alias = "keyColor")]
    key: Option<String>,
    #[serde(alias = "colonColor")]
    colon: Option<String>,
    #[serde(alias = "valueColor")]
    value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LogsColors {
    #[serde(alias = "fgColor")]
    fg: Option<String>,
    #[serde(alias = "bgColor")]
    bg: Option<String>,
}

/// Theme loader
pub struct ThemeLoader;

impl ThemeLoader {
    /// Load a skin by name
    ///
    /// Resolution order:
    /// 1. Config skins directory ($XDG_CONFIG_HOME/flux9s/skins/{name}.yaml) - user-installed skins
    /// 2. Data skins directory ($XDG_DATA_HOME/flux9s/skins/{name}.yaml) - legacy location
    /// 3. Built-in default skin
    pub fn load_theme(name: &str) -> Result<Theme> {
        // Try config skins directory first (where user-installed skins go)
        let config_skin_path = paths::skins_dir().join(format!("{}.yaml", name));
        if config_skin_path.exists() {
            return Self::load_from_file(&config_skin_path);
        }

        // Try data skins directory (legacy location)
        let data_skin_path = paths::user_skins_dir().join(format!("{}.yaml", name));
        if data_skin_path.exists() {
            return Self::load_from_file(&data_skin_path);
        }

        // Fall back to default skin
        if name == "default" {
            return Ok(Theme::default());
        }

        Err(anyhow::anyhow!(
            "Skin '{}' not found in either \n '{}'\n  or '{}'",
            name,
            paths::skins_dir().join(format!("{}.yaml", name)).display(),
            paths::user_skins_dir()
                .join(format!("{}.yaml", name))
                .display()
        ))
    }

    /// Install a skin from a YAML file
    ///
    /// Validates the skin file and copies it to the config skins directory.
    /// The skin name is derived from the source filename (without .yaml extension).
    pub fn install_theme(source_path: &std::path::Path) -> Result<String> {
        use std::fs;

        // Check if source file exists
        if !source_path.exists() {
            return Err(anyhow::anyhow!(
                "Skin file not found: {}",
                source_path.display()
            ));
        }

        // Extract skin name from filename
        let skin_name = source_path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow::anyhow!("Invalid filename: {}", source_path.display()))?;

        if skin_name.is_empty() {
            return Err(anyhow::anyhow!("Skin name cannot be empty"));
        }

        println!("Validating skin file: {}", source_path.display());

        // Read and validate the skin file
        let contents = fs::read_to_string(source_path)
            .with_context(|| format!("Failed to read skin file: {}", source_path.display()))?;

        // Parse YAML
        let skin: SkinFile = serde_yaml::from_str(&contents)
            .with_context(|| format!("Failed to parse YAML: {}", source_path.display()))?;

        // Validate skin structure
        Self::validate_theme_structure(&skin).with_context(|| "Skin validation failed")?;

        // Try to convert to Theme to validate colors can be parsed
        Self::convert_skin_to_theme(skin.clone())
            .with_context(|| "Failed to convert skin - invalid color values")?;

        println!("✓ Skin validation passed");

        // Ensure skins directory exists (in config directory)
        let skins_dir = paths::skins_dir();
        paths::ensure_dir(&skins_dir).with_context(|| {
            format!("Failed to create skins directory: {}", skins_dir.display())
        })?;

        // Determine destination path
        let dest_path = skins_dir.join(format!("{}.yaml", skin_name));

        // Check if skin already exists
        if dest_path.exists() {
            println!(
                "Warning: Skin '{}' already exists and will be overwritten",
                skin_name
            );
        }

        // Copy file to skins directory
        fs::copy(source_path, &dest_path)
            .with_context(|| format!("Failed to copy skin file to: {}", dest_path.display()))?;

        println!("✓ Skin '{}' installed successfully", skin_name);
        println!("  Location: {}", dest_path.display());

        // Return the skin name so caller can set it in config
        Ok(skin_name.to_string())
    }

    /// Validate skin structure - check for required keys and structure
    fn validate_theme_structure(skin: &SkinFile) -> Result<()> {
        // Check that k9s key exists
        let k9s = skin
            .k9s
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing required 'k9s' key in skin file"))?;

        // Check for at least some color definitions
        let mut has_colors = false;

        // Check body colors
        if let Some(body) = &k9s.body {
            if body.fg.is_some() || body.bg.is_some() || body.logo.is_some() {
                has_colors = true;
            }
        }

        // Check frame colors
        if let Some(frame) = &k9s.frame {
            if frame.border.is_some()
                || frame.menu.is_some()
                || frame.crumbs.is_some()
                || frame.status.is_some()
                || frame.title.is_some()
            {
                has_colors = true;
            }
        }

        // Check view colors
        if let Some(views) = &k9s.views {
            if views.table.is_some() || views.yaml.is_some() || views.logs.is_some() {
                has_colors = true;
            }
        }

        if !has_colors {
            return Err(anyhow::anyhow!(
                "Skin file must contain at least one color definition.\n\
                 Expected structure:\n\
                 k9s:\n\
                   body:\n\
                     fgColor: <color>\n\
                   views:\n\
                     table:\n\
                       cursor:\n\
                         fgColor: <color>\n\
                         bgColor: <color>"
            ));
        }

        Ok(())
    }

    /// Load theme from a YAML file
    fn load_from_file(path: &PathBuf) -> Result<Theme> {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read theme file: {}", path.display()))?;

        tracing::debug!("Loading theme from: {}", path.display());

        let skin: SkinFile = serde_yaml::from_str(&contents)
            .with_context(|| format!("Failed to parse theme file: {}", path.display()))?;

        tracing::debug!("Successfully parsed theme file");
        Self::convert_skin_to_theme(skin)
    }

    /// Convert k9s-style skin format to Theme
    fn convert_skin_to_theme(skin: SkinFile) -> Result<Theme> {
        let mut theme = Theme::default();
        let mut colors_applied = 0;

        if let Some(k9s) = skin.k9s {
            // Body colors
            if let Some(body) = k9s.body {
                if let Some(fg) = body.fg {
                    theme.text_primary = parse_color(&fg)?;
                    colors_applied += 1;
                }
                if let Some(_bg) = body.bg {
                    // Background color - not directly used in Theme, but could be
                }
                if let Some(logo) = body.logo {
                    theme.header_ascii = parse_color(&logo)?;
                    colors_applied += 1;
                }
            }

            // Frame colors
            if let Some(frame) = k9s.frame {
                if let Some(border) = frame.border {
                    if let Some(fg) = border.fg {
                        // Border color - could map to header colors
                        theme.header_resources = parse_color(&fg)?;
                        colors_applied += 1;
                    }
                }

                if let Some(menu) = frame.menu {
                    if let Some(fg) = menu.fg {
                        theme.text_primary = parse_color(&fg)?;
                        colors_applied += 1;
                    }
                    if let Some(key) = menu.key {
                        theme.footer_key = parse_color(&key)?;
                        colors_applied += 1;
                    }
                }

                if let Some(crumbs) = frame.crumbs {
                    if let Some(fg) = crumbs.fg {
                        theme.text_secondary = parse_color(&fg)?;
                        colors_applied += 1;
                    }
                    if let Some(active) = crumbs.active {
                        theme.table_selected = parse_color(&active)?;
                        colors_applied += 1;
                    }
                }

                if let Some(status) = frame.status {
                    if let Some(new) = status.new {
                        theme.status_ready = parse_color(&new)?;
                        colors_applied += 1;
                    }
                    if let Some(modify) = status.modify {
                        theme.status_pending = parse_color(&modify)?;
                        colors_applied += 1;
                    }
                    if let Some(_add) = status.add {
                        // Could map to a different status color
                        colors_applied += 1;
                    }
                    if let Some(error) = status.error {
                        theme.status_error = parse_color(&error)?;
                        colors_applied += 1;
                    }
                }

                if let Some(title) = frame.title {
                    if let Some(fg) = title.fg {
                        theme.header_context = parse_color(&fg)?;
                        colors_applied += 1;
                    }
                }
            }

            // View colors
            if let Some(views) = k9s.views {
                if let Some(table) = views.table {
                    if let Some(fg) = table.fg {
                        theme.table_normal = parse_color(&fg)?;
                        colors_applied += 1;
                    }
                    if let Some(_bg) = table.bg {
                        // Background - not directly used
                    }
                    if let Some(cursor) = table.cursor {
                        if let Some(fg) = cursor.fg {
                            theme.table_selected = parse_color(&fg)?;
                            colors_applied += 1;
                        }
                        if let Some(bg) = cursor.bg {
                            theme.table_selected_bg = parse_color(&bg)?;
                            colors_applied += 1;
                        }
                    }
                    if let Some(header) = table.header {
                        if let Some(fg) = header.fg {
                            theme.table_header = parse_color(&fg)?;
                            colors_applied += 1;
                        }
                    }
                }

                if let Some(yaml) = views.yaml {
                    if let Some(key) = yaml.key {
                        theme.text_label = parse_color(&key)?;
                        colors_applied += 1;
                    }
                    if let Some(value) = yaml.value {
                        theme.text_value = parse_color(&value)?;
                        colors_applied += 1;
                    }
                }

                if let Some(logs) = views.logs {
                    if let Some(fg) = logs.fg {
                        theme.text_primary = parse_color(&fg)?;
                        colors_applied += 1;
                    }
                }
            }
        }

        tracing::debug!("Applied {} color values from theme", colors_applied);
        Ok(theme)
    }

    /// List available skins
    pub fn list_themes() -> Vec<String> {
        let mut skins = Vec::new();

        // Check config skins directory first (where user-installed skins go)
        if let Ok(entries) = std::fs::read_dir(paths::skins_dir()) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if name.ends_with(".yaml") {
                        skins.push(name.trim_end_matches(".yaml").to_string());
                    }
                }
            }
        }

        // Check data skins directory (legacy location)
        if let Ok(entries) = std::fs::read_dir(paths::user_skins_dir()) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if name.ends_with(".yaml") {
                        let skin_name = name.trim_end_matches(".yaml").to_string();
                        if !skins.contains(&skin_name) {
                            skins.push(skin_name);
                        }
                    }
                }
            }
        }

        // Always include default
        if !skins.contains(&"default".to_string()) {
            skins.insert(0, "default".to_string());
        }

        skins.sort();
        skins
    }
}

/// Parse a color string to ratatui Color
///
/// Supports:
/// - Hex colors: #ffffff, #fff, ffffff, fff
/// - Named colors: CSS Color Module Level 4 names (140+ colors including X11 colors)
/// - Special: "default" returns a default color
///
/// Uses csscolorparser crate for comprehensive color name support.
fn parse_color(color_str: &str) -> Result<Color> {
    let color_str = color_str.trim();

    // Handle "default" special case
    if color_str.eq_ignore_ascii_case("default") {
        return Ok(Color::Reset);
    }

    // Use csscolorparser to parse the color string
    // It handles hex colors (with/without #), CSS named colors, rgb(), rgba(), hsl(), etc.
    let css_color = CssColor::from_str(color_str)
        .map_err(|e| anyhow::anyhow!("Invalid color '{}': {}", color_str, e))?;

    // Convert csscolorparser Color to ratatui Color
    // csscolorparser uses f64 for components (0.0-1.0), convert to u8 (0-255)
    let rgba = css_color.to_rgba8();
    let (r, g, b) = (rgba[0], rgba[1], rgba[2]);

    // Map common colors to ratatui's built-in colors for better terminal compatibility
    // This helps with terminals that don't support 24-bit color
    match (r, g, b) {
        (0, 0, 0) => Ok(Color::Black),
        (128, 0, 0) => Ok(Color::Red),            // darkred -> Red
        (0, 128, 0) => Ok(Color::Green),          // darkgreen -> Green
        (128, 128, 0) => Ok(Color::Yellow),       // darkyellow -> Yellow
        (0, 0, 128) => Ok(Color::Blue),           // darkblue -> Blue
        (128, 0, 128) => Ok(Color::Magenta),      // darkmagenta -> Magenta
        (0, 128, 128) => Ok(Color::Cyan),         // darkcyan -> Cyan
        (192, 192, 192) => Ok(Color::Gray),       // silver -> Gray
        (128, 128, 128) => Ok(Color::DarkGray),   // gray -> DarkGray
        (255, 0, 0) => Ok(Color::LightRed),       // red -> LightRed
        (0, 255, 0) => Ok(Color::LightGreen),     // lime -> LightGreen
        (255, 255, 0) => Ok(Color::LightYellow),  // yellow -> LightYellow
        (0, 0, 255) => Ok(Color::LightBlue),      // blue -> LightBlue
        (255, 0, 255) => Ok(Color::LightMagenta), // magenta -> LightMagenta
        (0, 255, 255) => Ok(Color::LightCyan),    // cyan/aqua -> LightCyan
        (255, 255, 255) => Ok(Color::White),
        _ => Ok(Color::Rgb(r, g, b)), // Use RGB for all other colors
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_named_colors() {
        // Basic colors - csscolorparser maps these to RGB values
        // "red" is RGB(255,0,0) which maps to LightRed
        assert_eq!(parse_color("red").unwrap(), Color::LightRed);
        // "green" is RGB(0,128,0) which maps to Green
        assert_eq!(parse_color("green").unwrap(), Color::Green);
        // "blue" is RGB(0,0,255) which maps to LightBlue
        assert_eq!(parse_color("blue").unwrap(), Color::LightBlue);
        // "black" maps to Black
        assert_eq!(parse_color("black").unwrap(), Color::Black);
        // "white" maps to White
        assert_eq!(parse_color("white").unwrap(), Color::White);
    }

    #[test]
    fn test_parse_extended_color_names() {
        // Test extended CSS/X11 color names that csscolorparser supports
        let dodgerblue = parse_color("dodgerblue").unwrap();
        if let Color::Rgb(r, g, b) = dodgerblue {
            assert_eq!(r, 30);
            assert_eq!(g, 144);
            assert_eq!(b, 255);
        } else {
            panic!("Expected Rgb color for dodgerblue");
        }

        let steelblue = parse_color("steelblue").unwrap();
        if let Color::Rgb(r, g, b) = steelblue {
            assert_eq!(r, 70);
            assert_eq!(g, 130);
            assert_eq!(b, 180);
        } else {
            panic!("Expected Rgb color for steelblue");
        }
    }

    #[test]
    fn test_parse_hex_colors() {
        // Pure red (#ff0000) maps to LightRed based on our color mapping
        let color = parse_color("#ff0000").unwrap();
        assert_eq!(color, Color::LightRed);

        // Test a color that doesn't map to a built-in color
        let color = parse_color("#123456").unwrap();
        if let Color::Rgb(r, g, b) = color {
            assert_eq!(r, 0x12);
            assert_eq!(g, 0x34);
            assert_eq!(b, 0x56);
        } else {
            panic!("Expected Rgb color for #123456");
        }
    }

    #[test]
    fn test_parse_short_hex() {
        // Short hex #f00 expands to #ff0000 which maps to LightRed
        let color = parse_color("#f00").unwrap();
        assert_eq!(color, Color::LightRed);

        // Test a color that doesn't map to a built-in color
        let color = parse_color("#abc").unwrap();
        if let Color::Rgb(r, g, b) = color {
            assert_eq!(r, 0xaa);
            assert_eq!(g, 0xbb);
            assert_eq!(b, 0xcc);
        } else {
            panic!("Expected Rgb color for #abc");
        }
    }

    #[test]
    fn test_parse_hex_without_hash() {
        // Hex without hash also works
        let color = parse_color("ff0000").unwrap();
        assert_eq!(color, Color::LightRed);

        // Test a color that doesn't map to a built-in color
        let color = parse_color("123456").unwrap();
        if let Color::Rgb(r, g, b) = color {
            assert_eq!(r, 0x12);
            assert_eq!(g, 0x34);
            assert_eq!(b, 0x56);
        } else {
            panic!("Expected Rgb color for 123456");
        }
    }

    #[test]
    fn test_parse_default() {
        assert_eq!(parse_color("default").unwrap(), Color::Reset);
        assert_eq!(parse_color("DEFAULT").unwrap(), Color::Reset);
    }

    #[test]
    fn test_parse_invalid_color() {
        assert!(parse_color("notacolor").is_err());
        assert!(parse_color("#gggggg").is_err());
    }
}
