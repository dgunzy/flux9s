# Plugin System Implementation Plan

## Overview

YAML-based plugin system enabling:
1. **Column enrichment** - Add columns to existing Flux resource views from external data
2. **Custom resource views** - Watch and display arbitrary CRDs with full TUI support (`:argo`, `:crossplane`, etc.)

**Status**: Phase 1 Complete, Phase 2 In Progress
**Target Version**: v0.7.0

## Philosophy

- **Optional** - flux9s works without plugins
- **Read-only** - No cluster modifications
- **Declarative** - YAML only, no code/templates
- **Extensible** - Watch any CRD, not just Flux resources

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                       Plugin System                          │
├─────────────────────────────────────────────────────────────┤
│  Plugin Manifest (YAML)                                      │
│  ├── source: data enrichment (K8s Service, HTTP, File)      │
│  ├── columns: add to existing views                          │
│  └── watched_resources: NEW resource views with CRD watching │
└─────────────────────────────────────────────────────────────┘
         │                              │
         ▼                              ▼
┌─────────────────┐          ┌─────────────────────┐
│ Column Renderer │          │ Dynamic Watcher     │
│ (enriches Flux  │          │ (watches plugin     │
│  resource list) │          │  CRDs via kube-rs)  │
└─────────────────┘          └─────────────────────┘
```

## Plugin YAML Schema

### Watched Resource Types

Each watched resource must specify a `type` that determines how data is fetched:

| Type | Description | Status |
|------|-------------|--------|
| `kubernetes_crd` | Watch Kubernetes CRD via kube-rs watcher | **Implemented** |
| `http_api` | Poll HTTP API endpoint | Future |
| `grpc` | Stream from gRPC service | Future |

This extensible design allows plugins to integrate with different data sources while maintaining a consistent view experience.

### Future Type Implementation Notes

When adding new watched resource types, the following changes are needed:

**1. Add enum variant** in `src/plugins/manifest.rs`:
```rust
pub enum WatchedResourceType {
    KubernetesCrd,
    HttpApi,    // New
    Grpc,       // New
}
```

**2. Add type-specific fields** to `WatchedResourceConfig`:
- `http_api`: endpoint, refresh_interval, auth, headers
- `grpc`: endpoint, service, method, tls_config

**3. Add validation** in `src/plugins/validator.rs`:
- Validate type-specific required fields
- Check endpoint URLs, intervals, etc.

**4. Implement watcher** in `src/watcher/mod.rs`:
- `http_api`: Polling loop with configurable interval, transform response to resources
- `grpc`: Streaming client, map events to WatchEvent

**5. Update template** in `src/cli/plugin.rs` to uncomment the examples

### Full Example: Argo CD Plugin

```yaml
name: argocd
version: 1.0.0
enabled: true

# FEATURE 1: Watch CRDs and create new resource views
watched_resources:
  - type: kubernetes_crd          # Required: data source type
    kind: Application
    group: argoproj.io
    version: v1alpha1
    plural: applications
    command: ":argo"              # Keybinding to access view
    display_name: "Argo Apps"     # Shown in header/footer

    # View capabilities
    supports_yaml: true           # Enable 'y' for YAML view
    supports_describe: true       # Enable 'd' for describe
    supports_logs: false          # No logs (not a workload)

    # Column definitions for list view
    columns:
      - name: NAME
        path: .metadata.name
        width: 25
      - name: NAMESPACE
        path: .metadata.namespace
        width: 15
      - name: SYNC
        path: .status.sync.status
        width: 10
        renderer: status_badge
      - name: HEALTH
        path: .status.health.status
        width: 10
        renderer: status_badge
      - name: REPO
        path: .spec.source.repoURL
        width: 30

    # Status extraction for ready/suspended indicators
    status:
      ready_path: .status.health.status
      ready_value: "Healthy"
      suspended_path: .spec.syncPolicy.automated
      suspended_when_missing: true  # suspended if field missing
      message_path: .status.conditions[0].message

  - kind: AppProject
    group: argoproj.io
    version: v1alpha1
    plural: appprojects
    command: ":argoproj"
    display_name: "Argo Projects"
    supports_yaml: true
    columns:
      - name: NAME
        path: .metadata.name
        width: 20
      - name: DESCRIPTION
        path: .spec.description
        width: 40

# FEATURE 2: Enrich existing Flux views with extra columns (existing feature)
source:
  type: kubernetes_service
  service: argocd-server
  namespace: argocd
  port: 8080
  path: /api/v1/applications
  refresh_interval: 30s

resources:
  - Kustomization
  - HelmRelease

columns:
  - name: argo-sync
    path: .status.sync.status
    width: 12
```

### Minimal Example: Crossplane Plugin

```yaml
name: crossplane
version: 1.0.0
enabled: true

watched_resources:
  - type: kubernetes_crd          # Required: only "kubernetes_crd" supported currently
    kind: Claim
    group: example.crossplane.io
    version: v1
    plural: claims
    command: ":xp"
    display_name: "XP Claims"
    supports_yaml: true
    columns:
      - name: NAME
        path: .metadata.name
        width: 30
      - name: READY
        path: .status.conditions[?(@.type=="Ready")].status
        width: 8
      - name: SYNCED
        path: .status.conditions[?(@.type=="Synced")].status
        width: 8
```

## Schema Reference

### WatchedResource

| Field | Required | Description |
|-------|----------|-------------|
| `type` | **Yes** | Resource type: `kubernetes_crd` (future: `http_api`, `grpc`) |
| `kind` | Yes* | CRD kind (e.g., "Application") - required for `kubernetes_crd` |
| `group` | Yes* | API group (e.g., "argoproj.io") - required for `kubernetes_crd` |
| `version` | Yes* | API version (e.g., "v1alpha1") - required for `kubernetes_crd` |
| `plural` | Yes* | Plural name (e.g., "applications") - required for `kubernetes_crd` |
| `command` | Yes | Keybinding (e.g., ":argo") |
| `display_name` | No | Header text (defaults to kind) |
| `supports_yaml` | No | Enable YAML view (default: true) |
| `supports_describe` | No | Enable describe (default: true) |
| `supports_logs` | No | Enable logs for pods (default: false) |
| `columns` | Yes | List view columns |
| `status` | No | Status field extraction config |

*Fields marked with * are required based on the `type` value.

### Column

| Field | Required | Description |
|-------|----------|-------------|
| `name` | Yes | Column header |
| `path` | Yes | JSONPath to extract value |
| `width` | Yes | Column width in characters |
| `renderer` | No | Renderer: text, status_badge, duration, age |

### Status

| Field | Description |
|-------|-------------|
| `ready_path` | JSONPath to ready indicator |
| `ready_value` | Value that means "ready" |
| `suspended_path` | JSONPath to suspended field |
| `suspended_when_missing` | Treat missing field as suspended |
| `message_path` | JSONPath to status message |

## Built-in Renderers

- `text` - Plain text (default)
- `status_badge` - Colored based on value (Ready=green, Degraded=yellow, etc.)
- `duration` - Human-readable duration (2h 30m)
- `age` - Time since timestamp
- `issue_badge` - Issue count with severity coloring

## Implementation

### Data Flow for Watched Resources

```
1. Plugin loaded → Register command (":argo")
2. User types ":argo" → Switch to plugin resource view
3. ResourceWatcher.watch_dynamic(group, version, kind, plural)
4. Events flow to same ResourceState as Flux resources
5. Plugin columns define how to render the list
6. 'y' key → fetch full YAML (if supports_yaml: true)
7. ESC → return to previous view
```

### Key Integration Points

**Command Registration**: Plugin commands registered alongside built-in commands
- Handled in `src/tui/commands.rs`
- Plugin keybindings must not conflict with built-ins

**Dynamic Watching**: New `watch_dynamic()` method on ResourceWatcher
- Uses `DynamicObject` + `ApiResource` (same pattern as OCIRepository multi-version)
- Events use plugin's `display_name` as resource_type

**View Rendering**: Plugin provides column config, core handles rendering
- Same `resource_list` view, different column definitions
- Status extraction uses plugin's `status` config

**YAML/Describe Support**: Controlled by flags
- `supports_yaml: true` → 'y' key fetches full object
- `supports_describe: true` → 'd' key shows formatted describe
- Objects stored in `resource_objects` HashMap same as Flux resources

## Implementation Phases

### Phase 1: Foundation (COMPLETE)
- [x] Plugin manifest schema
- [x] YAML validation
- [x] Plugin loader
- [x] CLI commands (list, validate, init, install, uninstall)
- [x] Conflict detection

### Phase 2: Data Sources (COMPLETE)
- [x] DataSource trait
- [x] HTTP connector
- [x] File connector
- [x] K8s Service connector
- [ ] K8s CRD connector (for data enrichment) - deferred to Phase 4
- [x] Data cache with TTL

### Phase 3: Watched Resources (COMPLETE)
- [x] `watched_resources` schema in manifest
- [x] `WatchedResourceType` enum for extensible resource types
- [x] `watch_dynamic()` method on ResourceWatcher
- [x] Command registration for plugin views (dynamic commands in help menu)
- [x] Plugin resource list view rendering with custom columns
- [x] Status extraction from plugin config
- [x] YAML view support flag
- [x] Describe view support flag
- [x] Plugin column rendering with custom renderers

### Phase 4: Column Enrichment (IN PROGRESS)
- [x] JSONPath extraction (`column_extraction.rs`)
- [x] Built-in renderers (text, status_badge, duration, age, boolean, issue_badge, percentage_bar)
- [x] Theme integration (colors from theme)
- [ ] Render plugin columns in Flux resource views (column enrichment for existing views)
- [ ] K8s CRD connector for data enrichment

### Phase 5: Polish
- [x] Error handling (graceful degradation, logging)
- [x] Example plugins (Argo CD)
- [ ] Additional example plugins (Crossplane, Kyverno, cert-manager)
- [ ] Documentation site updates
- [ ] Performance testing

## Example Plugins to Ship

1. **argocd.yaml** - Watch Applications and AppProjects
2. **crossplane.yaml** - Watch Claims and Compositions
3. **kyverno.yaml** - Watch PolicyReports
4. **cert-manager.yaml** - Watch Certificates and Issuers

## Design Decisions

### Why watched_resources vs extending source types?

**Decision**: Separate `watched_resources` section for CRD watching

**Rationale**:
- `source` is for data enrichment (fetch JSON, add columns to existing views)
- `watched_resources` is for new views (watch CRDs, render full TUI)
- Clear separation of concerns
- A plugin can do both (enrich Flux views AND add Argo view)

### Why require explicit column definitions?

**Decision**: Plugin must define columns, no auto-discovery

**Rationale**:
- Different CRDs have different important fields
- Auto-discovery would show too much noise
- Plugin author knows what's important
- Consistent with k9s custom resource views

### Why optional YAML/describe support?

**Decision**: Flags control view capabilities

**Rationale**:
- Some resources benefit from YAML view (Applications)
- Some don't (simple status CRDs)
- Keeps UI clean when features aren't useful
- Default to true for YAML, true for describe

## Success Criteria

### Functional
- [x] `:argo` command shows Argo Applications
- [x] YAML view works for plugin resources
- [ ] Plugin columns appear in Flux resource list (enrichment - Phase 4)
- [x] Multiple plugins can coexist
- [x] Namespace filtering works

### Quality
- [x] No performance regression
- [x] Plugin errors don't crash app
- [x] Clear error messages for invalid YAML

## Next Steps

1. Complete column enrichment for existing Flux views (Phase 4)
2. Add K8s CRD connector for data enrichment
3. Create additional example plugins (Crossplane, Kyverno, cert-manager)
4. Update documentation site with plugin guide
