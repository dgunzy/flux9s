//! Version command handler

/// Display version information
pub fn display_version() {
    println!("flux9s {}", env!("CARGO_PKG_VERSION"));
    println!("  {}", env!("CARGO_PKG_DESCRIPTION"));
    println!("  {}", env!("CARGO_PKG_AUTHORS"));
    println!("  License: {}", env!("CARGO_PKG_LICENSE"));
    println!("  Repository: {}", env!("CARGO_PKG_REPOSITORY"));
}
