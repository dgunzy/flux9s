//! Scrolling, one way for every view.
//!
//! A view that scrolls builds a [`Viewport`] from its offset and content
//! size, draws only [`Viewport::range`], and calls
//! [`Viewport::render_scrollbar`] on its bordered area. Two kinds cover every
//! view:
//!
//! - [`Viewport::text`] — free scrolling by line (YAML, describe, logs, …):
//!   the offset is clamped so the last line can reach the bottom and no
//!   further, so key handlers can overshoot (`G` sets `usize::MAX`).
//! - [`Viewport::list`] — a selected row (resource lists, kind lists, …): the
//!   offset follows the selection, keeping a margin of rows around it.
//!
//! The scrollbar is a full-block thumb drawn over the block's right border
//! (no track, no arrows): a whole cell wide, so it stands out from the thin
//! border line. It is sized to the visible share, placed at the current
//! position, and not drawn when everything fits. Key handling lives on the app side
//! (`View::scroll_model`, `App::scroll_down` / `scroll_up`).

use crate::tui::theme::Theme;
use ratatui::{
    Frame,
    layout::{Margin, Rect},
    widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState},
};
use std::ops::Range;

/// Rows kept visible above/below the selection in lists.
pub const LIST_MARGIN: usize = 2;

/// The visible window onto scrollable content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    offset: usize,
    visible: usize,
    total: usize,
}

impl Viewport {
    /// A line-scrolled view. Clamps `offset` in place.
    pub fn text(offset: &mut usize, total: usize, visible: usize) -> Self {
        *offset = (*offset).min(total.saturating_sub(visible));
        Self {
            offset: *offset,
            visible,
            total,
        }
    }

    /// A list with a selected row: moves `offset` so the selection stays on
    /// screen with [`LIST_MARGIN`] rows around it, then clamps.
    pub fn list(selected: usize, offset: &mut usize, total: usize, visible: usize) -> Self {
        Self::list_with_margin(selected, offset, total, visible, LIST_MARGIN)
    }

    /// [`Self::list`] with a custom margin.
    pub fn list_with_margin(
        selected: usize,
        offset: &mut usize,
        total: usize,
        visible: usize,
        margin: usize,
    ) -> Self {
        crate::tui::views::helpers::update_scroll_offset(selected, visible, offset, margin);
        Self::text(offset, total, visible)
    }

    /// First visible item.
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Items that fit on screen.
    pub fn visible(&self) -> usize {
        self.visible
    }

    /// Indices of the items to draw.
    pub fn range(&self) -> Range<usize> {
        self.offset..(self.offset + self.visible).min(self.total)
    }

    /// Whether the content overflows (the scrollbar shows).
    pub fn scrollable(&self) -> bool {
        self.total > self.visible
    }

    /// Draw the scroll thumb over the right border of `area` (the area the
    /// view's bordered block was rendered into). No-op when it all fits.
    pub fn render_scrollbar(&self, f: &mut Frame, area: Rect, theme: &Theme) {
        if !self.scrollable() || area.height < 3 {
            return;
        }
        let max_offset = self.total.saturating_sub(self.visible);
        // Thumb = visible / total of the track, placed at offset / total.
        let mut state = ScrollbarState::new(max_offset + 1)
            .position(self.offset)
            .viewport_content_length(self.visible);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(None)
            .thumb_symbol("█")
            .thumb_style(theme.scrollbar_style());
        // Between the corners, on the border column.
        let track = area.inner(Margin {
            vertical: 1,
            horizontal: 0,
        });
        f.render_stateful_widget(scrollbar, track, &mut state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend, widgets::Block};

    #[test]
    fn text_viewports_clamp_to_the_last_page() {
        let mut offset = usize::MAX;
        let viewport = Viewport::text(&mut offset, 100, 10);
        assert_eq!(offset, 90);
        assert_eq!(viewport.range(), 90..100);
        let mut offset = 5;
        let short = Viewport::text(&mut offset, 3, 10);
        assert_eq!(offset, 0);
        assert_eq!(short.range(), 0..3);
        assert!(!short.scrollable());
    }

    #[test]
    fn list_viewports_follow_the_selection() {
        let mut offset = 0;
        let viewport = Viewport::list(50, &mut offset, 100, 10);
        assert!(viewport.range().contains(&50));
        assert!(50 < viewport.range().end - 1, "keeps a margin below");
        let mut offset = 40;
        let viewport = Viewport::list(0, &mut offset, 100, 10);
        assert_eq!(viewport.range(), 0..10);
    }

    fn right_border(viewport: Viewport, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(10, height)).unwrap();
        terminal
            .draw(|f| {
                let area = f.area();
                f.render_widget(Block::bordered(), area);
                viewport.render_scrollbar(f, area, &Theme::default());
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| buffer[(9, y)].symbol().to_string())
            .collect()
    }

    #[test]
    fn scrollbar_thumb_sits_on_the_border_where_you_are() {
        // 12 rows: border, 10-row track, border. A quarter visible.
        let top = right_border(
            Viewport {
                offset: 0,
                visible: 10,
                total: 40,
            },
            12,
        );
        assert_eq!(top[0], "┐", "corners untouched");
        assert_eq!(top[11], "┘");
        let thumb: Vec<usize> = (1..11).filter(|&y| top[y] == "█").collect();
        assert_eq!(thumb.first(), Some(&1), "at the top");
        assert!((2..=3).contains(&thumb.len()), "≈ a quarter: {thumb:?}");
        assert!(top[10] == "│", "the rest of the border stays");

        let bottom = right_border(
            Viewport {
                offset: 30,
                visible: 10,
                total: 40,
            },
            12,
        );
        assert_eq!(bottom[10], "█", "at the bottom");
        assert_eq!(bottom[1], "│");
    }

    #[test]
    fn no_scrollbar_when_everything_fits() {
        let border = right_border(
            Viewport {
                offset: 0,
                visible: 10,
                total: 10,
            },
            12,
        );
        assert!(border[1..11].iter().all(|s| s == "│"));
    }
}
