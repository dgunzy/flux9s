# Developer Guide

This guide provides comprehensive information for developers working on flux9s, including architecture, design decisions, development workflow, testing, and publishing.

## Table of Contents

- [Project Overview](#project-overview)
- [Architecture](#architecture)
- [Project Structure](#project-structure)
- [Development Workflow](#development-workflow)
- [Testing](#testing)
- [Publishing](#publishing)
- [Design Decisions](#design-decisions)
- [Implementation Status](#implementation-status)

## Project Overview

flux9s is a K9s-inspired terminal UI for monitoring Flux GitOps resources in real-time. It's built in Rust with automated CRD-to-model generation to minimize maintenance overhead.

### Key Features

- **Real-time monitoring** via Kubernetes Watch API
- **Zero-maintenance model updates** from Flux CRDs
- **Familiar K9s-style navigation** and keybindings
- **Extensible architecture** for easy addition of new resource types
- **Comprehensive test suite** for CRD compatibility

### Technology Stack

- **Rust** - Systems programming language
- **kube-rs** - Kubernetes client library
- **kopium** - CRD to Rust model generator
- **ratatui** - Terminal UI framework
- **tokio** - Async runtime

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
│  │   list, etc) │    │   safe)      │    │              │  │
│  └──────────────┘    └──────────────┘    └──────────────┘  │
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

- **`mod.rs`** - Main watcher orchestration, watch event handling
- **`resource.rs`** - Resource type definitions and `WatchableResource` trait implementations
- **`state.rs`** - Thread-safe resource state management
- **`registry.rs`** - Resource registry for command mapping

**Key Design Decisions:**
- Uses `Api::namespaced` for efficiency when namespace is specified
- Falls back to `Api::all` for cluster-wide watching
- Handles CRD absence gracefully (404 errors)
- Implements error throttling to prevent API spam

#### 2. TUI Module (`src/tui/`)

Terminal user interface built with ratatui.

- **`app.rs`** - Main application state and event loop
- **`operations.rs`** - Flux operations (suspend, resume, delete, reconcile)
- **`theme.rs`** - Theme configuration (prepared for future customization)
- **`views/`** - View components (header, footer, resource list, detail, YAML, etc.)

**Key Design Decisions:**
- Non-blocking async operations using `tokio::spawn`
- Separate scroll offsets for different views
- Dynamic footer wrapping for smaller screens
- Extensible operation system via trait-based design

#### 3. Models Module (`src/models/`)

Generated Rust types from Flux CRDs.

- **`_generated/`** - Auto-generated models from kopium
- **`extensions.rs`** - Manual extensions and helper traits

**Key Design Decisions:**
- Generated models are version controlled for reproducible builds
- Models can be regenerated when CRDs update
- Extensions provide common functionality across resource types

## Project Structure

```
flux9s/
├── .github/
│   └── workflows/          # CI/CD workflows
│       ├── ci.yml          # PR and push testing
│       ├── release.yml     # Release automation
│       └── check-crd-updates.yml  # Weekly CRD update checks
├── crds/                    # Flux CRD files (version controlled)
│   ├── source-controller.crds.yaml
│   ├── kustomize-controller.crds.yaml
│   └── ...
├── src/
│   ├── kube/               # Kubernetes client setup
│   ├── models/             # Generated and extended models
│   │   ├── _generated/     # Auto-generated (version controlled)
│   │   └── extensions.rs   # Manual extensions
│   ├── tui/                # Terminal UI
│   │   ├── app.rs          # Main app state
│   │   ├── operations.rs   # Flux operations
│   │   ├── theme.rs        # Theme configuration
│   │   └── views/          # View components
│   ├── watcher/            # Resource watching
│   │   ├── mod.rs          # Watcher orchestration
│   │   ├── resource.rs     # Resource definitions
│   │   ├── state.rs        # State management
│   │   └── registry.rs     # Resource registry
│   ├── lib.rs              # Library entry point
│   └── main.rs             # Binary entry point
├── tests/                   # Test suite
│   ├── crd_compatibility.rs    # CRD compatibility tests
│   ├── resource_registry.rs    # Registry tests
│   ├── model_compatibility.rs  # Model tests
│   └── field_extraction.rs     # Field extraction tests
├── Cargo.toml              # Rust project configuration
├── LICENSE                 # Apache 2.0 License
├── README.md              # User-facing documentation
└── DEVELOPER_GUIDE.md     # This file
```

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
```

### Updating CRDs and Models

When Flux CRDs are updated:

1. **Download new CRDs** (or use the automated workflow):
   ```bash
   # Example: Download source-controller CRDs
   curl -L "https://github.com/fluxcd/source-controller/releases/download/v1.7.3/source-controller.crds.yaml" \
     -o crds/source-controller.crds.yaml
   ```

2. **Regenerate models**:
   ```bash
   # Install kopium if needed
   cargo install kopium
   
   # Generate models (adjust paths as needed)
   kopium --file crds/source-controller.crds.yaml --output src/models/_generated/
   ```

3. **Run tests** to ensure compatibility:
   ```bash
   cargo test --test crd_compatibility
   ```

4. **Commit changes**:
   ```bash
   git add crds/ src/models/_generated/
   git commit -m "chore: update Flux CRDs to latest versions"
   ```

### Adding a New Resource Type

1. **Ensure CRD is included** in model generation
2. **Re-export the type** in `src/watcher/resource.rs`:
   ```rust
   pub use source_controller::YourNewResource;
   ```

3. **Add `impl_watchable!` macro**:
   ```rust
   impl_watchable!(
       YourNewResource,
       "source.toolkit.fluxcd.io",
       "v1",
       "yournewresources",
       "YourNewResource"
   );
   ```

4. **Add to registry** in `src/watcher/registry.rs`:
   ```rust
   ResourceEntry {
       display_name: "YourNewResource",
       command_aliases: &["yournewresource", "ynr"],
   },
   ```

5. **Add watch call** in `src/watcher/mod.rs` `watch_all()`:
   ```rust
   self.watch::<resource::YourNewResource>()?;
   ```

## Testing

### Running Tests

```bash
# Run all tests
cargo test

# Run specific test suite
cargo test --test crd_compatibility
cargo test --test resource_registry
cargo test --test model_compatibility
cargo test --test field_extraction

# Run unit tests only
cargo test --lib --tests
```

### Test Organization

- **Unit Tests** (`src/**/tests`) - Test individual components
- **Integration Tests** (`tests/`) - Test CRD compatibility and cross-component integration

### Test Coverage

- **CRD Compatibility** - Ensures status field extraction works with various CRD structures
- **Resource Registry** - Verifies all resource types are registered
- **Model Compatibility** - Ensures generated models compile and API versions are correct
- **Field Extraction** - Tests resource-specific field extraction

### CI/CD Testing

GitHub Actions automatically runs:
- Formatting checks (`cargo fmt`)
- Linting (`cargo clippy`)
- All test suites
- Build verification on multiple platforms

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
- [ ] Documentation is up to date
- [ ] README.md is complete
- [ ] LICENSE file exists
- [ ] CHANGELOG.md is updated

#### Publishing Steps

1. **Update version in Cargo.toml**:
   ```toml
   [package]
   version = "0.1.0"  # Increment as needed
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

The GitHub Actions workflow (`.github/workflows/release.yml`) automates:
- Building binaries for Linux, macOS, Windows
- Publishing to crates.io (requires `CRATES_IO_TOKEN` secret)
- Creating GitHub releases with binaries

To trigger:
1. Create and push a tag: `git tag v0.1.0 && git push origin v0.1.0`
2. Workflow automatically builds, tests, and publishes

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

### Error Handling

**Decision**: Graceful handling of missing CRDs with throttling.

**Rationale**:
- Prevents API spam from 404 errors
- Allows application to work even if some CRDs aren't installed
- Provides better user experience

### Operation System

**Decision**: Trait-based extensible operation system.

**Rationale**:
- Easy to add new operations
- Type-safe operation handling
- Consistent operation interface
- Testable operations

### Testing Strategy

**Decision**: Comprehensive test suite focusing on CRD compatibility.

**Rationale**:
- Catches breaking changes when CRDs update
- Ensures status field extraction works correctly
- Verifies API version consistency
- Provides confidence in updates

## Implementation Status

### Completed Features

- ✅ Real-time resource monitoring via Watch API
- ✅ K9s-inspired TUI with navigation
- ✅ Unified and type-specific resource views
- ✅ Resource operations (suspend, resume, delete, reconcile)
- ✅ YAML viewing
- ✅ Namespace switching
- ✅ Status indicators
- ✅ Filtering and command mode
- ✅ Comprehensive test suite
- ✅ CI/CD workflows
- ✅ Automated CRD update checking

### In Progress

- 🔄 Additional resource-specific views
- 🔄 Performance optimizations
- 🔄 Enhanced error messages

### Planned Enhancements

- ⏳ Trace operation for viewing reconciliation logs
- ⏳ Custom column configuration
- ⏳ Multiple cluster support
- ⏳ Plugin/extensions system
- ⏳ Theme customization
- ⏳ Resource age and last reconciled columns

## Contributing

Contributions are welcome! Please:

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests for new functionality
5. Ensure all tests pass
6. Submit a pull request

### Code Style

- Follow Rust standard formatting (`cargo fmt`)
- Run clippy before submitting (`cargo clippy`)
- Write tests for new features
- Update documentation as needed

## Resources

- [K9s](https://github.com/derailed/k9s) - Inspiration for the UI
- [kube-rs](https://github.com/kube-rs/kube) - Kubernetes client library
- [kopium](https://github.com/kube-rs/kopium) - CRD to Rust model generator
- [ratatui](https://github.com/ratatui-org/ratatui) - Terminal UI framework

