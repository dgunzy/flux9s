//! Default configuration values
//!
//! Provides default configuration instances and helper functions.

use super::schema::Config;

/// Get the default configuration
pub fn default_config() -> Config {
    Config::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = default_config();
        assert!(config.read_only);
        assert_eq!(config.default_namespace, "flux-system");
    }
}
