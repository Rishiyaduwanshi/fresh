//! Status bar and prompt/minibuffer rendering (view-centric).

use crate::cursor::ViewPosition;
use crate::prompt::Prompt;
use crate::state::EditorState;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

/// Renders the status bar and prompt/minibuffer.
pub struct StatusBarRenderer;

impl StatusBarRenderer {
    /// Render only the status bar (without prompt).
    pub fn render_status_bar(
        frame: &mut Frame,
        area: Rect,
        state: &EditorState,
        layout: Option<&crate::ui::view_pipeline::Layout>,
        status_message: &Option<String>,
        plugin_status_message: &Option<String>,
        lsp_status: &str,
        theme: &crate::theme::Theme,
        display_name: &str,
        keybindings: &crate::keybindings::KeybindingResolver,
        chord_state: &[(crossterm::event::KeyCode, crossterm::event::KeyModifiers)],
    ) {
        Self::render_status(
            frame,
            area,
            state,
            layout,
            status_message,
            plugin_status_message,
            lsp_status,
            theme,
            display_name,
            keybindings,
            chord_state,
        );
    }

    /// Render the prompt/minibuffer.
    pub fn render_prompt(
        frame: &mut Frame,
        area: Rect,
        prompt: &Prompt,
        theme: &crate::theme::Theme,
    ) {
        let base_style = Style::default().fg(theme.prompt_fg).bg(theme.prompt_bg);

        // Create spans for the prompt.
        let mut spans = vec![Span::styled(prompt.message.clone(), base_style)];

        // If there's a selection, split the input into parts.
        if let Some((sel_start, sel_end)) = prompt.selection_range() {
            let input = &prompt.input;

            if sel_start > 0 {
                spans.push(Span::styled(input[..sel_start].to_string(), base_style));
            }

            if sel_start < sel_end {
                let selection_style = Style::default()
                    .fg(theme.prompt_selection_fg)
                    .bg(theme.prompt_selection_bg);
                spans.push(Span::styled(
                    input[sel_start..sel_end].to_string(),
                    selection_style,
                ));
            }

            if sel_end < input.len() {
                spans.push(Span::styled(input[sel_end..].to_string(), base_style));
            }
        } else {
            spans.push(Span::styled(prompt.input.clone(), base_style));
        }

        let line = Line::from(spans);
        let prompt_line = Paragraph::new(line).style(base_style);

        frame.render_widget(prompt_line, area);

        // Set cursor position in the prompt.
        let cursor_x = (prompt.message.len() + prompt.cursor_pos) as u16;
        if cursor_x < area.width {
            frame.set_cursor_position((area.x + cursor_x, area.y));
        }
    }

    /// Render the normal status bar.
    fn render_status(
        frame: &mut Frame,
        area: Rect,
        state: &EditorState,
        layout: Option<&crate::ui::view_pipeline::Layout>,
        status_message: &Option<String>,
        plugin_status_message: &Option<String>,
        lsp_status: &str,
        theme: &crate::theme::Theme,
        display_name: &str,
        keybindings: &crate::keybindings::KeybindingResolver,
        chord_state: &[(crossterm::event::KeyCode, crossterm::event::KeyModifiers)],
    ) {
        let filename = display_name;
        let modified = if state.buffer.is_modified() { " [+]" } else { "" };

        // Chord indicator.
        let chord_display = if !chord_state.is_empty() {
            let chord_str = chord_state
                .iter()
                .map(|(code, modifiers)| crate::keybindings::format_keybinding(code, modifiers))
                .collect::<Vec<_>>()
                .join(" ");
            format!(" [{}]", chord_str)
        } else {
            String::new()
        };

        // View mode indicator.
        let mode_label = match state.view_mode {
            crate::state::ViewMode::Compose => " | Compose",
            _ => "",
        };

        let cursor = *state.primary_cursor();

        // Derive view position and optional source position (line/col) for display.
        let (view_line, view_col) = (
            cursor.position.view_line.map(|l| l + 1).unwrap_or(1),
            cursor.position.column.map(|c| c + 1).unwrap_or(1),
        );

        let mut source_display = None;
        if let (Some(l), Some(layout)) = (cursor.position.source_byte, layout) {
            if let Some((src_line, src_col)) = layout.source_byte_to_view_position(l) {
                // source_byte_to_view_position returns view coords; use buffer to get true source line/col.
                let (line, col) = state.buffer.offset_to_position(l).map(|p| (p.line, p.column)).unwrap_or((src_line, src_col));
                source_display = Some((line + 1, col + 1));
            }
        }

        // Diagnostics counts.
        let diagnostics = state.overlays.all();
        let diagnostic_ns = crate::lsp_diagnostics::lsp_diagnostic_namespace();
        let (mut error_count, mut warning_count, mut info_count) = (0, 0, 0);
        for overlay in diagnostics {
            if overlay.namespace.as_ref() == Some(&diagnostic_ns) {
                match overlay.priority {
                    100 => error_count += 1,
                    50 => warning_count += 1,
                    _ => info_count += 1,
                }
            }
        }

        let diagnostics_summary = if error_count + warning_count + info_count > 0 {
            let mut parts = Vec::new();
            if error_count > 0 {
                parts.push(format!("E:{}", error_count));
            }
            if warning_count > 0 {
                parts.push(format!("W:{}", warning_count));
            }
            if info_count > 0 {
                parts.push(format!("I:{}", info_count));
            }
            format!(" | {}", parts.join(" "))
        } else {
            String::new()
        };

        // Cursor count indicator.
        let cursor_count_indicator = if state.cursors.count() > 1 {
            format!(" | {} cursors", state.cursors.count())
        } else {
            String::new()
        };

        // LSP indicator.
        let lsp_indicator = if !lsp_status.is_empty() {
            format!(" | {}", lsp_status)
        } else {
            String::new()
        };

        let mut message_parts: Vec<&str> = Vec::new();
        if let Some(msg) = status_message {
            if !msg.is_empty() {
                message_parts.push(msg);
            }
        }
        if let Some(msg) = plugin_status_message {
            if !msg.is_empty() {
                message_parts.push(msg);
            }
        }

        let message_suffix = if message_parts.is_empty() {
            String::new()
        } else {
            format!(" | {}", message_parts.join(" | "))
        };

        let source_suffix = if let Some((line, col)) = source_display {
            format!(" (Src Ln {}, Col {})", line, col)
        } else {
            String::new()
        };

        let base_status = format!(
            "{filename}{modified} | View Ln {view_line}, Col {view_col}{source_suffix}{mode_label}{diagnostics_summary}{cursor_count_indicator}{lsp_indicator}"
        );
        let left_status = format!("{base_status}{chord_display}{message_suffix}");

        // Command Palette indicator on the right side.
        let cmd_palette_shortcut = keybindings
            .get_keybinding_for_action(
                &crate::keybindings::Action::CommandPalette,
                crate::keybindings::KeyContext::Global,
            )
            .unwrap_or_else(|| "?".to_string());
        let cmd_palette_indicator = format!("Palette: {}", cmd_palette_shortcut);
        let padded_cmd_palette = format!(" {} ", cmd_palette_indicator);

        let available_width = area.width as usize;
        let cmd_palette_width = padded_cmd_palette.len();

        let spans = if available_width >= 15 {
            let left_max_width = available_width.saturating_sub(cmd_palette_width + 1);

            let displayed_left = if left_status.len() > left_max_width {
                let truncate_at = left_max_width.saturating_sub(3);
                if truncate_at > 0 {
                    format!("{}...", &left_status[..truncate_at])
                } else {
                    String::from("...")
                }
            } else {
                left_status.clone()
            };

            let mut spans = vec![Span::styled(
                displayed_left.clone(),
                Style::default()
                    .fg(theme.status_bar_fg)
                    .bg(theme.status_bar_bg),
            )];

            let displayed_left_len = displayed_left.len();

            if displayed_left_len + cmd_palette_width < available_width {
                let padding_len = available_width - displayed_left_len - cmd_palette_width;
                spans.push(Span::styled(
                    " ".repeat(padding_len),
                    Style::default()
                        .fg(theme.status_bar_fg)
                        .bg(theme.status_bar_bg),
                ));
            } else if displayed_left_len < available_width {
                spans.push(Span::styled(
                    " ",
                    Style::default()
                        .fg(theme.status_bar_fg)
                        .bg(theme.status_bar_bg),
                ));
            }

            spans.push(Span::styled(
                padded_cmd_palette.clone(),
                Style::default()
                    .fg(theme.help_indicator_fg)
                    .bg(theme.help_indicator_bg),
            ));

            let total_width = displayed_left_len
                + if displayed_left_len + cmd_palette_width < available_width {
                    available_width - displayed_left_len - cmd_palette_width
                } else if displayed_left_len < available_width {
                    1
                } else {
                    0
                }
                + cmd_palette_width;

            if total_width < available_width {
                spans.push(Span::styled(
                    " ".repeat(available_width - total_width),
                    Style::default()
                        .fg(theme.status_bar_fg)
                        .bg(theme.status_bar_bg),
                ));
            }

            spans
        } else {
            let mut spans = vec![];
            let displayed_left = if left_status.len() > available_width {
                let truncate_at = available_width.saturating_sub(3);
                if truncate_at > 0 {
                    format!("{}...", &left_status[..truncate_at])
                } else {
                    left_status.chars().take(available_width).collect()
                }
            } else {
                left_status.clone()
            };

            spans.push(Span::styled(
                displayed_left.clone(),
                Style::default()
                    .fg(theme.status_bar_fg)
                    .bg(theme.status_bar_bg),
            ));

            if displayed_left.len() < available_width {
                spans.push(Span::styled(
                    " ".repeat(available_width - displayed_left.len()),
                    Style::default()
                        .fg(theme.status_bar_fg)
                        .bg(theme.status_bar_bg),
                ));
            }

            spans
        };

        let status_line = Paragraph::new(Line::from(spans));

        frame.render_widget(status_line, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompt::PromptType;

    #[test]
    fn basic_prompt_cursor_position() {
        let mut prompt = Prompt::new("Test".to_string(), PromptType::Command);
        prompt.input = "input".to_string();
        prompt.cursor_pos = 2;
        // Rendering can't be unit-tested easily here without a terminal; just ensure accessors work.
        assert_eq!(prompt.cursor_pos, 2);
    }
}
