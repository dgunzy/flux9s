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

// Re-export commonly used types when extensions are implemented
// pub use extensions::*;
