//! Centralized keybindings and navigation commands
//!
//! Footer hint type and layout helpers. The keys themselves (dispatch,
//! footer layouts, help) live in the action table, `app/actions.rs`.

use ratatui::style::Color;

/// Navigation command with keybinding and label
#[derive(Debug, Clone)]
pub struct NavigationCommand {
    /// The keybinding string (e.g., "j/k ", "y", "Enter")
    pub key: &'static str,
    /// The human-readable label (e.g., "Navigate", "YAML")
    pub label: &'static str,
}

impl NavigationCommand {
    /// Create a new navigation command
    pub const fn new(key: &'static str, label: &'static str) -> Self {
        Self { key, label }
    }
}

/// Footer hints of the Flux resource list (every action in its layout
/// applies there). Built from the action table in `app/actions.rs`, the
/// single source for keys, footer, and help.
pub fn get_navigation_commands() -> Vec<NavigationCommand> {
    use crate::tui::app::actions::{FLUX_LAYOUT, layout_hints};
    layout_hints(FLUX_LAYOUT)
}

/// Footer hints before a connection is established.
pub fn get_connection_error_commands() -> Vec<NavigationCommand> {
    use crate::tui::app::actions::{CONNECTION_ERROR_LAYOUT, layout_hints};
    layout_hints(CONNECTION_ERROR_LAYOUT)
}

/// Convert navigation commands to segments with color for footer rendering
pub fn navigation_commands_to_segments(
    commands: &[NavigationCommand],
    color: Color,
) -> Vec<(String, String, Color)> {
    commands
        .iter()
        .map(|cmd| (cmd.key.to_string(), cmd.label.to_string(), color))
        .collect()
}

/// Convert navigation commands to segments without color (for height calculation)
pub fn navigation_commands_to_segments_simple(
    commands: &[NavigationCommand],
) -> Vec<(String, String)> {
    commands
        .iter()
        .map(|cmd| (cmd.key.to_string(), cmd.label.to_string()))
        .collect()
}

/// Calculate footer height based on navigation segments
///
/// This function uses the exact same logic as `render_navigation_footer` in footer.rs
/// to ensure consistent height calculations.
pub fn calculate_footer_height(terminal_width: u16, commands: &[NavigationCommand]) -> u16 {
    let nav_segments = navigation_commands_to_segments_simple(commands);

    let footer_available_width = terminal_width.saturating_sub(2); // Account for borders

    // Calculate segment lengths (matching footer.rs:229-238)
    let mut segment_lengths: Vec<usize> = Vec::new();
    for (idx, (key, label)) in nav_segments.iter().enumerate() {
        let separator_len = if idx > 0 { 3 } else { 0 }; // " | "
        let segment_len = if key == "j/k " {
            key.len() + label.len()
        } else {
            key.len() + 1 + label.len() // key + space + label
        };
        segment_lengths.push(separator_len + segment_len);
    }

    // Split segments into two lines (matching footer.rs:240-265)
    let mut line1_length = 0;
    let mut use_line2 = false;

    for (idx, _) in nav_segments.iter().enumerate() {
        let segment_len = segment_lengths[idx];

        // If adding this segment would exceed width and we're on line 1, start line 2
        if line1_length + segment_len > footer_available_width as usize
            && !use_line2
            && line1_length > 0
        {
            use_line2 = true;
            break; // We've determined we need 2 lines, no need to continue
        }

        if !use_line2 {
            line1_length += segment_len;
        }
    }

    // Calculate number of lines needed (1 or 2)
    let footer_content_lines: u16 = if use_line2 { 2 } else { 1 };

    footer_content_lines + 2 // Content + borders
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_footer_height_connection_error() {
        // Under a connection error state, the footer has fewer options and fits on one line.
        let height = calculate_footer_height(80, &get_connection_error_commands());
        assert_eq!(height, 3); // 1 content line + 2 borders
    }

    #[test]
    fn test_calculate_footer_height_normal() {
        // Under normal circumstances, check that it calculates correctly.
        let height = calculate_footer_height(80, &get_navigation_commands());
        // Normally at 80 cols, the default commands (lots of them) wrap to 2 lines.
        assert_eq!(height, 4); // 2 content lines + 2 borders
    }
}
