//! Cross-platform directory path resolution
//!
//! Provides functions to resolve platform-appropriate paths for configuration,
//! data, and state directories.
//! - Linux/macOS: XDG Base Directory specification (~/.config, ~/.local/share)
//! - Windows: Known Folder API (AppData\Roaming, AppData\Local)

use std::path::{Path, PathBuf};

/// Get the configuration directory path
///
/// Checks FLUX9S_CONFIG_DIR environment variable first, then falls back to:
/// - Unix (Linux/macOS): XDG_CONFIG_HOME/flux9s or ~/.config/flux9s
/// - Windows: %APPDATA%\flux9s\config
pub fn config_dir() -> PathBuf {
    std::env::var("FLUX9S_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            #[cfg(windows)]
            {
                // On Windows, use ProjectDirs for proper AppData paths
                use directories::ProjectDirs;
                ProjectDirs::from("", "", "flux9s")
                    .map(|dirs| dirs.config_dir().to_path_buf())
                    .unwrap_or_else(|| PathBuf::from(".").join(".config").join("flux9s"))
            }
            #[cfg(not(windows))]
            {
                // On Unix (Linux/macOS), use XDG_CONFIG_HOME or $HOME/.config
                use directories::BaseDirs;
                std::env::var("XDG_CONFIG_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| {
                        BaseDirs::new()
                            .map(|dirs| dirs.home_dir().join(".config"))
                            .unwrap_or_else(|| PathBuf::from(".").join(".config"))
                    })
                    .join("flux9s")
            }
        })
}

/// Get the data directory path
///
/// Checks FLUX9S_DATA_DIR environment variable first, then falls back to:
/// - Unix (Linux/macOS): XDG_DATA_HOME/flux9s or ~/.local/share/flux9s
/// - Windows: %LOCALAPPDATA%\flux9s\data
pub fn data_dir() -> PathBuf {
    std::env::var("FLUX9S_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            #[cfg(windows)]
            {
                // On Windows, use ProjectDirs for proper AppData paths
                use directories::ProjectDirs;
                ProjectDirs::from("", "", "flux9s")
                    .map(|dirs| dirs.data_dir().to_path_buf())
                    .unwrap_or_else(|| {
                        PathBuf::from(".")
                            .join(".local")
                            .join("share")
                            .join("flux9s")
                    })
            }
            #[cfg(not(windows))]
            {
                // On Unix (Linux/macOS), use XDG_DATA_HOME or $HOME/.local/share
                use directories::BaseDirs;
                std::env::var("XDG_DATA_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| {
                        BaseDirs::new()
                            .map(|dirs| dirs.home_dir().join(".local").join("share"))
                            .unwrap_or_else(|| PathBuf::from(".").join(".local").join("share"))
                    })
                    .join("flux9s")
            }
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
