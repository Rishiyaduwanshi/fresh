use super::*;
use ratatui::layout::{Constraint, Direction, Layout as UiLayout};
use ratatui::Frame;

impl Editor {
    /// Convert an action into events using the view-centric pipeline for the active split.
    pub fn action_to_events(&mut self, action: Action) -> Option<Vec<Event>> {
        let split_id = self.split_manager.active_split();
        if let (Some(view_state), Some(buffer_state)) = (
            self.split_view_states.get_mut(&split_id),
            self.buffers.get_mut(&self.active_buffer),
        ) {
            // Sync view state to buffer state.
            view_state.cursors = buffer_state.cursors.clone();
            view_state.viewport.width = buffer_state.viewport.width;
            view_state.viewport.height = buffer_state.viewport.height;

            // Ensure layout.
            let gutter_width = view_state.viewport.gutter_width(&buffer_state.buffer);
            let wrap_params = Some((view_state.viewport.width as usize, gutter_width));
            let mut layout = view_state
                .ensure_layout(
                    &mut buffer_state.buffer,
                    self.config.editor.estimated_line_length,
                    wrap_params,
                )
                .clone();

            // If cursor is outside layout's source_range (e.g., jumped to EOF),
            // we need to rebuild layout centered on the cursor first.
            let primary_byte = view_state
                .cursors
                .primary()
                .position
                .source_byte
                .unwrap_or(0);
            let estimated_line_length = self.config.editor.estimated_line_length;
            if !layout.source_range.contains(&primary_byte)
                && primary_byte != layout.source_range.end
            {
                // Cursor is outside layout range - rebuild from cursor position
                view_state.viewport.anchor_byte = primary_byte.saturating_sub(
                    estimated_line_length * view_state.viewport.visible_line_count() / 2,
                );
                view_state.invalidate_layout();
                layout = view_state
                    .ensure_layout(
                        &mut buffer_state.buffer,
                        estimated_line_length,
                        wrap_params,
                    )
                    .clone();
            }

            // Helper to sync cursor view positions from source_byte using the layout.
            fn sync_cursors_to_layout(
                cursors: &mut crate::cursor::Cursors,
                layout: &crate::ui::view_pipeline::Layout,
            ) {
                for cursor_id in cursors.iter().map(|(id, _)| id).collect::<Vec<_>>() {
                    if let Some(cursor) = cursors.get_mut(cursor_id) {
                        if let Some(byte) = cursor.position.source_byte {
                            if let Some((view_line, column)) =
                                layout.source_byte_to_view_position(byte)
                            {
                                cursor.position.view_line = Some(view_line);
                                cursor.position.column = Some(column);
                            }
                        }
                        // Also sync anchor if present
                        if let Some(ref mut anchor) = cursor.anchor {
                            if let Some(byte) = anchor.source_byte {
                                if let Some((view_line, column)) =
                                    layout.source_byte_to_view_position(byte)
                                {
                                    anchor.view_line = Some(view_line);
                                    anchor.column = Some(column);
                                }
                            }
                        }
                    }
                }
            }

            // Sync cursor view positions from source_byte using the layout.
            // This is necessary because after edits, source_byte is updated but view_line/column
            // may be stale. Navigation actions depend on correct view_line.
            sync_cursors_to_layout(&mut view_state.cursors, &layout);

            // Check if cursor is at top of layout but layout doesn't start at byte 0.
            // This means we need to expand backwards for upward navigation to work.
            // IMPORTANT: Also verify cursor.source_byte is actually near the start of buffer,
            // not just that view_line == 0 (which could be stale after failed sync).
            let cursor_view_line = view_state.cursors.primary().position.view_line;
            let cursor_source_byte = view_state
                .cursors
                .primary()
                .position
                .source_byte
                .unwrap_or(0);
            let cursor_near_layout_start =
                cursor_source_byte <= layout.source_range.start.saturating_add(estimated_line_length);
            if cursor_view_line == Some(0) && layout.source_range.start > 0 && cursor_near_layout_start {
                // Expand layout backwards from current anchor
                let backtrack_bytes =
                    estimated_line_length * view_state.viewport.visible_line_count();
                view_state.viewport.anchor_byte = layout
                    .source_range
                    .start
                    .saturating_sub(backtrack_bytes);
                view_state.invalidate_layout();
                layout = view_state
                    .ensure_layout(
                        &mut buffer_state.buffer,
                        estimated_line_length,
                        wrap_params,
                    )
                    .clone();
                // Re-sync cursor position after layout expansion
                sync_cursors_to_layout(&mut view_state.cursors, &layout);
            }

            // Convert action.
            let events = crate::navigation::action_convert::action_to_events(
                &mut view_state.cursors,
                &layout,
                &mut view_state.viewport,
                &buffer_state.buffer,
                action,
            );

            // Sync back.
            buffer_state.viewport = view_state.viewport.clone();
            buffer_state.cursors = view_state.cursors.clone();

            return events;
        }
        None
    }

    /// Render the editor to the terminal (simplified view-centric).
    pub fn render(&mut self, frame: &mut Frame) {
        let size = frame.area();

        // Prepare buffers.
        for (_, state) in &mut self.buffers {
            let _ = state.prepare_for_render();
        }

        // Layout: menu bar (1), main content, status bar (1), prompt (1).
        let constraints = vec![
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ];
        let main_chunks = UiLayout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(size);

        let menu_bar_area = main_chunks[0];
        let main_content_area = main_chunks[1];
        let status_bar_idx = 2;
        let prompt_line_idx = 3;

        // Render splits.
        let split_areas = crate::ui::split_rendering::SplitRenderer::render_content(
            frame,
            main_content_area,
            &self.split_manager,
            &mut self.buffers,
            &self.buffer_metadata,
            &mut self.event_logs,
            &self.theme,
            self.ansi_background.as_ref(),
            self.background_fade,
            false,
            self.config.editor.large_file_threshold_bytes,
            self.config.editor.estimated_line_length,
            Some(&mut self.split_view_states),
            self.menu_state.active_menu.is_some(),
        );
        self.cached_layout.split_areas = split_areas;
        self.cached_layout.editor_content_area = Some(main_content_area);

        // Status bar.
        if let Some(state) = self.buffers.get(&self.active_buffer) {
            crate::ui::status_bar::StatusBarRenderer::render_status_bar(
                frame,
                main_chunks[status_bar_idx],
                state,
                self.split_view_states
                    .get(&self.split_manager.active_split())
                    .and_then(|vs| vs.get_layout())
                    .or_else(|| self.buffers.get(&self.active_buffer).and_then(|s| None)), // placeholder
                &self.status_message,
                &self.plugin_status_message,
                "",
                &self.theme,
                &self
                    .buffer_metadata
                    .get(&self.active_buffer)
                    .map(|m| m.display_name.clone())
                    .unwrap_or_default(),
                &self.keybindings,
                &self.chord_state,
            );
        }

        // Menu bar.
        let checkbox_states = crate::ui::menu::CheckboxStates {
            line_numbers: self
                .buffers
                .get(&self.active_buffer)
                .map(|s| s.margins.show_line_numbers)
                .unwrap_or(true),
            line_wrap: self
                .buffers
                .get(&self.active_buffer)
                .map(|s| s.viewport.line_wrap_enabled)
                .unwrap_or(false),
            compose_mode: self
                .buffers
                .get(&self.active_buffer)
                .map(|s| s.compose_width.is_some())
                .unwrap_or(false),
            file_explorer: self.file_explorer_visible,
            mouse_capture: self.mouse_enabled,
        };

        let selection_active = self
            .buffers
            .get(&self.active_buffer)
            .map(|s| s.cursors.primary().anchor.is_some())
            .unwrap_or(false);

        crate::ui::menu::MenuRenderer::render(
            frame,
            menu_bar_area,
            &self.config.menu,
            &self.menu_state,
            &self.keybindings,
            &self.theme,
            self.mouse_state.hover_target.as_ref(),
            selection_active,
            &checkbox_states,
        );

        // Prompt line.
        if let Some(prompt) = &self.prompt {
            crate::ui::status_bar::StatusBarRenderer::render_prompt(
                frame,
                main_chunks[prompt_line_idx],
                prompt,
                &self.theme,
            );

            // Suggestions popup (for command palette, autocomplete, etc.)
            if !prompt.suggestions.is_empty() {
                // Position suggestions popup above the prompt line, full width, left-aligned
                let popup_height = (prompt.suggestions.len() + 2).min(15) as u16;
                let popup_width = size.width;
                let popup_x = 0;
                let popup_y = main_chunks[prompt_line_idx]
                    .y
                    .saturating_sub(popup_height);

                let suggestions_area = ratatui::layout::Rect {
                    x: popup_x,
                    y: popup_y,
                    width: popup_width,
                    height: popup_height,
                };

                crate::ui::suggestions::SuggestionsRenderer::render_with_hover(
                    frame,
                    suggestions_area,
                    prompt,
                    &self.theme,
                    self.mouse_state.hover_target.as_ref(),
                );
            }
        }
    }

    /// Add an overlay to the active buffer and return a handle for later removal.
    pub fn add_overlay_with_handle(
        &mut self,
        namespace: Option<crate::overlay::OverlayNamespace>,
        range: std::ops::Range<usize>,
        face: crate::event::OverlayFace,
        priority: i32,
        message: Option<String>,
    ) -> crate::overlay::OverlayHandle {
        let event = Event::AddOverlay {
            namespace,
            range,
            face,
            priority,
            message,
        };
        self.apply_event_to_active_buffer(&event);
        // Return the handle of the last added overlay
        let state = self.active_state();
        state
            .overlays
            .all()
            .last()
            .map(|o| o.handle.clone())
            .unwrap_or_else(crate::overlay::OverlayHandle::new)
    }

    /// Remove an overlay by handle.
    pub fn remove_overlay_by_handle(&mut self, handle: crate::overlay::OverlayHandle) {
        let event = Event::RemoveOverlay { handle };
        self.apply_event_to_active_buffer(&event);
    }
}
