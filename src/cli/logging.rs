//! Logging initialization

use std::path::PathBuf;

/// Initialize logging based on debug flag
/// Returns the log file path if debug logging is enabled
pub fn init_logging(debug: bool) -> Option<PathBuf> {
    if debug {
        // Create a temporary log file using tempfile crate for cross-platform support
        // This works on Windows, macOS, and Linux
        // Use Builder to create a named temp file that persists
        let temp_file = tempfile::Builder::new()
            .prefix("flux9s-")
            .suffix(".log")
            .tempfile()
            .map(|f| {
                let path = f.path().to_path_buf();
                // Keep the file alive by leaking it (it will be cleaned up by the OS)
                // Alternatively, we could use persist(), but that requires a target path
                std::mem::forget(f);
                path
            })
            .unwrap_or_else(|_| {
                // Fallback: create file directly in temp_dir
                let temp_dir = std::env::temp_dir();
                temp_dir.join(format!("flux9s-{}.log", std::process::id()))
            });

        // Open the file for writing (it already exists from tempfile)
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temp_file)
            .expect("Failed to open log file");

        // Enable debug logging with tracing-subscriber
        // Write to file so TUI can use stdout/stderr without interference
        // File implements MakeWriter directly, so we can use it as-is
        tracing_subscriber::fmt()
            .with_writer(file)
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("debug")),
            )
            .with_ansi(false) // No ANSI codes in log file
            .with_target(true)
            .with_file(true)
            .with_line_number(true)
            .init();

        Some(temp_file)
    } else {
        // No logging by default (silent operation)
        None
    }
}
