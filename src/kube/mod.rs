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

pub mod inventory;

use anyhow::Result;
use kube::config::Kubeconfig;
use kube::{Client, Config};
use std::path::Path;
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
    tracing::debug!("Cluster URL: {}", cluster_url_str);

    if let Ok(url) = Url::parse(&cluster_url_str) {
        if let Some(host) = url.host_str() {
            tracing::debug!("Detected cluster host: {}", host);
            // Automatically add internal cluster hosts to NO_PROXY
            ensure_no_proxy_bypass(host);
        }
    } else {
        tracing::warn!("Failed to parse cluster URL: {}", cluster_url_str);
    }

    let client = Client::try_from(config)?;
    tracing::debug!("Kubernetes client created successfully");
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
        tracing::debug!(
            "Host {} is not detected as internal, skipping NO_PROXY update",
            host
        );
        return;
    }

    tracing::debug!("Host {} detected as internal, checking NO_PROXY", host);

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
        tracing::debug!("Host {} is already in NO_PROXY: {}", host, current_no_proxy);
        return;
    }

    // Add host to NO_PROXY
    let updated_no_proxy = if current_no_proxy.is_empty() {
        host.to_string()
    } else {
        format!("{},{}", current_no_proxy, host)
    };

    // Set both uppercase and lowercase variants for compatibility
    // SAFETY: set_var is unsafe in Rust 2024 due to potential data races in multi-threaded contexts.
    // This is safe here because:
    // 1. This function is called during client initialization, before any async tasks spawn
    // 2. We're setting proxy bypass configuration early in the program lifecycle
    // 3. The environment variable is set once per client creation, not in a hot loop
    unsafe {
        std::env::set_var("NO_PROXY", &updated_no_proxy);
        std::env::set_var("no_proxy", &updated_no_proxy);
    }
    tracing::info!("Added {} to NO_PROXY: {}", host, updated_no_proxy);
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

/// Load kubeconfig using kube crate's built-in path resolution
///
/// This function uses the kube crate's Kubeconfig::read() method which:
/// - Respects KUBECONFIG environment variable
/// - Falls back to platform-specific default locations (~/.kube/config on Unix, %USERPROFILE%\.kube\config on Windows)
/// - Handles multiple kubeconfig files separated by path separator
/// - Is OS-agnostic and cross-platform compatible
fn load_kubeconfig() -> Result<Kubeconfig> {
    Kubeconfig::read().map_err(|e| anyhow::anyhow!("Failed to load kubeconfig: {}", e))
}

/// Get the current Kubernetes context name
pub async fn get_context() -> Result<String> {
    let kubeconfig = load_kubeconfig()?;

    // Get current context from kubeconfig
    // If no current context is set, Config::infer() will use the first context
    // or we can fall back to checking what Config::infer() would use
    if let Some(current_context) = kubeconfig.current_context {
        Ok(current_context)
    } else {
        // Fallback: if no current context is set in kubeconfig, use "default"
        // This matches kubectl behavior when no current-context is set
        Ok("default".to_string())
    }
}

/// Get the current Kubernetes context name from a specific kubeconfig file
///
/// Returns an error if:
/// - The kubeconfig file cannot be read or does not exist
/// - The kubeconfig file is invalid or malformed
/// - No current context is set in the kubeconfig
pub fn get_context_from_kubeconfig_path(kubeconfig_path: &Path) -> Result<String> {
    // Check if file exists and is readable
    if !kubeconfig_path.exists() {
        anyhow::bail!(
            "Kubeconfig file does not exist: {}",
            kubeconfig_path.display()
        );
    }

    if !kubeconfig_path.is_file() {
        anyhow::bail!(
            "Kubeconfig path is not a file: {}",
            kubeconfig_path.display()
        );
    }

    // Load and parse kubeconfig
    let kubeconfig = Kubeconfig::read_from(kubeconfig_path)
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to load or parse kubeconfig from {}: {}. Please ensure the file is a valid kubeconfig.",
                kubeconfig_path.display(),
                e
            )
        })?;

    // Validate that contexts exist in the kubeconfig
    if kubeconfig.contexts.is_empty() {
        anyhow::bail!(
            "Kubeconfig file {} contains no contexts. Please ensure the kubeconfig is valid.",
            kubeconfig_path.display()
        );
    }

    // Get current context from kubeconfig - this is required
    match kubeconfig.current_context {
        Some(ref current_context) if !current_context.is_empty() => {
            // Validate that the current context actually exists in the contexts list
            if !kubeconfig
                .contexts
                .iter()
                .any(|ctx| ctx.name == *current_context)
            {
                let available_contexts: Vec<String> =
                    kubeconfig.contexts.iter().map(|c| c.name.clone()).collect();
                anyhow::bail!(
                    "Current context '{}' specified in kubeconfig {} does not exist in the contexts list. Available contexts: {}",
                    current_context,
                    kubeconfig_path.display(),
                    available_contexts.join(", ")
                );
            }
            Ok(current_context.clone())
        }
        _ => {
            let available_contexts: Vec<String> =
                kubeconfig.contexts.iter().map(|c| c.name.clone()).collect();
            anyhow::bail!(
                "No current context is set in kubeconfig {}. Please set a current context or specify one using 'kubectl config use-context <context-name>'. Available contexts: {}",
                kubeconfig_path.display(),
                available_contexts.join(", ")
            )
        }
    }
}

/// List all available Kubernetes contexts from kubeconfig
pub fn list_contexts() -> Result<Vec<String>> {
    let kubeconfig = load_kubeconfig()?;

    let context_names: Vec<String> = kubeconfig
        .contexts
        .iter()
        .map(|ctx| ctx.name.clone())
        .collect();

    if context_names.is_empty() {
        anyhow::bail!("No contexts found in kubeconfig");
    }

    Ok(context_names)
}

/// Create a Kubernetes client from a specific kubeconfig file path
///
/// Uses the specified kubeconfig file instead of the default loading strategy.
/// Automatically configures proxy bypass for internal cluster hosts.
///
/// Returns an error if:
/// - The kubeconfig file cannot be read or does not exist
/// - The kubeconfig file is invalid or malformed
/// - The kubeconfig cannot be used to create a valid Kubernetes client
pub async fn create_client_from_kubeconfig_path(kubeconfig_path: &Path) -> Result<Client> {
    // Check if file exists and is readable
    if !kubeconfig_path.exists() {
        anyhow::bail!(
            "Kubeconfig file does not exist: {}",
            kubeconfig_path.display()
        );
    }

    if !kubeconfig_path.is_file() {
        anyhow::bail!(
            "Kubeconfig path is not a file: {}",
            kubeconfig_path.display()
        );
    }

    // Load kubeconfig from the specified path
    let kubeconfig = Kubeconfig::read_from(kubeconfig_path)
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to load or parse kubeconfig from {}: {}. Please ensure the file is a valid kubeconfig.",
                kubeconfig_path.display(),
                e
            )
        })?;

    // Create config from the kubeconfig
    let config = Config::from_custom_kubeconfig(kubeconfig, &kube::config::KubeConfigOptions::default())
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to create Kubernetes config from kubeconfig {}: {}. Please ensure the kubeconfig is valid and contains valid cluster, user, and context information.",
                kubeconfig_path.display(),
                e
            )
        })?;

    // Extract cluster host for NO_PROXY auto-detection
    let cluster_url_str = config.cluster_url.to_string();
    tracing::debug!(
        "Cluster URL from kubeconfig {}: {}",
        kubeconfig_path.display(),
        cluster_url_str
    );

    if let Ok(url) = Url::parse(&cluster_url_str) {
        if let Some(host) = url.host_str() {
            tracing::debug!("Detected cluster host: {}", host);
            ensure_no_proxy_bypass(host);
        }
    }

    let client = Client::try_from(config)?;
    tracing::info!(
        "Kubernetes client created from kubeconfig: {}",
        kubeconfig_path.display()
    );
    Ok(client)
}

/// Create a Kubernetes client for a specific context
pub async fn create_client_for_context(context_name: &str) -> Result<Client> {
    // Validate that context exists
    let contexts = list_contexts()?;
    if !contexts.contains(&context_name.to_string()) {
        anyhow::bail!(
            "Context '{}' not found. Available contexts: {}",
            context_name,
            contexts.join(", ")
        );
    }

    // Load config with specific context
    let config = Config::from_kubeconfig(&kube::config::KubeConfigOptions {
        context: Some(context_name.to_string()),
        ..Default::default()
    })
    .await?;

    // Extract cluster host for NO_PROXY auto-detection
    let cluster_url_str = config.cluster_url.to_string();
    tracing::debug!(
        "Cluster URL for context {}: {}",
        context_name,
        cluster_url_str
    );

    if let Ok(url) = Url::parse(&cluster_url_str) {
        if let Some(host) = url.host_str() {
            tracing::debug!("Detected cluster host: {}", host);
            ensure_no_proxy_bypass(host);
        }
    }

    let client = Client::try_from(config)?;
    tracing::info!("Kubernetes client created for context: {}", context_name);
    Ok(client)
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

/// Discover namespaces that contain Flux resources
///
/// Returns a list of namespaces sorted by the number of Flux resources they contain.
/// This is used to populate default namespace hotkeys (2-9).
///
/// Uses FluxResourceKind enum to query all Flux resource types dynamically,
/// avoiding hardcoded resource types and API versions.
pub async fn discover_namespaces_with_flux_resources(client: &Client) -> Result<Vec<String>> {
    use crate::models::FluxResourceKind;
    use crate::tui::get_gvk_for_resource_type;
    use kube::api::{Api, ListParams};
    use kube::core::{ApiResource, DynamicObject};

    // Map to count resources per namespace
    let mut namespace_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    // Query all Flux resource types using FluxResourceKind enum
    // This ensures we use the single source of truth for resource definitions
    for kind in FluxResourceKind::all() {
        let resource_type = kind.as_str();

        // Get API metadata from the resource type using the centralized function
        match get_gvk_for_resource_type(resource_type) {
            Ok((group, version, plural)) => {
                let api_resource = ApiResource {
                    group: group.clone(),
                    version: version.clone(),
                    api_version: format!("{}/{}", group, version),
                    kind: String::new(), // Not needed for listing
                    plural: plural.clone(),
                };

                let api: Api<DynamicObject> = Api::all_with(client.clone(), &api_resource);

                // List all resources of this type across all namespaces
                // Ignore errors for individual resource types (some may not exist in cluster)
                if let Ok(list) = api.list(&ListParams::default()).await {
                    for item in list.items {
                        if let Some(namespace) = item.metadata.namespace.as_ref() {
                            *namespace_counts.entry(namespace.clone()).or_insert(0) += 1;
                        }
                    }
                }
            }
            Err(e) => {
                // Log but continue - some resource types might not be available
                tracing::debug!("Failed to get API metadata for {}: {}", resource_type, e);
            }
        }
    }

    // Sort by count (descending), then by name
    let mut namespaces: Vec<(String, usize)> = namespace_counts.into_iter().collect();
    namespaces.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    // Return just the namespace names
    Ok(namespaces.into_iter().map(|(ns, _)| ns).collect())
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

    #[tokio::test]
    async fn test_get_context_with_kubeconfig() {
        // This test will only pass if a kubeconfig exists
        // In CI environments without kubeconfig, this will fail gracefully
        if load_kubeconfig().is_ok() {
            let context = get_context().await;
            // Should return either a context name or "default"
            assert!(context.is_ok());
            let ctx_name = context.unwrap();
            assert!(!ctx_name.is_empty());
        }
    }

    #[test]
    fn test_list_contexts_with_kubeconfig() {
        // This test will only pass if a kubeconfig exists
        // In CI environments without kubeconfig, this will fail gracefully
        if let Ok(contexts) = list_contexts() {
            assert!(!contexts.is_empty(), "Should have at least one context");
            // All context names should be non-empty strings
            for ctx in &contexts {
                assert!(!ctx.is_empty(), "Context name should not be empty");
            }
        }
    }

    #[test]
    fn test_load_kubeconfig_error_handling() {
        // Test that load_kubeconfig provides a meaningful error message
        // We can't easily test the success case without a real kubeconfig,
        // but we can verify the error handling
        let result = load_kubeconfig();
        // Either succeeds (if kubeconfig exists) or provides a clear error
        match result {
            Ok(_kubeconfig) => {
                // If kubeconfig exists, it's valid
                // The structure is verified by successful parsing
            }
            Err(e) => {
                // Error should be descriptive
                let error_msg = e.to_string();
                assert!(
                    error_msg.contains("kubeconfig") || error_msg.contains("Failed to load"),
                    "Error message should mention kubeconfig: {}",
                    error_msg
                );
            }
        }
    }

    #[tokio::test]
    async fn test_create_client_for_context_invalid_context() {
        // Test that create_client_for_context returns an error for invalid context
        // This test requires a kubeconfig to exist
        if load_kubeconfig().is_ok() {
            let result = create_client_for_context("nonexistent-context-12345").await;
            assert!(result.is_err());
            // Convert error to string to check message
            if let Err(e) = result {
                let error_msg = e.to_string();
                assert!(
                    error_msg.contains("not found") || error_msg.contains("Context"),
                    "Error should mention context not found: {}",
                    error_msg
                );
            }
        }
    }

    #[tokio::test]
    async fn test_create_client_for_context_validates_context_exists() {
        // Test that create_client_for_context validates context exists before creating client
        if let Ok(contexts) = list_contexts() {
            if !contexts.is_empty() {
                // Try to create client with a context that doesn't exist
                let invalid_result =
                    create_client_for_context("definitely-does-not-exist-xyz").await;
                assert!(invalid_result.is_err());
                // Convert error to string to check message
                if let Err(e) = invalid_result {
                    let error_msg = e.to_string();
                    // Error should list available contexts
                    assert!(
                        error_msg.contains("Available contexts") || error_msg.contains("not found"),
                        "Error should mention available contexts: {}",
                        error_msg
                    );
                }
            }
        }
    }

    #[test]
    fn test_get_context_from_kubeconfig_path_file_not_exists() {
        use std::path::PathBuf;
        let non_existent_path = PathBuf::from("/nonexistent/path/to/kubeconfig");
        let _ = non_existent_path; // Used in error message check below
        let result = get_context_from_kubeconfig_path(&non_existent_path);
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("does not exist"),
            "Error should mention file does not exist: {}",
            error_msg
        );
        assert!(
            error_msg.contains(non_existent_path.to_string_lossy().as_ref()),
            "Error should include the path: {}",
            error_msg
        );
    }

    #[test]
    fn test_get_context_from_kubeconfig_path_is_directory() {
        // Use a temp directory that definitely exists
        let temp_dir = std::env::temp_dir();
        let result = get_context_from_kubeconfig_path(&temp_dir);
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("not a file"),
            "Error should mention path is not a file: {}",
            error_msg
        );
    }

    #[test]
    fn test_get_context_from_kubeconfig_path_invalid_format() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        // Create a temporary file with invalid YAML/kubeconfig content
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "this is not valid yaml: [").unwrap();
        temp_file.flush().unwrap();

        let result = get_context_from_kubeconfig_path(temp_file.path());
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("Failed to load") || error_msg.contains("parse"),
            "Error should mention parsing/loading failure: {}",
            error_msg
        );
        assert!(
            error_msg.contains("valid kubeconfig"),
            "Error should mention valid kubeconfig: {}",
            error_msg
        );
    }

    #[test]
    fn test_get_context_from_kubeconfig_path_no_contexts() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        // Create a valid kubeconfig structure but with no contexts
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "apiVersion: v1").unwrap();
        writeln!(temp_file, "kind: Config").unwrap();
        writeln!(temp_file, "contexts: []").unwrap();
        writeln!(temp_file, "current-context: \"\"").unwrap();
        temp_file.flush().unwrap();

        let result = get_context_from_kubeconfig_path(temp_file.path());
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("no contexts"),
            "Error should mention no contexts: {}",
            error_msg
        );
    }

    #[test]
    fn test_get_context_from_kubeconfig_path_no_current_context() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        // Create a valid kubeconfig with contexts but no current context
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "apiVersion: v1").unwrap();
        writeln!(temp_file, "kind: Config").unwrap();
        writeln!(temp_file, "contexts:").unwrap();
        writeln!(temp_file, "  - name: test-context").unwrap();
        writeln!(temp_file, "    context:").unwrap();
        writeln!(temp_file, "      cluster: test-cluster").unwrap();
        writeln!(temp_file, "      user: test-user").unwrap();
        writeln!(temp_file, "clusters:").unwrap();
        writeln!(temp_file, "  - name: test-cluster").unwrap();
        writeln!(temp_file, "    cluster:").unwrap();
        writeln!(temp_file, "      server: https://test.example.com").unwrap();
        writeln!(temp_file, "users:").unwrap();
        writeln!(temp_file, "  - name: test-user").unwrap();
        temp_file.flush().unwrap();

        let result = get_context_from_kubeconfig_path(temp_file.path());
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("No current context"),
            "Error should mention no current context: {}",
            error_msg
        );
        assert!(
            error_msg.contains("test-context"),
            "Error should list available contexts: {}",
            error_msg
        );
    }

    #[test]
    fn test_get_context_from_kubeconfig_path_current_context_not_exists() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        // Create a valid kubeconfig with current context that doesn't exist in contexts list
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "apiVersion: v1").unwrap();
        writeln!(temp_file, "kind: Config").unwrap();
        writeln!(temp_file, "current-context: nonexistent-context").unwrap();
        writeln!(temp_file, "contexts:").unwrap();
        writeln!(temp_file, "  - name: test-context").unwrap();
        writeln!(temp_file, "    context:").unwrap();
        writeln!(temp_file, "      cluster: test-cluster").unwrap();
        writeln!(temp_file, "      user: test-user").unwrap();
        writeln!(temp_file, "clusters:").unwrap();
        writeln!(temp_file, "  - name: test-cluster").unwrap();
        writeln!(temp_file, "    cluster:").unwrap();
        writeln!(temp_file, "      server: https://test.example.com").unwrap();
        writeln!(temp_file, "users:").unwrap();
        writeln!(temp_file, "  - name: test-user").unwrap();
        temp_file.flush().unwrap();

        let result = get_context_from_kubeconfig_path(temp_file.path());
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("does not exist") || error_msg.contains("not exist"),
            "Error should mention context does not exist: {}",
            error_msg
        );
        assert!(
            error_msg.contains("nonexistent-context"),
            "Error should mention the invalid context name: {}",
            error_msg
        );
        assert!(
            error_msg.contains("test-context"),
            "Error should list available contexts: {}",
            error_msg
        );
    }

    #[test]
    fn test_get_context_from_kubeconfig_path_valid() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        // Create a valid kubeconfig with valid current context
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "apiVersion: v1").unwrap();
        writeln!(temp_file, "kind: Config").unwrap();
        writeln!(temp_file, "current-context: test-context").unwrap();
        writeln!(temp_file, "contexts:").unwrap();
        writeln!(temp_file, "  - name: test-context").unwrap();
        writeln!(temp_file, "    context:").unwrap();
        writeln!(temp_file, "      cluster: test-cluster").unwrap();
        writeln!(temp_file, "      user: test-user").unwrap();
        writeln!(temp_file, "clusters:").unwrap();
        writeln!(temp_file, "  - name: test-cluster").unwrap();
        writeln!(temp_file, "    cluster:").unwrap();
        writeln!(temp_file, "      server: https://test.example.com").unwrap();
        writeln!(temp_file, "users:").unwrap();
        writeln!(temp_file, "  - name: test-user").unwrap();
        temp_file.flush().unwrap();

        let result = get_context_from_kubeconfig_path(temp_file.path());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "test-context");
    }

    #[test]
    fn test_get_context_from_kubeconfig_path_empty_current_context() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        // Create a valid kubeconfig with empty current context string
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "apiVersion: v1").unwrap();
        writeln!(temp_file, "kind: Config").unwrap();
        writeln!(temp_file, "current-context: \"\"").unwrap();
        writeln!(temp_file, "contexts:").unwrap();
        writeln!(temp_file, "  - name: test-context").unwrap();
        writeln!(temp_file, "    context:").unwrap();
        writeln!(temp_file, "      cluster: test-cluster").unwrap();
        writeln!(temp_file, "      user: test-user").unwrap();
        writeln!(temp_file, "clusters:").unwrap();
        writeln!(temp_file, "  - name: test-cluster").unwrap();
        writeln!(temp_file, "    cluster:").unwrap();
        writeln!(temp_file, "      server: https://test.example.com").unwrap();
        writeln!(temp_file, "users:").unwrap();
        writeln!(temp_file, "  - name: test-user").unwrap();
        temp_file.flush().unwrap();

        let result = get_context_from_kubeconfig_path(temp_file.path());
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("No current context"),
            "Error should mention no current context: {}",
            error_msg
        );
    }
}
