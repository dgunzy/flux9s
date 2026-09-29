//! Who manages an object (#267 MANAGED-BY column; groundwork for #266/#274).
//!
//! Detection follows the conventions the tools themselves write, checked in
//! this order (first match wins):
//!
//! 1. **Flux** — `kustomize.toolkit.fluxcd.io/{name,namespace}`,
//!    `helm.toolkit.fluxcd.io/…`, `resourceset.fluxcd.controlplane.io/…`
//!    labels, and FluxInstance (`app.kubernetes.io/managed-by=flux-operator`
//!    with `fluxcd.controlplane.io/…`) — the same rules as the Flux Operator UI.
//!    Flux wins over Helm: helm-controller releases also carry Helm's marks.
//! 2. **Argo CD** — the `argocd.argoproj.io/tracking-id` annotation
//!    (`<app>:<group>/<kind>:<ns>/<name>`) or `argocd.argoproj.io/instance`.
//! 3. **Kubernetes owner** — the controller `ownerReference`. The resolver
//!    walks the chain (Pod → ReplicaSet → Deployment) to the first object a
//!    manager claims, so a Flux-managed Deployment's pods read as Flux.
//! 4. **Helm** — `meta.helm.sh/release-{name,namespace}` annotations, only on
//!    objects nothing owns (ReplicaSets inherit their Deployment's
//!    annotations, so they'd wrongly claim Helm for Flux-managed pods).
//! 5. **Any other manager** — the standard `app.kubernetes.io/managed-by`,
//!    only when there's no owner to follow: pods inherit it from their
//!    template (`managed-by: Helm` on a Flux HelmRelease's pods), so it's
//!    weaker evidence than the owner chain.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use kube::Api;
use kube::core::{DynamicObject, GroupVersionKind};
use kube::discovery::{ApiResource, Scope};
use serde_json::Value;

/// Distinct owners resolved per list — bounds the API calls one big list can
/// cause. Beyond it, rows show their direct owner.
pub const MAX_OWNER_LOOKUPS: usize = 300;
/// How far up an owner chain to walk (Pod → RS → Deployment is 2).
const MAX_OWNER_DEPTH: usize = 4;
/// Per-lookup cap.
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(5);
/// Owner lookups in flight at once.
const LOOKUP_CONCURRENCY: usize = 8;

/// A manager that claims an object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Manager {
    /// `kind` is the Flux kind (Kustomization, HelmRelease, ResourceSet,
    /// FluxInstance).
    Flux {
        kind: String,
        namespace: String,
        name: String,
    },
    ArgoCd {
        app: String,
    },
    Helm {
        namespace: String,
        release: String,
    },
    /// Whatever `app.kubernetes.io/managed-by` names.
    Other(String),
}

impl Manager {
    /// Short form for the MANAGED-BY column: *what kind* of manager (the
    /// full name is one keypress away in describe / workload detail).
    pub fn short_label(&self) -> String {
        match self {
            Manager::Flux { kind, .. } => match kind.as_str() {
                "Kustomization" => "Flux ks".to_string(),
                "HelmRelease" => "Flux hr".to_string(),
                "ResourceSet" => "Flux rset".to_string(),
                "FluxInstance" => "Flux instance".to_string(),
                other => format!("Flux {other}"),
            },
            Manager::ArgoCd { .. } => "Argo CD".to_string(),
            Manager::Helm { .. } => "Helm".to_string(),
            Manager::Other(name) => name.clone(),
        }
    }

    /// Full form for the Managed By field: kind and name of the manager.
    pub fn describe(&self) -> String {
        match self {
            Manager::Flux {
                kind,
                namespace,
                name,
            } => {
                let target = if namespace.is_empty() {
                    name.clone()
                } else {
                    format!("{namespace}/{name}")
                };
                if kind == "FluxInstance" {
                    format!("FluxInstance {target}")
                } else {
                    format!("Flux {kind} {target}")
                }
            }
            Manager::ArgoCd { app } => format!("Argo CD application {app}"),
            Manager::Helm { namespace, release } if namespace.is_empty() => {
                format!("Helm release {release}")
            }
            Manager::Helm { namespace, release } => format!("Helm release {namespace}/{release}"),
            Manager::Other(name) => format!("{name} (app.kubernetes.io/managed-by)"),
        }
    }
}

/// A controller owner reference.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OwnerRef {
    pub api_version: String,
    pub kind: String,
    pub name: String,
    pub uid: String,
}

/// What an object's own metadata says about who manages it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ownership {
    Managed(Manager),
    /// No manager marks, but a controller owner that may have one.
    Owned(OwnerRef),
    Unmanaged,
}

fn string_map<'a>(obj: &'a Value, field: &str) -> impl Fn(&str) -> Option<&'a str> {
    let map = obj.pointer(&format!("/metadata/{field}"));
    move |key: &str| map.and_then(|m| m.get(key)).and_then(Value::as_str)
}

/// The manager an object's own metadata **definitively** names — Flux
/// labels or Argo CD tracking, which mark objects directly and aren't
/// copied down to controller-created children.
pub fn detect_manager(obj: &Value) -> Option<Manager> {
    let label = string_map(obj, "labels");
    let annotation = string_map(obj, "annotations");

    for (prefix, kind) in [
        ("kustomize.toolkit.fluxcd.io", "Kustomization"),
        ("helm.toolkit.fluxcd.io", "HelmRelease"),
        ("resourceset.fluxcd.controlplane.io", "ResourceSet"),
    ] {
        if let Some(name) = label(&format!("{prefix}/name")) {
            return Some(Manager::Flux {
                kind: kind.to_string(),
                namespace: label(&format!("{prefix}/namespace"))
                    .unwrap_or_default()
                    .to_string(),
                name: name.to_string(),
            });
        }
    }
    if label("app.kubernetes.io/managed-by") == Some("flux-operator") {
        return Some(Manager::Flux {
            kind: "FluxInstance".to_string(),
            namespace: label("fluxcd.controlplane.io/namespace")
                .unwrap_or_default()
                .to_string(),
            name: label("fluxcd.controlplane.io/name")
                .unwrap_or("flux")
                .to_string(),
        });
    }

    if let Some(app) = annotation("argocd.argoproj.io/tracking-id")
        .and_then(|id| id.split(':').next())
        .or_else(|| label("argocd.argoproj.io/instance"))
        .filter(|app| !app.is_empty())
    {
        return Some(Manager::ArgoCd {
            app: app.to_string(),
        });
    }

    None
}

/// Helm's release annotations. Only decisive on objects nothing owns: the
/// Deployment controller copies a Deployment's annotations onto its
/// ReplicaSets (but not its labels), so an owned ReplicaSet would otherwise
/// claim "Helm" for pods whose Deployment Flux actually manages.
fn helm_manager(obj: &Value) -> Option<Manager> {
    let annotation = string_map(obj, "annotations");
    annotation("meta.helm.sh/release-name").map(|release| Manager::Helm {
        namespace: annotation("meta.helm.sh/release-namespace")
            .unwrap_or_default()
            .to_string(),
        release: release.to_string(),
    })
}

/// The generic `app.kubernetes.io/managed-by` claim — weak evidence (pod
/// templates copy it), used only when there's no owner chain to follow.
fn weak_manager(obj: &Value) -> Option<Manager> {
    string_map(obj, "labels")("app.kubernetes.io/managed-by")
        .filter(|m| !m.is_empty())
        .map(|m| Manager::Other(m.to_string()))
}

/// The controller owner reference, if any.
pub fn controller_owner(obj: &Value) -> Option<OwnerRef> {
    obj.pointer("/metadata/ownerReferences")?
        .as_array()?
        .iter()
        .find(|r| r.get("controller").and_then(Value::as_bool) == Some(true))
        .and_then(|r| {
            Some(OwnerRef {
                api_version: r.get("apiVersion")?.as_str()?.to_string(),
                kind: r.get("kind")?.as_str()?.to_string(),
                name: r.get("name")?.as_str()?.to_string(),
                uid: r.get("uid")?.as_str()?.to_string(),
            })
        })
}

/// An object's ownership from its own metadata: a definitive manager (Flux,
/// Argo CD), else its controller owner, else Helm release annotations, else
/// a weak `managed-by` claim, else unmanaged.
pub fn ownership(obj: &Value) -> Ownership {
    if let Some(manager) = detect_manager(obj) {
        return Ownership::Managed(manager);
    }
    if let Some(owner) = controller_owner(obj) {
        return Ownership::Owned(owner);
    }
    helm_manager(obj)
        .or_else(|| weak_manager(obj))
        .map_or(Ownership::Unmanaged, Ownership::Managed)
}

/// What an owner chain resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    /// A manager claims something up the chain.
    Manager(Manager),
    /// No manager — the topmost owner reached (e.g. a hand-made Deployment).
    Owner { kind: String, name: String },
}

impl Resolved {
    /// Short form for the MANAGED-BY column.
    pub fn short_label(&self) -> String {
        match self {
            Resolved::Manager(m) => m.short_label(),
            Resolved::Owner { kind, .. } => kind.clone(),
        }
    }
}

/// MANAGED-BY column text: the manager's kind (`Flux ks`, `Helm`, …) from
/// the object itself or its resolved owner chain; the direct owner's kind
/// while that resolves; the top owner's kind when nothing manages the
/// chain; `-` when unmanaged.
pub fn managed_by_text(own: &Ownership, resolved: Option<&Resolved>) -> String {
    match (own, resolved) {
        (Ownership::Managed(m), _) => m.short_label(),
        (Ownership::Owned(_), Some(r)) => r.short_label(),
        (Ownership::Owned(owner), None) => owner.kind.clone(),
        (Ownership::Unmanaged, _) => "-".to_string(),
    }
}

/// Full Managed By text for one object (describe, workload detail),
/// resolving its owner chain if it has one — a handful of lookups at most.
pub async fn describe_manager(client: &kube::Client, obj: &Value) -> String {
    match ownership(obj) {
        Ownership::Managed(manager) => manager.describe(),
        Ownership::Unmanaged => "not managed (no Flux, Argo CD, Helm, or owner)".to_string(),
        Ownership::Owned(owner) => {
            let namespace = obj
                .pointer("/metadata/namespace")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let mut resolver = OwnerResolver::default();
            resolver
                .resolve_many(client, vec![(owner.clone(), namespace)])
                .await;
            describe_resolution(&owner, resolver.get(&owner.uid))
        }
    }
}

/// Managed By text for an owned object once its chain is (or isn't) known.
fn describe_resolution(owner: &OwnerRef, resolved: Option<&Resolved>) -> String {
    let via = format!("via {}/{}", owner.kind, owner.name);
    match resolved {
        Some(Resolved::Manager(manager)) => format!("{} ({via})", manager.describe()),
        Some(Resolved::Owner { kind, name }) => format!("not managed — top owner {kind}/{name}"),
        None => format!(
            "owned by {}/{} (chain not readable)",
            owner.kind, owner.name
        ),
    }
}

/// Walks owner chains for one list, caching by owner UID so a Deployment's
/// many pods cost one lookup per ReplicaSet (and one for the Deployment).
#[derive(Default)]
pub struct OwnerResolver {
    resolved: HashMap<String, Resolved>,
    /// UIDs that failed (RBAC, gone) — not retried for this list.
    failed: HashSet<String>,
    apis: HashMap<(String, String), Option<(ApiResource, Scope)>>,
    lookups: usize,
}

impl OwnerResolver {
    pub fn get(&self, uid: &str) -> Option<&Resolved> {
        self.resolved.get(uid)
    }

    /// Record a resolution directly (tests).
    #[cfg(test)]
    pub(crate) fn seed(&mut self, uid: &str, resolved: Resolved) {
        self.resolved.insert(uid.to_string(), resolved);
    }

    /// Whether `uid` still needs resolving (and the lookup budget allows).
    pub fn wants(&self, uid: &str) -> bool {
        !self.resolved.contains_key(uid)
            && !self.failed.contains(uid)
            && self.lookups < MAX_OWNER_LOOKUPS
    }

    async fn api_for(
        &mut self,
        client: &kube::Client,
        owner: &OwnerRef,
        namespace: &str,
    ) -> Option<Api<DynamicObject>> {
        let key = (owner.api_version.clone(), owner.kind.clone());
        if !self.apis.contains_key(&key) {
            let (group, version) = owner
                .api_version
                .split_once('/')
                .unwrap_or(("", owner.api_version.as_str()));
            let gvk = GroupVersionKind::gvk(group, version, &owner.kind);
            let found =
                tokio::time::timeout(LOOKUP_TIMEOUT, kube::discovery::pinned_kind(client, &gvk))
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .map(|(resource, caps)| (resource, caps.scope));
            self.apis.insert(key.clone(), found);
        }
        let (resource, scope) = self.apis.get(&key)?.as_ref()?;
        Some(match scope {
            Scope::Namespaced => Api::namespaced_with(client.clone(), namespace, resource),
            Scope::Cluster => Api::all_with(client.clone(), resource),
        })
    }

    /// Resolve many owner chains at once, level by level: each level's
    /// lookups run concurrently, every hop is cached, and chains sharing an
    /// owner (a Deployment's ReplicaSets) share its lookup. Failures degrade
    /// to the direct owner and aren't retried.
    pub async fn resolve_many(&mut self, client: &kube::Client, owners: Vec<(OwnerRef, String)>) {
        use futures::StreamExt;

        // Each chain: the UIDs walked so far and the owner to look up next.
        struct Chain {
            uids: Vec<String>,
            next: OwnerRef,
            namespace: String,
        }
        let mut chains: Vec<Chain> = owners
            .into_iter()
            .filter(|(owner, _)| self.wants(&owner.uid))
            .map(|(owner, namespace)| Chain {
                uids: vec![owner.uid.clone()],
                next: owner,
                namespace,
            })
            .collect();

        for _ in 0..MAX_OWNER_DEPTH {
            // Chains that hit a cached hop are done.
            let mut open = Vec::new();
            for chain in chains {
                if let Some(done) = self.resolved.get(&chain.next.uid).cloned() {
                    self.finish(&chain.uids, Some(done));
                } else {
                    open.push(chain);
                }
            }
            if open.is_empty() {
                return;
            }

            // Distinct lookups this level, within the budget.
            let mut targets: Vec<(OwnerRef, String)> = Vec::new();
            for chain in &open {
                if !targets.iter().any(|(o, _)| o.uid == chain.next.uid)
                    && self.lookups < MAX_OWNER_LOOKUPS
                {
                    self.lookups += 1;
                    targets.push((chain.next.clone(), chain.namespace.clone()));
                }
            }
            let mut apis = Vec::new();
            for (owner, namespace) in &targets {
                apis.push(self.api_for(client, owner, namespace).await);
            }
            let fetched: HashMap<String, Option<Value>> =
                futures::stream::iter(targets.into_iter().zip(apis))
                    .map(|((owner, _), api)| async move {
                        let value = match api {
                            Some(api) => tokio::time::timeout(LOOKUP_TIMEOUT, api.get(&owner.name))
                                .await
                                .ok()
                                .and_then(Result::ok)
                                .and_then(|object| serde_json::to_value(&object).ok()),
                            None => None,
                        };
                        (owner.uid, value)
                    })
                    .buffer_unordered(LOOKUP_CONCURRENCY)
                    .collect()
                    .await;

            // Advance each chain by one hop.
            chains = Vec::new();
            for mut chain in open {
                match fetched.get(&chain.next.uid) {
                    Some(Some(value)) => match ownership(value) {
                        Ownership::Managed(manager) => {
                            self.finish(&chain.uids, Some(Resolved::Manager(manager)));
                        }
                        Ownership::Owned(parent) => {
                            chain.uids.push(parent.uid.clone());
                            chain.next = parent;
                            chains.push(chain);
                        }
                        Ownership::Unmanaged => {
                            let top = Resolved::Owner {
                                kind: chain.next.kind.clone(),
                                name: chain.next.name.clone(),
                            };
                            self.finish(&chain.uids, Some(top));
                        }
                    },
                    // Lookup failed or over budget: degrade.
                    _ => self.finish(&chain.uids, None),
                }
            }
        }
        // Chains still open at the depth limit keep their direct owner.
        for chain in chains {
            self.finish(&chain.uids, None);
        }
    }

    /// Record a chain's outcome for every UID it walked.
    fn finish(&mut self, uids: &[String], outcome: Option<Resolved>) {
        for uid in uids {
            match &outcome {
                Some(resolved) => {
                    self.resolved.insert(uid.clone(), resolved.clone());
                }
                None => {
                    self.failed.insert(uid.clone());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn meta(labels: Value, annotations: Value) -> Value {
        json!({"metadata": {"name": "x", "labels": labels, "annotations": annotations}})
    }

    #[test]
    fn flux_kustomize_helm_resourceset_and_instance() {
        let ks = meta(
            json!({"kustomize.toolkit.fluxcd.io/name": "apps", "kustomize.toolkit.fluxcd.io/namespace": "flux-system"}),
            json!({}),
        );
        assert_eq!(
            detect_manager(&ks).unwrap().describe(),
            "Flux Kustomization flux-system/apps"
        );
        assert_eq!(detect_manager(&ks).unwrap().short_label(), "Flux ks");

        // helm-controller releases also carry Helm's marks — Flux wins.
        let hr = meta(
            json!({"helm.toolkit.fluxcd.io/name": "podinfo", "helm.toolkit.fluxcd.io/namespace": "apps",
                   "app.kubernetes.io/managed-by": "Helm"}),
            json!({"meta.helm.sh/release-name": "podinfo", "meta.helm.sh/release-namespace": "apps"}),
        );
        assert_eq!(detect_manager(&hr).unwrap().short_label(), "Flux hr");
        assert_eq!(
            detect_manager(&hr).unwrap().describe(),
            "Flux HelmRelease apps/podinfo"
        );

        let rset = meta(
            json!({"resourceset.fluxcd.controlplane.io/name": "tenants", "resourceset.fluxcd.controlplane.io/namespace": "flux-system"}),
            json!({}),
        );
        assert_eq!(
            detect_manager(&rset).unwrap().describe(),
            "Flux ResourceSet flux-system/tenants"
        );

        let instance = meta(
            json!({"app.kubernetes.io/managed-by": "flux-operator",
                   "fluxcd.controlplane.io/name": "flux", "fluxcd.controlplane.io/namespace": "flux-system"}),
            json!({}),
        );
        assert_eq!(
            detect_manager(&instance).unwrap().describe(),
            "FluxInstance flux-system/flux"
        );
    }

    #[test]
    fn argo_helm_and_generic_managers() {
        let argo = meta(
            json!({}),
            json!({"argocd.argoproj.io/tracking-id": "guestbook:apps/Deployment:default/guestbook-ui"}),
        );
        assert_eq!(detect_manager(&argo).unwrap().short_label(), "Argo CD");
        assert_eq!(
            detect_manager(&argo).unwrap().describe(),
            "Argo CD application guestbook"
        );

        let helm = meta(
            json!({"app.kubernetes.io/managed-by": "Helm"}),
            json!({"meta.helm.sh/release-name": "metallb", "meta.helm.sh/release-namespace": "metallb-system"}),
        );
        assert_eq!(managed_by_text(&ownership(&helm), None), "Helm");

        // A ReplicaSet carries its Deployment's copied Helm annotations but
        // not its Flux labels: follow the chain instead of trusting them.
        let replica_set = json!({"metadata": {"name": "rs",
            "annotations": {"meta.helm.sh/release-name": "cert-manager"},
            "ownerReferences": [{"apiVersion": "apps/v1", "kind": "Deployment", "name": "cert-manager", "uid": "d", "controller": true}]}});
        assert!(matches!(ownership(&replica_set), Ownership::Owned(_)));

        // Generic managed-by is a weak claim: used only without an owner.
        let other = meta(json!({"app.kubernetes.io/managed-by": "cnpg"}), json!({}));
        assert!(detect_manager(&other).is_none());
        assert_eq!(managed_by_text(&ownership(&other), None), "cnpg");
        let templated_pod = json!({"metadata": {"name": "p",
            "labels": {"app.kubernetes.io/managed-by": "Helm"},
            "ownerReferences": [{"apiVersion": "apps/v1", "kind": "ReplicaSet", "name": "rs", "uid": "u", "controller": true}]}});
        assert!(
            matches!(ownership(&templated_pod), Ownership::Owned(_)),
            "a pod's template label doesn't beat its owner chain"
        );

        assert!(detect_manager(&meta(json!({}), json!({}))).is_none());
    }

    #[test]
    fn controller_owner_is_followed_when_unmanaged() {
        let pod = json!({"metadata": {"name": "web-abc", "ownerReferences": [
            {"apiVersion": "v1", "kind": "ConfigMap", "name": "not-controller", "uid": "u0"},
            {"apiVersion": "apps/v1", "kind": "ReplicaSet", "name": "web-7b5", "uid": "u1", "controller": true}
        ]}});
        let own = ownership(&pod);
        let Ownership::Owned(owner) = &own else {
            panic!("expected an owner, got {own:?}");
        };
        assert_eq!(owner.kind, "ReplicaSet");
        assert_eq!(
            managed_by_text(&own, None),
            "ReplicaSet",
            "direct owner's kind while resolving"
        );
        let resolved = Resolved::Manager(Manager::Flux {
            kind: "Kustomization".into(),
            namespace: "flux-system".into(),
            name: "apps".into(),
        });
        assert_eq!(managed_by_text(&own, Some(&resolved)), "Flux ks");
        assert_eq!(managed_by_text(&Ownership::Unmanaged, None), "-");

        // The full answer lives in describe / workload detail.
        assert_eq!(
            describe_resolution(owner, Some(&resolved)),
            "Flux Kustomization flux-system/apps (via ReplicaSet/web-7b5)"
        );
        let top = Resolved::Owner {
            kind: "Deployment".into(),
            name: "coredns".into(),
        };
        assert_eq!(managed_by_text(&own, Some(&top)), "Deployment");
        assert_eq!(
            describe_resolution(owner, Some(&top)),
            "not managed — top owner Deployment/coredns"
        );
    }

    #[test]
    fn own_manager_beats_owner_reference() {
        let obj = json!({"metadata": {"name": "x",
            "labels": {"kustomize.toolkit.fluxcd.io/name": "apps", "kustomize.toolkit.fluxcd.io/namespace": "flux-system"},
            "ownerReferences": [{"apiVersion": "v1", "kind": "X", "name": "y", "uid": "u", "controller": true}]}});
        assert!(matches!(ownership(&obj), Ownership::Managed(_)));
    }

    #[test]
    fn resolver_budget_stops_new_lookups() {
        let mut resolver = OwnerResolver::default();
        assert!(resolver.wants("a"));
        resolver.lookups = MAX_OWNER_LOOKUPS;
        assert!(!resolver.wants("a"));
        resolver.lookups = 0;
        resolver.failed.insert("gone".into());
        assert!(!resolver.wants("gone"), "failures aren't retried");
    }
}
