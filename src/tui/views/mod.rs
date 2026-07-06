//! TUI view components
//!
//! This module contains all the rendering components for different views
//! in the TUI. Each component is responsible for rendering a specific
//! part of the interface.

mod confirmation;
mod connection_error;
mod describe;
mod detail;
mod events;
mod footer;
mod graph;
mod header;
mod help;
mod helpers;
mod history;
mod quit_confirm;
pub mod resource_fields;
mod resource_list;
mod splash;
mod submenu;
pub mod trace;
mod yaml;

pub use confirmation::*;
pub use connection_error::render_connection_error;
pub use describe::*;
pub use detail::*;
pub use events::*;
// favorites module is not exported - favorites view uses render_resource_list instead
pub use footer::*;
pub use graph::*;
pub use header::*;
pub use help::*;
#[allow(unused_imports)] // Used via fully qualified paths (crate::tui::views::helpers::)
pub use helpers::*;
pub use history::*;
pub use quit_confirm::*;
pub use resource_fields::*;
pub use resource_list::*;
pub use splash::*;
pub use submenu::*;
pub use yaml::*;
