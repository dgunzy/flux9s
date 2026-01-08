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
    /// Currently selected index
    pub selected_index: usize,
    /// Scroll offset for rendering
    pub scroll_offset: usize,
    /// Optional title for the submenu
    pub title: Option<String>,
    /// Optional help text to show in the submenu
    pub help_text: Option<String>,
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

    /// Move selection down
    pub fn move_down(&mut self) {
        if self.selected_index < self.items.len().saturating_sub(1) {
            self.selected_index += 1;
        }
    }

    /// Move selection up
    pub fn move_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    /// Get the currently selected item
    pub fn selected_item(&self) -> Option<&SubmenuItem> {
        self.items.get(self.selected_index)
    }

    /// Get the value of the currently selected item
    pub fn selected_value(&self) -> Option<String> {
        self.selected_item().map(|item| item.value.clone())
    }

    /// Update scroll offset to ensure selected item is visible
    pub fn update_scroll(&mut self, visible_height: usize) {
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
