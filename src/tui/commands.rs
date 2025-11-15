//! Command registry for TUI commands
//!
//! Provides an extensible system for registering and handling TUI commands.
//! Commands are executed via the `:command` syntax in the TUI.

use std::collections::HashMap;

/// Command handler function type
pub type CommandHandler = fn(&mut CommandContext) -> CommandResult;

/// Context passed to command handlers
pub struct CommandContext {
    /// Command arguments (everything after the command name)
    pub args: Vec<String>,
    /// App state (will be passed through)
    pub app_state: *mut (),
}

/// Result of command execution
#[derive(Debug, Clone)]
pub enum CommandResult {
    /// Command executed successfully, continue
    Continue,
    /// Command executed successfully, quit application
    Quit,
    /// Command not found or invalid
    NotFound,
    /// Command executed with a status message
    Status(String, bool), // (message, is_error)
}

/// Command registry
pub struct CommandRegistry {
    commands: HashMap<String, CommandInfo>,
}

/// Information about a command
pub struct CommandInfo {
    /// Command name
    pub name: String,
    /// Aliases for the command
    pub aliases: Vec<String>,
    /// Description for help text
    pub description: String,
    /// Usage example
    pub usage: Option<String>,
}

impl CommandRegistry {
    /// Create a new command registry with built-in commands
    pub fn new() -> Self {
        let mut registry = Self {
            commands: HashMap::new(),
        };

        // Register built-in commands
        registry.register_builtin_commands();

        registry
    }

    /// Register a built-in command
    fn register_builtin_commands(&mut self) {
        // Help command
        self.commands.insert(
            "help".to_string(),
            CommandInfo {
                name: "help".to_string(),
                aliases: vec!["h".to_string(), "?".to_string()],
                description: "Show help information".to_string(),
                usage: Some(":help".to_string()),
            },
        );

        // Readonly command
        self.commands.insert(
            "readonly".to_string(),
            CommandInfo {
                name: "readonly".to_string(),
                aliases: vec!["read-only".to_string()],
                description: "Toggle readonly mode (prevents modification operations)".to_string(),
                usage: Some(":readonly".to_string()),
            },
        );

        // Quit command
        self.commands.insert(
            "quit".to_string(),
            CommandInfo {
                name: "quit".to_string(),
                aliases: vec!["q".to_string(), "exit".to_string()],
                description: "Quit the application".to_string(),
                usage: Some(":q or :quit".to_string()),
            },
        );

        // Namespace command
        self.commands.insert(
            "namespace".to_string(),
            CommandInfo {
                name: "namespace".to_string(),
                aliases: vec!["ns".to_string()],
                description: "Switch namespace".to_string(),
                usage: Some(":ns <name> or :ns all".to_string()),
            },
        );

        // All/Clear command
        self.commands.insert(
            "all".to_string(),
            CommandInfo {
                name: "all".to_string(),
                aliases: vec!["clear".to_string()],
                description: "Show all resources (clear filters)".to_string(),
                usage: Some(":all or :clear".to_string()),
            },
        );
    }

    /// Get command info by name or alias
    pub fn get_command(&self, name: &str) -> Option<&CommandInfo> {
        // Try exact match first
        if let Some(cmd) = self.commands.get(name) {
            return Some(cmd);
        }

        // Try aliases
        self.commands
            .values()
            .find(|cmd| cmd.name == name || cmd.aliases.iter().any(|a| a == name))
    }

    /// Get all commands for help display
    pub fn get_all_commands(&self) -> Vec<&CommandInfo> {
        let mut commands: Vec<&CommandInfo> = self.commands.values().collect();
        commands.sort_by_key(|c| &c.name);
        commands
    }

    /// Find commands matching a prefix (for autocomplete)
    pub fn find_matching(&self, prefix: &str) -> Vec<&CommandInfo> {
        let prefix_lower = prefix.to_lowercase();
        self.commands
            .values()
            .filter(|cmd| {
                cmd.name.to_lowercase().starts_with(&prefix_lower)
                    || cmd
                        .aliases
                        .iter()
                        .any(|a| a.to_lowercase().starts_with(&prefix_lower))
            })
            .collect()
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_registry() {
        let registry = CommandRegistry::new();
        assert!(registry.get_command("help").is_some());
        assert!(registry.get_command("h").is_some());
        assert!(registry.get_command("readonly").is_some());
    }

    #[test]
    fn test_find_matching() {
        let registry = CommandRegistry::new();
        let matches = registry.find_matching("he");
        assert!(matches.iter().any(|c| c.name == "help"));
    }
}
