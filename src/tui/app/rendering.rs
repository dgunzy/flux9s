//! Rendering logic for the application
//!
//! This module contains all rendering logic including the main render loop,
//! view-specific rendering, and layout calculations.

use super::core::App;
use super::state::{HealthFilter, View};
use crate::tui::keybindings::calculate_footer_height;
use crate::tui::views::{self, *};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::Line,
    widgets::{Block, Borders, Paragraph},
};

impl App {
    /// Main render entry point
    ///
    /// Renders the entire TUI interface based on current application state
    pub fn render(&mut self, f: &mut Frame) {
        // Show splash screen for 1.5 seconds, then auto-dismiss
        if self.ui_state.show_splash {
            if let Some(start_time) = self.ui_state.splash_start_time {
                let elapsed = start_time.elapsed();
                tracing::debug!(
                    "Splash render check: elapsed={:?}ms, show_splash={}",
                    elapsed.as_millis(),
                    self.ui_state.show_splash
                );
                use crate::tui::constants::SPLASH_DISPLAY_MS;
                if elapsed >= std::time::Duration::from_millis(SPLASH_DISPLAY_MS) {
                    tracing::debug!(
                        "Splash screen auto-dismissing after {:?}ms",
                        elapsed.as_millis()
                    );
                    self.ui_state.show_splash = false;
                    self.ui_state.splash_start_time = None;
                } else {
                    tracing::debug!(
                        "Rendering splash screen (elapsed: {:?}ms)",
                        elapsed.as_millis()
                    );
                    let chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Min(0)])
                        .split(f.size());
                    render_splash(f, chunks[0], &self.theme);
                    return;
                }
            } else {
                // Fallback: if start_time is None but show_splash is true, hide it
                tracing::warn!(
                    "Splash screen should show but splash_start_time is None - hiding splash"
                );
                self.ui_state.show_splash = false;
            }
        }

        let terminal_width = f.size().width;
        let terminal_height = f.size().height;
        let current_size = (terminal_width, terminal_height);

        // Only recalculate layout dimensions when terminal size changes
        // This prevents flickering/bouncing caused by per-frame recalculation
        let size_changed = self.ui_state.cached_terminal_size != Some(current_size);
        if size_changed {
            self.ui_state.cached_terminal_size = Some(current_size);

            // Calculate header height using EXACT same logic as header.rs
            // header.rs uses: left_area.width.saturating_sub(12) where left_area is 70% of total
            // We need to match this exactly to prevent mismatched wrapping calculations
            let header_left_width = {
                // Layout::split with Percentage(70) gives floor(width * 70 / 100)
                // but we need to account for potential rounding - use the same method
                let header_chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
                    .split(Rect::new(0, 0, terminal_width, 1));
                header_chunks[0].width
            };
            let available_width_for_resources = header_left_width.saturating_sub(12);

            // Header content lines:
            // 1. Context line (Context: xxx  Namespace: xxx)
            // 2. Flux9s | Total line
            // 3+ Resource type lines (variable based on wrapping)
            // +1 if filter is active (filter status line)
            // Plus 2 for borders
            let base_content_lines: u16 = 2; // Context line + Flux9s/Total line
            let filter_line: u16 = if !self.view_state.filter.is_empty()
                || self.view_state.selected_resource_type.is_some()
            {
                1
            } else {
                0
            };

            let resource_type_lines: u16 = {
                let counts = self.state().count_by_type();
                if counts.is_empty() {
                    1 // At least one line for "no resources"
                } else {
                    let mut lines: u16 = 1;
                    let mut current_len: usize = 11; // "Resources: " prefix

                    // Sort counts to match header.rs rendering order (alphabetical)
                    let mut type_counts: Vec<_> = counts.iter().collect();
                    type_counts.sort_by_key(|(resource_type, _)| *resource_type);

                    for (rt, count) in type_counts.iter() {
                        let part = format!("{}:{} ", rt, count);
                        // Match header.rs wrapping logic exactly (line 77-78)
                        if current_len + part.len() > available_width_for_resources as usize
                            && current_len > 11
                        {
                            lines += 1;
                            current_len = part.len();
                        } else {
                            current_len += part.len();
                        }
                    }
                    lines
                }
            };

            // Total content lines + 2 for borders
            let content_lines = base_content_lines + filter_line + resource_type_lines;
            // Minimum height for ASCII art + borders
            use crate::tui::constants::MIN_HEADER_HEIGHT;
            self.ui_state.cached_header_height = (content_lines + 2).max(MIN_HEADER_HEIGHT);

            // Calculate footer height using centralized function
            self.ui_state.cached_footer_height =
                calculate_footer_height(terminal_width, self.namespace_hotkeys(), self.namespace());
        }

        let header_height = self.ui_state.cached_header_height;
        let footer_constraint = self.ui_state.cached_footer_height;

        // Ensure we have minimum terminal size
        use crate::tui::constants::MIN_TERMINAL_WIDTH;
        let min_height = header_height + footer_constraint + 3; // header + footer + min content
        let min_width = MIN_TERMINAL_WIDTH;

        if terminal_height < min_height || terminal_width < min_width {
            // Terminal too small - show error
            let error_msg = format!(
                "Terminal too small! Need at least {}x{} (current: {}x{})",
                min_width, min_height, terminal_width, terminal_height
            );
            let error_lines = vec![
                Line::from(""),
                Line::from(error_msg),
                Line::from("Please resize your terminal window."),
            ];
            let error_block = Block::default().title("Error").borders(Borders::ALL);
            let error_para = Paragraph::new(error_lines).block(error_block);
            f.render_widget(error_para, f.size());
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                if self.config.ui.headless {
                    Constraint::Length(0) // No header in headless mode
                } else {
                    Constraint::Length(header_height) // Cached header height
                },
                Constraint::Min(0),                    // Main content (flexible)
                Constraint::Length(footer_constraint), // Cached footer height
            ])
            .split(f.size());

        let resources = self.get_filtered_resources();
        // Only render header if not in headless mode
        if !self.config.ui.headless {
            let health_percentage = self.calculate_health_percentage();
            let health_filter_status = match self.view_state.health_filter {
                HealthFilter::Healthy => Some("healthy"),
                HealthFilter::Unhealthy => Some("unhealthy"),
                HealthFilter::All => None,
            };
            render_header(
                f,
                chunks[0],
                &self.state,
                &self.controller_pods,
                &self.context,
                &self.namespace,
                &self.view_state.filter,
                &self.view_state.selected_resource_type,
                resources.len(),
                health_percentage,
                health_filter_status,
                self.config.read_only,
                &self.theme,
                self.config.ui.no_icons,
                self.namespace_hotkeys(),
            );
        }
        self.render_main(f, chunks[1]);
        render_footer(
            f,
            chunks[2],
            self.ui_state.command_mode,
            &self.ui_state.command_buffer,
            self.view_state.filter_mode,
            &self.view_state.filter,
            self.ui_state.show_help,
            &self.async_state.confirmation_pending,
            &self.ui_state.status_message,
            &self.operation_registry,
            &self.state,
            &self.theme,
            self.namespace_hotkeys(),
            &self.namespace,
        );
    }

    /// Calculate health percentage based on filtered resources
    /// This calculates health for resources matching the current name/resource type filters,
    /// but before applying the health filter itself.
    pub(crate) fn calculate_health_percentage(&self) -> f64 {
        let mut filtered_resources =
            if let Some(ref resource_type) = self.view_state.selected_resource_type {
                self.state.by_type(resource_type)
            } else {
                self.state.all()
            };

        // Apply namespace filter if set
        if let Some(namespace) = self.namespace() {
            filtered_resources.retain(|r| r.namespace == *namespace);
        }

        if !self.view_state.filter.is_empty() {
            if let Some(label_filter) = self.view_state.filter.strip_prefix("label:") {
                if label_filter.is_empty() {
                    filtered_resources.retain(|r| !r.labels.is_empty());
                } else if let Some((key, value)) = label_filter.split_once('=') {
                    filtered_resources.retain(|r| {
                        r.labels
                            .iter()
                            .any(|(k, v)| k.starts_with(key) && v.starts_with(value))
                    });
                } else {
                    filtered_resources
                        .retain(|r| r.labels.keys().any(|k| k.starts_with(label_filter)));
                }
            } else if let Some(ann_filter) = self
                .view_state
                .filter
                .strip_prefix("ann:")
                .or_else(|| self.view_state.filter.strip_prefix("annotations:"))
            {
                if ann_filter.is_empty() {
                    filtered_resources.retain(|r| !r.annotations.is_empty());
                } else if let Some((key, value)) = ann_filter.split_once('=') {
                    filtered_resources.retain(|r| {
                        r.annotations
                            .iter()
                            .any(|(k, v)| k.starts_with(key) && v.starts_with(value))
                    });
                } else {
                    filtered_resources
                        .retain(|r| r.annotations.keys().any(|k| k.starts_with(ann_filter)));
                }
            } else {
                filtered_resources.retain(|r| r.name.contains(&self.view_state.filter));
            }
        }

        if filtered_resources.is_empty() {
            return 100.0; // No resources = 100% healthy (nothing to be unhealthy)
        }

        let healthy_count = filtered_resources
            .iter()
            .filter(|r| {
                let is_ready = r.ready.unwrap_or(true); // null status treated as healthy
                let is_suspended = r.suspended.unwrap_or(false);
                is_ready && !is_suspended
            })
            .count();

        (healthy_count as f64 / filtered_resources.len() as f64) * 100.0
    }

    fn render_main(&mut self, f: &mut Frame, area: Rect) {
        if self.async_state.confirmation_pending.is_some() {
            if let Some(ref confirmation) = self.async_state.confirmation_pending {
                render_confirmation(
                    f,
                    area,
                    confirmation,
                    &self.operation_registry,
                    &self.state,
                    &self.theme,
                );
            }
            return;
        }

        if self.ui_state.show_help {
            render_help(f, area, &self.theme, self.namespace_hotkeys());
        } else if let Some(ref submenu) = self.view_state.submenu_state {
            // First render the current view in the background
            match self.view_state.current_view {
                View::ResourceList => {
                    let resources = self.get_filtered_resources();
                    render_resource_list(
                        f,
                        area,
                        &resources,
                        self.view_state.selected_index,
                        &mut self.view_state.scroll_offset,
                        &self.view_state.selected_resource_type,
                        &self.resource_objects,
                        &self.theme,
                        self.config.ui.no_icons,
                        &self.selection_state.favorites,
                    );
                }
                View::ResourceFavorites => {
                    let resources = self.get_filtered_resources();
                    render_resource_list(
                        f,
                        area,
                        &resources,
                        self.view_state.selected_index,
                        &mut self.view_state.scroll_offset,
                        &self.view_state.selected_resource_type,
                        &self.resource_objects,
                        &self.theme,
                        self.config.ui.no_icons,
                        &self.selection_state.favorites,
                    );
                }
                _ => {
                    // For other views, just show empty background
                }
            }
            // Then render submenu overlay on top
            render_submenu(f, area, submenu, &self.theme);
        } else {
            match self.view_state.current_view {
                View::ResourceList => {
                    let resources = self.get_filtered_resources();
                    render_resource_list(
                        f,
                        area,
                        &resources,
                        self.view_state.selected_index,
                        &mut self.view_state.scroll_offset,
                        &self.view_state.selected_resource_type,
                        &self.resource_objects,
                        &self.theme,
                        self.config.ui.no_icons,
                        &self.selection_state.favorites,
                    );
                }
                View::ResourceDetail => {
                    render_resource_detail(
                        f,
                        area,
                        &self.selection_state.selected_resource_key,
                        &self.state,
                        &self.resource_objects,
                        &self.theme,
                    );
                }
                View::ResourceYAML => {
                    render_resource_yaml(
                        f,
                        area,
                        &self.selection_state.selected_resource_key,
                        &self.state,
                        &self.resource_objects,
                        &self.async_state.yaml_fetched,
                        &self.async_state.yaml_fetch_pending,
                        &mut self.view_state.yaml_scroll_offset,
                        &self.theme,
                    );
                }
                View::ResourceTrace => {
                    views::trace::render_resource_trace(
                        f,
                        area,
                        &self.selection_state.selected_resource_key,
                        &self.async_state.trace_result,
                        &self.async_state.trace_pending,
                        &mut self.view_state.trace_scroll_offset,
                        &self.theme,
                    );
                }
                View::ResourceFavorites => {
                    let resources = self.get_filtered_resources();
                    render_resource_list(
                        f,
                        area,
                        &resources,
                        self.view_state.selected_index,
                        &mut self.view_state.scroll_offset,
                        &self.view_state.selected_resource_type,
                        &self.resource_objects,
                        &self.theme,
                        self.config.ui.no_icons,
                        &self.selection_state.favorites,
                    );
                }
                View::ResourceGraph => {
                    views::render_resource_graph(
                        f,
                        area,
                        &self.selection_state.selected_resource_key,
                        &self.async_state.graph_result,
                        &self.async_state.graph_pending,
                        &mut self.view_state.graph_scroll_offset,
                        &self.theme,
                    );
                }
                View::ResourceHistory => {
                    if let Some(key) = &self.selection_state.selected_resource_key {
                        if let Some(resource) = self.state.get(key) {
                            if render_reconciliation_history(
                                f,
                                area,
                                &resource,
                                &self.resource_objects,
                                &mut self.view_state.history_scroll_offset,
                                &self.theme,
                            )
                            .is_err()
                            {
                                // Error already rendered in the function
                            }
                        } else {
                            let text = vec![
                                ratatui::text::Line::from("Resource not found"),
                                ratatui::text::Line::from(""),
                                ratatui::text::Line::from("Press Esc to go back"),
                            ];
                            let paragraph = Paragraph::new(text)
                                .style(Style::default().fg(self.theme.text_secondary));
                            f.render_widget(paragraph, area);
                        }
                    } else {
                        let text = vec![
                            ratatui::text::Line::from("No resource selected"),
                            ratatui::text::Line::from(""),
                            ratatui::text::Line::from(
                                "Select a resource and press 'h' to view history",
                            ),
                        ];
                        let paragraph = Paragraph::new(text)
                            .style(Style::default().fg(self.theme.text_secondary));
                        f.render_widget(paragraph, area);
                    }
                }
                View::Help => {
                    render_help(f, area, &self.theme, self.namespace_hotkeys());
                }
            }
        }
    }
}
