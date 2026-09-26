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
/// nothing more to say than the status itself).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectStatus {
    pub health: ObjectHealth,
    pub message: String,
}

impl ObjectStatus {
    pub fn new(health: ObjectHealth, message: impl Into<String>) -> Self {
        Self {
            health,
            message: message.into(),
        }
    }
}

/// Compute the health of a fetched object.
pub fn compute_object_status(obj: &Value) -> ObjectStatus {
    if obj.pointer("/metadata/deletionTimestamp").is_some() {
        return ObjectStatus::new(ObjectHealth::Terminating, "Deletion in progress");
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

    let kind = obj.get("kind").and_then(Value::as_str).unwrap_or_default();
    if let Some(status) = kind_status(kind, obj) {
        return status;
    }
    conditions_status(obj).unwrap_or_else(|| ObjectStatus::new(ObjectHealth::Current, ""))
}

fn int_at(obj: &Value, pointer: &str) -> i64 {
    obj.pointer(pointer).and_then(Value::as_i64).unwrap_or(0)
}

fn str_at<'a>(obj: &'a Value, pointer: &str) -> &'a str {
    obj.pointer(pointer)
        .and_then(Value::as_str)
        .unwrap_or_default()
}

/// Find a status condition by type.
fn condition<'a>(obj: &'a Value, type_: &str) -> Option<&'a Value> {
    obj.pointer("/status/conditions")?
        .as_array()?
        .iter()
        .find(|c| c.get("type").and_then(Value::as_str) == Some(type_))
}

fn condition_is(obj: &Value, type_: &str, status: &str) -> bool {
    condition(obj, type_).and_then(|c| c.get("status").and_then(Value::as_str)) == Some(status)
}

fn condition_message(obj: &Value, type_: &str) -> String {
    condition(obj, type_)
        .and_then(|c| c.get("message").and_then(Value::as_str))
        .unwrap_or_default()
        .to_string()
}

/// Rules for built-in kinds whose health isn't expressed as conditions.
/// `None` falls through to the generic condition rules.
fn kind_status(kind: &str, obj: &Value) -> Option<ObjectStatus> {
    use ObjectHealth::*;
    let status = match kind {
        "Deployment" => {
            if condition(obj, "Progressing").and_then(|c| c.get("reason").and_then(Value::as_str))
                == Some("ProgressDeadlineExceeded")
            {
                return Some(ObjectStatus::new(
                    Failed,
                    condition_message(obj, "Progressing"),
                ));
            }
            let desired = obj
                .pointer("/spec/replicas")
                .and_then(Value::as_i64)
                .unwrap_or(1);
            let total = int_at(obj, "/status/replicas");
            let updated = int_at(obj, "/status/updatedReplicas");
            let ready = int_at(obj, "/status/readyReplicas");
            let available = int_at(obj, "/status/availableReplicas");
            if updated < desired || total > updated {
                ObjectStatus::new(InProgress, format!("Updated: {updated}/{desired}"))
            } else if ready < desired || available < desired {
                ObjectStatus::new(InProgress, format!("Ready: {ready}/{desired}"))
            } else {
                ObjectStatus::new(Current, format!("Replicas: {ready}/{desired}"))
            }
        }
        "StatefulSet" => {
            let desired = obj
                .pointer("/spec/replicas")
                .and_then(Value::as_i64)
                .unwrap_or(1);
            let ready = int_at(obj, "/status/readyReplicas");
            let current_rev = str_at(obj, "/status/currentRevision");
            let update_rev = str_at(obj, "/status/updateRevision");
            if ready < desired {
                ObjectStatus::new(InProgress, format!("Ready: {ready}/{desired}"))
            } else if !update_rev.is_empty() && current_rev != update_rev {
                ObjectStatus::new(InProgress, "Rolling update in progress")
            } else {
                ObjectStatus::new(Current, format!("Replicas: {ready}/{desired}"))
            }
        }
        "DaemonSet" => {
            let desired = int_at(obj, "/status/desiredNumberScheduled");
            let updated = int_at(obj, "/status/updatedNumberScheduled");
            let ready = int_at(obj, "/status/numberReady");
            let available = int_at(obj, "/status/numberAvailable");
            if updated < desired {
                ObjectStatus::new(InProgress, format!("Updated: {updated}/{desired}"))
            } else if ready < desired || available < desired {
                ObjectStatus::new(InProgress, format!("Ready: {ready}/{desired}"))
            } else {
                ObjectStatus::new(Current, format!("Ready: {ready}/{desired}"))
            }
        }
        "ReplicaSet" => {
            let desired = obj
                .pointer("/spec/replicas")
                .and_then(Value::as_i64)
                .unwrap_or(1);
            let ready = int_at(obj, "/status/readyReplicas");
            if ready < desired {
                ObjectStatus::new(InProgress, format!("Ready: {ready}/{desired}"))
            } else {
                ObjectStatus::new(Current, format!("Ready: {ready}/{desired}"))
            }
        }
        "Pod" => pod_status(obj),
        "Job" => {
            if condition_is(obj, "Failed", "True") {
                ObjectStatus::new(Failed, condition_message(obj, "Failed"))
            } else if condition_is(obj, "Complete", "True") {
                ObjectStatus::new(Current, "Job completed")
            } else {
                ObjectStatus::new(InProgress, "Job in progress")
            }
        }
        "PersistentVolumeClaim" => match str_at(obj, "/status/phase") {
            "Bound" => ObjectStatus::new(Current, "Bound"),
            "Lost" => ObjectStatus::new(Failed, "Claim lost its volume"),
            phase => ObjectStatus::new(InProgress, format!("Phase: {phase}")),
        },
        "Service" => {
            let lb_pending = str_at(obj, "/spec/type") == "LoadBalancer"
                && obj
                    .pointer("/status/loadBalancer/ingress")
                    .and_then(Value::as_array)
                    .is_none_or(|ingress| ingress.is_empty());
            if lb_pending {
                ObjectStatus::new(InProgress, "Waiting for load balancer address")
            } else {
                ObjectStatus::new(Current, "")
            }
        }
        "CustomResourceDefinition" => {
            if condition_is(obj, "NamesAccepted", "False") {
                ObjectStatus::new(Failed, condition_message(obj, "NamesAccepted"))
            } else if condition_is(obj, "Established", "True") {
                ObjectStatus::new(Current, "Established")
            } else {
                ObjectStatus::new(InProgress, "Not yet established")
            }
        }
        _ => return None,
    };
    Some(status)
}

fn pod_status(obj: &Value) -> ObjectStatus {
    use ObjectHealth::*;
    match str_at(obj, "/status/phase") {
        "Succeeded" => return ObjectStatus::new(Current, "Pod succeeded"),
        "Failed" => return ObjectStatus::new(Failed, str_at(obj, "/status/message")),
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
        return ObjectStatus::new(Failed, reason);
    }
    if condition_is(obj, "Ready", "True") {
        ObjectStatus::new(Current, "Pod is ready")
    } else {
        ObjectStatus::new(
            InProgress,
            format!("Phase: {}", str_at(obj, "/status/phase")),
        )
    }
}

/// Generic condition rules for custom resources: `Stalled` fails,
/// `Reconciling` progresses, then `Ready` decides. `None` when the object
/// carries none of them.
fn conditions_status(obj: &Value) -> Option<ObjectStatus> {
    use ObjectHealth::*;
    if condition_is(obj, "Stalled", "True") {
        return Some(ObjectStatus::new(Failed, condition_message(obj, "Stalled")));
    }
    if condition_is(obj, "Reconciling", "True") {
        return Some(ObjectStatus::new(
            InProgress,
            condition_message(obj, "Reconciling"),
        ));
    }
    let ready = condition(obj, "Ready")?;
    let message = condition_message(obj, "Ready");
    Some(match ready.get("status").and_then(Value::as_str) {
        Some("True") => ObjectStatus::new(Current, message),
        Some("False") => ObjectStatus::new(Failed, message),
        _ => ObjectStatus::new(InProgress, message),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn health(obj: Value) -> ObjectHealth {
        compute_object_status(&obj).health
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
}
