---
title: "Developer Guide"
linkTitle: "Developer Guide"
weight: 5
description: "Information for developers contributing to flux9s"
toc: true
type: docs
---

## Architecture

flux9s follows a modified Elm Architecture pattern with async task spawning:

{{< blocks/section color="white" >}}
{{% blocks/feature icon="fa-database" title="Model" %}}
`App` struct holds all application state in a centralized location. State is organized into logical sub-structures for better maintainability.
{{% /blocks/feature %}}

{{% blocks/feature icon="fa-sync" title="Update" %}}
`handle_key()` processes events and updates state synchronously. Event handling is centralized in `src/tui/app/events.rs`.
{{% /blocks/feature %}}

{{% blocks/feature icon="fa-eye" title="View" %}}
`render()` displays current state using stateless components. Views are organized in `src/tui/views/` and receive all needed data as parameters.
{{% /blocks/feature %}}

{{% blocks/feature icon="fa-bolt" title="Async Layer" %}}
Spawned tasks + oneshot channels handle I/O operations without blocking the UI. Async operations are managed in `src/tui/app/async_ops.rs`.
{{% /blocks/feature %}}
{{< /blocks/section >}}

## Project Structure

```
flux9s/
├── src/
│   ├── cli/          # CLI command parsing
│   ├── config/       # Configuration management
│   ├── kube/         # Kubernetes client wrapper
│   ├── models/       # Generated and custom models
│   ├── trace/        # Resource tracing
│   ├── tui/          # Terminal UI
│   │   ├── app/      # Application state and logic (refactored)
│   │   │   ├── core.rs       # App struct and core logic
│   │   │   ├── state.rs      # State structures
│   │   │   ├── events.rs     # Event handling
│   │   │   ├── rendering.rs  # Render orchestration
│   │   │   └── async_ops.rs  # Async operations
│   │   ├── views/    # View components
│   │   ├── submenu.rs        # Submenu system
│   │   ├── keybindings.rs    # Keybinding management
│   │   └── ...      # Other TUI modules
│   └── watcher/      # Resource watching
├── crds/             # Flux CRD files
├── tests/            # Integration tests
└── docs/             # Documentation
```

### Recent Architecture Changes

**App Module Refactoring:** The application logic has been refactored from a single `app.rs` file into a modular structure under `src/tui/app/`:

- **`core.rs`** - Main App struct and core logic
- **`state.rs`** - Organized state structures (ViewState, SelectionState, UIState, AsyncOperationState)
- **`events.rs`** - Event handling and input processing
- **`rendering.rs`** - Rendering orchestration
- **`async_ops.rs`** - Async operation management

This separation improves code organization, maintainability, and makes the codebase easier to navigate.

**Submenu System:** An interactive submenu system has been added for commands like `:ctx` and `:skin`, providing a user-friendly way to select from available options. The system is built using the `CommandSubmenu` trait and can be extended to other commands. See the main `DEVELOPER_GUIDE.md` for implementation details.

**Keybinding Centralization:** All keybindings are now centralized in `src/tui/keybindings.rs`, providing a single source of truth for footer rendering, help text, and layout calculations.

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
make ci
```

This runs:

- `cargo fmt` - Format code
- `cargo clippy` - Lint code
- `cargo test` - Run tests

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

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
