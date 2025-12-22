//! CLI command handling module
//!
//! Handles all CLI subcommands and argument parsing.

mod config;
mod logging;
mod version;

pub use config::{ConfigSubcommand, handle_config_command};
pub use logging::*;
pub use version::display_version;
