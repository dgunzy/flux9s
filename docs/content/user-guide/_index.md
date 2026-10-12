---
title: "User Guide"
linkTitle: "User Guide"
weight: 3
description: "Learn how to use flux9s to monitor and manage Flux resources"
type: docs
---

flux9s is a keyboard-driven terminal UI for Flux. It watches your cluster
live, so everything on screen updates as Flux reconciles. Press `?` anytime
for the keys that work in the current view.

## Where to go next

| Page | What it covers |
| --- | --- |
| [Navigating](navigation/) | Keys, `:` commands, menus, filtering, sorting, and searching |
| [Flux Resources](flux-resources/) | Suspend, resume, reconcile, delete, edit, history, favorites, and the supported kinds |
| [Graph and Live Views](views/) | The graph, workloads and inventory, trace, HelmRelease values, events, pulse, and controller logs |
| [Native Resources (preview)](native-resources/) | Browse and act on any Kubernetes kind or CRD with `:<kind>` |
| [Command Line](command-line/) | `flux9s` flags, `config` subcommands, and shell completions |

## Demos

{{< rawhtml >}}

<div class="mb-4">
  <h3>Graph, workload detail, logs, and a restart</h3>
  <div class="ratio ratio-16x9" style="background: transparent;">
    <video autoplay loop muted playsinline class="w-100 h-100" style="object-fit: contain; background: transparent;" onerror="this.style.display='none'; this.nextElementSibling.style.display='block';">
      <source src="/images/demo-graph.mp4" type="video/mp4">
      Your browser does not support the video tag.
    </video>
    <div style="display:none; padding: 2rem; text-align: center;">
      <p class="mb-0"><strong>Graph view demo</strong> — video playback is not available.</p>
    </div>
  </div>
</div>

<div class="mb-4">
  <h3>Native resources: any kind, and what manages it</h3>
  <div class="ratio ratio-16x9" style="background: transparent;">
    <video autoplay loop muted playsinline class="w-100 h-100" style="object-fit: contain; background: transparent;" onerror="this.style.display='none'; this.nextElementSibling.style.display='block';">
      <source src="/images/demo-native.mp4" type="video/mp4">
      Your browser does not support the video tag.
    </video>
    <div style="display:none; padding: 2rem; text-align: center;">
      <p class="mb-0"><strong>Native resources demo</strong> — video playback is not available.</p>
    </div>
  </div>
</div>

<div class="mb-4">
  <h3>Theme selection with live preview</h3>
  <div class="ratio ratio-16x9" style="background: transparent;">
    <video autoplay loop muted playsinline class="w-100 h-100" style="object-fit: contain; background: transparent;" onerror="this.style.display='none'; this.nextElementSibling.style.display='block';">
      <source src="/images/demo-skin.mp4" type="video/mp4">
      Your browser does not support the video tag.
    </video>
    <div style="display:none; padding: 2rem; text-align: center;">
      <p class="mb-0"><strong>Theme selection demo</strong> — video playback is not available.</p>
    </div>
  </div>
</div>
{{< /rawhtml >}}

![The main resource list](/images/screenshot.png)
