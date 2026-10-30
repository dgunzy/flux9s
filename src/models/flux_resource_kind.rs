//! Flux Resource Kind definitions
//!
//! This module provides a centralized enum for all Flux CRD resource kinds.
//! This eliminates hardcoded strings throughout the codebase and provides
//! type safety for resource kind references.

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use serde_json::Value;

// Column and field name constants
pub mod field_names {
    // Common columns
    pub const STATUS: &str = "STATUS";
    pub const NAMESPACE: &str = "NAMESPACE";
    pub const NAME: &str = "NAME";
    pub const TYPE: &str = "TYPE";
    pub const SUSPENDED: &str = "SUSPENDED";
    pub const READY: &str = "READY";
    pub const MESSAGE: &str = "MESSAGE";

    // Resource-specific fields
    pub const URL: &str = "URL";
    pub const BRANCH: &str = "BRANCH";
    pub const REVISION: &str = "REVISION";
    pub const SEMVER: &str = "SEMVER";
    pub const DIGEST: &str = "DIGEST";
    pub const PATH: &str = "PATH";
    pub const PRUNE: &str = "PRUNE";
    pub const CHART: &str = "CHART";
    pub const VERSION: &str = "VERSION";
    pub const SOURCE: &str = "SOURCE";
    pub const IMAGE: &str = "IMAGE";
    pub const TAG: &str = "TAG";
    pub const INTERVAL: &str = "INTERVAL";
    pub const SECRET: &str = "SECRET";
    pub const ENDPOINT: &str = "ENDPOINT";
    pub const PROVIDER: &str = "PROVIDER";
    pub const CHANNEL: &str = "CHANNEL";
    pub const ADDRESS: &str = "ADDRESS";
    pub const WEBHOOK: &str = "WEBHOOK";
    pub const INPUTS: &str = "INPUTS";
}

/// Labels and annotations Flux puts on the objects it manages.
pub mod labels {
    /// `app.kubernetes.io/managed-by`; the Flux Operator sets it to
    /// [`FLUX_OPERATOR`] on what a FluxInstance applies.
    pub const MANAGED_BY: &str = "app.kubernetes.io/managed-by";
    pub const FLUX_OPERATOR: &str = "flux-operator";
    /// The FluxInstance an operator-applied object belongs to.
    pub const OPERATOR_NAME: &str = "fluxcd.controlplane.io/name";
    pub const OPERATOR_NAMESPACE: &str = "fluxcd.controlplane.io/namespace";
    /// `disabled` opts an object out of Flux garbage collection.
    pub const PRUNE: &str = "kustomize.toolkit.fluxcd.io/prune";
}

/// How a kind's reconciliation is switched off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuspendSwitch {
    /// `spec.suspend: true`.
    Spec,
    /// The annotation set to `disabled` (Flux Operator kinds,
    /// ArtifactGenerator); resumed with `enabled`.
    Annotation(&'static str),
}

impl SuspendSwitch {
    /// Whether `obj` is suspended.
    pub fn is_suspended(self, obj: &Value) -> bool {
        match self {
            SuspendSwitch::Spec => obj
                .pointer("/spec/suspend")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            SuspendSwitch::Annotation(key) => {
                obj.pointer("/metadata/annotations")
                    .and_then(|a| a.get(key))
                    .and_then(Value::as_str)
                    == Some("disabled")
            }
        }
    }

    /// The merge patch that suspends (or resumes) an object.
    pub fn patch(self, suspend: bool) -> Value {
        match self {
            SuspendSwitch::Spec => serde_json::json!({ "spec": { "suspend": suspend } }),
            SuspendSwitch::Annotation(key) => serde_json::json!({
                "metadata": { "annotations": { key: if suspend { "disabled" } else { "enabled" } } }
            }),
        }
    }
}

/// Declares every Flux kind once: its variant (named exactly as the CRD's
/// `kind`), the generated model it is read with, and its `:` command
/// aliases. The enum, [`FluxResourceKind::ALL`], the name ↔ kind mappings,
/// the aliases, and the API group/version/plural ([`FluxResourceKind::api_resource`],
/// read from the generated model's `#[kube(...)]` attributes) all come from
/// this one table, so adding a kind is one line and nothing can drift.
macro_rules! flux_kinds {
    ($( $(#[$doc:meta])* $variant:ident => $model:ty, [$($alias:literal),+ $(,)?]; )+) => {
        /// Enumeration of all Flux CRD resource kinds
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum FluxResourceKind {
            $( $(#[$doc])* $variant, )+
        }

        impl FluxResourceKind {
            /// Every Flux kind, in table order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// The kind's name, as the API server spells it.
            pub fn as_str(&self) -> &'static str {
                match self {
                    $( Self::$variant => stringify!($variant), )+
                }
            }

            /// The `:` command names for this kind (lowercase): its name,
            /// plural, and short aliases.
            pub fn aliases(&self) -> &'static [&'static str] {
                match self {
                    $( Self::$variant => &[$($alias),+], )+
                }
            }

            /// Group, current version, and plural, taken from the generated
            /// model — the version flux9s asks for first.
            pub fn api_resource(&self) -> kube::core::ApiResource {
                match self {
                    $( Self::$variant => kube::core::ApiResource::erase::<$model>(&()), )+
                }
            }

            fn from_name(s: &str) -> Option<Self> {
                match s {
                    $( stringify!($variant) => Some(Self::$variant), )+
                    _ => None,
                }
            }
        }
    };
}

use crate::models::_generated::{
    flux_operator_fluxinstances, flux_operator_fluxreports,
    flux_operator_resourcesetinputproviders, flux_operator_resourcesets, helm_controller,
    image_automation_controller, image_reflector_controller, kustomize_controller,
    notification_controller, source_controller, source_watcher,
};

flux_kinds! {
    // Source Controller resources
    GitRepository => source_controller::GitRepository, ["gitrepository", "gitrepo", "gitrepositories"];
    OCIRepository => source_controller::OciRepository, ["ocirepository", "oci", "ocirepositories"];
    HelmRepository => source_controller::HelmRepository, ["helmrepository", "helmrepo", "helmrepositories"];
    Bucket => source_controller::Bucket, ["bucket", "buckets"];
    HelmChart => source_controller::HelmChart, ["helmchart", "helmcharts"];
    ExternalArtifact => source_controller::ExternalArtifact, ["externalartifact", "externalartifacts", "ea"];
    ArtifactGenerator => source_watcher::ArtifactGenerator, ["artifactgenerator", "artifactgenerators", "ag"];
    // Kustomize Controller resources
    Kustomization => kustomize_controller::Kustomization, ["kustomization", "ks", "kustomizations"];
    // Helm Controller resources
    HelmRelease => helm_controller::HelmRelease, ["helmrelease", "hr", "helmreleases"];
    // Image Reflector Controller resources
    ImageRepository => image_reflector_controller::ImageRepository, ["imagerepository", "imagerepositories"];
    ImagePolicy => image_reflector_controller::ImagePolicy, ["imagepolicy", "imagepolicies"];
    // Image Automation Controller resources
    ImageUpdateAutomation => image_automation_controller::ImageUpdateAutomation, ["imageupdateautomation", "imageupdateautomations"];
    // Notification Controller resources
    Alert => notification_controller::Alert, ["alert", "alerts"];
    Provider => notification_controller::Provider, ["provider", "providers"];
    Receiver => notification_controller::Receiver, ["receiver", "receivers"];
    // Flux Operator resources
    ResourceSet => flux_operator_resourcesets::ResourceSet, ["resourceset", "resourcesets", "rset"];
    ResourceSetInputProvider => flux_operator_resourcesetinputproviders::ResourceSetInputProvider, ["resourcesetinputprovider", "resourcesetinputproviders", "rsip"];
    FluxReport => flux_operator_fluxreports::FluxReport, ["fluxreport", "fluxreports", "fr"];
    FluxInstance => flux_operator_fluxinstances::FluxInstance, ["fluxinstance", "fluxinstances", "fi"];
}

impl FluxResourceKind {
    /// API versions older Flux releases serve this kind at, newest first —
    /// tried in order when the current version (from the generated model)
    /// isn't served. Exactly the versions each kind has had, so no cluster
    /// is asked for one that never existed.
    pub fn older_api_versions(&self) -> &'static [&'static str] {
        match self {
            // v1 since Flux 2.3 (HelmRepository, HelmChart), 2.4 (Bucket),
            // and 2.6 (OCIRepository); v1beta2 before.
            Self::OCIRepository | Self::HelmRepository | Self::Bucket | Self::HelmChart => {
                &["v1beta2"]
            }
            // v2 since Flux 2.3; v2beta2 since 2.2.
            Self::HelmRelease => &["v2beta2", "v2beta1"],
            // v1 since Flux 2.10; v1beta3 since 2.2.
            Self::Alert | Self::Provider => &["v1beta3", "v1beta2"],
            // v1 since Flux 2.0.
            Self::Receiver => &["v1beta2"],
            // v1 since Flux 2.7.
            Self::ImageRepository | Self::ImagePolicy => &["v1beta2"],
            // v1 since Flux 2.7; v1beta2 since 2.3, v1beta1 before.
            Self::ImageUpdateAutomation => &["v1beta2", "v1beta1"],
            Self::GitRepository
            | Self::Kustomization
            | Self::ExternalArtifact
            | Self::ArtifactGenerator
            | Self::ResourceSet
            | Self::ResourceSetInputProvider
            | Self::FluxReport
            | Self::FluxInstance => &[],
        }
    }

    /// Every version to try for this kind, newest first: the generated
    /// model's ([`Self::api_resource`]), then [`Self::older_api_versions`].
    pub fn api_resources(&self) -> Vec<kube::core::ApiResource> {
        let current = self.api_resource();
        let older = self
            .older_api_versions()
            .iter()
            .map(|version| kube::core::ApiResource {
                version: (*version).to_string(),
                api_version: format!("{}/{}", current.group, version),
                ..current.clone()
            });
        std::iter::once(current.clone()).chain(older).collect()
    }

    /// Annotation key used by Flux Operator resources to control reconciliation.
    pub const RECONCILE_ANNOTATION: &str = "fluxcd.controlplane.io/reconcile";

    /// Try to parse a string into a FluxResourceKind, returning None if invalid
    /// Use this when you want Option<Self> instead of Result<Self, String>
    pub fn parse_optional(s: &str) -> Option<Self> {
        s.parse().ok()
    }

    /// The Flux kind an object is, by its `apiVersion` and `kind`: the group
    /// must be the kind's, so a same-named kind of another API (Crossplane's
    /// `Bucket`) isn't taken for Flux's. Any version counts (older Flux).
    pub fn from_api(api_version: &str, kind: &str) -> Option<Self> {
        let parsed = Self::parse_optional(kind).filter(|parsed| parsed.as_str() == kind)?;
        let group = api_version.rsplit_once('/').map_or("", |(group, _)| group);
        (parsed.api_resource().group == group).then_some(parsed)
    }

    /// Get all Flux resource kinds ([`Self::ALL`]).
    pub fn all() -> &'static [Self] {
        Self::ALL
    }

    /// The kind a `:` command names (any case): a name, plural, or alias.
    pub fn from_alias(s: &str) -> Option<Self> {
        let s = s.to_lowercase();
        Self::ALL
            .iter()
            .copied()
            .find(|kind| kind.aliases().contains(&s.as_str()))
    }

    /// Check if this resource type supports the graph view
    ///
    /// Only resources with inventory tracking capabilities support graphs:
    /// - Kustomization
    /// - HelmRelease
    /// - ArtifactGenerator
    /// - FluxInstance
    /// - ResourceSet
    pub fn supports_graph(&self) -> bool {
        matches!(
            self,
            FluxResourceKind::Kustomization
                | FluxResourceKind::HelmRelease
                | FluxResourceKind::ArtifactGenerator
                | FluxResourceKind::FluxInstance
                | FluxResourceKind::ResourceSet
        )
    }

    /// Whether this kind records the objects it applies in
    /// `status.inventory`, so the graph can discover its downstream
    /// resources from the inventory:
    /// - Kustomization, ResourceSet, FluxInstance (`status.inventory.entries`)
    /// - ArtifactGenerator (`status.inventory[]`: its ExternalArtifacts,
    ///   possibly in other namespaces since Flux 2.10)
    ///
    /// HelmRelease is deliberately excluded — its inventory lives in Helm
    /// storage Secrets and has a dedicated discovery path.
    pub fn has_inventory_downstream(&self) -> bool {
        matches!(
            self,
            FluxResourceKind::Kustomization
                | FluxResourceKind::ResourceSet
                | FluxResourceKind::FluxInstance
                | FluxResourceKind::ArtifactGenerator
        )
    }

    /// Check if this resource type supports reconciliation history
    ///
    /// Only resources with status.history field support history:
    /// - FluxInstance
    /// - ResourceSet
    /// - Kustomization
    /// - HelmRelease
    pub fn supports_history(&self) -> bool {
        matches!(
            self,
            FluxResourceKind::FluxInstance
                | FluxResourceKind::ResourceSet
                | FluxResourceKind::Kustomization
                | FluxResourceKind::HelmRelease
        )
    }

    /// Check if this resource type is stateless (has no status.conditions in its CRD)
    ///
    /// Stateless resources are treated as always ready since they are
    /// configuration-only resources with no reconciliation status.
    pub fn is_stateless(&self) -> bool {
        matches!(self, FluxResourceKind::Alert | FluxResourceKind::Provider)
    }

    /// How this kind's reconciliation is suspended — `None` when it can't
    /// be. Matches the CRDs (`spec.suspend`; checked against `crds/` by a
    /// test) and the Flux CLI's annotations for kinds without the field.
    pub fn suspend_switch(&self) -> Option<SuspendSwitch> {
        match self {
            Self::GitRepository
            | Self::OCIRepository
            | Self::HelmRepository
            | Self::Bucket
            | Self::HelmChart
            | Self::Kustomization
            | Self::HelmRelease
            | Self::ImageRepository
            | Self::ImagePolicy
            | Self::ImageUpdateAutomation
            | Self::Alert
            | Self::Provider
            | Self::Receiver => Some(SuspendSwitch::Spec),
            Self::FluxInstance | Self::ResourceSet | Self::ResourceSetInputProvider => {
                Some(SuspendSwitch::Annotation(Self::RECONCILE_ANNOTATION))
            }
            // `flux suspend artifact generator` (Flux 2.10).
            Self::ArtifactGenerator => Some(SuspendSwitch::Annotation(
                "source.extensions.fluxcd.io/reconcile",
            )),
            Self::ExternalArtifact | Self::FluxReport => None,
        }
    }

    /// Whether `s`/`r` (suspend / resume) apply ([`Self::suspend_switch`]).
    pub fn supports_suspend(&self) -> bool {
        self.suspend_switch().is_some()
    }

    /// Whether the object is suspended, read the way its kind records it;
    /// `false` for kinds that can't be.
    pub fn extract_suspended(&self, obj: &Value) -> Option<bool> {
        Some(
            self.suspend_switch()
                .is_some_and(|switch| switch.is_suspended(obj)),
        )
    }

    /// The kinds with reconciliation history ([`Self::supports_history`]).
    pub fn history_supported_types() -> impl Iterator<Item = Self> {
        Self::ALL.iter().copied().filter(Self::supports_history)
    }

    /// Whether the kind pulls from a Flux source (`spec.sourceRef`, or a
    /// HelmRelease's chart source): what traces resolve and `W` reconciles
    /// first.
    pub fn has_source_ref(&self) -> bool {
        matches!(self, Self::Kustomization | Self::HelmRelease)
    }

    /// The label prefix this kind stamps on what it applies
    /// (`<prefix>/name`, `<prefix>/namespace`). These are labels, not API
    /// groups — ResourceSet's differs from its group. FluxInstance marks
    /// its objects differently ([`labels::MANAGED_BY`]).
    pub fn owner_label_prefix(&self) -> Option<&'static str> {
        match self {
            Self::Kustomization => Some("kustomize.toolkit.fluxcd.io"),
            Self::HelmRelease => Some("helm.toolkit.fluxcd.io"),
            Self::ResourceSet => Some("resourceset.fluxcd.controlplane.io"),
            Self::GitRepository
            | Self::OCIRepository
            | Self::HelmRepository
            | Self::Bucket
            | Self::HelmChart
            | Self::ExternalArtifact
            | Self::ArtifactGenerator
            | Self::ImageRepository
            | Self::ImagePolicy
            | Self::ImageUpdateAutomation
            | Self::Alert
            | Self::Provider
            | Self::Receiver
            | Self::ResourceSetInputProvider
            | Self::FluxReport
            | Self::FluxInstance => None,
        }
    }

    /// Whether objects it applied can be left behind (orphaned): the kinds
    /// that label what they apply.
    pub fn can_orphan(&self) -> bool {
        self.owner_label_prefix().is_some() || *self == Self::FluxInstance
    }

    /// Get column headers for this resource type
    pub fn columns(&self) -> Vec<&'static str> {
        use field_names::*;
        match self {
            FluxResourceKind::GitRepository => vec![
                STATUS, NAMESPACE, NAME, URL, BRANCH, REVISION, SUSPENDED, READY,
            ],
            FluxResourceKind::OCIRepository => {
                vec![STATUS, NAMESPACE, NAME, SEMVER, DIGEST, REVISION]
            }
            FluxResourceKind::HelmRepository => {
                vec![STATUS, NAMESPACE, NAME, URL, REVISION, SUSPENDED, READY]
            }
            FluxResourceKind::Kustomization => {
                vec![
                    STATUS, NAMESPACE, NAME, PATH, REVISION, PRUNE, SUSPENDED, READY,
                ]
            }
            FluxResourceKind::HelmRelease => {
                vec![
                    STATUS, NAMESPACE, NAME, CHART, VERSION, REVISION, SUSPENDED, READY,
                ]
            }
            FluxResourceKind::HelmChart => {
                vec![
                    STATUS, NAMESPACE, NAME, CHART, VERSION, SOURCE, SUSPENDED, READY,
                ]
            }
            FluxResourceKind::ImageRepository => {
                vec![STATUS, NAMESPACE, NAME, IMAGE, SUSPENDED, READY]
            }
            FluxResourceKind::ImagePolicy => {
                vec![STATUS, NAMESPACE, NAME, IMAGE, SUSPENDED, READY]
            }
            FluxResourceKind::ImageUpdateAutomation => {
                vec![STATUS, NAMESPACE, NAME, IMAGE, BRANCH, SUSPENDED, READY]
            }
            FluxResourceKind::Bucket => {
                vec![
                    STATUS, NAMESPACE, NAME, ENDPOINT, PROVIDER, INTERVAL, SUSPENDED, READY,
                ]
            }
            FluxResourceKind::ExternalArtifact => {
                vec![STATUS, NAMESPACE, NAME, SOURCE, TYPE, READY, MESSAGE]
            }
            FluxResourceKind::Alert => {
                vec![STATUS, NAMESPACE, NAME, PROVIDER, SUSPENDED, READY, MESSAGE]
            }
            FluxResourceKind::Provider => {
                vec![
                    STATUS, NAMESPACE, NAME, TYPE, ADDRESS, CHANNEL, SUSPENDED, READY, MESSAGE,
                ]
            }
            FluxResourceKind::Receiver => {
                vec![
                    STATUS, NAMESPACE, NAME, TYPE, WEBHOOK, SUSPENDED, READY, MESSAGE,
                ]
            }
            FluxResourceKind::ResourceSet => {
                vec![STATUS, NAMESPACE, NAME, INPUTS, READY, MESSAGE]
            }
            FluxResourceKind::ResourceSetInputProvider => vec![
                STATUS, NAMESPACE, NAME, TYPE, URL, SECRET, INTERVAL, READY, MESSAGE,
            ],
            FluxResourceKind::FluxInstance => {
                vec![STATUS, NAMESPACE, NAME, VERSION, READY, MESSAGE]
            }
            _ => vec![STATUS, NAMESPACE, NAME, TYPE, SUSPENDED, READY, MESSAGE],
        }
    }

    /// Extract resource-specific display fields from a JSON object
    pub fn extract_fields(&self, obj: &Value) -> HashMap<String, String> {
        use field_names::*;
        let mut fields = HashMap::new();

        if let Some(spec) = obj.get("spec").and_then(|s| s.as_object()) {
            match self {
                FluxResourceKind::GitRepository | FluxResourceKind::HelmRepository => {
                    if let Some(url) = spec.get("url").and_then(|u| u.as_str()) {
                        fields.insert(URL.to_string(), url.to_string());
                    }
                    if let Some(branch) = spec.get("branch").and_then(|b| b.as_str()) {
                        fields.insert(BRANCH.to_string(), branch.to_string());
                    }
                    // Extract INTERVAL (common across types)
                    if let Some(interval) = spec.get("interval").and_then(|i| i.as_str()) {
                        fields.insert(INTERVAL.to_string(), interval.to_string());
                    }
                }
                FluxResourceKind::OCIRepository => {
                    if let Some(semver) = spec
                        .get("ref")
                        .and_then(|s| s.get("semver"))
                        .and_then(|se| se.as_str())
                    {
                        fields.insert(SEMVER.to_string(), semver.to_string());
                    }
                    // Extract TAG (was only in detail.rs)
                    if let Some(tag) = spec
                        .get("ref")
                        .and_then(|r| r.get("tag"))
                        .and_then(|t| t.as_str())
                    {
                        fields.insert(TAG.to_string(), tag.to_string());
                    }
                    // Extract INTERVAL (common across types)
                    if let Some(interval) = spec.get("interval").and_then(|i| i.as_str()) {
                        fields.insert(INTERVAL.to_string(), interval.to_string());
                    }
                }
                FluxResourceKind::Kustomization => {
                    if let Some(path) = spec.get("path").and_then(|p| p.as_str()) {
                        fields.insert(PATH.to_string(), path.to_string());
                    }
                    if let Some(prune) = spec.get("prune").and_then(|p| p.as_bool()) {
                        fields.insert(
                            PRUNE.to_string(),
                            if prune { "True" } else { "False" }.to_string(),
                        );
                    }
                    // Extract INTERVAL (common across types)
                    if let Some(interval) = spec.get("interval").and_then(|i| i.as_str()) {
                        fields.insert(INTERVAL.to_string(), interval.to_string());
                    }
                }
                FluxResourceKind::HelmRelease => {
                    let chart_spec = spec.get("chart").and_then(|c| c.get("spec"));
                    let chart_field = |key: &str| {
                        chart_spec
                            .and_then(|cs| cs.get(key))
                            .and_then(|v| v.as_str())
                    };
                    // An OCIRepository chart template (Flux 2.10+) has a `url`
                    // and a `ref` in place of `chart` and `version`.
                    let oci_ref = |key: &str| {
                        chart_spec
                            .and_then(|cs| cs.get("ref"))
                            .and_then(|r| r.get(key))
                            .and_then(|v| v.as_str())
                    };
                    if let Some(chart) = chart_field("chart").or_else(|| chart_field("url")) {
                        fields.insert(CHART.to_string(), chart.to_string());
                    }
                    // An OCIRepository template pulls by its ref, in the
                    // controller's precedence (digest, then semver, then tag);
                    // `version` (the CRD allows `*`) doesn't apply to it.
                    let oci_template = spec
                        .get("chart")
                        .and_then(|c| c.get("kind"))
                        .and_then(|k| k.as_str())
                        == Some("OCIRepository");
                    let version = if oci_template {
                        oci_ref("digest")
                            .or_else(|| oci_ref("semver"))
                            .or_else(|| oci_ref("tag"))
                    } else {
                        chart_field("version")
                    };
                    if let Some(version) = version {
                        fields.insert(VERSION.to_string(), version.to_string());
                    }
                    // Extract INTERVAL (common across types)
                    if let Some(interval) = spec.get("interval").and_then(|i| i.as_str()) {
                        fields.insert(INTERVAL.to_string(), interval.to_string());
                    }
                }
                FluxResourceKind::HelmChart => {
                    if let Some(chart) = spec.get("chart").and_then(|c| c.as_str()) {
                        fields.insert(CHART.to_string(), chart.to_string());
                    }
                    if let Some(version) = spec.get("version").and_then(|v| v.as_str()) {
                        fields.insert(VERSION.to_string(), version.to_string());
                    }
                    if let Some(source_ref) = spec.get("sourceRef")
                        && let Some(name) = source_ref.get("name").and_then(|n| n.as_str())
                    {
                        fields.insert(SOURCE.to_string(), name.to_string());
                    }
                    // Extract INTERVAL (common across types)
                    if let Some(interval) = spec.get("interval").and_then(|i| i.as_str()) {
                        fields.insert(INTERVAL.to_string(), interval.to_string());
                    }
                }
                FluxResourceKind::ImageRepository => {
                    if let Some(image) = spec.get("image").and_then(|i| i.as_str()) {
                        fields.insert(IMAGE.to_string(), image.to_string());
                    }
                    // Extract INTERVAL (common across types)
                    if let Some(interval) = spec.get("interval").and_then(|i| i.as_str()) {
                        fields.insert(INTERVAL.to_string(), interval.to_string());
                    }
                }
                FluxResourceKind::ImagePolicy => {
                    if let Some(image_ref) = spec
                        .get("imageRepositoryRef")
                        .and_then(|ir| ir.get("name"))
                        .and_then(|n| n.as_str())
                    {
                        fields.insert(IMAGE.to_string(), image_ref.to_string());
                    }
                    // Extract INTERVAL (common across types)
                    if let Some(interval) = spec.get("interval").and_then(|i| i.as_str()) {
                        fields.insert(INTERVAL.to_string(), interval.to_string());
                    }
                }
                FluxResourceKind::ImageUpdateAutomation => {
                    if let Some(image_ref) = spec
                        .get("sourceRef")
                        .and_then(|sr| sr.get("name"))
                        .and_then(|n| n.as_str())
                    {
                        fields.insert(IMAGE.to_string(), image_ref.to_string());
                    }
                    if let Some(branch) = spec
                        .get("git")
                        .and_then(|g| g.get("checkout"))
                        .and_then(|c| c.get("ref"))
                        .and_then(|r| r.get("branch"))
                        .and_then(|b| b.as_str())
                    {
                        fields.insert(BRANCH.to_string(), branch.to_string());
                    }
                    // Extract INTERVAL (common across types)
                    if let Some(interval) = spec.get("interval").and_then(|i| i.as_str()) {
                        fields.insert(INTERVAL.to_string(), interval.to_string());
                    }
                }
                FluxResourceKind::Bucket => {
                    if let Some(endpoint) = spec.get("endpoint").and_then(|e| e.as_str()) {
                        fields.insert(ENDPOINT.to_string(), endpoint.to_string());
                    }
                    if let Some(provider) = spec.get("provider").and_then(|p| p.as_str()) {
                        fields.insert(PROVIDER.to_string(), provider.to_string());
                    }
                    if let Some(secret_name) = spec
                        .get("secretRef")
                        .and_then(|s| s.get("name"))
                        .and_then(|n| n.as_str())
                    {
                        fields.insert(SECRET.to_string(), secret_name.to_string());
                    }
                    if let Some(interval) = spec.get("interval").and_then(|i| i.as_str()) {
                        fields.insert(INTERVAL.to_string(), interval.to_string());
                    }
                }
                FluxResourceKind::ExternalArtifact => {
                    if let Some(source_ref) = spec.get("sourceRef") {
                        if let Some(name) = source_ref.get("name").and_then(|n| n.as_str()) {
                            fields.insert(SOURCE.to_string(), name.to_string());
                        }
                        if let Some(kind) = source_ref.get("kind").and_then(|k| k.as_str()) {
                            fields.insert(TYPE.to_string(), kind.to_string());
                        }
                    }
                }
                FluxResourceKind::Alert => {
                    if let Some(provider_name) = spec
                        .get("providerRef")
                        .and_then(|p| p.get("name"))
                        .and_then(|n| n.as_str())
                    {
                        fields.insert(PROVIDER.to_string(), provider_name.to_string());
                    }
                }
                FluxResourceKind::Provider => {
                    if let Some(provider_type) = spec.get("type").and_then(|t| t.as_str()) {
                        fields.insert(TYPE.to_string(), provider_type.to_string());
                    }
                    if let Some(address) = spec.get("address").and_then(|a| a.as_str()) {
                        fields.insert(ADDRESS.to_string(), address.to_string());
                    }
                    if let Some(channel) = spec.get("channel").and_then(|c| c.as_str()) {
                        fields.insert(CHANNEL.to_string(), channel.to_string());
                    }
                    if let Some(secret_name) = spec
                        .get("secretRef")
                        .and_then(|s| s.get("name"))
                        .and_then(|n| n.as_str())
                    {
                        fields.insert(SECRET.to_string(), secret_name.to_string());
                    }
                }
                FluxResourceKind::Receiver => {
                    if let Some(receiver_type) = spec.get("type").and_then(|t| t.as_str()) {
                        fields.insert(TYPE.to_string(), receiver_type.to_string());
                    }
                    if let Some(secret_name) = spec
                        .get("secretRef")
                        .and_then(|s| s.get("name"))
                        .and_then(|n| n.as_str())
                    {
                        fields.insert(SECRET.to_string(), secret_name.to_string());
                    }
                    if let Some(interval) = spec.get("interval").and_then(|i| i.as_str()) {
                        fields.insert(INTERVAL.to_string(), interval.to_string());
                    }
                }
                FluxResourceKind::ResourceSet => {
                    if let Some(inputs_from) = spec.get("inputsFrom").and_then(|i| i.as_array()) {
                        let names: Vec<&str> = inputs_from
                            .iter()
                            .filter_map(|item| item.get("name").and_then(|n| n.as_str()))
                            .collect();
                        if !names.is_empty() {
                            fields.insert(INPUTS.to_string(), names.join(", "));
                        }
                    }
                }
                FluxResourceKind::ResourceSetInputProvider => {
                    if let Some(input_type) = spec.get("type").and_then(|t| t.as_str()) {
                        fields.insert(TYPE.to_string(), input_type.to_string());
                    }
                    if let Some(url) = spec.get("url").and_then(|u| u.as_str()) {
                        fields.insert(URL.to_string(), url.to_string());
                    }
                    if let Some(secret_name) = spec
                        .get("secretRef")
                        .and_then(|s| s.get("name"))
                        .and_then(|n| n.as_str())
                    {
                        fields.insert(SECRET.to_string(), secret_name.to_string());
                    }
                    if let Some(reconcile_every) = obj
                        .get("metadata")
                        .and_then(|m| m.get("annotations"))
                        .and_then(|a| a.get("fluxcd.controlplane.io/reconcileEvery"))
                        .and_then(|v| v.as_str())
                    {
                        fields.insert(INTERVAL.to_string(), reconcile_every.to_string());
                    }
                }
                FluxResourceKind::FluxInstance => {
                    if let Some(version) = spec
                        .get("distribution")
                        .and_then(|d| d.get("version"))
                        .and_then(|v| v.as_str())
                    {
                        fields.insert(VERSION.to_string(), version.to_string());
                    }
                }
                _ => {
                    // Extract INTERVAL for other types that might have it
                    if let Some(interval) = spec.get("interval").and_then(|i| i.as_str()) {
                        fields.insert(INTERVAL.to_string(), interval.to_string());
                    }
                }
            }
        }

        // Extract status fields
        if let Some(status) = obj.get("status").and_then(|s| s.as_object()) {
            if matches!(self, FluxResourceKind::HelmRelease) {
                // The chart from spec (name, or an OCI template's URL) wins;
                // the generated source only stands in when spec has none.
                if !fields.contains_key(CHART)
                    && let Some(helm_chart) = status.get("helmChart").and_then(|hc| hc.as_str())
                {
                    fields.insert(CHART.to_string(), helm_chart.to_string());
                }
                if let Some(release_status) = status
                    .get("conditions")
                    .and_then(|c| {
                        crate::kube::conditions::of_type(c, crate::kube::conditions::READY)
                    })
                    .and_then(|c| c.status())
                {
                    fields.insert(STATUS.to_string(), release_status.to_string());
                }
            }
            if matches!(self, FluxResourceKind::Receiver)
                && let Some(webhook) = status.get("webhookPath").and_then(|w| w.as_str())
            {
                fields.insert(WEBHOOK.to_string(), webhook.to_string());
            }
            if matches!(self, FluxResourceKind::OCIRepository) {
                if let Some(digest) = status
                    .get("artifact")
                    .and_then(|a| a.get("digest"))
                    .and_then(|d| d.as_str())
                {
                    fields.insert(DIGEST.to_string(), digest.to_string());
                }
                if let Some(revision) = status
                    .get("artifact")
                    .and_then(|a| a.get("revision"))
                    .and_then(|r| r.as_str())
                {
                    fields.insert(REVISION.to_string(), revision.to_string());
                }
            }
        }

        fields
    }
}

impl fmt::Display for FluxResourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl From<FluxResourceKind> for String {
    fn from(kind: FluxResourceKind) -> Self {
        kind.as_str().to_string()
    }
}

impl FromStr for FluxResourceKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_name(s).ok_or_else(|| format!("Unknown Flux resource kind: {}", s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_as_str() {
        assert_eq!(FluxResourceKind::GitRepository.as_str(), "GitRepository");
        assert_eq!(FluxResourceKind::OCIRepository.as_str(), "OCIRepository");
        assert_eq!(FluxResourceKind::Kustomization.as_str(), "Kustomization");
    }

    #[test]
    fn test_from_str() {
        assert_eq!(
            FluxResourceKind::parse_optional("GitRepository"),
            Some(FluxResourceKind::GitRepository)
        );
        assert_eq!(
            FluxResourceKind::parse_optional("OCIRepository"),
            Some(FluxResourceKind::OCIRepository)
        );
        assert_eq!(FluxResourceKind::parse_optional("Unknown"), None);
    }

    #[test]
    fn test_from_alias() {
        assert_eq!(
            FluxResourceKind::from_alias("gitrepository"),
            Some(FluxResourceKind::GitRepository)
        );
        assert_eq!(
            FluxResourceKind::from_alias("GitRepository"),
            Some(FluxResourceKind::GitRepository)
        );
        assert_eq!(
            FluxResourceKind::from_alias("ks"),
            Some(FluxResourceKind::Kustomization)
        );
        assert_eq!(
            FluxResourceKind::from_alias("oci"),
            Some(FluxResourceKind::OCIRepository)
        );
    }

    #[test]
    fn every_kind_round_trips_its_name() {
        for kind in FluxResourceKind::ALL {
            assert_eq!(FluxResourceKind::parse_optional(kind.as_str()), Some(*kind));
            assert_eq!(FluxResourceKind::from_alias(kind.as_str()), Some(*kind));
        }
    }

    #[test]
    fn aliases_cover_name_and_plural_and_are_unique() {
        let mut seen = std::collections::HashMap::new();
        for kind in FluxResourceKind::ALL {
            let aliases = kind.aliases();
            assert!(
                aliases.contains(&kind.as_str().to_lowercase().as_str()),
                "{kind}"
            );
            assert!(
                aliases.contains(&kind.api_resource().plural.as_str()),
                "{kind}"
            );
            for alias in aliases {
                assert_eq!(
                    *alias,
                    alias.to_lowercase(),
                    "{kind}: aliases are lowercase"
                );
                if let Some(other) = seen.insert(*alias, *kind) {
                    panic!("alias {alias} names both {other} and {kind}");
                }
            }
        }
        // Once only accepted by one of two alias tables.
        assert_eq!(
            FluxResourceKind::from_alias("helmrepo"),
            Some(FluxResourceKind::HelmRepository)
        );
    }

    #[test]
    fn older_versions_never_repeat_the_current_one() {
        for kind in FluxResourceKind::ALL {
            let versions: Vec<_> = kind
                .api_resources()
                .into_iter()
                .map(|r| r.version)
                .collect();
            let unique: std::collections::HashSet<_> = versions.iter().collect();
            assert_eq!(unique.len(), versions.len(), "{kind}: {versions:?}");
        }
    }

    #[test]
    fn test_display() {
        assert_eq!(
            format!("{}", FluxResourceKind::GitRepository),
            "GitRepository"
        );
        assert_eq!(
            format!("{}", FluxResourceKind::Kustomization),
            "Kustomization"
        );
    }

    #[test]
    fn test_into_string() {
        let s: String = FluxResourceKind::HelmRelease.into();
        assert_eq!(s, "HelmRelease");
    }

    #[test]
    fn test_is_stateless() {
        assert!(FluxResourceKind::Alert.is_stateless());
        assert!(FluxResourceKind::Provider.is_stateless());
        assert!(!FluxResourceKind::Receiver.is_stateless());
        assert!(!FluxResourceKind::Kustomization.is_stateless());
        assert!(!FluxResourceKind::HelmRelease.is_stateless());
        assert!(!FluxResourceKind::GitRepository.is_stateless());
    }

    #[test]
    fn test_columns() {
        let gitrepo_cols = FluxResourceKind::GitRepository.columns();
        assert!(gitrepo_cols.contains(&"URL"));
        assert!(gitrepo_cols.contains(&"BRANCH"));

        let kustomization_cols = FluxResourceKind::Kustomization.columns();
        assert!(kustomization_cols.contains(&"PATH"));
        assert!(kustomization_cols.contains(&"PRUNE"));

        let helmrelease_cols = FluxResourceKind::HelmRelease.columns();
        assert!(helmrelease_cols.contains(&"CHART"));
        assert!(helmrelease_cols.contains(&"VERSION"));

        let alert_cols = FluxResourceKind::Alert.columns();
        assert!(alert_cols.contains(&"STATUS"));
        assert!(alert_cols.contains(&"PROVIDER"));

        let provider_cols = FluxResourceKind::Provider.columns();
        assert!(provider_cols.contains(&"TYPE"));
        assert!(provider_cols.contains(&"ADDRESS"));
        assert!(provider_cols.contains(&"CHANNEL"));

        let bucket_cols = FluxResourceKind::Bucket.columns();
        assert!(bucket_cols.contains(&"ENDPOINT"));
        assert!(bucket_cols.contains(&"PROVIDER"));

        let receiver_cols = FluxResourceKind::Receiver.columns();
        assert!(receiver_cols.contains(&"TYPE"));
        assert!(receiver_cols.contains(&"WEBHOOK"));

        let resourceset_cols = FluxResourceKind::ResourceSet.columns();
        assert!(resourceset_cols.contains(&"STATUS"));
        assert!(resourceset_cols.contains(&"INPUTS"));

        // Default columns for types without custom columns
        let default_cols = FluxResourceKind::ArtifactGenerator.columns();
        assert!(default_cols.contains(&"STATUS"));
        assert!(default_cols.contains(&"TYPE"));
    }

    #[test]
    fn test_extract_suspended_spec_based_resource() {
        let obj = json!({
            "spec": {
                "suspend": true
            }
        });

        assert_eq!(
            FluxResourceKind::Kustomization.extract_suspended(&obj),
            Some(true)
        );
    }

    #[test]
    fn test_extract_suspended_annotation_based_resource() {
        let obj = json!({
            "metadata": {
                "annotations": {
                    "fluxcd.controlplane.io/reconcile": "disabled"
                }
            }
        });

        assert_eq!(
            FluxResourceKind::ResourceSet.extract_suspended(&obj),
            Some(true)
        );
        assert_eq!(
            FluxResourceKind::FluxInstance.extract_suspended(&obj),
            Some(true)
        );
    }

    #[test]
    fn artifact_generators_suspend_through_their_annotation() {
        let ag = |value: &str| json!({"metadata": {"annotations": {"source.extensions.fluxcd.io/reconcile": value}}});
        let kind = FluxResourceKind::ArtifactGenerator;
        assert_eq!(kind.extract_suspended(&ag("disabled")), Some(true));
        assert_eq!(kind.extract_suspended(&ag("enabled")), Some(false));
        assert_eq!(kind.extract_suspended(&json!({})), Some(false));
        // The Flux Operator's annotation doesn't suspend an ArtifactGenerator.
        let operator =
            json!({"metadata": {"annotations": {"fluxcd.controlplane.io/reconcile": "disabled"}}});
        assert_eq!(kind.extract_suspended(&operator), Some(false));

        let switch = kind.suspend_switch().unwrap();
        assert_eq!(switch.patch(true), ag("disabled"));
        assert_eq!(switch.patch(false), ag("enabled"));
        assert_eq!(
            FluxResourceKind::Kustomization
                .suspend_switch()
                .unwrap()
                .patch(true),
            json!({"spec": {"suspend": true}})
        );
        assert_eq!(
            FluxResourceKind::FluxReport.extract_suspended(&json!({"spec": {"suspend": true}})),
            Some(false)
        );
    }

    #[test]
    fn test_extract_suspended_annotation_defaults_false() {
        let obj = json!({
            "metadata": {
                "annotations": {}
            }
        });

        assert_eq!(
            FluxResourceKind::ResourceSetInputProvider.extract_suspended(&obj),
            Some(false)
        );
    }

    #[test]
    fn test_extract_fields_gitrepository() {
        let obj = json!({
            "spec": {
                "url": "https://github.com/fluxcd/flux2",
                "branch": "main",
                "interval": "5m"
            }
        });

        let fields = FluxResourceKind::GitRepository.extract_fields(&obj);
        assert_eq!(
            fields.get("URL"),
            Some(&"https://github.com/fluxcd/flux2".to_string())
        );
        assert_eq!(fields.get("BRANCH"), Some(&"main".to_string()));
        assert_eq!(fields.get("INTERVAL"), Some(&"5m".to_string()));
    }

    #[test]
    fn test_extract_fields_kustomization() {
        let obj = json!({
            "spec": {
                "path": "./clusters/prod",
                "prune": true,
                "interval": "10m"
            }
        });

        let fields = FluxResourceKind::Kustomization.extract_fields(&obj);
        assert_eq!(fields.get("PATH"), Some(&"./clusters/prod".to_string()));
        assert_eq!(fields.get("PRUNE"), Some(&"True".to_string()));
        assert_eq!(fields.get("INTERVAL"), Some(&"10m".to_string()));
    }

    #[test]
    fn test_extract_fields_helmrelease() {
        let obj = json!({
            "spec": {
                "chart": {
                    "spec": {
                        "chart": "cert-manager",
                        "version": "v1.13.6"
                    }
                },
                "interval": "15m"
            }
        });

        let fields = FluxResourceKind::HelmRelease.extract_fields(&obj);
        assert_eq!(fields.get("CHART"), Some(&"cert-manager".to_string()));
        assert_eq!(fields.get("VERSION"), Some(&"v1.13.6".to_string()));
        assert_eq!(fields.get("INTERVAL"), Some(&"15m".to_string()));
    }

    #[test]
    fn helmrelease_chart_comes_from_spec_even_once_reconciled() {
        let obj = json!({
            "spec": {"chart": {"kind": "OCIRepository", "spec": {
                "url": "oci://ghcr.io/stefanprodan/charts/podinfo",
                "version": "*",
                "ref": {"tag": "6.7.0", "semver": ">=6.0.0", "digest": "sha256:abc"}
            }}},
            "status": {"helmChart": "OCIRepository/apps/apps-podinfo"}
        });
        let fields = FluxResourceKind::HelmRelease.extract_fields(&obj);
        assert_eq!(
            fields.get("CHART").map(String::as_str),
            Some("oci://ghcr.io/stefanprodan/charts/podinfo"),
            "status.helmChart doesn't replace the URL"
        );
        assert_eq!(
            fields.get("VERSION").map(String::as_str),
            Some("sha256:abc"),
            "the ref decides (digest, semver, tag), not the ignored `version: *`"
        );
        let by_ref = json!({"spec": {"chartRef": {"kind": "OCIRepository", "name": "podinfo"}},
            "status": {"helmChart": "OCIRepository/apps/podinfo"}});
        assert_eq!(
            FluxResourceKind::HelmRelease
                .extract_fields(&by_ref)
                .get("CHART")
                .map(String::as_str),
            Some("OCIRepository/apps/podinfo"),
            "without a chart in spec, the source stands in"
        );
    }

    #[test]
    fn test_extract_fields_helmrelease_oci_chart_template() {
        // Flux 2.10: chart.kind OCIRepository has a url and ref, no chart/version.
        let obj = json!({
            "spec": {
                "chart": {
                    "kind": "OCIRepository",
                    "spec": {
                        "url": "oci://ghcr.io/stefanprodan/charts/podinfo",
                        "ref": { "semver": ">=6.0.0" }
                    }
                }
            }
        });

        let fields = FluxResourceKind::HelmRelease.extract_fields(&obj);
        assert_eq!(
            fields.get("CHART").map(String::as_str),
            Some("oci://ghcr.io/stefanprodan/charts/podinfo")
        );
        assert_eq!(fields.get("VERSION").map(String::as_str), Some(">=6.0.0"));
    }

    #[test]
    fn test_extract_fields_ocirepository() {
        let obj = json!({
            "spec": {
                "ref": {
                    "semver": ">=1.0.0",
                    "tag": "latest"
                },
                "interval": "5m"
            },
            "status": {
                "artifact": {
                    "digest": "sha256:abc123",
                    "revision": "v1.0.0"
                }
            }
        });

        let fields = FluxResourceKind::OCIRepository.extract_fields(&obj);
        assert_eq!(fields.get("SEMVER"), Some(&">=1.0.0".to_string()));
        assert_eq!(fields.get("TAG"), Some(&"latest".to_string()));
        assert_eq!(fields.get("INTERVAL"), Some(&"5m".to_string()));
        assert_eq!(fields.get("DIGEST"), Some(&"sha256:abc123".to_string()));
        assert_eq!(fields.get("REVISION"), Some(&"v1.0.0".to_string()));
    }

    #[test]
    fn test_extract_fields_resourcesetinputprovider() {
        let obj = json!({
            "metadata": {
                "annotations": {
                    "fluxcd.controlplane.io/reconcileEvery": "30s"
                }
            },
            "spec": {
                "type": "ExternalService",
                "url": "http://flux-api.flux-system.svc.cluster.local:8080/api/v2/flux/clusters/demo-cluster-01.k8s.example.com/platform-components",
                "secretRef": {
                    "name": "internal-api-token"
                }
            }
        });

        let fields = FluxResourceKind::ResourceSetInputProvider.extract_fields(&obj);
        assert_eq!(fields.get("TYPE"), Some(&"ExternalService".to_string()));
        assert_eq!(
            fields.get("URL"),
            Some(&"http://flux-api.flux-system.svc.cluster.local:8080/api/v2/flux/clusters/demo-cluster-01.k8s.example.com/platform-components".to_string())
        );
        assert_eq!(
            fields.get("SECRET"),
            Some(&"internal-api-token".to_string())
        );
        assert_eq!(fields.get("INTERVAL"), Some(&"30s".to_string()));
    }

    #[test]
    fn test_extract_fields_missing_spec() {
        let obj = json!({});

        let fields = FluxResourceKind::Kustomization.extract_fields(&obj);
        assert!(fields.is_empty());
    }

    #[test]
    fn test_extract_fields_default_type() {
        let obj = json!({
            "spec": {
                "someField": "value",
                "interval": "5m"
            }
        });

        // ArtifactGenerator falls through to the default arm
        let fields = FluxResourceKind::ArtifactGenerator.extract_fields(&obj);
        // Should only extract INTERVAL for default types
        assert_eq!(fields.get("INTERVAL"), Some(&"5m".to_string()));
        assert_eq!(fields.len(), 1);
    }

    #[test]
    fn test_extract_fields_alert() {
        let obj = json!({
            "spec": {
                "providerRef": {
                    "name": "slack-bot"
                },
                "eventSeverity": "error"
            }
        });

        let fields = FluxResourceKind::Alert.extract_fields(&obj);
        assert_eq!(fields.get("PROVIDER"), Some(&"slack-bot".to_string()));
    }

    #[test]
    fn test_extract_fields_provider() {
        let obj = json!({
            "spec": {
                "type": "slack",
                "address": "https://hooks.slack.com/services/xxx",
                "channel": "#alerts",
                "secretRef": {
                    "name": "slack-webhook"
                }
            }
        });

        let fields = FluxResourceKind::Provider.extract_fields(&obj);
        assert_eq!(fields.get("TYPE"), Some(&"slack".to_string()));
        assert_eq!(
            fields.get("ADDRESS"),
            Some(&"https://hooks.slack.com/services/xxx".to_string())
        );
        assert_eq!(fields.get("CHANNEL"), Some(&"#alerts".to_string()));
        assert_eq!(fields.get("SECRET"), Some(&"slack-webhook".to_string()));
    }

    #[test]
    fn test_extract_fields_bucket() {
        let obj = json!({
            "spec": {
                "endpoint": "minio.example.com",
                "provider": "generic",
                "bucketName": "my-bucket",
                "interval": "5m",
                "secretRef": {
                    "name": "minio-credentials"
                }
            }
        });

        let fields = FluxResourceKind::Bucket.extract_fields(&obj);
        assert_eq!(
            fields.get("ENDPOINT"),
            Some(&"minio.example.com".to_string())
        );
        assert_eq!(fields.get("PROVIDER"), Some(&"generic".to_string()));
        assert_eq!(fields.get("INTERVAL"), Some(&"5m".to_string()));
        assert_eq!(fields.get("SECRET"), Some(&"minio-credentials".to_string()));
    }

    #[test]
    fn test_extract_fields_receiver() {
        let obj = json!({
            "spec": {
                "type": "github",
                "secretRef": {
                    "name": "webhook-token"
                },
                "interval": "10m"
            },
            "status": {
                "webhookPath": "/hook/abc123"
            }
        });

        let fields = FluxResourceKind::Receiver.extract_fields(&obj);
        assert_eq!(fields.get("TYPE"), Some(&"github".to_string()));
        assert_eq!(fields.get("SECRET"), Some(&"webhook-token".to_string()));
        assert_eq!(fields.get("INTERVAL"), Some(&"10m".to_string()));
        assert_eq!(fields.get("WEBHOOK"), Some(&"/hook/abc123".to_string()));
    }

    #[test]
    fn test_extract_fields_flux_instance() {
        let obj = json!({
            "spec": {
                "distribution": {
                    "version": "2.3.0",
                    "registry": "ghcr.io/fluxcd"
                }
            }
        });

        let fields = FluxResourceKind::FluxInstance.extract_fields(&obj);
        assert_eq!(fields.get("VERSION"), Some(&"2.3.0".to_string()));
    }

    #[test]
    fn test_extract_fields_external_artifact() {
        let obj = json!({
            "spec": {
                "sourceRef": {
                    "kind": "ConfigMap",
                    "name": "my-config"
                }
            }
        });

        let fields = FluxResourceKind::ExternalArtifact.extract_fields(&obj);
        assert_eq!(fields.get("SOURCE"), Some(&"my-config".to_string()));
        assert_eq!(fields.get("TYPE"), Some(&"ConfigMap".to_string()));
    }

    #[test]
    fn test_extract_fields_resource_set() {
        let obj = json!({
            "spec": {
                "inputsFrom": [
                    {"name": "namespaces"},
                    {"name": "clusters"}
                ],
                "resources": [{}]
            }
        });

        let fields = FluxResourceKind::ResourceSet.extract_fields(&obj);
        assert_eq!(
            fields.get("INPUTS"),
            Some(&"namespaces, clusters".to_string())
        );
    }

    #[test]
    fn test_extract_fields_resource_set_no_inputs_from() {
        let obj = json!({
            "spec": {
                "inputs": [{"name": "test"}],
                "resources": [{}]
            }
        });

        let fields = FluxResourceKind::ResourceSet.extract_fields(&obj);
        assert!(!fields.contains_key("INPUTS"));
    }

    #[test]
    fn test_has_inventory_downstream() {
        // The status.inventory.entries kinds share the graph discovery path
        assert!(FluxResourceKind::Kustomization.has_inventory_downstream());
        assert!(FluxResourceKind::ResourceSet.has_inventory_downstream());
        assert!(FluxResourceKind::FluxInstance.has_inventory_downstream());
        // ArtifactGenerator's status.inventory[] lists its ExternalArtifacts
        assert!(FluxResourceKind::ArtifactGenerator.has_inventory_downstream());
        // HelmRelease has its own path (Helm storage Secrets)
        assert!(!FluxResourceKind::HelmRelease.has_inventory_downstream());
        assert!(!FluxResourceKind::GitRepository.has_inventory_downstream());
        // Every inventory-downstream kind must also support the graph view
        for kind in FluxResourceKind::all() {
            if kind.has_inventory_downstream() {
                assert!(
                    kind.supports_graph(),
                    "{kind:?} has inventory but no graph support"
                );
            }
        }
    }
}
