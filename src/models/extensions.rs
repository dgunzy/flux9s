//! Manual extensions for generated Flux models
//!
//! This module contains helper traits and functions that extend the
//! auto-generated CRD models with TUI-specific functionality.
//!
//! These extensions are implemented externally (not modifying generated code)
//! to allow safe regeneration of models without conflicts.

// TODO: Implement extension traits for:
// - Status checking (is_ready, is_reconciling, is_failed)
// - Display formatting (status icons, summaries)
// - Flux-specific parsing (interval format, condition extraction)

/// Trait for checking resource readiness status
pub trait ResourceStatus {
    /// Returns true if the resource is ready/reconciled
    fn is_ready(&self) -> bool;

    /// Returns true if the resource is currently reconciling
    fn is_reconciling(&self) -> bool;

    /// Returns true if the resource has failed
    fn is_failed(&self) -> bool;

    /// Returns a status icon for display in the TUI
    fn status_icon(&self) -> &'static str;
}

/// Trait for getting resource display summaries
pub trait ResourceDisplay {
    /// Returns a short summary string for the resource
    fn summary(&self) -> String;

    /// Returns the resource age as a human-readable string
    fn age(&self) -> String;
}

// Placeholder implementations will be added once models are generated
// Example:
// impl ResourceStatus for GitRepository {
//     fn is_ready(&self) -> bool { ... }
//     ...
// }
