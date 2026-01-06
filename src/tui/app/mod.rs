//! Application module
//!
//! This module contains the main TUI application state and logic, organized
//! into sub-modules for better maintainability.

pub mod state;

mod async_ops;
mod core;
mod events;
mod rendering;

pub use core::*;
pub use state::PendingOperation;
