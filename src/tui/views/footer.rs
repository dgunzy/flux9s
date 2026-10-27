//! Footer view rendering

use crate::tui::keybindings::{NavigationCommand, navigation_commands_to_segments};
use crate::tui::theme::Theme;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

/// What the footer shows: the command / filter / search line, or the status and key hints.
pub struct FooterProps<'a> {
    pub command_mode: bool,
    pub command_buffer: &'a str,
    pub filter_mode: bool,
    pub filter: &'a str,
    pub search_input_mode: bool,
    pub search_query: &'a str,
    pub show_help: bool,
    pub show_quit_confirm: bool,
    pub confirmation_message: Option<&'a str>,
    pub status_message: &'a Option<(String, bool)>,
    pub nav_commands: &'a [NavigationCommand],
}

/// Render the footer based on current application state
/// Returns the number of lines used (for dynamic height calculation)
pub fn render_footer(f: &mut Frame, area: Rect, props: FooterProps<'_>, theme: &Theme) -> usize {
    let FooterProps {
        command_mode,
        command_buffer,
        filter_mode,
        filter,
        search_input_mode,
        search_query,
        show_help,
        show_quit_confirm,
        confirmation_message,
        status_message,
        nav_commands,
    } = props;
    if command_mode {
        return render_command_footer(f, area, command_buffer, theme);
    }

    if filter_mode {
        return render_filter_footer(f, area, filter, theme);
    }

    if search_input_mode {
        return render_search_footer(f, area, search_query, theme);
    }

    // While the quit confirm dialog is visible, show only its relevant keys so
    // the footer accurately reflects what is active — not the normal nav hints.
    if show_quit_confirm {
        return render_quit_confirm_footer(f, area, theme);
    }

    // Handle default navigation footer (wrapped for smaller screens)
    if !show_help && confirmation_message.is_none() && status_message.is_none() {
        return render_navigation_footer(f, area, theme, nav_commands);
    }

    // Messages wrap; the layout reserves the rows (see `footer_message_line`).
    let line = footer_message_line(show_help, confirmation_message, status_message, theme)
        .unwrap_or_default();
    let footer =
        crate::tui::views::text::wrapped(line).block(Block::default().borders(Borders::ALL));
    f.render_widget(footer, area);
    1
}

/// The line the footer shows instead of key hints (help hint, pending
/// confirmation, status message), if any. Shared with the layout so the
/// footer's height fits the wrapped message.
pub fn footer_message_line<'a>(
    show_help: bool,
    confirmation_message: Option<&str>,
    status_message: &Option<(String, bool)>,
    theme: &Theme,
) -> Option<Line<'a>> {
    let spans: Vec<Span> = if show_help {
        vec![
            Span::raw("Press "),
            Span::styled("?", theme.footer_key_style()),
            Span::raw(" to hide help · "),
            Span::styled("j/k", theme.footer_key_style()),
            Span::raw(" scroll"),
        ]
    } else if let Some(message) = confirmation_message {
        render_confirmation_footer_text(message, theme)
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
        return None;
    };
    Some(Line::from(spans))
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

/// Footer shown while typing a text-view search query (`/` in YAML/describe/trace)
fn render_search_footer(f: &mut Frame, area: Rect, query: &str, theme: &Theme) -> usize {
    let search_line = vec![
        Span::raw("Search "),
        Span::styled("/", theme.filter_prompt_style()),
        Span::raw(query),
        Span::raw("_"), // Cursor
        Span::raw(" (Enter to search, n/N next/prev, Esc to cancel)"),
    ];
    let footer =
        Paragraph::new(Line::from(search_line)).block(Block::default().borders(Borders::ALL));
    f.render_widget(footer, area);
    1
}

fn render_navigation_footer(
    f: &mut Frame,
    area: Rect,
    theme: &Theme,
    commands: &[NavigationCommand],
) -> usize {
    // Wraps onto a second line for smaller screens; returns the lines used.
    let nav_segments = navigation_commands_to_segments(commands, theme.footer_key);

    // Build wrapped lines similar to header logic
    // Wrap footer content into 2 lines to prevent overflow
    let available_width = area.width.saturating_sub(2); // Account for borders

    // Calculate segment lengths
    let mut segment_lengths: Vec<usize> = Vec::new();
    for (idx, (key, label, _)) in nav_segments.iter().enumerate() {
        let separator_len = if idx > 0 { 3 } else { 0 }; // " | "
        let segment_len = if *key == "j/k " {
            key.len() + label.len()
        } else {
            key.len() + 1 + label.len() // key + space + label
        };
        segment_lengths.push(separator_len + segment_len);
    }

    // Split segments into two lines
    let mut line1_segments = Vec::new();
    let mut line2_segments = Vec::new();
    let mut current_line_length = 0;
    let mut use_line2 = false;

    for (idx, segment) in nav_segments.iter().enumerate() {
        let segment_len = segment_lengths[idx];

        // If adding this segment would exceed width and we're on line 1, start line 2
        if current_line_length + segment_len > available_width as usize
            && !use_line2
            && current_line_length > 0
        {
            use_line2 = true;
            current_line_length = 0;
        }

        if use_line2 {
            line2_segments.push((idx, segment));
            current_line_length += segment_len;
        } else {
            line1_segments.push((idx, segment));
            current_line_length += segment_len;
        }
    }

    // Build lines with spans
    let mut footer_lines = Vec::new();

    // Line 1
    if !line1_segments.is_empty() {
        let mut line1_spans = Vec::new();
        for (idx, (key, label, color)) in line1_segments.iter() {
            if *idx > 0 {
                line1_spans.push(Span::raw(" | "));
            }
            if *key == "j/k " {
                line1_spans.push(Span::raw(key.clone()));
                line1_spans.push(Span::styled(label.clone(), Style::default().fg(*color)));
            } else {
                line1_spans.push(Span::styled(key.clone(), Style::default().fg(*color)));
                line1_spans.push(Span::raw(format!(" {}", label)));
            }
        }
        footer_lines.push(Line::from(line1_spans));
    }

    // Line 2
    if !line2_segments.is_empty() {
        let mut line2_spans = Vec::new();
        for (idx, (key, label, color)) in line2_segments.iter() {
            if *idx > 0 {
                line2_spans.push(Span::raw(" | "));
            }
            if *key == "j/k " {
                line2_spans.push(Span::raw(key.clone()));
                line2_spans.push(Span::styled(label.clone(), Style::default().fg(*color)));
            } else {
                line2_spans.push(Span::styled(key.clone(), Style::default().fg(*color)));
                line2_spans.push(Span::raw(format!(" {}", label)));
            }
        }
        footer_lines.push(Line::from(line2_spans));
    }

    // Render footer with wrapped lines
    let num_lines = footer_lines.len().min(2);
    let footer = Paragraph::new(footer_lines)
        .block(Block::default().borders(Borders::ALL))
        .wrap(Wrap { trim: true });

    f.render_widget(footer, area);

    // Return number of lines used (max 2)
    num_lines
}

/// Footer displayed while the quit confirmation dialog is active.
///
/// Shows only the keys that are valid in this state so the footer matches
/// what the user can actually press.
fn render_quit_confirm_footer(f: &mut Frame, area: Rect, theme: &Theme) -> usize {
    let spans = vec![
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
        Span::raw(" Quit  "),
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
            "q",
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
    ];
    let footer = Paragraph::new(Line::from(spans)).block(Block::default().borders(Borders::ALL));
    f.render_widget(footer, area);
    1
}

/// Footer keys while a confirmation is pending. The question itself is the
/// dialog's (always drawn in the main area), so it isn't repeated here.
fn render_confirmation_footer_text<'a>(_confirmation_msg: &str, theme: &Theme) -> Vec<Span<'a>> {
    vec![
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
