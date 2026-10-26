---
title: "Native Resources (preview)"
linkTitle: "Native Resources"
weight: 4
description: "Browse and act on any Kubernetes kind or CRD"
type: docs
---

{{% alert title="Preview — off by default" color="info" %}}
Browsing native resources and CRDs is a **preview** while it matures, so
flux9s stays Flux-only unless you opt in:

- **For one session:** `:native` (again to turn it off)
- **Permanently:** `flux9s config set nativeResources true` (flux9s 1.2.0+)

While it's off, flux9s makes no discovery calls and no extra watches, and
`:deploy` and friends explain how to turn it on. Feedback is welcome — it
becomes the default once it has had some mileage.
{{% /alert %}}

## Browsing any kind {#browsing-any-kind}

`:<kind>` opens a live list of any kind the cluster serves — built-in
resources and third-party CRDs alike — by the names `kubectl` accepts: the
kind, its plural, or a short name (`:deploy`, `:po`, `:svc`, `:httproutes`).
When two API groups serve the same name, use `plural.group`
(`:clusters.postgresql.cnpg.io`).

- **Scope:** lists follow `:ns`; cluster-scoped kinds (nodes, CRDs) always
  show everything. A watch keeps the list live while it's on screen.
- **Columns:** STATUS is kstatus health (the same rules Flux health checks
  use). CRDs add the columns their authors declared — what `kubectl get`
  shows. MANAGED-BY says what manages each object ([below](#managed-by)).
- **Keys:** `d` describes, `y` shows YAML (Secret values redacted), `/`
  filters by name or namespace. The footer shows only the keys a kind
  supports:
  - **Deployments, StatefulSets, DaemonSets:** `Enter` opens the
    [workload detail](../views/#workloads), `r` restarts, `l` streams pod logs
  - **CronJobs:** `Enter` opens the workload detail
  - **Pods:** `l` streams logs, `Ctrl+d` deletes the pod so its controller
    replaces it
  - **Anything patchable:** `e` edits ([below](#editing-non-flux-objects))
  - **Anything Flux manages:** `t` traces it, `g` opens its manager's graph,
    `m` opens the manager ([below](#to-the-flux-manager))
- **Getting back:** `Esc` walks back through the lists you opened, and
  `:flux` returns to the Flux list from anywhere.

![A :deploy list across all namespaces, with what manages each Deployment](/images/kind-list.png)

Flux names keep their meaning — `:ks` and `:hr` still filter the Flux list —
and flux9s commands win over kind names (`:ns`, `:events`, `:logs`); use the
plural (`:namespaces`) for the kind. Kinds are discovered on connect and
after `:ctx`, and an unknown `:<kind>` re-checks, so a CRD installed while
flux9s runs is found on the next try.

Very large lists show the first 5,000 rows and say so — narrow with `:ns`
or `/`. Only what's displayed is kept in memory (listing Secrets holds no
secret data).

## MANAGED-BY {#managed-by}

What kind of manager owns each object, using the conventions the tools write
themselves; first match wins:

1. **Flux** — `Flux ks`, `Flux hr`, `Flux rset`, `Flux instance`
2. **Argo CD** — tracking annotation or instance label
3. **An owner chain** — controller-created objects are followed up
   (`Pod → ReplicaSet → Deployment`) to whatever manages the top, so a
   Flux-managed Deployment's pods read `Flux ks`. With nothing at the top,
   the top owner's kind is shown (e.g. `DaemonSet`).
4. **Helm** — release annotations, on objects nothing owns, and Helm's
   release storage Secrets (`owner=helm`)
5. Any other `app.kubernetes.io/managed-by`
6. `system` for what Kubernetes creates itself (`kube-root-ca.crt`, the
   `default` ServiceAccount, ServiceAccount tokens, default RBAC), else `-`

A Flux-labelled object reads `orphaned (ks)` (or `hr`, `rset`, `instance`)
when Flux no longer manages it. The list is still a watch; to notice an
owner's inventory changing (which sends no event for the object), it also
re-reads each Flux owner its rows name — one GET per owner, every 30
seconds, while the list is open. See
[unmanaged and orphaned objects](#unmanaged-and-orphaned).

Describe (`d`) and the workload detail show the full answer, e.g.
`Flux HelmRelease cert-manager/cert-manager (via ReplicaSet/cert-manager-857cf84654)`.

## Unmanaged and orphaned objects {#unmanaged-and-orphaned}

What Flux doesn't manage, and what it left behind:

| Command / key | Shows |
| --- | --- |
| `:unmanaged` | Objects in the namespace nothing manages, and orphaned ones |
| `:unmanaged all` | …plus those another tool manages (Argo CD, Helm, `app.kubernetes.io/managed-by`) |
| `:orphaned` | Only orphaned objects |
| `O` on a Kustomization, HelmRelease, ResourceSet, or FluxInstance | What that Flux object left behind, in every namespace (also `:orphaned ks/apps`) |

Add `-n <namespace>` or `-A` (all namespaces, plus cluster-scoped kinds) to
the commands. Each row says where the object stands and why:

- **Orphaned** — its Flux labels name a Kustomization / HelmRelease /
  ResourceSet / FluxInstance that **no longer exists**, or that **no longer
  lists it in its inventory** (`status.inventory`) — typically removed from
  source while `prune: false` or `kustomize.toolkit.fluxcd.io/prune: disabled`
  kept it. Flux won't touch it again.
- **Unmanaged** — no Flux, Argo CD, or Helm marks and no owner. DETAIL says
  who created it, from its `managedFields` (`created by kubectl`,
  `created by Terraform`, …).
- **Other** (with `all`) — the tool that does manage it.

`Enter`/`d` describes the object (Managed By says `orphaned — …` there too),
`y` shows its YAML, and `t` / `m` go to the Flux object an orphan names, while
it exists. Unlike the rest of flux9s, a sweep is **not a watch**: it lists
each kind once, metadata only, when you run it — nothing repeats in the
background. `Ctrl-r` rescans.

**How the verdict is reached.** A sweep lists every discovered kind once,
metadata only, and keeps top-level objects — anything with an owner
reference is its owner's concern (a Deployment's pods, an operator's
Secrets). Each Flux owner the objects name is read once. flux9s only calls
an object orphaned when it's sure: when the owner can't be read (RBAC,
timeout, another cluster), is still reconciling, or keeps no inventory, the
object stays Flux-managed. Objects younger than two minutes, Helm hooks, and
the HelmChart / OCIRepository a HelmRelease generates are never orphans.

**Never reported:** Kubernetes' own objects (as in MANAGED-BY's `system`),
Helm's release storage, churn kinds (Events, Leases, Endpoints,
EndpointSlices, ControllerRevisions), Nodes, and the `kube-system`,
`kube-public`, and `kube-node-lease` namespaces. Hide anything else that's
expected with [`ownership.ignore`](/configuration/#ownership).

## From an object to its Flux manager {#to-the-flux-manager}

When Flux manages an object — directly, or through its owner chain (a Pod
whose Deployment a Kustomization applied) — three keys lead from it to that
Flux object. They work in kind lists, native YAML / describe / trace views,
and the workload detail, and show in the footer once the manager is known
(owner chains resolve in the background):

| Key | Action |
| --- | --- |
| `t` | [Trace](../views/#trace-t) it: owners, the Flux object, its source and revision |
| `g` | The Flux object's [graph](../views/#graph-g), focused on the object (or the owner it applied) — marked by name in a workload group, by its kind's line in a resource group |
| `m` | Open the Flux object — its detail, or its describe when it's outside the namespaces you watch |

Objects that Argo CD, Helm, or nothing manages don't offer them;
`:trace <kind>/<name>` still traces those, and says where the chain stops.

## Editing non-Flux objects {#editing-non-flux-objects}

`e` edits any object whose kind the API server lists as patchable — in kind
lists, inventory and event rows, and native YAML / describe views — through
the same [editor flow](../flux-resources/#editing) as Flux resources. RBAC
still decides on save.

Before the editor opens, flux9s checks who manages the object and asks
first when something does:

- **Flux:** "its next reconcile will revert your change". If the managing
  Kustomization, HelmRelease, ResourceSet, or FluxInstance is **suspended**,
  it says so instead: your change stays until it's resumed. flux9s reads the
  manager from the cluster when it's outside the namespaces you're watching.
- **Helm, Argo CD, or another manager:** that tool may overwrite or revert
  the change.
- **A controller owner** with nothing above it: its controller may overwrite
  the change.

![The warning before editing a Flux-managed Deployment](/images/edit-warning.png)

`y` opens the editor; `n` or `Esc` backs out. Unmanaged objects open
straight away.

**Secrets are never editable** — flux9s only holds their values redacted, so
saving would overwrite them. `e` suggests the matching `kubectl edit`
instead, as it does for kinds the API won't patch.

## All namespaces

With native resources on, the `:ns` menu lists every namespace in the
cluster, with namespaces holding Flux resources first and marked `◆ flux`.
If your RBAC can't list namespaces, it shows the Flux namespaces.

## Flux-adjacent CRDs

CRDs labeled `app.kubernetes.io/part-of=flux` (Flagger, tofu-controller,
ExternalArtifact controllers) can join the Flux list itself, view-only — turn
it on with `discoverFluxResources` or `:discover` for a session. See
[Discovering Flux-adjacent kinds](/configuration/#discovering-flux-adjacent-resource-kinds).
