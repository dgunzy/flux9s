# Plugin System Implementation Plan

## Overview

Add a YAML-based plugin system to flux9s that enables external data sources to enhance resource views with additional columns and custom detail views.

**Status**: Planning
**Target Version**: v0.7.0
**Maintainer**: @dgunzy

## Philosophy

flux9s is a Flux monitoring tool. Plugins extend it but don't change its core identity:
- Plugins are **optional** - flux9s works perfectly without them
- Plugins are **read-only** - no cluster modifications
- Plugins are **declarative** - YAML configuration, no templates or code
- Plugins are **flexible** - support on-cluster and off-cluster data sources

## Goals

1. **Kubernetes-Native**: Support CRDs and Services as data sources (most common use case)
2. **Resource Agnostic**: Enhance ANY Kubernetes resource, not just Flux
3. **Zero Bundled Plugins**: Ships with examples only, nothing enabled
4. **Backwards Compatible**: Existing flux9s behavior unchanged
5. **Strong Validation**: CLI catches configuration errors early
6. **Theme Integration**: Use existing theme system for colors

## Non-Goals

- Plugin sandboxing (future)
- Write operations (future)
- Plugin marketplace (future)
- Binary/WASM plugins (YAML only)

## Architecture

```
flux9s Core                         Plugin System
────────────                        ─────────────

┌─────────────┐                    ┌──────────────────┐
│ TUI Views   │◄───────────────────│ Plugin Registry  │
│ (list,      │  Add columns       │                  │
│  detail)    │  Add views         │  - Loader        │
└─────────────┘                    │  - Validator     │
      ▲                            │  - Data Cache    │
      │                            └──────────────────┘
      │                                     ▲
┌─────────────┐                            │
│ App State   │                            │
│ (resources) │                    ┌───────┴──────────┐
└─────────────┘                    │  Data Sources    │
      ▲                            │                  │
      │                            │  - K8s CRD       │
┌─────────────┐                    │  - K8s Service   │
│ Watcher     │                    │  - HTTP API      │
│ (kube-rs)   │                    │  - File          │
└─────────────┘                    └──────────────────┘
      ▲                                     ▲
      │                                     │
      └─────────────┬───────────────────────┘
                    │
            ┌───────▼────────┐
            │  Kubernetes    │
            │   API Server   │
            └────────────────┘
```

### Data Flow

1. Load plugins from `~/.config/flux9s/plugins/*.yaml`
2. Validate schemas and initialize data source connectors
3. Start background task to fetch plugin data (respects refresh_interval)
4. When rendering views, extract values using JSONPath and apply renderers
5. Plugin views accessible via keybindings (e.g., `:agent`)

## Plugin YAML Schema

### Example: ConfigHub Agent Plugin

```yaml
name: confighub-agent
version: 1.0.0
enabled: true

# Data Source (choose one type)
source:
  # Option 1: Kubernetes Service (MOST COMMON)
  type: kubernetes_service
  service: confighub-agent
  namespace: confighub-system
  port: 8080
  path: /api/map
  refresh_interval: 30s

  # Option 2: Kubernetes CRD
  # type: kubernetes_crd
  # kind: ConfigHubData
  # group: confighub.com
  # version: v1
  # namespace: confighub-system  # optional, default all
  # name: cluster-data           # optional, default all
  # data_path: .status.data      # JSONPath to extract data
  # refresh_interval: 30s

  # Option 3: External HTTP API
  # type: http
  # endpoint: https://api.example.com/data
  # auth:
  #   type: bearer
  #   token_env: API_TOKEN
  # refresh_interval: 30s

# Resources this plugin enhances (ANY K8s resource, not just Flux)
resources:
  - Kustomization          # Flux resources
  - HelmRelease
  - Deployment             # Core K8s resources
  - Service
  - MyCustomResource       # Your CRDs

# Add columns to resource list view
columns:
  - name: owner
    path: .ownership.owner
    width: 12
    renderer: text

  - name: issues
    path: .ccve.count
    width: 8
    renderer: issue_badge  # Built-in: text, issue_badge, percentage_bar, duration

# Custom detail views
views:
  - name: agent_detail
    keybinding: ":agent"

  - name: relationships
    keybinding: ":graph"
```

### Data Source Types

**kubernetes_service**: Query K8s service endpoint (in-cluster or via port-forward)
- Most common use case for on-cluster APIs
- Uses current kubeconfig context

**kubernetes_crd**: Watch a CRD that contains plugin data
- Data stored in cluster as CRD
- Efficient for frequently updated data

**http**: External HTTP API
- For off-cluster data sources
- Supports authentication

**file**: Local JSON file
- For testing or static data

### Built-in Renderers

Renderers apply theme-appropriate styling automatically:

- `text` - Plain text (default, uses theme's text color)
- `issue_badge` - "-", "⚠ 1", "🔴 2" (themed warn/error colors)
- `percentage_bar` - `[████████░░] 80%` (themed based on percentage)
- `duration` - "2h 30m" (themed text color)

All styling respects user's active theme (rose-pine, dracula, etc.)

## File Structure

```
src/
├── plugins/                  # NEW
│   ├── manifest.rs           # Plugin YAML schema
│   ├── loader.rs             # Load and validate plugins
│   ├── registry.rs           # Plugin registry
│   ├── datasource/           # Data source connectors
│   │   ├── k8s_service.rs    # Kubernetes Service
│   │   ├── k8s_crd.rs        # Kubernetes CRD
│   │   ├── http.rs           # HTTP API
│   │   └── file.rs           # File
│   ├── renderer.rs           # Built-in renderers
│   └── views/                # Plugin view implementations
│
├── tui/
│   ├── app/state.rs          # UPDATED: Add plugin state
│   └── views/
│       └── resource_list.rs  # UPDATED: Render plugin columns
│
└── cli/commands.rs           # UPDATED: Add plugin CLI

examples/plugins/             # NEW: Example plugins (not enabled)
├── confighub-agent.yaml
├── argocd.yaml
└── minimal.yaml

tests/plugin_tests.rs         # NEW: Plugin tests
```

## CLI Commands

```bash
flux9s plugin list                         # List loaded plugins
flux9s plugin validate <file>              # Validate plugin YAML
flux9s plugin init <name>                  # Generate template
flux9s plugin install <file>               # Install plugin
flux9s plugin uninstall <name>             # Remove plugin
```

## Plugin Discovery View

**`:plugins`** - Interactive view listing all loaded plugins and their available views

Shows:
- Plugin name and version
- Data source type and status
- Columns added
- Views available with keybindings
- Last refresh time
- Any errors

Navigate to plugin views directly from this menu.

## Implementation Phases

### Phase 1: Foundation
- Plugin manifest schema (Serde structs)
- YAML validation with helpful error messages
- Plugin loader (scan `~/.config/flux9s/plugins/*.yaml`)
- Column conflict detection (error if multiple plugins define same column name)
- CLI: `plugin list`, `plugin validate`, `plugin init`
- Tests for schema validation

### Phase 2: Data Sources
- DataSource trait (`fetch()`, `health_check()`)
- Kubernetes Service connector (uses kube-rs client)
- Kubernetes CRD connector (watch CRD, extract data path)
- HTTP connector (with auth support)
- File connector (for testing)
- Data cache with TTL
- Async refresh in background

### Phase 3: Column Rendering
- JSONPath extraction from plugin data
- Built-in renderers (text, issue_badge, percentage_bar, duration)
- Theme style integration
- Update resource_list view to render plugin columns
- Handle missing data gracefully (show "-")

### Phase 4: Custom Views
- Plugin view registry
- `:plugins` discovery view (list all plugins and their views)
- View implementations (agent_detail, relationships, fleet_summary)
- Keybinding handling
- Footer shows plugin view keybindings
- Navigation (ESC returns to main view)

### Phase 5: Polish
- Error handling review
- Performance optimization
- Documentation (PLUGIN_SYSTEM.md, examples/plugins/README.md)
- Example plugins tested against real data sources
- Release notes

## Example Plugins

### Minimal Template
```yaml
name: my-plugin
version: 1.0.0
enabled: true

source:
  type: kubernetes_service
  service: my-service
  namespace: default
  port: 8080
  path: /api/data

resources:
  - Deployment

columns:
  - name: status
    path: .status
    width: 10
```

### ConfigHub Agent (Full-Featured)
```yaml
name: confighub-agent
version: 1.0.0
enabled: true

source:
  type: kubernetes_service
  service: confighub-agent
  namespace: confighub-system
  port: 8080
  path: /api/map
  refresh_interval: 30s

resources:
  - Kustomization
  - HelmRelease
  - Deployment
  - Service

columns:
  - name: owner
    path: .ownership.owner
    width: 12

  - name: unit
    path: .ownership.unit
    width: 15

  - name: issues
    path: .ccve.count
    width: 8
    renderer: issue_badge

views:
  - name: agent_detail
    keybinding: ":agent"
  - name: relationships
    keybinding: ":graph"
```

### Argo CD
```yaml
name: argocd
version: 1.0.0
enabled: true

source:
  type: kubernetes_service
  service: argocd-server
  namespace: argocd
  port: 8080
  path: /api/v1/applications

resources:
  - Kustomization
  - HelmRelease

columns:
  - name: sync
    path: .status.sync.status
    width: 12

  - name: health
    path: .status.health.status
    width: 10
```

## Design Decisions

### Kubernetes-First Architecture
**Decision**: Default to Kubernetes Service and CRD data sources

**Rationale**:
- flux9s already uses kubeconfig for authentication
- Users expect on-cluster integrations
- Consistent with flux9s philosophy
- No additional credentials needed

### Resource Agnostic
**Decision**: Plugins can enhance ANY K8s resource, not just Flux

**Rationale**:
- Users may want to enhance Deployments, Services, custom CRDs
- Plugin system should be flexible
- Still primarily a Flux tool, but extensible

### Theme Integration
**Decision**: Renderers manage all styling, no color configuration in YAML

**Rationale**:
- Respects user's chosen theme (rose-pine, dracula, etc.)
- Consistent with existing flux9s design
- Simpler - no color mapping needed
- Renderers apply semantic colors (warn/error/success) automatically

### No Templating
**Decision**: Use JSONPath for data extraction, renderers for formatting

**Rationale**:
- Simpler to understand and debug
- Type-safe rendering in Rust code
- Avoids security risks of template injection
- Clear separation: YAML = config, Rust = logic

### Read-Only
**Decision**: v0.7.0 plugins cannot modify cluster state

**Rationale**:
- Safer for initial release
- Focus on observation/enrichment use case
- Write operations can be added later with proper safeguards

## Backwards Compatibility

### Guarantees
1. flux9s works without any plugins loaded
2. Plugin loading failures are logged, not fatal
3. No config file changes required
4. Existing CLI commands unchanged
5. Performance impact only when plugins enabled

### Implementation
```
AppState {
    plugins: Option<PluginRegistry>  // None if no plugins or load failed
}

// In views
if let Some(plugins) = &app.state.plugins {
    render_plugin_columns(plugins, ...);
}
```

### Testing
- Test suite includes "no plugins" scenario
- Plugin errors logged with `tracing::warn`
- Graceful degradation on data fetch failures

## ConfigHub Agent Integration

The plugin system is designed to support the ConfigHub Agent use case:

### What ConfigHub Agent Provides
- **Ownership detection**: Who owns each resource (ConfigHub, Flux, Argo, Helm, Native)
- **CCVE scanning**: Configuration vulnerabilities (637 patterns)
- **Drift detection**: Live state vs desired state
- **Relationship graphs**: Resource ownership chains (GitRepo → Kustomization → Deployment)
- **Fleet queries**: Query across all resources

### How Plugins Enable This

**Data Source**: Kubernetes Service connector
```yaml
source:
  type: kubernetes_service
  service: confighub-agent
  namespace: confighub-system
  port: 8080
  path: /api/map
```

**Ownership Columns**: Show owner for all resources
```yaml
columns:
  - name: owner
    path: .ownership.owner  # ConfigHub, Flux, Argo, Helm, Native
    width: 12

  - name: unit
    path: .ownership.unit   # ConfigHub unit
    width: 15
```

**Issue Detection**: Show CCVEs
```yaml
columns:
  - name: issues
    path: .ccve.count
    width: 8
    renderer: issue_badge  # ⚠ 1, 🔴 2
```

**Custom Views**: Detailed ownership and relationships
```yaml
views:
  - name: agent_detail      # Show ownership, drift, CCVEs for selected resource
    keybinding: ":agent"

  - name: relationships     # Show resource ownership graph
    keybinding: ":graph"
```

### Expected Data Format

ConfigHub Agent's `/api/map` endpoint returns:
```json
{
  "resources": [
    {
      "kind": "Kustomization",
      "namespace": "flux-system",
      "name": "apps",
      "ownership": {
        "owner": "ConfigHub",
        "unit": "apps",
        "space": "prod"
      },
      "ccve": {
        "count": 1,
        "findings": [...]
      },
      "drift": {
        "status": "detected"
      }
    }
  ]
}
```

Plugins use JSONPath to extract values from this structure.

## Design Decisions (Resolved)

1. **Plugin directory**: `~/.config/flux9s/plugins/`
2. **Default refresh interval**: 30s (reasonable for most use cases)
3. **Column name conflicts**: Error out with verbose message listing conflicting plugins
4. **View discovery**: `:plugins` view lists all plugins and their views
5. **Data format**: No enforcement, JSONPath extracts whatever exists
6. **TLS/Service Mesh**: Not in v0.7.0, architecture supports future extension
7. **Resource validation**: Accept any string (maximum flexibility for custom CRDs)

## Success Criteria

### Functional
- [ ] Load plugins from config directory
- [ ] Kubernetes Service connector works
- [ ] Plugin columns render correctly
- [ ] Theme styles apply
- [ ] JSONPath extraction works
- [ ] Plugin views accessible via keybindings
- [ ] CLI commands functional

### Quality
- [ ] All tests pass
- [ ] No clippy warnings
- [ ] Documentation complete
- [ ] Example plugins tested
- [ ] Performance < 100ms overhead

### Compatibility
- [ ] flux9s works without plugins
- [ ] No breaking changes
- [ ] Plugin errors don't crash app

## Timeline

- **Phase 1**: Foundation (1 week)
- **Phase 2**: Data Sources (1 week)
- **Phase 3**: Column Rendering (1 week)
- **Phase 4**: Custom Views (1 week)
- **Phase 5**: Polish (1 week)

**Total**: 5 weeks to v0.7.0

## Next Steps

1. Review and approve this plan
2. Start Phase 1 implementation
3. Create minimal working example
4. Iterate based on feedback
5. Test with real ConfigHub Agent integration
