---
title: "Getting Started"
linkTitle: "Getting Started"
weight: 2
description: "Install and configure flux9s to start monitoring your Flux resources"
toc: true
type: docs
---

## Quick Start

Follow these steps to get started with flux9s:

### 1. Install flux9s

Choose your preferred installation method from the [Installation Guide](/getting-started/installation/):

- **Homebrew** (macOS/Linux): `brew install dgunzy/tap/flux9s`
- **cargo-binstall**: `cargo binstall flux9s`
- **From source**: `cargo install flux9s`

### 2. Prerequisites

- A Kubernetes cluster with Flux CD installed
- `kubeconfig` configured to access your cluster

### 3. Run flux9s

```bash
flux9s
```

By default, `flux9s` watches the `flux-system` namespace. Use `:ns all` to view all namespaces or `:ns <namespace>` to switch to a specific namespace.

{{% alert title="Note" color="info" %}}
`flux9s` launches in readonly mode by default.  
You can change this with `flux9s config set readOnly false` or toggle it in a session using `:readonly`.
{{% /alert %}}

## Next Steps

{{< blocks/section color="white" >}}
{{% blocks/feature icon="fa-download" title="Installation Guide" url="/getting-started/installation/" %}}
Detailed installation instructions for all platforms and methods.
{{% /blocks/feature %}}

{{% blocks/feature icon="fa-book" title="User Guide" url="/user-guide/" %}}
Learn how to navigate and use flux9s effectively.
{{% /blocks/feature %}}

{{% blocks/feature icon="fa-cog" title="Configuration" url="/configuration/" %}}
Customize flux9s to suit your workflow and preferences.
{{% /blocks/feature %}}
{{< /blocks/section >}}
