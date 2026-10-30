//! Watch-driven live data for the drill-down views.
//!
//! The workload detail and the inventory breakdown show cluster state that
//! changes under the user (a rollout, a pod being replaced, an object turning
//! healthy). Rather than a one-shot fetch, each runs as a long-lived task on
//! the watch API that pushes a fresh snapshot whenever something it shows
//! changes. The tasks run only while their view is open: the app aborts them
//! on leave, and a closed channel ends them too.

use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::time::Duration;

use futures::stream::BoxStream;
use futures::{FutureExt, StreamExt};
use k8s_openapi::api::core::v1::{Event as CoreEvent, Pod};
use kube::core::DynamicObject;
use kube::runtime::{WatchStreamExt, watcher};
use kube::{Api, Resource};
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;

use crate::kube::app::{AppRequest, AppRole, AppShape, AppSnapshot};
use crate::kube::inventory::InventoryEntry;
use crate::kube::object_status::{ObjectHealth, ObjectStatus, compute_object_status};
use crate::kube::workloads::{WorkloadData, fetch_workload_data};

/// How long an app's records settle before its tree is rebuilt (an apply
/// changes the inventory and several objects at once).
const APP_RESHAPE_DEBOUNCE: Duration = Duration::from_millis(500);

/// How long to let a burst of watch events settle before refetching. A
/// rollout emits dozens of pod/ReplicaSet/event changes in quick succession.
const WORKLOAD_DEBOUNCE: Duration = Duration::from_millis(300);

/// Watch events for `api`, reconnecting with the app's capped backoff. The
/// stream ends if RBAC forbids the watch — retrying cannot fix that, and the
/// view keeps its last snapshot.
fn change_stream<K>(api: Api<K>, config: watcher::Config) -> BoxStream<'static, watcher::Event<K>>
where
    K: Resource + Clone + DeserializeOwned + Debug + Send + 'static,
{
    watcher(api, config)
        .backoff(crate::watcher::CappedBackoff::new())
        .take_while(|result| {
            let forbidden = matches!(
                result,
                Err(e) if crate::kube::api::is_forbidden_error(&e.to_string())
            );
            if forbidden {
                tracing::debug!("Live watch forbidden, keeping last snapshot");
            }
            futures::future::ready(!forbidden)
        })
        .filter_map(|result| futures::future::ready(result.ok()))
        .boxed()
}

/// Map watch events to "did the watched state change?". The task already
/// fetched the initial state, so the first listing (up to and including its
/// `InitDone`) is not a change; a later relist after a reconnect is.
fn changes<K: Send + 'static>(
    events: BoxStream<'static, watcher::Event<K>>,
) -> BoxStream<'static, bool> {
    events
        .scan(false, |listed, event| {
            let changed = match event {
                watcher::Event::Init | watcher::Event::InitApply(_) => false,
                watcher::Event::InitDone => std::mem::replace(listed, true),
                watcher::Event::Apply(_) | watcher::Event::Delete(_) => true,
            };
            futures::future::ready(Some(changed))
        })
        .boxed()
}

/// Keep a workload's detail live: send the initial snapshot, then a fresh
/// one whenever the workload, its pods, or its events change.
///
/// Ends when the receiver is dropped (view closed) or the initial fetch fails.
pub async fn watch_workload(
    client: kube::Client,
    kind: String,
    namespace: String,
    name: String,
    tx: UnboundedSender<anyhow::Result<WorkloadData>>,
) {
    let initial = fetch_workload_data(&client, &kind, &namespace, &name).await;
    let selector = initial.as_ref().ok().and_then(|d| d.pod_selector.clone());
    let failed = initial.is_err();
    if tx.send(initial).is_err() || failed {
        return;
    }

    let resource = match crate::kube::get_api_resource_with_fallback(
        &client, &kind, &namespace, &name,
    )
    .await
    {
        Ok(resource) => resource,
        Err(e) => {
            tracing::warn!("Live workload watch unavailable for {kind}/{name}: {e:#}");
            return;
        }
    };

    let workload_api: Api<DynamicObject> =
        Api::namespaced_with(client.clone(), &namespace, &resource);
    let by_name = watcher::Config::default().fields(&format!("metadata.name={name}"));
    let mut streams: Vec<BoxStream<'static, bool>> =
        vec![changes(change_stream(workload_api, by_name))];
    if let Some(selector) = selector {
        let pods: Api<Pod> = Api::namespaced(client.clone(), &namespace);
        let config = watcher::Config::default().labels(&selector);
        streams.push(changes(change_stream(pods, config)));
    }
    let events: Api<CoreEvent> = Api::namespaced(client.clone(), &namespace);
    let config = watcher::Config::default().fields(&format!(
        "involvedObject.kind={kind},involvedObject.name={name}"
    ));
    streams.push(changes(change_stream(events, config)));

    let mut changes = futures::stream::select_all(streams);
    while let Some(changed) = changes.next().await {
        if !changed {
            continue;
        }
        // Coalesce the burst, then refetch once.
        tokio::time::sleep(WORKLOAD_DEBOUNCE).await;
        while let Some(Some(_)) = changes.next().now_or_never() {}
        let result = fetch_workload_data(&client, &kind, &namespace, &name).await;
        if let Err(ref e) = result {
            tracing::debug!("Live workload refetch failed for {kind}/{name}: {e:#}");
        }
        if tx.send(result).is_err() {
            return;
        }
    }
}

/// Keep a HelmRelease's effective values live (#264): send the initial
/// values, then recompute whenever the release or any ConfigMap/Secret it
/// references changes. When `valuesFrom` itself changes, the set of watched
/// objects is rebuilt.
///
/// Ends when the receiver is dropped (view closed) or the initial fetch fails.
pub async fn watch_helm_values(
    client: kube::Client,
    namespace: String,
    name: String,
    reveal_secrets: bool,
    tx: UnboundedSender<anyhow::Result<crate::kube::helm_values::HelmValues>>,
) {
    use crate::kube::helm_values::fetch_helm_values;
    use k8s_openapi::api::core::v1::{ConfigMap, Secret};

    let initial = fetch_helm_values(&client, &namespace, &name, reveal_secrets).await;
    let mut refs: Vec<_> = match &initial {
        Ok(values) => values.sources.iter().map(|s| s.reference.clone()).collect(),
        Err(_) => Vec::new(),
    };
    let failed = initial.is_err();
    if tx.send(initial).is_err() || failed {
        return;
    }
    let release = match crate::kube::get_api_resource_with_fallback(
        &client,
        "HelmRelease",
        &namespace,
        &name,
    )
    .await
    {
        Ok(resource) => resource,
        Err(e) => {
            tracing::warn!("Live values watch unavailable for {name}: {e:#}");
            return;
        }
    };

    loop {
        let by_name = |n: &str| watcher::Config::default().fields(&format!("metadata.name={n}"));
        let mut streams: Vec<BoxStream<'static, bool>> = vec![changes(change_stream(
            Api::<DynamicObject>::namespaced_with(client.clone(), &namespace, &release),
            by_name(&name),
        ))];
        for reference in &refs {
            let config = by_name(&reference.name);
            match reference.kind.as_str() {
                "ConfigMap" => streams.push(changes(change_stream(
                    Api::<ConfigMap>::namespaced(client.clone(), &namespace),
                    config,
                ))),
                "Secret" => streams.push(changes(change_stream(
                    Api::<Secret>::namespaced(client.clone(), &namespace),
                    config,
                ))),
                _ => {}
            }
        }

        let mut events = futures::stream::select_all(streams);
        let rebuild = loop {
            let Some(changed) = events.next().await else {
                return; // Every watch ended (forbidden); keep the last values.
            };
            if !changed {
                continue;
            }
            tokio::time::sleep(WORKLOAD_DEBOUNCE).await;
            while let Some(Some(_)) = events.next().now_or_never() {}
            let result = fetch_helm_values(&client, &namespace, &name, reveal_secrets).await;
            let new_refs: Option<Vec<_>> = result
                .as_ref()
                .ok()
                .map(|v| v.sources.iter().map(|s| s.reference.clone()).collect());
            if tx.send(result).is_err() {
                return;
            }
            // valuesFrom changed: watch the new set of objects.
            if let Some(new_refs) = new_refs
                && new_refs != refs
            {
                break new_refs;
            }
        };
        refs = rebuild;
    }
}

/// Inventory rows sharing one watch: the row indexes for each object name.
type RowsByName = HashMap<String, Vec<usize>>;

/// One watched kind/namespace group of inventory rows.
struct WatchGroup {
    api_version: String,
    kind: String,
    rows: RowsByName,
}

/// A watched object as JSON with its `apiVersion` / `kind`, which list and
/// watch items omit and the kind-specific status rules key on.
fn stamped(obj: &DynamicObject, api_version: &str, kind: &str) -> serde_json::Result<Value> {
    let mut value = serde_json::to_value(obj)?;
    if let Some(map) = value.as_object_mut() {
        map.insert("apiVersion".into(), api_version.into());
        map.insert("kind".into(), kind.into());
    }
    Ok(value)
}

/// Apply one watch event to the status rows it touches. Returns whether any
/// row changed.
fn apply_status_event(
    statuses: &mut [ObjectStatus],
    group: &WatchGroup,
    event: watcher::Event<DynamicObject>,
) -> bool {
    let rows = &group.rows;
    let (obj, deleted) = match event {
        watcher::Event::Apply(obj) | watcher::Event::InitApply(obj) => (obj, false),
        watcher::Event::Delete(obj) => (obj, true),
        watcher::Event::Init | watcher::Event::InitDone => return false,
    };
    let Some(indexes) = obj.metadata.name.as_ref().and_then(|name| rows.get(name)) else {
        return false;
    };
    let status = if deleted {
        ObjectStatus::new(ObjectHealth::NotFound, "Not found in cluster")
    } else {
        match stamped(&obj, &group.api_version, &group.kind) {
            Ok(value) => compute_object_status(&value),
            Err(e) => ObjectStatus::new(ObjectHealth::Unknown, e.to_string()),
        }
    };
    let mut changed = false;
    for &index in indexes {
        if let Some(slot) = statuses.get_mut(index)
            && *slot != status
        {
            *slot = status.clone();
            changed = true;
        }
    }
    changed
}

/// Keep inventory statuses live: send the initial statuses (one GET per
/// object), then watch each kind/namespace group and send the updated list
/// whenever an object's health changes.
///
/// A group whose kind can't be discovered or watched keeps its initial
/// status. Ends when the receiver is dropped (view closed).
pub async fn watch_object_statuses(
    client: kube::Client,
    entries: Vec<InventoryEntry>,
    tx: UnboundedSender<anyhow::Result<Vec<ObjectStatus>>>,
) {
    let mut statuses = crate::kube::objects::fetch_object_statuses(&client, &entries).await;
    if tx.send(Ok(statuses.clone())).is_err() {
        return;
    }

    // One watch per (apiVersion, kind, namespace).
    let mut groups: HashMap<(String, String, String), RowsByName> = HashMap::new();
    for (index, entry) in entries.iter().enumerate() {
        groups
            .entry((
                entry.api_version.clone(),
                entry.kind.clone(),
                entry.namespace.clone(),
            ))
            .or_default()
            .entry(entry.name.clone())
            .or_default()
            .push(index);
    }

    let mut watch_groups: Vec<WatchGroup> = Vec::new();
    let mut streams = Vec::new();
    for ((api_version, kind, namespace), rows) in groups {
        let (resource, scope) =
            match crate::kube::objects::discover(&client, &api_version, &kind).await {
                Ok(found) => found,
                Err(e) => {
                    tracing::debug!("No live watch for {kind} ({api_version}): {e:#}");
                    continue;
                }
            };
        let api = crate::kube::objects::api_for(&client, &resource, &scope, &namespace);
        // A single object is watched by name; several share a kind watch.
        let config = match rows.keys().next() {
            Some(only) if rows.len() == 1 => {
                watcher::Config::default().fields(&format!("metadata.name={only}"))
            }
            _ => watcher::Config::default(),
        };
        let group = watch_groups.len();
        watch_groups.push(WatchGroup {
            api_version,
            kind,
            rows,
        });
        streams.push(
            change_stream(api, config)
                .map(move |event| (group, event))
                .boxed(),
        );
    }
    if streams.is_empty() {
        return;
    }

    let mut changes = futures::stream::select_all(streams);
    while let Some((group, event)) = changes.next().await {
        let mut changed = watch_groups
            .get(group)
            .is_some_and(|g| apply_status_event(&mut statuses, g, event));
        // Fold in whatever else is already queued before sending.
        while let Some(Some((group, event))) = changes.next().now_or_never() {
            changed |= watch_groups
                .get(group)
                .is_some_and(|g| apply_status_event(&mut statuses, g, event));
        }
        if changed && tx.send(Ok(statuses.clone())).is_err() {
            return;
        }
    }
}

/// Keep an app view (`kube::app`) live: send the tree, then the updated
/// tree whenever one of its objects changes status. When an expanded Flux
/// object's records change shape (a new inventory, another source) or a
/// workload gains or loses a pod, the tree is rebuilt and sent again.
///
/// Ends when the receiver is dropped (view closed), or when nothing in the
/// tree can be watched.
pub async fn watch_app(
    client: kube::Client,
    request: AppRequest,
    tx: UnboundedSender<anyhow::Result<AppSnapshot>>,
) {
    loop {
        let (mut snapshot, shape) = crate::kube::app::resolve(&client, &request).await;
        if tx.send(Ok(snapshot.clone())).is_err() {
            return;
        }
        match follow_app(&client, &mut snapshot, &shape, &tx).await {
            AppFollow::Reshape => tokio::time::sleep(APP_RESHAPE_DEBOUNCE).await,
            AppFollow::Done => return,
        }
    }
}

/// Why following an app's tree stopped.
enum AppFollow {
    /// Its records changed shape: rebuild it.
    Reshape,
    /// The receiver is gone, or nothing could be watched.
    Done,
}

/// What one of an app's watches reported.
enum AppEvent {
    /// A watch over the tree's objects of one kind and namespace.
    Object(usize, watcher::Event<DynamicObject>),
    /// A watch over one workload's pods (by its index in `pod_sets`).
    Pod(usize, watcher::Event<DynamicObject>),
}

/// The names one watch's current listing has delivered. A watch lists
/// before it watches — at start, and again after a reconnect — and an object
/// deleted while it wasn't watching just isn't in that listing: no delete
/// event ever comes. When the listing ends, what the tree expected but the
/// listing lacked is gone.
#[derive(Default)]
struct Listing {
    seen: HashSet<String>,
}

impl Listing {
    /// Note `event`; at the end of a listing, the names it lacked from
    /// `expected`.
    fn track<'a>(
        &mut self,
        event: &watcher::Event<DynamicObject>,
        expected: impl Iterator<Item = &'a String>,
    ) -> Vec<String> {
        match event {
            watcher::Event::Init => self.seen.clear(),
            watcher::Event::InitApply(obj) => {
                if let Some(name) = &obj.metadata.name {
                    self.seen.insert(name.clone());
                }
            }
            watcher::Event::InitDone => {
                return expected
                    .filter(|name| !self.seen.contains(*name))
                    .cloned()
                    .collect();
            }
            watcher::Event::Apply(_) | watcher::Event::Delete(_) => {}
        }
        Vec::new()
    }
}

/// What one event means for the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppChange {
    Unchanged,
    Changed,
    Reshape,
}

impl AppChange {
    /// Both changes: a reshape wins, then a change.
    fn and(self, other: AppChange) -> AppChange {
        match (self, other) {
            (AppChange::Reshape, _) | (_, AppChange::Reshape) => AppChange::Reshape,
            (AppChange::Changed, _) | (_, AppChange::Changed) => AppChange::Changed,
            _ => AppChange::Unchanged,
        }
    }
}

/// Watch an app's objects and pods, updating `snapshot` in place until the
/// tree changes shape.
async fn follow_app(
    client: &kube::Client,
    snapshot: &mut AppSnapshot,
    shape: &AppShape,
    tx: &UnboundedSender<anyhow::Result<AppSnapshot>>,
) -> AppFollow {
    // One watch per (apiVersion, kind, namespace) of the non-pod rows.
    let mut groups: HashMap<(String, String, String), RowsByName> = HashMap::new();
    // Pod rows by (namespace, name): a pod not among them is a new one.
    let mut pods: HashMap<(String, String), Vec<usize>> = HashMap::new();
    for (index, row) in snapshot.rows.iter().enumerate() {
        let o = &row.object;
        if row.role == AppRole::Pod {
            pods.entry((o.namespace.clone(), o.name.clone()))
                .or_default()
                .push(index);
            continue;
        }
        groups
            .entry((o.api_version.clone(), o.kind.clone(), o.namespace.clone()))
            .or_default()
            .entry(o.name.clone())
            .or_default()
            .push(index);
    }

    let mut watch_groups: Vec<WatchGroup> = Vec::new();
    let mut streams = Vec::new();
    for ((api_version, kind, namespace), rows) in groups {
        let (resource, scope) =
            match crate::kube::objects::discover(client, &api_version, &kind).await {
                Ok(found) => found,
                Err(e) => {
                    tracing::debug!("No live watch for {kind} ({api_version}): {e:#}");
                    continue;
                }
            };
        let api = crate::kube::objects::api_for(client, &resource, &scope, &namespace);
        let config = match rows.keys().next() {
            Some(only) if rows.len() == 1 => {
                watcher::Config::default().fields(&format!("metadata.name={only}"))
            }
            _ => watcher::Config::default(),
        };
        let group = watch_groups.len();
        watch_groups.push(WatchGroup {
            api_version,
            kind,
            rows,
        });
        streams.push(
            change_stream(api, config)
                .map(move |event| AppEvent::Object(group, event))
                .boxed(),
        );
    }
    let pod_resource = kube::api::ApiResource::erase::<Pod>(&());
    for (set_index, set) in shape.pod_sets.iter().enumerate() {
        let api: Api<DynamicObject> =
            Api::namespaced_with(client.clone(), &set.namespace, &pod_resource);
        let config = watcher::Config::default().labels(&set.selector);
        streams.push(
            change_stream(api, config)
                .map(move |event| AppEvent::Pod(set_index, event))
                .boxed(),
        );
    }
    if streams.is_empty() {
        return AppFollow::Done;
    }

    let mut object_listings: Vec<Listing> =
        watch_groups.iter().map(|_| Listing::default()).collect();
    let mut pod_listings: Vec<Listing> =
        shape.pod_sets.iter().map(|_| Listing::default()).collect();
    let mut apply = |snapshot: &mut AppSnapshot, event: AppEvent| -> AppChange {
        match event {
            AppEvent::Object(index, event) => {
                let (Some(group), Some(listing)) =
                    (watch_groups.get(index), object_listings.get_mut(index))
                else {
                    return AppChange::Unchanged;
                };
                let gone = listing.track(&event, group.rows.keys());
                let mut change = app_object_event(snapshot, shape, group, event);
                for name in gone {
                    change = change.and(app_object_gone(snapshot, shape, group, &name));
                }
                change
            }
            AppEvent::Pod(index, event) => {
                let (Some(set), Some(listing)) =
                    (shape.pod_sets.get(index), pod_listings.get_mut(index))
                else {
                    return AppChange::Unchanged;
                };
                if !listing.track(&event, set.pods.iter()).is_empty() {
                    return AppChange::Reshape;
                }
                app_pod_event(snapshot, &pods, event)
            }
        }
    };
    let mut events = futures::stream::select_all(streams);
    while let Some(event) = events.next().await {
        let mut changed = false;
        let mut next = Some(event);
        // Fold in whatever else is already queued before sending.
        while let Some(event) = next {
            match apply(snapshot, event) {
                AppChange::Reshape => return AppFollow::Reshape,
                AppChange::Changed => changed = true,
                AppChange::Unchanged => {}
            }
            next = events.next().now_or_never().flatten();
        }
        if changed && tx.send(Ok(snapshot.clone())).is_err() {
            return AppFollow::Done;
        }
    }
    AppFollow::Done
}

/// An event on one of the tree's objects: a new status, or a reshape when
/// an expanded Flux object's records changed or it was deleted.
fn app_object_event(
    snapshot: &mut AppSnapshot,
    shape: &AppShape,
    group: &WatchGroup,
    event: watcher::Event<DynamicObject>,
) -> AppChange {
    let (obj, deleted) = match event {
        watcher::Event::Apply(obj) | watcher::Event::InitApply(obj) => (obj, false),
        watcher::Event::Delete(obj) => (obj, true),
        watcher::Event::Init | watcher::Event::InitDone => return AppChange::Unchanged,
    };
    let Some(name) = obj.metadata.name.as_deref() else {
        return AppChange::Unchanged;
    };
    let value = if deleted {
        None
    } else {
        match stamped(&obj, &group.api_version, &group.kind) {
            Ok(value) => Some(value),
            Err(e) => {
                tracing::debug!("Unreadable watch object: {e}");
                return AppChange::Unchanged;
            }
        }
    };
    app_object_update(snapshot, shape, group, name, value.as_ref())
}

/// One of the tree's objects missing from its watch's listing: deleted
/// while the watch wasn't running.
fn app_object_gone(
    snapshot: &mut AppSnapshot,
    shape: &AppShape,
    group: &WatchGroup,
    name: &str,
) -> AppChange {
    app_object_update(snapshot, shape, group, name, None)
}

/// The tree's rows for object `name` of `group`, given the object (`None`:
/// deleted): a reshape when an object that shaped the tree (an expanded
/// Flux object, a workload) appeared, went, or changed shape, else its
/// rows' new status.
fn app_object_update(
    snapshot: &mut AppSnapshot,
    shape: &AppShape,
    group: &WatchGroup,
    name: &str,
    value: Option<&Value>,
) -> AppChange {
    let Some(indexes) = group.rows.get(name) else {
        return AppChange::Unchanged;
    };
    // An object that shaped the tree below it (a Flux object's inventory,
    // a workload's selector) rebuilds it when that changed — appearing,
    // going, or edited. Still missing is no change.
    let reshaped = indexes.iter().any(|index| {
        shape
            .shaped
            .get(index)
            .is_some_and(|recorded| !crate::kube::app::same_shape(recorded, value))
    });
    if reshaped {
        return AppChange::Reshape;
    }
    let status = match value {
        Some(value) => compute_object_status(value),
        None => ObjectStatus::new(ObjectHealth::NotFound, "Not found in cluster"),
    };
    let mut changed = AppChange::Unchanged;
    for &index in indexes {
        if let Some(row) = snapshot.rows.get_mut(index)
            && row.status != status
        {
            row.status = status.clone();
            changed = AppChange::Changed;
        }
    }
    changed
}

/// An event on a workload's pods: a known pod's new status, or a reshape
/// when a pod appears or goes.
fn app_pod_event(
    snapshot: &mut AppSnapshot,
    pods: &HashMap<(String, String), Vec<usize>>,
    event: watcher::Event<DynamicObject>,
) -> AppChange {
    let obj = match event {
        watcher::Event::Apply(obj) | watcher::Event::InitApply(obj) => obj,
        watcher::Event::Delete(_) => return AppChange::Reshape,
        watcher::Event::Init | watcher::Event::InitDone => return AppChange::Unchanged,
    };
    let key = (
        obj.metadata.namespace.clone().unwrap_or_default(),
        obj.metadata.name.clone().unwrap_or_default(),
    );
    let Some(indexes) = pods.get(&key) else {
        return AppChange::Reshape;
    };
    let status = match stamped(&obj, "v1", "Pod") {
        Ok(value) => compute_object_status(&value),
        Err(e) => ObjectStatus::new(ObjectHealth::Unknown, e.to_string()),
    };
    let mut changed = AppChange::Unchanged;
    for &index in indexes {
        if let Some(row) = snapshot.rows.get_mut(index)
            && row.status != status
        {
            row.status = status.clone();
            changed = AppChange::Changed;
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A Deployment as a watch delivers it: no apiVersion/kind.
    fn deployment(name: &str, ready: i64) -> DynamicObject {
        serde_json::from_value(json!({
            "metadata": {"name": name, "namespace": "apps"},
            "spec": {"replicas": 1},
            "status": {"replicas": 1, "updatedReplicas": 1, "readyReplicas": ready, "availableReplicas": ready}
        }))
        .unwrap()
    }

    fn rows() -> WatchGroup {
        WatchGroup {
            api_version: "apps/v1".to_string(),
            kind: "Deployment".to_string(),
            rows: HashMap::from([("web".to_string(), vec![1])]),
        }
    }

    #[test]
    fn apply_event_updates_only_the_matching_row() {
        let mut statuses = vec![
            ObjectStatus::new(ObjectHealth::Current, ""),
            ObjectStatus::new(ObjectHealth::Current, ""),
        ];
        let changed = apply_status_event(
            &mut statuses,
            &rows(),
            watcher::Event::Apply(deployment("web", 0)),
        );
        assert!(changed);
        assert_eq!(statuses[0].health, ObjectHealth::Current);
        // Deployment rules applied even though the watch omitted the kind.
        assert_eq!(statuses[1].health, ObjectHealth::InProgress);
        assert_eq!(statuses[1].message, "Ready: 0/1");

        // The same state again is not a change.
        assert!(!apply_status_event(
            &mut statuses,
            &rows(),
            watcher::Event::Apply(deployment("web", 0)),
        ));
    }

    #[test]
    fn apply_event_ignores_untracked_objects() {
        let mut statuses = vec![ObjectStatus::new(ObjectHealth::Current, ""); 2];
        assert!(!apply_status_event(
            &mut statuses,
            &rows(),
            watcher::Event::Apply(deployment("other", 0)),
        ));
    }

    #[test]
    fn delete_marks_row_not_found() {
        let mut statuses = vec![ObjectStatus::new(ObjectHealth::Current, ""); 2];
        assert!(apply_status_event(
            &mut statuses,
            &rows(),
            watcher::Event::Delete(deployment("web", 1)),
        ));
        assert_eq!(statuses[1].health, ObjectHealth::NotFound);
    }

    #[tokio::test]
    async fn first_listing_is_not_a_change_but_relist_is() {
        let events = futures::stream::iter(vec![
            watcher::Event::Init,
            watcher::Event::InitApply(Pod::default()),
            watcher::Event::InitDone, // end of the first listing
            watcher::Event::Apply(Pod::default()),
            watcher::Event::Init, // reconnect relist
            watcher::Event::InitDone,
        ])
        .boxed();
        let flags: Vec<bool> = changes(events).collect().await;
        assert_eq!(flags, vec![false, false, false, true, false, true]);
    }

    /// An app tree: a Kustomization (expanded, row 0), its Deployment, and a pod.
    fn app_tree() -> (AppSnapshot, AppShape, WatchGroup) {
        use crate::kube::app::AppRow;
        let entry = |api_version: &str, kind: &str, name: &str| InventoryEntry {
            kind: kind.into(),
            namespace: "apps".into(),
            name: name.into(),
            api_version: api_version.into(),
        };
        let row = |object, depth, role| AppRow {
            object,
            depth,
            role,
            status: ObjectStatus::new(ObjectHealth::Current, ""),
        };
        let snapshot = AppSnapshot {
            rows: vec![
                row(
                    entry("kustomize.toolkit.fluxcd.io/v1", "Kustomization", "shop"),
                    0,
                    AppRole::App,
                ),
                row(entry("apps/v1", "Deployment", "web"), 1, AppRole::Applied),
                row(entry("v1", "Pod", "web-1"), 2, AppRole::Pod),
            ],
            ..AppSnapshot::default()
        };
        let ks = json!({"status": {"inventory": {"entries": []}}});
        let shape = AppShape {
            shaped: HashMap::from([(0, crate::kube::app::shape_of(&ks))]),
            pod_sets: Vec::new(),
        };
        let group = WatchGroup {
            api_version: "kustomize.toolkit.fluxcd.io/v1".into(),
            kind: "Kustomization".into(),
            rows: HashMap::from([("shop".to_string(), vec![0])]),
        };
        (snapshot, shape, group)
    }

    fn object(value: Value) -> DynamicObject {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn an_apps_records_changing_rebuilds_its_tree_but_status_doesnt() {
        let (mut snapshot, shape, group) = app_tree();
        let failing = object(json!({
            "metadata": {"name": "shop", "namespace": "apps"},
            "status": {"inventory": {"entries": []},
                       "conditions": [{"type": "Ready", "status": "False", "message": "boom"}]}
        }));
        assert!(matches!(
            app_object_event(
                &mut snapshot,
                &shape,
                &group,
                watcher::Event::Apply(failing)
            ),
            AppChange::Changed
        ));
        assert_eq!(snapshot.rows[0].status.health, ObjectHealth::Failed);

        let pruned = object(json!({
            "metadata": {"name": "shop", "namespace": "apps"},
            "status": {"inventory": {"entries": [{"id": "apps_x__ConfigMap", "v": "v1"}]}}
        }));
        assert!(matches!(
            app_object_event(&mut snapshot, &shape, &group, watcher::Event::Apply(pruned)),
            AppChange::Reshape
        ));
        let gone = object(json!({"metadata": {"name": "shop", "namespace": "apps"}}));
        assert!(matches!(
            app_object_event(&mut snapshot, &shape, &group, watcher::Event::Delete(gone)),
            AppChange::Reshape
        ));
    }

    #[test]
    fn an_app_that_appears_later_is_expanded() {
        let (mut snapshot, mut shape, group) = app_tree();
        // The root couldn't be read when the tree was built.
        shape.shaped.insert(0, String::new());
        snapshot.rows[0].status = ObjectStatus::new(ObjectHealth::NotFound, "not found");
        let created = object(json!({
            "metadata": {"name": "shop", "namespace": "apps"},
            "status": {"inventory": {"entries": []}}
        }));
        assert!(matches!(
            app_object_event(
                &mut snapshot,
                &shape,
                &group,
                watcher::Event::Apply(created)
            ),
            AppChange::Reshape
        ));
    }

    #[test]
    fn a_listing_reports_what_it_lacked() {
        let pod = |name: &str| object(json!({"metadata": {"name": name, "namespace": "apps"}}));
        let expected = ["web-1".to_string(), "web-2".to_string()];
        let mut listing = Listing::default();
        // The first listing: web-2 was deleted before the watch started.
        assert!(
            listing
                .track(&watcher::Event::Init, expected.iter())
                .is_empty()
        );
        assert!(
            listing
                .track(&watcher::Event::InitApply(pod("web-1")), expected.iter())
                .is_empty()
        );
        assert_eq!(
            listing.track(&watcher::Event::InitDone, expected.iter()),
            ["web-2"]
        );
        // Watch events in between don't count; a relist starts over.
        assert!(
            listing
                .track(&watcher::Event::Apply(pod("web-2")), expected.iter())
                .is_empty()
        );
        listing.track(&watcher::Event::Init, expected.iter());
        listing.track(&watcher::Event::InitApply(pod("web-1")), expected.iter());
        listing.track(&watcher::Event::InitApply(pod("web-2")), expected.iter());
        assert!(
            listing
                .track(&watcher::Event::InitDone, expected.iter())
                .is_empty()
        );
    }

    #[test]
    fn a_missing_root_waits_for_creation_instead_of_rebuilding() {
        let (mut snapshot, mut shape, group) = app_tree();
        shape
            .shaped
            .insert(0, crate::kube::app::MISSING.to_string());
        snapshot.rows[0].status = ObjectStatus::new(ObjectHealth::NotFound, "Not found in cluster");
        // Every listing that still lacks it: nothing to do.
        assert_eq!(
            app_object_gone(&mut snapshot, &shape, &group, "shop"),
            AppChange::Unchanged
        );
        assert_eq!(
            app_object_gone(&mut snapshot, &shape, &group, "shop"),
            AppChange::Unchanged
        );
    }

    #[test]
    fn a_workload_appearing_or_changing_its_selector_rebuilds_its_pods() {
        let (mut snapshot, mut shape, _) = app_tree();
        let deployments = WatchGroup {
            api_version: "apps/v1".into(),
            kind: "Deployment".into(),
            rows: HashMap::from([("web".to_string(), vec![1])]),
        };
        let web = |selector: Value, ready: i64| {
            object(json!({
                "metadata": {"name": "web", "namespace": "apps"},
                "spec": {"replicas": 1, "selector": selector},
                "status": {"replicas": 1, "updatedReplicas": 1, "readyReplicas": ready, "availableReplicas": ready}
            }))
        };
        // It couldn't be read when the tree was built; it appears.
        shape
            .shaped
            .insert(1, crate::kube::app::MISSING.to_string());
        assert_eq!(
            app_object_event(
                &mut snapshot,
                &shape,
                &deployments,
                watcher::Event::Apply(web(json!({"matchLabels": {"app": "web"}}), 1))
            ),
            AppChange::Reshape
        );
        // Read with a selector: readiness only updates the row…
        let stamped = stamped(
            &web(json!({"matchLabels": {"app": "web"}}), 1),
            "apps/v1",
            "Deployment",
        )
        .unwrap();
        shape.shaped.insert(1, crate::kube::app::shape_of(&stamped));
        assert_eq!(
            app_object_event(
                &mut snapshot,
                &shape,
                &deployments,
                watcher::Event::Apply(web(json!({"matchLabels": {"app": "web"}}), 0))
            ),
            AppChange::Changed
        );
        // …a new selector selects other pods.
        assert_eq!(
            app_object_event(
                &mut snapshot,
                &shape,
                &deployments,
                watcher::Event::Apply(web(json!({"matchLabels": {"app": "web2"}}), 0))
            ),
            AppChange::Reshape
        );
    }

    #[test]
    fn an_object_missing_from_its_listing_is_gone() {
        let (mut snapshot, shape, group) = app_tree();
        // The expanded Kustomization: the tree is rebuilt.
        assert_eq!(
            app_object_gone(&mut snapshot, &shape, &group, "shop"),
            AppChange::Reshape
        );
        // Any other object reads NotFound.
        let deployments = WatchGroup {
            api_version: "apps/v1".into(),
            kind: "Deployment".into(),
            rows: HashMap::from([("web".to_string(), vec![1])]),
        };
        assert_eq!(
            app_object_gone(&mut snapshot, &shape, &deployments, "web"),
            AppChange::Changed
        );
        assert_eq!(snapshot.rows[1].status.health, ObjectHealth::NotFound);
    }

    #[test]
    fn a_new_or_deleted_pod_rebuilds_the_tree() {
        let (mut snapshot, _, _) = app_tree();
        let pods = HashMap::from([(("apps".to_string(), "web-1".to_string()), vec![2])]);
        let pod = |name: &str, phase: &str| {
            object(
                json!({"metadata": {"name": name, "namespace": "apps"}, "status": {"phase": phase}}),
            )
        };
        assert!(matches!(
            app_pod_event(
                &mut snapshot,
                &pods,
                watcher::Event::Apply(pod("web-1", "Pending"))
            ),
            AppChange::Changed
        ));
        assert_eq!(snapshot.rows[2].status.health, ObjectHealth::InProgress);
        assert!(matches!(
            app_pod_event(
                &mut snapshot,
                &pods,
                watcher::Event::Apply(pod("web-2", "Pending"))
            ),
            AppChange::Reshape
        ));
        assert!(matches!(
            app_pod_event(
                &mut snapshot,
                &pods,
                watcher::Event::Delete(pod("web-1", "Running"))
            ),
            AppChange::Reshape
        ));
    }
}
