use super::*;
use crate::cursor::ViewPosition;
use crate::event::SplitDirection;
use crate::hooks::HookArgs;
use crate::keybindings::Action;
use crate::prompt::PromptType;

impl Editor {
    /// Determine the current keybinding context based on UI state.
    pub(super) fn get_key_context(&self) -> crate::keybindings::KeyContext {
        use crate::keybindings::KeyContext;

        if self.menu_state.active_menu.is_some() {
            KeyContext::Menu
        } else if self.is_prompting() {
            KeyContext::Prompt
        } else if self.active_state().popups.is_visible() {
            KeyContext::Popup
        } else {
            self.key_context
        }
    }

    /// Handle a key event (view-centric rewrite).
    pub fn handle_key(
        &mut self,
        code: crossterm::event::KeyCode,
        modifiers: crossterm::event::KeyModifiers,
    ) -> std::io::Result<()> {
        let key_event = crossterm::event::KeyEvent::new(code, modifiers);

        // Resolve context and handle chorded bindings first.
        let mut context = self.get_key_context();

        // Dismiss hover/signature popups on any key press.
        if matches!(context, crate::keybindings::KeyContext::Popup) {
            let is_dismissable = self
                .active_state()
                .popups
                .top()
                .and_then(|p| p.title.as_ref())
                .is_some_and(|title| title == "Hover" || title == "Signature Help");
            if is_dismissable {
                self.hide_popup();
                context = self.get_key_context();
            }
        }

        // Mode keybindings (virtual buffers) when in normal/file-explorer.
        if matches!(
            context,
            crate::keybindings::KeyContext::Normal | crate::keybindings::KeyContext::FileExplorer
        ) {
            if let Some(command_name) = self.resolve_mode_keybinding(code, modifiers) {
                let commands = self.command_registry.read().unwrap().get_all();
                if let Some(cmd) = commands.iter().find(|c| c.name == command_name) {
                    let action = cmd.action.clone();
                    drop(commands);
                    return self.handle_action(action);
                } else if command_name == "close-buffer" {
                    let buffer_id = self.active_buffer;
                    return self.close_buffer(buffer_id);
                } else if command_name == "revert-buffer" {
                    self.set_status_message("Refreshing buffer...".to_string());
                    return Ok(());
                } else {
                    let action = Action::PluginAction(command_name.clone());
                    drop(commands);
                    return self.handle_action(action);
                }
            }
        }

        // Chord resolution.
        match self
            .keybindings
            .resolve_chord(&self.chord_state, &key_event, context)
        {
            crate::keybindings::ChordResolution::Complete(action) => {
                self.chord_state.clear();
                return self.handle_action(action);
            }
            crate::keybindings::ChordResolution::Partial => {
                self.chord_state.push((code, modifiers));
                return Ok(());
            }
            crate::keybindings::ChordResolution::NoMatch => {
                if !self.chord_state.is_empty() {
                    self.chord_state.clear();
                }
            }
        }

        // Single-key resolution.
        let action = self.keybindings.resolve(&key_event, context);

        // Cancel pending LSP requests on user actions (except LSP actions).
        match action {
            Action::LspCompletion
            | Action::LspGotoDefinition
            | Action::LspReferences
            | Action::LspHover
            | Action::None => {}
            _ => self.cancel_pending_lsp_requests(),
        }

        // Handle prompts explicitly.
        if matches!(context, crate::keybindings::KeyContext::Prompt) {
            return self.handle_prompt_action(action);
        }

        self.handle_action(action)
    }

    /// Handle prompt-specific actions (view-agnostic).
    fn handle_prompt_action(&mut self, action: Action) -> std::io::Result<()> {
        match action {
            Action::PromptConfirm => self.handle_action(action),
            Action::PromptCancel => {
                self.cancel_prompt();
                Ok(())
            }
            Action::PromptBackspace => {
                if let Some(prompt) = self.prompt_mut() {
                    if prompt.has_selection() {
                        prompt.delete_selection();
                    } else if prompt.cursor_pos > 0 {
                        let mut char_start = prompt.cursor_pos - 1;
                        while char_start > 0 && !prompt.input.is_char_boundary(char_start) {
                            char_start -= 1;
                        }
                        prompt.input.remove(char_start);
                        prompt.cursor_pos = char_start;
                    }
                }
                self.update_prompt_suggestions();
                Ok(())
            }
            Action::PromptDelete => {
                if let Some(prompt) = self.prompt_mut() {
                    if prompt.has_selection() {
                        prompt.delete_selection();
                    } else if prompt.cursor_pos < prompt.input.len() {
                        let mut char_end = prompt.cursor_pos + 1;
                        while char_end < prompt.input.len()
                            && !prompt.input.is_char_boundary(char_end)
                        {
                            char_end += 1;
                        }
                        prompt.input.drain(prompt.cursor_pos..char_end);
                    }
                }
                self.update_prompt_suggestions();
                Ok(())
            }
            Action::PromptMoveLeft => {
                if let Some(prompt) = self.prompt_mut() {
                    prompt.clear_selection();
                    if prompt.cursor_pos > 0 {
                        let mut new_pos = prompt.cursor_pos - 1;
                        while new_pos > 0 && !prompt.input.is_char_boundary(new_pos) {
                            new_pos -= 1;
                        }
                        prompt.cursor_pos = new_pos;
                    }
                }
                Ok(())
            }
            Action::PromptMoveRight => {
                if let Some(prompt) = self.prompt_mut() {
                    prompt.clear_selection();
                    if prompt.cursor_pos < prompt.input.len() {
                        let mut new_pos = prompt.cursor_pos + 1;
                        while new_pos < prompt.input.len()
                            && !prompt.input.is_char_boundary(new_pos)
                        {
                            new_pos += 1;
                        }
                        prompt.cursor_pos = new_pos;
                    }
                }
                Ok(())
            }
            Action::PromptMoveStart => {
                if let Some(prompt) = self.prompt_mut() {
                    prompt.clear_selection();
                    prompt.cursor_pos = 0;
                }
                Ok(())
            }
            Action::PromptMoveEnd => {
                if let Some(prompt) = self.prompt_mut() {
                    prompt.clear_selection();
                    prompt.cursor_pos = prompt.input.len();
                }
                Ok(())
            }
            Action::PromptSelectPrev => {
                if let Some(prompt) = self.prompt_mut() {
                    if !prompt.suggestions.is_empty() {
                        if let Some(selected) = prompt.selected_suggestion {
                            prompt.selected_suggestion = if selected == 0 {
                                Some(0)
                            } else {
                                Some(selected - 1)
                            };
                        }
                    }
                }
                Ok(())
            }
            Action::PromptSelectNext => {
                if let Some(prompt) = self.prompt_mut() {
                    if !prompt.suggestions.is_empty() {
                        if let Some(selected) = prompt.selected_suggestion {
                            let max_idx = prompt.suggestions.len().saturating_sub(1);
                            prompt.selected_suggestion = Some((selected + 1).min(max_idx));
                        } else {
                            prompt.selected_suggestion = Some(0);
                        }
                    }
                }
                Ok(())
            }
            _ => Ok(()), // Other prompt actions are no-ops or handled elsewhere.
        }
    }

    /// Handle a resolved action (view-centric edits and nav).
    pub fn handle_action(&mut self, action: Action) -> std::io::Result<()> {
        // Pre/post hooks (e.g., before/after command).
        if let Some(ref ts_manager) = self.ts_plugin_manager {
            let hook_args = HookArgs::PreCommand { action: action.clone() };
            ts_manager.run_hook("pre-command", hook_args);
        }

        match action {
            Action::None => {}
            Action::Quit => {
                self.should_quit = true;
            }
            Action::OpenFile => {
                let path = self.file_dialog("Open file: ")?;
                if let Some(p) = path {
                    self.open_file(&p)?;
                }
            }
            Action::Save => {
                self.save()?;
            }
            Action::SaveAll => {
                self.save_all()?;
            }
            Action::CommandPalette => {
                self.open_command_palette();
            }
            Action::GotoLine => {
                self.start_prompt("Go to line: ".to_string(), crate::prompt::PromptType::GotoLine);
            }
            Action::PopupConfirm => {
                self.handle_popup_confirm();
            }
            Action::PopupCancel => {
                self.hide_popup();
            }
            Action::PopupSelectNext => {
                self.active_state_mut().popups.select_next();
            }
            Action::PopupSelectPrev => {
                self.active_state_mut().popups.select_prev();
            }
            Action::PopupPageDown => {
                self.active_state_mut().popups.page_down();
            }
            Action::PopupPageUp => {
                self.active_state_mut().popups.page_up();
            }
            Action::InsertChar(c) => {
                if let Some(events) = self.action_to_events(Action::InsertChar(c)) {
                    self.apply_events(events);
                }
            }
            Action::InsertNewline => {
                if let Some(events) = self.action_to_events(Action::InsertNewline) {
                    self.apply_events(events);
                }
            }
            Action::DeleteBackward => {
                if let Some(events) = self.action_to_events(Action::DeleteBackward) {
                    self.apply_events(events);
                }
            }
            Action::DeleteForward => {
                if let Some(events) = self.action_to_events(Action::DeleteForward) {
                    self.apply_events(events);
                }
            }
            Action::MoveLeft
            | Action::MoveRight
            | Action::MoveUp
            | Action::MoveDown
            | Action::MoveLineStart
            | Action::MoveLineEnd
            | Action::MovePageUp
            | Action::MovePageDown
            | Action::MoveDocumentStart
            | Action::MoveDocumentEnd
            | Action::SelectLeft
            | Action::SelectRight
            | Action::SelectUp
            | Action::SelectDown
            | Action::SelectLineStart
            | Action::SelectLineEnd
            | Action::SelectDocumentStart
            | Action::SelectDocumentEnd
            | Action::ScrollUp
            | Action::ScrollDown => {
                if let Some(events) = self.action_to_events(action.clone()) {
                    self.apply_events(events);
                }
            }
            Action::Prompt => {
                // No-op placeholder for prompt actions handled elsewhere.
            }
            Action::PromptConfirm => {
                // Handle prompt confirmation - process based on prompt type
                if let Some((input, prompt_type, _selected_index)) = self.confirm_prompt() {
                    self.handle_prompt_confirm(input, prompt_type)?;
                }
            }
            Action::PopupShowDocumentation => {
                // No-op placeholder.
            }
            Action::PopupScrollDown | Action::PopupScrollUp => {
                // No-op placeholder.
            }
            Action::Back => {
                if let Some(entry) = self.position_history.back().cloned() {
                    self.jump_to_history_entry(&entry);
                }
            }
            Action::Forward => {
                if let Some(entry) = self.position_history.forward().cloned() {
                    self.jump_to_history_entry(&entry);
                }
            }
            Action::LspCompletion => {
                self.trigger_completion();
            }
            Action::LspGotoDefinition => {
                self.goto_definition();
            }
            Action::LspHover => {
                self.lsp_hover();
            }
            Action::LspReferences => {
                self.lsp_references();
            }
            Action::LspRename => {
                self.lsp_rename();
            }
            Action::Undo => {
                self.undo();
            }
            Action::Redo => {
                self.redo();
            }
            Action::Cut => {
                self.cut_selection();
            }
            Action::Copy => {
                self.copy_selection();
            }
            Action::Paste => {
                self.paste_clipboard();
            }
            Action::SelectAll => {
                self.select_all();
            }
            Action::Find => {
                self.prompt_search();
            }
            Action::FindNext => {
                self.find_next();
            }
            Action::FindPrev => {
                self.find_prev();
            }
            Action::Replace => {
                self.prompt_replace();
            }
            Action::ReplaceNext => {
                self.replace_next();
            }
            Action::ToggleLineNumbers => {
                let enabled = !self.active_state().margins.line_numbers_enabled();
                self.active_state_mut().margins.set_line_numbers(enabled);
            }
            Action::ToggleLineWrap => {
                self.toggle_line_wrap();
            }
            Action::SplitHorizontal => {
                self.split_horizontal();
            }
            Action::SplitVertical => {
                self.split_vertical();
            }
            Action::CloseSplit => {
                self.close_split();
            }
            Action::NextSplit => {
                self.next_split();
            }
            Action::PrevSplit => {
                self.prev_split();
            }
            Action::FocusFileExplorer => {
                self.focus_file_explorer();
            }
            Action::ToggleFileExplorer => {
                self.toggle_file_explorer();
            }
            Action::OpenRecent => {
                self.open_recent();
            }
            Action::OpenConfig => {
                self.open_config();
            }
            Action::OpenHelp => {
                self.open_help();
            }
            Action::OpenThemeSwitcher => {
                self.open_theme_switcher();
            }
            Action::ToggleComposeMode => {
                self.toggle_compose_mode();
            }
            Action::PromptSaveAs => {
                self.prompt_save_as();
            }
            Action::PromptOpen => {
                self.prompt_open();
            }
            Action::PromptSearch => {
                self.prompt_search();
            }
            Action::PromptReplace => {
                self.prompt_replace();
            }
            Action::PromptCommand => {
                self.open_command_palette();
            }
            Action::PromptClose => {
                self.cancel_prompt();
            }
            Action::OpenLogs => {
                self.open_logs();
            }
            Action::PluginAction(ref name) => {
                self.run_plugin_action(name);
            }
            _ => {}
        }

        if let Some(ref ts_manager) = self.ts_plugin_manager {
            let hook_args = HookArgs::PostCommand { action };
            ts_manager.run_hook("post-command", hook_args);
        }

        Ok(())
    }

    fn jump_to_history_entry(&mut self, entry: &crate::position_history::PositionEntry) {
        let buffer_id = entry.buffer_id;
        self.set_active_buffer(buffer_id);
        let mut new_pos = entry.position.into();
        let mut new_anchor = entry.anchor.map(|a| a.into());

        let move_event = Event::MoveCursor {
            cursor_id: self.active_state().cursors.primary_id(),
            old_position: new_pos,
            new_position: new_pos,
            old_anchor: new_anchor,
            new_anchor,
            old_sticky_column: None,
            new_sticky_column: Some(new_pos.column),
        };
        self.apply_event_to_active_buffer(&move_event);
    }

    /// Apply a batch of events to the active buffer and log them.
    fn apply_events(&mut self, events: Vec<Event>) {
        for event in events {
            self.active_event_log_mut().append(event.clone());
            self.apply_event_to_active_buffer(&event);
        }
    }

    /// Handle prompt confirmation based on prompt type (view-centric).
    fn handle_prompt_confirm(
        &mut self,
        input: String,
        prompt_type: crate::prompt::PromptType,
    ) -> std::io::Result<()> {
        match prompt_type {
            crate::prompt::PromptType::GotoLine => {
                self.handle_goto_line(input)
            }
            _ => {
                // Other prompt types not yet implemented - placeholder
                self.set_status_message("Prompt type not yet implemented".to_string());
                Ok(())
            }
        }
    }

    /// Handle goto line prompt (view-centric implementation).
    fn handle_goto_line(&mut self, input: String) -> std::io::Result<()> {
        match input.trim().parse::<usize>() {
            Ok(line_num) if line_num > 0 => {
                let target_line = line_num.saturating_sub(1); // Convert to 0-based
                let buffer_id = self.active_buffer;
                let split_id = self.split_manager.active_split();

                // Get view state and buffer
                if let (Some(view_state), Some(buffer_state)) = (
                    self.split_view_states.get_mut(&split_id),
                    self.buffers.get_mut(&buffer_id),
                ) {
                    let cursor_id = buffer_state.cursors.primary_id();
                    let old_position = buffer_state.cursors.primary().position;
                    let old_anchor = buffer_state.cursors.primary().anchor;
                    let old_sticky_column = buffer_state.cursors.primary().sticky_column;

                    // Ensure we have a layout
                    let gutter_width = view_state.viewport.gutter_width(&buffer_state.buffer);
                    let wrap_params = Some((view_state.viewport.width as usize, gutter_width));
                    let layout = view_state.ensure_layout(
                        &mut buffer_state.buffer,
                        self.config.editor.estimated_line_length,
                        wrap_params,
                    );

                    // Determine if large file mode
                    let is_large_file = buffer_state.buffer.line_count().is_none();
                    let buffer_len = buffer_state.buffer.len();
                    let estimated_line_length = self.config.editor.estimated_line_length;

                    let (new_position, status_message) = if is_large_file {
                        // Large file: estimate byte offset, find line start via buffer, then map to view
                        let estimated_offset = target_line * estimated_line_length;
                        let clamped_offset = estimated_offset.min(buffer_len);

                        // Find actual line start in buffer
                        let source_byte = {
                            let iter =
                                buffer_state
                                    .buffer
                                    .line_iterator(clamped_offset, estimated_line_length);
                            iter.current_position()
                        };

                        // Map source byte to view position via layout
                        let view_pos =
                            crate::navigation::mapping::source_to_view_pos(layout, source_byte, None);

                        let msg = format!(
                            "Jumped to estimated line {} (large file mode)",
                            line_num
                        );
                        (view_pos, msg)
                    } else {
                        // Small file: use exact line position, map buffer line → source byte → view
                        let max_line = buffer_state
                            .buffer
                            .line_count()
                            .unwrap_or(1)
                            .saturating_sub(1);
                        let actual_line = target_line.min(max_line);
                        let source_byte =
                            buffer_state.buffer.line_col_to_position(actual_line, 0);

                        // Map source byte to view position
                        let view_pos =
                            crate::navigation::mapping::source_to_view_pos(layout, source_byte, None);

                        let msg = if target_line > max_line {
                            format!(
                                "Line {} doesn't exist, jumped to line {}",
                                line_num,
                                actual_line + 1
                            )
                        } else {
                            format!("Jumped to line {}", line_num)
                        };
                        (view_pos, msg)
                    };

                    // Create MoveCursor event with view position
                    let event = crate::event::Event::MoveCursor {
                        cursor_id,
                        old_position: old_position.into(),
                        new_position: new_position.into(),
                        old_anchor: old_anchor.map(|a| a.into()),
                        new_anchor: None,
                        old_sticky_column,
                        new_sticky_column: Some(new_position.column),
                    };

                    // Apply the event
                    self.active_event_log_mut().append(event.clone());
                    self.apply_event_to_active_buffer(&event);

                    // Record position history
                    let view_event_pos = self.view_pos_to_event(new_position);
                    self.position_history
                        .record_movement(buffer_id, view_event_pos, None);

                    self.set_status_message(status_message);
                }
                Ok(())
            }
            Ok(_) => {
                self.set_status_message("Line number must be positive".to_string());
                Ok(())
            }
            Err(_) => {
                self.set_status_message(format!("Invalid line number: {}", input));
                Ok(())
            }
        }
    }

    /// Handle mouse events
    pub fn handle_mouse(
        &mut self,
        mouse_event: crossterm::event::MouseEvent,
    ) -> std::io::Result<()> {
        use crossterm::event::{MouseButton, MouseEventKind};

        // Cancel LSP rename prompt on any mouse interaction
        if let Some(ref prompt) = self.prompt {
            if matches!(prompt.prompt_type, PromptType::LspRename { .. }) {
                self.cancel_prompt();
            }
        }

        let col = mouse_event.column;
        let row = mouse_event.row;

        tracing::debug!(
            "handle_mouse: kind={:?}, col={}, row={}",
            mouse_event.kind,
            col,
            row
        );

        match mouse_event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.handle_mouse_click(col, row)?;
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                self.handle_mouse_drag(col, row)?;
            }
            MouseEventKind::Up(MouseButton::Left) => {
                // Stop dragging and clear drag state
                self.mouse_state.dragging_scrollbar = None;
                self.mouse_state.drag_start_row = None;
                self.mouse_state.drag_start_top_view_line = None;
                self.mouse_state.dragging_separator = None;
                self.mouse_state.drag_start_position = None;
                self.mouse_state.drag_start_ratio = None;
            }
            MouseEventKind::Moved => {
                self.update_hover_target(col, row);
            }
            MouseEventKind::ScrollUp => {
                self.dismiss_transient_popups();
                self.handle_mouse_scroll(col, row, -3)?;
            }
            MouseEventKind::ScrollDown => {
                self.dismiss_transient_popups();
                self.handle_mouse_scroll(col, row, 3)?;
            }
            _ => {}
        }

        self.mouse_state.last_position = Some((col, row));
        Ok(())
    }

    /// Dismiss hover/signature help popups
    fn dismiss_transient_popups(&mut self) {
        let state = self.active_state_mut();
        if let Some(popup) = state.popups.top() {
            if popup.title.as_ref().is_some_and(|t| t == "Hover" || t == "Signature Help") {
                state.popups.clear();
            }
        }
    }

    /// Update the current hover target based on mouse position
    pub(super) fn update_hover_target(&mut self, col: u16, row: u16) {
        use super::types::HoverTarget;

        // Check suggestions area first (command palette, autocomplete)
        if let Some((inner_rect, start_idx, _visible_count, total_count)) =
            &self.cached_layout.suggestions_area
        {
            if col >= inner_rect.x
                && col < inner_rect.x + inner_rect.width
                && row >= inner_rect.y
                && row < inner_rect.y + inner_rect.height
            {
                let relative_row = (row - inner_rect.y) as usize;
                let item_idx = start_idx + relative_row;

                if item_idx < *total_count {
                    self.mouse_state.hover_target = Some(HoverTarget::SuggestionItem(item_idx));
                    return;
                }
            }
        }

        // Check popups (they're rendered on top)
        for (popup_idx, _popup_rect, inner_rect, scroll_offset, num_items) in
            self.cached_layout.popup_areas.iter().rev()
        {
            if col >= inner_rect.x
                && col < inner_rect.x + inner_rect.width
                && row >= inner_rect.y
                && row < inner_rect.y + inner_rect.height
                && *num_items > 0
            {
                let relative_row = (row - inner_rect.y) as usize;
                let item_idx = scroll_offset + relative_row;

                if item_idx < *num_items {
                    self.mouse_state.hover_target =
                        Some(HoverTarget::PopupListItem(*popup_idx, item_idx));
                    return;
                }
            }
        }

        // Check menu bar (row 0)
        if row == 0 {
            let all_menus: Vec<crate::config::Menu> = self
                .config
                .menu
                .menus
                .iter()
                .chain(self.menu_state.plugin_menus.iter())
                .cloned()
                .collect();

            if let Some(menu_idx) = self.menu_state.get_menu_at_position(&all_menus, col) {
                self.mouse_state.hover_target = Some(HoverTarget::MenuBarItem(menu_idx));
                return;
            }
        }

        // Check split separators
        for (split_id, direction, sep_x, sep_y, sep_length) in &self.cached_layout.separator_areas {
            let is_on_separator = match direction {
                SplitDirection::Horizontal => {
                    row == *sep_y && col >= *sep_x && col < sep_x + sep_length
                }
                SplitDirection::Vertical => {
                    col == *sep_x && row >= *sep_y && row < sep_y + sep_length
                }
            };

            if is_on_separator {
                self.mouse_state.hover_target =
                    Some(HoverTarget::SplitSeparator(*split_id, *direction));
                return;
            }
        }

        // Check scrollbars
        for (split_id, _buffer_id, _content_rect, scrollbar_rect, thumb_start, thumb_end) in
            &self.cached_layout.split_areas
        {
            if col >= scrollbar_rect.x
                && col < scrollbar_rect.x + scrollbar_rect.width
                && row >= scrollbar_rect.y
                && row < scrollbar_rect.y + scrollbar_rect.height
            {
                let relative_row = row.saturating_sub(scrollbar_rect.y) as usize;
                let is_on_thumb = relative_row >= *thumb_start && relative_row < *thumb_end;

                if is_on_thumb {
                    self.mouse_state.hover_target = Some(HoverTarget::ScrollbarThumb(*split_id));
                } else {
                    self.mouse_state.hover_target = Some(HoverTarget::ScrollbarTrack(*split_id));
                }
                return;
            }
        }

        self.mouse_state.hover_target = None;
    }

    /// Handle mouse click (down event)
    pub(super) fn handle_mouse_click(&mut self, col: u16, row: u16) -> std::io::Result<()> {
        // Check if click is on suggestions (command palette, autocomplete)
        if let Some((inner_rect, start_idx, _visible_count, total_count)) =
            &self.cached_layout.suggestions_area.clone()
        {
            if col >= inner_rect.x
                && col < inner_rect.x + inner_rect.width
                && row >= inner_rect.y
                && row < inner_rect.y + inner_rect.height
            {
                let relative_row = (row - inner_rect.y) as usize;
                let item_idx = start_idx + relative_row;

                if item_idx < *total_count {
                    if let Some(prompt) = &mut self.prompt {
                        prompt.selected_suggestion = Some(item_idx);
                    }
                    return self.handle_action(Action::PromptConfirm);
                }
            }
        }

        // Check if click is on a popup
        for (_popup_idx, _popup_rect, inner_rect, scroll_offset, num_items) in
            self.cached_layout.popup_areas.iter().rev()
        {
            if col >= inner_rect.x
                && col < inner_rect.x + inner_rect.width
                && row >= inner_rect.y
                && row < inner_rect.y + inner_rect.height
                && *num_items > 0
            {
                let relative_row = (row - inner_rect.y) as usize;
                let item_idx = scroll_offset + relative_row;

                if item_idx < *num_items {
                    let state = self.active_state_mut();
                    if let Some(popup) = state.popups.top_mut() {
                        if let crate::popup::PopupContent::List { items: _, selected } =
                            &mut popup.content
                        {
                            *selected = item_idx;
                        }
                    }
                    return self.handle_action(Action::PopupConfirm);
                }
            }
        }

        // Check if click is on menu bar (row 0)
        if row == 0 {
            let all_menus: Vec<crate::config::Menu> = self
                .config
                .menu
                .menus
                .iter()
                .chain(self.menu_state.plugin_menus.iter())
                .cloned()
                .collect();

            if let Some(menu_idx) = self.menu_state.get_menu_at_position(&all_menus, col) {
                if self.menu_state.active_menu == Some(menu_idx) {
                    self.menu_state.close_menu();
                } else {
                    self.menu_state.open_menu(menu_idx);
                }
            } else {
                self.menu_state.close_menu();
            }
            return Ok(());
        }

        // Check if click is on an open menu dropdown
        if let Some(active_idx) = self.menu_state.active_menu {
            let all_menus: Vec<crate::config::Menu> = self
                .config
                .menu
                .menus
                .iter()
                .chain(self.menu_state.plugin_menus.iter())
                .cloned()
                .collect();

            if let Some(menu) = all_menus.get(active_idx) {
                let mut menu_x = 0u16;
                for m in all_menus.iter().take(active_idx) {
                    menu_x += m.label.len() as u16 + 3;
                }

                let max_label_len = menu
                    .items
                    .iter()
                    .map(|item| match item {
                        crate::config::MenuItem::Action { label, .. } => label.len(),
                        crate::config::MenuItem::Separator { .. } => 0,
                        crate::config::MenuItem::Submenu { label, .. } => label.len(),
                    })
                    .max()
                    .unwrap_or(0);
                let dropdown_width = max_label_len + 30;
                let dropdown_height = menu.items.len() as u16 + 2;

                if col >= menu_x
                    && col < menu_x + dropdown_width as u16
                    && row >= 1
                    && row < 1 + dropdown_height
                {
                    if let Some(item_idx) = self.menu_state.get_item_at_position(menu, row) {
                        if let Some(crate::config::MenuItem::Action { action, args, .. }) =
                            menu.items.get(item_idx)
                        {
                            let action_name = action.clone();
                            let action_args = args.clone();
                            self.menu_state.close_menu();

                            if let Some(action) = Action::from_str(&action_name, &action_args) {
                                return self.handle_action(action);
                            }
                        }
                    }
                    return Ok(());
                }
            }

            self.menu_state.close_menu();
            return Ok(());
        }

        // Check if click is on file explorer
        if let Some(explorer_area) = self.cached_layout.file_explorer_area {
            if col >= explorer_area.x
                && col < explorer_area.x + explorer_area.width
                && row >= explorer_area.y
                && row < explorer_area.y + explorer_area.height
            {
                self.handle_file_explorer_click(col, row, explorer_area)?;
                return Ok(());
            }
        }

        // Check if click is on a scrollbar
        let scrollbar_hit = self.cached_layout.split_areas.iter().find_map(
            |(split_id, buffer_id, _content_rect, scrollbar_rect, thumb_start, thumb_end)| {
                if col >= scrollbar_rect.x
                    && col < scrollbar_rect.x + scrollbar_rect.width
                    && row >= scrollbar_rect.y
                    && row < scrollbar_rect.y + scrollbar_rect.height
                {
                    let relative_row = row.saturating_sub(scrollbar_rect.y) as usize;
                    let is_on_thumb = relative_row >= *thumb_start && relative_row < *thumb_end;
                    Some((*split_id, *buffer_id, *scrollbar_rect, is_on_thumb))
                } else {
                    None
                }
            },
        );

        if let Some((split_id, buffer_id, scrollbar_rect, is_on_thumb)) = scrollbar_hit {
            self.split_manager.set_active_split(split_id);
            if buffer_id != self.active_buffer {
                self.position_history.commit_pending_movement();
                self.set_active_buffer(buffer_id);
            }

            if is_on_thumb {
                self.mouse_state.dragging_scrollbar = Some(split_id);
                self.mouse_state.drag_start_row = Some(row);
                if let Some(state) = self.buffers.get(&buffer_id) {
                    self.mouse_state.drag_start_top_view_line = Some(state.viewport.top_view_line);
                }
            } else {
                self.mouse_state.dragging_scrollbar = Some(split_id);
                self.handle_scrollbar_jump(col, row, buffer_id, scrollbar_rect)?;
            }
            return Ok(());
        }

        // Check if click is on a split separator
        for (split_id, direction, sep_x, sep_y, sep_length) in &self.cached_layout.separator_areas {
            let is_on_separator = match direction {
                SplitDirection::Horizontal => {
                    row == *sep_y && col >= *sep_x && col < sep_x + sep_length
                }
                SplitDirection::Vertical => {
                    col == *sep_x && row >= *sep_y && row < sep_y + sep_length
                }
            };

            if is_on_separator {
                self.mouse_state.dragging_separator = Some((*split_id, *direction));
                self.mouse_state.drag_start_position = Some((col, row));
                if let Some(ratio) = self.split_manager.get_ratio(*split_id) {
                    self.mouse_state.drag_start_ratio = Some(ratio);
                }
                return Ok(());
            }
        }

        // Check if click is in editor content area
        for (split_id, buffer_id, content_rect, _scrollbar_rect, _thumb_start, _thumb_end) in
            &self.cached_layout.split_areas
        {
            if col >= content_rect.x
                && col < content_rect.x + content_rect.width
                && row >= content_rect.y
                && row < content_rect.y + content_rect.height
            {
                self.handle_editor_click(col, row, *split_id, *buffer_id, *content_rect)?;
                return Ok(());
            }
        }

        Ok(())
    }

    /// Handle mouse drag event
    pub(super) fn handle_mouse_drag(&mut self, col: u16, row: u16) -> std::io::Result<()> {
        if let Some(dragging_split_id) = self.mouse_state.dragging_scrollbar {
            for (split_id, buffer_id, _content_rect, scrollbar_rect, _thumb_start, _thumb_end) in
                &self.cached_layout.split_areas
            {
                if *split_id == dragging_split_id {
                    if self.mouse_state.drag_start_row.is_some() {
                        self.handle_scrollbar_drag_relative(row, *buffer_id, *scrollbar_rect)?;
                    } else {
                        self.handle_scrollbar_jump(col, row, *buffer_id, *scrollbar_rect)?;
                    }
                    return Ok(());
                }
            }
        }

        if let Some((split_id, direction)) = self.mouse_state.dragging_separator {
            self.handle_separator_drag(col, row, split_id, direction)?;
            return Ok(());
        }

        Ok(())
    }

    /// Handle separator drag for split resizing
    pub(super) fn handle_separator_drag(
        &mut self,
        col: u16,
        row: u16,
        split_id: SplitId,
        direction: SplitDirection,
    ) -> std::io::Result<()> {
        let Some((start_col, start_row)) = self.mouse_state.drag_start_position else {
            return Ok(());
        };
        let Some(start_ratio) = self.mouse_state.drag_start_ratio else {
            return Ok(());
        };
        let Some(editor_area) = self.cached_layout.editor_content_area else {
            return Ok(());
        };

        let (delta, total_size) = match direction {
            SplitDirection::Horizontal => {
                let delta = row as i32 - start_row as i32;
                let total = editor_area.height as i32;
                (delta, total)
            }
            SplitDirection::Vertical => {
                let delta = col as i32 - start_col as i32;
                let total = editor_area.width as i32;
                (delta, total)
            }
        };

        if total_size > 0 {
            let ratio_delta = delta as f32 / total_size as f32;
            let new_ratio = (start_ratio + ratio_delta).clamp(0.1, 0.9);
            let _ = self.split_manager.set_ratio(split_id, new_ratio);
        }

        Ok(())
    }

    /// Handle mouse wheel scroll event
    pub(super) fn handle_mouse_scroll(
        &mut self,
        col: u16,
        row: u16,
        delta: i32,
    ) -> std::io::Result<()> {
        // Check if scroll is over the file explorer
        if let Some(explorer_area) = self.cached_layout.file_explorer_area {
            if col >= explorer_area.x
                && col < explorer_area.x + explorer_area.width
                && row >= explorer_area.y
                && row < explorer_area.y + explorer_area.height
            {
                if let Some(explorer) = &mut self.file_explorer {
                    let visible = explorer.tree().get_visible_nodes();
                    if visible.is_empty() {
                        return Ok(());
                    }

                    let current_index = explorer.get_selected_index().unwrap_or(0);
                    let new_index = if delta < 0 {
                        current_index.saturating_sub(delta.abs() as usize)
                    } else {
                        (current_index + delta as usize).min(visible.len() - 1)
                    };

                    if let Some(node_id) = explorer.get_node_at_index(new_index) {
                        explorer.set_selected(Some(node_id));
                        explorer.update_scroll_for_selection();
                    }
                }
                return Ok(());
            }
        }

        // Scroll the editor in the active split
        if let Some(state) = self.buffers.get_mut(&self.active_buffer) {
            // Calculate new top view line
            let new_top = if delta < 0 {
                state.viewport.top_view_line.saturating_sub(delta.abs() as usize)
            } else {
                let total_lines = state.buffer.line_count().unwrap_or(1);
                let max_top = total_lines.saturating_sub(state.viewport.height as usize);
                (state.viewport.top_view_line + delta as usize).min(max_top)
            };
            state.viewport.top_view_line = new_top;
        }

        Ok(())
    }

    /// Handle scrollbar drag with relative movement
    pub(super) fn handle_scrollbar_drag_relative(
        &mut self,
        row: u16,
        buffer_id: BufferId,
        scrollbar_rect: ratatui::layout::Rect,
    ) -> std::io::Result<()> {
        let Some(start_row) = self.mouse_state.drag_start_row else {
            return Ok(());
        };
        let Some(start_top_view_line) = self.mouse_state.drag_start_top_view_line else {
            return Ok(());
        };

        let Some(state) = self.buffers.get_mut(&buffer_id) else {
            return Ok(());
        };

        let total_lines = state.buffer.line_count().unwrap_or(1);
        let viewport_height = state.viewport.height as usize;
        let scrollbar_height = scrollbar_rect.height as usize;

        if scrollbar_height == 0 || total_lines <= viewport_height {
            return Ok(());
        }

        let row_delta = row as i32 - start_row as i32;
        let scrollable_lines = total_lines.saturating_sub(viewport_height);
        let lines_per_row = scrollable_lines as f32 / scrollbar_height as f32;
        let line_delta = (row_delta as f32 * lines_per_row) as i32;

        let new_top = (start_top_view_line as i32 + line_delta)
            .max(0)
            .min(scrollable_lines as i32) as usize;

        state.viewport.top_view_line = new_top;

        Ok(())
    }

    /// Handle scrollbar jump (click on track)
    pub(super) fn handle_scrollbar_jump(
        &mut self,
        _col: u16,
        row: u16,
        buffer_id: BufferId,
        scrollbar_rect: ratatui::layout::Rect,
    ) -> std::io::Result<()> {
        let Some(state) = self.buffers.get_mut(&buffer_id) else {
            return Ok(());
        };

        let total_lines = state.buffer.line_count().unwrap_or(1);
        let viewport_height = state.viewport.height as usize;
        let scrollbar_height = scrollbar_rect.height as usize;

        if scrollbar_height == 0 || total_lines <= viewport_height {
            return Ok(());
        }

        let relative_row = row.saturating_sub(scrollbar_rect.y) as usize;
        let click_fraction = relative_row as f32 / scrollbar_height as f32;
        let scrollable_lines = total_lines.saturating_sub(viewport_height);
        let new_top = (click_fraction * scrollable_lines as f32) as usize;

        state.viewport.top_view_line = new_top.min(scrollable_lines);

        Ok(())
    }

    /// Handle file explorer click
    pub(super) fn handle_file_explorer_click(
        &mut self,
        _col: u16,
        row: u16,
        explorer_area: ratatui::layout::Rect,
    ) -> std::io::Result<()> {
        let relative_row = row.saturating_sub(explorer_area.y) as usize;

        // Get info about the clicked node first
        let node_info = if let Some(explorer) = &mut self.file_explorer {
            let scroll_offset = explorer.get_scroll_offset();
            let item_index = scroll_offset + relative_row;

            if let Some(node_id) = explorer.get_node_at_index(item_index) {
                explorer.set_selected(Some(node_id));

                // Check if it's a file or directory
                if let Some(node) = explorer.tree().get_node(node_id) {
                    if node.is_file() {
                        Some((true, node.entry.path.clone()))
                    } else {
                        // Directory clicked - just select it
                        // Use Enter key or double-click to expand (expansion is async)
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        // If it was a file, open it (outside the explorer borrow)
        if let Some((true, path)) = node_info {
            self.open_file(&path)?;
        }

        Ok(())
    }

    /// Handle click in editor content area
    ///
    /// In the view-centric architecture, click positioning works as follows:
    /// 1. Calculate view line from row (viewport.top_view_line + relative_row)
    /// 2. Calculate visual column from col (accounting for gutter)
    /// 3. Create ViewPosition with view_line, column, and derive source_byte
    pub(super) fn handle_editor_click(
        &mut self,
        col: u16,
        row: u16,
        split_id: SplitId,
        buffer_id: BufferId,
        content_rect: ratatui::layout::Rect,
    ) -> std::io::Result<()> {
        // Focus this split
        self.split_manager.set_active_split(split_id);
        if buffer_id != self.active_buffer {
            self.position_history.commit_pending_movement();
            self.set_active_buffer(buffer_id);
        }

        // Calculate position in buffer
        let Some(state) = self.buffers.get_mut(&buffer_id) else {
            return Ok(());
        };

        let relative_col = col.saturating_sub(content_rect.x) as usize;
        let relative_row = row.saturating_sub(content_rect.y) as usize;

        // Account for line numbers (gutter width) - estimate based on total lines
        let total_lines = state.buffer.line_count().unwrap_or(1);
        let gutter_width = if self.config.editor.line_numbers {
            let digits = (total_lines.max(1) as f64).log10().floor() as usize + 1;
            digits + 2 // digits + space + separator
        } else {
            0
        };

        let text_col = relative_col.saturating_sub(gutter_width);
        let view_line = state.viewport.top_view_line + relative_row;

        // Clamp to valid line range
        let view_line = view_line.min(total_lines.saturating_sub(1));

        // Get line content and calculate source byte position
        // In the view-centric model, we calculate source_byte from the line content
        let line_content = state.buffer.get_line(view_line).unwrap_or_default();
        let line_str = String::from_utf8_lossy(&line_content);

        let mut char_offset = 0;
        let mut visual_col = 0;

        for ch in line_str.chars() {
            if visual_col >= text_col {
                break;
            }
            visual_col += if ch == '\t' {
                4 - (visual_col % 4)
            } else {
                1
            };
            char_offset += ch.len_utf8();
        }

        // Get byte offset for this line using buffer's line_start_offset
        let byte_offset = state.buffer.line_start_offset(view_line).unwrap_or(0);
        let position = byte_offset + char_offset;
        let position = position.min(state.buffer.len());

        // Create a ViewPosition and move the cursor
        let view_pos = ViewPosition {
            view_line,
            column: text_col,
            source_byte: Some(position),
        };

        state.cursors.primary_mut().move_to(view_pos, false);

        Ok(())
    }
}
