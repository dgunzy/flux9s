# Theme/Skin System Documentation

## Overview

The flux9s theme system allows you to customize the appearance of the TUI by loading color schemes from YAML files. Themes follow the k9s skin format for familiarity and compatibility.

## How It Works

### Theme Resolution Order

When loading a theme, flux9s checks in this order:

1. **User Skins Directory**: `$XDG_DATA_HOME/flux9s/skins/{name}.yaml` (default: `~/.local/share/flux9s/skins/`)
2. **System Skins Directory**: `$XDG_CONFIG_HOME/flux9s/skins/{name}.yaml` (default: `~/.config/flux9s/skins/`)
3. **Built-in Default**: Falls back to hardcoded default theme

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

While in the TUI, type:

```
:skin dracula
```

This changes the theme immediately (temporary, doesn't persist to config file).

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

## Example Themes

### Default Theme

The default theme is built-in and uses:

- Yellow for headers and context
- Cyan for resources and labels
- Green for ready status
- Red for errors
- White for normal text

### Creating Custom Themes

1. Create a YAML file in `~/.local/share/flux9s/skins/` or `~/.config/flux9s/skins/`
2. Name it `{theme-name}.yaml`
3. Use the k9s format shown above
4. Set it with `flux9s config set ui.skin {theme-name}` or `:skin {theme-name}` in TUI

## Implementation Details

### Code Structure

- **Theme Definition**: `src/tui/theme.rs` - Defines the `Theme` struct with all color fields
- **Theme Loader**: `src/config/theme_loader.rs` - Handles loading themes from YAML files
- **Theme Resolution**: Checks user dir → system dir → built-in default
- **Theme Application**: Theme is loaded at startup and can be changed via `:skin` command

### Key Functions

- `ThemeLoader::load_theme(name)` - Load a theme by name
- `ThemeLoader::list_themes()` - List all available themes
- `App::set_theme(name)` - Change theme in running TUI
- `parse_color(color_str)` - Parse color string to ratatui Color

## Current Status

✅ **Implemented:**

- Theme loading from YAML files
- k9s-style skin format support
- Theme resolution (user → system → default)
- `:skin` command to change themes
- Config file support (`ui.skin`)
- Environment variable override (`FLUX9S_SKIN`)

🚧 **Future Enhancements:**

- More built-in themes (dracula, solarized-dark)
- Theme preview/switcher UI
- Hot-reload on theme file changes
- Better color mapping coverage
- Theme validation and error reporting

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



