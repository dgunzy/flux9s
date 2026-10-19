---
title: "Command Line"
linkTitle: "Command Line"
weight: 5
description: "flux9s flags, config subcommands, and shell completions"
type: docs
---

```bash
# Start the UI (current kubeconfig context)
flux9s

# Use a specific kubeconfig file
flux9s --kubeconfig /path/to/kubeconfig

# Check the connection and exit (0 = healthy) without starting the UI
flux9s --check

# Debug logging (the log file path is printed at startup)
flux9s --debug

# Show the version
flux9s --version
```

## Start on a view: `-c` {#start-command}

`-c` (`--command`) runs a `:` command once flux9s has connected, the way
`k9s -c` does — anything you'd type after `:`:

```bash
flux9s -c ks,hr          # Kustomizations and HelmReleases
flux9s -c apps           # a view from your config
flux9s -c pulse          # the health dashboard
flux9s -c "ns apps"      # a namespace
flux9s -c deploy         # a Kubernetes kind (with native resources on)
```

To start the same way every time, set
[`defaultResourceFilter`](/configuration/#default-resource-filter).

## Configuration

```bash
# Every option, its value, and whether it's the default
flux9s config list

# Read or set a value
flux9s config get readOnly
flux9s config set readOnly false
flux9s config set nativeResources true
flux9s config set ui.skinReadOnly rose-pine

# Context- or cluster-specific values
flux9s config set ui.skin dracula --cluster my-cluster
flux9s config set ui.skin dracula --cluster my-cluster --context prod

# Import a skin file and use it
flux9s config skins set navy.yaml
```

See [Configuration](/configuration/) for every option.

{{% alert title="Skin compatibility" color="warning" %}}
flux9s skins follow the K9s format, but not every K9s skin works unchanged.
{{% /alert %}}

## Shell completions

```bash
flux9s completions zsh > "${fpath[1]}/_flux9s"          # zsh
flux9s completions bash > /etc/bash_completion.d/flux9s  # bash
flux9s completions fish > ~/.config/fish/completions/flux9s.fish  # fish
```

`elvish` and `powershell` are supported too.
