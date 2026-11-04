//! Curated Pod and Node columns and view-scoped usage tasks.
use std::collections::HashMap;

use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;

use super::kind_list::{KindListRequest, KindListSnapshot};
use super::metrics::{self, MetricsSnapshot, MetricsSourceSetting, PodKey, PodUsage};
use crate::models::kinds::{BuiltinColumn, Column, Gvk};

/// Computed fields for core Pods and Nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeColumn {
    Ready,
    Phase,
    Restarts,
    Cpu,
    Memory,
    Node,
    Roles,
    Pods,
    Version,
}

impl NativeColumn {
    pub fn header(self) -> &'static str {
        match self {
            Self::Ready => "READY",
            Self::Phase => "STATUS",
            Self::Restarts => "RESTARTS",
            Self::Cpu => "CPU",
            Self::Memory => "MEM",
            Self::Node => "NODE",
            Self::Roles => "ROLES",
            Self::Pods => "PODS",
            Self::Version => "VERSION",
        }
    }
    /// Initial values from watch data; usage replaces CPU/MEM asynchronously.
    pub fn cell(self, obj: &Value) -> String {
        let text = |path| {
            obj.pointer(path)
                .and_then(Value::as_str)
                .unwrap_or("-")
                .to_string()
        };
        let statuses = obj
            .pointer("/status/containerStatuses")
            .and_then(Value::as_array);
        match self {
            Self::Ready => super::object_status::compute_object_status(obj)
                .ready
                .unwrap_or_else(|| "-".into()),
            Self::Phase => super::object_status::compute_object_status(obj)
                .label()
                .to_string(),
            Self::Restarts => statuses
                .into_iter()
                .flatten()
                .map(|s| s.get("restartCount").and_then(Value::as_u64).unwrap_or(0))
                .sum::<u64>()
                .to_string(),
            Self::Node => text("/spec/nodeName"),
            Self::Roles => {
                let roles: Vec<_> = obj
                    .pointer("/metadata/labels")
                    .and_then(Value::as_object)
                    .into_iter()
                    .flatten()
                    .filter_map(|(key, _)| key.strip_prefix("node-role.kubernetes.io/"))
                    .collect();
                if roles.is_empty() {
                    "-".into()
                } else {
                    roles.join(",")
                }
            }
            Self::Version => text("/status/nodeInfo/kubeletVersion"),
            Self::Pods => "-".into(),
            Self::Cpu | Self::Memory => {
                let cpu = self == Self::Cpu;
                if obj.get("kind").and_then(Value::as_str) == Some("Node") {
                    let value = obj
                        .pointer(if cpu {
                            "/status/allocatable/cpu"
                        } else {
                            "/status/allocatable/memory"
                        })
                        .and_then(Value::as_str)
                        .and_then(if cpu {
                            metrics::parse_cpu_millicores
                        } else {
                            metrics::parse_memory_bytes
                        });
                    return format!("use - / req - / cap {}", quantity(value, cpu));
                }
                let resources = metrics::pod_resources(obj);
                let (request, limit) = if cpu {
                    (resources.cpu_request, resources.cpu_limit)
                } else {
                    (resources.memory_request, resources.memory_limit)
                };
                format!(
                    "use - / req {} / lim {}",
                    quantity(request, cpu),
                    quantity(limit, cpu)
                )
            }
        }
    }
}
fn quantity(value: Option<f64>, cpu: bool) -> String {
    value
        .map(if cpu {
            metrics::format_cpu
        } else {
            metrics::format_memory
        })
        .unwrap_or_else(|| "-".into())
}

/// Curated columns only for core Pod and Node kinds.
pub fn columns(gvk: &Gvk) -> Option<Vec<Column>> {
    use NativeColumn::*;
    if !gvk.group.is_empty() {
        return None;
    }
    let mut columns = match gvk.kind.as_str() {
        "Pod" => vec![Ready, Phase, Restarts, Cpu, Memory, Node]
            .into_iter()
            .map(Column::Native)
            .collect::<Vec<_>>(),
        "Node" => {
            let mut columns = vec![Column::Builtin(BuiltinColumn::Status)];
            columns.extend(
                [Roles, Cpu, Memory, Pods, Version]
                    .into_iter()
                    .map(Column::Native),
            );
            columns
        }
        _ => return None,
    };
    columns.extend([
        Column::Builtin(BuiltinColumn::ManagedBy),
        Column::Builtin(BuiltinColumn::Age),
    ]);
    Some(columns)
}

fn apply_usage(
    snapshot: &mut KindListSnapshot,
    pods: &HashMap<PodKey, PodUsage>,
    nodes: &HashMap<String, metrics::NodeUsage>,
    is_node: bool,
) {
    for row in &mut snapshot.rows {
        let usage = if is_node {
            nodes.get(&row.name).map(|n| &n.usage)
        } else {
            pods.get(&PodKey::new(&row.namespace, &row.name))
        };
        for (index, column) in snapshot.columns.iter().enumerate() {
            let Some(cell) = row.cells.get_mut(index) else {
                continue;
            };
            if *column == Column::Native(NativeColumn::Pods)
                && let Some(count) = nodes.get(&row.name).and_then(|n| n.pods)
            {
                *cell = count.to_string();
            }
            let cpu = *column == Column::Native(NativeColumn::Cpu);
            if cpu || *column == Column::Native(NativeColumn::Memory) {
                let value = usage.and_then(|u| {
                    if cpu {
                        u.cpu_millicores
                    } else {
                        u.memory_bytes
                    }
                });
                if let Some(value) = value {
                    let used = quantity(Some(value), cpu);
                    let marker = if is_node && nodes.get(&row.name).is_some_and(|n| n.requests) {
                        "req "
                    } else {
                        "use "
                    };
                    if let Some(start) = cell.find(marker) {
                        let start = start + marker.len();
                        let end = cell[start..]
                            .find(" / ")
                            .map_or(cell.len(), |end| start + end);
                        cell.replace_range(start..end, &used);
                    }
                }
            }
        }
    }
}

/// Run a kind watch and its metrics tasks together. Dropping this future
/// aborts every child, including when navigating away or switching namespace.
pub async fn watch_native(
    client: kube::Client,
    request: KindListRequest,
    setting: MetricsSourceSetting,
    tx: UnboundedSender<anyhow::Result<KindListSnapshot>>,
) {
    if columns(&request.spec.gvk).is_none() {
        super::kind_list::watch_kind(client, request, tx).await;
        return;
    }
    let is_node = request.spec.gvk.is_core("Node");
    let namespace = request.namespace.clone();
    let mut tasks = tokio::task::JoinSet::new();
    let (list_tx, mut list_rx) = tokio::sync::mpsc::unbounded_channel();
    tasks.spawn(super::kind_list::watch_kind(
        client.clone(),
        request,
        list_tx,
    ));
    let (usage_tx, mut usage_rx) = tokio::sync::mpsc::unbounded_channel::<MetricsSnapshot>();
    let (node_tx, mut node_rx) = tokio::sync::mpsc::unbounded_channel();
    if is_node {
        tasks.spawn(metrics::watch_node_metrics(
            client.clone(),
            setting,
            node_tx,
        ));
    }
    let (targets_tx, targets_rx) = tokio::sync::watch::channel(HashMap::new());
    if !is_node {
        tasks.spawn(metrics::watch_list_metrics(
            client.clone(),
            namespace,
            setting,
            targets_rx,
            usage_tx,
        ));
    }
    let mut snapshot = None;
    let mut pods = HashMap::new();
    let mut nodes = HashMap::new();
    loop {
        tokio::select! {
            _ = tx.closed() => return,
            result = list_rx.recv() => {
                let Some(result) = result else { return };
                match result {
                    Err(error) => { if tx.send(Err(error)).is_err() { return; } continue; }
                    Ok(next) => {
                        if !is_node {
                            let node_column = next.columns.iter().position(|c| *c == Column::Native(NativeColumn::Node));
                            let targets = next.rows.iter().map(|row| {
                                let node = node_column.and_then(|i| row.cells.get(i)).filter(|n| n.as_str() != "-").cloned().unwrap_or_default();
                                (PodKey::new(&row.namespace, &row.name), node)
                            }).collect();
                            targets_tx.send_replace(targets);
                        }
                        snapshot = Some(next);
                    }
                }
            }
            Some(usage) = usage_rx.recv() => pods = usage.pods,
            Some(usage) = node_rx.recv(), if is_node => nodes = usage,
        }
        // Reap completed tasks so a long-lived view does not accumulate them.
        while tasks.try_join_next().is_some() {}
        if let Some(base) = &snapshot {
            let mut display = base.clone();
            apply_usage(&mut display, &pods, &nodes, is_node);
            if tx.send(Ok(display)).is_err() {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pods_show_ready_restarts_waiting_reason_and_explicit_fallbacks() {
        let pod = json!({"kind": "Pod", "spec": {"containers": [{"resources": {"requests": {"cpu": "100m"}}}], "nodeName": "worker"}, "status": {"phase": "Running", "containerStatuses": [{"ready": false, "restartCount": 3, "state": {"waiting": {"reason": "CrashLoopBackOff"}}}]}});
        assert_eq!(NativeColumn::Ready.cell(&pod), "0/1");
        assert_eq!(NativeColumn::Restarts.cell(&pod), "3");
        assert_eq!(NativeColumn::Phase.cell(&pod), "CrashLoopBackOff");
        assert_eq!(NativeColumn::Node.cell(&pod), "worker");
        assert!(
            NativeColumn::Cpu
                .cell(&pod)
                .starts_with("use - / req 100m / lim -")
        );
    }

    #[test]
    fn usage_matches_both_namespace_and_name_and_missing_usage_keeps_requests() {
        let columns = vec![Column::Native(NativeColumn::Cpu)];
        let row = |ns| {
            super::super::kind_list::kind_row(&columns, &json!({"kind":"Pod", "metadata":{"name":"web", "namespace":ns}, "spec":{"containers":[{"resources":{"requests":{"cpu":"100m"}}}]}}), chrono::Utc::now()).unwrap()
        };
        let mut snapshot = KindListSnapshot {
            columns: columns.clone(),
            rows: vec![row("a"), row("b"), row("c")],
            total: 3,
        };
        let usage = [
            (
                PodKey::new("a", "web"),
                PodUsage {
                    cpu_millicores: Some(10.0),
                    memory_bytes: None,
                },
            ),
            (
                PodKey::new("b", "web"),
                PodUsage {
                    cpu_millicores: Some(20.0),
                    memory_bytes: None,
                },
            ),
        ]
        .into();
        apply_usage(&mut snapshot, &usage, &HashMap::new(), false);
        assert_eq!(snapshot.rows[0].cells[0], "use 10m / req 100m / lim -");
        assert_eq!(snapshot.rows[1].cells[0], "use 20m / req 100m / lim -");
        assert!(snapshot.rows[2].cells[0].starts_with("use - / req 100m"));
    }

    #[test]
    fn node_usage_preserves_allocatable_and_labels_requests() {
        let columns = vec![
            Column::Native(NativeColumn::Cpu),
            Column::Native(NativeColumn::Pods),
        ];
        let row = super::super::kind_list::kind_row(&columns, &json!({"kind":"Node", "metadata":{"name":"worker"}, "status":{"allocatable":{"cpu":"2"}}}), chrono::Utc::now()).unwrap();
        let mut snapshot = KindListSnapshot {
            columns,
            rows: vec![row],
            total: 1,
        };
        let nodes = [(
            "worker".into(),
            metrics::NodeUsage {
                usage: PodUsage {
                    cpu_millicores: Some(100.0),
                    memory_bytes: None,
                },
                pods: Some(5),
                requests: true,
            },
        )]
        .into();
        apply_usage(&mut snapshot, &HashMap::new(), &nodes, true);
        assert_eq!(
            snapshot.rows[0].cells[0],
            format!("use - / req 100m / cap {}", metrics::format_cpu(2000.0))
        );
        assert_eq!(snapshot.rows[0].cells[1], "5");
    }
}
