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
`App` struct holds all application state in a centralized location.
{{% /blocks/feature %}}

{{% blocks/feature icon="fa-sync" title="Update" %}}
`handle_key()` processes events and updates state synchronously.
{{% /blocks/feature %}}

{{% blocks/feature icon="fa-eye" title="View" %}}
`render()` displays current state using stateless components.
{{% /blocks/feature %}}

{{% blocks/feature icon="fa-bolt" title="Async Layer" %}}
Spawned tasks + oneshot channels handle I/O operations without blocking the UI.
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
│   └── watcher/      # Resource watching
├── crds/             # Flux CRD files
├── tests/            # Integration tests
└── docs/             # Documentation
```

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
