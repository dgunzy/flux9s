//! Plugin CLI commands
//!
//! Provides commands for managing flux9s plugins.

use crate::plugins::{PluginLoader, PluginValidator};
use anyhow::{Context, Result};
use clap::Subcommand;
use std::path::{Path, PathBuf};

/// Convert a string to title case (e.g., "my-plugin" -> "My Plugin")
fn to_title_case(s: &str) -> String {
    s.split(['-', '_'])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Plugin subcommands
#[derive(Subcommand, Debug)]
pub enum PluginSubcommand {
    /// List loaded plugins
    List,

    /// Validate a plugin YAML file
    Validate {
        /// Path to plugin YAML file
        path: PathBuf,
    },

    /// Generate a plugin template
    Init {
        /// Plugin name
        name: String,

        /// Output path (defaults to plugins directory)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Install a plugin from a file
    Install {
        /// Path to plugin YAML file
        path: PathBuf,
    },

    /// Uninstall a plugin
    Uninstall {
        /// Plugin name
        name: String,
    },
}

/// Handle plugin CLI commands
pub async fn handle_plugin_command(subcommand: PluginSubcommand) -> Result<()> {
    tracing::debug!("Handling plugin command: {:?}", subcommand);

    match subcommand {
        PluginSubcommand::List => list_plugins().await,
        PluginSubcommand::Validate { path } => validate_plugin(&path).await,
        PluginSubcommand::Init { name, output } => init_plugin(&name, output.as_deref()).await,
        PluginSubcommand::Install { path } => install_plugin(&path).await,
        PluginSubcommand::Uninstall { name } => uninstall_plugin(&name).await,
    }
}

/// List all loaded plugins
async fn list_plugins() -> Result<()> {
    let loader = PluginLoader::new()?;
    let plugins = loader.load_all()?;

    if plugins.is_empty() {
        println!("No plugins found.");
        println!(
            "\nPlugins directory: {:?}",
            PluginLoader::get_plugins_dir()?
        );
        println!("To create a plugin: flux9s plugin init <name>");
        return Ok(());
    }

    println!("Loaded plugins ({}):\n", plugins.len());

    for plugin in plugins {
        println!("  {} (v{})", plugin.name, plugin.version);
        if let Some(desc) = &plugin.description {
            println!("    Description: {}", desc);
        }
        if let Some(ref source) = plugin.source {
            println!("    Source: {:?}", source.source_type);
            println!("    Resources: {}", plugin.resources.join(", "));
        }
        if !plugin.columns.is_empty() {
            println!("    Columns: {}", plugin.columns.len());
        }
        if !plugin.views.is_empty() {
            println!("    Views: {}", plugin.views.len());
        }
        if !plugin.watched_resources.is_empty() {
            println!("    Watched resources: {}", plugin.watched_resources.len());
            for watched in &plugin.watched_resources {
                println!("      - {} ({})", watched.display_name(), watched.command);
            }
        }
        println!();
    }

    Ok(())
}

/// Validate a plugin YAML file
async fn validate_plugin(path: &PathBuf) -> Result<()> {
    println!("Validating plugin: {:?}", path);

    // Check if file exists
    if !path.exists() {
        anyhow::bail!("Plugin file not found: {:?}", path);
    }

    // Load and validate
    let loader = PluginLoader::new()?;
    let plugin = loader
        .load_plugin(path)
        .context("Plugin validation failed")?;

    // Additional validation
    PluginValidator::validate(&plugin)?;

    println!("✓ Plugin is valid!");
    println!("\nPlugin details:");
    println!("  Name: {}", plugin.name);
    println!("  Version: {}", plugin.version);
    println!("  Enabled: {}", plugin.enabled);
    if let Some(desc) = &plugin.description {
        println!("  Description: {}", desc);
    }
    if let Some(ref source) = plugin.source {
        println!("  Source type: {:?}", source.source_type);
        println!("  Resources: {}", plugin.resources.len());
    }
    if !plugin.columns.is_empty() {
        println!("  Columns: {}", plugin.columns.len());
    }
    if !plugin.views.is_empty() {
        println!("  Views: {}", plugin.views.len());
    }
    if !plugin.watched_resources.is_empty() {
        println!("  Watched resources: {}", plugin.watched_resources.len());
        for watched in &plugin.watched_resources {
            println!("    - {} ({})", watched.display_name(), watched.command);
        }
    }

    Ok(())
}

/// Generate a plugin template
async fn init_plugin(name: &str, output: Option<&Path>) -> Result<()> {
    // Determine output path
    let output_path = if let Some(path) = output {
        path.to_path_buf()
    } else {
        let plugins_dir = PluginLoader::get_plugins_dir()?;
        std::fs::create_dir_all(&plugins_dir).context("Failed to create plugins directory")?;
        plugins_dir.join(format!("{}.yaml", name))
    };

    // Check if file already exists
    if output_path.exists() {
        anyhow::bail!(
            "Plugin file already exists: {:?}\nUse a different name or specify --output",
            output_path
        );
    }

    // Generate template - focused on watched_resources (CRD watching)
    let template = format!(
        r#"name: {name}
version: 1.0.0
enabled: true
description: "{name} plugin - watch custom Kubernetes CRDs"

# =============================================================================
# WATCHED RESOURCES - Create new resource views for any Kubernetes CRD
# =============================================================================
# This is the primary plugin pattern. Each watched resource:
# - Registers a command (e.g., ":{name}") to access the view
# - Watches the CRD via Kubernetes API (real-time updates)
# - Displays custom columns you define
# - Supports YAML view, describe, and status indicators

watched_resources:
  - type: kubernetes_crd        # Required: resource type (see Future Types below)
    kind: MyResource            # CRD kind (e.g., "Application", "Certificate")
    group: example.com          # API group (e.g., "argoproj.io", "cert-manager.io")
    version: v1                 # API version (e.g., "v1", "v1alpha1")
    plural: myresources         # Plural name for API calls
    command: ":{name}"          # Command to access this view
    display_name: "{display}"   # Shown in header (defaults to kind)
    supports_yaml: true         # Enable 'y' key for YAML view
    supports_describe: true     # Enable 'd' key for describe view

    # Column definitions for the list view
    columns:
      - name: NAME
        path: .metadata.name
        width: 30
      - name: NAMESPACE
        path: .metadata.namespace
        width: 20
      - name: STATUS
        path: .status.phase       # Adjust to your CRD's status field
        width: 12
        renderer: status_badge    # Renderers: text, status_badge, duration, age, boolean
      - name: AGE
        path: .metadata.creationTimestamp
        width: 10
        renderer: age

    # Optional: Status extraction for ready/suspended indicators
    status:
      ready_path: .status.conditions[0].status    # JSONPath to ready indicator
      ready_value: "True"                          # Value that means "ready"
      message_path: .status.conditions[0].message  # Status message to display

# =============================================================================
# FUTURE RESOURCE TYPES (not yet implemented, architecture supports these)
# =============================================================================
# When these types are implemented, you'll be able to watch non-CRD sources:
#
#   - type: http_api              # Poll an HTTP/REST API endpoint
#     endpoint: https://api.example.com/resources
#     refresh_interval: 30s
#     auth:
#       type: bearer
#       token_env: API_TOKEN
#     command: ":myapi"
#     columns: [...]
#
#   - type: grpc                  # Stream from a gRPC service
#     endpoint: grpc.example.com:443
#     service: MyService
#     method: WatchResources
#     command: ":mygrpc"
#     columns: [...]

# =============================================================================
# COLUMN ENRICHMENT (optional) - Add columns to existing Flux resource views
# =============================================================================
# Use this pattern to fetch data from an external source and add columns
# to existing Flux resources (Kustomization, HelmRelease, etc.)
#
# source:
#   type: kubernetes_service     # Options: kubernetes_service, http, file
#   service: my-data-service
#   namespace: default
#   port: 8080
#   path: /api/data
#   refresh_interval: 30s
#
#   # Note: Kubernetes DNS suffix is configured in flux9s config file
#   # Default: ".svc.cluster.local"
#   # Override in config.yaml: plugin.kubernetesDnsSuffix: ".svc.cluster.local"
#
# resources:                     # Which Flux resources to enrich
#   - Kustomization
#   - HelmRelease
#
# columns:                       # Columns to add (data from source)
#   - name: owner
#     path: .ownership.owner     # JSONPath into source data
#     width: 15
#     renderer: text
"#,
        name = name,
        display = to_title_case(name)
    );

    // Write template
    std::fs::write(&output_path, template)
        .with_context(|| format!("Failed to write plugin template: {:?}", output_path))?;

    println!("✓ Plugin template created: {:?}", output_path);
    println!("\nNext steps:");
    println!("  1. Edit the plugin file to configure your data source");
    println!("  2. Validate: flux9s plugin validate {:?}", output_path);
    println!("  3. The plugin will be loaded automatically when flux9s starts");

    Ok(())
}

/// Install a plugin from a file
async fn install_plugin(path: &PathBuf) -> Result<()> {
    println!("Installing plugin from: {:?}", path);
    tracing::info!("Installing plugin from: {:?}", path);

    // Validate first
    println!("Validating plugin...");
    tracing::debug!("Validating plugin before installation");
    let loader = PluginLoader::new()?;
    let plugin = loader
        .load_plugin(path)
        .context("Plugin validation failed")?;

    tracing::info!(
        "Plugin '{}' v{} validated successfully",
        plugin.name,
        plugin.version
    );

    // Determine installation path
    let plugins_dir = PluginLoader::get_plugins_dir()?;
    std::fs::create_dir_all(&plugins_dir).context("Failed to create plugins directory")?;

    let install_path = plugins_dir.join(format!("{}.yaml", plugin.name));

    // Check if already installed
    if install_path.exists() {
        anyhow::bail!(
            "Plugin '{}' is already installed at: {:?}\nUninstall first with: flux9s plugin uninstall {}",
            plugin.name,
            install_path,
            plugin.name
        );
    }

    // Copy file to plugins directory
    std::fs::copy(path, &install_path)
        .with_context(|| format!("Failed to copy plugin to: {:?}", install_path))?;

    tracing::info!("Plugin '{}' installed to: {:?}", plugin.name, install_path);

    println!("✓ Plugin installed: {}", plugin.name);
    println!("  Location: {:?}", install_path);
    println!("\nThe plugin will be loaded automatically when flux9s starts.");

    Ok(())
}

/// Uninstall a plugin
async fn uninstall_plugin(name: &str) -> Result<()> {
    let plugins_dir = PluginLoader::get_plugins_dir()?;
    let plugin_path = plugins_dir.join(format!("{}.yaml", name));

    if !plugin_path.exists() {
        anyhow::bail!("Plugin '{}' is not installed", name);
    }

    // Remove the file
    std::fs::remove_file(&plugin_path)
        .with_context(|| format!("Failed to remove plugin file: {:?}", plugin_path))?;

    println!("✓ Plugin uninstalled: {}", name);

    Ok(())
}
