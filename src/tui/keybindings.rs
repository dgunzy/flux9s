//! Centralized keybindings and navigation commands
//!
//! This module provides a single source of truth for all keybindings
//! used in the footer, help view, and layout calculations.

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

/// Get all default navigation commands in the order they should appear
pub fn get_navigation_commands() -> Vec<NavigationCommand> {
    // Order matches original footer.rs to maintain snapshot compatibility
    vec![
        NavigationCommand::new("j/k ", "Navigate"),
        NavigationCommand::new(":", "Command"),
        NavigationCommand::new("Enter", "Details"),
        NavigationCommand::new("/", "Filter(Name)"),
        NavigationCommand::new("s", "Suspend"),
        NavigationCommand::new("r", "Resume"),
        NavigationCommand::new("R", "Reconcile"),
        NavigationCommand::new("y", "YAML"),
        NavigationCommand::new("f", "Favorite"),
        NavigationCommand::new("g", "Graph"),
        NavigationCommand::new("h", "History"),
        NavigationCommand::new("t", "Trace"),
        NavigationCommand::new("W", "Reconcile+Source"),
        NavigationCommand::new("d", "Delete"),
        NavigationCommand::new("?", "Help"),
        NavigationCommand::new("Esc", "Back/Quit"),
    ]
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

/// Get resource-specific commands for help view
///
/// Returns commands that are resource-specific (excluding navigation, command mode, filter, help, quit)
/// in the order they should appear in the help view, with help-specific descriptions.
pub fn get_resource_help_commands() -> Vec<(&'static str, &'static str)> {
    let commands = get_navigation_commands();
    commands
        .iter()
        .filter_map(|cmd| {
            // Map resource-specific commands to help format
            match cmd.key {
                "Enter" => Some(("<Enter>", "View resource details")),
                "s" => Some(("<s>", "Suspend reconciliation")),
                "r" => Some(("<r>", "Resume reconciliation")),
                "R" => Some(("<R>", "Reconcile resource")),
                "y" => Some(("<y>", "View YAML manifest")),
                "f" => Some(("<f>", "Toggle favorite")),
                "g" => Some(("<g>", "View resource graph")),
                "h" => Some(("<h>", "View reconciliation history")),
                "t" => Some(("<t>", "Trace ownership chain")),
                "W" => Some(("<W>", "Reconcile with source")),
                "d" => Some(("<d>", "Delete resource")),
                _ => None, // Skip non-resource commands (j/k, :, /, ?, Esc)
            }
        })
        .collect()
}

/// Calculate footer height based on navigation segments and namespace hotkeys
///
/// This function uses the exact same logic as `render_navigation_footer` in footer.rs
/// to ensure consistent height calculations.
pub fn calculate_footer_height(
    terminal_width: u16,
    namespace_hotkeys: &[String],
    current_namespace: &Option<String>,
) -> u16 {
    use crate::tui::constants::{MAX_FOOTER_NAMESPACE_HOTKEYS, MAX_FOOTER_NAMESPACE_LENGTH};

    // Get base navigation commands
    let mut nav_segments = navigation_commands_to_segments_simple(&get_navigation_commands());

    // Add namespace hotkeys (matching footer.rs logic)
    if !namespace_hotkeys.is_empty() {
        for (idx, ns) in namespace_hotkeys
            .iter()
            .take(MAX_FOOTER_NAMESPACE_HOTKEYS)
            .enumerate()
        {
            let display_ns = if ns == "all" {
                "all".to_string()
            } else if ns.len() > MAX_FOOTER_NAMESPACE_LENGTH {
                ns[..MAX_FOOTER_NAMESPACE_LENGTH].to_string()
            } else {
                ns.clone()
            };
            let label = if (ns == "all" && current_namespace.is_none())
                || current_namespace.as_ref() == Some(ns)
            {
                format!("NS:{}*", display_ns)
            } else {
                format!("NS:{}", display_ns)
            };
            nav_segments.push((idx.to_string(), label));
        }
    }

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
