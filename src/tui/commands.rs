//! Command registry and handling for flux9s commands
//!
//! Centralizes command definitions, autocomplete, and execution logic
//! to keep app.rs focused on application state management.

use crate::tui::submenu::{CommandSubmenu, SubmenuItem, SubmenuState};
use anyhow::Result;

/// Command definition
#[derive(Debug, Clone)]
pub struct Command {
    /// Command name/alias
    pub name: &'static str,
    /// Whether this command takes arguments
    pub takes_args: bool,
}

/// Application commands (non-CRD commands)
pub const APP_COMMANDS: &[Command] = &[
    Command {
        name: "healthy",
        takes_args: false,
    },
    Command {
        name: "unhealthy",
        takes_args: false,
    },
    Command {
        name: "readonly",
        takes_args: false,
    },
    Command {
        name: "read-only",
        takes_args: false,
    },
    Command {
        name: "help",
        takes_args: false,
    },
    Command {
        name: "quit",
        takes_args: false,
    },
    Command {
        name: "exit",
        takes_args: false,
    },
    Command {
        name: "skin",
        takes_args: true,
    },
    Command {
        name: "trace",
        takes_args: true,
    },
    Command {
        name: "context",
        takes_args: true,
    },
    Command {
        name: "namespace",
        takes_args: true,
    },
    Command {
        name: "favorites",
        takes_args: false,
    },
    Command {
        name: "fav",
        takes_args: false,
    },
];

/// Find all commands that match the given prefix
///
/// Returns commands sorted by priority (CRD commands first, then Plugin commands, then App commands)
/// and then alphabetically within each category.
/// Commands that take arguments are returned with a trailing space (e.g., "skin ").
pub fn find_matching_commands(
    prefix: &str,
    plugin_registry: Option<&crate::plugins::PluginRegistry>,
) -> Vec<String> {
    let prefix_lower = prefix.to_lowercase();
    let mut crd_matches: Vec<String> = Vec::new();
    let mut plugin_matches: Vec<String> = Vec::new();
    let mut app_matches: Vec<String> = Vec::new();

    // Get CRD commands from registry
    let crd_commands = crate::watcher::get_all_commands();
    for (_, aliases) in crd_commands {
        for alias in aliases.iter() {
            if alias.starts_with(&prefix_lower) {
                crd_matches.push((*alias).to_string());
            }
        }
    }

    // Get plugin watched resource commands
    if let Some(registry) = plugin_registry {
        for (cmd, _display_name) in registry.get_watched_resource_commands() {
            let cmd_lower = cmd.to_lowercase();
            if cmd_lower.starts_with(&prefix_lower) {
                plugin_matches.push(cmd);
            }
        }
    }

    // Get app commands
    for cmd in APP_COMMANDS {
        let cmd_name_lower = cmd.name.to_lowercase();
        if cmd.takes_args {
            // For commands with args, check if prefix matches the command part
            let full_cmd = format!("{} ", cmd.name);
            if full_cmd.starts_with(&prefix_lower) && prefix_lower != full_cmd {
                if prefix_lower.len() <= cmd.name.len() {
                    // Return command with space for autocomplete
                    app_matches.push(full_cmd.clone());
                }
            } else if prefix_lower == cmd_name_lower {
                // Exact match - return with space
                app_matches.push(full_cmd.clone());
            }
        } else {
            // Simple command - check if prefix matches
            if cmd_name_lower.starts_with(&prefix_lower) {
                app_matches.push(cmd.name.to_string());
            }
        }
    }

    // Sort matches alphabetically
    crd_matches.sort();
    plugin_matches.sort();
    app_matches.sort();

    // Combine: CRD commands first (higher priority), then plugin commands, then app commands
    let mut all_matches = crd_matches;
    all_matches.extend(plugin_matches);
    all_matches.extend(app_matches);
    all_matches
}

// Command matching helpers - use these instead of hardcoding command strings

/// Check if command is readonly (handles both "readonly" and "read-only")
pub fn is_readonly_command(cmd: &str) -> bool {
    let cmd_lower = cmd.to_lowercase();
    cmd_lower == "readonly" || cmd_lower == "read-only"
}

/// Check if command is help (handles "help", "h", "?")
pub fn is_help_command(cmd: &str) -> bool {
    let cmd_lower = cmd.to_lowercase();
    cmd_lower == "help" || cmd_lower == "h" || cmd_lower == "?"
}

/// Check if command is quit (handles "q", "q!", "quit", "exit")
pub fn is_quit_command(cmd: &str) -> bool {
    let cmd_lower = cmd.to_lowercase();
    cmd_lower == "q" || cmd_lower == "q!" || cmd_lower == "quit" || cmd_lower == "exit"
}

/// Check if command is healthy filter
pub fn is_healthy_command(cmd: &str) -> bool {
    cmd.to_lowercase() == "healthy"
}

/// Check if command is unhealthy filter
pub fn is_unhealthy_command(cmd: &str) -> bool {
    cmd.to_lowercase() == "unhealthy"
}

/// Check if command is "all" or "clear"
pub fn is_all_command(cmd: &str) -> bool {
    let cmd_lower = cmd.to_lowercase();
    cmd_lower == "all" || cmd_lower == "clear"
}

/// Check if command is skin command (with optional args)
pub fn is_skin_command(cmd: &str) -> bool {
    let cmd_lower = cmd.to_lowercase();
    cmd_lower == "skin" || cmd_lower.starts_with("skin ")
}

/// Check if command is trace command (with optional args)
pub fn is_trace_command(cmd: &str) -> bool {
    let cmd_lower = cmd.to_lowercase();
    cmd_lower == "trace" || cmd_lower.starts_with("trace ")
}

/// Check if command is context command (handles both "ctx" and "context")
pub fn is_context_command(cmd: &str) -> bool {
    let cmd_lower = cmd.to_lowercase();
    cmd_lower == "ctx"
        || cmd_lower.starts_with("ctx ")
        || cmd_lower == "context"
        || cmd_lower.starts_with("context ")
}

/// Check if command is namespace command (handles both "ns" and "namespace")
pub fn is_namespace_command(cmd: &str) -> bool {
    let cmd_lower = cmd.to_lowercase();
    cmd_lower == "ns"
        || cmd_lower.starts_with("ns ")
        || cmd_lower == "namespace"
        || cmd_lower.starts_with("namespace ")
}

/// Check if command is favorites command (handles both "favorites" and "fav")
pub fn is_favorites_command(cmd: &str) -> bool {
    let cmd_lower = cmd.to_lowercase();
    cmd_lower == "favorites" || cmd_lower == "fav"
}

/// Get plugin watched resource display name for a command
///
/// Returns the display name if the command matches a plugin watched resource
pub fn get_plugin_display_name_for_command(
    cmd: &str,
    plugin_registry: Option<&crate::plugins::PluginRegistry>,
) -> Option<String> {
    let registry = plugin_registry?;
    let (_, watched) = registry.get_watched_resource_for_command(cmd)?;
    Some(watched.display_name().to_string())
}

/// Extract argument from a command that takes arguments
/// Returns None if command doesn't match or has no argument
pub fn extract_command_arg(cmd: &str, command_name: &str) -> Option<String> {
    let cmd_lower = cmd.to_lowercase();
    let command_lower = command_name.to_lowercase();

    // Check exact match or starts with command + space
    if cmd_lower == command_lower {
        None // No argument provided
    } else if cmd_lower.starts_with(&format!("{} ", command_lower)) {
        // Extract everything after the command and space
        let arg = cmd
            .split_whitespace()
            .skip(1)
            .collect::<Vec<&str>>()
            .join(" ");
        if arg.is_empty() { None } else { Some(arg) }
    } else {
        None // Command doesn't match
    }
}

/// Context submenu provider for the :ctx command
pub struct ContextSubmenu {
    /// Current context name
    pub current_context: String,
}

impl ContextSubmenu {
    /// Create a new context submenu provider
    pub fn new(current_context: String) -> Self {
        Self { current_context }
    }
}

impl CommandSubmenu for ContextSubmenu {
    fn get_submenu(&self) -> Result<Option<SubmenuState>> {
        // Get available contexts from kubeconfig
        let contexts = crate::kube::list_contexts()?;

        if contexts.is_empty() {
            return Ok(None);
        }

        // Create submenu items, marking current context
        let items: Vec<SubmenuItem> = contexts
            .into_iter()
            .map(|ctx| {
                let display = if ctx == self.current_context {
                    format!("{} (current)", ctx)
                } else {
                    ctx.clone()
                };
                SubmenuItem::with_display(ctx, display)
            })
            .collect();

        // Create submenu state with title and help text
        let state = SubmenuState::new("ctx".to_string(), items)
            .with_title("Select Context".to_string())
            .with_help("j/k: Navigate | Enter: Select | Esc: Cancel".to_string());

        Ok(Some(state))
    }
}

/// Theme submenu provider for the :skin command
pub struct ThemeSubmenu {
    /// Current theme name
    pub current_theme: String,
}

impl ThemeSubmenu {
    /// Create a new theme submenu provider
    pub fn new(current_theme: String) -> Self {
        Self { current_theme }
    }
}

impl CommandSubmenu for ThemeSubmenu {
    fn get_submenu(&self) -> Result<Option<SubmenuState>> {
        // Get available themes (includes embedded + user-installed)
        let themes = crate::config::theme_loader::ThemeLoader::list_themes();

        if themes.is_empty() {
            return Ok(None);
        }

        // Create submenu items, marking current theme
        let items: Vec<SubmenuItem> = themes
            .into_iter()
            .map(|theme| {
                let display = if theme == self.current_theme {
                    format!("{} (current)", theme)
                } else {
                    theme.clone()
                };
                // Mark embedded themes
                let display = if crate::config::embedded_themes::is_embedded_theme(&theme) {
                    format!("{} [built-in]", display)
                } else {
                    display
                };
                SubmenuItem::with_display(theme, display)
            })
            .collect();

        // Create submenu state with title and help text
        let state = SubmenuState::new("skin".to_string(), items)
            .with_title("Select Theme".to_string())
            .with_help("j/k: Navigate | Enter: Apply | s: Save | Esc: Cancel".to_string());

        Ok(Some(state))
    }
}

/// Get submenu for a command if it supports submenus
///
/// Returns None if the command doesn't support submenus or if an argument was provided.
pub fn get_command_submenu(
    cmd: &str,
    current_context: &str,
    current_theme: &str,
) -> Option<SubmenuState> {
    // Only show submenu if command has no arguments
    if is_context_command(cmd) {
        let arg = extract_command_arg(cmd, "ctx").or_else(|| extract_command_arg(cmd, "context"));

        // If argument provided, don't show submenu (preserve existing behavior)
        if arg.is_some() {
            return None;
        }

        // Create context submenu
        let submenu_provider = ContextSubmenu::new(current_context.to_string());
        match submenu_provider.get_submenu() {
            Ok(Some(state)) => Some(state),
            _ => None,
        }
    } else if is_skin_command(cmd) {
        let arg = extract_command_arg(cmd, "skin");

        // If argument provided, don't show submenu (preserve existing behavior)
        if arg.is_some() {
            return None;
        }

        // Create theme submenu
        let submenu_provider = ThemeSubmenu::new(current_theme.to_string());
        match submenu_provider.get_submenu() {
            Ok(Some(state)) => Some(state),
            _ => None,
        }
    } else {
        // Other commands don't support submenus yet
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_theme_submenu_creation() {
        let submenu = ThemeSubmenu::new("default".to_string());
        let result = submenu.get_submenu();
        assert!(result.is_ok(), "Should create theme submenu successfully");

        let submenu_state = result.unwrap();
        assert!(submenu_state.is_some(), "Should have submenu state");

        let state = submenu_state.unwrap();
        assert_eq!(state.command, "skin");
        assert_eq!(state.title, Some("Select Theme".to_string()));
        assert!(!state.items.is_empty(), "Should have theme items");
    }

    #[test]
    fn test_theme_submenu_marks_current() {
        let current_theme = "default".to_string();
        let submenu = ThemeSubmenu::new(current_theme.clone());
        let result = submenu.get_submenu().unwrap();

        assert!(result.is_some());
        let state = result.unwrap();

        // Find the current theme item
        let current_item = state.items.iter().find(|item| item.value == current_theme);

        assert!(current_item.is_some(), "Should find current theme in items");
        let item = current_item.unwrap();
        assert!(
            item.display_text.contains("(current)"),
            "Current theme should be marked"
        );
    }

    #[test]
    fn test_theme_submenu_marks_embedded() {
        // Use an embedded theme as current
        let current_theme = "dracula".to_string();
        let submenu = ThemeSubmenu::new(current_theme.clone());
        let result = submenu.get_submenu().unwrap();

        assert!(result.is_some());
        let state = result.unwrap();

        // Find dracula theme item
        if let Some(item) = state.items.iter().find(|item| item.value == "dracula") {
            assert!(
                item.display_text.contains("[built-in]"),
                "Embedded theme should be marked as [built-in]"
            );
        }
    }

    #[test]
    fn test_theme_submenu_includes_embedded_themes() {
        let submenu = ThemeSubmenu::new("default".to_string());
        let result = submenu.get_submenu().unwrap();

        assert!(result.is_some());
        let state = result.unwrap();

        // Should include at least some embedded themes
        let theme_names: Vec<&String> = state.items.iter().map(|item| &item.value).collect();
        let dracula_str = "dracula".to_string();
        let nord_str = "nord".to_string();
        assert!(
            theme_names.contains(&&dracula_str) || theme_names.contains(&&nord_str),
            "Should include embedded themes"
        );
    }

    #[test]
    fn test_get_command_submenu_skin_no_arg() {
        // Test that skin without args returns submenu
        // Note: commands are processed without the ':' prefix
        let result = get_command_submenu("skin", "context1", "default");
        assert!(
            result.is_some(),
            "Should return submenu for skin without args"
        );

        let submenu = result.unwrap();
        assert_eq!(submenu.command, "skin");
    }

    #[test]
    fn test_get_command_submenu_skin_with_arg() {
        // Test that skin with args doesn't return submenu
        let result = get_command_submenu("skin dracula", "context1", "default");
        assert!(
            result.is_none(),
            "Should not return submenu when arg provided"
        );
    }

    #[test]
    fn test_get_command_submenu_skin_variations() {
        // Test various skin command formats (without ':' prefix as commands are processed)
        assert!(get_command_submenu("skin", "context1", "default").is_some());
        assert!(get_command_submenu("skin ", "context1", "default").is_some());
        assert!(get_command_submenu("skin dracula", "context1", "default").is_none());
    }
}
