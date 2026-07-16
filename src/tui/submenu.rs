/// Submenu system for commands that require user selection from a list of options.
///
/// This module provides a trait-based approach for commands to implement interactive
/// submenus. When a command is executed without arguments, it can optionally present
/// a submenu overlay where users can navigate and select from available options.
use anyhow::Result;

/// Represents a single item in a submenu
#[derive(Debug, Clone)]
pub struct SubmenuItem {
    /// The text to display in the submenu
    pub display_text: String,
    /// The value to use when this item is selected
    pub value: String,
    /// Optional description or additional info
    pub description: Option<String>,
}

impl SubmenuItem {
    /// Create a new submenu item with custom display text
    pub fn with_display(value: String, display_text: String) -> Self {
        Self {
            display_text,
            value,
            description: None,
        }
    }
}

/// State for managing submenu interaction
#[derive(Debug, Clone)]
pub struct SubmenuState {
    /// The command that opened this submenu
    pub command: String,
    /// All available items in the submenu
    pub items: Vec<SubmenuItem>,
    /// Currently selected index **within the filtered items**
    pub selected_index: usize,
    /// Scroll offset for rendering, kept in sync with the selection at render
    /// time via [`Self::clamp_scroll`] (the renderer knows the real height)
    pub scroll_offset: usize,
    /// Optional title for the submenu
    pub title: Option<String>,
    /// Optional help text to show in the submenu
    pub help_text: Option<String>,
    /// Filter narrowing the items (case-insensitive substring on display
    /// text), edited while `filter_mode` is active (#128)
    pub filter: String,
    /// Whether the user is typing the filter — entered with `/`, applied
    /// with Enter, cancelled with Esc, mirroring the resource-list filter.
    pub filter_mode: bool,
}

impl SubmenuState {
    /// Create a new submenu state
    pub fn new(command: String, items: Vec<SubmenuItem>) -> Self {
        Self {
            command,
            items,
            selected_index: 0,
            scroll_offset: 0,
            title: None,
            help_text: None,
            filter: String::new(),
            filter_mode: false,
        }
    }

    /// Create a submenu state with a title
    pub fn with_title(mut self, title: String) -> Self {
        self.title = Some(title);
        self
    }

    /// Create a submenu state with help text
    pub fn with_help(mut self, help: String) -> Self {
        self.help_text = Some(help);
        self
    }

    /// Items matching the type-ahead filter, in original order. An empty
    /// filter matches everything.
    pub fn filtered_items(&self) -> Vec<&SubmenuItem> {
        if self.filter.is_empty() {
            return self.items.iter().collect();
        }
        let filter = self.filter.to_lowercase();
        self.items
            .iter()
            .filter(|item| item.display_text.to_lowercase().contains(&filter))
            .collect()
    }

    /// Number of items currently visible through the filter.
    fn filtered_len(&self) -> usize {
        self.filtered_items().len()
    }

    /// Move selection down by `amount`, clamped to the filtered items.
    pub fn move_down(&mut self, amount: usize) {
        self.selected_index =
            (self.selected_index + amount).min(self.filtered_len().saturating_sub(1));
    }

    /// Move selection up by `amount`.
    pub fn move_up(&mut self, amount: usize) {
        self.selected_index = self.selected_index.saturating_sub(amount);
    }

    /// Append a character to the type-ahead filter and reset the selection.
    pub fn push_filter_char(&mut self, c: char) {
        self.filter.push(c);
        self.selected_index = 0;
        self.scroll_offset = 0;
    }

    /// Remove the last filter character. Returns false if there was nothing
    /// to remove (so callers can fall through to other Backspace behavior).
    pub fn pop_filter_char(&mut self) -> bool {
        if self.filter.pop().is_some() {
            self.selected_index = 0;
            self.scroll_offset = 0;
            true
        } else {
            false
        }
    }

    /// Clear the filter (and leave filter mode). Returns false if the filter
    /// was already empty (so Esc can fall through to closing the submenu).
    pub fn clear_filter(&mut self) -> bool {
        self.filter_mode = false;
        if self.filter.is_empty() {
            false
        } else {
            self.filter.clear();
            self.selected_index = 0;
            self.scroll_offset = 0;
            true
        }
    }

    /// Get the currently selected item (within the filtered items)
    pub fn selected_item(&self) -> Option<&SubmenuItem> {
        self.filtered_items().get(self.selected_index).copied()
    }

    /// Get the value of the currently selected item
    pub fn selected_value(&self) -> Option<String> {
        self.selected_item().map(|item| item.value.clone())
    }

    /// Keep the selection visible for the renderer's **actual** list height.
    /// Called at render time — the event handler doesn't know the popup
    /// geometry, and guessing it is how selections used to walk off-screen.
    pub fn clamp_scroll(&mut self, visible_height: usize) {
        if visible_height == 0 {
            return;
        }
        self.selected_index = self
            .selected_index
            .min(self.filtered_len().saturating_sub(1));
        if self.selected_index < self.scroll_offset {
            self.scroll_offset = self.selected_index;
        } else if self.selected_index >= self.scroll_offset + visible_height {
            self.scroll_offset = self.selected_index.saturating_sub(visible_height - 1);
        }
    }
}

/// Trait for commands that can provide a submenu
///
/// Commands implementing this trait will show a submenu when executed without
/// arguments, allowing users to navigate and select from available options.
pub trait CommandSubmenu {
    /// Get the submenu items for this command
    ///
    /// Returns `Ok(Some(SubmenuState))` if a submenu should be shown,
    /// `Ok(None)` if no submenu is available, or `Err` if there was an error
    /// getting the submenu data.
    fn get_submenu(&self) -> Result<Option<SubmenuState>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu(names: &[&str]) -> SubmenuState {
        let items = names
            .iter()
            .map(|n| SubmenuItem::with_display(n.to_string(), n.to_string()))
            .collect();
        SubmenuState::new("ctx".to_string(), items)
    }

    /// Regression: the selection used to walk below the visible window
    /// because the event handler guessed the popup height (hardcoded 20).
    /// clamp_scroll runs at render with the real height and must always keep
    /// the selection inside [scroll, scroll + height).
    #[test]
    fn clamp_scroll_keeps_selection_visible_at_any_height() {
        let names: Vec<String> = (0..40).map(|i| format!("cluster-{i:02}")).collect();
        let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut menu = menu(&name_refs);

        for height in [3usize, 8, 20] {
            for _ in 0..40 {
                menu.move_down(1);
                menu.clamp_scroll(height);
                assert!(
                    menu.selected_index >= menu.scroll_offset
                        && menu.selected_index < menu.scroll_offset + height,
                    "selection {} outside window [{}, {}) at height {}",
                    menu.selected_index,
                    menu.scroll_offset,
                    menu.scroll_offset + height,
                    height
                );
            }
            // Walk back up
            for _ in 0..40 {
                menu.move_up(1);
                menu.clamp_scroll(height);
                assert!(menu.selected_index >= menu.scroll_offset);
            }
        }
    }

    #[test]
    fn type_ahead_filters_and_selection_follows() {
        let mut menu = menu(&["prod-east", "prod-west", "staging", "kind-dev"]);
        menu.move_down(3); // select kind-dev

        menu.push_filter_char('p');
        assert_eq!(menu.selected_index, 0, "filter change resets selection");
        let filtered: Vec<&str> = menu
            .filtered_items()
            .iter()
            .map(|i| i.display_text.as_str())
            .collect();
        assert_eq!(filtered, ["prod-east", "prod-west"]);

        // Matching is a case-insensitive substring on display text
        menu.clear_filter();
        menu.push_filter_char('G');
        assert_eq!(menu.filtered_items().len(), 1);
        assert_eq!(menu.selected_value().as_deref(), Some("staging"));

        // Backspace widens; popping past empty reports false
        assert!(menu.pop_filter_char());
        assert_eq!(menu.filtered_items().len(), 4);
        assert!(!menu.pop_filter_char());
        assert!(!menu.clear_filter(), "already-empty filter reports false");
    }

    #[test]
    fn navigation_is_bounded_by_the_filtered_list() {
        let mut menu = menu(&["prod-east", "prod-west", "staging"]);
        menu.push_filter_char('p'); // 2 items visible
        menu.move_down(10); // page jump past the end
        assert_eq!(menu.selected_index, 1, "clamped to last filtered item");
        assert_eq!(menu.selected_value().as_deref(), Some("prod-west"));

        menu.move_up(10);
        assert_eq!(menu.selected_index, 0);
    }

    #[test]
    fn no_filter_match_yields_no_selection() {
        let mut menu = menu(&["prod-east"]);
        menu.push_filter_char('z');
        assert!(menu.filtered_items().is_empty());
        assert!(menu.selected_value().is_none());
        // clamp_scroll must not panic on an empty filtered list
        menu.clamp_scroll(5);
    }
}
