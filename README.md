# flux9s

A [K9s](https://github.com/derailed/k9s)-inspired terminal UI for monitoring Flux GitOps resources in real-time.

![flux9s](https://img.shields.io/crates/v/flux9s)
![License](https://img.shields.io/crates/l/flux9s)
![Rust](https://img.shields.io/badge/rust-1.70%2B-orange)

## Overview

`flux9s` provides a terminal-based interface for monitoring and managing Flux CD resources, inspired by the excellent [K9s](https://github.com/derailed/k9s) project. It offers real-time monitoring of Flux Custom Resources (CRDs) including Kustomizations, GitRepositories, HelmReleases, and more.

### Features

- **Real-time monitoring** - Watch Flux resources as they change using Kubernetes Watch API
- **K9s-inspired interface** - Familiar navigation and keybindings for K9s users
- **Unified and type-specific views** - View all resources together or filter by type
- **Resource operations** - Suspend, resume, reconcile, and delete Flux resources
- **YAML viewing** - Inspect full resource manifests
- **Namespace switching** - Monitor resources across namespaces or cluster-wide
- **Status indicators** - Visual indicators for resource health and suspension state

## Installation

### From Crates.io (Recommended)

```bash
cargo install flux9s
```

### From Source

```bash
git clone https://github.com/dgunzy/flux9s.git
cd flux9s
cargo build --release
```

The binary will be available at `target/release/flux9s`.

### Pre-built Binaries

Pre-built binaries for Linux, macOS, and Windows are available on the [Releases](https://github.com/dgunzy/flux9s/releases) page.

## Quick Start

1. Ensure you have a Kubernetes cluster with Flux installed
2. Configure your `kubeconfig` to point to your cluster
3. Run `flux9s`

```bash
flux9s
```

By default, `flux9s` watches the `flux-system` namespace. Use `:ns all` to view all namespaces or `:ns <namespace>` to switch to a specific namespace.

## Usage

### Navigation

- `j` / `k` - Navigate up/down
- `Enter` - View resource details
- `y` - View resource YAML
- `Esc` - Go back / Quit
- `/` - Filter resources by name
- `:` - Command mode (e.g., `:kustomization`, `:gitrepository`)

### Commands

- `:ns <namespace>` - Switch namespace
- `:ns all` - View all namespaces
- `:q` or `:q!` - Quit
- `:help` - Show help

### Operations

- `s` - Suspend resource
- `r` - Resume resource
- `R` - Reconcile resource
- `d` - Delete resource (with confirmation)

## Acknowledgments

This project is inspired by and built with the following excellent tools:

- **[K9s](https://github.com/derailed/k9s)** - The terminal UI for Kubernetes that inspired this project
- **[kube-rs](https://github.com/kube-rs/kube)** - The Rust Kubernetes client library powering the Kubernetes API interactions
- **[kopium](https://github.com/kube-rs/kopium)** - The tool used to generate Rust types from Kubernetes CRDs

## AI Note

AI was used to get the scaffold of this project together, if there are mistakes or
issues please open an issue, or a PR!

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

This project is licensed under the Apache License, Version 2.0 - see the [LICENSE](LICENSE) file for details.
