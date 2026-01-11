//! Submenu view rendering
//!
//! Renders an interactive submenu overlay for command selection.

use crate::tui::submenu::SubmenuState;
use crate::tui::theme::Theme;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

/// Render the submenu overlay
///
/// The submenu is displayed as a centered popup overlay over the current view.
pub fn render_submenu(f: &mut Frame, area: Rect, submenu: &SubmenuState, theme: &Theme) {
    // Calculate popup size - take up a reasonable portion of the screen
    let popup_width = area.width.clamp(30, 60); // Between 30-60 columns
    let popup_height = (submenu.items.len() as u16)
        .saturating_add(6) // Items + border + title + help
        .clamp(10, area.height.saturating_sub(4)); // Between 10 and area height - 4

    // Center the popup
    let popup_area = centered_rect(popup_width, popup_height, area);

    // Clear the background area to make it opaque
    f.render_widget(Clear, popup_area);

    // Render the block with a solid background
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.table_selected_style())
        .style(Style::default().fg(theme.text_primary).bg(Color::Black));

    f.render_widget(block, popup_area);

    // Create inner layout: title, list, help text
    let inner_area = Rect {
        x: popup_area.x + 1,
        y: popup_area.y + 1,
        width: popup_area.width.saturating_sub(2),
        height: popup_area.height.saturating_sub(2),
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Title
            Constraint::Min(3),    // List
            Constraint::Length(1), // Help text
        ])
        .split(inner_area);

    // Render title
    if let Some(title) = &submenu.title {
        let title_paragraph = Paragraph::new(title.as_str())
            .style(
                Style::default()
                    .fg(theme.table_selected)
                    .bg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center);
        f.render_widget(title_paragraph, chunks[0]);
    }

    // Render list items
    let list_height = chunks[1].height as usize;
    let visible_items: Vec<ListItem> = submenu
        .items
        .iter()
        .enumerate()
        .skip(submenu.scroll_offset)
        .take(list_height)
        .map(|(idx, item)| {
            let is_selected = idx == submenu.selected_index;
            let style = if is_selected {
                theme.table_selected_style()
            } else {
                Style::default().fg(theme.text_primary)
            };

            let prefix = if is_selected { "> " } else { "  " };
            let content = format!("{}{}", prefix, item.display_text);

            // If there's a description, show it on a second line
            if let Some(desc) = &item.description {
                let lines = vec![
                    Line::from(Span::styled(content, style)),
                    Line::from(Span::styled(
                        format!("  {}", desc),
                        Style::default()
                            .fg(theme.text_secondary)
                            .add_modifier(Modifier::DIM),
                    )),
                ];
                ListItem::new(lines)
            } else {
                ListItem::new(Line::from(Span::styled(content, style)))
            }
        })
        .collect();

    let list =
        List::new(visible_items).style(Style::default().fg(theme.text_primary).bg(Color::Black));
    f.render_widget(list, chunks[1]);

    // Render help text at the bottom
    if let Some(help) = &submenu.help_text {
        let help_paragraph = Paragraph::new(help.as_str())
            .style(Style::default().fg(theme.text_secondary).bg(Color::Black))
            .alignment(Alignment::Center);
        f.render_widget(help_paragraph, chunks[2]);
    }
}

/// Helper to create a centered rectangle
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let popup_x = area.x.saturating_add(area.width.saturating_sub(width) / 2);
    let popup_y = area
        .y
        .saturating_add(area.height.saturating_sub(height) / 2);

    Rect {
        x: popup_x,
        y: popup_y,
        width: width.min(area.width),
        height: height.min(area.height),
    }
}
