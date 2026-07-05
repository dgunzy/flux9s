//! Kubernetes Events (core/v1) support.
//!
//! One shared model backs both event surfaces: the live events view (fed by
//! the watcher) and the Events section of the describe view (fetched on
//! demand for one resource). Flux controllers emit these events through the
//! standard event recorder, so they carry the reconciliation error detail
//! that `status.conditions` truncates.

use anyhow::Context;
use k8s_openapi::api::core::v1::Event as CoreEvent;
use kube::Api;
use kube::api::ListParams;

/// A Kubernetes Event reduced to the fields flux9s displays.
#[derive(Debug, Clone)]
pub struct KubeEventInfo {
    /// Object UID — the dedup key for the live event store.
    pub uid: String,
    /// "Normal" or "Warning".
    pub event_type: String,
    /// Machine-readable reason (e.g. "ReconciliationSucceeded").
    pub reason: String,
    /// Human-readable message.
    pub message: String,
    /// Kind of the object this event is about.
    pub involved_kind: String,
    /// Namespace of the involved object (falls back to the event's namespace).
    pub involved_namespace: String,
    /// Name of the involved object.
    pub involved_name: String,
    /// Number of occurrences (deduplicated events aggregate here).
    pub count: i64,
    /// Most recent occurrence.
    pub last_seen: Option<chrono::DateTime<chrono::Utc>>,
    /// Reporting component (e.g. "kustomize-controller").
    pub source: String,
}

impl KubeEventInfo {
    /// Whether this is a Warning event (drives error styling).
    pub fn is_warning(&self) -> bool {
        self.event_type == "Warning"
    }

    /// `Kind/name` label for the OBJECT column.
    pub fn object_label(&self) -> String {
        format!("{}/{}", self.involved_kind, self.involved_name)
    }

    /// Parse from a core/v1 Event JSON object.
    ///
    /// Returns `None` when the UID is missing (never the case for real API
    /// objects). Absent display fields degrade to empty strings so a sparse
    /// event still renders.
    pub fn from_json(event_json: &serde_json::Value) -> Option<Self> {
        let uid = event_json["metadata"]["uid"].as_str()?.to_string();
        let str_field = |value: &serde_json::Value| value.as_str().unwrap_or_default().to_string();

        let involved = &event_json["involvedObject"];
        let involved_namespace = involved["namespace"]
            .as_str()
            .or(event_json["metadata"]["namespace"].as_str())
            .unwrap_or_default()
            .to_string();

        // Events created through the newer events.k8s.io API and mirrored into
        // core/v1 carry their occurrence data in different fields.
        let last_seen = ["lastTimestamp", "eventTime"]
            .iter()
            .map(|field| &event_json[field])
            .chain([
                &event_json["series"]["lastObservedTime"],
                &event_json["metadata"]["creationTimestamp"],
            ])
            .find_map(|value| {
                value
                    .as_str()
                    .and_then(|ts| ts.parse::<chrono::DateTime<chrono::Utc>>().ok())
            });
        let count = event_json["count"]
            .as_i64()
            .or(event_json["series"]["count"].as_i64())
            .unwrap_or(1);
        let source = event_json["source"]["component"]
            .as_str()
            .or(event_json["reportingComponent"].as_str())
            .unwrap_or_default()
            .to_string();

        Some(Self {
            uid,
            event_type: str_field(&event_json["type"]),
            reason: str_field(&event_json["reason"]),
            message: str_field(&event_json["message"]),
            involved_kind: str_field(&involved["kind"]),
            involved_namespace,
            involved_name: str_field(&involved["name"]),
            count,
            last_seen,
            source,
        })
    }
}

/// Fetch the Events for one resource, newest first — the same query
/// `kubectl describe` runs. Returns an empty list when the resource has no
/// events; errors (e.g. RBAC) are surfaced so the caller can degrade.
pub async fn fetch_events_for_resource(
    client: &kube::Client,
    kind: &str,
    namespace: &str,
    name: &str,
) -> anyhow::Result<Vec<KubeEventInfo>> {
    let api: Api<CoreEvent> = Api::namespaced(client.clone(), namespace);
    let field_selector = format!(
        "involvedObject.kind={},involvedObject.name={},involvedObject.namespace={}",
        kind, name, namespace
    );
    let params = ListParams::default().fields(&field_selector);
    let events = api.list(&params).await.with_context(|| {
        format!(
            "Failed to list events for {}/{} in namespace {}",
            kind, name, namespace
        )
    })?;

    let mut infos: Vec<KubeEventInfo> = events
        .items
        .iter()
        .filter_map(|event| {
            serde_json::to_value(event)
                .ok()
                .as_ref()
                .and_then(KubeEventInfo::from_json)
        })
        .collect();
    infos.sort_by_key(|info| std::cmp::Reverse(info.last_seen));
    Ok(infos)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_event() -> serde_json::Value {
        serde_json::json!({
            "metadata": {
                "name": "podinfo.17c1e8a",
                "namespace": "flux-system",
                "uid": "aaaa-bbbb",
                "creationTimestamp": "2026-07-01T10:00:00Z"
            },
            "involvedObject": {
                "kind": "Kustomization",
                "namespace": "flux-system",
                "name": "podinfo"
            },
            "type": "Warning",
            "reason": "ReconciliationFailed",
            "message": "kustomization path not found",
            "count": 4,
            "lastTimestamp": "2026-07-01T12:30:00Z",
            "source": {"component": "kustomize-controller"}
        })
    }

    #[test]
    fn parses_core_v1_event() {
        let info = KubeEventInfo::from_json(&sample_event()).expect("event should parse");
        assert_eq!(info.uid, "aaaa-bbbb");
        assert!(info.is_warning());
        assert_eq!(info.reason, "ReconciliationFailed");
        assert_eq!(info.object_label(), "Kustomization/podinfo");
        assert_eq!(info.involved_namespace, "flux-system");
        assert_eq!(info.count, 4);
        assert_eq!(info.source, "kustomize-controller");
        assert_eq!(
            info.last_seen.unwrap().to_rfc3339(),
            "2026-07-01T12:30:00+00:00"
        );
    }

    #[test]
    fn parses_events_k8s_io_style_fields() {
        // Events mirrored from events.k8s.io/v1: no lastTimestamp/count,
        // occurrence data lives in eventTime/series, source in reportingComponent.
        let event = serde_json::json!({
            "metadata": {"uid": "cccc", "namespace": "default"},
            "involvedObject": {"kind": "HelmRelease", "name": "podinfo"},
            "type": "Normal",
            "reason": "UpgradeSucceeded",
            "message": "Helm upgrade succeeded",
            "eventTime": "2026-07-02T08:00:00.000000Z",
            "series": {"count": 7, "lastObservedTime": "2026-07-02T09:00:00.000000Z"},
            "reportingComponent": "helm-controller"
        });
        let info = KubeEventInfo::from_json(&event).expect("event should parse");
        assert!(!info.is_warning());
        assert_eq!(info.count, 7);
        assert_eq!(info.source, "helm-controller");
        // eventTime is preferred over series.lastObservedTime
        assert_eq!(
            info.last_seen.unwrap().to_rfc3339(),
            "2026-07-02T08:00:00+00:00"
        );
        // involvedObject.namespace falls back to the event's namespace
        assert_eq!(info.involved_namespace, "default");
    }

    #[test]
    fn missing_uid_returns_none() {
        assert!(KubeEventInfo::from_json(&serde_json::json!({"metadata": {}})).is_none());
    }

    #[test]
    fn sparse_event_degrades_to_empty_fields() {
        let info = KubeEventInfo::from_json(&serde_json::json!({
            "metadata": {"uid": "dddd"}
        }))
        .expect("uid alone is enough");
        assert_eq!(info.event_type, "");
        assert_eq!(info.count, 1);
        assert!(info.last_seen.is_none());
    }
}
