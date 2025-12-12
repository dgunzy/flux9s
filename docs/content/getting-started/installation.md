---
title: "Installation"
linkTitle: "Installation"
weight: 1
description: "Install flux9s on your system"
toc: true
type: docs
---

## Quick Install

{{< blocks/section color="white" >}}
{{% blocks/feature icon="fa-beer" title="Homebrew" %}}
The easiest way to install on macOS and Linux:

```bash
brew install dgunzy/tap/flux9s
```

Or tap the repository first:

```bash
brew tap dgunzy/tap
brew install flux9s
```

{{% /blocks/feature %}}

{{% blocks/feature icon="fa-download" title="cargo-binstall" %}}
If you have [cargo-binstall](https://github.com/cargo-bins/cargo-binstall) installed:

```bash
cargo binstall flux9s
```

Downloads and installs pre-built binaries without compiling from source.
{{% /blocks/feature %}}

{{% blocks/feature icon="fa-box" title="Crates.io" %}}
Install directly from crates.io:

```bash
cargo install flux9s
```

Requires Rust toolchain to be installed.
{{% /blocks/feature %}}
{{< /blocks/section >}}

## Manual Download

Download pre-built binaries from the [Releases](https://github.com/dgunzy/flux9s/releases) page:

| Platform                  | File                          |
| ------------------------- | ----------------------------- |
| **Linux (x86_64)**        | `flux9s-linux-x86_64.tar.gz`  |
| **macOS (Intel)**         | `flux9s-macos-x86_64.tar.gz`  |
| **macOS (Apple Silicon)** | `flux9s-macos-aarch64.tar.gz` |
| **Windows (x86_64)**      | `flux9s-windows-x86_64.zip`   |

Extract and move the binary to a directory in your `PATH`.

## Build from Source

Build flux9s from the source repository:

```bash
git clone https://github.com/dgunzy/flux9s.git
cd flux9s
cargo build --release
```

The binary will be available at `target/release/flux9s`.

## Requirements

{{% alert title="Prerequisites" color="info" %}}

- **Rust 1.70+** (if compiling from source)
- **Kubernetes cluster** with Flux CD installed
- **kubeconfig** configured to access your cluster
  {{% /alert %}}

## Verify Installation

After installation, verify that flux9s is working:

```bash
flux9s --version
```

You should see the version number printed.
