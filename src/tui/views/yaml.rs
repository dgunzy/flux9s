//! YAML view rendering

use crate::tui::app::state::TextSearchState;
use crate::tui::theme::Theme;
use crate::watcher::ResourceState;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};
use std::collections::HashMap;

/// Find the (0-based) indexes of lines containing the search query (case-insensitive).
pub(crate) fn find_match_lines<S: AsRef<str>>(lines: &[S], query: &str) -> Vec<usize> {
    if query.is_empty() {
        return Vec::new();
    }
    let query_lower = query.to_lowercase();
    lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.as_ref().to_lowercase().contains(&query_lower))
        .map(|(idx, _)| idx)
        .collect()
}

/// Update search state from the computed match list and jump the scroll offset
/// to the current match when a jump is pending. Returns the current match line.
pub(crate) fn apply_text_search(
    search: &mut TextSearchState,
    match_lines: &[usize],
    scroll_offset: &mut usize,
    visible_height: usize,
) -> Option<usize> {
    search.total_matches = match_lines.len();
    if !match_lines.is_empty() {
        search.current_match = search.current_match.min(match_lines.len() - 1);
    }
    let current_line = match_lines.get(search.current_match).copied();
    if search.pending_jump {
        search.pending_jump = false;
        if let Some(line_idx) = current_line {
            // Position the match about a third of the way down the view
            *scroll_offset = line_idx.saturating_sub(visible_height / 3);
        }
    }
    current_line
}

/// Append the search status (`/query (i/n)`) to a view title.
pub(crate) fn decorate_title_with_search(title: &mut String, search: &TextSearchState) {
    if search.input_mode {
        title.push_str(&format!(" — /{}_", search.query));
    } else if search.is_active() {
        let position = if search.total_matches == 0 {
            "0/0".to_string()
        } else {
            format!("{}/{}", search.current_match + 1, search.total_matches)
        };
        title.push_str(&format!(" — /{} ({})", search.query, position));
    }
}

/// Render the YAML view
pub fn render_resource_yaml(
    f: &mut Frame,
    area: Rect,
    selected_resource_key: &Option<String>,
    state: &ResourceState,
    resource_objects: &HashMap<String, serde_json::Value>,
    yaml_fetched: Option<&serde_json::Value>,
    yaml_loading: bool,
    yaml_scroll_offset: &mut usize,
    search: &mut TextSearchState,
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
    } else if yaml_loading {
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
    let mut title = if let Some(ref r) = resource {
        format!("YAML - {} - {}", r.resource_type, r.name)
    } else {
        "YAML".to_string()
    };

    // Split YAML into lines and apply scrolling
    let all_lines: Vec<&str> = yaml_text.lines().collect();
    let visible_height = area.height.saturating_sub(2); // Account for borders

    // Text search: find matches, jump to the current one when requested
    let match_lines = find_match_lines(&all_lines, &search.query);
    let current_match_line = apply_text_search(
        search,
        &match_lines,
        yaml_scroll_offset,
        visible_height as usize,
    );
    decorate_title_with_search(&mut title, search);

    // Clamp scroll offset to valid range
    let max_scroll = all_lines.len().saturating_sub(visible_height as usize);
    *yaml_scroll_offset = (*yaml_scroll_offset).min(max_scroll);

    // Get visible lines based on scroll offset
    // Preserve leading spaces for proper YAML indentation and apply syntax highlighting
    let visible_lines: Vec<Line> = all_lines
        .iter()
        .enumerate()
        .skip(*yaml_scroll_offset)
        .take(visible_height as usize)
        .map(|(idx, line)| {
            let styled = highlight_yaml_line(line, theme);
            if Some(idx) == current_match_line {
                styled.style(Style::default().add_modifier(Modifier::REVERSED))
            } else if match_lines.binary_search(&idx).is_ok() {
                styled.style(Style::default().add_modifier(Modifier::UNDERLINED))
            } else {
                styled
            }
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_match_lines_case_insensitive() {
        let lines = vec!["kind: Kustomization", "  name: my-app", "  path: ./apps"];
        assert_eq!(find_match_lines(&lines, "KUSTOMIZATION"), vec![0]);
        assert_eq!(find_match_lines(&lines, "my-app"), vec![1]);
        assert_eq!(find_match_lines(&lines, "a"), vec![0, 1, 2]);
        assert!(find_match_lines(&lines, "nomatch").is_empty());
        assert!(find_match_lines(&lines, "").is_empty());
    }

    #[test]
    fn test_apply_text_search_jumps_to_match() {
        let mut search = TextSearchState {
            query: "x".to_string(),
            pending_jump: true,
            ..Default::default()
        };
        let mut scroll = 0usize;
        let current = apply_text_search(&mut search, &[30, 60], &mut scroll, 30);
        assert_eq!(current, Some(30));
        assert_eq!(search.total_matches, 2);
        assert!(!search.pending_jump);
        // Match placed about a third of the way down a 30-line view
        assert_eq!(scroll, 20);
    }

    #[test]
    fn test_apply_text_search_clamps_current_match() {
        let mut search = TextSearchState {
            query: "x".to_string(),
            current_match: 9, // Stale index from a previous, longer match list
            ..Default::default()
        };
        let mut scroll = 0usize;
        apply_text_search(&mut search, &[5], &mut scroll, 30);
        assert_eq!(search.current_match, 0);
        assert_eq!(search.total_matches, 1);
    }

    #[test]
    fn test_decorate_title_with_search() {
        let mut title = "YAML".to_string();
        let mut search = TextSearchState {
            query: "spec".to_string(),
            current_match: 1,
            total_matches: 4,
            ..Default::default()
        };
        decorate_title_with_search(&mut title, &search);
        assert_eq!(title, "YAML — /spec (2/4)");

        let mut typing_title = "YAML".to_string();
        search.input_mode = true;
        decorate_title_with_search(&mut typing_title, &search);
        assert_eq!(typing_title, "YAML — /spec_");
    }
}
