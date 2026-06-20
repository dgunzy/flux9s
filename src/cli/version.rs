//! Version command handler

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use update_informer::{Check, registry};

use crate::config::paths;

/// How often the update banner may be displayed (24 hours, in seconds).
///
/// Note: this throttles how often the *notification* is shown. The network
/// check is throttled separately by `update-informer`'s own interval. Without
/// this, `update-informer` returns the cached "update available" result on
/// every run, printing the banner on every invocation.
const NOTIFY_INTERVAL_SECS: u64 = 60 * 60 * 24;

/// Persisted record of the last displayed update notification.
#[derive(Debug, Serialize, Deserialize)]
struct NotifierState {
    /// Version advertised the last time the banner was shown.
    last_notified_version: String,
    /// Unix timestamp (seconds) when the banner was last shown.
    last_notified_at: u64,
}

/// Display version information
pub fn display_version(debug: bool) {
    let version = env!("CARGO_PKG_VERSION");
    println!("flux9s {}", version);
    println!("  {}", env!("CARGO_PKG_DESCRIPTION"));
    println!("  {}", env!("CARGO_PKG_AUTHORS"));
    println!("  License: {}", env!("CARGO_PKG_LICENSE"));
    println!("  Repository: {}", env!("CARGO_PKG_REPOSITORY"));

    // Check for updates (blocking, suitable for explicit version command)
    check_for_updates_blocking(debug);
}

/// Check for newer versions available on crates.io (blocking)
/// Displays a gentle notification if an update is available.
/// Respects the `NO_UPDATE_NOTIFIER` environment variable.
///
/// The network check is throttled to once per 24h by `update-informer`, and
/// the *notification itself* is throttled to once per 24h via a small state
/// file (see [`paths::update_notifier_state_path`]).
pub fn check_for_updates_blocking(debug: bool) {
    if notifications_disabled() {
        if debug {
            eprintln!("DEBUG: Update notifications disabled via NO_UPDATE_NOTIFIER");
        }
        return;
    }

    let name = env!("CARGO_PKG_NAME");
    let version = env!("CARGO_PKG_VERSION");

    // Use Duration::ZERO in debug mode to check immediately, otherwise 24 hours
    // With 24-hour interval, first check happens after 24 hours (cached behavior)
    let interval = if debug {
        std::time::Duration::ZERO
    } else {
        std::time::Duration::from_secs(NOTIFY_INTERVAL_SECS)
    };

    let informer = update_informer::new(registry::Crates, name, version).interval(interval);

    match informer.check_version() {
        Ok(Some(new_version)) => {
            let new_version = new_version.to_string();
            // `update-informer` returns the cached result on every run within
            // its interval, so gate the banner on our own display throttle to
            // honor "show at most once per 24h".
            if should_display(&new_version, debug) {
                eprintln!(
                    "💡 A new version of flux9s is available: {} (current: v{}) - Disable: NO_UPDATE_NOTIFIER=1",
                    new_version, version
                );
                record_notification(&new_version, debug);
            } else if debug {
                eprintln!(
                    "DEBUG: Update {} available but notification recently shown; suppressed",
                    new_version
                );
            }
        }
        Ok(None) => {
            if debug {
                eprintln!("DEBUG: No update available (current: v{})", version);
            }
        }
        Err(e) => {
            if debug {
                eprintln!("DEBUG: Update check failed: {}", e);
            }
        }
    }
}

/// Whether update notifications are disabled via `NO_UPDATE_NOTIFIER`.
///
/// Follows the common convention: disabled when the variable is set to any
/// value other than the empty string, `0`, or `false`.
fn notifications_disabled() -> bool {
    is_disable_value(std::env::var("NO_UPDATE_NOTIFIER").ok().as_deref())
}

/// Pure helper: does the given `NO_UPDATE_NOTIFIER` value disable notifications?
fn is_disable_value(value: Option<&str>) -> bool {
    match value {
        Some(val) => {
            let val = val.trim().to_ascii_lowercase();
            !(val.is_empty() || val == "0" || val == "false")
        }
        None => false,
    }
}

/// Current time as seconds since the Unix epoch (0 if the clock is before it).
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Decide whether the banner should be displayed for `new_version`.
///
/// Always shows in debug mode. Otherwise shows when no prior notification is
/// recorded, when the advertised version changed since it was last shown, or
/// when more than [`NOTIFY_INTERVAL_SECS`] has elapsed since it was last shown.
fn should_display(new_version: &str, debug: bool) -> bool {
    if debug {
        return true;
    }
    decide_display(new_version, read_state().as_ref(), now_secs())
}

/// Pure decision logic for the display throttle (testable without I/O).
fn decide_display(new_version: &str, state: Option<&NotifierState>, now: u64) -> bool {
    match state {
        Some(state) => {
            // A newer release should always surface immediately, even if we
            // showed a (different) banner recently.
            if state.last_notified_version != new_version {
                return true;
            }
            now.saturating_sub(state.last_notified_at) >= NOTIFY_INTERVAL_SECS
        }
        // No (readable) state yet: show it.
        None => true,
    }
}

/// Read the persisted notifier state, if present and parseable.
fn read_state() -> Option<NotifierState> {
    let path = paths::update_notifier_state_path();
    let contents = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&contents).ok()
}

/// Persist that the banner was shown for `new_version` at the current time.
///
/// Failures are non-fatal: the worst case is the banner shows again next run.
fn record_notification(new_version: &str, debug: bool) {
    let path = paths::update_notifier_state_path();

    let write_result = (|| -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            paths::ensure_dir(parent)?;
        }
        let state = NotifierState {
            last_notified_version: new_version.to_string(),
            last_notified_at: now_secs(),
        };
        let json = serde_json::to_string(&state).map_err(std::io::Error::other)?;
        std::fs::write(&path, json)
    })();

    if let Err(e) = write_result
        && debug
    {
        eprintln!(
            "DEBUG: Failed to persist update-notifier state to {}: {}",
            path.display(),
            e
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_disable_value_respects_truthy_values() {
        assert!(is_disable_value(Some("1")));
        assert!(is_disable_value(Some("true")));
        assert!(is_disable_value(Some("yes")));
        assert!(is_disable_value(Some(" TRUE ")));

        assert!(!is_disable_value(Some("0")));
        assert!(!is_disable_value(Some("false")));
        assert!(!is_disable_value(Some("")));
        assert!(!is_disable_value(None));
    }

    #[test]
    fn decide_display_shows_when_no_state() {
        assert!(decide_display("v0.10.2", None, 1_000_000));
    }

    #[test]
    fn decide_display_shows_when_version_changed() {
        let state = NotifierState {
            last_notified_version: "v0.10.1".to_string(),
            last_notified_at: 1_000_000,
        };
        // Same instant, but a different version -> show immediately.
        assert!(decide_display("v0.10.2", Some(&state), 1_000_000));
    }

    #[test]
    fn decide_display_suppresses_within_interval() {
        let state = NotifierState {
            last_notified_version: "v0.10.2".to_string(),
            last_notified_at: 1_000_000,
        };
        let just_before = 1_000_000 + NOTIFY_INTERVAL_SECS - 1;
        assert!(!decide_display("v0.10.2", Some(&state), just_before));
    }

    #[test]
    fn decide_display_shows_after_interval() {
        let state = NotifierState {
            last_notified_version: "v0.10.2".to_string(),
            last_notified_at: 1_000_000,
        };
        let after = 1_000_000 + NOTIFY_INTERVAL_SECS;
        assert!(decide_display("v0.10.2", Some(&state), after));
    }

    #[test]
    fn notifier_state_roundtrips() {
        let state = NotifierState {
            last_notified_version: "v0.10.2".to_string(),
            last_notified_at: 1_718_960_000,
        };
        let json = serde_json::to_string(&state).unwrap();
        let parsed: NotifierState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.last_notified_version, "v0.10.2");
        assert_eq!(parsed.last_notified_at, 1_718_960_000);
    }
}
