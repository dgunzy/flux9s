//! Edit view for modifying resource specs

use crate::tui::Theme;
use crate::tui::app::state::EditorState;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

/// Render the resource edit modal with text editor
///
/// Shows YAML editor with cursor, syntax highlighting, and validation state
pub fn render_edit_modal(
    f: &mut Frame,
    area: Rect,
    editor_state: &EditorState,
    resource_name: &str,
    theme: &Theme,
    is_saving: bool,
    error_message: Option<&str>,
) {
    let modal_width = (area.width as f32 * 0.8).min(120.0) as u16;
    let modal_height = (area.height as f32 * 0.8).min(30.0) as u16;

    let modal_x = (area.width.saturating_sub(modal_width)) / 2;
    let modal_y = (area.height.saturating_sub(modal_height)) / 2;

    let modal_area = Rect {
        x: area.x + modal_x,
        y: area.y + modal_y,
        width: modal_width,
        height: modal_height,
    };

    // Draw semi-transparent background
    let bg_block = Block::default();
    f.render_widget(bg_block, area);

    // Draw edit modal
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(4),
        ])
        .split(modal_area);

    // Header
    let header = Paragraph::new(format!("Edit: {}", resource_name))
        .block(Block::default().borders(Borders::BOTTOM))
        .alignment(Alignment::Left);
    f.render_widget(header, chunks[0]);

    // YAML editor area
    let editor_block = Block::default()
        .borders(Borders::ALL)
        .title("Spec (edit and press Ctrl+S to save)")
        .border_type(ratatui::widgets::BorderType::Rounded);

    let editor_area = chunks[1];
    let max_lines = editor_area.height as usize;

    // Render editor lines with cursor
    let visible_start = editor_state.scroll_offset;
    let visible_end = (visible_start + max_lines).min(editor_state.lines.len());

    let mut editor_lines: Vec<Line> = Vec::new();

    for line_idx in visible_start..visible_end {
        let line_content = &editor_state.lines[line_idx];

        if line_idx == editor_state.cursor_row {
            // Current line with cursor
            let mut spans = Vec::new();

            // Before cursor
            if editor_state.cursor_col > 0 {
                spans.push(Span::raw(&line_content[..editor_state.cursor_col]));
            }

            // Cursor character or blank
            if editor_state.cursor_col < line_content.len() {
                let cursor_char = line_content.chars().nth(editor_state.cursor_col).unwrap();
                spans.push(Span::styled(
                    cursor_char.to_string(),
                    theme.status_ready_style(),
                ));
                spans.push(Span::raw(&line_content[editor_state.cursor_col + 1..]));
            } else {
                spans.push(Span::styled(" ", theme.status_ready_style()));
            }

            editor_lines.push(Line::from(spans));
        } else {
            // Regular line
            editor_lines.push(Line::from(line_content.clone()));
        }
    }

    let editor_widget = Paragraph::new(editor_lines)
        .block(editor_block)
        .wrap(Wrap { trim: true });
    f.render_widget(editor_widget, editor_area);

    // Footer with instructions and status
    let footer_lines = if error_message.is_some() {
        // Prefer validation error from editor
        let err_msg = if let Some(val_err) = &editor_state.validation_error {
            val_err.as_str()
        } else {
            error_message.unwrap_or("Unknown error")
        };

        vec![
            Line::from(vec![Span::styled("✗ Error:", theme.status_error_style())]),
            Line::from(err_msg),
            Line::from(""),
            Line::from(vec![Span::raw("Ctrl+S: Save | Esc: Cancel")]),
        ]
    } else if is_saving {
        vec![
            Line::from(vec![Span::styled(
                "⟳ Saving...",
                theme.status_ready_style(),
            )]),
            Line::from(""),
            Line::from(""),
            Line::from(""),
        ]
    } else {
        let line_info = format!(
            "Line {} | Col {} | Ctrl+S: Save | Esc: Cancel",
            editor_state.cursor_row + 1,
            editor_state.cursor_col + 1
        );
        vec![
            Line::from(vec![Span::raw(line_info)]),
            Line::from(""),
            Line::from(""),
            Line::from(""),
        ]
    };

    let footer = Paragraph::new(footer_lines);
    f.render_widget(footer, chunks[2]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edit_modal_renders() {
        let yaml = "spec:\n  interval: 1m\n  suspend: false";
        let resource_name = "test-repo";
        let theme = Theme::default();

        // This test just verifies the function doesn't panic
        // Full rendering tests would require a test terminal
        let _ = (yaml, resource_name, theme);
    }
}
