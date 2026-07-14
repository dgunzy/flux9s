//! Controller pod log view rendering
//!
//! Renders the `:logs` stream: a scrollable text view over the bounded line
//! buffer, following the newest output until the user scrolls up (`G`
//! resumes). Reuses the shared text-search machinery (`/`, `n`/`N`).

use crate::tui::app::logs::LogSession;
use crate::tui::app::state::TextSearchState;
use crate::tui::theme::Theme;
use crate::tui::views::yaml::{apply_text_search, decorate_title_with_search, find_match_lines};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

/// Render the controller log view.
pub fn render_controller_logs(
    f: &mut Frame,
    area: Rect,
    session: Option<&LogSession>,
    loading: bool,
    follow: bool,
    scroll_offset: &mut usize,
    search: &mut TextSearchState,
    theme: &Theme,
) {
    let Some(session) = session else {
        if loading {
            crate::tui::views::helpers::render_loading_state(
                f,
                area,
                "Logs",
                "Starting log stream...",
                theme,
            );
        } else {
            crate::tui::views::helpers::render_empty_state(
                f,
                area,
                "Logs",
                "No log stream active",
                "Run :logs to stream a Flux controller pod",
                theme,
            );
        }
        return;
    };

    let mut title = format!("Logs: {}/{}", session.namespace, session.pod);
    if let Some(ref status) = session.status {
        title.push_str(&format!(" ({})", status));
    } else if follow {
        title.push_str(" [following]");
    }

    let lines: Vec<&str> = session.lines().iter().map(String::as_str).collect();
    let visible_height = (area.height as usize).saturating_sub(2);
    let max_scroll = lines.len().saturating_sub(visible_height);

    // Text search: matches pin the scroll position; following would yank it
    // back to the bottom, so an active search pauses the auto-follow.
    let match_lines = find_match_lines(&lines, &search.query);
    let current_match_line = apply_text_search(search, &match_lines, scroll_offset, visible_height);
    decorate_title_with_search(&mut title, search);

    if follow && !search.is_active() {
        *scroll_offset = max_scroll;
    }
    *scroll_offset = (*scroll_offset).min(max_scroll);

    let visible_lines: Vec<Line> = lines
        .iter()
        .enumerate()
        .skip(*scroll_offset)
        .take(visible_height)
        .map(|(idx, line)| {
            let styled = Line::from(Span::styled(
                (*line).to_string(),
                Style::default().fg(theme.text_primary),
            ));
            if Some(idx) == current_match_line {
                styled.style(Style::default().add_modifier(Modifier::REVERSED))
            } else if match_lines.binary_search(&idx).is_ok() {
                styled.style(Style::default().add_modifier(Modifier::UNDERLINED))
            } else {
                styled
            }
        })
        .collect();

    if lines.len() > visible_height {
        let first = *scroll_offset + 1;
        let last = (*scroll_offset + visible_height).min(lines.len());
        title.push_str(&format!(" [{}-{}/{}]", first, last, lines.len()));
    }

    let block = crate::tui::views::helpers::create_themed_block(&title, theme);
    let paragraph = Paragraph::new(visible_lines).block(block);
    f.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::logs::{LogEvent, LogState};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    /// Build a LogState with an active session containing the given lines.
    /// Channels work without a runtime, so no tokio::test needed.
    fn state_with_lines(lines: &[&str]) -> LogState {
        let mut state = LogState::default();
        state.request(
            "flux-system".to_string(),
            "source-controller-abc".to_string(),
        );
        let (_, tx) = state.dispatch().unwrap();
        for line in lines {
            tx.send(LogEvent::Line((*line).to_string())).unwrap();
        }
        state.drain();
        state
    }

    fn render_to_text(state: &mut LogState, scroll_offset: &mut usize, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(60, height)).unwrap();
        terminal
            .draw(|frame| {
                render_controller_logs(
                    frame,
                    frame.area(),
                    state.session.as_ref(),
                    state.is_loading(),
                    state.follow,
                    scroll_offset,
                    &mut TextSearchState::default(),
                    &Theme::default(),
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut text = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                text.push_str(buffer[(x, y)].symbol());
            }
            text.push('\n');
        }
        text
    }

    #[test]
    fn no_session_renders_empty_state() {
        let mut state = LogState::default();
        let mut scroll = 0;
        let text = render_to_text(&mut state, &mut scroll, 8);
        assert!(text.contains("No log stream active"));
    }

    #[test]
    fn follow_pins_view_to_newest_lines() {
        // 20 lines into an 8-row terminal (6 content rows): following must
        // show the newest lines, not the oldest.
        let lines: Vec<String> = (1..=20).map(|i| format!("log line {i}")).collect();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let mut state = state_with_lines(&refs);
        let mut scroll = 0;

        let text = render_to_text(&mut state, &mut scroll, 8);
        assert!(text.contains("log line 20"), "newest line visible");
        assert!(!text.contains("log line 1 "), "oldest line scrolled out");
        assert!(text.contains("[following]"), "title shows follow mode");
        assert!(text.contains("source-controller-abc"));

        // Follow paused: the manual scroll position wins
        state.follow = false;
        scroll = 0;
        let text = render_to_text(&mut state, &mut scroll, 8);
        assert!(text.contains("log line 1"), "top of buffer visible");
        assert!(!text.contains("[following]"));
    }

    #[test]
    fn stream_status_shows_in_title() {
        let mut state = LogState::default();
        state.request("ns".to_string(), "pod".to_string());
        let (_, tx) = state.dispatch().unwrap();
        tx.send(LogEvent::Line("only line".to_string())).unwrap();
        tx.send(LogEvent::Ended).unwrap();
        state.drain();

        let mut scroll = 0;
        let text = render_to_text(&mut state, &mut scroll, 8);
        assert!(text.contains("stream ended"));
        assert!(text.contains("only line"));
    }
}
