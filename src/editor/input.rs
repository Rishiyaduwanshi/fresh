use super::*;
use crate::hooks::HookArgs;
use crate::keybindings::Action;

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
        if let Some(hook_registry) = self.hook_registry.as_ref() {
            let hook_args = HookArgs::PreCommand { action: action.clone() };
            hook_registry.read().unwrap().run_hooks("pre-command", &hook_args);
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
                if let Some(entry) = self.position_history.back() {
                    self.jump_to_history_entry(entry);
                }
            }
            Action::Forward => {
                if let Some(entry) = self.position_history.forward() {
                    self.jump_to_history_entry(entry);
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
            Action::PluginAction(name) => {
                self.run_plugin_action(&name);
            }
            _ => {}
        }

        if let Some(hook_registry) = self.hook_registry.as_ref() {
            let hook_args = HookArgs::PostCommand { action };
            hook_registry.read().unwrap().run_hooks("post-command", &hook_args);
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
                        old_position,
                        new_position,
                        old_anchor,
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
}
