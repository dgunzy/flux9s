//! Flux TUI Model Layer
//!
//! This module provides Rust types for Flux CRD resources.
//!
//! Structure:
//! - `_generated/` - Auto-generated models from CRDs (gitignored)
//! - `extensions.rs` - Manual extensions and helper traits
//! - `mod.rs` - Public API re-exports

// Re-export generated models
pub mod _generated;

// Manual extensions
pub mod extensions;

// Flux resource kind definitions
pub mod flux_resource_kind;

// Re-export commonly used types
pub use flux_resource_kind::FluxResourceKind;
