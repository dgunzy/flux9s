---
title: "Configuration"
linkTitle: "Configuration"
weight: 4
description: "Configure flux9s to suit your needs"
toc: true
type: docs
---

## Configuration File Location

flux9s stores its configuration in a YAML file. The location depends on your operating system:

| OS          | Location                       |
| ----------- | ------------------------------ |
| **Linux**   | `~/.config/flux9s/config.yaml` |
| **macOS**   | `~/.config/flux9s/config.yaml` |
| **Windows** | `%APPDATA%\flux9s\config.yaml` |

## Configuration Options

### Read-Only Mode

By default, flux9s launches in readonly mode to prevent accidental changes. You can change this:

**Via command line:**

```bash
flux9s config set readOnly false
```

**During a session:**
Use the `:readonly` command to toggle readonly mode.

{{% alert title="Safety First" color="info" %}}
Readonly mode is enabled by default to prevent accidental modifications to your Flux resources. Only disable it if you need to perform operations.
{{% /alert %}}

### UI Configuration

#### Skin Configuration

flux9s supports custom skins to personalize the interface. Skins follow a similar format to K9s but may require adjustments.

**Set a skin for readonly mode:**

```bash
flux9s config set ui.skinReadOnly rose-pine
```

**Import and set a skin:**

```bash
flux9s config skins set navy.yaml
```

Skins must be placed in your system's `flux9s/skins` directory:

- **Linux/macOS**: `~/.config/flux9s/skins/`
- **Windows**: `%APPDATA%\flux9s\skins\`

{{% alert title="Skin Compatibility" color="warning" %}}
Not all K9s skins are compatible with flux9s. flux9s skins follow a similar format but may require adjustments to work properly.
{{% /alert %}}

## Command Reference

{{< blocks/section color="white" >}}
{{% blocks/feature icon="fa-question-circle" title="Show Help" %}}

```bash
flux9s config --help
```

Show all available configuration options.
{{% /blocks/feature %}}

{{% blocks/feature icon="fa-edit" title="Set Value" %}}

```bash
flux9s config set {KEY} {VALUE}
```

Set a configuration value.
{{% /blocks/feature %}}

{{% blocks/feature icon="fa-eye" title="Get Value" %}}

```bash
flux9s config get {KEY}
```

Get a configuration value.
{{% /blocks/feature %}}

{{% blocks/feature icon="fa-palette" title="Set Skin" %}}

```bash
flux9s config skins set {skin-file}
```

Import and set a skin from a YAML file.
{{% /blocks/feature %}}
{{< /blocks/section >}}
