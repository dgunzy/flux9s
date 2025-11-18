//! TUI view components
//!
//! This module contains all the rendering components for different views
//! in the TUI. Each component is responsible for rendering a specific
//! part of the interface.

mod confirmation;
mod detail;
mod footer;
mod header;
mod help;
pub mod resource_fields;
mod resource_list;
mod splash;
pub mod trace;
mod yaml;

pub use confirmation::*;
pub use detail::*;
pub use footer::*;
pub use header::*;
pub use help::*;
pub use resource_fields::*;
pub use resource_list::*;
pub use splash::*;
pub use yaml::*;
