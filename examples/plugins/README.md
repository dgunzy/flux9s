# Example Plugins

This directory contains example plugin configurations for flux9s.

## Available Plugins

- **argocd.yaml** - Watch Argo CD Applications and AppProjects

## Configuration

### Kubernetes Service DNS Suffix

When using `kubernetes_service` data sources in plugins, flux9s needs to know the DNS suffix
used by your Kubernetes cluster to resolve service names.

**Default:** `.svc.cluster.local` (works for most standard Kubernetes clusters)

**Override:** Add to your flux9s config file (`~/.config/flux9s/config.yaml`):

```yaml
plugin:
  kubernetesDnsSuffix: ".svc.cluster.local"
```

Some Kubernetes distributions or custom clusters may use different DNS suffixes:
- `.svc.cluster.local.cluster.local` (some OpenShift setups)
- `.svc` (minimal setups)
- Custom DNS suffixes configured in your cluster

To find your cluster's DNS suffix, check:
```bash
# Check CoreDNS config
kubectl get configmap coredns -n kube-system -o yaml

# Or test DNS resolution
kubectl run -it --rm debug --image=busybox --restart=Never -- nslookup kubernetes.default.svc.cluster.local
```

## Installation

### Using the CLI (Recommended)

1. **Validate the plugin:**
   ```bash
   flux9s plugin validate examples/plugins/argocd.yaml
   ```

2. **Install the plugin:**
   ```bash
   flux9s plugin install examples/plugins/argocd.yaml
   ```

3. **List installed plugins:**
   ```bash
   flux9s plugin list
   ```

4. **Start flux9s** - plugins are automatically loaded:
   ```bash
   flux9s
   ```

### Manual Installation

Plugins are loaded from: `~/.config/flux9s/plugins/` (Linux/macOS) or `%APPDATA%\flux9s\plugins\` (Windows)

You can override the plugin directory location using the `FLUX9S_PLUGINS_DIR` environment variable:

```bash
export FLUX9S_PLUGINS_DIR=/custom/path/to/plugins
flux9s
```

1. Create the plugins directory if it doesn't exist:
   ```bash
   mkdir -p ~/.config/flux9s/plugins
   ```

2. Copy the plugin file:
   ```bash
   cp examples/plugins/argocd.yaml ~/.config/flux9s/plugins/
   ```

3. Start flux9s - the plugin will be automatically loaded

## Usage

After installing the Argo CD plugin:

- Type `:argo` to view Argo CD Applications
- Type `:argoproj` to view Argo CD AppProjects
- Use standard navigation keys (j/k, Enter, y, etc.)

## Plugin Commands

- `flux9s plugin list` - List all installed plugins
- `flux9s plugin validate <path>` - Validate a plugin file
- `flux9s plugin init <name>` - Create a new plugin template
- `flux9s plugin install <path>` - Install a plugin
- `flux9s plugin uninstall <name>` - Uninstall a plugin
