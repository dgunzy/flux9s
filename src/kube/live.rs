//! Watch-driven live data for the drill-down views.
//!
//! The workload detail and the inventory breakdown show cluster state that
//! changes under the user (a rollout, a pod being replaced, an object turning
//! healthy). Rather than a one-shot fetch, each runs as a long-lived task on
//! the watch API that pushes a fresh snapshot whenever something it shows
//! changes. The tasks run only while their view is open: the app aborts them
//! on leave, and a closed channel ends them too.

use std::collections::HashMap;
use std::fmt::Debug;
use std::time::Duration;

use futures::stream::BoxStream;
use futures::{FutureExt, StreamExt};
use k8s_openapi::api::core::v1::{Event as CoreEvent, Pod};
use kube::core::DynamicObject;
use kube::runtime::{WatchStreamExt, watcher};
use kube::{Api, Resource};
use serde::de::DeserializeOwned;
use tokio::sync::mpsc::UnboundedSender;

use crate::kube::inventory::InventoryEntry;
use crate::kube::object_status::{ObjectHealth, ObjectStatus, compute_object_status};
use crate::kube::workloads::{WorkloadData, fetch_workload_data};

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
        match serde_json::to_value(&obj) {
            Ok(mut value) => {
                // List/watch items omit apiVersion/kind, which the
                // kind-specific status rules key on.
                if let Some(map) = value.as_object_mut() {
                    map.insert("apiVersion".into(), group.api_version.clone().into());
                    map.insert("kind".into(), group.kind.clone().into());
                }
                compute_object_status(&value)
            }
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
}
