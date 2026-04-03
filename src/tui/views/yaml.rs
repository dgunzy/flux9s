//! YAML view rendering

use crate::tui::theme::Theme;
use crate::watcher::ResourceState;
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};
use std::collections::HashMap;

/// Render the YAML view
pub fn render_resource_yaml(
    f: &mut Frame,
    area: Rect,
    selected_resource_key: &Option<String>,
    state: &ResourceState,
    resource_objects: &HashMap<String, serde_json::Value>,
    yaml_fetched: &Option<serde_json::Value>,
    yaml_fetch_pending: &Option<String>,
    yaml_scroll_offset: &mut usize,
    theme: &Theme,
) {
    let key = match selected_resource_key {
        Some(k) => k,
        None => {
            crate::tui::views::helpers::render_empty_state(
                f,
                area,
                "YAML",
                "No resource selected",
                "Select a resource to view YAML",
                theme,
            );
            return;
        }
    };

    // Check if we have fetched YAML or need to use stored object
    let obj_json = if let Some(fetched) = yaml_fetched {
        // Use fetched YAML (complete)
        fetched.clone()
    } else if yaml_fetch_pending.is_some() {
        crate::tui::views::helpers::render_loading_state(
            f,
            area,
            "YAML",
            "Loading YAML from API... Fetching complete resource...",
            theme,
        );
        return;
    } else {
        // Fall back to stored object
        match resource_objects.get(key).cloned() {
            Some(obj) => obj,
            None => {
                crate::tui::views::helpers::render_empty_state(
                    f,
                    area,
                    "YAML",
                    "Resource YAML not available",
                    "Resource may have been deleted",
                    theme,
                );
                return;
            }
        }
    };

    // Clean the JSON object to remove Kubernetes internal fields
    let cleaned_json = crate::tui::views::helpers::clean_resource_json(&obj_json);

    // Convert JSON to YAML using serde_yaml with proper formatting
    // serde_yaml automatically handles indentation with spaces
    let yaml_text = match serde_yaml::to_string(&cleaned_json) {
        Ok(yaml) => yaml,
        Err(e) => {
            // Fallback to JSON pretty print if YAML conversion fails
            format!(
                "Error converting to YAML: {}\n\nJSON:\n{}",
                e,
                serde_json::to_string_pretty(&cleaned_json)
                    .unwrap_or_else(|_| "Failed to serialize".to_string())
            )
        }
    };

    let resource = state.get(key);
    let title = if let Some(ref r) = resource {
        format!("YAML - {} - {}", r.resource_type, r.name)
    } else {
        "YAML".to_string()
    };

    // Split YAML into lines and apply scrolling
    let all_lines: Vec<&str> = yaml_text.lines().collect();
    let visible_height = area.height.saturating_sub(2); // Account for borders

    // Clamp scroll offset to valid range
    let max_scroll = all_lines.len().saturating_sub(visible_height as usize);
    *yaml_scroll_offset = (*yaml_scroll_offset).min(max_scroll);

    // Get visible lines based on scroll offset
    // Preserve leading spaces for proper YAML indentation and apply syntax highlighting
    let visible_lines: Vec<Line> = all_lines
        .iter()
        .skip(*yaml_scroll_offset)
        .take(visible_height as usize)
        .map(|line| highlight_yaml_line(line, theme))
        .collect();

    let block = crate::tui::views::helpers::create_themed_block(&title, theme);
    // Use Wrap { trim: false } to preserve leading spaces for YAML indentation
    let paragraph = Paragraph::new(visible_lines)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}

/// Highlight a YAML line using the active theme's label, value, and secondary text colors.
fn highlight_yaml_line(line: &str, theme: &Theme) -> Line<'static> {
    let mut spans = Vec::new();
    let mut chars = line.chars().peekable();
    let mut current_token = String::new();
    let mut in_string = false;
    let mut string_quote = None;
    let mut in_comment = false;
    let mut after_colon = false;

    // Process the line character by character
    while let Some(ch) = chars.next() {
        if in_comment {
            // Everything after # is a comment
            current_token.push(ch);
            continue;
        }

        if in_string {
            current_token.push(ch);
            if ch == string_quote.unwrap() {
                // End of string
                spans.push(Span::styled(
                    current_token.clone(),
                    Style::default().fg(theme.text_value),
                ));
                current_token.clear();
                in_string = false;
                string_quote = None;
                after_colon = false;
            }
            continue;
        }

        match ch {
            '#' => {
                // Start of comment
                if !current_token.is_empty() {
                    // Flush current token
                    let color = if after_colon {
                        get_value_color(theme)
                    } else {
                        theme.text_label
                    };
                    spans.push(Span::styled(
                        current_token.clone(),
                        Style::default().fg(color),
                    ));
                    current_token.clear();
                }
                in_comment = true;
                current_token.push(ch);
            }
            ':' => {
                // Key-value separator
                if !current_token.is_empty() {
                    // This is a key
                    spans.push(Span::styled(
                        current_token.clone(),
                        Style::default().fg(theme.text_label),
                    ));
                    current_token.clear();
                }
                spans.push(Span::styled(":", Style::default().fg(theme.text_label)));
                after_colon = true;
            }
            '"' | '\'' => {
                // Start of string
                if !current_token.is_empty() {
                    // Flush current token (might be part of key or value)
                    let color = if after_colon {
                        get_value_color(theme)
                    } else {
                        theme.text_label
                    };
                    spans.push(Span::styled(
                        current_token.clone(),
                        Style::default().fg(color),
                    ));
                    current_token.clear();
                }
                in_string = true;
                string_quote = Some(ch);
                current_token.push(ch);
            }
            '-' if current_token.is_empty() && chars.peek().map(|c| *c == ' ') == Some(true) => {
                // List item marker
                spans.push(Span::styled("-", Style::default().fg(theme.text_label)));
                chars.next(); // Skip the space
            }
            ' ' | '\t' => {
                if !current_token.is_empty() {
                    // Flush token
                    let color = if after_colon {
                        get_value_color(theme)
                    } else {
                        theme.text_label
                    };
                    spans.push(Span::styled(
                        current_token.clone(),
                        Style::default().fg(color),
                    ));
                    current_token.clear();
                }
                // Preserve whitespace
                spans.push(Span::raw(ch.to_string()));
            }
            _ => {
                current_token.push(ch);
            }
        }
    }

    // Flush remaining token
    if !current_token.is_empty() {
        let color = if in_comment {
            theme.text_secondary
        } else if in_string {
            theme.text_value
        } else if after_colon {
            get_value_color(theme)
        } else {
            theme.text_label
        };
        spans.push(Span::styled(current_token, Style::default().fg(color)));
    }

    Line::from(spans)
}

/// YAML values should use the same themed value color as other detail views.
fn get_value_color(theme: &Theme) -> ratatui::style::Color {
    theme.text_value
}
