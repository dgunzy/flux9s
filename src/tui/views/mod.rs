//! TUI view components
//!
//! This module contains all the rendering components for different views
//! in the TUI. Each component is responsible for rendering a specific
//! part of the interface.

mod confirmation;
mod detail;
mod footer;
mod graph;
mod header;
mod help;
mod helpers;
mod history;
pub mod resource_fields;
mod resource_list;
mod splash;
pub mod trace;
mod yaml;

pub use confirmation::*;
pub use detail::*;
// favorites module is not exported - favorites view uses render_resource_list instead
pub use footer::*;
pub use graph::*;
pub use header::*;
pub use help::*;
#[allow(unused_imports)] // Used via fully qualified paths (crate::tui::views::helpers::)
pub use helpers::*;
pub use history::*;
pub use resource_fields::*;
pub use resource_list::*;
pub use splash::*;
pub use yaml::*;
