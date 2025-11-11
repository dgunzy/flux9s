//! Splash screen rendering

use crate::tui::theme::Theme;
use ratatui::{
    layout::Rect,
    style::Modifier,
    text::Line,
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Render the splash screen with ASCII art
pub fn render_splash(f: &mut Frame, area: Rect, theme: &Theme) {
    let ascii_art = r#"
 _____ _             ___      
|  ___| |_   ___  __/ _ \ ___ 
| |_  | | | | \ \/ / (_) / __|
|  _| | | |_| |>  < \__, \__ \
|_|   |_|\__,_/_/\_\  /_/|___/
                           
"#;

    let lines: Vec<Line> = ascii_art
        .lines()
        .map(|line| {
            Line::from(vec![ratatui::text::Span::styled(
                line,
                ratatui::style::Style::default()
                    .fg(theme.header_ascii)
                    .add_modifier(Modifier::BOLD),
            )])
        })
        .collect();

    let block = Block::default()
        .title("Flux9s - Flux GitOps TUI")
        .borders(Borders::ALL);
    let paragraph = Paragraph::new(lines)
        .block(block)
        .alignment(ratatui::layout::Alignment::Center);
    f.render_widget(paragraph, area);
}
