---
title: "Flux Resources"
linkTitle: "Flux Resources"
weight: 2
description: "Suspend, resume, reconcile, delete, edit, history, and favorites"
type: docs
---

## Operations

Select a resource in the list (or open its detail, YAML, or graph) and press:

| Key | Operation | Applies to |
| --- | --- | --- |
| `s` | Suspend reconciliation | Resources with `spec.suspend` (GitRepository, OCIRepository, HelmRepository, Kustomization, HelmRelease, ImageUpdateAutomation, …); Flux Operator kinds use their reconcile annotation |
| `r` | Resume reconciliation | Same as suspend |
| `R` | Reconcile now | All Flux resources (not while suspended) |
| `W` | Reconcile with its source | Kustomization, HelmRelease |
| `Ctrl+d` | Delete (asks first) | All Flux resources |

Read-only mode (`readOnly: true`, or `:readonly` for the session) blocks every
write.

## Editing

`e` opens the resource in your editor. flux9s fetches it, opens a temporary
`.yaml` file, and applies your changes with server-side apply when you save
and quit. Saving without changes does nothing; a conflicting change made
meanwhile is rejected rather than overwritten. You return to the view you
pressed `e` in.

**Editor selection**, highest priority first:

1. `FLUX9S_EDITOR`
2. the `editor` config key (`flux9s config set editor vim`)
3. `$VISUAL`
4. `$EDITOR`
5. `vi`

GUI editors (VS Code, Sublime Text, Atom, Zed) get `--wait` automatically.

With native resources on, `e` also edits non-Flux objects, with a warning
first when Flux or another tool manages them — see
[Editing non-Flux objects](../native-resources/#editing-non-flux-objects).

## Reconciliation history (`h`)

For FluxInstances, ResourceSets, Kustomizations, and HelmReleases that track
it: each reconciliation's time, revision, status, and message. Step-based
ResourceSets include each snapshot's step count.

## Favorites (`f`)

`f` toggles a favorite. Favorites are saved to your config, listed first in
every resource list, and gathered in `:favorites` (`:fav`).

## ResourceSet steps

Step-based ResourceSets (Flux Operator v0.53+) show their ordered steps in the
detail view (`Enter`): each step's phase (done, applying, failed, or pending)
live from the reconciliation status, its resource count, template marker,
and timeout.

## Supported kinds

flux9s watches every Flux kind from the Flux controllers and the Flux
Operator, and resolves each one's API version at runtime, so it keeps
working as Flux promotes CRDs.

| Controller | Kinds |
| --- | --- |
| Source (`source.toolkit.fluxcd.io`) | GitRepository, OCIRepository, HelmRepository, Bucket, HelmChart, ExternalArtifact |
| Source Watcher (`source.extensions.fluxcd.io`) | ArtifactGenerator |
| Kustomize (`kustomize.toolkit.fluxcd.io`) | Kustomization |
| Helm (`helm.toolkit.fluxcd.io`) | HelmRelease |
| Image Reflector (`image.toolkit.fluxcd.io`) | ImageRepository, ImagePolicy |
| Image Automation (`image.toolkit.fluxcd.io`) | ImageUpdateAutomation |
| Notification (`notification.toolkit.fluxcd.io`) | Alert, Provider, Receiver |
| Flux Operator (`fluxcd.controlplane.io`) | FluxInstance, FluxReport, ResourceSet, ResourceSetInputProvider |

CRDs labeled `app.kubernetes.io/part-of=flux` (Flagger, tofu-controller, …)
can join the list too — see
[Discovering Flux-adjacent kinds](/configuration/#discovering-flux-adjacent-resource-kinds).
