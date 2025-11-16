//! XDG directory path resolution
//!
//! Provides functions to resolve XDG Base Directory paths for configuration,
//! data, and state directories.

use std::path::{Path, PathBuf};
use xdg::BaseDirectories;

/// Get the configuration directory path
///
/// Checks FLUX9S_CONFIG_DIR environment variable first, then falls back to
/// XDG_CONFIG_HOME/flux9s
pub fn config_dir() -> PathBuf {
    std::env::var("FLUX9S_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            BaseDirectories::with_prefix("flux9s")
                .map(|xdg| xdg.get_config_home())
                .unwrap_or_else(|_| {
                    // Fallback to current directory if XDG fails
                    PathBuf::from(".").join(".config").join("flux9s")
                })
        })
}

/// Get the data directory path
///
/// Checks FLUX9S_DATA_DIR environment variable first, then falls back to
/// XDG_DATA_HOME/flux9s
pub fn data_dir() -> PathBuf {
    std::env::var("FLUX9S_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            BaseDirectories::with_prefix("flux9s")
                .map(|xdg| xdg.get_data_home())
                .unwrap_or_else(|_| {
                    // Fallback to current directory if XDG fails
                    PathBuf::from(".")
                        .join(".local")
                        .join("share")
                        .join("flux9s")
                })
        })
}

/// Get the root configuration file path
pub fn root_config_path() -> PathBuf {
    config_dir().join("config.yaml")
}

/// Get the skins directory path (in config dir for built-in themes)
pub fn skins_dir() -> PathBuf {
    config_dir().join("skins")
}

/// Get the user skins directory path (in data dir for user themes)
pub fn user_skins_dir() -> PathBuf {
    data_dir().join("skins")
}

/// Get the cluster-specific config directory path
pub fn cluster_config_dir(cluster: &str, context: Option<&str>) -> PathBuf {
    let mut path = data_dir().join("clusters").join(cluster);
    if let Some(ctx) = context {
        path = path.join(ctx);
    }
    path
}

/// Get the cluster-specific config file path
pub fn cluster_config_path(cluster: &str, context: Option<&str>) -> PathBuf {
    cluster_config_dir(cluster, context).join("config.yaml")
}

/// Ensure a directory exists, creating it if necessary
pub fn ensure_dir(path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        std::fs::create_dir_all(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_dir() {
        let dir = config_dir();
        assert!(dir.to_string_lossy().contains("flux9s"));
    }

    #[test]
    fn test_paths_are_absolute() {
        assert!(config_dir().is_absolute() || config_dir().to_string_lossy().starts_with("."));
        assert!(data_dir().is_absolute() || data_dir().to_string_lossy().starts_with("."));
    }
}
