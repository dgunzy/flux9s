//! Health computation for arbitrary Kubernetes objects (#262).
//!
//! A compact port of the kstatus rules Flux uses for health checks and the
//! Flux Operator web UI uses for its inventory view: deletion and generation
//! checks first, then kind-specific rules for the built-in workload/storage
//! kinds, then the `Stalled` / `Reconciling` / `Ready` conditions any custom
//! resource may carry. Kinds nothing applies to are `Current` — existing is
//! all kstatus can say about a ConfigMap.
//!
//! Pure functions over JSON so every rule is unit-testable without a cluster.

use crate::kube::conditions::{self, Condition};
use crate::models::FluxResourceKind;
use serde_json::Value;
use std::fmt;

/// Health of one object, in kstatus terms plus the lookup failures an
/// inventory row can hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectHealth {
    /// Fully reconciled / available.
    Current,
    /// Rolling out, not yet observed, or otherwise converging.
    InProgress,
    /// Stalled, failed, or `Ready=False`.
    Failed,
    /// A Flux object whose reconciliation is suspended: Flux won't act
    /// on it until it's resumed, whatever its last condition says.
    Suspended,
    /// Has a deletion timestamp.
    Terminating,
    /// Listed in the inventory but no longer in the cluster.
    NotFound,
    /// RBAC denies reading the object.
    Forbidden,
    /// Any other lookup failure (discovery, network, …).
    Unknown,
}

impl ObjectHealth {
    /// Short label for the STATUS column.
    pub fn label(self) -> &'static str {
        match self {
            ObjectHealth::Current => "Current",
            ObjectHealth::InProgress => "InProgress",
            ObjectHealth::Failed => "Failed",
            ObjectHealth::Suspended => "Suspended",
            ObjectHealth::Terminating => "Terminating",
            ObjectHealth::NotFound => "NotFound",
            ObjectHealth::Forbidden => "Forbidden",
            ObjectHealth::Unknown => "Unknown",
        }
    }
}

impl fmt::Display for ObjectHealth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// An object's health plus a one-line explanation (empty when there is
/// nothing more to say than the status itself), and what its own kind calls
/// that state — `Ready` / `Not Ready` / `Suspended` for a Flux object,
/// `Available` for a Deployment, `CrashLoopBackOff` for a pod.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectStatus {
    pub health: ObjectHealth,
    pub message: String,
    /// The kind's own word for the state; `None` shows the health.
    pub state: Option<String>,
    /// How ready it is, in the kind's terms: replicas (`2/3`), a pod's
    /// containers, a Job's completions, or a `Ready` condition (`True`).
    pub ready: Option<String>,
}

impl ObjectStatus {
    pub fn new(health: ObjectHealth, message: impl Into<String>) -> Self {
        Self {
            health,
            message: message.into(),
            state: None,
            ready: None,
        }
    }

    /// Name the state the way the object's kind does.
    pub fn with_state(mut self, state: impl Into<String>) -> Self {
        self.state = Some(state.into());
        self
    }

    pub fn with_ready(mut self, ready: impl Into<String>) -> Self {
        self.ready = Some(ready.into());
        self
    }

    /// What a STATUS column shows: the kind's word, else the health.
    pub fn label(&self) -> &str {
        self.state.as_deref().unwrap_or(self.health.label())
    }
}

/// Compute the health of a fetched object.
pub fn compute_object_status(obj: &Value) -> ObjectStatus {
    if obj.pointer("/metadata/deletionTimestamp").is_some() {
        return ObjectStatus::new(ObjectHealth::Terminating, "Deletion in progress");
    }
    let api_version = obj
        .get("apiVersion")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let kind = obj.get("kind").and_then(Value::as_str).unwrap_or_default();
    if let Some(flux) = FluxResourceKind::from_api(api_version, kind) {
        return flux_status(flux, obj);
    }

    let generation = obj.pointer("/metadata/generation").and_then(Value::as_i64);
    let observed = obj
        .pointer("/status/observedGeneration")
        .and_then(Value::as_i64);
    if let (Some(generation), Some(observed)) = (generation, observed)
        && observed < generation
    {
        return ObjectStatus::new(
            ObjectHealth::InProgress,
            format!("Generation {generation} not yet observed (at {observed})"),
        );
    }

    if is_builtin(kind, obj)
        && let Some(status) = kind_status(kind, obj)
    {
        return status;
    }
    conditions_status(obj).unwrap_or_else(|| ObjectStatus::new(ObjectHealth::Current, ""))
}

/// A Flux object's status in Flux's terms, as the Flux list reads it:
/// suspension first (a suspended object's conditions are stale), then a
/// pending generation, `Stalled`, `Reconciling`, and `Ready`. READY is the
/// `Ready` condition's status.
fn flux_status(kind: FluxResourceKind, obj: &Value) -> ObjectStatus {
    use ObjectHealth::*;
    let ready = condition(obj, conditions::READY);
    let message = condition_message(obj, conditions::READY);
    let with_ready = |status: ObjectStatus| match ready.and_then(Condition::status) {
        Some(ready) => status.with_ready(ready),
        None => status,
    };
    if kind.extract_suspended(obj) == Some(true) {
        return with_ready(ObjectStatus::new(Suspended, message).with_state("Suspended"));
    }
    let generation = obj.pointer("/metadata/generation").and_then(Value::as_i64);
    let observed = obj
        .pointer("/status/observedGeneration")
        .and_then(Value::as_i64);
    if let (Some(generation), Some(observed)) = (generation, observed)
        && observed < generation
    {
        return with_ready(
            ObjectStatus::new(
                InProgress,
                format!("Generation {generation} not yet observed (at {observed})"),
            )
            .with_state("Reconciling"),
        );
    }
    let status = conditions_status(obj).unwrap_or_else(|| ObjectStatus::new(Current, ""));
    with_ready(status)
}

/// The API group each kind-specific rule was written for.
fn builtin_group(kind: &str) -> Option<&'static str> {
    match kind {
        "Deployment" | "StatefulSet" | "DaemonSet" | "ReplicaSet" => Some("apps"),
        "Pod" | "PersistentVolumeClaim" | "Service" => Some(""),
        "Job" => Some("batch"),
        "CustomResourceDefinition" => Some("apiextensions.k8s.io"),
        _ => None,
    }
}

/// Whether the kind-specific rules apply: the kind must come from its
/// built-in API group. A same-named CRD (`services.serving.knative.dev`,
/// `example.com/Deployment`) is judged by the generic condition rules
/// instead — otherwise e.g. a Knative Service would always read Current.
/// Objects without `apiVersion` (none of flux9s's own fetch paths produce
/// these; they stamp it) keep the kind-only behaviour.
fn is_builtin(kind: &str, obj: &Value) -> bool {
    let Some(expected) = builtin_group(kind) else {
        return false;
    };
    match obj.get("apiVersion").and_then(Value::as_str) {
        Some(api_version) => {
            let group = api_version.split_once('/').map_or("", |(group, _)| group);
            group == expected
        }
        None => true,
    }
}

fn int_at(obj: &Value, pointer: &str) -> i64 {
    obj.pointer(pointer).and_then(Value::as_i64).unwrap_or(0)
}

fn str_at<'a>(obj: &'a Value, pointer: &str) -> &'a str {
    obj.pointer(pointer)
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn condition<'a>(obj: &'a Value, type_: &str) -> Option<Condition<'a>> {
    conditions::find(obj, type_)
}

fn condition_is(obj: &Value, type_: &str, status: &str) -> bool {
    condition(obj, type_).and_then(Condition::status) == Some(status)
}

fn condition_message(obj: &Value, type_: &str) -> String {
    condition(obj, type_)
        .and_then(Condition::message)
        .unwrap_or_default()
        .to_string()
}

/// Rules for built-in kinds whose health isn't expressed as conditions.
/// `None` falls through to the generic condition rules.
fn kind_status(kind: &str, obj: &Value) -> Option<ObjectStatus> {
    use ObjectHealth::*;
    let status = match kind {
        "Deployment" => {
            if condition(obj, "Progressing").and_then(Condition::reason)
                == Some("ProgressDeadlineExceeded")
            {
                return Some(
                    ObjectStatus::new(Failed, condition_message(obj, "Progressing"))
                        .with_state("Deadline exceeded"),
                );
            }
            let desired = obj
                .pointer("/spec/replicas")
                .and_then(Value::as_i64)
                .unwrap_or(1);
            let total = int_at(obj, "/status/replicas");
            let updated = int_at(obj, "/status/updatedReplicas");
            let ready = int_at(obj, "/status/readyReplicas");
            let available = int_at(obj, "/status/availableReplicas");
            let status = if updated < desired || total > updated {
                ObjectStatus::new(InProgress, format!("Updated: {updated}/{desired}"))
                    .with_state("Rolling out")
            } else if ready < desired || available < desired {
                ObjectStatus::new(InProgress, format!("Ready: {ready}/{desired}"))
                    .with_state("Progressing")
            } else if desired == 0 {
                ObjectStatus::new(Current, "Replicas: 0/0").with_state("Scaled down")
            } else {
                ObjectStatus::new(Current, format!("Replicas: {ready}/{desired}"))
                    .with_state("Available")
            };
            status.with_ready(format!("{ready}/{desired}"))
        }
        "StatefulSet" => {
            let desired = obj
                .pointer("/spec/replicas")
                .and_then(Value::as_i64)
                .unwrap_or(1);
            let ready = int_at(obj, "/status/readyReplicas");
            let current_rev = str_at(obj, "/status/currentRevision");
            let update_rev = str_at(obj, "/status/updateRevision");
            let status = if ready < desired {
                ObjectStatus::new(InProgress, format!("Ready: {ready}/{desired}"))
                    .with_state("Progressing")
            } else if !update_rev.is_empty() && current_rev != update_rev {
                ObjectStatus::new(InProgress, "Rolling update in progress")
                    .with_state("Rolling out")
            } else {
                ObjectStatus::new(Current, format!("Replicas: {ready}/{desired}"))
                    .with_state("Available")
            };
            status.with_ready(format!("{ready}/{desired}"))
        }
        "DaemonSet" => {
            let desired = int_at(obj, "/status/desiredNumberScheduled");
            let updated = int_at(obj, "/status/updatedNumberScheduled");
            let ready = int_at(obj, "/status/numberReady");
            let available = int_at(obj, "/status/numberAvailable");
            let status = if updated < desired {
                ObjectStatus::new(InProgress, format!("Updated: {updated}/{desired}"))
                    .with_state("Rolling out")
            } else if ready < desired || available < desired {
                ObjectStatus::new(InProgress, format!("Ready: {ready}/{desired}"))
                    .with_state("Progressing")
            } else {
                ObjectStatus::new(Current, format!("Ready: {ready}/{desired}"))
                    .with_state("Available")
            };
            status.with_ready(format!("{ready}/{desired}"))
        }
        "ReplicaSet" => {
            let desired = obj
                .pointer("/spec/replicas")
                .and_then(Value::as_i64)
                .unwrap_or(1);
            let ready = int_at(obj, "/status/readyReplicas");
            let status = if ready < desired {
                ObjectStatus::new(InProgress, format!("Ready: {ready}/{desired}"))
            } else {
                ObjectStatus::new(Current, format!("Ready: {ready}/{desired}"))
            };
            status.with_ready(format!("{ready}/{desired}"))
        }
        "Pod" => pod_status(obj),
        "Job" => {
            let completions = obj
                .pointer("/spec/completions")
                .and_then(Value::as_i64)
                .unwrap_or(1);
            let succeeded = int_at(obj, "/status/succeeded");
            let status = if condition_is(obj, "Failed", "True") {
                ObjectStatus::new(Failed, condition_message(obj, "Failed")).with_state("Failed")
            } else if condition_is(obj, "Complete", "True") {
                ObjectStatus::new(Current, "Job completed").with_state("Complete")
            } else {
                ObjectStatus::new(InProgress, "Job in progress").with_state("Running")
            };
            status.with_ready(format!("{succeeded}/{completions}"))
        }
        "PersistentVolumeClaim" => match str_at(obj, "/status/phase") {
            "Bound" => ObjectStatus::new(Current, "Bound").with_state("Bound"),
            "Lost" => ObjectStatus::new(Failed, "Claim lost its volume").with_state("Lost"),
            "" => ObjectStatus::new(InProgress, "Phase: "),
            phase => ObjectStatus::new(InProgress, format!("Phase: {phase}")).with_state(phase),
        },
        "Service" => {
            let lb_pending = str_at(obj, "/spec/type") == "LoadBalancer"
                && obj
                    .pointer("/status/loadBalancer/ingress")
                    .and_then(Value::as_array)
                    .is_none_or(|ingress| ingress.is_empty());
            if lb_pending {
                ObjectStatus::new(InProgress, "Waiting for load balancer address")
                    .with_state("Pending")
            } else {
                ObjectStatus::new(Current, "")
            }
        }
        "CustomResourceDefinition" => {
            if condition_is(obj, "NamesAccepted", "False") {
                ObjectStatus::new(Failed, condition_message(obj, "NamesAccepted"))
                    .with_state("Names rejected")
            } else if condition_is(obj, "Established", "True") {
                ObjectStatus::new(Current, "Established").with_state("Established")
            } else {
                ObjectStatus::new(InProgress, "Not yet established").with_state("Pending")
            }
        }
        _ => return None,
    };
    Some(status)
}

/// A pod: its phase, or the reason a container is stuck, with READY its
/// ready containers and the restarts in the message.
fn pod_status(obj: &Value) -> ObjectStatus {
    let statuses = obj
        .pointer("/status/containerStatuses")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let total = statuses.len().max(
        obj.pointer("/spec/containers")
            .and_then(Value::as_array)
            .map_or(0, Vec::len),
    );
    let ready = statuses
        .iter()
        .filter(|s| s.get("ready").and_then(Value::as_bool) == Some(true))
        .count();
    let restarts: i64 = statuses
        .iter()
        .filter_map(|s| s.get("restartCount").and_then(Value::as_i64))
        .sum();
    let mut status = pod_state(obj).with_ready(format!("{ready}/{total}"));
    if restarts > 0 {
        let plural = if restarts == 1 { "" } else { "s" };
        status.message = match status.message.as_str() {
            "" => format!("{restarts} restart{plural}"),
            message => format!("{message} · {restarts} restart{plural}"),
        };
    }
    status
}

fn pod_state(obj: &Value) -> ObjectStatus {
    use ObjectHealth::*;
    match str_at(obj, "/status/phase") {
        "Succeeded" => return ObjectStatus::new(Current, "Pod succeeded").with_state("Completed"),
        "Failed" => {
            let reason = str_at(obj, "/status/reason");
            let state = if reason.is_empty() { "Failed" } else { reason };
            return ObjectStatus::new(Failed, str_at(obj, "/status/message")).with_state(state);
        }
        _ => {}
    }
    // A container stuck restarting or unable to pull fails the pod outright
    // instead of reporting it as perpetually progressing.
    let waiting_reason = obj
        .pointer("/status/containerStatuses")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|cs| cs.pointer("/state/waiting/reason").and_then(Value::as_str))
        .find(|reason| {
            matches!(
                *reason,
                "CrashLoopBackOff"
                    | "ImagePullBackOff"
                    | "ErrImagePull"
                    | "CreateContainerConfigError"
            )
        });
    if let Some(reason) = waiting_reason {
        return ObjectStatus::new(Failed, reason).with_state(reason);
    }
    let phase = str_at(obj, "/status/phase");
    let status = if conditions::is_true(obj, conditions::READY) {
        ObjectStatus::new(Current, "Pod is ready")
    } else {
        ObjectStatus::new(InProgress, format!("Phase: {phase}"))
    };
    if phase.is_empty() {
        status
    } else {
        status.with_state(phase)
    }
}

/// Generic condition rules for custom resources: `Stalled` fails,
/// `Reconciling` progresses, then `Ready` decides. `None` when the object
/// carries none of them.
fn conditions_status(obj: &Value) -> Option<ObjectStatus> {
    use ObjectHealth::*;
    if conditions::is_true(obj, conditions::STALLED) {
        return Some(
            ObjectStatus::new(Failed, condition_message(obj, conditions::STALLED))
                .with_state("Stalled"),
        );
    }
    if conditions::is_true(obj, conditions::RECONCILING) {
        return Some(
            ObjectStatus::new(InProgress, condition_message(obj, "Reconciling"))
                .with_state("Reconciling"),
        );
    }
    let ready = condition(obj, "Ready")?;
    let message = condition_message(obj, "Ready");
    Some(match ready.status() {
        Some("True") => ObjectStatus::new(Current, message).with_state("Ready"),
        Some("False") => ObjectStatus::new(Failed, message).with_state("Not Ready"),
        _ => ObjectStatus::new(InProgress, message).with_state("Progressing"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn health(obj: Value) -> ObjectHealth {
        compute_object_status(&obj).health
    }

    /// `(STATUS, READY)` as a list shows them.
    fn shown(obj: Value) -> (String, Option<String>) {
        let status = compute_object_status(&obj);
        (status.label().to_string(), status.ready)
    }

    #[test]
    fn flux_objects_read_in_fluxs_terms() {
        let ks = |spec: Value, ready: &str| {
            json!({
                "apiVersion": "kustomize.toolkit.fluxcd.io/v1",
                "kind": "Kustomization",
                "metadata": {"name": "apps", "generation": 2},
                "spec": spec,
                "status": {"observedGeneration": 1, "conditions": [
                    {"type": "Ready", "status": ready, "message": "Applied revision: main@sha1:abc"}
                ]}
            })
        };
        // Suspended wins over its (stale) conditions and a pending generation.
        let suspended = compute_object_status(&ks(json!({"suspend": true}), "True"));
        assert_eq!(suspended.health, ObjectHealth::Suspended);
        assert_eq!(suspended.label(), "Suspended");
        assert_eq!(suspended.ready.as_deref(), Some("True"));
        assert_eq!(suspended.message, "Applied revision: main@sha1:abc");

        assert_eq!(
            shown(ks(json!({}), "True")),
            ("Reconciling".into(), Some("True".into())),
            "generation 2 not yet observed"
        );
        let mut current = ks(json!({}), "False");
        current["status"]["observedGeneration"] = json!(2);
        assert_eq!(shown(current), ("Not Ready".into(), Some("False".into())));

        // Annotation-suspended kinds read their annotation.
        let generator = json!({
            "apiVersion": FluxResourceKind::ArtifactGenerator.api_resource().api_version,
            "kind": "ArtifactGenerator",
            "metadata": {"name": "g", "annotations": {"source.extensions.fluxcd.io/reconcile": "disabled"}},
        });
        assert_eq!(shown(generator).0, "Suspended");

        // A same-named kind of another API isn't Flux's.
        let crossplane = json!({
            "apiVersion": "s3.aws.upbound.io/v1beta1",
            "kind": "Bucket",
            "spec": {"suspend": true},
            "status": {"conditions": [{"type": "Ready", "status": "True"}]}
        });
        assert_eq!(shown(crossplane), ("Ready".into(), None));
    }

    #[test]
    fn workloads_say_how_ready_they_are() {
        let deploy = |ready: i64| {
            json!({
                "apiVersion": "apps/v1", "kind": "Deployment",
                "spec": {"replicas": 2},
                "status": {"replicas": 2, "updatedReplicas": 2, "readyReplicas": ready, "availableReplicas": ready}
            })
        };
        assert_eq!(shown(deploy(2)), ("Available".into(), Some("2/2".into())));
        assert_eq!(shown(deploy(1)), ("Progressing".into(), Some("1/2".into())));

        let pod = compute_object_status(&json!({
            "apiVersion": "v1", "kind": "Pod",
            "spec": {"containers": [{"name": "app"}, {"name": "proxy"}]},
            "status": {"phase": "Running", "containerStatuses": [
                {"ready": false, "restartCount": 3, "state": {"waiting": {"reason": "CrashLoopBackOff"}}},
                {"ready": true, "restartCount": 0}
            ]}
        }));
        assert_eq!(pod.label(), "CrashLoopBackOff");
        assert_eq!(pod.ready.as_deref(), Some("1/2"));
        assert_eq!(pod.message, "CrashLoopBackOff · 3 restarts");

        let job = json!({
            "apiVersion": "batch/v1", "kind": "Job",
            "spec": {"completions": 1},
            "status": {"succeeded": 1, "conditions": [{"type": "Complete", "status": "True"}]}
        });
        assert_eq!(shown(job), ("Complete".into(), Some("1/1".into())));
        // Kinds without a word of their own show their health.
        assert_eq!(
            shown(json!({"apiVersion": "v1", "kind": "ConfigMap"})),
            ("Current".into(), None)
        );
    }

    #[test]
    fn plain_objects_are_current() {
        assert_eq!(
            health(json!({"kind": "ConfigMap", "metadata": {"name": "c"}})),
            ObjectHealth::Current
        );
    }

    #[test]
    fn deletion_timestamp_is_terminating() {
        assert_eq!(
            health(json!({
                "kind": "ConfigMap",
                "metadata": {"deletionTimestamp": "2026-09-26T00:00:00Z"}
            })),
            ObjectHealth::Terminating
        );
    }

    #[test]
    fn unobserved_generation_is_in_progress() {
        assert_eq!(
            health(json!({
                "kind": "Deployment",
                "metadata": {"generation": 3},
                "status": {"observedGeneration": 2}
            })),
            ObjectHealth::InProgress
        );
    }

    #[test]
    fn deployment_rules() {
        let ready = json!({
            "kind": "Deployment",
            "spec": {"replicas": 2},
            "status": {"replicas": 2, "updatedReplicas": 2, "readyReplicas": 2, "availableReplicas": 2}
        });
        assert_eq!(health(ready), ObjectHealth::Current);

        let rolling = json!({
            "kind": "Deployment",
            "spec": {"replicas": 2},
            "status": {"replicas": 3, "updatedReplicas": 2, "readyReplicas": 2, "availableReplicas": 2}
        });
        assert_eq!(health(rolling), ObjectHealth::InProgress);

        let stuck = json!({
            "kind": "Deployment",
            "spec": {"replicas": 1},
            "status": {"conditions": [{
                "type": "Progressing", "status": "False",
                "reason": "ProgressDeadlineExceeded", "message": "timed out"
            }]}
        });
        let status = compute_object_status(&stuck);
        assert_eq!(status.health, ObjectHealth::Failed);
        assert_eq!(status.message, "timed out");
    }

    #[test]
    fn pod_crashloop_fails() {
        assert_eq!(
            health(json!({
                "kind": "Pod",
                "status": {
                    "phase": "Running",
                    "containerStatuses": [{"state": {"waiting": {"reason": "CrashLoopBackOff"}}}]
                }
            })),
            ObjectHealth::Failed
        );
    }

    #[test]
    fn pending_load_balancer_is_in_progress() {
        assert_eq!(
            health(json!({"kind": "Service", "spec": {"type": "LoadBalancer"}, "status": {}})),
            ObjectHealth::InProgress
        );
        assert_eq!(
            health(json!({"kind": "Service", "spec": {"type": "ClusterIP"}})),
            ObjectHealth::Current
        );
    }

    #[test]
    fn pvc_and_job_rules() {
        assert_eq!(
            health(json!({"kind": "PersistentVolumeClaim", "status": {"phase": "Pending"}})),
            ObjectHealth::InProgress
        );
        assert_eq!(
            health(json!({
                "kind": "Job",
                "status": {"conditions": [{"type": "Complete", "status": "True"}]}
            })),
            ObjectHealth::Current
        );
    }

    #[test]
    fn custom_resource_conditions() {
        let cr = |conds: Value| json!({"kind": "Certificate", "status": {"conditions": conds}});
        assert_eq!(
            health(cr(json!([{"type": "Ready", "status": "True"}]))),
            ObjectHealth::Current
        );
        assert_eq!(
            health(cr(json!([{"type": "Ready", "status": "False"}]))),
            ObjectHealth::Failed
        );
        assert_eq!(
            health(cr(json!([{"type": "Ready", "status": "Unknown"}]))),
            ObjectHealth::InProgress
        );
        // Stalled beats Ready.
        assert_eq!(
            health(cr(json!([
                {"type": "Ready", "status": "True"},
                {"type": "Stalled", "status": "True"}
            ]))),
            ObjectHealth::Failed
        );
    }

    #[test]
    fn kind_rules_only_apply_to_their_builtin_group() {
        // Knative Service: a CRD named Service, not the core one. The core
        // rule would say Current; its Ready=False condition must win.
        let knative = json!({
            "apiVersion": "serving.knative.dev/v1",
            "kind": "Service",
            "spec": {"type": "ClusterIP"},
            "status": {"conditions": [{"type": "Ready", "status": "False", "message": "RevisionFailed"}]}
        });
        let status = compute_object_status(&knative);
        assert_eq!(status.health, ObjectHealth::Failed);
        assert_eq!(status.message, "RevisionFailed");

        // A same-named CRD with no conditions isn't judged by apps rules.
        let fake =
            json!({"apiVersion": "example.com/v1", "kind": "Deployment", "spec": {"replicas": 3}});
        assert_eq!(health(fake), ObjectHealth::Current);

        // The real kinds still get their rules.
        let core_lb = json!({"apiVersion": "v1", "kind": "Service", "spec": {"type": "LoadBalancer"}, "status": {}});
        assert_eq!(health(core_lb), ObjectHealth::InProgress);
        let apps = json!({"apiVersion": "apps/v1", "kind": "Deployment", "spec": {"replicas": 2},
            "status": {"replicas": 2, "updatedReplicas": 2, "readyReplicas": 1, "availableReplicas": 1}});
        assert_eq!(health(apps), ObjectHealth::InProgress);
        let job = json!({"apiVersion": "batch/v1", "kind": "Job", "status": {}});
        assert_eq!(health(job), ObjectHealth::InProgress);
    }
}
