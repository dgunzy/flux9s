//! Configuration system for flux9s
//!
//! This module provides a comprehensive configuration system modeled after k9s,
//! supporting multiple configuration layers, theme management, and persistent settings.

mod defaults;
pub mod loader;
pub mod paths;
pub mod schema;
pub mod theme_loader;

pub use loader::ConfigLoader;
#[allow(unused_imports)] // Public API exports - may be used by external code
pub use schema::Config;
#[allow(unused_imports)] // Public API exports - may be used by external code
pub use schema::LoggerConfig;
#[allow(unused_imports)] // Public API exports - may be used by external code
pub use schema::UiConfig;
pub use theme_loader::ThemeLoader;
