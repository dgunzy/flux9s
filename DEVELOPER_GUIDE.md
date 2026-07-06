# Developer Guide

This guide provides comprehensive information for developers working on flux9s, including architecture, design decisions, development workflow, testing, and publishing.

## Table of Contents

- [Project Overview](#project-overview)
- [Architecture](#architecture)
- [Project Structure](#project-structure)
- [Supported Flux Resources](#supported-flux-resources)
- [CRD Generation Workflow](#crd-generation-workflow)
- [Development Workflow](#development-workflow)
- [Adding New Resource Types](#adding-new-resource-types)
- [Testing](#testing)
- [Publishing](#publishing)
- [Design Decisions](#design-decisions)
- [Implementation Status](#implementation-status)

## Project Overview

flux9s is a K9s-inspired terminal UI for monitoring Flux GitOps resources in real-time. It's built in Rust with automated CRD-to-model generation to minimize maintenance overhead.

### Key Features

- **Real-time monitoring** via Kubernetes Watch API
- **Zero-maintenance model updates** from Flux CRDs using automated scripts
- **Familiar K9s-style navigation** and keybindings
- **Extensible architecture** for easy addition of new resource types
- **Comprehensive test suite** for CRD compatibility
- **Trace operation** for viewing resource ownership chains
- **Configuration system** with YAML-based config files
- **Theme support** with customizable skins

### Technology Stack

- **Rust** - Systems programming language
- **kube-rs** - Kubernetes client library
- **kopium** - CRD to Rust model generator
- **ratatui** - Terminal UI framework
- **tokio** - Async runtime
- **serde** - Serialization framework
- **anyhow** - Error handling

## Architecture

### High-Level Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                         flux9s TUI                          │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐  │
│  │   TUI Layer  │◄───│  App State   │◄───│   Watcher    │  │
│  │  (ratatui)   │    │  Management  │    │   (kube-rs)  │  │
│  └──────────────┘    └──────────────┘    └──────────────┘  │
│         │                   │                    │          │
│         │                   │                    │          │
│         ▼                   ▼                    ▼          │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐  │
│  │   Views      │    │  Resource    │    │   Models     │  │
│  │  (header,    │    │   State      │    │  (generated) │  │
│  │   footer,    │    │  (thread-    │    │              │  │
│  │   list,      │    │   safe)      │    │              │  │
│  │   detail,    │    │              │    │              │  │
│  │   trace,     │    │              │    │              │  │
│  │   yaml)      │    │              │    │              │  │
│  └──────────────┘    └──────────────┘    └──────────────┘  │
│                                                              │
│  ┌──────────────┐    ┌──────────────┐                      │
│  │   Config     │    │   Trace      │                      │
│  │   System     │    │   Engine     │                      │
│  └──────────────┘    └──────────────┘                      │
│                                                              │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
                  ┌──────────────────┐
                  │  Kubernetes API   │
                  │   (Watch API)    │
                  └──────────────────┘
```

### Component Overview

#### 1. Watcher Module (`src/watcher/`)

Responsible for watching Flux resources via Kubernetes Watch API.

- **`mod.rs`** - Main watcher orchestration, watch event handling, namespace management
- **`resource.rs`** - Resource type definitions and `WatchableResource` trait implementations
- **`state.rs`** - Thread-safe resource state management with concurrent access
- **`registry.rs`** - Resource registry for command mapping and aliases

**Key Design Decisions:**

- Uses `Api::namespaced` for efficiency when namespace is specified
- Falls back to `Api::all` for cluster-wide watching
- Handles CRD absence gracefully (404 errors stop watcher for that resource type)
- Implements error throttling to prevent API spam
- Supports version-agnostic watching for resources with multiple API versions (e.g., OCIRepository)

#### 2. TUI Module (`src/tui/`)

Terminal user interface built with ratatui.

- **`app/`** - Application state and logic (refactored from single file):
  - `core.rs` - Main App struct and core logic
  - `state.rs` - State structures (ViewState, SelectionState, UIState, AsyncOperationState, KubeEventStore)
  - `events.rs` - Event handling and input processing
  - `rendering.rs` - Rendering orchestration
  - `async_task.rs` - Generic `AsyncTask<K, T>`: one type owns the
    request → dispatch → poll lifecycle for every view fetch (YAML, describe,
    trace, graph). Event handlers call `request()`, the main loop calls
    `dispatch()` and spawns the work, then polls `try_recv()` each tick.
  - `async_ops.rs` - Mutating-operation flow (registry validation, status
    message bookkeeping) and the graph result hook
- **`operations.rs`** - Flux operations (suspend, resume, delete, reconcile, reconcile with source)
- **`theme.rs`** - Theme configuration and loading
- **`trace.rs`** - Trace operation orchestration
- **`api.rs`** - API resource fetching with version fallback
- **`commands.rs`** - Command parsing and execution with submenu support
- **`submenu.rs`** - Submenu system (SubmenuItem, SubmenuState, CommandSubmenu trait)
- **`keybindings.rs`** - Keybinding management
- **`constants.rs`** - Application constants
- **`views/`** - View components:
  - `header.rs` - Top bar with namespace and status
  - `footer.rs` - Bottom bar with keybindings and command autocomplete
  - `resource_list.rs` - Main resource list view
  - `detail.rs` - Resource detail view
  - `yaml.rs` - YAML manifest viewer
  - `trace.rs` - Trace view showing resource ownership chains
  - `graph.rs` - Graph visualization view
  - `history.rs` - Reconciliation history view
  - `events.rs` - Live Kubernetes events feed (`:events`)
  - `confirmation.rs` - Confirmation dialogs
  - `help.rs` - Help screen
  - `splash.rs` - Splash screen
  - `submenu.rs` - Submenu rendering overlay
  - `helpers.rs` - Helper functions
  - `resource_fields.rs` - Resource-specific field extraction

**Key Design Decisions:**

- Non-blocking async operations using `tokio::spawn`
- Modular app structure with separated concerns (state, events, rendering, async)
- Separate scroll offsets for different views
- The Kubernetes events watcher (`ResourceWatcher::watch_kube_events`) has an
  independent lifecycle from the resource watchers: started lazily when the
  events view opens, stopped (and its `KubeEventStore` feed cleared) when the
  view is left, and restarted automatically across namespace/context switches
- Per-view behavior (scroll target, back navigation, classification) lives on
  `impl View` (`src/tui/app/state.rs`) rather than scattered `match` arms
- Dynamic footer wrapping for smaller screens
- Extensible operation system via trait-based design
- Command mode with autocomplete support; `:` commands dispatch through a
  data-driven `COMMAND_TABLE` of `(predicate, handler)` pairs in `events.rs`
- Interactive submenu overlays for command selection
- Graph view separates pure layout/geometry from drawing: node sizing lives on
  `GraphNode::render_width`/`render_height`, and connector routing is computed by
  the `Frame`-free `fanout_routes()` so it can be unit tested
- Graph view supports keyboard focus navigation (`j`/`k` move focus, `Enter`
  opens the focused node, `Esc` returns to the graph)

#### 3. Models Module (`src/models/`)

Generated Rust types from Flux CRDs.

- **`_generated/`** - Auto-generated models from kopium (version controlled)
  - `source_controller.rs` - GitRepository, OCIRepository, HelmRepository, Bucket, HelmChart, ExternalArtifact
  - `kustomize_controller.rs` - Kustomization
  - `helm_controller.rs` - HelmRelease
  - `image_reflector_controller.rs` - ImageRepository, ImagePolicy
  - `image_automation_controller.rs` - ImageUpdateAutomation
  - `notification_controller.rs` - Alert, Provider, Receiver
  - `source_watcher.rs` - SourceWatcher resources
  - `flux_operator_*.rs` - Flux Operator resources (ResourceSet, ResourceSetInputProvider, FluxReport, FluxInstance)
- **`flux_resource_kind.rs`** - Centralized enum for all Flux resource kinds
- **`extensions.rs`** - Manual extensions and helper traits

**Key Design Decisions:**

- Generated models are version controlled for reproducible builds
- Models can be regenerated when CRDs update using automated scripts
- Extensions provide common functionality across resource types
- Centralized resource kind enum eliminates hardcoded strings

#### 4. Config Module (`src/config/`)

Configuration management system.

- **`schema.rs`** - Configuration schema definition
- **`loader.rs`** - Configuration loading from files and environment
- **`defaults.rs`** - Default configuration values
- **`paths.rs`** - Configuration file path resolution
- **`theme_loader.rs`** - Theme file loading and parsing

**Key Design Decisions:**

- YAML-based configuration files
- Environment variable overrides
- System-specific configuration directories
- Theme support with external YAML files

#### 5. Trace Module (`src/trace/`)

Resource ownership chain tracing.

- **`core.rs`** - Trace engine implementation
- **`models.rs`** - Trace data structures

**Key Design Decisions:**

- Recursive resource relationship discovery
- Support for Kustomization → HelmRelease → Deployment chains
- Visual representation of ownership hierarchy

#### 6. CLI Module (`src/cli/`)

Command-line interface handling.

- **`commands.rs`** - CLI command parsing and execution
- **`logging.rs`** - Logging configuration

## Project Structure

```
flux9s/
├── .github/
│   └── workflows/              # CI/CD workflows
│       ├── ci.yml              # PR and push testing
│       ├── release.yml          # Release automation
│       ├── prepare-release.yml # Release preparation
│       ├── auto-tag-release.yml # Automatic version tagging
│       └── check-crd-updates.yml  # Weekly CRD update checks
├── crds/                       # Flux CRD files (version controlled)
│   ├── source-controller.crds.yaml
│   ├── kustomize-controller.crds.yaml
│   ├── helm-controller.crds.yaml
│   ├── notification-controller.crds.yaml
│   ├── image-reflector-controller.crds.yaml
│   ├── image-automation-controller.crds.yaml
│   ├── source-watcher.crds.yaml
│   └── flux-operator-*.crds.yaml  # Flux Operator CRDs
├── scripts/                    # Automation scripts
│   ├── fetch-crds.sh          # Download CRDs from GitHub releases
│   ├── generate-models.sh     # Generate Rust models using kopium
│   └── update-flux.sh         # Orchestrate CRD fetch and model generation
├── src/
│   ├── cli/                   # CLI command handling
│   │   ├── commands.rs
│   │   ├── logging.rs
│   │   └── mod.rs
│   ├── config/                # Configuration system
│   │   ├── defaults.rs
│   │   ├── loader.rs
│   │   ├── paths.rs
│   │   ├── schema.rs
│   │   ├── theme_loader.rs
│   │   └── mod.rs
│   ├── kube/                  # Kubernetes client setup
│   │   └── mod.rs
│   ├── models/                # Generated and extended models
│   │   ├── _generated/        # Auto-generated (version controlled)
│   │   │   ├── mod.rs
│   │   │   ├── source_controller.rs
│   │   │   ├── kustomize_controller.rs
│   │   │   ├── helm_controller.rs
│   │   │   ├── image_reflector_controller.rs
│   │   │   ├── image_automation_controller.rs
│   │   │   ├── notification_controller.rs
│   │   │   ├── source_watcher.rs
│   │   │   └── flux_operator_*.rs
│   │   ├── extensions.rs      # Manual extensions
│   │   ├── flux_resource_kind.rs
│   │   └── mod.rs
│   ├── trace/                 # Trace operation
│   │   ├── core.rs
│   │   ├── models.rs
│   │   └── mod.rs
│   ├── tui/                   # Terminal UI
│   │   ├── app/               # Application state and logic (refactored)
│   │   │   ├── mod.rs
│   │   │   ├── core.rs        # App struct and core logic
│   │   │   ├── state.rs       # State structures
│   │   │   ├── events.rs      # Event handling
│   │   │   ├── rendering.rs   # Render orchestration
│   │   │   └── async_ops.rs   # Async operations
│   │   ├── operations.rs      # Flux operations
│   │   ├── theme.rs           # Theme configuration
│   │   ├── trace.rs           # Trace UI integration
│   │   ├── api.rs             # API resource fetching
│   │   ├── commands.rs        # Command parsing with submenu support
│   │   ├── submenu.rs         # Submenu system
│   │   ├── keybindings.rs     # Keybinding management
│   │   ├── constants.rs       # Application constants
│   │   ├── mod.rs
│   │   └── views/             # View components
│   │       ├── mod.rs
│   │       ├── header.rs
│   │       ├── footer.rs
│   │       ├── resource_list.rs
│   │       ├── detail.rs
│   │       ├── yaml.rs
│   │       ├── trace.rs
│   │       ├── graph.rs
│   │       ├── history.rs
│   │       ├── confirmation.rs
│   │       ├── help.rs
│   │       ├── splash.rs
│   │       ├── submenu.rs
│   │       ├── helpers.rs
│   │       └── resource_fields.rs
│   ├── watcher/               # Resource watching
│   │   ├── mod.rs             # Watcher orchestration
│   │   ├── resource.rs        # Resource definitions
│   │   ├── state.rs           # State management
│   │   └── registry.rs        # Resource registry
│   ├── lib.rs                 # Library entry point
│   └── main.rs                # Binary entry point
├── tests/                      # Test suite
│   ├── crd_compatibility.rs   # CRD compatibility tests
│   ├── resource_registry.rs   # Registry tests
│   ├── model_compatibility.rs # Model tests
│   ├── field_extraction.rs    # Field extraction tests
│   ├── trace_tests.rs         # Trace operation tests
│   ├── unit/                  # Unit test helpers
│   └── README.md
├── examples/                   # Example files
│   └── themes/                # Example theme files
│       ├── dracula.yaml
│       └── solarized-dark.yaml
├── docs/                       # Documentation
│   ├── CONFIGURATION_DESIGN.md
│   ├── CONFIGURATION_IMPLEMENTATION.md
│   ├── THEME_SYSTEM.md
│   ├── TRACE_AND_RECONCILE_IMPROVEMENTS.md
│   ├── VERSION_COMPATIBILITY.md
│   ├── flux-crds.yaml         # Example CRD resources
│   └── images/                # Screenshots
├── Cargo.toml                 # Rust project configuration
├── Cargo.lock                 # Dependency lock file
├── Makefile                   # Build automation
├── manifest.json              # CRD version manifest
├── LICENSE                    # Apache 2.0 License
├── CHANGELOG.md               # Change log
├── README.md                  # User-facing documentation
└── DEVELOPER_GUIDE.md         # This file
```

## Supported Flux Resources

flux9s supports all Flux CD resources from the official Flux controllers and Flux Operator. The definitive list of Flux CRDs and their API versions can be found in the [Flux Operator common types](https://github.com/controlplaneio-fluxcd/flux-operator/blob/main/api/v1/common_types.go#L83-L110).

### Currently Supported Resources

#### Source Controller (`source.toolkit.fluxcd.io`)

- **GitRepository** (v1) - Git repository sources
- **OCIRepository** (v1, v1beta2) - OCI artifact sources
- **HelmRepository** (v1) - Helm chart repositories
- **Bucket** (v1) - S3-compatible bucket sources
- **HelmChart** (v1) - Helm chart artifacts
- **ExternalArtifact** (v1) - External artifact sources

#### Kustomize Controller (`kustomize.toolkit.fluxcd.io`)

- **Kustomization** (v1) - Kustomize-based deployments

#### Helm Controller (`helm.toolkit.fluxcd.io`)

- **HelmRelease** (v2beta2) - Helm release management

#### Image Reflector Controller (`image.toolkit.fluxcd.io`)

- **ImageRepository** (v1) - Container image repositories
- **ImagePolicy** (v1) - Image version policies

#### Image Automation Controller (`image.toolkit.fluxcd.io`)

- **ImageUpdateAutomation** (v1) - Automated image updates

#### Notification Controller (`notification.toolkit.fluxcd.io`)

- **Alert** (v1beta3) - Alert configurations
- **Provider** (v1beta3) - Notification providers
- **Receiver** (v1) - Webhook receivers

#### Source Watcher (`source.toolkit.fluxcd.io`)

- SourceWatcher resources

#### Flux Operator (`fluxcd.controlplane.io`)

- **ResourceSet** (v1) - Declarative resource sets
- **ResourceSetInputProvider** (v1) - Input providers for ResourceSets
- **FluxReport** (v1) - Flux installation reports
- **FluxInstance** (v1) - Flux installation instances

## CRD Generation Workflow

flux9s uses an automated workflow to fetch CRDs and generate Rust models. This ensures models stay up-to-date with Flux releases.

### Automated Scripts

The project includes three main scripts in the `scripts/` directory:

1. **`fetch-crds.sh`** - Downloads CRDs from GitHub releases

   - Fetches Flux controller CRDs from official releases
   - Fetches Flux Operator CRDs from the main branch
   - Pins versions for reproducible builds
   - Creates `manifest.json` with version information

2. **`generate-models.sh`** - Generates Rust models using kopium

   - Processes all CRD files in `crds/` directory
   - Splits multi-document YAML files
   - Generates Rust structs with proper derives
   - Handles duplicate prelude modules
   - Creates `mod.rs` for generated modules

3. **`update-flux.sh`** - Orchestrates the full update process
   - Runs `fetch-crds.sh` to download CRDs
   - Runs `generate-models.sh` to generate models
   - Verifies the build compiles successfully

### Workflow Steps

To update CRDs and regenerate models:

```bash
# Run the update script (recommended)
./scripts/update-flux.sh

# Or run steps individually:
./scripts/fetch-crds.sh      # Download CRDs
./scripts/generate-models.sh # Generate models
cargo check                  # Verify build
```

### Version Management

CRD versions are pinned in `scripts/fetch-crds.sh`:

```bash
CONTROLLERS="source-controller:v1.7.3
kustomize-controller:v1.7.2
helm-controller:v1.4.3
notification-controller:v1.7.4
image-reflector-controller:v1.0.3
image-automation-controller:v1.0.3
source-watcher:v2.0.2"
```

Flux Operator CRDs are fetched from the main branch (latest).

### Generated Code Location

Generated models are stored in `src/models/_generated/` and are **version controlled**. This ensures:

- Reproducible builds without requiring kopium
- Easier CI/CD (no need to install kopium in CI)
- Users can build immediately without additional tools

### Clippy Configuration

Generated code has clippy warnings suppressed in `src/models/_generated/mod.rs`:

```rust
#![allow(clippy::all)]
#![allow(unknown_lints)]
#![allow(doc_markdown)]
#![allow(clippy::doc_overindented_list_items)]
```

This ensures only our code is checked by clippy, not generated code.

## Development Workflow

### Prerequisites

- Rust 1.70 or later
- `kopium` (for model generation): `cargo install kopium`
- Access to a Kubernetes cluster with Flux installed (for testing)

### Building

```bash
# Debug build
cargo build

# Release build
cargo build --release
```

### Running

```bash
# Run in debug mode
cargo run

# Run release build
cargo run --release

# Run with specific namespace
cargo run -- --namespace flux-system
```

### Updating CRDs and Models

When Flux CRDs are updated:

1. **Update versions** in `scripts/fetch-crds.sh` if needed
2. **Run update script**:
   ```bash
   ./scripts/update-flux.sh
   ```
3. **Review changes** in `src/models/_generated/`
4. **Run tests** to ensure compatibility:
   ```bash
   cargo test --test crd_compatibility
   cargo test --test model_compatibility
   ```
5. **Commit changes**:
   ```bash
   git add crds/ src/models/_generated/ scripts/fetch-crds.sh manifest.json
   git commit -m "chore: update Flux CRDs to latest versions"
   ```

## Adding New Resource Types

When adding support for a new Flux resource type:

1. **Ensure CRD is fetched** - Add to `scripts/fetch-crds.sh` if needed
2. **Regenerate models** - Run `./scripts/update-flux.sh`
3. **Add to FluxResourceKind enum** (`src/models/flux_resource_kind.rs`):

   ```rust
   pub enum FluxResourceKind {
       // ... existing variants
       YourNewResource,
   }
   ```

   Update `as_str()`, `from_str()`, and `from_str_case_insensitive()` methods.

4. **Re-export the type** in `src/watcher/resource.rs`:

   ```rust
   pub use source_controller::YourNewResource;
   ```

5. **Add `impl_watchable!` macro**:

   ```rust
   impl_watchable!(
       YourNewResource,
       "source.toolkit.fluxcd.io",
       "v1",
       "yournewresources",
       "YourNewResource"
   );
   ```

6. **Add to registry** in `src/watcher/registry.rs`:

   ```rust
   ResourceEntry {
       display_name: "YourNewResource",
       command_aliases: &["yournewresource", "ynr"],
   },
   ```

7. **Add watch call** in `src/watcher/mod.rs` `watch_all()`:

   ```rust
   self.watch::<resource::YourNewResource>()?;
   ```

8. **Add fetch_resource! case** in `src/tui/mod.rs` `fetch_resource_yaml()`:

   ```rust
   Some(FluxResourceKind::YourNewResource) => fetch_resource!(YourNewResource),
   ```

9. **Add get_gvk case** in `src/tui/api.rs` `get_gvk_for_resource_type()`:

   ```rust
   Some(FluxResourceKind::YourNewResource) => (
       YourNewResource::api_group(),
       YourNewResource::api_version(),
       YourNewResource::plural(),
   ),
   ```

10. **Update tests** in `tests/resource_registry.rs`:
    ```rust
    let expected_types = vec![
        // ... existing types
        "YourNewResource",
    ];
    ```

## Adding Submenu Commands

The submenu system allows commands to present interactive selection menus when executed without arguments. This provides a more user-friendly way to select from available options.

### System Architecture

The submenu system consists of:
- **`SubmenuItem`** - Represents a single selectable item with display text and value
- **`SubmenuState`** - Manages submenu state (items, selection, scroll, title, help text)
- **`CommandSubmenu` trait** - Interface for commands that provide submenus
- **Submenu view** (`src/tui/views/submenu.rs`) - Renders the submenu overlay as a centered popup

### Implementation Flow

1. **Define a submenu provider** - Implement the `CommandSubmenu` trait for your command
2. **Register the submenu** - Add to `get_command_submenu()` in `src/tui/commands.rs`
3. **Handle selection** - When a user selects an item, the command is re-executed with the selected value as an argument

### Submenu Navigation

Users navigate submenus using:
- `j` / `k` or `↓` / `↑` - Navigate through options
- `Enter` - Select the highlighted option
- `Esc` - Cancel and close submenu

### Event Handling Priority

The submenu system follows this priority in event handling (see `src/tui/app/events.rs`):
1. Confirmation dialogs (highest priority)
2. Submenu navigation
3. Filter mode
4. Normal command/key handling (lowest priority)

This ensures submenus capture navigation keys when active without interfering with other modes.

### Reference Implementation

See `ContextSubmenu` in `src/tui/commands.rs` for a complete working example of the submenu system.

## Testing

### Running Tests

```bash
# Run all tests
cargo test

# Run unit tests only
cargo test --lib --tests

# Run specific test suite
cargo test --test crd_compatibility
cargo test --test resource_registry
cargo test --test model_compatibility
cargo test --test field_extraction
cargo test --test trace_tests

# Run integration tests
cargo test --test crd_compatibility --test resource_registry --test model_compatibility --test field_extraction --test trace_tests
```

### Test Organization

- **Unit Tests** (`src/**/tests`) - Test individual components
- **Integration Tests** (`tests/`) - Test CRD compatibility and cross-component integration

### Test Coverage

- **CRD Compatibility** (`tests/crd_compatibility.rs`) - Ensures status field extraction works with various CRD structures
- **Resource Registry** (`tests/resource_registry.rs`) - Verifies all resource types are registered and have command aliases
- **Model Compatibility** (`tests/model_compatibility.rs`) - Ensures generated models compile and API versions are correct
- **Field Extraction** (`tests/field_extraction.rs`) - Tests resource-specific field extraction
- **Trace Tests** (`tests/trace_tests.rs`) - Tests trace operation functionality

### CI/CD Testing

GitHub Actions automatically runs (`.github/workflows/ci.yml`):

- Formatting checks (`cargo fmt`)
- Linting (`cargo clippy`) with warnings treated as errors
- All test suites
- Build verification

## Publishing

### Publishing to Crates.io

#### Prerequisites

1. Create an account on [crates.io](https://crates.io/users/sign_up)
2. Get your API token from [Account Settings](https://crates.io/me)
3. Add the token to cargo:
   ```bash
   cargo login <your-api-token>
   ```

#### Pre-publishing Checklist

- [ ] Version number updated in `Cargo.toml`
- [ ] All tests pass: `cargo test`
- [ ] Clippy passes: `cargo clippy -- -D warnings`
- [ ] Documentation is up to date
- [ ] README.md is complete
- [ ] LICENSE file exists
- [ ] CHANGELOG.md is updated

#### Publishing Steps

1. **Update version in Cargo.toml**:

   ```toml
   [package]
   version = "0.3.1"  # Increment as needed
   ```

2. **Verify the package**:

   ```bash
   cargo package
   ```

3. **Publish to crates.io**:

   ```bash
   cargo publish
   ```

4. **Verify publication**:
   Visit https://crates.io/crates/flux9s

#### Version Management

Follow [Semantic Versioning](https://semver.org/):

- **MAJOR** - Breaking changes
- **MINOR** - New features, backwards compatible
- **PATCH** - Bug fixes, backwards compatible

### Automated Publishing

The GitHub Actions workflows automate:

- **`.github/workflows/prepare-release.yml`** - Prepares release by updating version
- **`.github/workflows/release.yml`** - Builds binaries for Linux, macOS, Windows and publishes to crates.io
- **`.github/workflows/auto-tag-release.yml`** - Automatically tags releases

To trigger:

1. Update version in `Cargo.toml`
2. Push to main branch
3. Workflows automatically build, test, and publish

### Weekly CRD Updates

The `.github/workflows/check-crd-updates.yml` workflow:

- Runs every Monday at 9 AM UTC
- Checks for latest Flux CRD versions
- Downloads and compares CRDs
- Regenerates models if changed
- Creates a PR if updates are available

## Design Decisions

### Version Control Strategy

**Decision**: Generated models (`_generated/*.rs`) and CRDs (`crds/*.yaml`) are version controlled.

**Rationale**:

- Simplified releases and builds (no kopium dependency for users)
- Reproducible builds
- Easier CI/CD
- Users can build immediately without additional tools

### Namespace Handling

**Decision**: Use `Api::namespaced` when namespace is specified, `Api::all` otherwise.

**Rationale**:

- More efficient than always watching all namespaces
- Reduces API load on large clusters
- Allows efficient namespace switching
- Watchers restart when namespace changes

### Error Handling

**Decision**: Graceful handling of missing CRDs with throttling.

**Rationale**:

- Prevents API spam from 404 errors
- Allows application to work even if some CRDs aren't installed
- Provides better user experience
- Stops watcher immediately on 404 (CRD doesn't exist)

### Operation System

**Decision**: Trait-based extensible operation system.

**Rationale**:

- Easy to add new operations
- Type-safe operation handling
- Consistent operation interface
- Testable operations
- Support for confirmation dialogs

### Testing Strategy

**Decision**: Comprehensive test suite focusing on CRD compatibility.

**Rationale**:

- Catches breaking changes when CRDs update
- Ensures status field extraction works correctly
- Verifies API version consistency
- Provides confidence in updates
- Tests resource registry completeness

### Configuration System

**Decision**: YAML-based configuration with environment variable overrides.

**Rationale**:

- Human-readable configuration
- Easy to edit and version control
- Environment variables for CI/CD
- System-specific configuration directories
- Theme support with external files

## Implementation Status

### Completed Features

- ✅ Real-time resource monitoring via Watch API
- ✅ K9s-inspired TUI with navigation
- ✅ Unified and type-specific resource views
- ✅ Resource operations (suspend, resume, delete, reconcile, reconcile with source)
- ✅ YAML viewing
- ✅ Namespace switching
- ✅ Status indicators
- ✅ Filtering and command mode with autocomplete
- ✅ Comprehensive test suite
- ✅ CI/CD workflows
- ✅ Automated CRD update checking
- ✅ Trace operation for resource ownership chains
- ✅ Configuration system with YAML files
- ✅ Theme support with customizable skins
- ✅ CLI commands for configuration management
- ✅ Support for all Flux controller resources
- ✅ Support for Flux Operator resources
- ✅ Version-agnostic resource watching

### In Progress

- 🔄 Performance optimizations for large clusters
- 🔄 Enhanced error messages and diagnostics

### Planned Enhancements

- ⏳ Custom column configuration
- ⏳ Multiple cluster support
- ⏳ Plugin/extensions system
- ⏳ Resource age and last reconciled columns
- ⏳ Advanced filtering options

## Contributing

Contributions are welcome! Please:

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests for new functionality
5. Ensure all tests pass (`cargo test`)
6. Run clippy (`cargo clippy -- -D warnings`)
7. Format code (`cargo fmt`)
8. Submit a pull request

### Code Style

- Follow Rust standard formatting (`cargo fmt`)
- Run clippy before submitting (`cargo clippy -- -D warnings`)
- Write tests for new features
- Update documentation as needed
- Add examples to `docs/flux-crds.yaml` for new resource types

## Resources

- [K9s](https://github.com/derailed/k9s) - Inspiration for the UI
- [kube-rs](https://github.com/kube-rs/kube) - Kubernetes client library
- [kopium](https://github.com/kube-rs/kopium) - CRD to Rust model generator
- [ratatui](https://github.com/ratatui-org/ratatui) - Terminal UI framework
- [Flux CD](https://github.com/fluxcd/flux2) - GitOps toolkit
- [Flux Operator](https://github.com/controlplaneio-fluxcd/flux-operator) - Flux installation operator
- [Flux CRD Reference](https://github.com/controlplaneio-fluxcd/flux-operator/blob/main/api/v1/common_types.go#L83-L110) - Definitive list of Flux CRDs and API versions
