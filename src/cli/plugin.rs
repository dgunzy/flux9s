//! Plugin CLI commands
//!
//! Provides commands for managing flux9s plugins.

use crate::plugins::{PluginLoader, PluginValidator};
use anyhow::{Context, Result};
use clap::Subcommand;
use std::path::{Path, PathBuf};

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
        println!("    Source: {:?}", plugin.source.source_type);
        println!("    Resources: {}", plugin.resources.join(", "));
        println!("    Columns: {}", plugin.columns.len());
        println!("    Views: {}", plugin.views.len());
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
    println!("  Source type: {:?}", plugin.source.source_type);
    println!("  Resources: {}", plugin.resources.len());
    println!("  Columns: {}", plugin.columns.len());
    println!("  Views: {}", plugin.views.len());

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

    // Generate template
    let template = format!(
        r#"name: {name}
version: 1.0.0
enabled: true
description: "{name} plugin"

# Data source configuration
source:
  # Option 1: Kubernetes Service (MOST COMMON)
  type: kubernetes_service
  service: my-service
  namespace: default
  port: 8080
  path: /api/data
  refresh_interval: 30s

  # Option 2: Kubernetes CRD
  # type: kubernetes_crd
  # kind: MyData
  # group: example.com
  # version: v1
  # namespace: default
  # data_path: .status.data

  # Option 3: External HTTP API
  # type: http
  # endpoint: https://api.example.com/data
  # refresh_interval: 30s

  # Option 4: Local file (for testing)
  # type: file
  # file_path: /path/to/data.json

# Resources this plugin enhances (ANY Kubernetes resource)
resources:
  - Deployment
  - Service

# Columns to add to resource list view
columns:
  - name: status
    path: .status        # JSONPath to extract value
    width: 10
    enabled: true
    renderer: text       # text, issue_badge, percentage_bar, duration

# Custom detail views (optional)
views: []
  # - name: my_view
  #   keybinding: ":myview"
  #   description: "My custom view"
"#,
        name = name
    );

    // Write template
    std::fs::write(&output_path, template)
        .with_context(|| format!("Failed to write plugin template: {:?}", output_path))?;

    println!("✓ Plugin template created: {:?}", output_path);
    println!("\nNext steps:");
    println!("  1. Edit the plugin file to configure your data source");
    println!("  2. Validate: flux9s plugin validate {:?}", output_path);
    println!("  3. Install: flux9s plugin install {:?}", output_path);
    println!("  4. The plugin will be loaded automatically when flux9s starts");

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

    tracing::info!("Plugin '{}' v{} validated successfully", plugin.name, plugin.version);

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
