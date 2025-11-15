//! Theme loading and management
//!
//! Handles loading themes from YAML files following k9s-style skin format.

use super::paths;
use crate::tui::Theme;
use anyhow::{Context, Result};
use ratatui::style::Color;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
    /// Load a theme by name
    ///
    /// Resolution order:
    /// 1. User skins directory ($XDG_DATA_HOME/flux9s/skins/{name}.yaml)
    /// 2. System skins directory ($XDG_CONFIG_HOME/flux9s/skins/{name}.yaml)
    /// 3. Built-in default theme
    pub fn load_theme(name: &str) -> Result<Theme> {
        // Try user skins directory first
        let user_skin_path = paths::user_skins_dir().join(format!("{}.yaml", name));
        if user_skin_path.exists() {
            return Self::load_from_file(&user_skin_path);
        }

        // Try system skins directory
        let system_skin_path = paths::skins_dir().join(format!("{}.yaml", name));
        if system_skin_path.exists() {
            return Self::load_from_file(&system_skin_path);
        }

        // Fall back to default theme
        if name == "default" {
            return Ok(Theme::default());
        }

        // If theme not found and not "default", try loading "default" first, then apply overrides
        // For now, just return default
        tracing::warn!("Theme '{}' not found, using default theme", name);
        Ok(Theme::default())
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

    /// List available themes
    pub fn list_themes() -> Vec<String> {
        let mut themes = Vec::new();

        // Check user skins directory
        if let Ok(entries) = std::fs::read_dir(paths::user_skins_dir()) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if name.ends_with(".yaml") {
                        themes.push(name.trim_end_matches(".yaml").to_string());
                    }
                }
            }
        }

        // Check system skins directory
        if let Ok(entries) = std::fs::read_dir(paths::skins_dir()) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if name.ends_with(".yaml") {
                        let theme_name = name.trim_end_matches(".yaml").to_string();
                        if !themes.contains(&theme_name) {
                            themes.push(theme_name);
                        }
                    }
                }
            }
        }

        // Always include default
        if !themes.contains(&"default".to_string()) {
            themes.insert(0, "default".to_string());
        }

        themes.sort();
        themes
    }
}

/// Parse a color string to ratatui Color
///
/// Supports:
/// - Hex colors: #ffffff, #fff
/// - Named colors: white, black, red, green, blue, yellow, cyan, magenta
/// - Special: "default" returns a default color
fn parse_color(color_str: &str) -> Result<Color> {
    let color_str = color_str.trim().to_lowercase();

    // Handle "default" special case
    if color_str == "default" {
        return Ok(Color::Reset);
    }

    // Handle hex colors
    if color_str.starts_with('#') {
        return parse_hex_color(&color_str);
    }

    // Handle named colors
    match color_str.as_str() {
        "black" => Ok(Color::Black),
        "red" => Ok(Color::Red),
        "green" => Ok(Color::Green),
        "yellow" => Ok(Color::Yellow),
        "blue" => Ok(Color::Blue),
        "magenta" => Ok(Color::Magenta),
        "cyan" => Ok(Color::Cyan),
        "white" => Ok(Color::White),
        "gray" | "grey" => Ok(Color::Gray),
        "darkgray" | "darkgrey" => Ok(Color::DarkGray),
        "lightred" => Ok(Color::LightRed),
        "lightgreen" => Ok(Color::LightGreen),
        "lightyellow" => Ok(Color::LightYellow),
        "lightblue" => Ok(Color::LightBlue),
        "lightmagenta" => Ok(Color::LightMagenta),
        "lightcyan" => Ok(Color::LightCyan),
        _ => {
            // Try parsing as hex without #
            if color_str.len() == 6 || color_str.len() == 3 {
                parse_hex_color(&format!("#{}", color_str))
            } else {
                Err(anyhow::anyhow!("Unknown color: {}", color_str))
            }
        }
    }
}

/// Parse hex color string (#RRGGBB or #RGB)
fn parse_hex_color(hex: &str) -> Result<Color> {
    let hex = hex.trim_start_matches('#');
    let (r, g, b) = if hex.len() == 6 {
        let r = u8::from_str_radix(&hex[0..2], 16)
            .map_err(|_| anyhow::anyhow!("Invalid hex color: {}", hex))?;
        let g = u8::from_str_radix(&hex[2..4], 16)
            .map_err(|_| anyhow::anyhow!("Invalid hex color: {}", hex))?;
        let b = u8::from_str_radix(&hex[4..6], 16)
            .map_err(|_| anyhow::anyhow!("Invalid hex color: {}", hex))?;
        (r, g, b)
    } else if hex.len() == 3 {
        let r = u8::from_str_radix(&hex[0..1], 16)
            .map_err(|_| anyhow::anyhow!("Invalid hex color: {}", hex))?;
        let g = u8::from_str_radix(&hex[1..2], 16)
            .map_err(|_| anyhow::anyhow!("Invalid hex color: {}", hex))?;
        let b = u8::from_str_radix(&hex[2..3], 16)
            .map_err(|_| anyhow::anyhow!("Invalid hex color: {}", hex))?;
        // Expand short hex: #abc -> #aabbcc
        (r * 17, g * 17, b * 17)
    } else {
        return Err(anyhow::anyhow!("Invalid hex color length: {}", hex));
    };

    Ok(Color::Rgb(r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_named_colors() {
        assert_eq!(parse_color("red").unwrap(), Color::Red);
        assert_eq!(parse_color("green").unwrap(), Color::Green);
        assert_eq!(parse_color("blue").unwrap(), Color::Blue);
    }

    #[test]
    fn test_parse_hex_colors() {
        let color = parse_color("#ff0000").unwrap();
        if let Color::Rgb(r, g, b) = color {
            assert_eq!(r, 255);
            assert_eq!(g, 0);
            assert_eq!(b, 0);
        } else {
            panic!("Expected Rgb color");
        }
    }

    #[test]
    fn test_parse_short_hex() {
        let color = parse_color("#f00").unwrap();
        if let Color::Rgb(r, g, b) = color {
            assert_eq!(r, 255);
            assert_eq!(g, 0);
            assert_eq!(b, 0);
        } else {
            panic!("Expected Rgb color");
        }
    }
}
