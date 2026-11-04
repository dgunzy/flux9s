//! Pod CPU/memory usage (#265).
//!
//! Sources are tried in the same order as ktop, so clusters without
//! metrics-server still get live usage:
//!
//! 1. **Kubelet** — each node's resource metrics
//!    (`/api/v1/nodes/{node}/proxy/metrics/resource`, falling back to
//!    `/metrics/cadvisor`), read through the API server. Needs `get
//!    nodes/proxy`, checked with a SelfSubjectAccessReview. CPU is the rate of
//!    `container_cpu_usage_seconds_total` between two scrapes.
//! 2. **metrics-server** — `metrics.k8s.io` PodMetrics.
//! 3. **None** — no usage; views show requests/limits instead.
//!
//! A source that fails at runtime drops to the next one rather than erroring.
//!
//! Neither source can be watched — `metrics.k8s.io` serves only `get`/`list`,
//! and kubelet metrics are pull-only scrape endpoints — so usage is polled.
//! Both refresh their data about every 15s (kubelet housekeeping,
//! metrics-server's default resolution), which sets the poll interval;
//! polling faster only re-reads identical samples. Failures back off
//! exponentially to [`MAX_BACKOFF`]. Like ktop, each request is capped at
//! [`REQUEST_TIMEOUT`].
//!
//! **Every view that shows usage hydrates on open.** The first round runs
//! immediately, and for the kubelet source it also reads the node summary
//! (`/stats/summary`), whose `usageNanoCores` is an instantaneous rate, so
//! CPU doesn't wait ~30s for the cumulative counter to move. That one call
//! seeds the tracker; later rounds only poll `/metrics/resource`. Views
//! (workload detail today, `:pods`/`:deploy` later) all go through
//! [`watch_pod_metrics`] and get this behaviour for free.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::time::Duration;

use anyhow::Context;
use k8s_openapi::api::authorization::v1::{
    ResourceAttributes, SelfSubjectAccessReview, SelfSubjectAccessReviewSpec,
};
use k8s_openapi::api::core::v1::Pod;
use kube::Api;
use kube::api::{ListParams, PostParams};
use kube::core::DynamicObject;
use kube::discovery::ApiResource;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;

/// Time between polls — the sources' own refresh resolution.
const POLL_INTERVAL: Duration = Duration::from_secs(15);
/// Ceiling for the failure backoff.
const MAX_BACKOFF: Duration = Duration::from_secs(120);
/// Per-request timeout (as ktop).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Delay before the next poll: the interval, doubled per consecutive failed
/// round, capped at [`MAX_BACKOFF`].
fn poll_delay(consecutive_failures: u32) -> Duration {
    POLL_INTERVAL
        .saturating_mul(2u32.saturating_pow(consecutive_failures.min(8)))
        .min(MAX_BACKOFF)
}

/// Run a request with [`REQUEST_TIMEOUT`].
async fn with_timeout<T>(
    what: &str,
    future: impl std::future::Future<Output = anyhow::Result<T>>,
) -> anyhow::Result<T> {
    tokio::time::timeout(REQUEST_TIMEOUT, future)
        .await
        .map_err(|_| anyhow::anyhow!("{what} timed out after {}s", REQUEST_TIMEOUT.as_secs()))?
}

/// Configured metrics source (`metricsSource` in the config file).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MetricsSourceSetting {
    /// Kubelet if `nodes/proxy` is allowed, else metrics-server, else none.
    #[default]
    Auto,
    Kubelet,
    MetricsServer,
    None,
}

impl MetricsSourceSetting {
    /// Parse the config value (`auto`, `kubelet`, `metrics-server`, `none`).
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "auto" => Some(Self::Auto),
            "kubelet" => Some(Self::Kubelet),
            "metrics-server" => Some(Self::MetricsServer),
            "none" => Some(Self::None),
            _ => None,
        }
    }
}

/// The source actually in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricsSource {
    Kubelet,
    MetricsServer,
    None,
}

impl fmt::Display for MetricsSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            MetricsSource::Kubelet => "kubelet",
            MetricsSource::MetricsServer => "metrics-server",
            MetricsSource::None => "none",
        })
    }
}

/// One pod's usage. CPU is `None` until two kubelet samples exist.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PodUsage {
    pub cpu_millicores: Option<f64>,
    pub memory_bytes: Option<f64>,
}

/// Identity of a pod in a metrics snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PodKey {
    pub namespace: String,
    pub name: String,
}

impl PodKey {
    pub fn new(namespace: &str, name: &str) -> Self {
        Self {
            namespace: namespace.into(),
            name: name.into(),
        }
    }
}

/// Usage for the pods of one view, from one source.
#[derive(Debug, Clone, PartialEq)]
pub struct MetricsSnapshot {
    pub source: MetricsSource,
    /// Namespace and name keep identically named pods distinct.
    pub pods: HashMap<PodKey, PodUsage>,
}

/// What a live usage task watches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricsRequest {
    pub namespace: String,
    /// Label selector of the pods to report on.
    pub selector: String,
    pub setting: MetricsSourceSetting,
}

// ---------------------------------------------------------------------------
// Quantities
// ---------------------------------------------------------------------------

/// Parse a CPU quantity (`250m`, `1`, `0.5`, `123456n`, `10u`) into millicores.
pub fn parse_cpu_millicores(quantity: &str) -> Option<f64> {
    let q = quantity.trim();
    let (number, scale) = match q.char_indices().last()? {
        (i, 'n') => (&q[..i], 1e-6),
        (i, 'u') => (&q[..i], 1e-3),
        (i, 'm') => (&q[..i], 1.0),
        _ => (q, 1000.0),
    };
    number.parse::<f64>().ok().map(|n| n * scale)
}

/// Parse a memory quantity (`128Mi`, `1Gi`, `500M`, `1e3`, plain bytes).
pub fn parse_memory_bytes(quantity: &str) -> Option<f64> {
    const SUFFIXES: &[(&str, f64)] = &[
        ("Ki", 1024.0),
        ("Mi", 1024.0 * 1024.0),
        ("Gi", 1024.0 * 1024.0 * 1024.0),
        ("Ti", 1024.0 * 1024.0 * 1024.0 * 1024.0),
        ("Pi", 1024.0 * 1024.0 * 1024.0 * 1024.0 * 1024.0),
        ("k", 1e3),
        ("M", 1e6),
        ("G", 1e9),
        ("T", 1e12),
        ("P", 1e15),
        ("m", 1e-3),
    ];
    let q = quantity.trim();
    for (suffix, factor) in SUFFIXES {
        if let Some(number) = q.strip_suffix(suffix) {
            return number.parse::<f64>().ok().map(|n| n * factor);
        }
    }
    q.parse::<f64>().ok()
}

/// Format millicores the way kubectl does (`250m`, `1.5`).
pub fn format_cpu(millicores: f64) -> String {
    if millicores >= 1000.0 {
        let cores = millicores / 1000.0;
        if (cores - cores.round()).abs() < 0.05 {
            format!("{}", cores.round())
        } else {
            format!("{cores:.1}")
        }
    } else {
        format!("{}m", millicores.round())
    }
}

/// Format bytes with binary units (`45Mi`, `1.2Gi`).
pub fn format_memory(bytes: f64) -> String {
    const UNITS: &[(&str, f64)] = &[
        ("Gi", 1024.0 * 1024.0 * 1024.0),
        ("Mi", 1024.0 * 1024.0),
        ("Ki", 1024.0),
    ];
    for (unit, size) in UNITS {
        if bytes >= *size {
            let value = bytes / size;
            // One decimal for small Gi values, but never a trailing `.0`.
            return if value >= 10.0 || *unit != "Gi" || (value - value.round()).abs() < 0.05 {
                format!("{}{unit}", value.round())
            } else {
                format!("{value:.1}{unit}")
            };
        }
    }
    format!("{}", bytes.round())
}

// ---------------------------------------------------------------------------
// Prometheus text format
// ---------------------------------------------------------------------------

/// One sample: metric name, labels, value, and optional timestamp (ms).
#[derive(Debug, Clone, PartialEq)]
struct Sample {
    name: String,
    labels: HashMap<String, String>,
    value: f64,
    timestamp_ms: Option<f64>,
}

/// Parse one exposition-format line. Comments and blank lines give `None`.
fn parse_sample(line: &str) -> Option<Sample> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let i = line.find(['{', ' '])?;
    let (name, rest) = (&line[..i], &line[i..]);
    let mut labels = HashMap::new();
    let rest = if let Some(body) = rest.strip_prefix('{') {
        // Walk key="value" pairs, honouring \" escapes inside values.
        let mut chars = body.char_indices().peekable();
        let mut end = None;
        let mut key = String::new();
        while let Some((i, c)) = chars.next() {
            match c {
                '}' => {
                    end = Some(i);
                    break;
                }
                ',' | ' ' => {}
                '=' => {
                    if chars.next().map(|(_, q)| q) != Some('"') {
                        return None;
                    }
                    let mut value = String::new();
                    let mut escaped = false;
                    for (_, v) in chars.by_ref() {
                        match v {
                            _ if escaped => {
                                value.push(v);
                                escaped = false;
                            }
                            '\\' => escaped = true,
                            '"' => break,
                            _ => value.push(v),
                        }
                    }
                    labels.insert(std::mem::take(&mut key), value);
                }
                _ => key.push(c),
            }
        }
        body.get(end? + 1..)?
    } else {
        rest
    };
    let mut fields = rest.split_whitespace();
    let value = fields.next()?.parse::<f64>().ok()?;
    let timestamp_ms = fields.next().and_then(|t| t.parse::<f64>().ok());
    Some(Sample {
        name: name.to_string(),
        labels,
        value,
        timestamp_ms,
    })
}

/// Per-pod totals from one scrape: summed container CPU seconds (with the
/// sample time) and working-set memory.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct PodTotals {
    cpu_seconds: Option<f64>,
    cpu_timestamp_ms: Option<f64>,
    memory_bytes: Option<f64>,
}

/// Sum container samples for the wanted pods. Works for both
/// `/metrics/resource` and `/metrics/cadvisor` (whose pod-level `container=""`
/// and sandbox `container="POD"` series are skipped).
fn pod_totals(text: &str, namespace: &str, pods: &HashSet<String>) -> HashMap<String, PodTotals> {
    let wanted = pods
        .iter()
        .map(|name| PodKey::new(namespace, name))
        .collect();
    all_pod_totals(text, &wanted)
        .into_iter()
        .map(|(key, total)| (key.name, total))
        .collect()
}

fn all_pod_totals(text: &str, pods: &HashSet<PodKey>) -> HashMap<PodKey, PodTotals> {
    let mut totals: HashMap<PodKey, PodTotals> = HashMap::new();
    for sample in text.lines().filter_map(parse_sample) {
        let is_cpu = sample.name == "container_cpu_usage_seconds_total";
        let is_memory = sample.name == "container_memory_working_set_bytes";
        if !is_cpu && !is_memory {
            continue;
        }
        let label = |k: &str| sample.labels.get(k).map(String::as_str).unwrap_or_default();
        let container = label("container");
        let key = PodKey::new(label("namespace"), label("pod"));
        if !pods.contains(&key) || container.is_empty() || container == "POD" {
            continue;
        }
        let entry = totals.entry(key).or_default();
        if is_cpu {
            *entry.cpu_seconds.get_or_insert(0.0) += sample.value;
            entry.cpu_timestamp_ms = match (entry.cpu_timestamp_ms, sample.timestamp_ms) {
                (Some(a), Some(b)) => Some(a.max(b)),
                (a, b) => a.or(b),
            };
        } else {
            *entry.memory_bytes.get_or_insert(0.0) += sample.value;
        }
    }
    totals
}

/// Per-pod CPU state between scrapes. The kubelet only refreshes its
/// counters every ~15s, so a scrape often returns the very same sample: the
/// baseline is kept until the sample's timestamp advances, and the last rate
/// is reported in between.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct CpuTracker {
    baseline: Option<(f64, f64)>,
    rate: Option<f64>,
}

impl CpuTracker {
    /// Feed a `(cpu_seconds, timestamp_ms)` sample; returns the current rate.
    fn observe(&mut self, sample: (f64, f64)) -> Option<f64> {
        match self.baseline {
            Some(baseline) if sample.1 > baseline.1 => {
                // Counter reset (restart): start over from this sample.
                self.rate = cpu_rate(baseline, sample);
                self.baseline = Some(sample);
            }
            Some(_) => {} // Same (or older) sample: keep the last rate.
            None => self.baseline = Some(sample),
        }
        self.rate
    }

    /// Adopt an instantaneous rate (from the node summary) until the
    /// counters produce one of their own.
    fn seed(&mut self, rate: f64) {
        if self.rate.is_none() {
            self.rate = Some(rate);
        }
    }
}

/// CPU millicores from two cumulative samples; `None` on a counter reset
/// (container restart) or a non-advancing clock.
fn cpu_rate(previous: (f64, f64), current: (f64, f64)) -> Option<f64> {
    let (prev_cpu, prev_ms) = previous;
    let (cur_cpu, cur_ms) = current;
    let elapsed = (cur_ms - prev_ms) / 1000.0;
    let used = cur_cpu - prev_cpu;
    (elapsed > 0.0 && used >= 0.0).then(|| used / elapsed * 1000.0)
}

// ---------------------------------------------------------------------------
// Sources
// ---------------------------------------------------------------------------

/// Whether the current identity may `get nodes/proxy`.
async fn can_proxy_nodes(client: &kube::Client) -> bool {
    let review = SelfSubjectAccessReview {
        spec: SelfSubjectAccessReviewSpec {
            resource_attributes: Some(ResourceAttributes {
                verb: Some("get".to_string()),
                resource: Some("nodes".to_string()),
                subresource: Some("proxy".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    match Api::<SelfSubjectAccessReview>::all(client.clone())
        .create(&PostParams::default(), &review)
        .await
    {
        Ok(result) => result.status.is_some_and(|s| s.allowed),
        Err(e) => {
            tracing::debug!("nodes/proxy access review failed: {e}");
            false
        }
    }
}

/// Whether the `metrics.k8s.io` API is served.
async fn metrics_server_available(client: &kube::Client) -> bool {
    kube::discovery::group(client, "metrics.k8s.io")
        .await
        .is_ok()
}

/// The source to fall back to when `failed` stops working. Never errors:
/// the chain always ends at `None` (requests/limits only).
fn fallback_from(failed: MetricsSource, metrics_server_served: bool) -> MetricsSource {
    match failed {
        MetricsSource::Kubelet if metrics_server_served => MetricsSource::MetricsServer,
        _ => MetricsSource::None,
    }
}

/// Resolve a setting to the source to start with. In `auto` every probe
/// fails closed — a denied, errored, or forbidden check just moves on — so
/// users without `nodes/proxy` (common) or metrics-server silently get the
/// next source.
pub async fn detect_source(client: &kube::Client, setting: MetricsSourceSetting) -> MetricsSource {
    match setting {
        MetricsSourceSetting::Kubelet => MetricsSource::Kubelet,
        MetricsSourceSetting::MetricsServer => MetricsSource::MetricsServer,
        MetricsSourceSetting::None => MetricsSource::None,
        MetricsSourceSetting::Auto => {
            if can_proxy_nodes(client).await {
                MetricsSource::Kubelet
            } else if metrics_server_available(client).await {
                MetricsSource::MetricsServer
            } else {
                MetricsSource::None
            }
        }
    }
}

/// GET a path on a node's kubelet through the API server proxy.
async fn kubelet_get(client: &kube::Client, node: &str, path: &str) -> anyhow::Result<String> {
    let request = http::Request::get(format!("/api/v1/nodes/{node}/proxy{path}"))
        .body(Vec::new())
        .context("Failed to build kubelet request")?;
    client
        .request_text(request)
        .await
        .with_context(|| format!("Failed to read {path} from node {node}"))
}

/// Scrape one node: `/metrics/resource`, or `/metrics/cadvisor` on kubelets
/// that don't serve it.
async fn scrape_node(client: &kube::Client, node: &str) -> anyhow::Result<String> {
    match kubelet_get(client, node, "/metrics/resource").await {
        Ok(text) if text.contains("container_cpu_usage_seconds_total") => Ok(text),
        _ => kubelet_get(client, node, "/metrics/cadvisor").await,
    }
}

/// Usage from the kubelets hosting `pods` (name → node). `previous` carries
/// CPU counters between calls to compute rates.
async fn kubelet_usage(
    client: &kube::Client,
    namespace: &str,
    pods: &[(String, Option<String>)],
    trackers: &mut HashMap<String, CpuTracker>,
) -> anyhow::Result<HashMap<String, PodUsage>> {
    let names: HashSet<String> = pods.iter().map(|(name, _)| name.clone()).collect();
    let nodes: HashSet<&str> = pods
        .iter()
        .filter_map(|(_, node)| node.as_deref())
        .collect();

    let mut totals: HashMap<String, PodTotals> = HashMap::new();
    let mut failures = Vec::new();
    for node in &nodes {
        match with_timeout("kubelet scrape", scrape_node(client, node)).await {
            Ok(text) => totals.extend(pod_totals(&text, namespace, &names)),
            Err(e) => failures.push(format!("{e:#}")),
        }
    }
    if !nodes.is_empty() && failures.len() == nodes.len() {
        anyhow::bail!("kubelet scrape failed: {}", failures.join("; "));
    }

    trackers.retain(|pod, _| names.contains(pod));
    let now_ms = chrono::Utc::now().timestamp_millis() as f64;
    let mut usage = HashMap::new();
    for (pod, total) in totals {
        let cpu_millicores = total.cpu_seconds.and_then(|cpu| {
            let sample = (cpu, total.cpu_timestamp_ms.unwrap_or(now_ms));
            trackers.entry(pod.clone()).or_default().observe(sample)
        });
        usage.insert(
            pod,
            PodUsage {
                cpu_millicores,
                memory_bytes: total.memory_bytes,
            },
        );
    }
    Ok(usage)
}

/// Parse a kubelet `/stats/summary` document into per-pod usage for the
/// wanted pods. CPU comes from `usageNanoCores` (already a rate).
fn summary_usage(
    summary: &Value,
    namespace: &str,
    pods: &HashSet<String>,
) -> HashMap<String, PodUsage> {
    summary
        .get("pods")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|pod| pod.pointer("/podRef/namespace").and_then(Value::as_str) == Some(namespace))
        .filter_map(|pod| {
            let name = pod.pointer("/podRef/name")?.as_str()?;
            if !pods.contains(name) {
                return None;
            }
            let containers = pod.get("containers")?.as_array()?;
            let sum = |pointer: &str| {
                containers
                    .iter()
                    .filter_map(|c| c.pointer(pointer).and_then(Value::as_f64))
                    .reduce(|a, b| a + b)
            };
            Some((
                name.to_string(),
                PodUsage {
                    cpu_millicores: sum("/cpu/usageNanoCores").map(|n| n / 1e6),
                    memory_bytes: sum("/memory/workingSetBytes"),
                },
            ))
        })
        .collect()
}

/// Instant usage for the first round: the kubelet node summaries, or one
/// metrics-server read when those aren't served. Best effort — an empty map
/// just leaves CPU `measuring…` until the counters move.
async fn hydrate_usage(
    client: &kube::Client,
    request: &MetricsRequest,
    pods: &[(String, Option<String>)],
) -> HashMap<String, PodUsage> {
    let names: HashSet<String> = pods.iter().map(|(name, _)| name.clone()).collect();
    let nodes: HashSet<&str> = pods
        .iter()
        .filter_map(|(_, node)| node.as_deref())
        .collect();
    let mut usage = HashMap::new();
    for node in nodes {
        let fetched = with_timeout("kubelet summary", async {
            let text = kubelet_get(client, node, "/stats/summary").await?;
            serde_json::from_str::<Value>(&text).context("Invalid kubelet summary")
        })
        .await;
        match fetched {
            Ok(summary) => usage.extend(summary_usage(&summary, &request.namespace, &names)),
            Err(e) => tracing::debug!("No kubelet summary for hydration: {e:#}"),
        }
    }
    if usage.is_empty() && metrics_server_available(client).await {
        match with_timeout(
            "metrics-server",
            metrics_server_usage(client, &request.namespace, &request.selector),
        )
        .await
        {
            Ok(from_server) => usage = from_server,
            Err(e) => tracing::debug!("No metrics-server hydration: {e:#}"),
        }
    }
    usage
}

/// Usage from metrics-server PodMetrics.
async fn metrics_server_usage(
    client: &kube::Client,
    namespace: &str,
    selector: &str,
) -> anyhow::Result<HashMap<String, PodUsage>> {
    Ok(metrics_server_snapshot(client, Some(namespace), selector)
        .await?
        .into_iter()
        .map(|(key, usage)| (key.name, usage))
        .collect())
}

fn metrics_resource(kind: &str, plural: &str) -> ApiResource {
    ApiResource {
        group: "metrics.k8s.io".into(),
        version: "v1beta1".into(),
        api_version: "metrics.k8s.io/v1beta1".into(),
        kind: kind.into(),
        plural: plural.into(),
    }
}

async fn metrics_server_snapshot(
    client: &kube::Client,
    namespace: Option<&str>,
    selector: &str,
) -> anyhow::Result<HashMap<PodKey, PodUsage>> {
    let resource = metrics_resource("PodMetrics", "pods");
    let api = match namespace {
        Some(ns) => Api::<DynamicObject>::namespaced_with(client.clone(), ns, &resource),
        None => Api::all_with(client.clone(), &resource),
    };
    let list = api
        .list(&ListParams::default().labels(selector))
        .await
        .context("Failed to list PodMetrics")?;
    Ok(list
        .items
        .iter()
        .filter_map(|item| {
            let name = item.metadata.name.clone()?;
            let containers = item.data.get("containers")?.as_array()?;
            let sum = |field: &str, parse: fn(&str) -> Option<f64>| {
                containers
                    .iter()
                    .filter_map(|c| c.pointer(&format!("/usage/{field}"))?.as_str())
                    .filter_map(parse)
                    .reduce(|a, b| a + b)
            };
            Some((
                PodKey::new(
                    item.metadata.namespace.as_deref().unwrap_or_default(),
                    &name,
                ),
                PodUsage {
                    cpu_millicores: sum("cpu", parse_cpu_millicores),
                    memory_bytes: sum("memory", parse_memory_bytes),
                },
            ))
        })
        .collect())
}

/// The selected pods with their nodes.
async fn list_pods(
    client: &kube::Client,
    namespace: &str,
    selector: &str,
) -> anyhow::Result<Vec<(String, Option<String>)>> {
    let pods = Api::<Pod>::namespaced(client.clone(), namespace)
        .list(&ListParams::default().labels(selector))
        .await
        .with_context(|| format!("Failed to list pods for '{selector}'"))?;
    Ok(pods
        .items
        .into_iter()
        .filter_map(|pod| {
            let node = pod.spec.and_then(|s| s.node_name);
            Some((pod.metadata.name?, node))
        })
        .collect())
}

/// Keep usage for a set of pods live: detect the source, then send a
/// snapshot every [`REFRESH_INTERVAL`] until the receiver is dropped. With
/// no source available a single empty snapshot is sent and the task ends.
pub async fn watch_pod_metrics(
    client: kube::Client,
    request: MetricsRequest,
    tx: UnboundedSender<anyhow::Result<MetricsSnapshot>>,
) {
    let mut source = detect_source(&client, request.setting).await;
    tracing::debug!("Pod metrics source: {source}");
    let mut trackers = HashMap::new();
    let mut failures = 0u32;
    // Whether the current source has ever produced usage. Only a source that
    // never worked here falls back; one that did just backs off and retries.
    let mut source_proven = false;
    // The kubelet source hydrates CPU on its first successful round.
    let mut hydrated = false;

    loop {
        if source == MetricsSource::None {
            // Nothing to poll: one empty snapshot tells the view to show
            // requests/limits, then the task ends.
            let _ = tx.send(Ok(MetricsSnapshot {
                source,
                pods: HashMap::new(),
            }));
            return;
        }
        let round = async {
            let pods = with_timeout(
                "pod list",
                list_pods(&client, &request.namespace, &request.selector),
            )
            .await?;
            match source {
                MetricsSource::Kubelet => {
                    let mut usage =
                        kubelet_usage(&client, &request.namespace, &pods, &mut trackers).await?;
                    if !hydrated {
                        hydrated = true;
                        for (pod, instant) in hydrate_usage(&client, &request, &pods).await {
                            let (Some(cpu), Some(entry)) =
                                (instant.cpu_millicores, usage.get_mut(&pod))
                            else {
                                continue;
                            };
                            if entry.cpu_millicores.is_none() {
                                entry.cpu_millicores = Some(cpu);
                                trackers.entry(pod).or_default().seed(cpu);
                            }
                        }
                    }
                    Ok(usage)
                }
                MetricsSource::MetricsServer => {
                    with_timeout(
                        "metrics-server",
                        metrics_server_usage(&client, &request.namespace, &request.selector),
                    )
                    .await
                }
                MetricsSource::None => Ok(HashMap::new()), // handled above
            }
        }
        .await;

        match round {
            Ok(pods) => {
                failures = 0;
                source_proven = true;
                let pods = pods
                    .into_iter()
                    .map(|(name, usage)| (PodKey::new(&request.namespace, &name), usage))
                    .collect();
                if tx.send(Ok(MetricsSnapshot { source, pods })).is_err() {
                    return; // View closed
                }
            }
            Err(e) if !source_proven && source != MetricsSource::None => {
                // Never worked here (proxy blocked, kubelet auth, PodMetrics
                // forbidden): drop to the next source straight away.
                tracing::debug!("Pod metrics via {source} unavailable, falling back: {e:#}");
                let served =
                    source == MetricsSource::Kubelet && metrics_server_available(&client).await;
                source = fallback_from(source, served);
                continue;
            }
            Err(e) => {
                // Still failing (e.g. the pod list itself): back off slowly.
                failures = failures.saturating_add(1);
                tracing::debug!(
                    "Pod metrics round failed ({failures} in a row), retrying in {:?}: {e:#}",
                    poll_delay(failures)
                );
                if tx.is_closed() {
                    return;
                }
            }
        }
        tokio::time::sleep(poll_delay(failures)).await;
    }
}

/// Parse requests/limits out of a pod spec: summed over containers; a limit
/// is only reported when every container sets one.
pub fn pod_resources(pod: &Value) -> PodResources {
    let containers = pod
        .pointer("/spec/containers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let sum = |field: &str, parse: fn(&str) -> Option<f64>, require_all: bool| {
        let values: Vec<Option<f64>> = containers
            .iter()
            .map(|c| {
                c.pointer(&format!("/resources/{field}"))
                    .and_then(Value::as_str)
                    .and_then(parse)
            })
            .collect();
        if values.is_empty() || (require_all && values.iter().any(Option::is_none)) {
            return None;
        }
        let present: Vec<f64> = values.into_iter().flatten().collect();
        (!present.is_empty()).then(|| present.iter().sum())
    };
    PodResources {
        cpu_request: sum("requests/cpu", parse_cpu_millicores, false),
        cpu_limit: sum("limits/cpu", parse_cpu_millicores, true),
        memory_request: sum("requests/memory", parse_memory_bytes, false),
        memory_limit: sum("limits/memory", parse_memory_bytes, true),
    }
}

/// A pod's summed requests/limits (millicores / bytes).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PodResources {
    pub cpu_request: Option<f64>,
    pub cpu_limit: Option<f64>,
    pub memory_request: Option<f64>,
    pub memory_limit: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_cpu_quantities() {
        assert_eq!(parse_cpu_millicores("250m"), Some(250.0));
        assert_eq!(parse_cpu_millicores("2"), Some(2000.0));
        assert_eq!(parse_cpu_millicores("0.5"), Some(500.0));
        assert_eq!(parse_cpu_millicores("5000000n"), Some(5.0));
        assert_eq!(parse_cpu_millicores("1500u"), Some(1.5));
        assert_eq!(parse_cpu_millicores("abc"), None);
    }

    #[test]
    fn parses_memory_quantities() {
        assert_eq!(parse_memory_bytes("128Mi"), Some(128.0 * 1024.0 * 1024.0));
        assert_eq!(parse_memory_bytes("1Gi"), Some(1024.0 * 1024.0 * 1024.0));
        assert_eq!(parse_memory_bytes("500M"), Some(500e6));
        assert_eq!(parse_memory_bytes("4096"), Some(4096.0));
        assert_eq!(parse_memory_bytes("1e3"), Some(1000.0));
    }

    #[test]
    fn formats_like_kubectl() {
        assert_eq!(format_cpu(250.0), "250m");
        assert_eq!(format_cpu(2000.0), "2");
        assert_eq!(format_cpu(1500.0), "1.5");
        assert_eq!(format_memory(45.0 * 1024.0 * 1024.0), "45Mi");
        assert_eq!(format_memory(1.5 * 1024.0 * 1024.0 * 1024.0), "1.5Gi");
        assert_eq!(format_memory(1024.0 * 1024.0 * 1024.0), "1Gi");
        assert_eq!(format_memory(512.0), "512");
    }

    #[test]
    fn parses_exposition_lines() {
        let s = parse_sample(
            r#"container_cpu_usage_seconds_total{container="app",namespace="ns",pod="web-1"} 12.5 1690000000000"#,
        )
        .unwrap();
        assert_eq!(s.name, "container_cpu_usage_seconds_total");
        assert_eq!(s.labels["pod"], "web-1");
        assert_eq!(s.value, 12.5);
        assert_eq!(s.timestamp_ms, Some(1690000000000.0));

        let escaped = parse_sample(r#"m{a="x\"y",b="z"} 1"#).unwrap();
        assert_eq!(escaped.labels["a"], "x\"y");
        assert_eq!(escaped.timestamp_ms, None);

        assert!(parse_sample("# HELP foo bar").is_none());
        assert_eq!(parse_sample("scrape_error 0").unwrap().value, 0.0);
    }

    #[test]
    fn totals_sum_containers_and_skip_sandbox_series() {
        let text = r#"
# TYPE container_cpu_usage_seconds_total counter
container_cpu_usage_seconds_total{container="app",namespace="ns",pod="web-1"} 10 1000
container_cpu_usage_seconds_total{container="sidecar",namespace="ns",pod="web-1"} 5 1000
container_cpu_usage_seconds_total{container="",namespace="ns",pod="web-1"} 99 1000
container_cpu_usage_seconds_total{container="POD",namespace="ns",pod="web-1"} 99 1000
container_cpu_usage_seconds_total{container="app",namespace="other",pod="web-1"} 99 1000
container_memory_working_set_bytes{container="app",namespace="ns",pod="web-1"} 100 1000
container_memory_working_set_bytes{container="sidecar",namespace="ns",pod="web-1"} 50 1000
container_memory_working_set_bytes{container="app",namespace="ns",pod="unrelated"} 7 1000
"#;
        let pods = HashSet::from(["web-1".to_string()]);
        let totals = pod_totals(text, "ns", &pods);
        assert_eq!(totals.len(), 1);
        assert_eq!(totals["web-1"].cpu_seconds, Some(15.0));
        assert_eq!(totals["web-1"].memory_bytes, Some(150.0));
    }

    #[test]
    fn cpu_rate_from_counters() {
        // 0.5 CPU-seconds over 2s = 250m
        assert_eq!(cpu_rate((10.0, 1000.0), (10.5, 3000.0)), Some(250.0));
        assert_eq!(
            cpu_rate((10.0, 1000.0), (1.0, 3000.0)),
            None,
            "counter reset"
        );
        assert_eq!(
            cpu_rate((10.0, 1000.0), (11.0, 1000.0)),
            None,
            "no time passed"
        );
    }

    #[test]
    fn pod_resources_sum_and_require_all_limits() {
        let pod = json!({"spec": {"containers": [
            {"resources": {"requests": {"cpu": "100m", "memory": "64Mi"}, "limits": {"cpu": "200m", "memory": "128Mi"}}},
            {"resources": {"requests": {"cpu": "50m"}, "limits": {"memory": "64Mi"}}}
        ]}});
        let r = pod_resources(&pod);
        assert_eq!(r.cpu_request, Some(150.0));
        assert_eq!(r.cpu_limit, None, "second container has no CPU limit");
        assert_eq!(r.memory_request, Some(64.0 * 1024.0 * 1024.0));
        assert_eq!(r.memory_limit, Some(192.0 * 1024.0 * 1024.0));
    }

    #[test]
    fn cpu_tracker_waits_for_the_counter_to_advance() {
        let rounded = |r: Option<f64>| r.map(|v| v.round());
        let mut t = CpuTracker::default();
        assert_eq!(
            t.observe((10.0, 1_000.0)),
            None,
            "first sample is a baseline"
        );
        assert_eq!(
            t.observe((10.0, 1_000.0)),
            None,
            "same sample: still no rate"
        );
        // 0.3 CPU-seconds over 15s = 20m
        assert_eq!(rounded(t.observe((10.3, 16_000.0))), Some(20.0));
        assert_eq!(
            rounded(t.observe((10.3, 16_000.0))),
            Some(20.0),
            "unchanged sample keeps the last rate"
        );
        assert_eq!(t.observe((1.0, 31_000.0)), None, "counter reset");
    }

    #[test]
    fn seeded_rate_shows_until_counters_advance() {
        let mut t = CpuTracker::default();
        assert_eq!(t.observe((10.0, 1_000.0)), None);
        t.seed(12.0);
        assert_eq!(
            t.observe((10.0, 1_000.0)),
            Some(12.0),
            "seed shows immediately"
        );
        // Once the counter moves, the measured rate takes over.
        assert_eq!(t.observe((10.3, 16_000.0)).map(f64::round), Some(20.0));
        t.seed(99.0);
        assert_eq!(
            t.observe((10.3, 16_000.0)).map(f64::round),
            Some(20.0),
            "a seed never replaces a measured rate"
        );
    }

    #[test]
    fn summary_usage_sums_containers_for_wanted_pods() {
        let summary = json!({"pods": [
            {"podRef": {"name": "web-1", "namespace": "ns"}, "containers": [
                {"cpu": {"usageNanoCores": 15_000_000.0}, "memory": {"workingSetBytes": 100.0}},
                {"cpu": {"usageNanoCores": 5_000_000.0}, "memory": {"workingSetBytes": 50.0}}
            ]},
            {"podRef": {"name": "web-1", "namespace": "other"}, "containers": [
                {"cpu": {"usageNanoCores": 1.0}}
            ]},
            {"podRef": {"name": "unrelated", "namespace": "ns"}, "containers": []}
        ]});
        let usage = summary_usage(&summary, "ns", &HashSet::from(["web-1".to_string()]));
        assert_eq!(usage.len(), 1);
        assert_eq!(usage["web-1"].cpu_millicores, Some(20.0));
        assert_eq!(usage["web-1"].memory_bytes, Some(150.0));
    }

    #[test]
    fn poll_delay_backs_off_slowly_to_a_cap() {
        assert_eq!(poll_delay(0), Duration::from_secs(15));
        assert_eq!(poll_delay(1), Duration::from_secs(30));
        assert_eq!(poll_delay(2), Duration::from_secs(60));
        assert_eq!(poll_delay(3), Duration::from_secs(120));
        assert_eq!(poll_delay(50), MAX_BACKOFF);
    }

    #[test]
    fn fallback_chain_always_ends_at_none() {
        assert_eq!(
            fallback_from(MetricsSource::Kubelet, true),
            MetricsSource::MetricsServer
        );
        assert_eq!(
            fallback_from(MetricsSource::Kubelet, false),
            MetricsSource::None
        );
        assert_eq!(
            fallback_from(MetricsSource::MetricsServer, true),
            MetricsSource::None
        );
        assert_eq!(
            fallback_from(MetricsSource::None, true),
            MetricsSource::None
        );
    }

    #[test]
    fn setting_parses_config_values() {
        assert_eq!(
            MetricsSourceSetting::parse("metrics-server"),
            Some(MetricsSourceSetting::MetricsServer)
        );
        assert_eq!(MetricsSourceSetting::parse("prometheus"), None);
        let yaml: MetricsSourceSetting = crate::yaml::from_str("kubelet").unwrap();
        assert_eq!(yaml, MetricsSourceSetting::Kubelet);
    }
}

/// Node usage, or summed pod requests when metrics are unavailable.
#[derive(Debug, Clone, Default)]
pub struct NodeUsage {
    pub usage: PodUsage,
    pub pods: Option<usize>,
    pub requests: bool,
}

/// Refresh node usage through the configured provider, retaining useful
/// allocation information when metrics or pod listing are forbidden.
pub async fn watch_node_metrics(
    client: kube::Client,
    setting: MetricsSourceSetting,
    tx: UnboundedSender<HashMap<String, NodeUsage>>,
) {
    use k8s_openapi::api::core::v1::Node;
    let (allocations_tx, allocations_rx) = tokio::sync::watch::channel(None);
    let mut tasks = tokio::task::JoinSet::new();
    tasks.spawn(watch_node_allocations(client.clone(), allocations_tx));
    let source = detect_source(&client, setting).await;
    loop {
        let round = async {
            let nodes = with_timeout("node list", async {
                Api::<Node>::all(client.clone())
                    .list(&ListParams::default())
                    .await
                    .context("Failed to list nodes for metrics")
            })
            .await?;
            let mut result: HashMap<String, NodeUsage> = nodes
                .items
                .into_iter()
                .filter_map(|node| {
                    Some((
                        node.metadata.name?,
                        NodeUsage {
                            requests: true,
                            ..Default::default()
                        },
                    ))
                })
                .collect();
            if let Some(allocations) = allocations_rx.borrow().as_ref() {
                for (name, node) in &mut result {
                    node.pods = Some(0);
                    if let Some(allocation) = allocations.get(name) {
                        *node = allocation.clone();
                    }
                }
            }
            if source == MetricsSource::Kubelet {
                use futures::{StreamExt, stream};
                let names: Vec<_> = result.keys().cloned().collect();
                let mut summaries = stream::iter(names.into_iter().map(|name| {
                    let client = &client;
                    async move {
                        let fetched = with_timeout("node summary", async {
                            let text = kubelet_get(client, &name, "/stats/summary").await?;
                            serde_json::from_str::<Value>(&text).context("Invalid node summary")
                        })
                        .await;
                        (name, fetched)
                    }
                }))
                .buffer_unordered(8);
                while let Some((name, fetched)) = summaries.next().await {
                    let Some(node) = result.get_mut(&name) else {
                        continue;
                    };
                    match fetched {
                        Ok(summary) => {
                            let usage = PodUsage {
                                cpu_millicores: summary
                                    .pointer("/node/cpu/usageNanoCores")
                                    .and_then(Value::as_f64)
                                    .map(|n| n / 1e6),
                                memory_bytes: summary
                                    .pointer("/node/memory/workingSetBytes")
                                    .and_then(Value::as_f64),
                            };
                            if usage.cpu_millicores.is_some() || usage.memory_bytes.is_some() {
                                node.usage = usage;
                                node.requests = false;
                            }
                        }
                        Err(error) => {
                            tracing::debug!("Node summary unavailable for {name}: {error:#}")
                        }
                    }
                }
            }
            if source != MetricsSource::None && result.values().any(|n| n.requests) {
                let resource = metrics_resource("NodeMetrics", "nodes");
                match with_timeout("node metrics list", async {
                    Api::<DynamicObject>::all_with(client.clone(), &resource)
                        .list(&ListParams::default())
                        .await
                        .context("Failed to list NodeMetrics")
                })
                .await
                {
                    Ok(metrics) => {
                        for item in metrics.items {
                            let Some(node) = item
                                .metadata
                                .name
                                .as_ref()
                                .and_then(|name| result.get_mut(name))
                                .filter(|n| n.requests)
                            else {
                                continue;
                            };
                            node.usage = PodUsage {
                                cpu_millicores: item
                                    .data
                                    .pointer("/usage/cpu")
                                    .and_then(Value::as_str)
                                    .and_then(parse_cpu_millicores),
                                memory_bytes: item
                                    .data
                                    .pointer("/usage/memory")
                                    .and_then(Value::as_str)
                                    .and_then(parse_memory_bytes),
                            };
                            node.requests = false;
                        }
                    }
                    Err(error) => tracing::debug!("NodeMetrics unavailable: {error}"),
                }
            }
            Ok::<_, anyhow::Error>(result)
        };
        tokio::select! {
            _ = tx.closed() => return,
            result = round => match result {
                Ok(snapshot) => if tx.send(snapshot).is_err() { return; },
                Err(error) => tracing::debug!("Node usage unavailable: {error:#}"),
            }
        }
        tokio::select! {
            _ = tx.closed() => return,
            _ = tokio::time::sleep(POLL_INTERVAL) => {}
        }
    }
}

/// Poll usage for pods already supplied by the kind watch. One task covers
/// every namespace, and each distinct node is scraped once per round.
pub async fn watch_list_metrics(
    client: kube::Client,
    namespace: Option<String>,
    setting: MetricsSourceSetting,
    mut targets: tokio::sync::watch::Receiver<HashMap<PodKey, String>>,
    tx: UnboundedSender<MetricsSnapshot>,
) {
    use futures::{StreamExt, stream};
    let mut source = detect_source(&client, setting).await;
    let mut trackers: HashMap<PodKey, CpuTracker> = HashMap::new();
    let mut hydrated = false;
    let mut proven = false;
    let mut failures = 0;
    loop {
        let selected = targets.borrow_and_update().clone();
        if selected.is_empty() {
            tokio::select! { _ = tx.closed() => return, result = targets.changed() => if result.is_err() { return; } }
            continue;
        }
        let wanted: HashSet<_> = selected.keys().cloned().collect();
        trackers.retain(|key, _| wanted.contains(key));
        let round = async {
            if source == MetricsSource::None {
                return Ok(HashMap::new());
            }
            if source == MetricsSource::MetricsServer {
                let mut values = with_timeout(
                    "PodMetrics list",
                    metrics_server_snapshot(&client, namespace.as_deref(), ""),
                )
                .await?;
                values.retain(|key, _| wanted.contains(key));
                return Ok(values);
            }
            let nodes: HashSet<_> = selected
                .values()
                .filter(|node| !node.is_empty())
                .cloned()
                .collect();
            let count = nodes.len();
            let mut failed = 0;
            let mut usage = HashMap::new();
            let mut scrapes = stream::iter(nodes.into_iter().map(|node| {
                let client = &client;
                async move {
                    let scraped = with_timeout("kubelet scrape", scrape_node(client, &node)).await;
                    let summary = if !hydrated {
                        with_timeout("kubelet summary", async {
                            let text = kubelet_get(client, &node, "/stats/summary").await?;
                            serde_json::from_str::<Value>(&text).context("Invalid kubelet summary")
                        })
                        .await
                        .ok()
                    } else {
                        None
                    };
                    (scraped, summary)
                }
            }))
            .buffer_unordered(8);
            while let Some((scraped, summary)) = scrapes.next().await {
                match scraped {
                    Ok(text) => {
                        for (key, total) in all_pod_totals(&text, &wanted) {
                            let cpu = total.cpu_seconds.and_then(|cpu| {
                                trackers.entry(key.clone()).or_default().observe((
                                    cpu,
                                    total
                                        .cpu_timestamp_ms
                                        .unwrap_or(chrono::Utc::now().timestamp_millis() as f64),
                                ))
                            });
                            usage.insert(
                                key,
                                PodUsage {
                                    cpu_millicores: cpu,
                                    memory_bytes: total.memory_bytes,
                                },
                            );
                        }
                    }
                    Err(error) => {
                        failed += 1;
                        tracing::debug!("Pod metrics scrape failed: {error:#}");
                    }
                }
                if let Some(summary) = summary {
                    let namespaces: HashSet<_> =
                        wanted.iter().map(|key| key.namespace.as_str()).collect();
                    for ns in namespaces {
                        let names = wanted
                            .iter()
                            .filter(|key| key.namespace == ns)
                            .map(|key| key.name.clone())
                            .collect();
                        for (name, instant) in summary_usage(&summary, ns, &names) {
                            let key = PodKey::new(ns, &name);
                            let entry = usage.entry(key.clone()).or_insert(instant);
                            if entry.cpu_millicores.is_none() {
                                entry.cpu_millicores = instant.cpu_millicores;
                            }
                            if let Some(cpu) = entry.cpu_millicores {
                                trackers.entry(key).or_default().seed(cpu);
                            }
                        }
                    }
                }
            }
            if count > 0 && failed == count && usage.is_empty() {
                anyhow::bail!("All kubelet scrapes failed");
            }
            Ok(usage)
        };
        let result = tokio::select! { _ = tx.closed() => return, result = round => result };
        match result {
            Ok(pods) => {
                failures = 0;
                proven = true;
                hydrated = true;
                if tx.send(MetricsSnapshot { source, pods }).is_err() {
                    return;
                }
            }
            Err(error) => {
                tracing::debug!("List metrics unavailable: {error:#}");
                if !proven {
                    source = fallback_from(
                        source,
                        source == MetricsSource::Kubelet && metrics_server_available(&client).await,
                    );
                    continue;
                }
                failures += 1;
            }
        }
        tokio::select! { _ = tx.closed() => return, _ = tokio::time::sleep(poll_delay(failures)) => {} }
    }
}

#[cfg(test)]
mod list_metrics_tests {
    use super::*;

    #[tokio::test]
    async fn one_node_scrape_serves_two_namespaces_without_pod_lists_and_stops_on_close() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (requests_tx, mut requests_rx) = tokio::sync::mpsc::unbounded_channel();
        let server = tokio::spawn(async move {
            loop {
                let (socket, _) = listener.accept().await.unwrap();
                let mut bytes = vec![0; 8192];
                let count = loop {
                    socket.readable().await.unwrap();
                    match socket.try_read(&mut bytes) {
                        Ok(count) => break count,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => continue,
                        Err(error) => panic!("{error}"),
                    }
                };
                let request = String::from_utf8_lossy(&bytes[..count]);
                let path = request.split_whitespace().nth(1).unwrap().to_string();
                requests_tx.send(path.clone()).unwrap();
                let body = if path.ends_with("/metrics/resource") {
                    "# TYPE container_cpu_usage_seconds_total counter\ncontainer_memory_working_set_bytes{namespace=\"a\",pod=\"web\",container=\"main\"} 100\ncontainer_memory_working_set_bytes{namespace=\"b\",pod=\"web\",container=\"main\"} 200\n"
                } else {
                    "{\"pods\":[]}"
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let mut remaining = response.as_bytes();
                while !remaining.is_empty() {
                    socket.writable().await.unwrap();
                    match socket.try_write(remaining) {
                        Ok(count) => remaining = &remaining[count..],
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => continue,
                        Err(error) => panic!("{error}"),
                    }
                }
            }
        });
        let client = kube::Client::try_from(kube::Config::new(
            format!("http://{address}").parse().unwrap(),
        ))
        .unwrap();
        let (_targets_tx, targets_rx) = tokio::sync::watch::channel(
            [
                (PodKey::new("a", "web"), "worker".into()),
                (PodKey::new("b", "web"), "worker".into()),
            ]
            .into(),
        );
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let task = tokio::spawn(watch_list_metrics(
            client,
            None,
            MetricsSourceSetting::Kubelet,
            targets_rx,
            tx,
        ));
        let snapshot = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            snapshot.pods[&PodKey::new("a", "web")].memory_bytes,
            Some(100.0)
        );
        assert_eq!(
            snapshot.pods[&PodKey::new("b", "web")].memory_bytes,
            Some(200.0)
        );
        let mut paths = Vec::new();
        while let Ok(path) = requests_rx.try_recv() {
            paths.push(path);
        }
        assert_eq!(
            paths,
            [
                "/api/v1/nodes/worker/proxy/metrics/resource",
                "/api/v1/nodes/worker/proxy/stats/summary"
            ]
        );
        drop(rx);
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap();
        server.abort();
    }
}

/// The allocation cache retains only node placement and scalar requests,
/// never full Pod objects. Completed pods don't count as node allocations.
#[derive(Clone)]
struct NodeAllocation {
    node: String,
    usage: PodUsage,
}

fn node_allocation(pod: &Pod) -> Option<NodeAllocation> {
    if pod
        .status
        .as_ref()
        .and_then(|s| s.phase.as_deref())
        .is_some_and(|p| matches!(p, "Succeeded" | "Failed"))
    {
        return None;
    }
    let spec = pod.spec.as_ref()?;
    let mut usage = PodUsage::default();
    for container in &spec.containers {
        if let Some(requests) = container
            .resources
            .as_ref()
            .and_then(|r| r.requests.as_ref())
        {
            if let Some(cpu) = requests.get("cpu").and_then(|q| parse_cpu_millicores(&q.0)) {
                *usage.cpu_millicores.get_or_insert(0.0) += cpu;
            }
            if let Some(mem) = requests
                .get("memory")
                .and_then(|q| parse_memory_bytes(&q.0))
            {
                *usage.memory_bytes.get_or_insert(0.0) += mem;
            }
        }
    }
    Some(NodeAllocation {
        node: spec.node_name.clone()?,
        usage,
    })
}

async fn watch_node_allocations(
    client: kube::Client,
    tx: tokio::sync::watch::Sender<Option<HashMap<String, NodeUsage>>>,
) {
    use futures::{FutureExt, StreamExt};
    use kube::runtime::{WatchStreamExt, watcher};
    let mut events = watcher(Api::<Pod>::all(client), watcher::Config::default())
        .backoff(crate::watcher::CappedBackoff::new())
        .boxed();
    let mut pods: HashMap<PodKey, NodeAllocation> = HashMap::new();
    let mut relist = HashMap::new();
    let mut listed = false;
    loop {
        let event = tokio::select! { _ = tx.closed() => return, event = events.next() => event };
        let Some(event) = event else {
            return;
        };
        let mut batch = vec![event];
        tokio::time::sleep(Duration::from_millis(200)).await;
        while let Some(Some(event)) = events.next().now_or_never() {
            batch.push(event);
        }
        for event in batch {
            let event = match event {
                Ok(event) => event,
                Err(error) => {
                    tracing::debug!("Node allocations unavailable: {error}");
                    if crate::kube::api::is_forbidden_error(&error.to_string()) {
                        return;
                    }
                    continue;
                }
            };
            match event {
                watcher::Event::Init => {
                    relist.clear();
                    listed = false;
                }
                watcher::Event::InitDone => {
                    pods = std::mem::take(&mut relist);
                    listed = true;
                }
                watcher::Event::Apply(pod) | watcher::Event::InitApply(pod) => {
                    let key = PodKey::new(
                        pod.metadata.namespace.as_deref().unwrap_or_default(),
                        pod.metadata.name.as_deref().unwrap_or_default(),
                    );
                    let store = if listed { &mut pods } else { &mut relist };
                    if let Some(allocation) = node_allocation(&pod) {
                        store.insert(key, allocation);
                    } else {
                        store.remove(&key);
                    }
                }
                watcher::Event::Delete(pod) => {
                    pods.remove(&PodKey::new(
                        pod.metadata.namespace.as_deref().unwrap_or_default(),
                        pod.metadata.name.as_deref().unwrap_or_default(),
                    ));
                }
            }
        }
        if listed {
            let mut totals: HashMap<String, NodeUsage> = HashMap::new();
            for pod in pods.values() {
                let entry = totals.entry(pod.node.clone()).or_insert_with(|| NodeUsage {
                    requests: true,
                    pods: Some(0),
                    ..Default::default()
                });
                *entry.pods.get_or_insert(0) += 1;
                if let Some(cpu) = pod.usage.cpu_millicores {
                    *entry.usage.cpu_millicores.get_or_insert(0.0) += cpu;
                }
                if let Some(mem) = pod.usage.memory_bytes {
                    *entry.usage.memory_bytes.get_or_insert(0.0) += mem;
                }
            }
            tx.send_replace(Some(totals));
        }
    }
}

#[cfg(test)]
mod node_allocation_tests {
    use super::*;

    #[test]
    fn counts_only_nonterminated_scheduled_pods_and_extracts_typed_requests() {
        let pod: Pod = serde_json::from_value(serde_json::json!({
            "metadata": {"name": "web", "namespace": "apps"},
            "spec": {"nodeName": "worker", "containers": [{"name": "web", "resources": {"requests": {"cpu": "100m", "memory": "64Mi"}}}]},
            "status": {"phase": "Running"}
        })).unwrap();
        let allocation = node_allocation(&pod).unwrap();
        assert_eq!(allocation.node, "worker");
        assert_eq!(allocation.usage.cpu_millicores, Some(100.0));
        assert_eq!(allocation.usage.memory_bytes, Some(64.0 * 1024.0 * 1024.0));
        for phase in ["Succeeded", "Failed"] {
            let mut completed = pod.clone();
            completed.status.as_mut().unwrap().phase = Some(phase.into());
            assert!(node_allocation(&completed).is_none());
        }
        let mut pending = pod;
        pending.spec.as_mut().unwrap().node_name = None;
        assert!(node_allocation(&pending).is_none());
    }
}
