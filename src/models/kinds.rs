//! Kind registry (#267): every `:<kind>` resolves to a [`KindSpec`].
//!
//! Kinds are **data**, not code paths. Providers are consulted in precedence
//! order and the first match wins:
//!
//! 1. **Flux** — the built-in [`FluxResourceKind`]s (static; full capabilities)
//! 2. **Flux-adjacent** — CRDs labeled `part-of=flux` ([`super::extra_kinds`])
//! 3. **Discovered** — every other kind the API server serves ([`catalog`]),
//!    populated by API discovery when `nativeResources` is enabled
//!
//! Columns, health, and capabilities are plain enums/structs so a later
//! config provider (#277) can overlay specs field by field without new code
//! paths. Capabilities are deny-by-default: only the Flux family grants Flux
//! operations.
//!
//! `expect()` on the catalog lock is deliberate (same rationale as
//! `extra_kinds`): a poisoned lock means a thread panicked mid-update and
//! the discovery state is untrustworthy — there is nothing to degrade to.
#![allow(clippy::expect_used)]

use std::fmt;
use std::sync::{Arc, OnceLock, RwLock};

use crate::models::FluxResourceKind;

/// Group/version/kind of a kind. `group` is empty for the core API.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Gvk {
    pub group: String,
    pub version: String,
    pub kind: String,
}

impl Gvk {
    /// `apiVersion` as it appears in manifests (`v1`, `apps/v1`).
    pub fn api_version(&self) -> String {
        if self.group.is_empty() {
            self.version.clone()
        } else {
            format!("{}/{}", self.group, self.version)
        }
    }
}

impl fmt::Display for Gvk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.kind, self.api_version())
    }
}

/// Whether objects of a kind live in a namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KindScope {
    Namespaced,
    Cluster,
}

/// Where a kind comes from — decides which rich behaviour it can have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// Built-in Flux kind: the existing Flux list, operations, graph, trace.
    Flux(FluxResourceKind),
    /// A CRD labeled as part of the Flux instance (view-only, #197).
    FluxAdjacent,
    /// Any other served kind — native Kubernetes or third-party CRD.
    Native,
}

/// Names a kind answers to on the `:` command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KindNames {
    pub kind: String,
    pub plural: String,
    pub short_names: Vec<String>,
}

impl KindNames {
    /// Every command token, lowercased: kind, plural, short names.
    pub fn tokens(&self) -> impl Iterator<Item = String> + '_ {
        std::iter::once(self.kind.to_lowercase())
            .chain(std::iter::once(self.plural.to_lowercase()))
            .chain(self.short_names.iter().map(|s| s.to_lowercase()))
    }

    fn matches(&self, token: &str) -> bool {
        self.tokens().any(|t| t == token)
    }
}

/// One list column. Data, so config can add `JsonPath` columns later (#277).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Column {
    /// A column flux9s computes itself.
    Builtin(BuiltinColumn),
    /// A value at a JSONPath-style path (`.status.phase`).
    JsonPath { name: String, path: String },
}

impl Column {
    pub fn header(&self) -> String {
        match self {
            Column::Builtin(b) => b.header().to_string(),
            Column::JsonPath { name, .. } => name.to_uppercase(),
        }
    }
}

/// Columns every kind can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinColumn {
    /// kstatus health (Current / InProgress / Failed / …).
    Status,
    /// The same kstatus health, labelled HEALTH — used when a CRD brings its
    /// own `Status` printer column.
    Health,
    /// Why the object isn't Current.
    Message,
    /// Who manages the object: Flux, Argo CD, Helm, another
    /// `app.kubernetes.io/managed-by`, or its Kubernetes owner chain.
    ManagedBy,
    Age,
}

impl BuiltinColumn {
    pub fn header(self) -> &'static str {
        match self {
            BuiltinColumn::Status => "STATUS",
            BuiltinColumn::Health => "HEALTH",
            BuiltinColumn::Message => "MESSAGE",
            BuiltinColumn::ManagedBy => "MANAGED-BY",
            BuiltinColumn::Age => "AGE",
        }
    }
}

/// How an object's health is judged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthRule {
    /// kstatus rules ([`crate::kube::object_status`]).
    Kstatus,
}

/// What can be done with a kind's objects. Deny by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Capabilities {
    /// `y` / `d` / Enter — every kind.
    pub view: bool,
    /// Suspend / resume / reconcile / delete — Flux family only.
    pub flux_operations: bool,
    /// Graph and trace views.
    pub graph: bool,
    /// Reconciliation history.
    pub history: bool,
    /// Enter opens the workload detail view (rollout, pods, events, usage)
    /// — the same view the graph's workload drill-down uses.
    pub workload_detail: bool,
    /// `r` rolls out a restart.
    pub restart: bool,
    /// `l` streams the object's pod logs (container picker included).
    pub pod_logs: bool,
    /// `Ctrl+d` deletes the pod.
    pub pod_delete: bool,
}

impl Capabilities {
    pub const VIEW_ONLY: Self = Self {
        view: true,
        flux_operations: false,
        graph: false,
        history: false,
        workload_detail: false,
        restart: false,
        pod_logs: false,
        pod_delete: false,
    };

    /// Curated capabilities for well-known native kinds, so browsing them
    /// offers the same actions as the workload views. Everything else stays
    /// view-only (deny by default).
    pub fn curated(gvk: &Gvk) -> Self {
        let mut caps = Self::VIEW_ONLY;
        match (gvk.group.as_str(), gvk.kind.as_str()) {
            ("apps", "Deployment" | "StatefulSet" | "DaemonSet") => {
                caps.workload_detail = true;
                caps.restart = true;
                caps.pod_logs = true;
            }
            ("batch", "CronJob") => caps.workload_detail = true,
            ("", "Pod") => {
                caps.pod_logs = true;
                caps.pod_delete = true;
            }
            _ => {}
        }
        caps
    }
}

/// Everything flux9s knows about a kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KindSpec {
    pub gvk: Gvk,
    pub scope: KindScope,
    pub names: KindNames,
    pub family: Family,
    /// Columns after NAMESPACE / NAME.
    pub columns: Vec<Column>,
    pub health: HealthRule,
    pub caps: Capabilities,
}

impl KindSpec {
    /// The generic spec for a served kind with no richer description:
    /// STATUS / MESSAGE / AGE, kstatus health, view-only.
    pub fn generic(gvk: Gvk, scope: KindScope, plural: String, short_names: Vec<String>) -> Self {
        let gvk_for_caps = gvk.clone();
        let names = KindNames {
            kind: gvk.kind.clone(),
            plural,
            short_names,
        };
        Self {
            gvk,
            scope,
            names,
            family: Family::Native,
            columns: vec![
                Column::Builtin(BuiltinColumn::Status),
                Column::Builtin(BuiltinColumn::ManagedBy),
                Column::Builtin(BuiltinColumn::Age),
                Column::Builtin(BuiltinColumn::Message),
            ],
            health: HealthRule::Kstatus,
            caps: Capabilities::curated(&gvk_for_caps),
        }
    }

    pub fn is_namespaced(&self) -> bool {
        self.scope == KindScope::Namespaced
    }

    /// Whether the existing Flux list handles this kind.
    pub fn is_flux(&self) -> bool {
        matches!(self.family, Family::Flux(_))
    }
}

// ---------------------------------------------------------------------------
// Providers
// ---------------------------------------------------------------------------

/// The spec for a built-in Flux kind.
fn flux_spec(kind: FluxResourceKind) -> Option<KindSpec> {
    let entry = crate::watcher::RESOURCE_REGISTRY
        .iter()
        .find(|e| e.display_name == kind.as_str())?;
    let (group, version, plural) = crate::kube::get_gvk_for_resource_type(kind.as_str()).ok()?;
    Some(KindSpec {
        gvk: Gvk {
            group,
            version,
            kind: kind.as_str().to_string(),
        },
        scope: KindScope::Namespaced,
        names: KindNames {
            kind: kind.as_str().to_string(),
            plural,
            short_names: entry
                .command_aliases
                .iter()
                .map(|a| a.to_string())
                .collect(),
        },
        family: Family::Flux(kind),
        columns: vec![Column::Builtin(BuiltinColumn::Status)],
        health: HealthRule::Kstatus,
        caps: Capabilities {
            view: true,
            flux_operations: true,
            graph: kind.supports_graph(),
            history: kind.supports_history(),
            ..Capabilities::VIEW_ONLY
        },
    })
}

/// The spec for a Flux-adjacent discovered CRD.
fn extra_spec(extra: &super::extra_kinds::ExtraKind) -> KindSpec {
    let mut spec = KindSpec::generic(
        Gvk {
            group: extra.group.clone(),
            version: extra.version.clone(),
            kind: extra.kind.clone(),
        },
        KindScope::Namespaced,
        extra.plural.clone(),
        extra.short_names.clone(),
    );
    spec.family = Family::FluxAdjacent;
    spec
}

// ---------------------------------------------------------------------------
// Discovered catalog
// ---------------------------------------------------------------------------

/// Kinds found by API discovery for the current cluster. Empty until
/// discovery completes, and always empty when `nativeResources` is off.
#[derive(Debug, Clone, Default)]
pub struct KindCatalog {
    kinds: Arc<RwLock<Vec<KindSpec>>>,
}

impl KindCatalog {
    /// Replace the catalog with a fresh discovery result.
    pub fn replace(&self, kinds: Vec<KindSpec>) {
        *self.kinds.write().expect("kind catalog poisoned") = kinds;
    }

    /// Forget every kind (context switch / teardown).
    pub fn clear(&self) {
        self.kinds.write().expect("kind catalog poisoned").clear();
    }

    pub fn len(&self) -> usize {
        self.kinds.read().expect("kind catalog poisoned").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Find by kind/plural/short name, or kubectl's fully qualified
    /// `plural.group` (`clusters.postgresql.cnpg.io`) — the way to pick one
    /// when two groups serve the same name.
    fn find(&self, token: &str) -> Option<KindSpec> {
        let kinds = self.kinds.read().expect("kind catalog poisoned");
        if let Some(found) = kinds.iter().find(|spec| {
            !spec.gvk.group.is_empty()
                && token
                    .strip_prefix(&spec.names.plural.to_lowercase())
                    .and_then(|rest| rest.strip_prefix('.'))
                    == Some(spec.gvk.group.to_lowercase().as_str())
        }) {
            return Some(found.clone());
        }
        kinds.iter().find(|spec| spec.names.matches(token)).cloned()
    }

    /// A discovered kind by API group and kind name.
    pub fn find_by_group_kind(&self, group: &str, kind: &str) -> Option<KindSpec> {
        self.kinds
            .read()
            .expect("kind catalog poisoned")
            .iter()
            .find(|spec| spec.gvk.group == group && spec.gvk.kind == kind)
            .cloned()
    }

    /// Command tokens of every discovered kind (plural + short names — the
    /// forms people type).
    fn tokens(&self) -> Vec<String> {
        self.kinds
            .read()
            .expect("kind catalog poisoned")
            .iter()
            .flat_map(|spec| {
                std::iter::once(spec.names.plural.to_lowercase())
                    .chain(spec.names.short_names.iter().map(|s| s.to_lowercase()))
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}

/// The process-wide catalog of discovered kinds.
pub fn catalog() -> &'static KindCatalog {
    static CATALOG: OnceLock<KindCatalog> = OnceLock::new();
    CATALOG.get_or_init(KindCatalog::default)
}

// ---------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------

/// Resolve a `:` command token to a kind, consulting providers in
/// precedence order (Flux → Flux-adjacent → discovered).
pub fn resolve(token: &str) -> Option<KindSpec> {
    resolve_in(token, catalog())
}

fn resolve_in(token: &str, discovered: &KindCatalog) -> Option<KindSpec> {
    let token = token.trim().to_lowercase();
    if token.is_empty() {
        return None;
    }
    if let Some(display) = crate::watcher::get_display_name_for_command(&token)
        && let Some(kind) = FluxResourceKind::parse_optional(display)
    {
        return flux_spec(kind);
    }
    let extras = super::extra_kinds::global();
    if let Some(kind) = extras.resolve_command(&token)
        && let Some(extra) = extras.get(&kind)
    {
        return Some(extra_spec(&extra));
    }
    discovered.find(&token)
}

/// Names of common Kubernetes kinds (plurals and kubectl short names).
/// Used only to give a helpful hint when native browsing is off — no API
/// call, so it works without discovery.
const COMMON_KIND_TOKENS: &[&str] = &[
    "pods",
    "pod",
    "po",
    "deployments",
    "deployment",
    "deploy",
    "services",
    "service",
    "svc",
    "configmaps",
    "configmap",
    "cm",
    "secrets",
    "secret",
    "statefulsets",
    "statefulset",
    "sts",
    "daemonsets",
    "daemonset",
    "ds",
    "replicasets",
    "replicaset",
    "rs",
    "jobs",
    "job",
    "cronjobs",
    "cronjob",
    "cj",
    "ingresses",
    "ingress",
    "ing",
    "persistentvolumeclaims",
    "pvc",
    "persistentvolumes",
    "pv",
    "serviceaccounts",
    "sa",
    "nodes",
    "node",
    "no",
    "endpoints",
    "ep",
    "networkpolicies",
    "netpol",
    "storageclasses",
    "sc",
    "horizontalpodautoscalers",
    "hpa",
    "poddisruptionbudgets",
    "pdb",
    "roles",
    "rolebindings",
    "clusterroles",
    "clusterrolebindings",
    "customresourcedefinitions",
    "crd",
    "crds",
    "namespaces",
    "leases",
    "gateways",
    "httproutes",
];

/// Whether a `:` token looks like a Kubernetes kind — a common kind name,
/// or kubectl's fully qualified `plural.group` form.
pub fn looks_like_kubernetes_kind(token: &str) -> bool {
    let token = token.trim().to_lowercase();
    COMMON_KIND_TOKENS.contains(&token.as_str())
        || token
            .split_once('.')
            .is_some_and(|(plural, group)| !plural.is_empty() && group.contains('.'))
}

/// Autocomplete candidates from discovered kinds.
pub fn discovered_command_tokens() -> Vec<String> {
    catalog().tokens()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn native(group: &str, kind: &str, plural: &str, short: &[&str]) -> KindSpec {
        KindSpec::generic(
            Gvk {
                group: group.into(),
                version: "v1".into(),
                kind: kind.into(),
            },
            KindScope::Namespaced,
            plural.into(),
            short.iter().map(|s| s.to_string()).collect(),
        )
    }

    #[test]
    fn flux_kinds_resolve_first_with_full_capabilities() {
        let catalog = KindCatalog::default();
        // A discovered kind using a Flux alias must not shadow the Flux kind.
        catalog.replace(vec![native("example.com", "Kuiper", "kuipers", &["ks"])]);
        let spec = resolve_in("ks", &catalog).unwrap();
        assert_eq!(spec.family, Family::Flux(FluxResourceKind::Kustomization));
        assert!(spec.caps.flux_operations);
        assert_eq!(spec.gvk.group, "kustomize.toolkit.fluxcd.io");
    }

    #[test]
    fn discovered_kinds_resolve_by_kind_plural_or_short_name() {
        let catalog = KindCatalog::default();
        catalog.replace(vec![native(
            "apps",
            "Deployment",
            "deployments",
            &["deploy"],
        )]);
        for token in ["deploy", "Deployments", "deployment"] {
            let spec = resolve_in(token, &catalog).unwrap();
            assert_eq!(spec.gvk.kind, "Deployment", "{token}");
            assert_eq!(spec.family, Family::Native);
            assert!(
                spec.caps.workload_detail && spec.caps.restart,
                "curated workload kind"
            );
        }
        assert!(resolve_in("nope", &catalog).is_none());
        assert!(resolve_in("  ", &catalog).is_none());
    }

    #[test]
    fn fully_qualified_plural_group_disambiguates() {
        let catalog = KindCatalog::default();
        catalog.replace(vec![
            native("cluster.x-k8s.io", "Cluster", "clusters", &[]),
            native("postgresql.cnpg.io", "Cluster", "clusters", &[]),
        ]);
        assert_eq!(
            resolve_in("clusters", &catalog).unwrap().gvk.group,
            "cluster.x-k8s.io",
            "bare name: first served wins"
        );
        assert_eq!(
            resolve_in("clusters.postgresql.cnpg.io", &catalog)
                .unwrap()
                .gvk
                .group,
            "postgresql.cnpg.io"
        );
    }

    #[test]
    fn cleared_catalog_resolves_nothing_native() {
        let catalog = KindCatalog::default();
        catalog.replace(vec![native("", "Pod", "pods", &["po"])]);
        assert!(resolve_in("po", &catalog).is_some());
        catalog.clear();
        assert!(resolve_in("po", &catalog).is_none());
        // Flux kinds never depend on discovery.
        assert!(resolve_in("hr", &catalog).is_some());
    }

    #[test]
    fn generic_spec_has_status_age_message_columns() {
        let spec = native("", "ConfigMap", "configmaps", &["cm"]);
        let headers: Vec<_> = spec.columns.iter().map(Column::header).collect();
        assert_eq!(headers, ["STATUS", "MANAGED-BY", "AGE", "MESSAGE"]);
        assert_eq!(spec.gvk.api_version(), "v1");
    }

    #[test]
    fn api_version_includes_group() {
        let gvk = Gvk {
            group: "apps".into(),
            version: "v1".into(),
            kind: "Deployment".into(),
        };
        assert_eq!(gvk.api_version(), "apps/v1");
        assert_eq!(gvk.to_string(), "Deployment (apps/v1)");
    }

    #[test]
    fn curated_capabilities_only_for_known_kinds() {
        let caps = |group: &str, kind: &str| {
            Capabilities::curated(&Gvk {
                group: group.into(),
                version: "v1".into(),
                kind: kind.into(),
            })
        };
        assert!(caps("apps", "StatefulSet").restart);
        assert!(caps("", "Pod").pod_delete && caps("", "Pod").pod_logs);
        assert!(!caps("", "Pod").restart);
        assert!(caps("batch", "CronJob").workload_detail && !caps("batch", "CronJob").restart);
        // Same kind name in another group is not trusted.
        assert_eq!(caps("example.com", "Deployment"), Capabilities::VIEW_ONLY);
        assert_eq!(caps("", "ConfigMap"), Capabilities::VIEW_ONLY);
    }

    #[test]
    fn recognises_kubernetes_kinds_without_discovery() {
        for token in ["deploy", "PO", "svc", "ipaddresspools.metallb.io"] {
            assert!(looks_like_kubernetes_kind(token), "{token}");
        }
        for token in ["ks", "nosuchthing", "skin", "a.b"] {
            assert!(!looks_like_kubernetes_kind(token), "{token}");
        }
    }
}
