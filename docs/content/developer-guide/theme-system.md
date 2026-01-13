# Theme/Skin System Documentation

## Overview

The flux9s theme system allows you to customize the appearance of the TUI by loading color schemes from YAML files. Themes follow the k9s skin format for familiarity and compatibility.

## How It Works

### Theme Resolution Order

When loading a theme, flux9s checks in this order:

1. **User Skins Directory**: `$XDG_CONFIG_HOME/flux9s/skins/{name}.yaml` (default: `~/.config/flux9s/skins/`)
2. **Legacy Data Directory**: `$XDG_DATA_HOME/flux9s/skins/{name}.yaml` (default: `~/.local/share/flux9s/skins/`)
3. **Embedded Themes**: Built-in themes embedded in the binary (17 popular themes)
4. **Built-in Default**: Falls back to hardcoded default theme

### Setting a Theme

#### Via Configuration File

Edit `~/.config/flux9s/config.yaml`:

```yaml
ui:
  skin: dracula
```

Or use the CLI:

```bash
flux9s config set ui.skin dracula
```

#### Via Environment Variable

```bash
export FLUX9S_SKIN=dracula
flux9s
```

#### Via TUI Command

While in the TUI, you can change themes in two ways:

**Direct command:**
```
:skin dracula
```
This changes the theme immediately (temporary, doesn't persist to config file).

**Interactive submenu:**
```
:skin
```
This opens an interactive theme selection menu with live preview. Navigate with `j`/`k`, press `Enter` to apply, `s` to save to config, or `Esc` to cancel.

![Theme Submenu](/images/skin-submenu.png)

The submenu shows:
- All available themes (embedded + user-installed)
- Current theme marked with "(current)"
- Built-in themes marked with "[built-in]"
- Live preview as you navigate (theme changes immediately)

## Theme File Format

Themes use k9s-style YAML format:

```yaml
k9s:
  body:
    fg: "#ffffff" # Foreground color
    bg: "#000000" # Background color
    logo: "#00ff00" # Logo/ASCII art color

  frame:
    border:
      fg: "#00ffff" # Border color
    menu:
      fg: "#ffffff"
      bg: "#000000"
      key: "#00ff00" # Keybinding color
    crumbs:
      fg: "#ffff00"
      active: "#ffffff"
    status:
      new: "#00ff00" # Success/ready color
      error: "#ff0000" # Error color
    title:
      fg: "#ffffff"
      bg: "#000000"

  views:
    table:
      fg: "#ffffff"
      bg: "#000000"
      cursor:
        fg: "#000000"
        bg: "#ffffff"
      header:
        fg: "#ffffff"
        bg: "#000000"
    yaml:
      key: "#00ffff"
      value: "#ffffff"
    logs:
      fg: "#ffffff"
      bg: "#000000"
```

### Color Format Support

- **Hex colors**: `#ffffff`, `#fff` (short form)
- **Named colors**: `white`, `black`, `red`, `green`, `blue`, `yellow`, `cyan`, `magenta`, `gray`
- **Special**: `default` (transparent/inherit)

### Color Mapping

The k9s skin format is mapped to flux9s Theme fields:

- `body.fg` → `text_primary`
- `body.logo` → `header_ascii`
- `frame.border.fg` → `header_resources`
- `frame.menu.key` → `footer_key`
- `frame.crumbs.active` → `table_selected`
- `frame.status.new` → `status_ready`
- `frame.status.error` → `status_error`
- `frame.title.fg` → `header_context`
- `views.table.fg` → `table_normal`
- `views.table.cursor.fg` → `table_selected`
- `views.table.header.fg` → `table_header`
- `views.yaml.key` → `text_label`
- `views.yaml.value` → `text_value`

## Built-in Themes

flux9s includes 17 popular themes embedded in the binary, so they're available immediately without installation:

**Dark Themes:**
- `dracula` - Dark theme with vibrant colors
- `nord` - Arctic-inspired cool colors
- `solarized-dark` - Carefully designed dark theme
- `monokai` - Classic dark theme with bright colors
- `gruvbox-dark` - Retro groove color scheme
- `catppuccin-mocha` - Warm dark theme with pastel colors
- `rose-pine-moon` - Soft dark theme with muted colors
- `inthenavy` - Navy blue theme
- `one-dark` - Popular Atom/VS Code theme
- `tokyo-night` - VS Code theme with purple/blue tones
- `ayu-dark` - Dark theme with warm accents
- `snazzy` - Vibrant terminal theme
- `tomorrow-night` - Classic dark theme
- `papercolor-dark` - Minimal dark theme
- `base16-dark` - Base16 dark variant

**Light Themes:**
- `default-light` - Light theme for bright terminals
- `kiss` - Minimalistic theme

### Default Theme

The default theme is built-in and uses:

- Yellow for headers and context
- Cyan for resources and labels
- Green for ready status
- Red for errors
- White for normal text

### Creating Custom Themes

1. Create a YAML file in `~/.config/flux9s/skins/` (or `~/.local/share/flux9s/skins/` for legacy)
2. Name it `{theme-name}.yaml`
3. Use the k9s format shown above
4. Set it with `flux9s config set ui.skin {theme-name}` or use the `:skin` submenu in TUI

## Implementation Details

### Code Structure

- **Theme Definition**: `src/tui/theme.rs` - Defines the `Theme` struct with all color fields
- **Theme Loader**: `src/config/theme_loader.rs` - Handles loading themes from YAML files
- **Theme Resolution**: Checks user dir → system dir → built-in default
- **Theme Application**: Theme is loaded at startup and can be changed via `:skin` command

### Key Functions

- `ThemeLoader::load_theme(name)` - Load a theme by name
- `ThemeLoader::list_themes()` - List all available themes (includes embedded + user-installed)
- `App::set_theme(name)` - Change theme in running TUI
- `App::preview_theme(name)` - Preview theme temporarily (for submenu)
- `App::persist_theme(name)` - Save theme to config file
- `parse_color(color_str)` - Parse color string to ratatui Color

## Theme Submenu Features

The `:skin` command without arguments opens an interactive submenu with:

- **Live Preview**: Theme changes immediately as you navigate with `j`/`k`
- **Current Theme Indicator**: Shows which theme is currently active
- **Built-in Theme Markers**: Embedded themes are marked with `[built-in]`
- **Quick Apply**: Press `Enter` to apply theme temporarily (session only)
- **Persist to Config**: Press `s` to save theme to config file (persists across sessions)
- **Cancel & Restore**: Press `Esc` to cancel and restore original theme

The submenu saves themes to `ui.skin` in normal mode, or `ui.skinReadOnly` when readonly mode is enabled.

## Current Status

✅ **Implemented:**

- Theme loading from YAML files
- k9s-style skin format support
- Theme resolution (user → legacy → embedded → default)
- 17 embedded themes built into binary
- `:skin` command to change themes
- Interactive theme submenu with live preview
- Theme persistence to config file
- Config file support (`ui.skin`, `ui.skinReadOnly`)
- Environment variable override (`FLUX9S_SKIN`)
- Context-specific skins

🚧 **Future Enhancements:**

- Hot-reload on theme file changes
- Better color mapping coverage
- Theme validation and error reporting
- More embedded themes

## Troubleshooting

**Theme not loading?**

- Check file exists in correct directory
- Verify YAML syntax is valid
- Check theme name matches filename (without .yaml)
- Use `--debug` flag to see error messages

**Colors not applying correctly?**

- Some k9s color fields may not map to flux9s Theme fields yet
- Check which Theme fields are actually used in the UI
- Fall back to default theme if needed



