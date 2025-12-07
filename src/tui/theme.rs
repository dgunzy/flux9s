//! Theme and styling definitions
//!
//! This module provides a centralized place for all color and style definitions.
//! It's designed to be extensible - in the future, themes can be loaded from
//! plugin files or configuration.

use ratatui::style::{Color, Modifier, Style};

/// Theme configuration for the TUI
///
/// This struct holds all color and style definitions. In the future, this can
/// be loaded from a plugin file or configuration.
pub struct Theme {
    // Header colors
    pub header_context: Color,
    pub header_namespace: Color,
    pub header_namespace_all: Color,
    pub header_total: Color,
    pub header_resources: Color,
    pub header_filter: Color,
    pub header_ascii: Color,

    // Status colors
    pub status_ready: Color,
    pub status_suspended: Color,
    pub status_error: Color,
    pub status_unknown: Color,
    pub status_pending: Color,

    // Table colors
    pub table_header: Color,
    pub table_selected: Color,
    pub table_selected_bg: Color, // Background color for selected row
    pub table_normal: Color,

    // Text colors
    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_label: Color,
    pub text_value: Color,

    // Command/Input colors
    pub command_prompt: Color,
    pub command_autocomplete: Color,
    pub filter_prompt: Color,

    // Operation colors
    pub operation_success: Color,
    pub operation_error: Color,
    pub operation_warning: Color,
    pub operation_confirm: Color,
    pub operation_cancel: Color,

    // Footer colors
    pub footer_key: Color,
    #[allow(dead_code)] // Reserved for visual flux trace feature
    pub footer_text: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            // Header colors
            header_context: Color::Yellow,
            header_namespace: Color::Yellow,
            header_namespace_all: Color::Green,
            header_total: Color::Yellow,
            header_resources: Color::Cyan,
            header_filter: Color::Magenta,
            header_ascii: Color::Cyan,

            // Status colors
            status_ready: Color::Green,
            status_suspended: Color::Gray,
            status_error: Color::Red,
            status_unknown: Color::Yellow,
            status_pending: Color::Yellow,

            // Table colors
            table_header: Color::Cyan,
            table_selected: Color::Blue,
            table_selected_bg: Color::DarkGray, // Dark gray background for selected row
            table_normal: Color::White,

            // Text colors
            text_primary: Color::White,
            text_secondary: Color::Gray,
            text_label: Color::Cyan,
            text_value: Color::White,

            // Command/Input colors
            command_prompt: Color::Yellow,
            command_autocomplete: Color::Gray,
            filter_prompt: Color::Yellow,

            // Operation colors
            operation_success: Color::Green,
            operation_error: Color::Red,
            operation_warning: Color::Yellow,
            operation_confirm: Color::Green,
            operation_cancel: Color::Red,

            // Footer colors
            footer_key: Color::Yellow,
            footer_text: Color::White,
        }
    }
}

impl Theme {
    // Helper methods for common style combinations

    pub fn header_context_style(&self) -> Style {
        Style::default()
            .fg(self.header_context)
            .add_modifier(Modifier::BOLD)
    }

    pub fn header_namespace_style(&self, is_all: bool) -> Style {
        Style::default()
            .fg(if is_all {
                self.header_namespace_all
            } else {
                self.header_namespace
            })
            .add_modifier(Modifier::BOLD)
    }

    pub fn status_ready_style(&self) -> Style {
        Style::default().fg(self.status_ready)
    }

    pub fn status_error_style(&self) -> Style {
        Style::default().fg(self.status_error)
    }

    pub fn status_suspended_style(&self) -> Style {
        Style::default().fg(self.status_suspended)
    }

    pub fn table_selected_style(&self) -> Style {
        Style::default()
            .fg(self.table_selected)
            .bg(self.table_selected_bg)
    }

    pub fn footer_key_style(&self) -> Style {
        Style::default().fg(self.footer_key)
    }

    pub fn operation_success_style(&self) -> Style {
        Style::default()
            .fg(self.operation_success)
            .add_modifier(Modifier::BOLD)
    }

    pub fn operation_error_style(&self) -> Style {
        Style::default()
            .fg(self.operation_error)
            .add_modifier(Modifier::BOLD)
    }

    pub fn operation_warning_style(&self) -> Style {
        Style::default()
            .fg(self.operation_warning)
            .add_modifier(Modifier::BOLD)
    }
}
