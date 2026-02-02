//! Help view rendering

use crate::tui::keybindings::get_resource_help_commands;
use crate::tui::theme::Theme;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

/// Render the help view with columns (K9s-style)
pub fn render_help(
    f: &mut Frame,
    area: Rect,
    theme: &Theme,
    namespace_hotkeys: &[String],
    plugin_registry: Option<&crate::plugins::PluginRegistry>,
) {
    // Create inner area with padding for the border
    let inner_area = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };

    // Split into 4 columns
    let column_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25), // RESOURCE
            Constraint::Percentage(25), // GENERAL
            Constraint::Percentage(25), // NAVIGATION
            Constraint::Percentage(25), // Namespace Hotkeys
        ])
        .split(inner_area);

    // RESOURCE column - use centralized keybindings
    let resource_items = get_resource_help_commands();
    render_help_column(f, column_chunks[0], "RESOURCE", &resource_items, theme);

    // GENERAL column - build dynamically to include plugin commands
    let mut general_items: Vec<(String, String)> = vec![
        ("<q>".to_string(), "Quit".to_string()),
        ("<Esc>".to_string(), "Back/Quit".to_string()),
        ("<?>".to_string(), "Show/hide help".to_string()),
        ("<:>".to_string(), "Command mode".to_string()),
        ("</>".to_string(), "Filter resources".to_string()),
        ("<Tab>".to_string(), "Autocomplete command".to_string()),
        (":help".to_string(), "Show/hide help".to_string()),
        (":readonly".to_string(), "Toggle readonly mode".to_string()),
        (":skin <n>".to_string(), "Change theme/skin".to_string()),
        (":ctx <n>".to_string(), "Switch context".to_string()),
        (":ctx".to_string(), "Open context submenu".to_string()),
        (":ns <n>".to_string(), "Switch namespace".to_string()),
        (":ns all".to_string(), "Show all namespaces".to_string()),
        (":all".to_string(), "Show all resources".to_string()),
        (":healthy".to_string(), "Show healthy resources".to_string()),
        (
            ":unhealthy".to_string(),
            "Show unhealthy resources".to_string(),
        ),
        (":favorites".to_string(), "View favorites".to_string()),
        (":fav".to_string(), "View favorites".to_string()),
        (":q".to_string(), "Quit application".to_string()),
    ];

    // Add plugin watched resource commands dynamically
    if let Some(registry) = plugin_registry {
        for (cmd, display_name) in registry.get_watched_resource_commands() {
            let cmd_with_colon = if cmd.starts_with(':') {
                cmd.clone()
            } else {
                format!(":{}", cmd)
            };
            general_items.push((cmd_with_colon, format!("View {}", display_name)));
        }
    }

    // Convert to &str slices for rendering
    let general_items_ref: Vec<(&str, &str)> = general_items
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    render_help_column(f, column_chunks[1], "GENERAL", &general_items_ref, theme);

    // NAVIGATION column
    let nav_items = vec![
        ("<j>", "Navigate down"),
        ("<k>", "Navigate up"),
        ("<Enter>", "View details"),
        ("<Esc>", "Back/Quit"),
    ];
    render_help_column(f, column_chunks[2], "NAVIGATION", &nav_items, theme);

    // HOTKEYS column - namespace hotkeys (0-9)
    let mut hotkey_items: Vec<(String, String)> = Vec::new();

    for (idx, ns) in namespace_hotkeys.iter().enumerate() {
        if idx > 9 {
            break; // Only 0-9 supported
        }
        let display_ns = if ns == "all" {
            "all"
        } else if ns.len() > 25 {
            &ns[..25] // Truncate very long names (doubled from 12)
        } else {
            ns
        };
        hotkey_items.push((format!("<{}>", idx), display_ns.to_string()));
    }

    // If no hotkeys configured, show defaults
    if hotkey_items.is_empty() {
        hotkey_items.push(("<0>".to_string(), "all".to_string()));
        hotkey_items.push(("<1>".to_string(), "flux-system".to_string()));
    }

    // Convert to &str slices for rendering
    let hotkey_items_ref: Vec<(&str, &str)> = hotkey_items
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    render_help_column(
        f,
        column_chunks[3],
        "Namespace Hotkeys",
        &hotkey_items_ref,
        theme,
    );

    // Render single border around all columns
    let block = Block::default().title("Help").borders(Borders::ALL);
    f.render_widget(block, area);
}

/// Render a single help column (no borders, just content)
fn render_help_column(
    f: &mut Frame,
    area: Rect,
    title: &str,
    items: &[(&str, &str)],
    theme: &Theme,
) {
    let mut lines = Vec::new();

    // Header
    lines.push(Line::from(vec![Span::styled(
        title,
        Style::default()
            .fg(theme.table_header)
            .add_modifier(Modifier::BOLD),
    )]));

    // Items
    for (key, description) in items {
        let key_span = Span::styled(
            format!("{} ", key),
            Style::default()
                .fg(theme.footer_key)
                .add_modifier(Modifier::BOLD),
        );
        let desc_span = Span::raw(*description);
        lines.push(Line::from(vec![key_span, desc_span]));
    }

    // No borders - just render the content
    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, area);
}
