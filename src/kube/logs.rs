//! Pod log streaming.
//!
//! The log API refuses to guess for multi-container pods ("a container name
//! must be specified"), so the stream resolves a container first, the way
//! `kubectl logs` does: the `kubectl.kubernetes.io/default-container`
//! annotation when it names a real container, else the first container.

use futures::{AsyncBufReadExt, TryStreamExt};
use k8s_openapi::api::core::v1::Pod;
use kube::Api;
use kube::api::LogParams;
use tokio::sync::mpsc::UnboundedSender;

/// Pod annotation naming the container `kubectl logs`/`exec` default to.
pub const DEFAULT_CONTAINER_ANNOTATION: &str = "kubectl.kubernetes.io/default-container";

/// Which pod (and optionally which container) to stream logs from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogRequest {
    pub namespace: String,
    pub pod: String,
    /// `None` picks the pod's default container.
    pub container: Option<String>,
}

/// Message from the log stream task to the app.
#[derive(Debug)]
pub enum LogEvent {
    /// The container being streamed and every container the pod has (for
    /// switching). Sent once, before any lines.
    Containers {
        selected: String,
        available: Vec<String>,
    },
    /// One log line.
    Line(String),
    /// The stream failed (RBAC, pod gone, container restart, …).
    Error(String),
    /// The stream ended cleanly (pod terminated).
    Ended,
}

/// The pod's regular container names, in spec order.
pub fn container_names(pod: &Pod) -> Vec<String> {
    pod.spec
        .as_ref()
        .map(|spec| spec.containers.iter().map(|c| c.name.clone()).collect())
        .unwrap_or_default()
}

/// The container `kubectl logs` would pick: the default-container annotation
/// if it names an existing container, else the first container.
pub fn default_container(pod: &Pod) -> Option<String> {
    let names = container_names(pod);
    pod.metadata
        .annotations
        .as_ref()
        .and_then(|a| a.get(DEFAULT_CONTAINER_ANNOTATION))
        .filter(|annotated| names.contains(annotated))
        .cloned()
        .or_else(|| names.into_iter().next())
}

/// Stream a pod's logs into `tx` until the stream ends or the receiver is
/// dropped (the view was closed).
pub async fn stream_pod_logs(
    client: kube::Client,
    request: LogRequest,
    tx: UnboundedSender<LogEvent>,
) {
    let api: Api<Pod> = Api::namespaced(client, &request.namespace);

    // Resolve the container up front so multi-container pods work and the
    // view can offer the others.
    let container = match api.get(&request.pod).await {
        Ok(pod) => {
            let available = container_names(&pod);
            let selected = request
                .container
                .clone()
                .filter(|c| available.contains(c))
                .or_else(|| default_container(&pod));
            if let Some(ref selected) = selected {
                let _ = tx.send(LogEvent::Containers {
                    selected: selected.clone(),
                    available,
                });
            }
            selected
        }
        Err(e) => {
            // Couldn't read the pod (RBAC on get but not logs, …): fall back
            // to whatever was asked for and let the log API decide.
            tracing::debug!(
                "Could not resolve containers for {}/{}: {}",
                request.namespace,
                request.pod,
                e
            );
            request.container.clone()
        }
    };

    tracing::debug!(
        "Streaming logs for {}/{} (container {:?})",
        request.namespace,
        request.pod,
        container
    );
    let params = LogParams {
        follow: true,
        tail_lines: Some(crate::constants::LOG_TAIL_LINES),
        container,
        ..Default::default()
    };
    match api.log_stream(&request.pod, &params).await {
        Ok(stream) => {
            let mut lines = stream.lines();
            loop {
                match lines.try_next().await {
                    Ok(Some(line)) => {
                        if tx.send(LogEvent::Line(line)).is_err() {
                            break; // View closed
                        }
                    }
                    Ok(None) => {
                        let _ = tx.send(LogEvent::Ended);
                        break;
                    }
                    Err(e) => {
                        let _ = tx.send(LogEvent::Error(e.to_string()));
                        break;
                    }
                }
            }
        }
        Err(e) => {
            tracing::warn!(
                "Failed to start log stream for {}/{}: {}",
                request.namespace,
                request.pod,
                e
            );
            let _ = tx.send(LogEvent::Error(e.to_string()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pod(annotation: Option<&str>, containers: &[&str]) -> Pod {
        let mut value = json!({
            "metadata": {"name": "p", "annotations": {}},
            "spec": {"containers": containers.iter().map(|c| json!({"name": c})).collect::<Vec<_>>()}
        });
        if let Some(a) = annotation {
            value["metadata"]["annotations"][DEFAULT_CONTAINER_ANNOTATION] = json!(a);
        }
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn single_container_is_the_default() {
        assert_eq!(
            default_container(&pod(None, &["app"])).as_deref(),
            Some("app")
        );
    }

    #[test]
    fn annotation_picks_the_default() {
        let p = pod(Some("app"), &["istio-proxy", "app"]);
        assert_eq!(default_container(&p).as_deref(), Some("app"));
        assert_eq!(container_names(&p), vec!["istio-proxy", "app"]);
    }

    #[test]
    fn unknown_annotation_falls_back_to_first() {
        let p = pod(Some("gone"), &["manager", "sidecar"]);
        assert_eq!(default_container(&p).as_deref(), Some("manager"));
    }

    #[test]
    fn no_containers_means_no_default() {
        assert_eq!(default_container(&pod(None, &[])), None);
    }
}
