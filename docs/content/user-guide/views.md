---
title: "Graph and Live Views"
linkTitle: "Graph and Live Views"
weight: 3
description: "Graph, workloads, inventory, app view, trace, HelmRelease values, events, pulse, and controller logs"
type: docs
---

Every view here updates live while it's open, and stops watching when you
leave it.

## Graph (`g`)

For Kustomizations, HelmReleases, ResourceSets, FluxInstances, and
ArtifactGenerators: the sources upstream and everything the resource manages
downstream. Managed objects are grouped: Flux resources a ResourceSet
produces appear as their own nodes, workloads gather in a **workload group**,
and everything else in a **resource group** counted by kind.

- `j` / `k` move the focus between nodes; the view follows it.
- `Enter` opens the focused node; `y` / `d` show its YAML / describe output.
- `Esc` returns to the graph from anything you opened there.

![Graph view](/images/graph-screenshot.png)

### Workloads

`Enter` on a workload group lists its Deployments, StatefulSets, and
DaemonSets, with live readiness. `Enter` on one opens its **workload detail**:

- rollout status, containers and images, pods with restarts, and events
- **Resources:** each pod's CPU and memory next to its request and limit, with
  a bar against the limit that turns amber at 75% and red at 90% (throttling
  / OOM risk). The source (kubelet, metrics-server, or requests only) is
  detected automatically — see [Pod usage metrics](/configuration/#pod-usage-metrics).
- `l` streams a pod's logs (a picker opens when there are several), `r`
  rolls out a restart, `Ctrl+d` deletes a pod so its controller replaces it.
  Restarts and deletes ask first.

![Workload detail with live CPU and memory against requests and limits](/images/workload-detail.png)

### Inventory

`Enter` on a resource group lists every object it holds with live
[kstatus](https://github.com/kubernetes-sigs/cli-utils/tree/master/pkg/kstatus)
health: `Current`, `InProgress`, `Failed`, `Terminating`, or `NotFound` /
`Forbidden` when it can't be read, with a message for anything short of
`Current`. `Enter` / `d` describes an object, `y` shows its YAML (Secret
values always redacted).

## App view (`a`)

What a Flux app deploys, from Flux's own records — nothing to list by hand.
`a` on a Kustomization, HelmRelease, ResourceSet, or FluxInstance opens it;
`:app ks/frontend hr/data/redis` opens one or more (`<kind>/<name>`, or
`<kind>/<namespace>/<name>`; `-n` sets where `<kind>/<name>` is found). Each
Flux object is a top-level row, with underneath it:

- **source** — its GitRepository / OCIRepository / Bucket; for a
  HelmRelease, the HelmChart (or OCIRepository) generated for it and the
  chart's repository.
- **applied** — everything in its inventory (`status.inventory`; for a
  HelmRelease without one, the latest release's manifest). Nested
  Kustomizations and HelmReleases are expanded the same way, four levels
  deep.
- **pod** — the pods each Deployment, StatefulSet, and DaemonSet runs.

```
STATUS      KIND              NAMESPACE       NAME                      ROLE     READY
Suspended   HelmRelease       flux-resources  podinfo                   app      True
Ready       └ HelmChart       flux-resources  flux-resources-podinfo    source   True
Ready       └ HelmRepository  flux-resources  podinfo                   source   True
Available   └ Deployment      flux-resources  podinfo                   applied  1/1
Running       └ Pod           flux-resources  podinfo-5bd855f496-xckck  pod      1/1
Current     └ Service         flux-resources  podinfo                   applied  -
```

STATUS is each kind's own state, coloured by health: Flux objects read as
the Flux list does — **Suspended** first, then Reconciling / Stalled /
Ready / Not Ready, with READY the `Ready` condition. Workloads say
Available / Progressing / Rolling out with READY their replicas; pods
their phase or what's stuck (`CrashLoopBackOff`, `ImagePullBackOff`) with
ready containers and restarts; Jobs Complete / Running / Failed with
completions. Other custom resources read their `Ready` condition, and kinds
with no status of their own (ConfigMaps, Services) are `Current`. The
inventory list uses the same states.

Every row's health is live. When an inventory or source changes (a new
commit adds or prunes objects) or a pod comes or goes, the tree is rebuilt,
so it follows the app as Git changes. The title counts what's failing or in
progress, and notes what couldn't be read (RBAC, Helm storage).

`Enter` / `d` describes a row, `y` shows its YAML; on a Kubernetes row `t`
traces it and `m` goes to the Flux object it hangs under. `/` filters. To
keep a set of apps one command away, name it in a
[view](/configuration/#views) with `apps`.

## Trace (`t`)

What manages a resource, step by step: its controller owners
(`Pod → ReplicaSet → Deployment`, the same chain MANAGED-BY follows), the Flux object that applies it
(Kustomization, HelmRelease, ResourceSet, or FluxInstance), and that object's
source and revision. A HelmRelease's source is the one helm-controller
generated for it — a HelmChart and its repository, or (for a Flux 2.10
OCIRepository chart template) the generated OCIRepository. When something other than Flux manages the chain —
Argo CD, Helm, another tool — or nothing does, the trace ends there and says
so.

`t` traces the selected resource, and any Kubernetes object Flux manages
([native resources](../native-resources/#to-the-flux-manager)). On a native
object's trace, `g` and `m` go on to its Flux manager's graph and detail.

`:trace <kind>/<name>` traces an object by name — anything, whatever manages
it:

- Flux kinds and aliases: `:trace ks/apps`, `:trace hr/podinfo`
- Exact Kubernetes kind names: `:trace Deployment/web`
- Plurals and short names (`:trace deploy/web`, `:trace sts/db`) need
  [native resources](../native-resources/) on
- `-n <namespace>` picks the namespace (`:trace deploy/flux-operator -n flux-system`).
  Without it, flux9s looks in the current namespace; watching all
  namespaces, it searches them — one match is traced, several are listed so
  you can pick with `-n`. With native resources on, cluster-scoped kinds
  (`:trace clusterrole/flux-view`) need no namespace.

![Trace view](/images/trace-screenshot.png)

## HelmRelease values (`v`)

The values Helm actually receives: every `spec.valuesFrom` ConfigMap and
Secret merged in order, then `spec.values` on top, exactly as helm-controller
builds them.

- Each source shows whether it was applied, skipped (optional and absent),
  or failed (missing, forbidden, invalid YAML); one failure doesn't hide the
  rest.
- `targetPath` follows Helm `--set` typing.
- Values from a Secret show as `<redacted>` until `x` reveals them.
- `/` searches, and `R` still reconciles the release.

## Events (`:events`)

A live feed of Kubernetes Events in the current namespace (or the whole
cluster after `:ns all`), newest first, Warnings highlighted. Flux emits an
event for every reconciliation, so this shows error detail the list's
MESSAGE column truncates.

- `/` filters by type, reason, object, namespace, source, or message.
- `Enter` opens the event's object; `y` / `d` work for any kind, and Flux
  keys (`t`, `g`, `s`, `R`, …) work when it's a watched Flux resource.

Each resource's describe view (`d`) also ends with its recent events.

## Pulse (`:pulse`)

"Is my GitOps pipeline healthy?" at a glance: ready / failed / suspended
totals per kind, the latest failures with their messages, Flux version and
install status from the FluxReport, and controller pod health.

## Controller logs (`:logs`)

Stream a Flux controller's logs: `:logs` picks a pod, `:logs <pod>` streams
one by name or prefix. Scrolling up pauses following and `G` resumes it, `/`
searches, and `c` switches container (the default container comes from the
`kubectl.kubernetes.io/default-container` annotation, as `kubectl logs`
does).
