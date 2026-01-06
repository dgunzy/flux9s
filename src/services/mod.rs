//! Service layer for business logic
//!
//! This module provides a clean abstraction layer between the TUI and
//! Kubernetes API operations. Services handle async operations and return
//! results via channels, keeping the TUI layer focused on presentation.

pub mod resource_service;

pub use resource_service::ResourceService;
