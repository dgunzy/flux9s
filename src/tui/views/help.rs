//! Help view rendering

use crate::tui::app::actions::{HelpColumn, help_lines};
use crate::tui::theme::Theme;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

/// Render the help view with columns (K9s-style)
/// `scroll` is the shared offset of all columns, clamped here to the last
/// page so `G` (`usize::MAX`) lands at the bottom.
pub fn render_help(
    f: &mut Frame,
    area: Rect,
    theme: &Theme,
    namespace_hotkeys: &[String],
    views: &[(String, String)],
    scroll: &mut usize,
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

    // Key columns come from the action table (app/actions.rs).
    let resource_items = help_lines(HelpColumn::Resource);

    // GENERAL column: general keys, popup hints, then every `:` command.
    let mut general_items = help_lines(HelpColumn::General);
    general_items.extend([
        ("<Tab>", "Autocomplete command"),
        ("(menus)", "Type to filter, PgUp/PgDn to page"),
    ]);
    general_items.extend(crate::tui::app::command_mode::command_help());
    // User-defined views (#302), named like commands: `:apps  ks, hr`.
    general_items.extend(
        views
            .iter()
            .map(|(name, kinds)| (name.as_str(), kinds.as_str())),
    );
    // Discovered kinds (#197) get a dynamic entry only when discovery has
    // actually registered something — no noise for normal users.
    let discovered = crate::models::extra_kinds::global().kind_names();
    let discovered_entry = format!(":{} …", discovered.join(", :").to_lowercase());
    if !crate::models::extra_kinds::global().is_empty() {
        general_items.push((discovered_entry.as_str(), "Discovered kinds (CRD)"));
    }

    // NAVIGATION column
    let nav_items = help_lines(HelpColumn::Navigation);

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

    // All columns scroll together; clamp so the last line can reach the
    // bottom but no further (+1 for each column's header line).
    let longest = [
        resource_items.len(),
        general_items.len(),
        nav_items.len(),
        hotkey_items_ref.len(),
    ]
    .into_iter()
    .max()
    .unwrap_or(0)
        + 1;
    let viewport =
        crate::tui::views::scroll::Viewport::text(scroll, longest, inner_area.height as usize);
    let offset = u16::try_from(viewport.offset()).unwrap_or(u16::MAX);
    for (chunk, title, items) in [
        (column_chunks[0], "RESOURCE", resource_items.as_slice()),
        (column_chunks[1], "GENERAL", general_items.as_slice()),
        (column_chunks[2], "NAVIGATION", nav_items.as_slice()),
        (
            column_chunks[3],
            "Namespace Hotkeys",
            hotkey_items_ref.as_slice(),
        ),
    ] {
        render_help_column(f, chunk, title, items, offset, theme);
    }

    // Render single border around all columns
    let block = Block::default().title("Help").borders(Borders::ALL);
    f.render_widget(block, area);
    viewport.render_scrollbar(f, area, theme);
}

/// Render a single help column (no borders, just content)
fn render_help_column(
    f: &mut Frame,
    area: Rect,
    title: &str,
    items: &[(&str, &str)],
    scroll: u16,
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
    let paragraph = Paragraph::new(lines).scroll((scroll, 0));
    f.render_widget(paragraph, area);
}
