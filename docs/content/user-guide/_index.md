---
title: "User Guide"
linkTitle: "User Guide"
weight: 3
description: "Learn how to use flux9s to monitor and manage Flux resources"
toc: true
type: docs
---

## Video Demos

{{< rawhtml >}}

<div class="mb-4">
  <h3>Graph View / Favorite Demo</h3>
  <div class="ratio ratio-16x9" style="background: transparent;">
    <video autoplay loop muted playsinline class="w-100 h-100" style="object-fit: contain; background: transparent;" onerror="this.style.display='none'; this.nextElementSibling.style.display='block';">
      <source src="/images/demo-graph.mp4" type="video/mp4">
      Your browser does not support the video tag.
    </video>
    <div style="display:none; padding: 2rem; text-align: center; background: #f8f9fa; color: #6c757d;">
      <i class="fas fa-video fa-3x mb-3"></i>
      <p class="mb-0"><strong>Graph View Demo</strong></p>
      <p class="small mb-2">Visualize resource relationships and dependencies</p>
      <p class="small text-muted">Video playback is not available. The demo shows how the graph view visualizes resource relationships and dependencies.</p>
    </div>
  </div>
  <p>See how the graph view visualizes resource relationships and dependencies.</p>
</div>

<div class="mb-4">
  <h3>Theme Selection Demo</h3>
  <div class="ratio ratio-16x9" style="background: transparent;">
    <video autoplay loop muted playsinline class="w-100 h-100" style="object-fit: contain; background: transparent;" onerror="this.style.display='none'; this.nextElementSibling.style.display='block';">
      <source src="/images/demo-skin.mp4" type="video/mp4">
      Your browser does not support the video tag.
    </video>
    <div style="display:none; padding: 2rem; text-align: center; background: #f8f9fa; color: #6c757d;">
      <i class="fas fa-video fa-3x mb-3"></i>
      <p class="mb-0"><strong>Theme Selection Demo</strong></p>
      <p class="small mb-2">Interactive theme selection with live preview</p>
      <p class="small text-muted">Video playback is not available. The demo shows the interactive theme selection submenu with live preview in action.</p>
    </div>
  </div>
  <p>Watch the interactive theme selection with live preview in action.</p>
</div>
{{< /rawhtml >}}

## Navigation

Use these keyboard shortcuts to navigate flux9s:

| Key       | Action                                                  |
| --------- | ------------------------------------------------------- |
| `j` / `k` | Navigate up/down                                        |
| `:`       | Command mode (e.g., `:kustomization`, `:gitrepository`) |
| `Enter`   | View resource details                                   |
| `/`       | Filter resources by name (in list views) or search text (in YAML/describe/trace views) |
| `n` / `N` | Jump to next/previous search match (in YAML/describe/trace views) |
| `Shift+N` | Sort list by name (press again to reverse, third press restores default order) |
| `Shift+A` | Sort list by age                                        |
| `Shift+T` | Sort list by type                                       |
| `Shift+S` | Sort list by status (problems first)                    |
| `s`       | Suspend reconciliation                                  |
| `r`       | Resume reconciliation                                   |
| `R`       | Reconcile resource                                      |
| `y`       | View resource YAML                                      |
| `d`       | View describe output                                    |
| `e`       | Edit resource in system editor (disabled in read-only mode) |
| `f`       | Toggle favorite                                         |
| `g`       | View resource graph (Kustomization, HelmRelease, etc.)  |
| `h`       | View reconciliation history                             |
| `v`       | HelmRelease effective values                            |
| `t`       | Trace ownership chain                                   |
| `W`       | Reconcile with source                                   |
| `Ctrl+d`  | Delete resource (with confirmation)                     |
| `?`          | Show/hide help                                          |
| `q` / `Esc`  | Go back; shows a quit prompt when at the root view      |
| `Q`          | Quit immediately (no prompt)                            |
| `Ctrl+C`     | Quit (also `:q`, `:quit`)                               |
| `Tab`        | Autocomplete command                                    |

## Commands

Type these commands in command mode (press `:`):

| Command            | Description                              |
| ------------------ | ---------------------------------------- |
| `:ctx <name>`      | Switch to a different Kubernetes context |
| `:ctx`             | Open interactive context selection menu  |
| `:context <name>`  | Alias for `:ctx <name>`                  |
| `:ns`              | Namespace picker (with `:native`: every namespace, Flux ones first `◆ flux`) |
| `:ns <namespace>`  | Switch to a specific namespace           |
| `:namespace <ns>`  | Alias for `:ns <namespace>`              |
| `:ns all`          | View all namespaces                      |
| `:all`             | Show all resources (clear filters)       |
| `:healthy`         | Show only healthy resources              |
| `:unhealthy`       | Show only unhealthy resources            |
| `:suspended`       | Show only suspended resources            |
| `:favorites`       | View favorite resources                  |
| `:fav`             | Alias for `:favorites`                   |
| `:events`          | Live Kubernetes events feed              |
| `:ev`              | Alias for `:events`                      |
| `:pulse`           | Cluster health dashboard                 |
| `:dashboard`       | Alias for `:pulse`                       |
| `:logs`            | Controller log viewer (pod submenu)      |
| `:logs <pod>`      | Stream a controller pod by name/prefix   |
| `:skin <name>`     | Change theme/skin (direct)               |
| `:skin`            | Open interactive theme selection menu    |
| `:readonly`        | Toggle readonly mode                     |
| `:read-only`       | Alias for `:readonly`                    |
| `:discover`        | Toggle CRD discovery for this session    |
| `:discovery`       | Alias for `:discover`                    |
| `:help`            | Show/hide help                           |
| `:trace <res>`     | Trace ownership chain for a resource     |
| `:q` or `:q!`      | Quit application                         |
| `:quit` or `:exit` | Aliases for `:q`                         |

### Resource Type Commands

You can filter by resource type using commands like:

- `:kustomization` or `:ks` - View only Kustomization resources
- `:gitrepository` or `:gitrepo` - View only GitRepository resources
- `:helmrelease` or `:hr` - View only HelmRelease resources
- `:fluxinstance` or `:fi` - View only FluxInstance resources
- `:resourceset` or `:rset` - View only ResourceSet resources
- `:ocirepository` or `:oci` - View only OCIRepository resources
- And many more - use `Tab` for autocomplete to see all available resource types

All resource type commands support autocomplete with `Tab` key.

With [`discoverFluxResources`](../configuration/#discovering-flux-adjacent-resource-kinds)
enabled, CRDs labeled `app.kubernetes.io/part-of=flux` (the Flux Operator's
convention) also get commands here — the kind name, plural, and `kubectl`
short names all work, and the kinds appear in the unified list (view-only).

Use `:discover` to turn that discovery on or off without restarting flux9s. It
is the runtime sibling of `:readonly`: it applies to the current session only
(including after a context switch) and does not write the config file — use
`flux9s config set discoverFluxResources true` to make it the default. Turning
discovery off deregisters the discovered kinds, drops their `:` commands, and
removes their resources from the list.

### Browsing Any Kind (`:<kind>`) — preview {#browsing-any-kind}

{{% alert title="Preview — off by default" color="info" %}}
Browsing native resources and CRDs is a **preview** while it matures. It is
**off by default**, so flux9s stays Flux-only unless you opt in:

- **Try it for a session:** `:native` (again to turn it off)
- **Keep it on:** `flux9s config set nativeResources true`

With it off, `:deploy` and other Kubernetes kinds tell you how to turn it on.
Feedback welcome — it will become the default once it has had some mileage.
{{% /alert %}}

Beyond Flux, `:<kind>` opens a live list of **any** kind the cluster serves —
native resources and third-party CRDs alike — using the names `kubectl`
accepts: the kind, its plural, or a short name (`:deploy`, `:po`, `:svc`,
`:httproutes`). When two API groups serve the same name, use the fully
qualified `plural.group` form (`:clusters.postgresql.cnpg.io`).

- Lists follow the current namespace (`:ns`); cluster-scoped kinds (nodes,
  CRDs, …) always show everything. Updates arrive live through a watch that
  runs only while the list is on screen.
- STATUS uses the same kstatus health as the inventory view. CRDs also show
  the columns their author declared (`additionalPrinterColumns`, what
  `kubectl get` shows); when a CRD brings its own Status column, the health
  column is labelled HEALTH.
- `d` describes the selected object, `y` shows its YAML (Secret values
  redacted), and `/` filters by name or namespace (`Esc` clears an applied
  filter first). `Enter` describes too — except for workloads, below.
- **Same actions as the workload views.** Keys come from what each kind
  supports, and the footer only shows what applies:
  - Deployments, StatefulSets, DaemonSets: `Enter` opens the same workload
    detail as the graph drill-down (rollout, pods, events, CPU/memory), `r`
    restarts, `l` streams pod logs.
  - CronJobs: `Enter` opens the workload detail.
  - Pods: `l` streams logs (container picker included), `Ctrl+d` deletes the
    pod so its controller replaces it.
  - Writes ask for confirmation and are blocked in read-only mode.
- **MANAGED-BY** shows *what kind* of manager owns each object, following
  the conventions the tools write themselves, first match wins:
  1. **Flux** — `Flux ks`, `Flux hr`, `Flux rset`, or `Flux instance`
     (Kustomization / HelmRelease / ResourceSet / FluxInstance labels, as the
     Flux Operator UI reads them)
  2. **Argo CD** — `Argo CD` (tracking annotation or instance label)
  3. **Kubernetes owner chain** — objects created by a controller are traced
     up (`Pod → ReplicaSet → Deployment`) to whatever manages the top, so a
     Flux-managed Deployment's pods read `Flux ks`. With no manager at the
     top, the top owner's kind is shown (e.g. `DaemonSet`).
  4. **Helm** — `Helm` (release annotations, on objects nothing owns)
  5. Any other `app.kubernetes.io/managed-by` value, else `-` (unmanaged)

  Owner chains resolve in the background (cached per owner, at most 300
  lookups per batch of changes, so long-open lists keep resolving new
  owners) — rows show their direct owner's kind until then.
- **Managed By** — `Enter` (describe or workload detail) shows the full
  answer, e.g. `Flux HelmRelease cert-manager/cert-manager (via
  ReplicaSet/cert-manager-857cf84654)` or `not managed — top owner
  Deployment/coredns`. Describe shows the object and its events at once and
  fills Managed By in when the owner chain resolves (`resolving…` until
  then, `manager lookup timed out` after 3s on a slow API server). The
  cursor stays on the same object as the list updates live.
- Very large lists show the first 5,000 rows (sorted by namespace/name) and
  say so in the title — narrow with `:ns` or `/`. Only the displayed text of
  each object is kept in memory (listing Secrets holds no secret data).
- Flux kinds keep their meaning: `:ks`, `:hr`, … still filter the Flux list.
  flux9s commands also win over kind names — `:ns`, `:events`, and `:logs`
  keep their flux9s behaviour; use the plural (`:namespaces`) for the kind.
- Lists start watching when opened, so returning to one with `Esc` re-lists
  it (typically under a second).
- **Getting back:** `Esc` walks back through the lists you opened
  (`:deploy` → `:po` → `Esc` → `Esc`), and `:flux` returns to the Flux view
  from anywhere. While browsing, the header reads
  `Browsing: Cluster › deployments (27)` with the Flux total alongside.

Any kind the API server serves as a listable, watchable top-level resource
is browsable — in whichever served version it appears (preferred first), so
CRDs that keep some kinds in an older version (e.g. metallb's
`IPAddressPool` in `v1beta1`) work too. Kinds are discovered when flux9s
connects and after `:ctx`; an unknown `:<kind>` triggers a re-check (at most
every 30s), so a CRD installed while flux9s runs is found on the next try.

Non-Flux kinds are view-only for now: `e` explains that and suggests the
equivalent `kubectl edit` command.

While the preview is off (the default), flux9s makes no discovery calls and
no extra watches — Flux-adjacent CRD discovery (`discoverFluxResources`,
`:discover`) included, since that is an extra watch too — and the `:ns`
picker keeps its Flux-only list.

## Interactive Submenus

Some commands open interactive selection menus when used without arguments, providing an easier way to select from available options.

#### Context Submenu (`:ctx`)

When you type `:ctx` and press Enter without specifying a context name, flux9s displays an interactive menu of available Kubernetes contexts. The current context is marked with "(current)".

Switching context starts fresh on the new cluster: watchers restart, namespace
hotkeys and the `:ns` picker's Flux namespaces are re-discovered, and any view
showing an object from the old cluster (describe, workload detail, logs,
events, graph, kind lists) returns to the Flux list.

**Navigation:**

- `j` / `k` or `↓` / `↑` - Navigate through options
- `Enter` - Select the highlighted context
- `Esc` - Cancel and close submenu

The submenu appears as a centered overlay on top of the current view, making it easy to see and select your desired context without needing to remember exact names.

All submenus support **filtering with the same keys as the resource list**: press `/` to start filtering, type to narrow the list, `Enter` to apply the filter, and `Esc` to cancel it (a second `Esc` closes the menu). `PageUp`/`PageDown` (or `Ctrl+f`/`Ctrl+b`) page through long lists, and `:` closes the menu straight into command mode.

#### Theme Submenu (`:skin`)

When you type `:skin` and press Enter without specifying a theme name, flux9s displays an interactive menu of available themes with live preview.

![Theme Submenu](/images/skin-submenu.png)

**Features:**

- **Live Preview**: Theme changes immediately as you navigate
- **Current Theme**: Marked with "(current)"
- **Built-in Themes**: Embedded themes marked with "[built-in]"
- **17 Built-in Themes**: Includes popular themes like dracula, nord, monokai, gruvbox-dark, and more

**Navigation:**

- `j` / `k` or `↓` / `↑` - Navigate through themes (with live preview)
- `Enter` - Apply theme temporarily (session only)
- `s` - Save theme to config file (persists across sessions)
- `Esc` - Cancel and restore original theme

The submenu saves themes to `ui.skin` in normal mode, or `ui.skinReadOnly` when readonly mode is enabled.

With native resources on (`:native`), the `:ns` picker lists every namespace in the cluster: `all` first, then
every namespace holding Flux resources (marked `◆ flux`, or `[flux]` with
`ui.noIcons`), then any other hotkey namespaces and the rest alphabetically.
If the picker opens before the cluster's namespaces have loaded, its title
says `loading all namespaces…` and the list fills in place, keeping your
selection and filter. If your RBAC can't list namespaces, it shows the Flux
namespaces as before. With `nativeResources: false` it lists only the Flux
namespaces (plus any configured hotkeys and the current namespace).

## Health Filtering

Filter resources by health status:

- **`:healthy`** - Show only healthy resources (ready=true, not suspended, or null status)
- **`:unhealthy`** - Show only unhealthy resources (ready=false or suspended=true)
- **`:suspended`** - Show only suspended resources (suspended=true)
- **`:all`** - Clear health filter and show all resources

The header displays a health percentage indicator showing the overall health of your resources. The indicator uses color coding:

- **Green (●)** - 90% or higher health
- **Yellow (⚠)** - 70-89% health
- **Red (✗)** - Below 70% health

## Sorting

Sort the resource list k9s-style with shift-key shortcuts: `Shift+N` (name), `Shift+A` (age), `Shift+T` (type), or `Shift+S` (status, problems first). Press the same key again to reverse the order, and a third time to restore the default namespace/type/name ordering. The active sort column is marked with an arrow (`↑`/`↓`) in the table header, and favorites always stay grouped at the top.

## Searching Text Views

Inside the YAML (`y`), describe (`d`), and trace (`t`) views, press `/` to search:

- Type a query and press `Enter` to jump to the first match (matching is case-insensitive)
- `n` / `N` - Jump to the next/previous match
- `Esc` - Clear the search (press again to leave the view)

The view title shows the active query and match position (e.g., `/spec (2/7)`), and matching lines are highlighted.

## Watch Status Banner

flux9s watches the cluster continuously. If the connection to the API server degrades (network outage, VPN reconnect, laptop sleep), a red banner appears in the top-right corner of the resource list:

```
⚠ Watch degraded (N) — data may be stale, reconnecting...
```

Watchers retry automatically with exponential backoff; the banner disappears as soon as the watch streams recover. While the banner is visible, the displayed resources may be out of date.

## Resource Views

### Graph View (`g`)

Visualize resource relationships and dependencies. Shows upstream sources and downstream managed resources.

**Supported resource types:**

- Kustomization
- HelmRelease
- ArtifactGenerator
- FluxInstance
- ResourceSet

The graph view displays:

- **Upstream sources** (GitRepository, HelmRepository, etc.)
- **Managed resources** (workloads, ConfigMaps, Services, etc.) — for
  Kustomizations, HelmReleases, ResourceSets, and FluxInstances alike; a
  ResourceSet's produced Flux resources appear as individual navigable nodes,
  and arbitrary kinds (Namespaces, CRDs, custom resources) aggregate into
  the resource group
- **Resource groups** (aggregated by type)
- **Workload groups** (aggregated workloads with status)

**Navigating the graph:**

- `j` / `k` (or `↓` / `↑`) - Move the highlighted focus between nodes; the view scrolls to keep the focused node visible.
- `Enter` - Open the focused node's resource in the detail view. External upstream URLs aren't directly openable; aggregate nodes drill into their members (see below).
- `y` / `d` - View the focused node's YAML or describe output directly, including managed workloads (Deployments, Services, etc.).
- `Enter` on a **resource group** - Drill into the inventory breakdown: a table of the kind, namespace, name, and live health of every resource the group aggregates (cluster-scoped entries show `<cluster>` as their namespace), titled with the per-kind counts. STATUS follows the kstatus rules Flux health checks use (`Current`, `InProgress`, `Failed`, `Terminating`, plus `NotFound`/`Forbidden` when the object can't be read), and MESSAGE explains anything short of `Current`. `Enter` or `d` describes the selected object (with its events), `y` shows its YAML — Secret values are always shown as `<redacted>` — and `Esc` walks back to the inventory, then the graph. Statuses are live: each kind is watched while the breakdown is open, so a rollout or a deleted object shows up without reopening it.
- `Enter` on a **workload group** - Drill into the workload list: `Enter` on a workload opens its detail (rollout status, containers and images, pods with restarts, events), and `l` streams a pod's logs. `Esc` walks back up the chain.
  - `r` (list or detail) rolls out a restart of a Deployment, StatefulSet, or DaemonSet — the same `kubectl.kubernetes.io/restartedAt` pod-template annotation `kubectl rollout restart` sets.
  - `Ctrl+d` in the detail deletes a pod so its controller replaces it; with several pods a picker opens first.
  - Both ask for confirmation and are blocked in read-only mode.
  - The detail is live: the workload, its pods, and its events are watched while it is open, so a restart's new pods appear and old ones drain away as it happens.
  - A **Resources** section shows each pod's CPU and memory use next to its request and limit, with a bar filled against the limit (`│` marks the request) that turns amber at 75% and red at 90% of the limit (throttling / OOM risk). CPU is always in millicores so the three compare directly. The source — kubelet, metrics-server, or none (requests/limits only) — is picked automatically and named in the section header; see [Pod Usage Metrics](../configuration/#pod-usage-metrics).
- `Esc` / `Backspace` - Return to the graph (when you opened a view from it), then back to the resource list.

Focus starts on the resource you opened the graph from, so you can immediately walk its sources and dependencies.

### HelmRelease Values (`v`)

`v` on a HelmRelease shows the values Helm actually receives — every
`spec.valuesFrom` ConfigMap/Secret merged in order, then `spec.values` on top —
exactly as helm-controller builds them:

- A sources list shows each reference (`Kind/name [valuesKey → targetPath]`)
  and whether it was applied, skipped (optional and absent), or failed (not
  found, missing key, forbidden, invalid YAML). A failed source doesn't hide
  the rest — the view shows what the other sources produce.
- `targetPath` values follow Helm `--set` typing (quoted strings, `true`/`false`,
  integers, `{a,b}` lists, `\.` for literal dots).
- Values that come from a Secret show as `<redacted>` unless something later
  overrides them; `x` reveals / hides them (the title reads `[secrets shown]`).
- The view is live: changes to the release or any referenced ConfigMap/Secret
  update it in place. `/` searches, and Flux keys like `R` still act on the
  release.

### Reconciliation History (`h`)

View reconciliation history for resources that track it.

**Supported resource types:**

- FluxInstance
- ResourceSet
- Kustomization
- HelmRelease

The history view shows:

- Timestamp of each reconciliation
- Revision information
- Status (Success/Failed/Unknown)
- Messages from reconciliation events

### Favorites (`f`)

Mark frequently accessed resources as favorites for quick access.

- Press `f` on a resource to toggle favorite status
- Use `:favorites` or `:fav` command to view all favorites
- Favorites are saved to your configuration file
- Favorites appear first in resource lists

### Events View (`:events`)

A live feed of Kubernetes Events in the current namespace scope — the
"what is Flux doing right now" view. Flux controllers emit an Event for every
reconciliation success and failure, so this surfaces error detail that the
resource list's MESSAGE column truncates.

- The feed follows your namespace scope: the current namespace by default, or
  the whole cluster after `:ns all` (a NAMESPACE column appears)
- Events are streamed in real time, newest first, with Warnings highlighted
- `/` filters by type, reason, object, namespace, source, or message text
- `Enter` on an event jumps to the involved resource's detail view when it is
  a Flux resource flux9s watches, and opens its describe view otherwise (any
  kind, resolved through API discovery); `Esc` returns to the events feed
- Resource keys act on the selected event's involved object directly: `y`
  (YAML) and `d` (describe) work even for non-Flux objects like Pods and
  Deployments, while `t`/`g`/`h` and operations (`s`/`r`/`R`) work when the
  object is a watched Flux resource. `Esc` from any of these returns to the
  events feed
- `Esc` from the feed returns to the resource list and stops the events watch —
  events are only streamed while the view is open, so there is no overhead the
  rest of the time

Events also appear in the describe view (`d`): each resource's describe output
ends with a kubectl-style Events section listing that resource's recent events.

### Pulse Dashboard (`:pulse`)

An at-a-glance answer to "is my GitOps pipeline healthy?", updating in real
time from the watch state:

- Ready / failed / suspended totals and a per-kind breakdown, scoped to the
  current namespace (or the whole cluster with `:ns all`)
- The most recent failures with their reconcile messages, for fast triage
  (jump to the full list with `:unhealthy`)
- Flux distribution info from the FluxReport — version, install status,
  entitlement, operator version, and sync source — plus live controller
  pod health

### Controller Logs (`:logs`)

Stream the logs of any Flux controller pod without leaving flux9s — the next
step after Events when a reconciliation fails in a way conditions don't
explain.

- `:logs` opens a submenu of the discovered controller pods (readiness shown),
  `:logs <pod>` streams one directly by exact name or unique prefix
- The stream tails recent lines and follows new output live; scrolling up
  (`j`/`k`, page keys) pauses following and `G` jumps back to the newest line
- `/` searches the log buffer with `n`/`N` to cycle matches
- Multi-container pods stream their default container — the
  `kubectl.kubernetes.io/default-container` annotation, else the first
  container, as `kubectl logs` does — named in the title; `c` picks another
- The buffer is bounded (oldest lines evicted), and the stream runs only while
  the view is open — `Esc` stops it and returns to where you came from

### ResourceSet Steps

Step-based ResourceSets (Flux Operator v0.53+) show their ordered steps in the
detail view (`Enter`), with each step's phase — done, applying, failed, or
pending — derived live from the reconciliation status, alongside the step's
resource count, template marker, and timeout. The reconciliation history view
(`h`) includes the step count of each snapshot.

## Operations

Perform actions on selected resources:

| Key | Operation              | Valid For                                                                                       |
| --- | ---------------------- | ----------------------------------------------------------------------------------------------- |
| `s` | Suspend reconciliation | GitRepository, OCIRepository, HelmRepository, Kustomization, HelmRelease, ImageUpdateAutomation |
| `r` | Resume reconciliation  | GitRepository, OCIRepository, HelmRepository, Kustomization, HelmRelease, ImageUpdateAutomation |
| `R` | Reconcile resource     | All Flux resources (cannot reconcile suspended resources)                                       |
| `W` | Reconcile with source  | Kustomization, HelmRelease only                                                                 |
| `Ctrl+d` | Delete resource   | All Flux resources (with confirmation)                                                          |

**Note:** Suspend and Resume operations are only available for resources that support the `spec.suspend` field. Reconcile operations will fail if the resource is currently suspended.

## Editing Resources

Press `e` on any resource to open it in your system editor. flux9s fetches the full resource YAML, opens it in a temporary `.yaml` file, and applies your changes via Server-Side Apply when you save and quit.

**Editor selection** (highest priority first):

1. `FLUX9S_EDITOR` environment variable
2. `editor` config key (`flux9s config set editor vim`)
3. `$VISUAL` environment variable
4. `$EDITOR` environment variable
5. Fallback: `vi`

GUI editors (VS Code, Sublime Text, Atom, Zed) are automatically invoked with `--wait` so flux9s blocks until you close the editing window.

**Note:** Editing is disabled in read-only mode. The previous view (YAML, detail, graph, or list) is restored after saving, cancelling, or on error.

## Terminal Commands

Configure flux9s from the command line:

```bash
# Use a specific kubeconfig file
flux9s --kubeconfig /path/to/kubeconfig

# Show all config options
flux9s config --help

# Set a configuration value
flux9s config set {KEY} {VALUE}

# Set a skin for readonly mode
flux9s config set ui.skinReadOnly rose-pine

# Import and set a skin
flux9s config skins set navy.yaml

# Show the installed version
flux9s --version

# Generate shell completions (bash, zsh, fish, elvish, powershell)
flux9s completions zsh > "${fpath[1]}/_flux9s"   # zsh
flux9s completions bash > /etc/bash_completion.d/flux9s   # bash
```

{{% alert title="Skin Compatibility" color="warning" %}}
Not all K9s skins are compatible with flux9s. flux9s skins follow a similar format but may require adjustments to work properly.
{{% /alert %}}

## Supported Resource Types

flux9s supports all Flux CD resources from the official Flux controllers and Flux Operator:

flux9s resolves the API version for each resource at runtime, so it stays compatible as Flux promotes CRDs across versions.

### Source Controller (`source.toolkit.fluxcd.io`)

- **GitRepository** - Git repository sources
- **OCIRepository** - OCI artifact sources
- **HelmRepository** - Helm chart repositories
- **Bucket** - S3-compatible bucket sources
- **HelmChart** - Helm chart artifacts
- **ExternalArtifact** - External artifact sources

### Source Watcher (`source.extensions.fluxcd.io`)

- **ArtifactGenerator** - Artifact generation

### Kustomize Controller (`kustomize.toolkit.fluxcd.io`)

- **Kustomization** - Kustomize-based deployments

### Helm Controller (`helm.toolkit.fluxcd.io`)

- **HelmRelease** - Helm release management

### Image Reflector Controller (`image.toolkit.fluxcd.io`)

- **ImageRepository** - Container image repositories
- **ImagePolicy** - Image version policies

### Image Automation Controller (`image.toolkit.fluxcd.io`)

- **ImageUpdateAutomation** - Automated image updates

### Notification Controller (`notification.toolkit.fluxcd.io`)

- **Alert** - Alert configurations
- **Provider** - Notification providers
- **Receiver** - Webhook receivers

### Flux Operator (`fluxcd.controlplane.io`)

- **ResourceSet** - Declarative resource sets
- **ResourceSetInputProvider** - Input providers for ResourceSets
- **FluxReport** - Flux reports
- **FluxInstance** - Flux instances

## Screenshots

{{< rawhtml >}}

<div class="mb-4">
  <h3>Main View</h3>
  <div class="mb-3">
    <img src="/images/screenshot.png" alt="flux9s screenshot" class="img-fluid">
  </div>
  <p>The main resource view showing all Flux resources in your cluster with real-time updates.</p>
</div>

<div class="mb-4">
  <h3>Trace View</h3>
  <div class="mb-3">
    <img src="/images/trace-screenshot.png" alt="flux9s trace" class="img-fluid">
  </div>
  <p>Visualize resource relationships and ownership chains to understand dependencies.</p>
</div>

<div class="mb-4">
  <h3>Filter View</h3>
  <div class="mb-3">
    <img src="/images/filter-screenshot.png" alt="flux9s filter" class="img-fluid">
  </div>
  <p>Quickly find resources by name using the filter feature. Press <code>/</code> to start filtering.</p>
</div>

<div class="mb-4">
  <h3>Graph View</h3>
  <div class="mb-3">
    <img src="/images/graph-screenshot.png" alt="flux9s graph" class="img-fluid">
  </div>
  <p>Visualize resource relationships and dependencies in a graph format.</p>
  <p>Shows upstream sources and downstream managed resources for Kustomization, HelmRelease, ArtifactGenerator, FluxInstance, and ResourceSet.</p>
</div>
{{< /rawhtml >}}
