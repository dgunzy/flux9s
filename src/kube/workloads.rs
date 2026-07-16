//! Workload drill-down data (#194).
//!
//! Fetches everything the workload detail view shows for a Deployment,
//! StatefulSet, DaemonSet, or CronJob discovered in a Flux resource's
//! inventory: rollout summary, containers and their images, the pods behind
//! the workload's own label selector, and the workload's Events.

use anyhow::Context;
use k8s_openapi::api::core::v1::Pod;
use kube::Api;
use kube::api::ListParams;
use serde_json::Value;

/// A workload reference as carried inside a graph WorkloadGroup node's
/// description, one per line: `Kind|name|namespace|<indicator>|<status>`.
/// This struct is the single source of truth for that format — the graph
/// builder encodes with [`Self::to_graph_line`], and the graph renderer and
/// the workload list view decode with [`Self::parse_graph_line`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadRef {
    pub kind: String,
    pub name: String,
    pub namespace: String,
    /// The pre-rendered readiness indicator (●/○/?).
    pub indicator: String,
    /// Human-readable status (e.g. "Replicas: 1/1").
    pub status: String,
}

impl WorkloadRef {
    /// Encode for a WorkloadGroup node description line.
    pub fn to_graph_line(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}",
            self.kind, self.name, self.namespace, self.indicator, self.status
        )
    }

    /// Decode a WorkloadGroup node description line.
    pub fn parse_graph_line(line: &str) -> Option<Self> {
        let mut parts = line.splitn(5, '|');
        Some(Self {
            kind: parts.next()?.to_string(),
            name: parts.next()?.to_string(),
            namespace: parts.next()?.to_string(),
            indicator: parts.next()?.to_string(),
            status: parts.next().unwrap_or_default().to_string(),
        })
    }
}

/// A container and the image it runs.
#[derive(Debug, Clone)]
pub struct ContainerInfo {
    pub name: String,
    pub image: String,
}

/// One pod row for the workload detail's PODS table.
#[derive(Debug, Clone)]
pub struct PodRow {
    pub name: String,
    /// Pod phase, with `Terminating` derived from a deletion timestamp.
    pub phase: String,
    /// Ready containers as `ready/total`.
    pub ready: String,
    /// Total container restarts.
    pub restarts: i64,
    pub age: Option<chrono::DateTime<chrono::Utc>>,
}

/// Everything the workload detail view renders.
#[derive(Debug, Clone)]
pub struct WorkloadData {
    pub kind: String,
    pub name: String,
    pub namespace: String,
    /// Overall workload health derived from the kind-specific status.
    pub ready: Option<bool>,
    /// Ordered kind-specific summary rows (Replicas, Strategy, Schedule, …).
    pub summary: Vec<(String, String)>,
    pub containers: Vec<ContainerInfo>,
    /// Pods selected by the workload's own `spec.selector` (empty for
    /// CronJobs, whose pods belong to their spawned Jobs).
    pub pods: Vec<PodRow>,
    /// The workload's events, newest first.
    pub events: Vec<crate::kube::events::KubeEventInfo>,
    /// Set when the events lookup failed (e.g. RBAC) — the view degrades.
    pub events_error: Option<String>,
}

/// Fetch a workload and everything its detail view shows. The object fetch
/// failing fails the view; pod and event lookups only degrade their sections.
pub async fn fetch_workload_data(
    client: &kube::Client,
    kind: &str,
    namespace: &str,
    name: &str,
) -> anyhow::Result<WorkloadData> {
    let obj = crate::kube::fetch_resource(client, kind, namespace, name).await?;

    let ready = extract_workload_ready(kind, &obj);
    let summary = extract_workload_summary(kind, &obj);
    let containers = extract_containers(&obj);

    let pods = match extract_selector(&obj) {
        Some(selector) => {
            let api: Api<Pod> = Api::namespaced(client.clone(), namespace);
            let list = api
                .list(&ListParams::default().labels(&selector))
                .await
                .with_context(|| format!("Failed to list pods for selector '{selector}'"))?;
            let mut rows: Vec<PodRow> = list
                .items
                .iter()
                .filter_map(|pod| {
                    serde_json::to_value(pod)
                        .ok()
                        .as_ref()
                        .and_then(pod_row_from_json)
                })
                .collect();
            rows.sort_by(|a, b| a.name.cmp(&b.name));
            rows
        }
        None => Vec::new(),
    };

    let (events, events_error) =
        match crate::kube::events::fetch_events_for_resource(client, kind, namespace, name).await {
            Ok(events) => (events, None),
            Err(e) => (Vec::new(), Some(format!("{e}"))),
        };

    Ok(WorkloadData {
        kind: kind.to_string(),
        name: name.to_string(),
        namespace: namespace.to_string(),
        ready,
        summary,
        containers,
        pods,
        events,
        events_error,
    })
}

/// Overall readiness from the kind-specific status fields.
fn extract_workload_ready(kind: &str, obj: &Value) -> Option<bool> {
    let status = obj.get("status")?;
    match kind {
        "Deployment" | "StatefulSet" => {
            let desired = obj
                .pointer("/spec/replicas")
                .and_then(Value::as_i64)
                .unwrap_or(1);
            let ready = status
                .get("readyReplicas")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            Some(ready >= desired)
        }
        "DaemonSet" => {
            let desired = status
                .get("desiredNumberScheduled")
                .and_then(Value::as_i64)?;
            let ready = status
                .get("numberReady")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            Some(ready >= desired)
        }
        // A CronJob has no readiness — it either schedules or it doesn't.
        _ => None,
    }
}

/// Ordered kind-specific summary rows for the detail view.
fn extract_workload_summary(kind: &str, obj: &Value) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    let status = obj.get("status").cloned().unwrap_or(Value::Null);
    let int = |v: &Value, ptr: &str| v.pointer(ptr).and_then(Value::as_i64).unwrap_or(0);

    match kind {
        "Deployment" => {
            let desired = obj
                .pointer("/spec/replicas")
                .and_then(Value::as_i64)
                .unwrap_or(1);
            rows.push((
                "Replicas".to_string(),
                format!(
                    "{}/{} ready, {} updated, {} available",
                    int(&status, "/readyReplicas"),
                    desired,
                    int(&status, "/updatedReplicas"),
                    int(&status, "/availableReplicas"),
                ),
            ));
            if let Some(strategy) = obj.pointer("/spec/strategy/type").and_then(Value::as_str) {
                rows.push(("Strategy".to_string(), strategy.to_string()));
            }
        }
        "StatefulSet" => {
            let desired = obj
                .pointer("/spec/replicas")
                .and_then(Value::as_i64)
                .unwrap_or(1);
            rows.push((
                "Replicas".to_string(),
                format!(
                    "{}/{} ready, {} updated",
                    int(&status, "/readyReplicas"),
                    desired,
                    int(&status, "/updatedReplicas"),
                ),
            ));
            if let Some(strategy) = obj
                .pointer("/spec/updateStrategy/type")
                .and_then(Value::as_str)
            {
                rows.push(("Update strategy".to_string(), strategy.to_string()));
            }
        }
        "DaemonSet" => {
            rows.push((
                "Scheduled".to_string(),
                format!(
                    "{}/{} ready, {} updated",
                    int(&status, "/numberReady"),
                    int(&status, "/desiredNumberScheduled"),
                    int(&status, "/updatedNumberScheduled"),
                ),
            ));
        }
        "CronJob" => {
            if let Some(schedule) = obj.pointer("/spec/schedule").and_then(Value::as_str) {
                rows.push(("Schedule".to_string(), schedule.to_string()));
            }
            if let Some(suspended) = obj.pointer("/spec/suspend").and_then(Value::as_bool) {
                rows.push(("Suspended".to_string(), suspended.to_string()));
            }
            let active = status
                .get("active")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            rows.push(("Active jobs".to_string(), active.to_string()));
            if let Some(last) = status.get("lastScheduleTime").and_then(Value::as_str) {
                rows.push(("Last schedule".to_string(), last.to_string()));
            }
        }
        _ => {}
    }
    rows
}

/// Containers and their images from the pod template (or CronJob job template).
fn extract_containers(obj: &Value) -> Vec<ContainerInfo> {
    let containers = obj
        .pointer("/spec/template/spec/containers")
        .or_else(|| obj.pointer("/spec/jobTemplate/spec/template/spec/containers"))
        .and_then(Value::as_array);
    containers.map_or_else(Vec::new, |list| {
        list.iter()
            .filter_map(|c| {
                Some(ContainerInfo {
                    name: c.get("name")?.as_str()?.to_string(),
                    image: c
                        .get("image")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                })
            })
            .collect()
    })
}

/// The workload's own pod selector as a `k=v,k=v` label selector string.
/// CronJobs have none — their pods belong to spawned Jobs.
fn extract_selector(obj: &Value) -> Option<String> {
    let labels = obj
        .pointer("/spec/selector/matchLabels")?
        .as_object()
        .filter(|map| !map.is_empty())?;
    let mut pairs: Vec<String> = labels
        .iter()
        .filter_map(|(k, v)| v.as_str().map(|v| format!("{k}={v}")))
        .collect();
    pairs.sort();
    Some(pairs.join(","))
}

/// Reduce a pod object to its table row.
fn pod_row_from_json(pod: &Value) -> Option<PodRow> {
    let name = pod.pointer("/metadata/name")?.as_str()?.to_string();

    // Terminating isn't a phase — it's a deletion timestamp.
    let phase = if pod.pointer("/metadata/deletionTimestamp").is_some() {
        "Terminating".to_string()
    } else {
        pod.pointer("/status/phase")
            .and_then(Value::as_str)
            .unwrap_or("Unknown")
            .to_string()
    };

    let statuses = pod
        .pointer("/status/containerStatuses")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let total = statuses.len().max(
        pod.pointer("/spec/containers")
            .and_then(Value::as_array)
            .map_or(0, Vec::len),
    );
    let ready_count = statuses
        .iter()
        .filter(|s| s.get("ready").and_then(Value::as_bool) == Some(true))
        .count();
    let restarts = statuses
        .iter()
        .map(|s| s.get("restartCount").and_then(Value::as_i64).unwrap_or(0))
        .sum();

    let age = pod
        .pointer("/metadata/creationTimestamp")
        .and_then(Value::as_str)
        .and_then(|ts| ts.parse::<chrono::DateTime<chrono::Utc>>().ok());

    Some(PodRow {
        name,
        phase,
        ready: format!("{ready_count}/{total}"),
        restarts,
        age,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn deployment() -> Value {
        json!({
            "apiVersion": "apps/v1",
            "kind": "Deployment",
            "metadata": {"name": "source-controller", "namespace": "flux-system"},
            "spec": {
                "replicas": 2,
                "strategy": {"type": "RollingUpdate"},
                "selector": {"matchLabels": {"app": "source-controller"}},
                "template": {"spec": {"containers": [
                    {"name": "manager", "image": "ghcr.io/fluxcd/source-controller:v1.9.3"}
                ]}}
            },
            "status": {"readyReplicas": 1, "updatedReplicas": 2, "availableReplicas": 1}
        })
    }

    #[test]
    fn deployment_summary_ready_and_selector() {
        let obj = deployment();
        assert_eq!(extract_workload_ready("Deployment", &obj), Some(false));
        let summary = extract_workload_summary("Deployment", &obj);
        assert_eq!(summary[0].0, "Replicas");
        assert_eq!(summary[0].1, "1/2 ready, 2 updated, 1 available");
        assert_eq!(
            summary[1],
            ("Strategy".to_string(), "RollingUpdate".to_string())
        );
        assert_eq!(
            extract_selector(&obj).as_deref(),
            Some("app=source-controller")
        );

        let containers = extract_containers(&obj);
        assert_eq!(containers.len(), 1);
        assert_eq!(containers[0].name, "manager");
        assert!(containers[0].image.contains("source-controller:v1.9.3"));
    }

    #[test]
    fn daemonset_and_statefulset_readiness() {
        let ds = json!({
            "kind": "DaemonSet",
            "status": {"desiredNumberScheduled": 3, "numberReady": 3, "updatedNumberScheduled": 3}
        });
        assert_eq!(extract_workload_ready("DaemonSet", &ds), Some(true));
        assert_eq!(
            extract_workload_summary("DaemonSet", &ds)[0].1,
            "3/3 ready, 3 updated"
        );

        let sts = json!({
            "kind": "StatefulSet",
            "spec": {"replicas": 3},
            "status": {"readyReplicas": 3, "updatedReplicas": 2}
        });
        assert_eq!(extract_workload_ready("StatefulSet", &sts), Some(true));
    }

    #[test]
    fn cronjob_summary_has_no_readiness_or_selector() {
        let cj = json!({
            "kind": "CronJob",
            "spec": {
                "schedule": "0 3 * * *",
                "suspend": false,
                "jobTemplate": {"spec": {"template": {"spec": {"containers": [
                    {"name": "backup", "image": "busybox:1.36"}
                ]}}}}
            },
            "status": {"active": [{}], "lastScheduleTime": "2026-07-15T03:00:00Z"}
        });
        assert_eq!(extract_workload_ready("CronJob", &cj), None);
        assert!(extract_selector(&cj).is_none());
        let summary = extract_workload_summary("CronJob", &cj);
        assert!(summary.contains(&("Schedule".to_string(), "0 3 * * *".to_string())));
        assert!(summary.contains(&("Active jobs".to_string(), "1".to_string())));
        assert_eq!(extract_containers(&cj)[0].name, "backup");
    }

    #[test]
    fn pod_row_extracts_phase_ready_restarts() {
        let pod = json!({
            "metadata": {
                "name": "source-controller-abc",
                "creationTimestamp": "2026-07-15T10:00:00Z"
            },
            "spec": {"containers": [{"name": "manager"}, {"name": "sidecar"}]},
            "status": {
                "phase": "Running",
                "containerStatuses": [
                    {"ready": true, "restartCount": 2},
                    {"ready": false, "restartCount": 5}
                ]
            }
        });
        let row = pod_row_from_json(&pod).unwrap();
        assert_eq!(row.name, "source-controller-abc");
        assert_eq!(row.phase, "Running");
        assert_eq!(row.ready, "1/2");
        assert_eq!(row.restarts, 7);
        assert!(row.age.is_some());

        // Deletion timestamp overrides phase
        let terminating = json!({
            "metadata": {"name": "p", "deletionTimestamp": "2026-07-15T10:05:00Z"},
            "status": {"phase": "Running"}
        });
        assert_eq!(
            pod_row_from_json(&terminating).unwrap().phase,
            "Terminating"
        );
    }
}
