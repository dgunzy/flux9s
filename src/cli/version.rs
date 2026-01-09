//! Version command handler

use update_informer::{Check, registry};

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
/// Respects NO_UPDATE_NOTIFIER environment variable.
pub fn check_for_updates_blocking(debug: bool) {
    let name = env!("CARGO_PKG_NAME");
    let version = env!("CARGO_PKG_VERSION");

    // Use Duration::ZERO in debug mode to check immediately, otherwise 24 hours
    // With 24-hour interval, first check happens after 24 hours (cached behavior)
    let interval = if debug {
        std::time::Duration::ZERO
    } else {
        std::time::Duration::from_secs(60 * 60 * 24)
    };

    let informer = update_informer::new(registry::Crates, name, version).interval(interval);

    match informer.check_version() {
        Ok(Some(new_version)) => {
            eprintln!(
                "💡 A new version of flux9s is available: {} (current: v{}) - Disable: NO_UPDATE_NOTIFIER=1",
                new_version, version
            );
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
