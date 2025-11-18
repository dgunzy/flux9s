//! Footer view rendering

use crate::tui::operations::OperationRegistry;
use crate::tui::theme::Theme;
use crate::watcher::{get_all_commands, ResourceState};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Render the footer based on current application state
/// Returns the number of lines used (for dynamic height calculation)
pub fn render_footer(
    f: &mut Frame,
    area: Rect,
    command_mode: bool,
    command_buffer: &str,
    filter_mode: bool,
    filter: &str,
    show_help: bool,
    confirmation_pending: &Option<(String, String, String, char)>,
    status_message: &Option<(String, bool)>,
    operation_registry: &OperationRegistry,
    state: &ResourceState,
    theme: &Theme,
) -> usize {
    if command_mode {
        return render_command_footer(f, area, command_buffer, theme);
    }

    if filter_mode {
        return render_filter_footer(f, area, filter, theme);
    }

    // Handle default navigation footer (wrapped for smaller screens)
    if !show_help && confirmation_pending.is_none() && status_message.is_none() {
        return render_navigation_footer(f, area, theme);
    }

    // Build footer text for non-default cases
    let footer_text: Vec<Span> = if show_help {
        vec![
            Span::raw("Press "),
            Span::styled("?", theme.footer_key_style()),
            Span::raw(" to hide help"),
        ]
    } else if let Some((ref resource_type, ref namespace, ref name, op_key)) = confirmation_pending
    {
        render_confirmation_footer_text(
            operation_registry,
            state,
            resource_type,
            namespace,
            name,
            *op_key,
            theme,
        )
    } else if let Some((ref msg, is_error)) = status_message {
        vec![Span::styled(
            msg.clone(),
            if *is_error {
                theme.operation_error_style()
            } else {
                theme.operation_success_style()
            },
        )]
    } else {
        vec![]
    };

    let footer =
        Paragraph::new(Line::from(footer_text)).block(Block::default().borders(Borders::ALL));
    f.render_widget(footer, area);
    1 // Single line for these cases
}

fn render_command_footer(f: &mut Frame, area: Rect, command_buffer: &str, theme: &Theme) -> usize {
    let cmd = command_buffer.trim().to_lowercase();
    let mut command_line = vec![
        Span::styled(":", theme.command_prompt_style()),
        Span::raw(command_buffer),
        Span::raw("_"), // Cursor
    ];

    // Add autocomplete hint
    if !cmd.is_empty() {
        let commands = get_all_commands();
        let matches: Vec<&str> = commands
            .iter()
            .flat_map(|(_, aliases)| aliases.iter())
            .filter(|alias| alias.starts_with(&cmd))
            .copied()
            .take(1)
            .collect();

        if !matches.is_empty() {
            command_line.push(Span::raw("  ["));
            command_line.push(Span::styled(
                format!("Tab: {}", matches[0]),
                Style::default().fg(theme.command_autocomplete),
            ));
            command_line.push(Span::raw("]"));
        }
    } else {
        // Show help hint when command buffer is empty
        command_line.push(Span::raw("  ["));
        command_line.push(Span::styled(
            "Tab: autocomplete",
            Style::default().fg(theme.command_autocomplete),
        ));
        command_line.push(Span::raw("]"));
    }

    let footer =
        Paragraph::new(Line::from(command_line)).block(Block::default().borders(Borders::ALL));
    f.render_widget(footer, area);
    1
}

fn render_filter_footer(f: &mut Frame, area: Rect, filter: &str, theme: &Theme) -> usize {
    let filter_line = vec![
        Span::styled("/", theme.filter_prompt_style()),
        Span::raw(filter),
        Span::raw("_"), // Cursor
        Span::raw(" (Esc to cancel, Enter to apply)"),
    ];
    let footer =
        Paragraph::new(Line::from(filter_line)).block(Block::default().borders(Borders::ALL));
    f.render_widget(footer, area);
    1
}

fn render_navigation_footer(f: &mut Frame, area: Rect, theme: &Theme) -> usize {
    // Default navigation hints - wrap for smaller screens
    // Returns the number of lines used
    let nav_segments = vec![
        ("j/k ", "Navigate", theme.footer_key),
        (":", "Command", theme.footer_key),
        ("Enter", "Details", theme.footer_key),
        ("y", "YAML", theme.footer_key),
        ("t", "Trace", theme.footer_key),
        ("s", "Suspend", theme.footer_key),
        ("r", "Resume", theme.footer_key),
        ("R", "Reconcile", theme.footer_key),
        ("W", "Reconcile+Source", theme.footer_key),
        ("d", "Delete", theme.footer_key),
        ("/", "Filter(Name)", theme.footer_key),
        ("?", "Help", theme.footer_key),
        ("Esc", "Back/Quit", theme.footer_key),
    ];

    // Calculate available width (accounting for borders)
    let available_width = area.width.saturating_sub(2);
    let wrap_threshold = available_width as usize;

    // Build segments as text first to calculate lengths
    let mut segment_texts: Vec<String> = Vec::new();
    for (key, label, _) in &nav_segments {
        if *key == "j/k " {
            segment_texts.push(format!("{}{}", key, label));
        } else {
            segment_texts.push(format!("{} {}", key, label));
        }
    }

    // Build lines - always try to fit on one line first, wrap if needed
    let mut footer_lines = Vec::new();
    let mut current_line_spans = Vec::new();
    let mut current_length = 0;

    for (idx, ((key, label, color), segment_text)) in
        nav_segments.iter().zip(segment_texts.iter()).enumerate()
    {
        let segment_len = segment_text.len();
        let separator_len = if idx > 0 { 3 } else { 0 }; // " | " separator

        // Check if adding this segment would exceed width (and we haven't wrapped yet)
        if idx > 0
            && footer_lines.is_empty()
            && current_length + segment_len + separator_len > wrap_threshold
        {
            // Finish first line - remove trailing separator
            if !current_line_spans.is_empty() {
                current_line_spans.pop(); // Remove the last " | " separator
            }
            footer_lines.push(Line::from(current_line_spans));
            current_line_spans = Vec::new();
            current_length = 0;
        }

        // Add separator before segment (except first)
        if idx > 0 {
            current_line_spans.push(Span::raw(" | "));
            current_length += separator_len;
        }

        // Add segment spans
        if *key == "j/k " {
            current_line_spans.push(Span::raw(key.to_string()));
            current_line_spans.push(Span::styled(label.to_string(), Style::default().fg(*color)));
        } else {
            current_line_spans.push(Span::styled(key.to_string(), Style::default().fg(*color)));
            current_line_spans.push(Span::raw(format!(" {}", label)));
        }
        current_length += segment_len;
    }

    // Add the last line
    if !current_line_spans.is_empty() {
        footer_lines.push(Line::from(current_line_spans));
    }

    // Ensure we have at least one line
    if footer_lines.is_empty() {
        footer_lines.push(Line::from(vec![Span::raw("")]));
    }

    // Render footer with multiple lines
    let footer = Paragraph::new(footer_lines.clone()).block(Block::default().borders(Borders::ALL));
    f.render_widget(footer, area);

    // Return number of lines used (for dynamic height calculation)
    footer_lines.len()
}

fn render_confirmation_footer_text<'a>(
    operation_registry: &OperationRegistry,
    state: &ResourceState,
    resource_type: &str,
    namespace: &str,
    name: &str,
    op_key: char,
    theme: &Theme,
) -> Vec<Span<'a>> {
    let confirmation_msg = if let Some(operation) = operation_registry.get_by_keybinding(op_key) {
        if let Some(resource) = state.get(&crate::watcher::resource_key(
            namespace,
            name,
            resource_type,
        )) {
            operation.confirmation_message(&resource)
        } else {
            "Resource not found".to_string()
        }
    } else {
        "Unknown operation".to_string()
    };

    vec![
        Span::styled(confirmation_msg, theme.operation_warning_style()),
        Span::raw("  "),
        Span::styled(
            "y",
            Style::default()
                .fg(theme.operation_confirm)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("/"),
        Span::styled(
            "Y",
            Style::default()
                .fg(theme.operation_confirm)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" Confirm | "),
        Span::styled(
            "n",
            Style::default()
                .fg(theme.operation_cancel)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("/"),
        Span::styled(
            "N",
            Style::default()
                .fg(theme.operation_cancel)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("/"),
        Span::styled(
            "Esc",
            Style::default()
                .fg(theme.operation_cancel)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" Cancel"),
    ]
}

// Helper trait extensions for Theme
trait ThemeExt {
    fn command_prompt_style(&self) -> Style;
    fn filter_prompt_style(&self) -> Style;
}

impl ThemeExt for Theme {
    fn command_prompt_style(&self) -> Style {
        Style::default()
            .fg(self.command_prompt)
            .add_modifier(Modifier::BOLD)
    }

    fn filter_prompt_style(&self) -> Style {
        Style::default()
            .fg(self.filter_prompt)
            .add_modifier(Modifier::BOLD)
    }
}
