//! Footer view rendering

use crate::tui::app::PendingOperation;
use crate::tui::operations::OperationRegistry;
use crate::tui::theme::Theme;
use crate::watcher::ResourceState;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
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
    confirmation_pending: &Option<PendingOperation>,
    status_message: &Option<(String, bool)>,
    operation_registry: &OperationRegistry,
    state: &ResourceState,
    theme: &Theme,
    namespace_hotkeys: &[String],
    current_namespace: &Option<String>,
) -> usize {
    if command_mode {
        return render_command_footer(f, area, command_buffer, theme);
    }

    if filter_mode {
        return render_filter_footer(f, area, filter, theme);
    }

    // Handle default navigation footer (wrapped for smaller screens)
    if !show_help && confirmation_pending.is_none() && status_message.is_none() {
        return render_navigation_footer(f, area, theme, namespace_hotkeys, current_namespace);
    }

    // Build footer text for non-default cases
    let footer_text: Vec<Span> = if show_help {
        vec![
            Span::raw("Press "),
            Span::styled("?", theme.footer_key_style()),
            Span::raw(" to hide help"),
        ]
    } else if let Some(pending) = confirmation_pending {
        render_confirmation_footer_text(
            operation_registry,
            state,
            &pending.resource_type,
            &pending.namespace,
            &pending.name,
            pending.operation_key,
            theme,
        )
    } else if let Some((msg, is_error)) = status_message {
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
        let matches = crate::tui::commands::find_matching_commands(&cmd);
        if !matches.is_empty() {
            // Show first match, or multiple if there are conflicts
            if matches.len() == 1 {
                command_line.push(Span::raw("  ["));
                command_line.push(Span::styled(
                    format!("Tab: {}", matches[0]),
                    Style::default().fg(theme.command_autocomplete),
                ));
                command_line.push(Span::raw("]"));
            } else {
                // Show multiple options (up to 3) when there are conflicts
                let display_matches: Vec<&str> =
                    matches.iter().take(3).map(|s| s.as_str()).collect();
                let hint = if matches.len() > 3 {
                    format!(
                        "Tab: {} (+{} more)",
                        display_matches.join(", "),
                        matches.len() - 3
                    )
                } else {
                    format!("Tab: {}", display_matches.join(", "))
                };
                command_line.push(Span::raw("  ["));
                command_line.push(Span::styled(
                    hint,
                    Style::default().fg(theme.command_autocomplete),
                ));
                command_line.push(Span::raw("]"));
            }
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

fn render_navigation_footer(
    f: &mut Frame,
    area: Rect,
    theme: &Theme,
    namespace_hotkeys: &[String],
    current_namespace: &Option<String>,
) -> usize {
    // Default navigation hints - wrap for smaller screens
    // Returns the number of lines used
    // Use owned strings for dynamic content
    let mut nav_segments: Vec<(String, String, ratatui::style::Color)> = vec![
        ("j/k ".to_string(), "Navigate".to_string(), theme.footer_key),
        (":".to_string(), "Command".to_string(), theme.footer_key),
        ("Enter".to_string(), "Details".to_string(), theme.footer_key),
        ("y".to_string(), "YAML".to_string(), theme.footer_key),
        ("t".to_string(), "Trace".to_string(), theme.footer_key),
        ("s".to_string(), "Suspend".to_string(), theme.footer_key),
        ("r".to_string(), "Resume".to_string(), theme.footer_key),
        ("R".to_string(), "Reconcile".to_string(), theme.footer_key),
        (
            "W".to_string(),
            "Reconcile+Source".to_string(),
            theme.footer_key,
        ),
        ("d".to_string(), "Delete".to_string(), theme.footer_key),
        (
            "/".to_string(),
            "Filter(Name)".to_string(),
            theme.footer_key,
        ),
        ("?".to_string(), "Help".to_string(), theme.footer_key),
        ("Esc".to_string(), "Back/Quit".to_string(), theme.footer_key),
    ];

    // Add namespace hotkeys (show first few that fit)
    if !namespace_hotkeys.is_empty() {
        // Show up to 3 namespace hotkeys in footer (0, 1, 2)
        for (idx, ns) in namespace_hotkeys.iter().take(3).enumerate() {
            let key = idx.to_string();
            let display_ns = if ns == "all" {
                "all".to_string()
            } else {
                // Truncate long namespace names
                if ns.len() > 8 {
                    ns[..8].to_string()
                } else {
                    ns.clone()
                }
            };
            // Highlight current namespace
            let is_current = if ns == "all" {
                current_namespace.is_none()
            } else {
                current_namespace.as_ref() == Some(ns)
            };
            let label = if is_current {
                format!("NS:{}*", display_ns)
            } else {
                format!("NS:{}", display_ns)
            };
            nav_segments.push((key, label, theme.footer_key));
        }
    }

    // Build a single line with all segments and separators
    // Let ratatui's Paragraph widget handle the wrapping
    let mut spans = Vec::new();

    for (idx, (key, label, color)) in nav_segments.iter().enumerate() {
        // Add separator before segment (except first)
        if idx > 0 {
            spans.push(Span::raw(" | "));
        }

        // Add segment spans
        if key == "j/k " {
            spans.push(Span::raw(key.clone()));
            spans.push(Span::styled(label.clone(), Style::default().fg(*color)));
        } else {
            spans.push(Span::styled(key.clone(), Style::default().fg(*color)));
            spans.push(Span::raw(format!(" {}", label)));
        }
    }

    let line = Line::from(spans);

    // Use ratatui's built-in wrapping with trim to handle line breaks properly
    let footer = Paragraph::new(line)
        .block(Block::default().borders(Borders::ALL))
        .wrap(Wrap { trim: true });

    f.render_widget(footer, area);

    // Calculate number of lines that will be used after wrapping
    // This is an estimate for the dynamic height calculation
    let available_width = area.width.saturating_sub(2);

    // Calculate total content length
    let mut total_length = 0;
    for (idx, (key, label, _)) in nav_segments.iter().enumerate() {
        let separator_len = if idx > 0 { 3 } else { 0 }; // " | "
        let segment_len = if *key == "j/k " {
            key.len() + label.len()
        } else {
            key.len() + 1 + label.len() // key + space + label
        };
        total_length += separator_len + segment_len;
    }

    // Estimate number of lines needed
    let lines_needed = if available_width > 0 {
        ((total_length as f32) / (available_width as f32)).ceil() as usize
    } else {
        1
    };

    lines_needed.max(1)
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
