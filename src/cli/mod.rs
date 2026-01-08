//! CLI command handling module
//!
//! Handles all CLI subcommands and argument parsing.

mod config;
mod logging;
mod plugin;
mod version;

pub use config::{ConfigSubcommand, handle_config_command};
pub use logging::*;
pub use plugin::{PluginSubcommand, handle_plugin_command};
pub use version::display_version;
