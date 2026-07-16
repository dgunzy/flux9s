# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
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

### Changes\n- Version bump to 0.12.0

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

### Changes\n- Version bump to 0.11.1

### Fixed
- Restored SOCKS5 proxy support (#202): the `kube/socks5` cargo feature was
  accidentally dropped in a dependency cleanup (#182), so kubeconfigs with
  `proxy-url: socks5://…` failed at startup with `requires the disabled
  feature "kube/socks5"`. Regression tests now build a client through both
  socks5 and http proxy configs so a missing proxy feature fails CI.

## [0.11.0] - 2026-07-06

### Changes\n- Version bump to 0.11.0

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

### Changes\n- Version bump to 0.10.2

## [0.10.1] - 2026-06-15

### Changes\n- Version bump to 0.10.1

## [0.10.0] - 2026-06-13

### Changes\n- Version bump to 0.10.0

## [0.9.2] - 2026-06-05

### Changes\n- Version bump to 0.9.2

## [0.9.1] - 2026-05-31

### Changes\n- Version bump to 0.9.1

## [0.9.0] - 2026-04-28

### Changes\n- Version bump to 0.9.0

## [0.8.3] - 2026-04-23

### Changes\n- Version bump to 0.8.3

## [0.8.2] - 2026-04-22

### Changes\n- Version bump to 0.8.2

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

[Unreleased]: https://github.com/yourusername/flux9s/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/yourusername/flux9s/releases/tag/v0.1.0
