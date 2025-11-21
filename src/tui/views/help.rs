//! Help view rendering

use crate::tui::theme::Theme;
use crate::watcher::get_all_commands;
use ratatui::{
    layout::Rect,
    text::Line,
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Render the help view
pub fn render_help(f: &mut Frame, area: Rect, _theme: &Theme) {
    let mut help_text = vec![
        Line::from("Keyboard Shortcuts:"),
        Line::from("  q / q! / Esc  - Quit"),
        Line::from("  ?        - Show/hide help"),
        Line::from("  j/k      - Navigate up/down (vim)"),
        Line::from("  :        - Command mode (e.g., :kustomization)"),
        Line::from("  /        - Filter resources"),
        Line::from("  Enter    - View resource details"),
        Line::from("  y        - View YAML manifest"),
        Line::from("  Tab      - Autocomplete command"),
        Line::from(""),
        Line::from("Operations (select resource first):"),
        Line::from("  t        - Trace resource ownership chain"),
        Line::from("  s        - Suspend reconciliation"),
        Line::from("  r        - Resume reconciliation"),
        Line::from("  R        - Reconcile resource"),
        Line::from("  W        - Reconcile with source (Kustomization/HelmRelease)"),
        Line::from("  d        - Delete resource"),
        Line::from(""),
        Line::from("Filter Syntax (/):"),
        Line::from("  /text              - Filter by name"),
        Line::from("  /label:key         - Filter by label key (any value)"),
        Line::from("  /label:key=value   - Filter by label key=value"),
        Line::from("  /ann:key           - Filter by annotation key"),
        Line::from("  /ann:key=value     - Filter by annotation key=value"),
        Line::from("  /annotations:...   - Alias for /ann:..."),
        Line::from(""),
        Line::from("Commands:"),
        Line::from("  :help / :h / :?              - Show/hide this help"),
        Line::from("  :readonly                    - Toggle readonly mode"),
        Line::from("  :skin <name>                 - Change theme/skin"),
        Line::from("  :ns <name> / :namespace <name> - Switch namespace"),
        Line::from("  :ns all / :ns -A             - Show all namespaces"),
        Line::from("  :all / :clear                - Show all resources"),
        Line::from("  :q / :quit / :exit           - Quit application"),
        Line::from(""),
    ];

    // Add commands from registry (show first few with aliases)
    let commands = get_all_commands();
    for (display_name, aliases) in commands.iter().take(8) {
        let alias_str = aliases.first().unwrap_or(display_name);
        let cmd_line = format!(
            "  :{} / :{}  - Show {}",
            alias_str.to_lowercase(),
            display_name.to_lowercase(),
            display_name
        );
        help_text.push(Line::from(cmd_line));
    }

    if commands.len() > 8 {
        help_text.push(Line::from(format!("  ... and {} more", commands.len() - 8)));
    }

    help_text.push(Line::from("  :all / :clear  - Show all resources"));

    let block = Block::default().title("Help").borders(Borders::ALL);
    let paragraph = Paragraph::new(help_text).block(block);
    f.render_widget(paragraph, area);
}
