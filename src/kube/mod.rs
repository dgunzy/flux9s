//! Kubernetes client module
//!
//! Handles connection to Kubernetes API server and provides
//! a configured client for use throughout the application.

use anyhow::Result;
use kube::{Client, Config};

/// Initialize and return a Kubernetes client
///
/// Uses the default kubeconfig loading strategy:
/// 1. In-cluster config (if running in a pod)
/// 2. KUBECONFIG environment variable
/// 3. ~/.kube/config
pub async fn create_client() -> Result<Client> {
    let config = Config::infer().await?;
    let client = Client::try_from(config)?;
    Ok(client)
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
