//! Plugin column extraction and rendering
//!
//! Provides JSONPath evaluation and column rendering for plugin resources.

use crate::plugins::manifest::{ColumnConfig, Renderer};
use crate::tui::Theme;
use chrono::{DateTime, Utc};
use ratatui::style::Style;
use ratatui::text::Span;
use serde_json::Value;
use std::collections::HashMap;

/// Extract a value from a JSON object using a JSONPath expression
///
/// Supports basic JSONPath syntax:
/// - `.field` - access object field
/// - `.field.subfield` - nested access
/// - `[index]` - array access
/// - `[?(@.field == "value")]` - basic filter (limited support)
pub fn extract_jsonpath_value(obj: &Value, path: &str) -> Option<Value> {
    if path.is_empty() {
        return Some(obj.clone());
    }

    // Remove leading dot if present
    let path = path.strip_prefix('.').unwrap_or(path);

    // Handle array access [index]
    if let Some(bracket_pos) = path.find('[') {
        let before_bracket = &path[..bracket_pos];
        let after_bracket = &path[bracket_pos..];

        // First navigate to the object/array
        let mut current = if before_bracket.is_empty() {
            obj.clone()
        } else {
            extract_jsonpath_value(obj, before_bracket)?
        };

        // Handle array index access [0], [1], etc.
        if let Some(close_bracket) = after_bracket.find(']') {
            let index_str = &after_bracket[1..close_bracket];
            if let Ok(index) = index_str.parse::<usize>() {
                if let Some(arr) = current.as_array() {
                    current = arr.get(index)?.clone();
                } else {
                    return None;
                }
            } else {
                // Handle filter expressions like [?(@.field == "value")]
                // For now, just return None - can be extended later
                return None;
            }

            // Continue with remaining path after bracket
            let remaining = &after_bracket[close_bracket + 1..];
            if !remaining.is_empty() {
                return extract_jsonpath_value(&current, remaining);
            }
            return Some(current);
        }
    }

    // Split path by dots to navigate nested objects
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = obj.clone();

    for part in parts {
        if part.is_empty() {
            continue;
        }

        // Handle array access in path (e.g., "items[0].name")
        if let Some(bracket_pos) = part.find('[') {
            let field_name = &part[..bracket_pos];
            let bracket_part = &part[bracket_pos..];

            // Get the field first
            current = current.get(field_name)?.clone();

            // Handle array index
            if let Some(close_bracket) = bracket_part.find(']') {
                let index_str = &bracket_part[1..close_bracket];
                if let Ok(index) = index_str.parse::<usize>() {
                    if let Some(arr) = current.as_array() {
                        current = arr.get(index)?.clone();
                    } else {
                        return None;
                    }
                } else {
                    return None;
                }
            }
        } else {
            // Simple field access
            current = current.get(part)?.clone();
        }
    }

    Some(current)
}

/// Extract all plugin column values from a resource object
pub fn extract_plugin_columns(obj: &Value, columns: &[ColumnConfig]) -> HashMap<String, String> {
    let mut fields = HashMap::new();

    for column in columns {
        if !column.enabled {
            continue;
        }

        if let Some(value) = extract_jsonpath_value(obj, &column.path) {
            // Convert value to string (rendering happens later)
            let value_str = match value {
                Value::String(s) => s,
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                Value::Null => "-".to_string(),
                Value::Array(_) | Value::Object(_) => {
                    // For complex types, serialize to JSON string
                    serde_json::to_string(&value).unwrap_or_else(|_| "-".to_string())
                }
            };
            fields.insert(column.name.clone(), value_str);
        } else {
            fields.insert(column.name.clone(), "-".to_string());
        }
    }

    fields
}

/// Render a column value using the specified renderer
pub fn render_column_value(value: &str, renderer: &Renderer, theme: &Theme) -> Span<'static> {
    match renderer {
        Renderer::Text => Span::raw(value.to_string()),
        Renderer::StatusBadge => render_status_badge(value, theme),
        Renderer::Duration => Span::raw(format_duration(value)),
        Renderer::Age => Span::raw(format_age(value)),
        Renderer::Boolean => render_boolean(value, theme),
        Renderer::IssueBadge => render_issue_badge(value, theme),
        Renderer::PercentageBar => render_percentage_bar(value, theme),
    }
}

/// Render a status badge with color coding
fn render_status_badge(value: &str, theme: &Theme) -> Span<'static> {
    let (text, color) = match value.to_lowercase().as_str() {
        "healthy" | "ready" | "synced" | "succeeded" | "true" => {
            (value.to_string(), theme.status_ready_color())
        }
        "degraded" | "warning" | "progressing" | "pending" => {
            (value.to_string(), theme.status_warning_color())
        }
        "unhealthy" | "failed" | "error" | "false" => {
            (value.to_string(), theme.status_error_color())
        }
        "suspended" | "unknown" => (value.to_string(), theme.text_secondary),
        _ => (value.to_string(), theme.text_primary),
    };

    Span::styled(text, Style::default().fg(color))
}

/// Format a duration string (e.g., "3600" seconds -> "1h")
fn format_duration(value: &str) -> String {
    // Try to parse as seconds (integer)
    if let Ok(secs) = value.parse::<u64>() {
        if secs < 60 {
            return format!("{}s", secs);
        } else if secs < 3600 {
            let mins = secs / 60;
            let remaining_secs = secs % 60;
            if remaining_secs == 0 {
                return format!("{}m", mins);
            } else {
                return format!("{}m {}s", mins, remaining_secs);
            }
        } else {
            let hours = secs / 3600;
            let remaining_mins = (secs % 3600) / 60;
            if remaining_mins == 0 {
                return format!("{}h", hours);
            } else {
                return format!("{}h {}m", hours, remaining_mins);
            }
        }
    }

    // If not a number, return as-is
    value.to_string()
}

/// Format an age string from a timestamp
fn format_age(value: &str) -> String {
    // Try to parse as RFC3339 timestamp
    if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
        let now = Utc::now();
        let duration = now.signed_duration_since(dt.with_timezone(&Utc));

        if duration.num_seconds() < 60 {
            format!("{}s", duration.num_seconds())
        } else if duration.num_minutes() < 60 {
            format!("{}m", duration.num_minutes())
        } else if duration.num_hours() < 24 {
            format!("{}h", duration.num_hours())
        } else {
            format!("{}d", duration.num_days())
        }
    } else {
        // If not a timestamp, return as-is
        value.to_string()
    }
}

/// Render a boolean value
fn render_boolean(value: &str, theme: &Theme) -> Span<'static> {
    let (text, color) = match value.to_lowercase().as_str() {
        "true" | "1" | "yes" => ("✓", theme.status_ready_color()),
        "false" | "0" | "no" => ("✗", theme.status_error_color()),
        _ => ("?", theme.text_secondary),
    };

    Span::styled(text.to_string(), Style::default().fg(color))
}

/// Render an issue badge
fn render_issue_badge(value: &str, theme: &Theme) -> Span<'static> {
    if let Ok(count) = value.parse::<u32>() {
        if count == 0 {
            Span::raw("-")
        } else if count == 1 {
            Span::styled("⚠ 1", Style::default().fg(theme.status_warning_color()))
        } else {
            Span::styled(
                format!("🔴 {}", count),
                Style::default().fg(theme.status_error_color()),
            )
        }
    } else {
        Span::raw(value.to_string())
    }
}

/// Render a percentage bar
fn render_percentage_bar(value: &str, theme: &Theme) -> Span<'static> {
    if let Ok(percent) = value.parse::<u8>() {
        let bar_width = 8;
        let filled = (percent as f32 / 100.0 * bar_width as f32) as usize;
        let bar = format!(
            "[{}{}] {}%",
            "█".repeat(filled.min(bar_width)),
            "░".repeat((bar_width - filled).max(0)),
            percent
        );
        Span::styled(bar, Style::default().fg(theme.text_primary))
    } else {
        Span::raw(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_jsonpath_simple() {
        let json: Value = serde_json::json!({
            "name": "test",
            "status": "ready"
        });

        assert_eq!(
            extract_jsonpath_value(&json, ".name"),
            Some(Value::String("test".to_string()))
        );
        assert_eq!(
            extract_jsonpath_value(&json, ".status"),
            Some(Value::String("ready".to_string()))
        );
    }

    #[test]
    fn test_extract_jsonpath_nested() {
        let json: Value = serde_json::json!({
            "metadata": {
                "name": "test",
                "namespace": "default"
            }
        });

        assert_eq!(
            extract_jsonpath_value(&json, ".metadata.name"),
            Some(Value::String("test".to_string()))
        );
        assert_eq!(
            extract_jsonpath_value(&json, ".metadata.namespace"),
            Some(Value::String("default".to_string()))
        );
    }

    #[test]
    fn test_extract_jsonpath_array() {
        let json: Value = serde_json::json!({
            "items": ["a", "b", "c"]
        });

        assert_eq!(
            extract_jsonpath_value(&json, ".items[0]"),
            Some(Value::String("a".to_string()))
        );
        assert_eq!(
            extract_jsonpath_value(&json, ".items[1]"),
            Some(Value::String("b".to_string()))
        );
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration("30"), "30s");
        assert_eq!(format_duration("60"), "1m");
        assert_eq!(format_duration("90"), "1m 30s");
        assert_eq!(format_duration("3600"), "1h");
        assert_eq!(format_duration("3660"), "1h 1m");
    }
}
