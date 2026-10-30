---
title: "Configuration"
linkTitle: "Configuration"
weight: 4
description: "Configure flux9s to suit your needs"
toc: true
type: docs
---

## Configuration File Location

flux9s stores its configuration in a YAML file. The location depends on your operating system:

| OS          | Location                       |
| ----------- | ------------------------------ |
| **Linux**   | `~/.config/flux9s/config.yaml` |
| **macOS**   | `~/.config/flux9s/config.yaml` |
| **Windows** | `%APPDATA%\flux9s\config.yaml` |

To find the exact path on your system:

```bash
flux9s config path
```

## Complete Configuration Reference

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `readOnly` | bool | `true` | Disable all write operations |
| `defaultNamespace` | string | `flux-system` | Starting namespace (`all` or `-A` = all namespaces) |
| `defaultControllerNamespace` | string | `flux-system` | Namespace where Flux controllers run |
| `defaultResourceFilter` | string | *(none)* | Flux list filter at startup: a kind (`ks`), several (`ks,hr`), or a [view](#views) name |
| `defaultSort` | string | *(none)* | Flux list sort at startup: `name`, `age`, `type`, `status`; `-age` or `age:desc` reverses ([details](#default-sort)) |
| `views` | map | *(empty)* | Named Flux list views: `views.<name>.kinds` lists Flux kinds (and, with native resources on, Kubernetes kinds), with optional `namespace` and `sort`; or `views.<name>.apps` lists Flux objects to show with everything they deploy; `:<name>` opens it ([details](#views)) |
| `ownership.ignore` | list | *(empty)* | Objects `:unmanaged` / `:orphaned` never report: `group` / `kind` / `namespace` / `name` glob rules ([details](#ownership)) |
| `ownership.ignoreNamespaces` | string[] | `kube-system`, `kube-public`, `kube-node-lease` | Namespaces those sweeps skip; `[]` sweeps every namespace ([details](#ownership)) |
| `connectTimeoutSeconds` | integer | `10` | Startup Kubernetes API health-check timeout in seconds |
| `discoverFluxResources` | boolean | `false` | Opt-in dynamic discovery of Flux-adjacent CRDs (see below); needs `nativeResources: true` |
| `nativeResources` | boolean | `false` | **Preview.** Browse native Kubernetes resources and CRDs with `:<kind>` ([details](/user-guide/native-resources/#browsing-any-kind)); off keeps flux9s Flux-only — no API discovery or extra watches, including `discoverFluxResources`. Try it per session with `:native` |
| `metricsSource` | string | `auto` | Pod CPU/memory source: `auto`, `kubelet`, `metrics-server`, `none` (see below) |
| `editor` | string | *(none)* | Editor command for `e` keybinding; falls back through `$VISUAL`, `$EDITOR`, then `vi` |
| `ui.enableMouse` | bool | `false` | Enable mouse support |
| `ui.headless` | bool | `false` | Hide the header bar |
| `ui.noIcons` | bool | `false` | Disable Unicode icons for terminal compatibility |
| `ui.skin` | string | `default` | Theme/skin name |
| `ui.skinReadOnly` | string | *(none)* | Skin override when `readOnly=true` |
| `ui.splashless` | bool | `false` | Skip the startup splash screen |
| `ui.rbacWarnings` | bool | `true` | Show a "restricted" empty-state when RBAC (HTTP 403) blocks a Flux kind; set `false` to keep those views silently empty |
| `namespaceHotkeys` | string[] | *(auto-discover)* | Namespaces assigned to number keys 0–9 |
| `contextSkins` | map | *(empty)* | Per-context skin overrides |
| `favorites` | string[] | *(empty)* | Persisted favorite resource keys |

---

## Configuration Options

### Read-Only Mode

By default, flux9s launches in read-only mode to prevent accidental changes. Destructive operations (like `Ctrl+d` delete) always show a confirmation screen.

**Via command line:**

```bash
flux9s config set readOnly false
```

**During a session:**
Use the `:readonly` command to toggle read-only mode for that session.

{{% alert title="Safety First" color="info" %}}
Read-only mode is enabled by default. Only disable it when you need to perform write operations.
{{% /alert %}}

---

### Default Namespace

Control which namespace flux9s watches at startup.

```bash
# Watch only the flux-system namespace (default)
flux9s config set defaultNamespace flux-system

# Watch all namespaces
flux9s config set defaultNamespace all
```

Special values `all` and `-A` both cause flux9s to watch across all namespaces. Any other value is treated as a specific namespace name.

To set the namespace where your Flux controllers run (used for controller health display):

```bash
flux9s config set defaultControllerNamespace flux-system
```

---

### Default Resource Filter

Set what the Flux list shows when flux9s starts: one kind, several, or a
[view](#views). Without this, flux9s starts showing every kind. For a
one-off start, use [`flux9s -c`](/user-guide/command-line/#start-command)
instead.

```bash
# Always start filtered to Kustomizations
flux9s config set defaultResourceFilter Kustomization

# Use a short alias — saved as the canonical display name
flux9s config set defaultResourceFilter ks

# Kustomizations and HelmReleases together
flux9s config set defaultResourceFilter ks,hr

# A view from your config (it follows edits to the view)
flux9s config set defaultResourceFilter apps

# Clear the filter (return to showing all types at startup)
flux9s config set defaultResourceFilter ""
```

Accepted kinds are any Flux resource type name or alias:

| Resource Type | Accepted aliases |
|---|---|
| `Kustomization` | `ks`, `kustomization`, `kustomizations` |
| `HelmRelease` | `hr`, `helmrelease`, `helmreleases` |
| `GitRepository` | `gitrepo`, `gitrepository`, `gitrepositories` |
| `OCIRepository` | `oci`, `ocirepository`, `ocirepositories` |
| `HelmRepository` | `helmrepository`, `helmrepositories` |
| `HelmChart` | `helmchart`, `helmcharts` |
| `Alert` | `alert`, `alerts` |
| `Provider` | `provider`, `providers` |
| `Receiver` | `receiver`, `receivers` |
| `ImageRepository` | `imagerepository`, `imagerepositories` |
| `ImagePolicy` | `imagepolicy`, `imagepolicies` |
| `ImageUpdateAutomation` | `imageupdateautomation`, `imageupdateautomations` |
| `FluxInstance` | `fi`, `fluxinstance`, `fluxinstances` |
| `FluxReport` | `fr`, `fluxreport`, `fluxreports` |
| `ResourceSet` | `rset`, `resourceset`, `resourcesets` |

You can also change the filter interactively during a session by typing `:ks`, `:hr`, `:ks,hr`, etc. in the TUI, or `:all` to clear it.

### Default Sort {#default-sort}

How the Flux list is sorted when flux9s starts. Without it, the list is
ordered by namespace, then kind, then name. `Shift+N` / `A` / `T` / `S`
still change it at any time.

```bash
flux9s config set defaultSort type      # group by kind
flux9s config set defaultSort -age      # newest first (also: age:desc)
flux9s config set defaultSort status    # problems first
flux9s config set defaultSort default   # the default order (same as unset)
```

In a kind set or view (`:ks,hr`), the kinds you listed lead, in that order
— all Kustomizations, then all HelmReleases, across namespaces — and `T`
groups by kind in the same order. Outside a set, kinds sort alphabetically
after the namespace.

### Views {#views}

A view names a set of kinds you look at together. `:<name>` opens it,
`Tab` completes it, and `?` lists your views.

```yaml
views:
  apps:
    kinds: [ks, hr]          # :apps — Kustomizations, then HelmReleases
    sort: type               # optional: grouped by kind
    namespace: all           # optional: switch to this namespace (all = every namespace)
  sources:
    kinds: [gitrepo, oci, helmrepository]
  app-health:
    kinds: [ks, hr, deploy, svc]   # Flux and Kubernetes kinds in one list
  shop:
    apps:                    # an app view: these Flux objects and what they deploy
      - ks/frontend
      - ks/backend
      - hr/data/redis        # <kind>/<namespace>/<name>
    namespace: flux-system   # where <kind>/<name> entries are found
```

Or from the command line:

```bash
flux9s config set views.apps ks,hr
flux9s config set views.apps.sort type
flux9s config set views.apps.namespace all
flux9s config set views.apps ""      # remove it
flux9s config set views.shop.apps ks/flux-system/frontend,hr/data/redis
```

- **Sort** takes the same values as [`defaultSort`](#default-sort)
  (`default` returns to the default order). **Namespace** switches the
  namespace when the view opens, exactly like `:ns` (`all` = every
  namespace). Both stay in effect after you leave the view, as `:ns` and
  `Shift+T` would; leave them out to keep the current ones.
- **`-n`** opens a view, kind set, or kind in a namespace for this opening —
  `:apps -n prod`, `:ks,hr -n prod`, `:deploy -n prod` — and wins over the
  view's own `namespace`. It's checked first: a broken view or a bad kind
  leaves the namespace as it was.

- **Kinds** are Flux kinds by name, plural, or alias — the same names as
  `:ks`. [Flux-adjacent kinds](#discovering-flux-adjacent-resource-kinds)
  work once flux9s has discovered them; until then (and in
  `flux9s config`) they're noted as unknown, in case they're a typo.
- **Kubernetes kinds** (`deploy`, `svc`, any CRD the cluster serves) mix in
  with [native resources](/user-guide/native-resources/) on. Their objects
  join the Flux list with the shared columns — STATUS and READY from their
  health, MESSAGE — after the Flux kinds, in the order given. Each is
  watched only while the list is on screen, in the current namespace. Keys
  follow the selected row: Flux operations on Flux rows; on a Kubernetes row
  its kind's keys (Enter opens a workload's detail, `l` logs, `r` restart,
  `t` / `g` / `m` when Flux manages it). With native resources off, the
  view's Kubernetes kinds are skipped and its title says so. Name a kind by
  `plural.group` (`buckets.s3.aws.upbound.io`) when several groups serve the
  same name.
- **Apps** make the view an [app view](/user-guide/views/#app-view-a): each
  Flux object (Kustomization, HelmRelease, ResourceSet, FluxInstance) with
  its sources, everything its inventory lists, and its workloads' pods —
  read from Flux, so nothing else is configured. A view has `kinds` or
  `apps`, not both. Its `namespace` is where `<kind>/<name>` entries are
  found (it doesn't switch the session's namespace); without one, the
  current namespace is used, or the one Flux object of that name.
- **Names:** flux9s commands come first, then views, then kinds. A view
  can't be named like a command (`:events`, `:pulse` — it would never open).
  A view may reuse a kind name (`ks`, `po`) and then stands in for it; the
  kind's other names still work. flux9s warns about both when it starts,
  after it discovers the cluster's kinds, and in `flux9s config validate`.
- **Per cluster:** views in a [cluster config](#cluster-and-context-specific-configuration)
  add to the root's, and replace a root view of the same name.

---

### Unmanaged and Orphaned Objects {#ownership}

[`:unmanaged` and `:orphaned`](/user-guide/native-resources/#unmanaged-and-orphaned)
leave out Kubernetes' own objects already. Hide the rest of what's expected:

```yaml
ownership:
  ignore:
    # Every field given must match; * and ? are globs; kind ignores case.
    - group: velero.io       # "" is the core API group
      kind: Backup
      name: "capi-*"
    - kind: Secret
      namespace: monitoring
      name: "alertmanager-*"
  # Namespaces never swept (default: kube-system, kube-public, kube-node-lease)
  ignoreNamespaces: [kube-system, kube-public, kube-node-lease, velero]
```

```bash
flux9s config set ownership.ignoreNamespaces kube-system,velero
flux9s config set ownership.ignoreNamespaces none   # sweep every namespace
flux9s config set ownership.ignoreNamespaces ""     # back to the default
```

`ownership.ignore` is a list of rules, so it's edited in the config file
(`flux9s config path`). A `:orphaned <kind>/<name>` (or `O`) sweep still
applies `ignore` rules but looks in every namespace.

---

### Discovering Flux-Adjacent Resource Kinds

```yaml
discoverFluxResources: true
```

Off by default. When enabled, flux9s watches CustomResourceDefinitions
labeled `app.kubernetes.io/part-of=flux` — the **same label the Flux
Operator's FluxReport uses** to enumerate reconcilers — and shows their
resources alongside the built-in kinds: they appear in the unified list with
generic columns (readiness from standard conditions), get `:` commands from
the CRD's own names (`:widget`, its plural, and any `kubectl` short names),
and support `y`/`d`. Kinds appear and disappear live as CRDs are labeled or
removed — no restart needed.

To register a kind (for example Flagger's Canary), label its CRD once:

```bash
kubectl label crd canaries.flagger.app app.kubernetes.io/part-of=flux
```

That single label registers it with both the Flux Operator's report and
flux9s.

Guard rails:

- Discovered kinds are **view-only**: suspend, resume, reconcile, and delete
  never apply to them
- Built-in Flux kinds are excluded from discovery (the FluxInstance labels
  its own CRDs, and those are already watched natively)
- Cluster-scoped CRDs are skipped
- When the flag is off (the default), no CRD watch runs and no extra API
  calls are made

Use the `:discover` command to arm or disarm discovery mid-session without a
restart (the sibling of `:readonly`). The toggle applies to the current
session only — including across a context switch — and never writes the
config file; set `discoverFluxResources` above to change the default.
Disabling stops the CRD watch, deregisters the discovered kinds along with
their `:` commands, and removes their resources from the list.

### Pod Usage Metrics

The workload detail shows each pod's CPU and memory next to its limits. With
`metricsSource: auto` (the default) flux9s picks the first source that works,
the same order [ktop](https://github.com/vladimirvivien/ktop) uses:

1. **kubelet** — reads each node's resource metrics through the API server
   (`/api/v1/nodes/{node}/proxy/metrics/resource`). No Prometheus or
   metrics-server needed, but it requires `get` on `nodes/proxy`.
2. **metrics-server** — `metrics.k8s.io` PodMetrics, when the kubelet route
   isn't permitted.
3. **none** — no usage; the columns show requests/limits instead.

Every step fails closed: missing permissions or an absent metrics-server just
move on to the next source, silently. The source in use is shown in the view
title. Force one with:

```bash
flux9s config set metricsSource metrics-server   # auto | kubelet | metrics-server | none
```

Neither source supports watch, so usage is polled every 15s (both refresh
about that often), backing off to 2 minutes while requests fail. Opening a
view hydrates immediately: with the kubelet source, one read of each node's
summary (`/stats/summary`, same `nodes/proxy` permission) gives CPU straight
away instead of waiting for the next counter update.

To grant the kubelet route, allow:

```yaml
rules:
  - apiGroups: [""]
    resources: ["nodes/proxy"]
    verbs: ["get"]
```

### Kubernetes API Connection Timeout

At startup, flux9s probes the Kubernetes API server before starting watchers. If the kubeconfig, context, credentials, network, or API server is not working, flux9s shows a connection error screen instead of hanging indefinitely.

```bash
flux9s config set connectTimeoutSeconds 15
```

You can also override the configured value for a single run:

```bash
FLUX9S_CONNECT_TIMEOUT=15 flux9s
```

The timeout must be a positive integer. The default is `10` seconds.

---

### Namespace Hotkeys

Bind namespaces to number keys 0–9 for quick switching. If left empty, flux9s auto-discovers namespaces that contain Flux resources at startup.

The assigned hotkeys are listed in the help overlay (press `?`). For interactive, searchable namespace switching, use the `:ns` command, which opens a picker seeded from these namespaces.

**Via command line:**

```bash
# Set hotkeys manually
flux9s config set namespaceHotkeys "all,flux-system,production,staging"

# Or using YAML array format
flux9s config set namespaceHotkeys "[all, flux-system, production, staging]"
```

**In config.yaml:**

```yaml
namespaceHotkeys:
  - all           # Key 0
  - flux-system   # Key 1
  - production    # Key 2
  - staging       # Key 3
```

Maximum 10 entries (keys 0–9). To restore auto-discovery:

```bash
flux9s config restore-namespace-hotkeys
```

---

### UI Configuration

#### Skins / Themes

flux9s includes 17 themes embedded in the binary:

- **Dark:** dracula, nord, solarized-dark, monokai, gruvbox-dark, catppuccin-mocha, rose-pine-moon, one-dark, tokyo-night, and more
- **Light:** default-light, kiss

**Set a skin:**

```bash
flux9s config set ui.skin dracula
```

**Set a different skin when in read-only mode:**

```bash
flux9s config set ui.skinReadOnly rose-pine
```

**Interactive theme selection:**

In the TUI, type `:skin` to open a live-preview theme picker:

![Theme Submenu](/images/skin-submenu.png)

- `j`/`k` — navigate themes
- `Enter` — apply temporarily
- `s` — save to config
- `Esc` — cancel

**Install a custom skin from a file:**

```bash
flux9s config skins set my-skin.yaml
```

Custom skin files go in:
- **Linux/macOS:** `~/.config/flux9s/skins/`
- **Windows:** `%APPDATA%\flux9s\skins\`

{{% alert title="Skin Compatibility" color="warning" %}}
Not all K9s skins work directly with flux9s. The format is similar but may require minor adjustments.
{{% /alert %}}

**Context-specific skins:**

Automatically switch skins based on your current Kubernetes context:

```yaml
contextSkins:
  production-cluster: rose-pine-moon
  dev-cluster: dracula
```

Or via CLI (cluster-specific config):

```bash
flux9s config set ui.skin rose-pine-moon --cluster production-cluster
```

#### Other UI Options

```bash
# Enable mouse support
flux9s config set ui.enableMouse true

# Hide the header bar
flux9s config set ui.headless true

# Disable Unicode icons (for limited terminal compatibility)
flux9s config set ui.noIcons true

# Skip the startup splash screen
flux9s config set ui.splashless true
```

---

### Favorites

Mark resources as favorites for quick access. Favorites persist across sessions.

**During a session:**
- Press `f` on any resource to toggle favorite status
- Type `:favorites` or `:fav` to view all favorites

**In config.yaml** (managed automatically, but can be edited manually):

```yaml
favorites:
  - "Kustomization:flux-system:my-app"
  - "HelmRelease:production:nginx"
```

The format is `ResourceType:namespace:name`.

---

## Environment Variables

Environment variables override the config file and are useful for CI, containers, or temporary overrides:

| Variable | Overrides | Example |
|---|---|---|
| `FLUX9S_SKIN` | `ui.skin` | `FLUX9S_SKIN=dracula flux9s` |
| `FLUX9S_READ_ONLY` | `readOnly` | `FLUX9S_READ_ONLY=false flux9s` |
| `FLUX9S_DEFAULT_NAMESPACE` | `defaultNamespace` | `FLUX9S_DEFAULT_NAMESPACE=production flux9s` |
| `FLUX9S_DEFAULT_RESOURCE_FILTER` | `defaultResourceFilter` | `FLUX9S_DEFAULT_RESOURCE_FILTER=HelmRelease flux9s` |

---

## Cluster and Context-Specific Configuration

You can store config for a specific cluster or context. These are layered on top of the root config:

```bash
# Set a value for a specific cluster
flux9s config set readOnly false --cluster my-cluster

# Set a value for a specific context within a cluster
flux9s config set ui.skin dracula --cluster my-cluster --context prod-context
```

Cluster configs are stored under `~/.local/share/flux9s/clusters/<cluster-name>/`.

---

## Example config.yaml

```yaml
# ~/.config/flux9s/config.yaml

readOnly: false
defaultNamespace: flux-system
defaultControllerNamespace: flux-system
defaultResourceFilter: apps  # Start on the "apps" view (or a kind, or ks,hr)
defaultSort: type            # Group the list by kind

views:
  apps:
    kinds: [ks, hr]
    sort: type

ui:
  enableMouse: false
  headless: false
  noIcons: false
  skin: dracula
  skinReadOnly: rose-pine-moon  # Different skin in read-only mode
  splashless: false

namespaceHotkeys:
  - all           # Key 0 = all namespaces
  - flux-system   # Key 1
  - production    # Key 2
  - staging       # Key 3

contextSkins:
  production-cluster: rose-pine-moon
  dev-cluster: dracula

favorites:
  - "Kustomization:flux-system:apps"
  - "HelmRelease:production:nginx"
```

---

## Command Reference

```bash
# Show all current config with defaults annotated
flux9s config list

# Get a specific value
flux9s config get ui.skin

# Set a value
flux9s config set readOnly false

# Show config file path
flux9s config path

# Validate configuration
flux9s config validate

# List available skins
flux9s config skins list

# Install a custom skin
flux9s config skins set path/to/skin.yaml

# Restore namespace hotkeys to auto-discover
flux9s config restore-namespace-hotkeys
```
