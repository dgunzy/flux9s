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
| `:`       | Command mode (e.g., `:kustomization`, `:gitrepository`) |
| `Enter`   | View resource details                                   |
| `/`       | Filter resources by name                                |
| `s`       | Suspend reconciliation                                 |
| `r`       | Resume reconciliation                                  |
| `R`       | Reconcile resource                                     |
| `y`       | View resource YAML                                      |
| `f`       | Toggle favorite                                         |
| `g`       | View resource graph (Kustomization, HelmRelease, etc.)  |
| `h`       | View reconciliation history                             |
| `t`       | Trace ownership chain                                   |
| `W`       | Reconcile with source                                  |
| `d`       | Delete resource                                         |
| `?`       | Show/hide help                                          |
| `Esc`     | Go back / Quit                                          |
| `Tab`     | Autocomplete command                                    |

## Commands

Type these commands in command mode (press `:`):

| Command           | Description                                    |
| ----------------- | ---------------------------------------------- |
| `:ctx <name>`     | Switch to a different Kubernetes context       |
| `:ctx`            | Open interactive context selection menu        |
| `:ns <namespace>` | Switch to a specific namespace                 |
| `:ns all`         | View all namespaces                            |
| `:all`            | Show all resources                       |
| `:healthy`        | Show only healthy resources              |
| `:unhealthy`      | Show only unhealthy resources            |
| `:favorites`      | View favorite resources                  |
| `:fav`            | Alias for `:favorites`                   |
| `:skin <name>`    | Change theme/skin (direct)               |
| `:skin`           | Open interactive theme selection menu    |
| `:readonly`       | Toggle readonly mode                     |
| `:help`           | Show/hide help                           |
| `:q` or `:q!`     | Quit application                         |

## Interactive Submenus

Some commands open interactive selection menus when used without arguments, providing an easier way to select from available options.

### Interactive Submenus

Some commands open interactive selection menus when used without arguments, providing an easier way to select from available options.

#### Context Submenu (`:ctx`)

When you type `:ctx` and press Enter without specifying a context name, flux9s displays an interactive menu of available Kubernetes contexts. The current context is marked with "(current)".

**Navigation:**
- `j` / `k` or `↓` / `↑` - Navigate through options
- `Enter` - Select the highlighted context
- `Esc` - Cancel and close submenu

The submenu appears as a centered overlay on top of the current view, making it easy to see and select your desired context without needing to remember exact names.

#### Theme Submenu (`:skin`)

When you type `:skin` and press Enter without specifying a theme name, flux9s displays an interactive menu of available themes with live preview.

![Theme Submenu](/images/skin-submenu.png)

**Features:**
- **Live Preview**: Theme changes immediately as you navigate
- **Current Theme**: Marked with "(current)"
- **Built-in Themes**: Embedded themes marked with "[built-in]"
- **17 Built-in Themes**: Includes popular themes like dracula, nord, monokai, gruvbox-dark, and more

**Navigation:**
- `j` / `k` or `↓` / `↑` - Navigate through themes (with live preview)
- `Enter` - Apply theme temporarily (session only)
- `s` - Save theme to config file (persists across sessions)
- `Esc` - Cancel and restore original theme

The submenu saves themes to `ui.skin` in normal mode, or `ui.skinReadOnly` when readonly mode is enabled.

## Health Filtering

Filter resources by health status:

- **`:healthy`** - Show only healthy resources (ready=true, not suspended, or null status)
- **`:unhealthy`** - Show only unhealthy resources (ready=false or suspended=true)
- **`:all`** - Clear health filter and show all resources

The header displays a health percentage indicator showing the overall health of your resources. The indicator uses color coding:
- **Green (●)** - 90% or higher health
- **Yellow (⚠)** - 70-89% health
- **Red (✗)** - Below 70% health

## Resource Views

### Graph View (`g`)

Visualize resource relationships and dependencies. Shows upstream sources and downstream managed resources.

**Supported resource types:**
- Kustomization
- HelmRelease
- ArtifactGenerator
- FluxInstance
- ResourceSet

The graph view displays:
- **Upstream sources** (GitRepository, HelmRepository, etc.)
- **Managed resources** (workloads, ConfigMaps, Services, etc.)
- **Resource groups** (aggregated by type)
- **Workload groups** (aggregated workloads with status)

### Reconciliation History (`h`)

View reconciliation history for resources that track it.

**Supported resource types:**
- FluxInstance
- ResourceSet
- Kustomization
- HelmRelease

The history view shows:
- Timestamp of each reconciliation
- Revision information
- Status (Success/Failed/Unknown)
- Messages from reconciliation events

### Favorites (`f`)

Mark frequently accessed resources as favorites for quick access.

- Press `f` on a resource to toggle favorite status
- Use `:favorites` or `:fav` command to view all favorites
- Favorites are saved to your configuration file
- Favorites appear first in resource lists

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
# Use a specific kubeconfig file
flux9s --kubeconfig /path/to/kubeconfig

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

{{% blocks/feature icon="fa-project-diagram" title="Graph View" %}}
![flux9s graph](/images/graph-screenshot.png)

Visualize resource relationships and dependencies in a graph format.

Shows upstream sources and downstream managed resources for Kustomization, HelmRelease, and other resources with inventory tracking.
{{% /blocks/feature %}}

{{% blocks/feature icon="fa-palette" title="Theme Selection" %}}
![flux9s theme submenu](/images/skin-submenu.png)

Interactive theme selection with live preview. Choose from 17 built-in themes or your custom themes.

Navigate with `j`/`k` to preview, press `Enter` to apply, or `s` to save to config.
{{% /blocks/feature %}}
{{< /blocks/section >}}
