//! Command registry and handling for flux9s commands
//!
//! Centralizes command definitions, autocomplete, and execution logic
//! to keep app.rs focused on application state management.

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
/// Returns commands sorted by priority (CRD commands first, then App commands)
/// and then alphabetically within each category.
/// Commands that take arguments are returned with a trailing space (e.g., "skin ").
pub fn find_matching_commands(prefix: &str) -> Vec<String> {
    let prefix_lower = prefix.to_lowercase();
    let mut crd_matches: Vec<String> = Vec::new();
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
    app_matches.sort();

    // Combine: CRD commands first (higher priority), then app commands
    let mut all_matches = crd_matches;
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
