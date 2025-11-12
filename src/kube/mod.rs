//! Kubernetes client module
//!
//! Handles connection to Kubernetes API server and provides
//! a configured client for use throughout the application.
//!
//! Supports HTTP/HTTPS proxy configuration via standard environment variables:
//! - `HTTP_PROXY` / `http_proxy`: HTTP proxy URL
//! - `HTTPS_PROXY` / `https_proxy`: HTTPS proxy URL
//! - `NO_PROXY` / `no_proxy`: Comma-separated list of hosts to bypass proxy
//!
//! Automatically detects internal cluster hosts and adds them to NO_PROXY
//! to prevent proxy issues with corporate environments.

use anyhow::Result;
use kube::{Client, Config};
use url::Url;

/// Initialize and return a Kubernetes client with automatic proxy support
///
/// Uses the default kubeconfig loading strategy:
/// 1. In-cluster config (if running in a pod)
/// 2. KUBECONFIG environment variable
/// 3. ~/.kube/config
///
/// Automatically configures proxy bypass for internal cluster hosts by:
/// - Detecting the cluster API server hostname
/// - Adding it to NO_PROXY if it appears to be an internal domain
/// - Ensuring proper proxy bypass for corporate environments
pub async fn create_client() -> Result<Client> {
    let config = Config::infer().await?;

    // Extract cluster host for NO_PROXY auto-detection
    // Convert Uri to string and parse to extract hostname
    let cluster_url_str = config.cluster_url.to_string();
    if let Ok(url) = Url::parse(&cluster_url_str) {
        if let Some(host) = url.host_str() {
            // Automatically add internal cluster hosts to NO_PROXY
            ensure_no_proxy_bypass(host);
        }
    }

    let client = Client::try_from(config)?;
    Ok(client)
}

/// Ensure that a host is included in NO_PROXY for proxy bypass
///
/// This function automatically detects internal/private hosts and adds them
/// to the NO_PROXY environment variable if they're not already covered.
/// This prevents proxy issues in corporate environments where internal
/// Kubernetes clusters should bypass the corporate proxy.
fn ensure_no_proxy_bypass(host: &str) {
    // Only process if this looks like an internal host
    if !is_internal_host(host) {
        return;
    }

    // Check if host is already covered by NO_PROXY
    let no_proxy = std::env::var("NO_PROXY").unwrap_or_default();
    let no_proxy_lower = std::env::var("no_proxy").unwrap_or_default();

    // Use the non-empty value (NO_PROXY takes precedence)
    let current_no_proxy = if !no_proxy.is_empty() {
        no_proxy
    } else {
        no_proxy_lower
    };

    // Check if host is already covered
    if no_proxy_contains(&current_no_proxy, host) {
        return;
    }

    // Add host to NO_PROXY
    let updated_no_proxy = if current_no_proxy.is_empty() {
        host.to_string()
    } else {
        format!("{},{}", current_no_proxy, host)
    };

    // Set both uppercase and lowercase variants for compatibility
    std::env::set_var("NO_PROXY", &updated_no_proxy);
    std::env::set_var("no_proxy", &updated_no_proxy);
}

/// Check if a host looks like an internal/private domain
///
/// This detects common patterns for internal Kubernetes clusters:
/// - Private IP addresses (10.x.x.x, 172.16-31.x.x, 192.168.x.x)
/// - Localhost addresses
/// - Common internal TLDs (.local, .internal, .cluster.local)
/// - Internal domain patterns (e.g., *.corp.*, *.internal.*)
fn is_internal_host(host: &str) -> bool {
    // Check for private IP addresses
    if host.starts_with("10.")
        || host.starts_with("172.")
        || host.starts_with("192.168.")
        || host == "localhost"
        || host == "127.0.0.1"
        || host == "::1"
    {
        return true;
    }

    // Check for common internal TLDs
    if host.ends_with(".local")
        || host.ends_with(".internal")
        || host.ends_with(".cluster.local")
        || host.ends_with(".svc.cluster.local")
    {
        return true;
    }

    // Check for common internal domain patterns
    // These are heuristics for corporate internal domains
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() >= 2 {
        // Check for patterns like *.corp.*, *.internal.*, *.int.*
        let domain = parts[parts.len() - 2];
        if matches!(domain, "corp" | "internal" | "int" | "local") {
            return true;
        }
        // Check for patterns like *.dev.*, *.test.*, *.staging.*
        // These are often internal environments
        if parts.len() >= 3 {
            let subdomain = parts[parts.len() - 3];
            if matches!(subdomain, "dev" | "test" | "staging" | "qa" | "uat") {
                return true;
            }
        }
        // Check if any part of the hostname contains common internal prefixes
        // This handles cases like devprod.example.com, testapi.example.com, etc.
        for part in &parts {
            if part.starts_with("dev")
                || part.starts_with("test")
                || part.starts_with("staging")
                || part.starts_with("qa")
                || part.starts_with("uat")
                || part.starts_with("internal")
            {
                // Only consider it internal if it's not the TLD
                if part != parts.last().unwrap() {
                    return true;
                }
            }
        }
    }

    false
}

/// Check if NO_PROXY already contains the host (handles wildcards and patterns)
///
/// This function properly handles various NO_PROXY patterns:
/// - Exact matches: "example.com" matches "example.com"
/// - Wildcard patterns: ".example.com" matches "*.example.com" and "example.com"
/// - Subdomain matching: "example.com" matches "sub.example.com"
fn no_proxy_contains(no_proxy: &str, host: &str) -> bool {
    if no_proxy.is_empty() {
        return false;
    }

    no_proxy
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .any(|pattern| {
            // Exact match
            if pattern == host {
                return true;
            }

            // Wildcard pattern like .example.com matches subdomains
            if let Some(suffix) = pattern.strip_prefix('.') {
                // Matches exact domain or any subdomain
                if host == suffix || host.ends_with(&format!(".{}", suffix)) {
                    return true;
                }
            }

            // Pattern like example.com matches both example.com and *.example.com
            if host == pattern {
                return true;
            }
            if host.ends_with(&format!(".{}", pattern)) {
                return true;
            }

            // Check if pattern is a subdomain of host
            // e.g., "sub.example.com" pattern matches "sub.example.com" host
            if pattern.ends_with(host) && pattern.len() > host.len() {
                let prefix = &pattern[..pattern.len() - host.len()];
                if prefix.ends_with('.') {
                    return true;
                }
            }

            false
        })
}

/// Get the current Kubernetes context name
pub async fn get_context() -> Result<String> {
    // Try to get context from KUBECONFIG or default location
    let kubeconfig_path = std::env::var("KUBECONFIG").ok().or_else(|| {
        let home = std::env::var("HOME").ok()?;
        Some(format!("{}/.kube/config", home))
    });

    if let Some(path) = kubeconfig_path {
        if let Ok(contents) = std::fs::read_to_string(&path) {
            // Parse current-context from kubeconfig
            for line in contents.lines() {
                if line.trim().starts_with("current-context:") {
                    if let Some(context) = line.split(':').nth(1) {
                        return Ok(context.trim().to_string());
                    }
                }
            }
        }
    }

    // Fallback: try to get from Config
    let _config = Config::infer().await?;
    // Config doesn't expose current_context directly, use a default
    Ok("default".to_string())
}

/// Get the default namespace for Flux resources
///
/// Uses flux-system as default (like flux CLI), but can be overridden
/// with NAMESPACE environment variable or set to None to watch all namespaces
pub async fn get_default_namespace() -> Option<String> {
    // Check environment variable first
    if let Ok(ns) = std::env::var("NAMESPACE") {
        if ns.is_empty() || ns == "all" || ns == "-A" {
            return None; // Watch all namespaces
        }
        return Some(ns);
    }
    // Default to flux-system (like flux CLI)
    Some("flux-system".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_internal_host_private_ips() {
        assert!(is_internal_host("10.0.0.1"));
        assert!(is_internal_host("172.16.0.1"));
        assert!(is_internal_host("192.168.1.1"));
        assert!(is_internal_host("localhost"));
        assert!(is_internal_host("127.0.0.1"));
        assert!(is_internal_host("::1"));
    }

    #[test]
    fn test_is_internal_host_internal_tlds() {
        assert!(is_internal_host("example.local"));
        assert!(is_internal_host("cluster.internal"));
        assert!(is_internal_host("service.cluster.local"));
        assert!(is_internal_host("pod.svc.cluster.local"));
    }

    #[test]
    fn test_is_internal_host_corporate_patterns() {
        assert!(is_internal_host("dev.example.corp"));
        assert!(is_internal_host("api.internal"));
        assert!(is_internal_host("test.example.int"));
        assert!(is_internal_host("dev.cluster.local"));
        assert!(is_internal_host("staging.api.example"));
        assert!(is_internal_host("qa.service.example"));
        assert!(is_internal_host("uat.api.example"));
        // Test the actual scenario from the issue: devprod.example.com
        assert!(is_internal_host("devprod.example.com"));
        assert!(is_internal_host("testapi.example.com"));
        assert!(is_internal_host("devcluster.internal.com"));
    }

    #[test]
    fn test_is_internal_host_public_domains() {
        assert!(!is_internal_host("example.com"));
        assert!(!is_internal_host("api.github.com"));
        assert!(!is_internal_host("kubernetes.io"));
        assert!(!is_internal_host("google.com"));
    }

    #[test]
    fn test_no_proxy_contains_exact_match() {
        assert!(no_proxy_contains("example.com", "example.com"));
        assert!(no_proxy_contains("localhost,example.com", "example.com"));
        assert!(no_proxy_contains("example.com,localhost", "example.com"));
    }

    #[test]
    fn test_no_proxy_contains_wildcard() {
        // .example.com should match example.com and *.example.com
        assert!(no_proxy_contains(".example.com", "example.com"));
        assert!(no_proxy_contains(".example.com", "sub.example.com"));
        assert!(no_proxy_contains(".example.com", "api.sub.example.com"));
        // .prod.example.com matches *.prod.example.com but NOT devprod.example.com (different domain)
        assert!(no_proxy_contains(
            ".prod.example.com",
            "dev.prod.example.com"
        ));
        assert!(!no_proxy_contains(
            ".prod.example.com",
            "devprod.example.com"
        )); // This is why we need auto-detection
    }

    #[test]
    fn test_no_proxy_contains_subdomain() {
        // example.com should match example.com and *.example.com
        assert!(no_proxy_contains("example.com", "example.com"));
        assert!(no_proxy_contains("example.com", "sub.example.com"));
        assert!(no_proxy_contains("example.com", "api.sub.example.com"));
    }

    #[test]
    fn test_no_proxy_contains_not_matching() {
        assert!(!no_proxy_contains("", "example.com"));
        assert!(!no_proxy_contains("other.com", "example.com"));
        assert!(!no_proxy_contains(".other.com", "example.com"));
    }

    #[test]
    fn test_no_proxy_contains_with_spaces() {
        assert!(no_proxy_contains("localhost, example.com", "example.com"));
        assert!(no_proxy_contains(
            " localhost , example.com ",
            "example.com"
        ));
    }

    #[test]
    fn test_no_proxy_contains_real_world_scenarios() {
        // Test the actual problem scenario from the issue
        // .prod.example.com should match devprod.example.com (but it doesn't - that's the bug we're fixing)
        // However, devprod.example.com should be auto-added to NO_PROXY
        assert!(!no_proxy_contains(
            ".prod.example.com",
            "devprod.example.com"
        )); // This is why we need auto-detection

        // But .example.com should match devprod.example.com
        assert!(no_proxy_contains(".example.com", "devprod.example.com"));

        // Exact match works
        assert!(no_proxy_contains(
            "devprod.example.com",
            "devprod.example.com"
        ));
    }
}
