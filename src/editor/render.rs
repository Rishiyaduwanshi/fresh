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
            let layout = view_state
                .ensure_layout(
                    &mut buffer_state.buffer,
                    self.config.editor.estimated_line_length,
                    wrap_params,
                )
                .clone();

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

        // Prompt line.
        if let Some(prompt) = &self.prompt {
            crate::ui::status_bar::StatusBarRenderer::render_prompt(
                frame,
                main_chunks[prompt_line_idx],
                prompt,
                &self.theme,
            );
        }
    }
}
