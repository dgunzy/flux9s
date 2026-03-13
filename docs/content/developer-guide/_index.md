---
title: "Developer Guide"
linkTitle: "Developer Guide"
weight: 5
description: "Information for developers contributing to flux9s"
toc: true
type: docs
---

## Architecture

flux9s is a Rust TUI that keeps a live, local model of Flux resources and renders it through a keyboard-first interface. The architecture is intentionally simple: watch Kubernetes objects continuously, normalize them into in-memory state, and keep the UI loop responsive by pushing slower work onto async tasks.

### Entry Points

The repository currently has two top-level execution modes:

- `src/main.rs` is the CLI and TUI binary entry point.
- `src/lib.rs` exposes the reusable library API, including headless usage through `ClusterSession`.

The crate enables the TUI by default, but the `tui` feature is optional. That means contributor-facing architecture should be understood as "shared Flux watcher/library core plus an optional terminal UI", not only as a standalone binary.

### Data Flow

At a high level, the runtime looks like this:

1. `src/watcher/mod.rs` starts a watch stream for each supported Flux resource kind, plus dedicated watchers for Flux controller pods and controller deployments.
2. Watch events are normalized into `WatchEvent` values and applied to `ResourceState` in `src/watcher/state.rs`.
3. `App` in `src/tui/app/` reads that state and renders list, detail, YAML, trace, graph, history, and favorites views.
4. `ResourceService` and `ClusterSession` provide the same watcher/state/operation foundation without requiring the TUI layer.

### Watch Strategy And Performance

Performance is driven mostly by the watcher design:

- When a namespace is selected, `ResourceWatcher` uses `Api::namespaced(...)`.
- Only when the user switches to `all` does it use `Api::all(...)`.
- Namespace changes restart the watcher set instead of broad-watching everything and filtering afterward.
- The default configuration starts in `flux-system`, which keeps initial watch scope narrow on larger clusters.

This is why flux9s can provide live updates while staying responsive on clusters where a cluster-wide watch would be unnecessarily expensive.

### Model

`App` still follows a modified Elm-style split:

- `App` and the state structs in `src/tui/app/state.rs` hold the centralized TUI state.
- `handle_key()` in `src/tui/app/events.rs` processes input synchronously.
- `render()` in `src/tui/app/rendering.rs` draws the current state using stateless view functions from `src/tui/views/`.

This split keeps event handling predictable while making individual views easy to reason about and test.

### Async Layer

Network-bound and potentially slower operations are spawned off the main UI loop and returned through `oneshot` channels. This includes YAML fetches, trace resolution, graph building, and write operations such as suspend, resume, reconcile, and delete. The async entry points live in `src/tui/app/async_ops.rs`.

The important behavior is that the UI never blocks on these calls; it schedules the work, keeps rendering, and picks up results later.

### Type System And Generated Models

Flux and Flux Operator CRD types are generated into `src/models/_generated/`. The rest of the codebase relies on `FluxResourceKind` and the Kubernetes API helpers built around it as the single source of truth for resource names, aliases, groups, versions, and plural forms.

That approach is what keeps the watcher registry, fetch paths, command aliases, and type-specific views aligned when CRDs evolve.

### K9s Conventions And Flux-Specific Additions

flux9s deliberately reuses the K9s interaction model where it helps operators move quickly:

- `j`/`k` navigation and keyboard-first workflows
- `:` command mode with aliases and autocomplete
- context and namespace switching
- footer and help overlays driven from centralized keybinding definitions
- k9s-style skin compatibility

On top of that, flux9s adds workflows that are specific to Flux and Flux Operator:

- support for `FluxInstance`, `ResourceSet`, `ResourceSetInputProvider`, and `FluxReport`
- trace and graph views for Flux-managed relationships
- reconciliation history for resource types that expose `status.history`
- reconcile-with-source and other Flux-aware resource actions

The graph code in `src/trace/graph_builder.rs` explicitly follows patterns from the Flux Operator Web UI, which is why Flux Operator resources can participate in the same relationship views as core Flux resources.

## Project Structure

```
flux9s/
├── src/
│   ├── main.rs          # Binary entry point
│   ├── lib.rs           # Library entry point and public exports
│   ├── cli/             # CLI commands, config subcommands, logging, version output
│   ├── config/          # Config schema, loading, paths, themes
│   ├── kube/            # Kubernetes API helpers, dynamic fetch, inventory helpers
│   ├── models/          # Generated CRD models plus custom resource field extraction
│   ├── operations.rs    # Flux resource operations and operation registry
│   ├── services/        # Headless service layer (`ClusterSession`, `ResourceService`)
│   ├── trace/           # Trace and graph building logic
│   ├── watcher/         # Watch registry, resource bindings, watch state
│   └── tui/             # Optional ratatui/crossterm frontend
│       ├── app/         # App state, events, rendering, async operation polling
│       ├── views/       # Stateless renderers for list/detail/header/footer/etc.
│       ├── commands.rs  # Command parsing, aliases, submenus
│       ├── keybindings.rs
│       ├── operations.rs
│       ├── submenu.rs
│       ├── theme.rs
│       └── trace.rs
├── tests/              # Integration-style tests and snapshots
├── docs/               # Hugo docs site
└── justfile            # Common contributor workflows
```

### TUI Module Notes

The TUI is not a single monolith anymore. The current split is:

- **`core.rs`** - Main App struct and core logic
- **`state.rs`** - Organized state structures (view, selection, UI, async, controller pod state)
- **`events.rs`** - Event handling and input processing
- **`rendering.rs`** - Rendering orchestration
- **`async_ops.rs`** - Async operation management

The command palette and submenu behavior live outside that folder in `src/tui/commands.rs` and `src/tui/submenu.rs`, while footer/help consistency is driven by `src/tui/keybindings.rs`.

## Development Setup

### 1. Clone the Repository

```bash
git clone https://github.com/dgunzy/flux9s.git
cd flux9s
```

### 2. Install Dependencies

```bash
cargo build
```

### 3. Run Tests

```bash
just ci
```

The current `just ci` workflow runs:

- `cargo fmt` - Format code
- `cargo clippy` - Lint code
- `cargo audit` - Advisory check
- `cargo test --lib --tests` - Library and unit-style tests
- `cargo test --test ...` - Selected integration-style test binaries

`cargo audit` needs network access to refresh the RustSec advisory database, so it may fail in restricted environments even when the code is fine.

### Docs Workflow

The docs site is maintained in the same repository and has explicit `just` helpers:

```bash
just docs-deps
just docs-serve
just docs-build
```

Use these instead of ad hoc commands when working on the Hugo site from the repo root.

## Testing Layout

The test tree is flatter than some older internal notes imply. Today, most contributor-facing tests live as top-level files under `tests/`:

- `crd_compatibility.rs` - compatibility checks for watched CRDs and status extraction
- `resource_registry.rs` - registry completeness checks
- `field_extraction.rs` - resource field extraction behavior
- `trace_tests.rs` and `graph_tests.rs` - relationship discovery and graph behavior
- `reconciliation_history_tests.rs` - history extraction behavior
- `favorites_tests.rs` - config-backed favorites behavior
- `navigation_tests.rs` and `snapshot_tests.rs` - TUI behavior and rendering snapshots

Snapshot artifacts live in `tests/snapshots/`. TUI-specific tests are feature-gated in `Cargo.toml`, while the headless/library tests run without the `tui` feature.

## Contributing

Contributions are welcome! To contribute:

1. **Fork the repository** on GitHub
2. **Create a branch** in your fork for your changes
3. **Make your changes** following the development guidelines
4. **Submit a Pull Request** from your fork to the main repository

{{% alert title="Development Guidelines" color="info" %}}
For detailed development guidelines, see the [AGENTS.md](https://github.com/dgunzy/flux9s/blob/main/AGENTS.md) file in the repository.
{{% /alert %}}

### Key Principles

- **Zero-maintenance model updates**: Use code generation over manual types
- **Non-blocking operations**: Keep UI responsive with async task spawning
- **Graceful degradation**: Log errors, don't crash
- **K9s alignment**: Follow K9s conventions for keybindings and UX

## License

This project is licensed under the Apache License, Version 2.0 - see the [LICENSE](https://github.com/dgunzy/flux9s/blob/main/LICENSE) file for details.
