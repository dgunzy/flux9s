//! CLI command handling module
//!
//! Handles all CLI subcommands and argument parsing.

mod config;
mod logging;
mod version;

pub use config::{handle_config_command, ConfigSubcommand};
pub use logging::*;
pub use version::display_version;
