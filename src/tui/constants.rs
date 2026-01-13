//! Constants used throughout the TUI module
//!
//! This module centralizes magic numbers and strings to reduce duplication
//! and make values easier to maintain.

/// Resource key format: "resource_type:namespace:name"
pub const RESOURCE_KEY_FORMAT: &str = "resource_type:namespace:name";

/// Maximum number of reconciliation history events to store per resource
pub const MAX_RECONCILIATION_HISTORY: usize = 50;

/// Status message timeout in seconds
pub const STATUS_MESSAGE_TIMEOUT_SECS: u64 = 4;

/// Minimum terminal width required for the TUI
pub const MIN_TERMINAL_WIDTH: u16 = 80;

/// Default minimum header height (accommodates ASCII art and 8 controller status lines)
pub const MIN_HEADER_HEIGHT: u16 = 8;

/// Default minimum footer height
pub const MIN_FOOTER_HEIGHT: u16 = 3;

/// Maximum number of namespace hotkeys (0-9)
pub const MAX_NAMESPACE_HOTKEYS: usize = 10;

/// Maximum number of namespace hotkeys to display in footer
pub const MAX_FOOTER_NAMESPACE_HOTKEYS: usize = 3;

/// Maximum namespace name length to display in footer (truncate if longer)
pub const MAX_FOOTER_NAMESPACE_LENGTH: usize = 8;

/// Splash screen display duration in milliseconds
pub const SPLASH_DISPLAY_MS: u64 = 1500;

/// Known Flux controller pod name prefixes
pub const FLUX_CONTROLLER_NAMES: &[&str] = &[
    "flux-operator",
    "source-controller",
    "kustomize-controller",
    "helm-controller",
    "notification-controller",
    "image-reflector-controller",
    "image-automation-controller",
    "source-watcher",
];
