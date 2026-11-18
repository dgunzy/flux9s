//! An in-process API server for unit tests, so code that talks to the
//! cluster (trace, graph, operations) runs for real — the same requests,
//! version fallback, and error handling — against canned objects.
//!
//! It serves `GET` by path (404 `NotFound` otherwise), applies merge
//! `PATCH`es and records them, and acts as a responsive controller: a patch
//! requesting a reconcile (`reconcile.fluxcd.io/requestedAt`) is reported
//! handled in `status.lastHandledReconcileAt`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use serde_json::{Value, json};

/// Canned cluster state behind a [`kube::Client`].
#[derive(Clone, Default)]
pub struct FakeCluster {
    state: Arc<Mutex<State>>,
}

#[derive(Default)]
struct State {
    objects: HashMap<String, Value>,
    /// `(method, path)` of every write, in order.
    writes: Vec<(String, String)>,
}

impl FakeCluster {
    /// A cluster holding `objects` (each with `apiVersion`, `kind`, and
    /// `metadata.name` / `namespace`).
    pub fn new(objects: impl IntoIterator<Item = Value>) -> Self {
        let cluster = Self::default();
        {
            let mut state = cluster.lock();
            for obj in objects {
                if let Some(path) = path_of(&obj) {
                    state.objects.insert(path, obj);
                }
            }
        }
        cluster
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The writes made so far, as `(method, path)`.
    pub fn writes(&self) -> Vec<(String, String)> {
        self.lock().writes.clone()
    }

    /// A client whose every request this cluster answers.
    pub fn client(&self) -> kube::Client {
        let state = self.state.clone();
        let service = tower::service_fn(move |request: http::Request<kube::client::Body>| {
            let state = state.clone();
            async move {
                let method = request.method().to_string();
                let path = request.uri().path().to_string();
                let body = request
                    .into_body()
                    .collect_bytes()
                    .await
                    .unwrap_or_default();
                let found = {
                    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
                    match method.as_str() {
                        "GET" => state
                            .objects
                            .get(&path)
                            .cloned()
                            .or_else(|| discovery(&state.objects, &path)),
                        "PATCH" => {
                            state.writes.push((method.clone(), path.clone()));
                            let patch = serde_json::from_slice(&body).unwrap_or(Value::Null);
                            state.objects.get_mut(&path).map(|obj| {
                                merge(obj, &patch);
                                mark_handled(obj);
                                obj.clone()
                            })
                        }
                        _ => {
                            state.writes.push((method.clone(), path.clone()));
                            None
                        }
                    }
                };
                let (status, body) = match found {
                    Some(obj) => (200, obj),
                    None => (404, not_found(&path)),
                };
                http::Response::builder()
                    .status(status)
                    .header("content-type", "application/json")
                    .body(kube::client::Body::from(
                        serde_json::to_vec(&body).unwrap_or_default(),
                    ))
            }
        });
        kube::Client::new(service, "default")
    }
}

/// A Flux object at its newest API version, `body` merged over the
/// identity (labels, `spec`, `status`, …).
pub fn flux(kind: &str, namespace: &str, name: &str, body: Value) -> Value {
    let api_version = crate::kube::api::api_resource_for_kind(kind)
        .map(|resource| resource.api_version)
        .unwrap_or_default();
    object(&api_version, kind, namespace, name, body)
}

/// Any object, `body` merged over its identity.
pub fn object(api_version: &str, kind: &str, namespace: &str, name: &str, body: Value) -> Value {
    let mut obj = json!({"apiVersion": api_version, "kind": kind,
        "metadata": {"name": name, "namespace": namespace, "uid": format!("uid-{kind}-{name}")}});
    merge(&mut obj, &body);
    obj
}

/// A `Ready=True` status, merged into fixtures that should look healthy.
pub fn ready() -> Value {
    json!({"status": {"conditions": [{"type": "Ready", "status": "True"}]}})
}

/// The REST path an object is served at.
fn path_of(obj: &Value) -> Option<String> {
    let api_version = obj.get("apiVersion")?.as_str()?;
    let kind = obj.get("kind")?.as_str()?;
    let name = obj.pointer("/metadata/name")?.as_str()?;
    let namespace = obj.pointer("/metadata/namespace").and_then(Value::as_str);
    let (group, version) = api_version.split_once('/').unwrap_or(("", api_version));
    let plural = match crate::kube::api::api_resource_for_kind(kind) {
        Ok(resource) if resource.group == group => resource.plural,
        _ => {
            kube::discovery::ApiResource::from_gvk(&kube::api::GroupVersionKind::gvk(
                group, version, kind,
            ))
            .plural
        }
    };
    let root = if group.is_empty() {
        format!("/api/{version}")
    } else {
        format!("/apis/{group}/{version}")
    };
    Some(match namespace {
        Some(ns) => format!("{root}/namespaces/{ns}/{plural}/{name}"),
        None => format!("{root}/{plural}/{name}"),
    })
}

/// The discovery document for a group version (`/api/v1`,
/// `/apis/apps/v1`), listing the kinds stored under it — what native
/// object lookups resolve their plural from.
fn discovery(objects: &HashMap<String, Value>, path: &str) -> Option<Value> {
    let group_version = path
        .strip_prefix("/apis/")
        .or_else(|| path.strip_prefix("/api/"))?;
    let prefix = format!("{path}/");
    let mut resources: Vec<Value> = Vec::new();
    for (object_path, obj) in objects {
        let Some(rest) = object_path.strip_prefix(&prefix) else {
            continue;
        };
        let segments: Vec<&str> = rest.split('/').collect();
        let (plural, namespaced) = match segments.as_slice() {
            ["namespaces", _, plural, _] => (*plural, true),
            [plural, _] => (*plural, false),
            _ => continue,
        };
        if resources.iter().any(|r| r["name"] == plural) {
            continue;
        }
        resources.push(
            json!({"name": plural, "singularName": "", "namespaced": namespaced,
            "kind": obj["kind"], "verbs": ["get", "list", "watch", "patch"]}),
        );
    }
    (!resources.is_empty()).then(|| {
        json!({"kind": "APIResourceList", "apiVersion": "v1",
            "groupVersion": group_version, "resources": resources})
    })
}

/// RFC 7386 JSON merge patch.
fn merge(target: &mut Value, patch: &Value) {
    let Value::Object(patch) = patch else {
        *target = patch.clone();
        return;
    };
    if !target.is_object() {
        *target = json!({});
    }
    if let Value::Object(target) = target {
        for (key, value) in patch {
            if value.is_null() {
                target.remove(key);
            } else {
                merge(target.entry(key.clone()).or_insert(Value::Null), value);
            }
        }
    }
}

/// A responsive controller: a requested reconcile is handled at once.
fn mark_handled(obj: &mut Value) {
    if let Some(requested) = obj
        .pointer("/metadata/annotations/reconcile.fluxcd.io~1requestedAt")
        .cloned()
    {
        merge(
            obj,
            &json!({"status": {"lastHandledReconcileAt": requested}}),
        );
    }
}

/// The API server's answer for an object that doesn't exist.
fn not_found(path: &str) -> Value {
    json!({"kind": "Status", "apiVersion": "v1", "status": "Failure",
        "message": format!("{path} not found"), "reason": "NotFound", "code": 404})
}

/// Shared fixtures: the pipelines trace and graph are tested against.
pub mod fixtures {
    use super::{flux, merge, object, ready};
    use serde_json::{Value, json};

    fn healthy(mut obj: Value) -> Value {
        merge(&mut obj, &ready());
        obj
    }

    /// The parent Kustomization `flux-system/flux-system`, pulling from
    /// GitRepository `flux-system/fleet`.
    fn fleet() -> Vec<Value> {
        vec![
            healthy(flux(
                "GitRepository",
                "flux-system",
                "fleet",
                json!({"spec":
                {"url": "https://github.com/example/fleet"}}),
            )),
            healthy(flux(
                "Kustomization",
                "flux-system",
                "flux-system",
                json!({"spec":
                {"path": "./clusters/prod", "sourceRef": {"kind": "GitRepository", "name": "fleet"}}}),
            )),
        ]
    }

    /// Labels saying `flux-system/flux-system` applied an object.
    fn applied_by_fleet() -> Value {
        json!({"metadata": {"labels": {
            "kustomize.toolkit.fluxcd.io/name": "flux-system",
            "kustomize.toolkit.fluxcd.io/namespace": "flux-system"}}})
    }

    /// The user-reported shape: Kustomization `team-a/apps`, applied by the
    /// fleet Kustomization, sourced from ExternalArtifact
    /// `flux-system/apps`, produced by ArtifactGenerator `flux-system/gen`
    /// from GitRepository `flux-system/repo`. The parent pulls from a
    /// *different* repository (`fleet`), so taking the parent's source for
    /// the child's shows.
    pub fn external_artifact_pipeline() -> Vec<Value> {
        let mut child = flux(
            "Kustomization",
            "team-a",
            "apps",
            json!({"spec": {"path": "./",
            "sourceRef": {"kind": "ExternalArtifact", "name": "apps", "namespace": "flux-system"}}}),
        );
        merge(&mut child, &applied_by_fleet());
        let mut objects = fleet();
        objects.extend([
            healthy(flux(
                "GitRepository",
                "flux-system",
                "repo",
                json!({"spec":
                {"url": "https://github.com/example/apps"}}),
            )),
            healthy(flux(
                "ArtifactGenerator",
                "flux-system",
                "gen",
                json!({"spec": {"sources": [
                {"alias": "repo", "kind": "GitRepository", "name": "repo"}]}}),
            )),
            healthy(flux(
                "ExternalArtifact",
                "flux-system",
                "apps",
                json!({"spec": {"sourceRef": {
                "apiVersion": "source.extensions.fluxcd.io/v1beta1",
                "kind": "ArtifactGenerator", "name": "gen", "namespace": "flux-system"}}}),
            )),
            healthy(child),
        ]);
        objects
    }

    /// Pod `apps/web-abc` → ReplicaSet → Deployment `apps/web`, applied by
    /// HelmRelease `apps/web` (itself applied by the fleet Kustomization),
    /// whose chart helm-controller generated as HelmChart
    /// `flux-system/apps-web` from HelmRepository `flux-system/charts`.
    pub fn helm_pipeline() -> Vec<Value> {
        let owned_by = |kind: &str, name: &str| {
            json!({"metadata": {"ownerReferences": [{"apiVersion": "apps/v1", "kind": kind,
                "name": name, "uid": format!("uid-{kind}-{name}"), "controller": true}]}})
        };
        let mut release = flux(
            "HelmRelease",
            "apps",
            "web",
            json!({
            "spec": {"chart": {"spec": {"chart": "web", "sourceRef":
                {"kind": "HelmRepository", "name": "charts", "namespace": "flux-system"}}}},
            "status": {"helmChart": "flux-system/apps-web"}}),
        );
        merge(&mut release, &applied_by_fleet());
        let mut objects = fleet();
        objects.extend([
            object(
                "v1",
                "Pod",
                "apps",
                "web-abc",
                owned_by("ReplicaSet", "web-7b5"),
            ),
            object(
                "apps/v1",
                "ReplicaSet",
                "apps",
                "web-7b5",
                owned_by("Deployment", "web"),
            ),
            object(
                "apps/v1",
                "Deployment",
                "apps",
                "web",
                json!({"metadata": {"labels": {
                "helm.toolkit.fluxcd.io/name": "web",
                "helm.toolkit.fluxcd.io/namespace": "apps"}}}),
            ),
            healthy(release),
            healthy(flux(
                "HelmChart",
                "flux-system",
                "apps-web",
                json!({"spec": {"chart": "web",
                "sourceRef": {"kind": "HelmRepository", "name": "charts"}}}),
            )),
            healthy(flux(
                "HelmRepository",
                "flux-system",
                "charts",
                json!({"spec":
                {"url": "https://charts.example.com"}}),
            )),
        ]);
        objects
    }
}
