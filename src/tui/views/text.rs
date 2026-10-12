//! Text that may be longer than the screen is wide.
//!
//! Messages built from cluster data or long explanations (confirmations,
//! status lines, empty states) must wrap instead of running off the edge.
//! Render them with [`wrapped`], and when a layout has to make room for the
//! wrapped text (the footer), size it with [`wrapped_height`] — both use the
//! same greedy word wrap, so the space reserved matches what is drawn.

use ratatui::{
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

/// A paragraph that word-wraps at the area's width.
pub fn wrapped<'a>(lines: impl Into<ratatui::text::Text<'a>>) -> Paragraph<'a> {
    Paragraph::new(lines).wrap(Wrap { trim: true })
}

/// Rows `lines` occupy when word-wrapped at `width` columns (at least one
/// per line; a word longer than the width is split).
pub fn wrapped_height(lines: &[Line<'_>], width: u16) -> u16 {
    let width = usize::from(width.max(1));
    let rows: usize = lines.iter().map(|line| line_rows(line, width)).sum();
    u16::try_from(rows).unwrap_or(u16::MAX)
}

fn line_rows(line: &Line<'_>, width: usize) -> usize {
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    let mut rows = 1;
    let mut used = 0;
    for word in text.split_whitespace() {
        let len = Span::raw(word).width();
        let needed = if used == 0 { len } else { used + 1 + len };
        if needed <= width {
            used = needed;
            continue;
        }
        if used > 0 {
            rows += 1;
        }
        // A word wider than the row takes whole rows of its own.
        rows += len.saturating_sub(1) / width;
        used = len % width;
        if used == 0 && len > 0 {
            used = width;
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn height_counts_word_wrapped_rows() {
        assert_eq!(wrapped_height(&[Line::from("short")], 20), 1);
        assert_eq!(wrapped_height(&[Line::from("aaa bbb ccc")], 7), 2);
        assert_eq!(wrapped_height(&[Line::from(""), Line::from("x")], 10), 2);
        assert_eq!(
            wrapped_height(&[Line::from("abcdefghij")], 4),
            3,
            "long words split"
        );
    }

    #[test]
    fn height_matches_what_ratatui_draws() {
        let message = "Deployment cabot-cup is managed by Flux Kustomization flux-system/apps, which is suspended — your change stays until Kustomization apps is resumed. Edit anyway?";
        for width in [20u16, 33, 47, 80] {
            let lines = [Line::from(message)];
            let height = wrapped_height(&lines, width);
            let mut terminal = Terminal::new(TestBackend::new(width, 20)).unwrap();
            terminal
                .draw(|f| f.render_widget(wrapped(lines.to_vec()), f.area()))
                .unwrap();
            let buffer = terminal.backend().buffer();
            let drawn = (0..20u16)
                .filter(|&y| (0..width).any(|x| buffer[(x, y)].symbol() != " "))
                .count();
            assert_eq!(usize::from(height), drawn, "width {width}");
        }
    }
}
