# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Docs: supported and tested Flux versions (2.2 and later; tested on 2.2,
  2.9, 2.10)

### Fixed
- On Flux 2.1–2.9, `:trace alert/<name>` and `:trace provider/<name>`
  across all namespaces failed, and namespaces holding only Alerts or
  Providers were missing from namespace discovery — both used the Flux
  2.10 API version only. They now use the version the cluster serves
- A reconciled HelmRelease's CHART shows its chart (name, or an OCI chart
  template's URL) instead of the generated source's name; for an OCI chart
  template VERSION shows its ref in the controller's order (digest, then
  semver, then tag), never the ignored `version`
- Older API versions are tried only where a kind really had them (no more
  requests for versions that never existed, such as HelmRelease
  `v2beta3`); the image kinds now fall back on Flux before 2.7 —
  ImageUpdateAutomation to `v1beta2`, then `v1beta1` (Flux 2.2)
- Finding which version a Flux kind is served at uses API discovery, so it
  works for users whose RBAC only allows some names

## [1.4.3] - 2026-10-26

# Version 1.4.3 Changelog

**Added**
- View for orphaned resources (#314)
- Tests for Flux CRD changes (#315)

**Changed**
- Updated Flux CRDs to latest versions (#313, #312)

### Added
- Unmanaged and orphaned objects (#274, native resources on): `:unmanaged`
  lists what Flux doesn't manage in a namespace (`all` adds what Argo CD,
  Helm, or another tool manages; `-n` / `-A`), `:orphaned` lists what Flux
  left behind — objects whose Kustomization / HelmRelease / ResourceSet /
  FluxInstance is gone or no longer lists them in its inventory — and `O` on
  one of those Flux objects lists what it left behind. Rows say why
  (`created by kubectl`, `not in the inventory of …`); `Enter` / `y` open
  them, `m` / `t` go to their Flux object, `Ctrl-r` rescans. Sweeps read
  metadata only, skip owned objects, and call nothing orphaned unless the
  owner was read and settled
- Kind lists' MANAGED-BY shows `orphaned (ks)` for left-behind objects and
  `system` for Kubernetes' own, and describe's Managed By says why an object
  is orphaned (#274)
- `ownership.ignore` / `ownership.ignoreNamespaces` config to hide expected
  objects and namespaces from the sweeps (#274)
- Flux 2.10 support: CRDs and models regenerated for source / kustomize /
  notification-controller v1.10.0, helm-controller v1.7.0, the image
  controllers v1.3.0, and source-watcher v2.3.0 (#198)
- HelmReleases using an OCIRepository chart template (`chart.kind:
  OCIRepository`, Flux 2.10) show the chart URL and ref, and trace through
  the generated OCIRepository (#198)

### Changed
- Alerts, Providers, and Receivers are watched at
  `notification.toolkit.fluxcd.io/v1` (Alert and Provider were promoted in
  Flux 2.10; `v1beta3` is deprecated) and fall back to `v1beta3` / `v1beta2`
  on older clusters (#198)

## [1.4.2] - 2026-10-21

# Version 1.4.2 Changelog

**Changed**
- Improved column logic (#310)

### Added
- A kind set or view whose kinds all have revisions (`:ks,hr`) shows a
  REVISION column, so revisions compare across kinds (#309)

### Changed
- Flux list columns are sized to their content, measured across the whole
  list so they don't move as you scroll, and MESSAGE takes the rest of the
  row — `Applied revision: main@sha1:<hash>` shows in full instead of being
  cut at 40 / 50 characters (#309). Text that doesn't fit ends in `…`
  (measured by display width, so wide characters fit too). When the row
  doesn't fit, the widest columns shrink first and short ones (READY, AGE)
  stay readable. A column takes at most a third of the width, so one very
  long URL or name can't push the message out; multi-line messages are
  flattened to one line
- REVISION shows commit and digest hashes short (12 characters, like
  `git log --oneline`); describe and YAML have the full revision
- Dependency refresh: `rustls-webpki` 0.103.16 and `webpki-roots` 1.0.10 for
  RUSTSEC-2026-0382 and RUSTSEC-2026-0381, `tokio` 1.54

### Fixed
- A status message with multi-byte characters could crash the Flux list

## [1.4.1] - 2026-10-20

# Version 1.4.1

## Added
- Namespace-scoped view support

## Changed
- Sort order implementation

### Added
- `defaultSort` config: the Flux list's sort at startup (`type`, `-age`,
  `status:desc`, …) (#306)
- Views take an optional `sort` and `namespace` (switched like `:ns` when the
  view opens), and `:<view> -n <ns>` / `:ks,hr -n <ns>` / `:<kind> -n <ns>`
  open one in a namespace, checked before anything switches (#306)

### Changed
- In a kind set or view, the kinds you listed lead, in that order, across
  namespaces (`:ks,hr` lists every Kustomization, then every HelmRelease);
  `T` groups by kind in the same order (#306)

### Fixed
- `flux9s config set` accepts values starting with `-` (`defaultSort -age`,
  `views.apps.sort -name`) instead of reading them as flags

## [1.4.0] - 2026-10-19

# Version 1.4.0 Changelog

## Added
- Custom resource filtering support (#303)

## [1.3.1] - 2026-10-18

# Version 1.3.1 Changelog

**Added**
- Trace resources support (#300)

**Changed**
- Updated kube dependency from 4.0.0 to 4.2.0 (#298)
- Documentation overhaul with new videos and images (#294)

**Fixed**
- CI configuration (#297)
- Documentation build issues (#295)

### Added
- Trace and graph any Kubernetes object up to the Flux object that manages it
  (#266). With native resources on, `t` traces an object Flux manages (its
  owners, the Kustomization / HelmRelease / ResourceSet / FluxInstance, its
  source), `g` opens that Flux object's graph focused on the object (or the
  owner it applied — marked by name in a workload group, by its kind's line
  in a resource group), and `m` opens the Flux object. They
  work in kind lists, native YAML / describe / trace views, and the workload
  detail, and show only once a Flux manager is known
- Traces that end at Argo CD, Helm, another manager, or nothing say so
- `:trace <kind>/<name> -n <namespace>`; watching all namespaces, `:trace`
  without `-n` searches them (one match is traced, several are listed)
- Several Flux kinds in one list: `:ks,hr` (#302)
- Views: name a set of Flux kinds in the config (`views.apps.kinds: [ks, hr]`,
  or `flux9s config set views.apps ks,hr`) and open it with `:apps`. Views
  complete with `Tab` and show in help; flux9s warns when a view hides a
  command or kind name, at startup, after kind discovery, and in
  `flux9s config validate`
- `flux9s -c <command>` runs a `:` command once connected, like `k9s -c`
  (`-c ks,hr`, `-c apps`, `-c pulse`)
- `defaultResourceFilter` takes several kinds (`ks,hr`) or a view name

### Changed
- Trace follows controller owner references only — the same chain the
  MANAGED-BY column follows — and stops with a note when an owner can't be
  read. A kind named like a Flux kind in another API group (a Crossplane
  `Bucket`) is no longer treated as Flux
- **Library API (breaking):** `TraceResult` has a new `outcome` field and
  `TraceNode` a new `api_version` field, so struct literals must set them.
  `kube::ownership::describe_manager` / `immediate_manager` return
  `ManagedBy` (text plus the managing Flux object) instead of `String`, and
  `DescribeData::managed_by` / `WorkloadData::managed_by` hold it.
  `WorkloadData` has a new `api_version` field and `KindRow` a new `resolved`
  field. New: `trace::trace_target` (trace any `ObjectRef`),
  `trace::locate_in_graph`, `kube::objects::find_by_name`
- Replaced the deprecated `serde_yaml` (its `unsafe-libyaml` parser is unmaintained,
  RUSTSEC-2024-0449) with `serde-saphyr`, a YAML implementation in safe Rust (#296)
- YAML views keep the API server's field order (as `kubectl get -o yaml` does) instead of
  sorting keys alphabetically
- Unquoted leading-zero numbers in edited YAML are read as octal, as `kubectl edit` does
  (`defaultMode: 0644` applies 420)
- Minimum supported Rust version is now 1.89
- Dependency refresh, including `quinn-proto` 0.11.19 for RUSTSEC-2026-0338 to 0342
- `serde-saphyr` 2.0 and a second dependency refresh, including `rustls` 0.23.46 for
  RUSTSEC-2026-0346 to 0349 and `kube` 4.2
- Kubernetes API types target 1.36 (`k8s-openapi` `v1_36`, was `v1_33`)

### Removed
- **Library API (breaking):** `tui::trace::format_trace_result`, unused by
  flux9s (the trace view renders the result directly)

### Fixed
- `:trace <kind>/<name>` while watching all namespaces looked in `default`
  instead of where the object is (#266)
- Trace now reaches cluster-scoped objects and owners of any kind (it
  resolved owners by kind name only), and shows the owner chain it walked
- The trace view couldn't scroll a long trace to its end (the content moved
  only a row or two), cut off text that wrapped, and could crash on a long
  multi-byte status message. Blocks now size to their wrapped text and the
  view scrolls through the shared viewport
- A trace of an object whose Flux manager can't be read (RBAC, or deleted
  with the object left behind) said "Not managed by Flux"; it now says the
  manager couldn't be read
- YAML views showed list items without the space after the dash (`-item` instead of `- item`)
- Docs site build on Linux CI picked a Dart Sass binary that can't run on glibc

## [1.3.0] - 2026-10-12

# Version 1.3.0 Changelog

## Added
- Support for editing non-Flux objects with data-driven write actions

## Changed
- Action table now drives keys, footer, and help text
- Trace enhancements and code hygiene improvements

## Fixed
- Namespace filtering and context handling
- Changelog accuracy

### Added

- (preview) `e` edits non-Flux objects (#276): any kind the API server lists as patchable, in kind lists, inventory/event rows, and native YAML/describe views. Before the editor opens, flux9s resolves who manages the object and asks first — Flux (with different wording when the managing Kustomization/HelmRelease/ResourceSet is suspended: the change sticks until it's resumed), Helm, Argo CD, or a controller owner. Secrets are never editable (flux9s holds them redacted); `e` suggests `kubectl edit` there and for kinds the API won't patch
- Action table: `ActionSpec::Patch` / `ActionSpec::Annotate` describe write actions as data — a merge-patch template (`{{now}}`), the kinds it applies to, and its confirmation/status wording — sharing the confirmation and read-only gate. Workload restart (`r`) is now one of them; config-defined actions build on this in #277

### Changed

- Keys, footer hints, and help now come from one action table (#276), so they can't drift. Each view's footer shows only the keys that work there (e.g. the YAML/describe views no longer advertise `Enter`, `f`, or sort keys), and the help overlay is generated from the same table and the `:` command table. `Ctrl`+letter no longer triggers the plain letter's action
- Scrollable views show a subtle scroll thumb on their right border — sized to how much is visible and placed where you are — in every text view, list, the graph, and help. Scrolling is one mechanism (`View::scroll_model` + `Viewport`), so every view clamps and scrolls the same way
- Back navigation (`Esc` / `Backspace`) uses one history for every view, replacing five separate "where to go back to" fields. Back now always returns to where you came from — e.g. trace opened from a graph returns to the graph, and `:logs` from a detail view returns to it. `Backspace` behaves exactly like `Esc` except it never asks to quit. `:` commands that switch top-level view (`:flux`, `:favorites`, `:events`, `:pulse`) close what was open (e.g. a kind list's watch) instead of leaving it running
- `:trace <kind>/<name>` resolves kinds through the kind registry, like `:<kind>`: Flux aliases (`ks`, `hr`), Flux-adjacent kinds, and with native resources on any served kind by plural or short name (`deploy`, `sts`, `cj`). Exact kind names (`Deployment`) work without it

### Fixed

- (preview) A `:ns` picker opened before the cluster's namespaces had loaded (e.g. right after `:deploy` on a remote cluster) showed only the Flux namespaces until reopened; it now fills in place, keeping your selection and filter, with namespaces holding Flux resources first and marked
- `:ctx` now fully refreshes the new cluster: namespace hotkeys and the `:ns` picker's Flux namespaces are re-discovered (they kept the previous cluster's), and views tied to the old cluster (describe, workload detail, logs, events, graph, kind lists) return to the Flux list
- Long confirmation, status, and empty-state messages ran off the screen; they now wrap (one shared helper), and the footer grows to fit a long status message. The confirmation footer shows only the keys — the question is in the dialog
- Editing an object whose Flux manager is outside the watched namespaces couldn't tell whether that manager was suspended ("unless it's suspended…"); the edit pre-check now reads the manager from the cluster, so the suspended wording appears whenever RBAC allows reading it
- The help overlay (`?`) was cut off on short terminals with no way to see the rest; it now scrolls with `j`/`k`, `PgUp`/`PgDn`, `Ctrl+f`/`b`, `g`/`G`
- The graph's workload list showed readiness from when the graph was built; READY and STATUS are now watched live, so a restart or scale is visible immediately
- A kind list whose first listing stalled (e.g. a request stuck on a dead connection to a remote API server) showed "Listing…" for up to ~5 minutes; it now reports the stall after 10s and retries with a fresh watch (backing off to 60s)
- `NetworkPolicy` resolved to the wrong plural (`networkpolicyes`) when traced or fetched
- Trace (`t` / `:trace`) now recognises objects managed by a ResourceSet or FluxInstance, using the same detection as the MANAGED-BY column (it only knew Kustomization and HelmRelease labels)
- `:trace <kind>/<name>` is listed in the help overlay

## [1.2.0] - 2026-09-30

# Version 1.2.0 Changelog

**Added**
- View of Kubernetes resources and CRDs (#278)
- Workload metrics (#275)
- Helm chart values support (#272)

**Changed**
- Suspended view enhancement (#280)

**Fixed**
- Logs display for multi-container pods (#271)

### Added

- **Preview (opt-in, off by default):** native resource browsing. Turn it on per session with `:native` or permanently with `flux9s config set nativeResources true`; while off, `:deploy` and other Kubernetes kinds explain how. Everything below marked (preview) needs it (#267)
- (preview) `:<kind>` browses any kind the cluster serves — native resources and CRDs — as a live list with kstatus health and CRD printer columns. Built on a data-driven kind registry (Flux → Flux-adjacent → API discovery), so Flux aliases keep their meaning. `Esc` walks back through opened lists, `:flux` returns to the Flux view, and the header shows what's being browsed. `nativeResources: false` opts out and keeps flux9s Flux-only — including Flux-adjacent CRD discovery (`discoverFluxResources` / `:discover`) (#267). Kinds are found in every served version (not just the preferred one), an unknown kind re-checks discovery, and `e` on a non-Flux object explains it's view-only with the matching `kubectl edit` command. Hardened for large and untrusted data: rows (not objects) are stored, cluster-supplied text is sanitised before display, lists are capped at 5,000 rows, discovery requests time out, and the cursor stays on its object across live updates
- (preview) Kind lists share the workload views' actions, driven by each kind's capabilities: `Enter` on a Deployment/StatefulSet/DaemonSet/CronJob opens the same workload detail as the graph drill-down, `r` restarts, `l` streams pod logs, and `:po` gets `l` logs and `Ctrl+d` delete pod. One action table drives both the keys and the footer
- (preview) MANAGED-BY column on every non-Flux list showing the kind of manager (`Flux ks` / `Flux hr` / `Flux rset` / `Flux instance`, `Argo CD`, `Helm`, or any `app.kubernetes.io/managed-by`), following Kubernetes owner chains (Pod → ReplicaSet → Deployment) so a Flux-managed Deployment's pods read as Flux. Describe and workload detail show the full **Managed By** (e.g. `Flux Kustomization flux-system/apps (via ReplicaSet/web-7b5)`); describe never waits on it (`resolving…`, capped at 3s)
- (preview) `:ns` lists every namespace in the cluster, with namespaces holding Flux resources first and marked (`◆ flux`). Falls back to the Flux namespaces when listing namespaces is forbidden; unchanged while the preview is off
- Pod CPU/memory in workload detail: a Resources section with use / request / limit and a bar per pod, flagged near limits. Sources are auto-detected like ktop: kubelet resource metrics via `nodes/proxy`, then metrics-server, then requests/limits only. Every step fails closed, and `metricsSource` in config forces one. Hydrated immediately when a view opens, then polled every 15s with backoff, since neither source supports watch (#265)
- `v` on a HelmRelease shows its effective values: `valuesFrom` ConfigMaps/Secrets merged in order (honouring `valuesKey`, `targetPath`, `optional`) with `spec.values` last, a per-source outcome list, Secret values redacted until `x` reveals them, and live updates (#264)

### Fixed

- Namespace hotkeys (0–9) now share `:ns`'s code path, so kind lists and the events feed follow them (they previously skipped both)
- A `:<kind>` typed while discovery is still running (right after connecting) opens once discovery finishes instead of being reported as unknown
- Describe/YAML views of non-Flux objects are titled with their kind and name (they showed a bare "Describe"/"YAML")
- `scripts/dev-clusters.sh` only deletes the `flux9s-*` clusters it owns; it previously deleted every kind cluster on the machine

- Logs for multi-container pods failed with an API error; they now stream the default container (`kubectl.kubernetes.io/default-container`, else the first) and `c` switches containers

## [1.1.0] - 2026-09-27

# Version 1.1.0 Changelog

**Added**
- Workload create, update, delete (CUD) operations (#269)
- Inventory drill down functionality (#268)

**Changed**
- Updated clap_complete from 4.6.10 to 4.6.11 (#261)
- Updated cargo dependencies in minor-patch group (#260)

### Added

- Workload actions: `r` rolls out a restart of a Deployment/StatefulSet/DaemonSet from the workload list or detail, and `Ctrl+d` in workload detail deletes a pod (with a picker for several). Both are confirmed and blocked in read-only mode (#263)
- Workload detail and inventory statuses are live — driven by the watch API while the view is open instead of fetched once
- Footer hints follow the current view (e.g. `r Restart` / `^d Delete pod` in workload detail instead of the Flux resource keys)
- Inventory drill-down shows live health (STATUS + MESSAGE) for every managed object, using kstatus rules (#262)
- `Enter`/`d` and `y` on an inventory row open the describe and YAML views for any native Kubernetes object, resolved through API discovery; Secret values are redacted (#262)
- `Enter` on an event whose object flux9s doesn't watch opens its describe view instead of a status message (#262)

### Fixed

- Inventory entries now carry the full `apiVersion` (`apps/v1`, not `v1`) and decode `:` in names (e.g. `system:` ClusterRoles); the id parser previously dropped the API group (#262)

## [1.0.5] - 2026-09-17

# Version 1.0.5

## Fixed

- Namespace default bug (#258)

## [1.0.4] - 2026-09-07

# Version 1.0.4 Changelog

**Changed**
- Updated Flux CRDs to latest versions
- Bumped browserslist from 4.28.1 to 4.28.8
- Bumped flate2 from 1.1.9 to 1.1.10
- Bumped csscolorparser from 0.8.3 to 0.8.4
- Bumped futures from 0.3.33 to 0.3.34
- Updated GitHub Actions dependencies (7 updates)

## [1.0.3] - 2026-08-18

# Version 1.0.3 Changelog

**Added**
- Live labeled CRD discovery with `:discover` support and inventory in graph (#247)

**Changed**
- Updated Flux CRDs to latest versions (#244)
- Bumped cargo dependencies (minor/patch updates) (#246)

**Added**
- `:discover` command to toggle discovery of Flux-labeled CRDs at runtime, without a restart (#245)
- Graph resource group nodes drill into an inventory breakdown on `Enter`, listing the kind, namespace, and name of every resource they aggregate

## [1.0.2] - 2026-08-09

# Version 1.0.2 Changelog

**Added**
- Anchors on documentation site

**Changed**
- Binary entry point implementation
- Updated Flux CRDs to latest versions

**Fixed**
- Clippy linting issues
- Test standardization

## [1.0.1] - 2026-07-30

# Version 1.0.1 Changelog

**Added**
- Cosign integration for release signing

**Changed**
- Updated Flux CRDs to latest versions
- Hardened release workflow security

**Fixed**
- Null field handling in edit refinement

## [1.0.0] - 2026-07-24

# Version 1.0.0 Changelog

## Added
- Edit option to modify resources
- Resource discovery for extra objects
- Flux report dashboard
- Submenu search in workload view
- New namespace picker (replacing top menu namespace selector)

## Changed
- Updated Flux CRDs to latest versions
- Multiple dependency updates: base64, directories, mockall, thiserror, postcss, and cargo dependencies

## Fixed
- RBAC configuration and security follow-ups
- Security updates and open source compliance improvements
- Repository hygiene improvements

### Added
- `flux9s --version` / `-V`: the standard version flag now works (previously
  only the `version` subcommand existed — the Homebrew formula's test block
  has been calling the flag all along).
- `flux9s completions <shell>`: generate shell completions for bash, zsh,
  fish, elvish, and powershell.

### Changed
- Release binaries are now built with fat LTO, a single codegen unit, and
  stripped symbols — noticeably smaller downloads from brew, binstall, and
  GitHub releases.
- Declared a minimum supported Rust version (1.88), enforced by a new CI job.
- The Windows release build is now blocking: a release can no longer ship
  without `flux9s-windows-x86_64.zip` if the Windows build breaks.

### Fixed
- Release-notes automation had been silently failing since 0.9.x (retired
  API model), stamping every release with a literal `\n` fallback entry.
  Fixed the generation and backfilled real notes for 0.8.2–0.10.2; the
  changelog's version compare-links now point at the real repository.
- Opt-in dynamic CRD discovery (#197): with `discoverFluxResources: true`,
  flux9s watches CRDs labeled `app.kubernetes.io/part-of=flux` — the same
  label the Flux Operator's FluxReport reads — and shows their resources
  live: unified list with readiness from standard conditions, `:` commands
  from the CRD's own names and short names, `y`/`d`, filtering, and pulse
  counts. Kinds register and deregister dynamically as CRDs are labeled or
  deleted. Discovered kinds are strictly view-only (mutating operations are
  gated to built-in Flux kinds), built-in CRDs are excluded from discovery,
  and with the flag off (default) no CRD watch runs at all.
- Cluster pulse dashboard (#195): `:pulse` answers "is my GitOps pipeline
  healthy?" at a glance — ready/failed/suspended totals and per-kind counts
  for the current namespace scope, the most recent failures with their
  messages for fast triage, and the FluxReport's distribution, entitlement,
  operator, and sync info alongside live controller pod health. Renders
  instantly from the watch state (no fetch) and updates in real time.
- Workload drill-down (#194): `Enter` on a graph workload group opens the
  workload list; `Enter` on a workload opens its detail — rollout status and
  replica health, containers with images, a pods table (phase, ready,
  restarts, age), and the workload's events. `l` streams a pod's logs
  (directly for a single pod, via a pod submenu otherwise), and Back walks
  the whole chain in reverse: logs → detail → list → graph. Read-only.
- Submenu filtering and paging (#128): `/` filters `:ctx`/`:skin`/`:logs`
  menus with the same keys as the resource list (type to narrow, Enter to
  apply, Esc to cancel), PageUp/PageDown (or Ctrl+f/b) page through long
  menus, and `:` drops straight into command mode.

### Fixed
- Submenu selections no longer scroll below the visible popup: the scroll is
  now reconciled at render time with the popup's actual height instead of a
  hardcoded estimate in the key handler.
- Submenus (and the quit dialog) no longer hardcode black backgrounds or
  low-contrast title colors — they follow the active skin like every other
  overlay, fixing illegible popups across themes.
- `l` now also works from the workload list: it fetches the workload and
  continues straight into its pod logs once loaded — and Back from those
  logs returns to wherever `l` was pressed (list or detail).

## [0.12.0] - 2026-07-14

### Fixed
- Graph view no longer flashes when the focused node is taller than the
  viewport (e.g. a FluxInstance's workload group): the focus auto-scroll now
  pins oversized nodes to their top edge instead of oscillating between
  showing their top and bottom on alternating frames.

### Added
- Graph view downstream discovery for ResourceSet and FluxInstance (#204):
  their `status.inventory` now renders like a Kustomization's — Flux resources
  produced by a ResourceSet become individual navigable nodes, Deployments and
  other workloads get the workload group with live status, and arbitrary kinds
  (Namespaces, CRDs, custom resources) aggregate into the resource group.
- Controller pod log viewer (#192): `:logs` opens a submenu of the discovered
  Flux controller pods (`:logs <pod|prefix>` streams one directly). Logs are
  tailed and followed live in a bounded buffer; scrolling up pauses following
  and `G` jumps back to the newest line; `/` searches the buffer. The stream
  runs only while the view is open.
- ResourceSet step visualization (#193): the detail view of a step-based
  ResourceSet lists its ordered steps with per-step phase (done / applying /
  failed / pending) derived from the status conditions, plus each step's
  resource count, template marker, and timeout.

## [0.11.1] - 2026-07-13

### Fixed
- Restored SOCKS5 proxy support (#202): the `kube/socks5` cargo feature was
  accidentally dropped in a dependency cleanup (#182), so kubeconfigs with
  `proxy-url: socks5://…` failed at startup with `requires the disabled
  feature "kube/socks5"`. Regression tests now build a client through both
  socks5 and http proxy configs so a missing proxy feature fails CI.

## [0.11.0] - 2026-07-06

### Added
- Kubernetes Events support:
  - The describe view (`d`) now ends with a kubectl-style Events section
    showing the resource's recent events (Warnings highlighted; degrades to a
    notice when events are unavailable, e.g. RBAC).
  - New `:events` (alias `:ev`) live events feed for the current namespace
    scope — cluster-wide with `:ns all`. Streams in real time, newest first,
    filterable with `/`; `Enter` jumps to the involved Flux resource. The
    events watcher runs only while the view is open.
- Resource action keys now resolve their target from any view via a single
  `view_target()` resolver: `y`/`d` (and `t`/`g`/`h`/operations for watched
  resources) act on the selected event's involved object in the events feed
  and on the focused node in the graph view — previously these keys only
  worked from the resource list, favorites, and detail views. `y`/`d` work
  for non-Flux objects too (Pods, Deployments, …) via the API fetch path,
  and Back returns to the view you came from (events feed or graph).
- Graph view keyboard navigation: `j`/`k` move a highlighted focus between nodes
  (the view auto-scrolls to keep it visible), `Enter` opens the focused resource's
  detail view, and `Esc` returns to the graph.

### Changed
- Graph connectors are redrawn as consistent fan-outs (trunk → branch above the
  children → drop into each child) using proper box-drawing junctions, and nodes
  sit closer together for a tighter, clearer layout.

### Fixed
- `:all` now returns to the main resource list from the events feed (and stops
  the events watch), instead of only clearing filters while stuck in the view.
- The events feed's "not a watched Flux resource" message now names the
  involved object's namespace and points at `y`/`d`, so namespace-scope
  mismatches are visible instead of just confusing.

### Internal
- New live-cluster regression suite (`tests/live_tests.rs`, all `#[ignore]`d):
  exact assertions against the dev kind clusters' planted fixtures — graph
  inventory, describe events, the operator's step-failure message format, pod
  log streaming, and legacy v1beta2 version-fallback discovery. Run locally
  with `just test-live` after `./scripts/dev-clusters.sh ci`, or via the new
  weekly/dispatch `live-tests.yml` workflow.
- New generic `AsyncTask<K, T>` owns the request/dispatch/poll lifecycle for
  view fetches, replacing the five copy-pasted `*_pending`/`*_fetched`/`*_rx`
  field triplets and their hand-rolled trigger/poll methods.
- YAML/describe fetch requests are now typed `ResourceKey`s instead of
  `"type:namespace:name"` strings, removing the re-parse (and its can't-happen
  error branches) from the main loop.
- The Kubernetes events watcher has an independent lifecycle from the resource
  watchers (started lazily, stopped without a full watcher teardown), and
  survives namespace switches.
- Single source of truth for graph node sizing (`GraphNode::render_width`/
  `render_height`); connector geometry extracted into the testable, `Frame`-free
  `fanout_routes()`.
- Per-view behavior consolidated onto `impl View` helpers (`scroll_offset_mut`,
  `is_list_view`, `is_text_search_view`, `is_nested_view`).
- `:` command handling is now data-driven via `COMMAND_TABLE` with focused
  `cmd_*` handlers, replacing the long string-matching chain in `execute_command`.

## [0.10.2] - 2026-06-21

### Changed
- Dependency housekeeping: bumped kube-rs and other packages, removed unused
  dependencies (#180, #182), and corrected the license badge/metadata to
  Apache-2.0 (#181).

## [0.10.1] - 2026-06-15

### Changed
- Watchers that hit RBAC "forbidden" errors are now dropped cleanly instead of
  leaving the resource kind in a degraded retry loop (#178).
- Updated Flux CRDs to the latest upstream versions (#177).

## [0.10.0] - 2026-06-13

### Added
- YAML view search and an AGE column in the resource list (#175).

### Changed
- Reworked the resource watcher internals for reliability; assorted bug fixes
  (#175).

## [0.9.2] - 2026-06-05

### Fixed
- Patched the kube crate so TLS configuration works against Teleport-proxied
  clusters (#172).

### Internal
- DCO sign-off is now required on contributions (#169).

## [0.9.1] - 2026-05-31

### Changed
- Clearer startup feedback when the Kubernetes API connection fails or is slow
  (#167).
- Updated Flux CRDs to the latest upstream versions (#164, #165, #166).

## [0.9.0] - 2026-04-28

### Added
- `defaultResourceFilter` config option: start flux9s filtered to a chosen
  resource kind (#159).

### Changed
- Updated Flux CRDs to the latest upstream versions (#157); small bug fixes
  and UX tweaks (#160).

## [0.8.3] - 2026-04-23

### Changed
- Suspend status for Flux Operator CRDs is now read from their labels,
  matching how the operator reports it (#155).

## [0.8.2] - 2026-04-22

### Changed
- Reworked API version matching for broader cluster compatibility and bumped
  the test cluster (#153).
- Bumped kopium and cargo-audit toolchain versions (#152).

## [0.8.1] - 2026-04-03

## Version 0.8.1

### Added
- Added the `Describe` command

### Changed
- Changed the `d` command to `describe`
- Changed the `delete` command

## [0.8.0] - 2026-03-18

### Version 0.8.0

#### Added
- Updated Flux CRDs to the latest versions (#146, #135)
- Added quit warning when using `q` or `esc` commands (#143)

#### Changed
- Tweaked CRD workflow permissions (#137)
- Switched from `make` to `just` in CI (#136)

#### Fixed
- Addressed a vulnerability found by cargo audit (#138)
- Updated documentation and README (#140)
- Adjusted release workflow permissions (#144)

## [0.7.7] - 2026-02-28

### Changes in version 0.7.7:

#### Added
- Added more fields to the Flux CRD to improve the functionality of the Flux resource.

#### Changed
- Improved the development kind cluster to better support the Flux resource.

## [0.7.6] - 2026-02-28

### Version 0.7.6

#### Added
- Added a new external service `rscinptprvdr`

#### Changed
- Shortened the CI configuration in pull request #131

## [0.7.5] - 2026-02-23

### Version 0.7.5

#### Added
- Support for page up and page down scrolling keys (#125)

#### Changed
- Updated Flux CRDs to the latest versions (#126)

## [0.7.4] - 2026-02-22

### Version 0.7.4

#### Added
- Page up and page down scrolling functionality for line display

## [0.7.3] - 2026-02-17

## Version 0.7.3

### Added
- Compatibility with flux9s lib

## [0.7.2] - 2026-02-02

### Version 0.7.2

#### Added
- Support for marking stateless resources as ready (#113)
- New configuration options for controller/operator namespace (#107)

#### Changed
- Reduced Cargo package size and refactored column code (#116)
- Updated documentation with new links and minor formatting changes (#111, #109, #108)

#### Fixed
- Reverted an accidental version bump to 0.7.2 (#115)

## [0.7.1] - 2026-01-14

### Version 0.7.1

#### Changed
- Improved code readability and maintainability through code cleanup
- Updated Flux version display to show the current version

## [0.7.0] - 2026-01-13

### Version 0.7.0

#### Added
- Ability to open skin submenu from skin UI
- Controller pod status information

#### Changed
- Enhanced controller functionality

## [0.6.4] - 2026-01-09

### Version 0.6.4

#### Fixed
- Addressed version warning issues (#96)

## [0.6.3] - 2026-01-08

### Version 0.6.3

#### Added
- Added a sub-menu to the commands, accessible through the `ctx` command.

## [0.6.2] - 2026-01-06

### Version 0.6.2

#### Added
- Added backup fonts for documentation

#### Changed
- Refactored application structure, breaking up app struct (#91)

## [0.6.1] - 2025-12-30

### Version 0.6.1

#### Added
- Flux Webui features

#### Changed
- Version bumped to 0.6.0

## [0.6.0] - 2025-12-29

### Version 0.6.0

#### Added
- Added Flux WebUI features

## [0.5.11] - 2025-12-22

## Version 0.5.11

### Added
- Added "unhealthy" and "health" filters to display percentage status on top

### Changed
- Updated 2024 edition with status tweak

## [0.5.10] - 2025-12-22

Here is a concise changelog summary for version 0.5.10:

Added:
- Support for specifying a Kubernetes configuration file via the `--kubeconfig` option

## [0.5.9] - 2025-12-17

Here's a concise changelog summary for version 0.5.9:

### Fixed
- Resolved a bug that caused issues during application startup.

## [0.5.8] - 2025-12-12

## Version 0.5.8

### Added
- New `:ctx` for context switching
- Agent files for improved functionality

### Changed
- Website tweaks and improvements

## [0.5.7] - 2025-12-12

## Version 0.5.7

### Fixed
- Resolved issues with website and CI deployment

## [0.5.6] - 2025-12-12

### Version 0.5.6

#### Added
- Support for SOCKS proxy connections (#68)

#### Changed
- Compiled binary is statically linked

## [0.5.5] - 2025-12-07

## Version 0.5.5

### Changed
- Updated dependencies to address known security vulnerabilities

## [0.5.4] - 2025-12-07

### Version 0.5.4

#### Added
- Improved skin customization options
- Enhanced command-line interface (CLI)

#### Fixed
- Resolved scrolling bug reported in issue #61

## [0.5.3] - 2025-12-06

### Version 0.5.3

#### Added
- Ability to filter namespaces by number

#### Fixed
- Improvements to the help menu
- Various bug fixes and tweaks

## [0.5.2] - 2025-11-22

### Version 0.5.2

#### Changed
- Bumped version to 0.5.2

#### Fixed
- Resolved a CI issue

## [0.5.1] - 2025-11-22

### Version 0.5.1

#### Changed
- Refactored codebase and improved overall code hygiene (#51)

## [0.5.0] - 2025-11-22

## Version 0.5.0

### Added

- Better filtering
- Label and Annotation filtering
- Support for macOS 15 runners

## [0.4.3] - 2025-11-21

## Version 0.4.3

### Added

- Support for building on Windows platforms

## [0.4.2] - 2025-11-20

### Version 0.4.2

#### Fixed

- Resolved an issue with the wrap bug

#### Changed

- Refined the CI workflow configuration

## [0.4.1] - 2025-11-20

### Version 0.4.1

#### Changed

- Changed the CI agent model configuration (#40)
- Updated the CI workflow configuration (#38)

## [0.4.0] - 2025-11-20

### Added

- Support for Flux operator CRDs (#33)
- Screenshot image to documentation (#31, #32)

## [0.3.1] - 2025-11-20

### Fixed

- Fixed trace functionality (#28)
- Replaced hardcoded strings with configuration (#27)

## [0.3.0] - 2025-11-18

### Changed

- Default mode set to readOnly (#23)

## [0.2.4] - 2025-11-16

### Changed

- Workflow tweaks and improvements (#19)

## [0.2.2] - 2025-11-16

### Changed

- Code hygiene improvements (#16)
- Workflow release changes (#16)

## [0.2.1] - 2025-11-16

### Changed

- Version bump to 0.2.1

## [0.2.0] - 2025-11-16

### Added

- ReadOnly mode support
- Configuration and config-cli functionality

### Fixed

- OpenSSL build issues (#10, #8)
- Windows temporary file handling (#9)

## [0.1.5] - 2025-11-16

### Added

- OpenSSL support
- Debug logging (#7)
- Proxy support (#6)

## [0.1.4] - 2025-11-16

### Added

- Homebrew support (#5)

## [0.1.3] - 2025-11-16

### Added

- Homebrew macOS architecture support (#4)

## [0.1.2] - 2025-11-16

### Added

- Homebrew macOS architecture support (#3)
- macOS M-chip (Apple Silicon) support (#2)

## [0.1.1] - 2025-11-16

### Added

- Binstall support

## [0.1.0] - YYYY-MM-DD

### Added

- Initial release
- Real-time monitoring of Flux resources
- K9s-inspired terminal UI
- Support for all major Flux CRDs (Kustomization, GitRepository, HelmRelease, etc.)
- Resource operations (suspend, resume, reconcile, delete)
- YAML viewing
- Namespace switching
- Status indicators
- Comprehensive test suite

[Unreleased]: https://github.com/dgunzy/flux9s/compare/v0.12.0...HEAD
[0.12.0]: https://github.com/dgunzy/flux9s/compare/v0.11.1...v0.12.0
[0.11.1]: https://github.com/dgunzy/flux9s/compare/v0.11.0...v0.11.1
[0.11.0]: https://github.com/dgunzy/flux9s/compare/v0.10.2...v0.11.0
[0.10.2]: https://github.com/dgunzy/flux9s/compare/v0.10.1...v0.10.2
[0.10.1]: https://github.com/dgunzy/flux9s/compare/v0.10.0...v0.10.1
[0.10.0]: https://github.com/dgunzy/flux9s/compare/v0.9.2...v0.10.0
[0.9.2]: https://github.com/dgunzy/flux9s/compare/v0.9.1...v0.9.2
[0.9.1]: https://github.com/dgunzy/flux9s/compare/v0.9.0...v0.9.1
[0.9.0]: https://github.com/dgunzy/flux9s/compare/v0.8.3...v0.9.0
[0.8.3]: https://github.com/dgunzy/flux9s/compare/v0.8.2...v0.8.3
[0.8.2]: https://github.com/dgunzy/flux9s/compare/v0.8.1...v0.8.2
[0.8.1]: https://github.com/dgunzy/flux9s/compare/v0.8.0...v0.8.1
[0.8.0]: https://github.com/dgunzy/flux9s/compare/v0.7.7...v0.8.0
[0.7.7]: https://github.com/dgunzy/flux9s/compare/v0.7.6...v0.7.7
[0.7.6]: https://github.com/dgunzy/flux9s/compare/v0.7.5...v0.7.6
[0.7.5]: https://github.com/dgunzy/flux9s/compare/v0.7.4...v0.7.5
[0.7.4]: https://github.com/dgunzy/flux9s/compare/v0.7.3...v0.7.4
[0.7.3]: https://github.com/dgunzy/flux9s/compare/v0.7.2...v0.7.3
[0.7.2]: https://github.com/dgunzy/flux9s/compare/v0.7.1...v0.7.2
[0.7.1]: https://github.com/dgunzy/flux9s/compare/v0.7.0...v0.7.1
[0.7.0]: https://github.com/dgunzy/flux9s/compare/v0.6.4...v0.7.0
[0.6.4]: https://github.com/dgunzy/flux9s/compare/v0.6.3...v0.6.4
[0.6.3]: https://github.com/dgunzy/flux9s/compare/v0.6.2...v0.6.3
[0.6.2]: https://github.com/dgunzy/flux9s/compare/v0.6.1...v0.6.2
[0.6.1]: https://github.com/dgunzy/flux9s/compare/v0.6.0...v0.6.1
[0.6.0]: https://github.com/dgunzy/flux9s/compare/v0.5.11...v0.6.0
[0.5.11]: https://github.com/dgunzy/flux9s/compare/v0.5.10...v0.5.11
[0.5.10]: https://github.com/dgunzy/flux9s/compare/v0.5.9...v0.5.10
[0.5.9]: https://github.com/dgunzy/flux9s/compare/v0.5.8...v0.5.9
[0.5.8]: https://github.com/dgunzy/flux9s/compare/v0.5.7...v0.5.8
[0.5.7]: https://github.com/dgunzy/flux9s/compare/v0.5.6...v0.5.7
[0.5.6]: https://github.com/dgunzy/flux9s/compare/v0.5.5...v0.5.6
[0.5.5]: https://github.com/dgunzy/flux9s/compare/v0.5.4...v0.5.5
[0.5.4]: https://github.com/dgunzy/flux9s/compare/v0.5.3...v0.5.4
[0.5.3]: https://github.com/dgunzy/flux9s/compare/v0.5.2...v0.5.3
[0.5.2]: https://github.com/dgunzy/flux9s/compare/v0.5.0...v0.5.2
[0.5.0]: https://github.com/dgunzy/flux9s/compare/v0.4.3...v0.5.0
[0.4.3]: https://github.com/dgunzy/flux9s/compare/v0.4.2...v0.4.3
[0.4.2]: https://github.com/dgunzy/flux9s/compare/v0.4.1...v0.4.2
[0.4.1]: https://github.com/dgunzy/flux9s/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/dgunzy/flux9s/compare/v0.3.1...v0.4.0
[0.3.1]: https://github.com/dgunzy/flux9s/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/dgunzy/flux9s/compare/v0.2.4...v0.3.0
[0.2.4]: https://github.com/dgunzy/flux9s/compare/v0.2.2...v0.2.4
[0.2.2]: https://github.com/dgunzy/flux9s/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/dgunzy/flux9s/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/dgunzy/flux9s/compare/v0.1.5...v0.2.0
[0.1.5]: https://github.com/dgunzy/flux9s/compare/v0.1.4...v0.1.5
[0.1.4]: https://github.com/dgunzy/flux9s/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/dgunzy/flux9s/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/dgunzy/flux9s/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/dgunzy/flux9s/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/dgunzy/flux9s/releases/tag/v0.1.0
