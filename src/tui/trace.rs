//! TUI trace rendering and formatting
//!
//! This module provides rendering functionality for trace results.
//! The core trace logic is in the `crate::trace` module.

// Re-export from the trace module
pub use crate::trace::{TraceNode, TraceResult, trace_object};

/// Format trace result as a string (similar to flux trace output)
#[allow(dead_code)] // May be used for debugging or future CLI output
pub fn format_trace_result(result: &TraceResult) -> String {
    let mut output = Vec::new();

    // Main Object header - highlighted as the primary resource
    output.push("═══════════════════════════════════════════════════════════".to_string());
    output.push(format!(
        "  Object:          {}/{}",
        result.object.kind, result.object.name
    ));
    output.push(format!("  Namespace:       {}", result.object.namespace));
    output.push("  Status:          Managed by Flux".to_string());
    output.push("═══════════════════════════════════════════════════════════".to_string());

    // Chain (Kustomization/HelmRelease/HelmChart)
    // When tracing a Kustomization/HelmRelease directly, show it in chain to match Flux CLI
    for node in &result.chain {
        use crate::models::FluxResourceKind;
        if matches!(
            FluxResourceKind::parse_optional(&node.kind),
            Some(FluxResourceKind::Kustomization)
                | Some(FluxResourceKind::HelmRelease)
                | Some(FluxResourceKind::HelmChart)
        ) {
            // Add flow arrow with ASCII art
            output.push("".to_string());
            output.push("                    |".to_string());
            output.push("                    v".to_string());
            output.push("            managed by".to_string());
            output.push("".to_string());

            output.push("───────────────────────────────────────────────────".to_string());
            output.push(format!("{}:   {}", node.kind, node.name));
            output.push(format!("Namespace:       {}", node.namespace));

            if let Some(ref spec) = node.spec {
                if let Some(ref path) = spec.path {
                    output.push(format!("Path:            {}", path));
                }
            }

            if let Some(ref status) = node.status {
                if let Some(ref revision) = status.revision {
                    output.push(format!("Revision:        {}", revision));
                }
                if let Some(ref last_reconciled) = status.last_reconciled {
                    output.push(format!(
                        "Status:          Last reconciled at {}",
                        last_reconciled
                    ));
                }
                if let Some(ref message) = status.message {
                    output.push(format!("Message:         {}", message));
                }
                // Show Ready status
                if let Some(ready) = status.ready {
                    let ready_str = if ready { "True" } else { "False" };
                    output.push(format!("Ready:           {}", ready_str));
                }
            }
            output.push("───────────────────────────────────────────────────".to_string());
        }
    }

    // Source (GitRepository, OCIRepository, HelmRepository, ExternalArtifact, etc.)
    if let Some(ref source) = result.source {
        // Add flow arrow with ASCII art
        output.push("".to_string());
        output.push("                    |".to_string());
        output.push("                    v".to_string());
        output.push("            sourced from".to_string());
        output.push("".to_string());

        output.push("───────────────────────────────────────────────────".to_string());
        output.push(format!("{}:   {}", source.kind, source.name));
        output.push(format!("Namespace:       {}", source.namespace));

        if let Some(ref spec) = source.spec {
            if let Some(ref url) = spec.url {
                output.push(format!("URL:             {}", url));
            }
            if let Some(ref branch) = spec.branch {
                output.push(format!("Branch:          {}", branch));
            }
        }

        if let Some(ref status) = source.status {
            if let Some(ref revision) = status.revision {
                output.push(format!("Revision:        {}", revision));
            }
            if let Some(ref last_reconciled) = status.last_reconciled {
                output.push(format!(
                    "Status:          Last reconciled at {}",
                    last_reconciled
                ));
            }
            if let Some(ref message) = status.message {
                output.push(format!("Message:         {}", message));
            }
            // Show Ready status
            if let Some(ready) = status.ready {
                let ready_str = if ready { "True" } else { "False" };
                output.push(format!("Ready:           {}", ready_str));
            }
        }
        output.push("───────────────────────────────────────────────────".to_string());
    }

    output.join("\n")
}
