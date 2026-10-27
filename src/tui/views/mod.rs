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
mod helm_values;
mod help;
mod helpers;
mod history;
mod inventory;
mod kind_list;
mod logs;
mod ownership;
pub mod props;
mod pulse;
mod quit_confirm;
pub mod resource_fields;
mod resource_list;
pub mod scroll;
mod splash;
mod submenu;
pub mod text;
pub mod trace;
mod workloads;
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
pub use helm_values::*;
pub use help::*;
pub use helpers::*;
pub use history::*;
pub use inventory::*;
pub use kind_list::*;
pub use logs::*;
pub use ownership::*;
pub use pulse::*;
pub use quit_confirm::*;
pub use resource_fields::*;
pub use resource_list::*;
pub use splash::*;
pub use submenu::*;
pub use workloads::*;
pub use yaml::*;
