---
title: "Getting Started"
linkTitle: "Getting Started"
weight: 2
description: "Install and configure flux9s to start monitoring your Flux resources"
toc: true
type: docs
---

## What Is flux9s?

`flux9s` is a terminal UI for watching Flux resources and the cluster state around them in real time. It is designed for operators who already live in a shell and want fast visibility into what Flux is doing, how resources relate to each other, and whether quick intervention is needed.

That includes core Flux resources such as `Kustomization`, `HelmRelease`, and source objects, plus Flux Operator resources such as `FluxInstance` and `ResourceSet`. From the same interface you can inspect YAML, trace ownership, open graph and history views, and run common actions like suspend, resume, and reconcile.

flux9s reached 1.0 in October 2026 — see the [GA announcement on the Flux blog](https://fluxcd.io/blog/2026/10/flux9s-ga/).

## Why Use It If Flux Operator Has a Web UI?

The [Flux Operator Web UI](https://fluxoperator.dev/web-ui/) is built for the browser. `flux9s` is for the terminal: keyboard navigation, fast context switching, live watches, and actions without leaving your shell. Many people use both.

## Quick Start

### 1. Install flux9s

Choose your preferred installation method from the [Installation Guide](/getting-started/installation/):

- **Homebrew** (macOS/Linux): `brew install dgunzy/tap/flux9s`
- **cargo-binstall**: `cargo binstall flux9s`
- **From source**: `cargo install flux9s`

### 2. Prerequisites

- A Kubernetes cluster with Flux 2.2 or later installed
- `kubeconfig` configured to access your cluster

### 3. Run flux9s

```bash
flux9s
```

Or use a specific kubeconfig file:

```bash
flux9s --kubeconfig /path/to/kubeconfig
```

By default, `flux9s` watches the `flux-system` namespace. Use `:ns all` to view all namespaces or `:ns <namespace>` to switch to a specific namespace.

{{% alert title="Note" color="info" %}}
`flux9s` starts in read-only mode.  
Change this with `flux9s config set readOnly false` or toggle it in a session using `:readonly`.
{{% /alert %}}

## Next Steps

- **[Installation Guide](/getting-started/installation/)** - Every install method and platform
- **[User Guide](/user-guide/)** - Keys, commands, views, and operations
- **[Configuration](/configuration/)** - Every config option, views, themes, TLS and proxies
