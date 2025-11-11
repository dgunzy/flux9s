//! YAML view rendering

use crate::tui::theme::Theme;
use crate::watcher::ResourceState;
use ratatui::{
    layout::Rect,
    text::Line,
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;

/// Clean a JSON object by removing Kubernetes internal fields
fn clean_resource_json(obj: &Value) -> Value {
    match obj {
        Value::Object(map) => {
            let mut cleaned = serde_json::Map::new();
            for (key, value) in map {
                // Skip Kubernetes internal fields that clutter the YAML view
                match key.as_str() {
                    "managedFields" => continue, // Skip managedFields entirely (contains f: paths)
                    _ => {
                        // Recursively clean nested objects
                        cleaned.insert(key.clone(), clean_resource_json(value));
                    }
                }
            }
            Value::Object(cleaned)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(clean_resource_json).collect()),
        other => other.clone(),
    }
}

/// Render the YAML view
pub fn render_resource_yaml(
    f: &mut Frame,
    area: Rect,
    selected_resource_key: &Option<String>,
    state: &ResourceState,
    resource_objects: &Arc<RwLock<HashMap<String, serde_json::Value>>>,
    yaml_fetched: &Option<serde_json::Value>,
    yaml_fetch_pending: &Option<String>,
    yaml_scroll_offset: &mut usize,
    _theme: &Theme,
) {
    let key = match selected_resource_key {
        Some(k) => k,
        None => {
            let text = vec![Line::from("No resource selected")];
            let block = Block::default().title("YAML").borders(Borders::ALL);
            let paragraph = Paragraph::new(text).block(block);
            f.render_widget(paragraph, area);
            return;
        }
    };

    // Check if we have fetched YAML or need to use stored object
    let obj_json = if let Some(ref fetched) = yaml_fetched {
        // Use fetched YAML (complete)
        fetched.clone()
    } else if yaml_fetch_pending.is_some() {
        // Show loading message
        let text = vec![
            Line::from("Loading YAML from API..."),
            Line::from(""),
            Line::from("Fetching complete resource..."),
        ];
        let block = Block::default().title("YAML").borders(Borders::ALL);
        let paragraph = Paragraph::new(text).block(block);
        f.render_widget(paragraph, area);
        return;
    } else {
        // Fall back to stored object
        let objects = resource_objects.read().unwrap();
        match objects.get(key).cloned() {
            Some(obj) => obj,
            None => {
                let text = vec![Line::from("Resource YAML not available")];
                let block = Block::default().title("YAML").borders(Borders::ALL);
                let paragraph = Paragraph::new(text).block(block);
                f.render_widget(paragraph, area);
                return;
            }
        }
    };

    // Clean the JSON object to remove Kubernetes internal fields
    let cleaned_json = clean_resource_json(&obj_json);

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
    // Preserve leading spaces for proper YAML indentation
    let visible_lines: Vec<Line> = all_lines
        .iter()
        .skip(*yaml_scroll_offset)
        .take(visible_height as usize)
        .map(|line| {
            // Preserve the line as-is, including leading spaces for indentation
            Line::from(*line)
        })
        .collect();

    let block = Block::default().title(title).borders(Borders::ALL);
    // Use Wrap { trim: false } to preserve leading spaces for YAML indentation
    let paragraph = Paragraph::new(visible_lines)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}
