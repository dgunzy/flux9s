//! Application module
//!
//! This module contains the main TUI application state and logic, organized
//! into sub-modules for better maintainability.

pub mod state;

pub mod async_task;
pub mod live_task;
pub mod logs;

pub(crate) mod actions;
mod async_ops;
pub(crate) mod command_mode;
mod core;
mod events;
mod keys;
mod kind_browser;
mod managers;
mod navigation;
mod rendering;
mod submenus;
#[cfg(test)]
mod tests;

pub use core::*;
pub use state::PendingOperation;
