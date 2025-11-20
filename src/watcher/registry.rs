//! Resource registry for maintainable CRD registration
//!
//! This module provides a simple way to register Flux CRD resources.
//! To add a new resource type, simply add it to the RESOURCE_REGISTRY macro below.
//!
//! The registry automatically:
//! - Implements WatchableResource trait
//! - Registers it for watching
//! - Adds command mode support
//!
//! For resources that need custom code (like Kustomization), you can still
//! add custom implementations in resource.rs after the registry.

/// Registry entry for a Flux resource type
pub struct ResourceEntry {
    pub display_name: &'static str,
    pub command_aliases: &'static [&'static str],
}

/// Registry of all Flux resources
///
/// To add a new resource:
/// 1. Ensure it's implemented in src/watcher/resource.rs with impl_watchable!
/// 2. Add an entry here with display name and command aliases
/// 3. Add the watch call in watch_all() in mod.rs
pub const RESOURCE_REGISTRY: &[ResourceEntry] = &[
    // Source Controller resources
    ResourceEntry {
        display_name: "GitRepository",
        command_aliases: &["gitrepository", "gitrepo", "gitrepositories"],
    },
    ResourceEntry {
        display_name: "OCIRepository",
        command_aliases: &["ocirepository", "oci", "ocirepositories"],
    },
    ResourceEntry {
        display_name: "HelmRepository",
        command_aliases: &["helmrepository", "helmrepositories"],
    },
    ResourceEntry {
        display_name: "Bucket",
        command_aliases: &["bucket", "buckets"],
    },
    ResourceEntry {
        display_name: "HelmChart",
        command_aliases: &["helmchart", "helmcharts"],
    },
    ResourceEntry {
        display_name: "ExternalArtifact",
        command_aliases: &["externalartifact", "externalartifacts", "ea"],
    },
    // Kustomize Controller resources
    ResourceEntry {
        display_name: "Kustomization",
        command_aliases: &["kustomization", "ks", "kustomizations"],
    },
    // Helm Controller resources
    ResourceEntry {
        display_name: "HelmRelease",
        command_aliases: &["helmrelease", "hr", "helmreleases"],
    },
    // Image Reflector Controller resources
    ResourceEntry {
        display_name: "ImageRepository",
        command_aliases: &["imagerepository", "imagerepositories"],
    },
    ResourceEntry {
        display_name: "ImagePolicy",
        command_aliases: &["imagepolicy", "imagepolicies"],
    },
    // Image Automation Controller resources
    ResourceEntry {
        display_name: "ImageUpdateAutomation",
        command_aliases: &["imageupdateautomation", "imageupdateautomations"],
    },
    // Notification Controller resources
    ResourceEntry {
        display_name: "Alert",
        command_aliases: &["alert", "alerts"],
    },
    ResourceEntry {
        display_name: "Provider",
        command_aliases: &["provider", "providers"],
    },
    ResourceEntry {
        display_name: "Receiver",
        command_aliases: &["receiver", "receivers"],
    },
    // Flux Operator resources
    ResourceEntry {
        display_name: "ResourceSet",
        command_aliases: &["resourceset", "resourcesets", "rset"],
    },
    ResourceEntry {
        display_name: "ResourceSetInputProvider",
        command_aliases: &[
            "resourcesetinputprovider",
            "resourcesetinputproviders",
            "rsip",
        ],
    },
    ResourceEntry {
        display_name: "FluxReport",
        command_aliases: &["fluxreport", "fluxreports", "fr"],
    },
    ResourceEntry {
        display_name: "FluxInstance",
        command_aliases: &["fluxinstance", "fluxinstances", "fi"],
    },
];

/// Get display name for a command alias
pub fn get_display_name_for_command(cmd: &str) -> Option<&'static str> {
    let cmd_lower = cmd.to_lowercase();
    for entry in RESOURCE_REGISTRY {
        if entry
            .command_aliases
            .iter()
            .any(|&alias| alias == cmd_lower)
        {
            return Some(entry.display_name);
        }
    }
    None
}

/// Get all command aliases for help text
pub fn get_all_commands() -> Vec<(&'static str, &'static [&'static str])> {
    RESOURCE_REGISTRY
        .iter()
        .map(|e| (e.display_name, e.command_aliases))
        .collect()
}
