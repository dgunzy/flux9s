---
title: "User Guide"
linkTitle: "User Guide"
weight: 3
description: "Learn how to use flux9s to monitor and manage Flux resources"
toc: true
type: docs
---

## Navigation

Use these keyboard shortcuts to navigate flux9s:

| Key       | Action                                                  |
| --------- | ------------------------------------------------------- |
| `j` / `k` | Navigate up/down                                        |
| `Enter`   | View resource details                                   |
| `y`       | View resource YAML                                      |
| `t`       | Trace ownership chain                                   |
| `Esc`     | Go back / Quit                                          |
| `/`       | Filter resources by name                                |
| `:`       | Command mode (e.g., `:kustomization`, `:gitrepository`) |
| `Tab`     | Autocomplete command                                    |
| `?`       | Show/hide help                                          |

## Commands

Type these commands in command mode (press `:`):

| Command           | Description                    |
| ----------------- | ------------------------------ |
| `:ns <namespace>` | Switch to a specific namespace |
| `:ns all`         | View all namespaces            |
| `:all`            | Show all resources             |
| `:skin <name>`    | Change theme/skin              |
| `:readonly`       | Toggle readonly mode           |
| `:help`           | Show/hide help                 |
| `:q` or `:q!`     | Quit application               |

## Operations

Perform actions on selected resources:

| Key | Operation              | Valid For                              |
| --- | ---------------------- | -------------------------------------- |
| `s` | Suspend reconciliation | All Flux resources                     |
| `r` | Resume reconciliation  | All Flux resources                     |
| `R` | Reconcile resource     | All Flux resources                     |
| `W` | Reconcile with source  | Kustomization, HelmRelease             |
| `d` | Delete resource        | All Flux resources (with confirmation) |

## Terminal Commands

Configure flux9s from the command line:

```bash
# Show all config options
flux9s config --help

# Set a configuration value
flux9s config set {KEY} {VALUE}

# Set a skin for readonly mode
flux9s config set ui.skinReadOnly rose-pine

# Import and set a skin
flux9s config skins set navy.yaml
```

{{% alert title="Skin Compatibility" color="warning" %}}
Not all K9s skins are compatible with flux9s. flux9s skins follow a similar format but may require adjustments to work properly.
{{% /alert %}}

## Screenshots

{{< blocks/section color="white" >}}
{{% blocks/feature icon="fa-image" title="Main View" %}}
![flux9s screenshot](/images/screenshot.png)

The main resource view showing all Flux resources in your cluster.
{{% /blocks/feature %}}

{{% blocks/feature icon="fa-sitemap" title="Trace View" %}}
![flux9s trace](/images/trace-screenshot.png)

Visualize resource relationships and ownership chains.
{{% /blocks/feature %}}

{{% blocks/feature icon="fa-filter" title="Filter View" %}}
![flux9s filter](/images/filter-screenshot.png)

Quickly find resources by name using the filter feature.
{{% /blocks/feature %}}
{{< /blocks/section >}}
