//! Constants used throughout flux9s
//!
//! This module centralizes magic numbers and strings to reduce duplication
//! and make values easier to maintain.

/// Resource key format: "resource_type:namespace:name"
pub const RESOURCE_KEY_FORMAT: &str = "resource_type:namespace:name";

/// Maximum number of reconciliation history events to store per resource
pub const MAX_RECONCILIATION_HISTORY: usize = 50;

/// Cap on the live Kubernetes events feed. Events are the churniest resource
/// in a cluster; the store evicts oldest-seen entries past this bound so a
/// busy cluster can't grow memory without limit.
pub const MAX_KUBE_EVENTS: usize = 1000;

/// Cap on the controller log view's line buffer; oldest lines are evicted.
pub const MAX_LOG_LINES: usize = 5000;

/// Selection jump for PageUp/PageDown (and Ctrl+f/Ctrl+b) inside submenus.
/// A fixed jump rather than a "page": the popup's height varies and the
/// scroll self-corrects at render time.
pub const SUBMENU_PAGE_JUMP: usize = 10;

/// How many existing lines the log stream starts with (`tail_lines`) before
/// following new output.
pub const LOG_TAIL_LINES: i64 = 500;

/// Status message timeout in seconds
pub const STATUS_MESSAGE_TIMEOUT_SECS: u64 = 4;

/// Status message shown when a write action is attempted in readonly mode
pub const READ_ONLY_WRITE_ACTION_MESSAGE: &str =
    "Readonly mode is enabled. Use :readonly to toggle write actions.";

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
