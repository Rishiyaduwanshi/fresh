//! Split pane layout and buffer rendering (view-centric).

use crate::ansi_background::AnsiBackground;
use crate::cursor::Cursor;
use crate::plugin_api::ViewTokenWire;
use crate::editor::BufferMetadata;
use crate::event::{BufferId, EventLog, SplitDirection, SplitId};
use crate::plugin_api::ViewTransformPayload;
use crate::split::SplitManager;
use crate::state::{EditorState, ViewMode};
use crate::text_buffer::Buffer;
use crate::ui::tabs::TabsRenderer;
use crate::ui::view_pipeline::{Layout, LineStart, ViewLine};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;
use std::collections::{HashMap, HashSet};
use std::ops::Range;

/// Processed view data containing display lines from the view pipeline.
struct ViewData {
    lines: Vec<ViewLine>,
}

#[derive(Clone, Copy)]
struct ViewAnchor {
    start_line_idx: usize,
    start_line_skip: usize,
}

struct ComposeLayout {
    render_area: Rect,
    left_pad: u16,
    right_pad: u16,
}

struct SelectionContext {
    ranges: Vec<Range<usize>>,
    block_rects: Vec<(usize, usize, usize, usize)>,
    cursor_positions: Vec<(u16, u16)>,
    primary_cursor_position: (u16, u16),
}

struct DecorationContext {
    highlight_spans: Vec<crate::highlighter::HighlightSpan>,
    semantic_spans: Vec<crate::highlighter::HighlightSpan>,
    viewport_overlays: Vec<(crate::overlay::Overlay, Range<usize>)>,
    virtual_text_lookup: HashMap<usize, Vec<crate::virtual_text::VirtualText>>,
    diagnostic_lines: HashSet<usize>,
}

struct LineRenderOutput {
    lines: Vec<Line<'static>>,
    cursor: Option<(u16, u16)>,
    last_line_end: Option<LastLineEnd>,
    content_lines_rendered: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LastLineEnd {
    pos: (u16, u16),
    terminated_with_newline: bool,
}

struct SplitLayout {
    tabs_rect: Rect,
    content_rect: Rect,
    scrollbar_rect: Rect,
}

struct ViewPreferences {
    view_mode: ViewMode,
    compose_width: Option<u16>,
    compose_column_guides: Option<Vec<u16>>,
    view_transform: Option<ViewTransformPayload>,
}

struct LineRenderInput<'a> {
    state: &'a EditorState,
    theme: &'a crate::theme::Theme,
    view_lines: &'a [ViewLine],
    view_anchor: ViewAnchor,
    render_area: Rect,
    gutter_width: usize,
    selection: &'a SelectionContext,
    decorations: &'a DecorationContext,
    starting_line_num: usize,
    visible_line_count: usize,
    lsp_waiting: bool,
    is_active: bool,
    line_wrap: bool,
    estimated_lines: usize,
}

/// Renders split panes and their content (view-centric).
pub struct SplitRenderer;

impl SplitRenderer {
    /// Render the main content area with all splits.
    pub fn render_content(
        frame: &mut Frame,
        area: Rect,
        split_manager: &SplitManager,
        buffers: &mut HashMap<BufferId, EditorState>,
        buffer_metadata: &HashMap<BufferId, BufferMetadata>,
        event_logs: &mut HashMap<BufferId, EventLog>,
        theme: &crate::theme::Theme,
        ansi_background: Option<&AnsiBackground>,
        background_fade: f32,
        lsp_waiting: bool,
        large_file_threshold_bytes: u64,
        estimated_line_length: usize,
        mut split_view_states: Option<&mut HashMap<SplitId, crate::split::SplitViewState>>,
        hide_cursor: bool,
    ) -> Vec<(SplitId, BufferId, Rect, Rect, usize, usize)> {
        let _span = tracing::trace_span!("render_content").entered();
        let visible_buffers = split_manager.get_visible_buffers(area);
        let active_split_id = split_manager.active_split();
        let mut split_areas = Vec::new();

        for (split_id, buffer_id, split_area) in visible_buffers {
            let is_active = split_id == active_split_id;
            let layout = Self::split_layout(split_area);
            let (split_buffers, tab_scroll_offset) =
                Self::split_buffers_for_tabs(split_view_states.as_deref(), split_id, buffer_id);

            TabsRenderer::render_for_split(
                frame,
                layout.tabs_rect,
                &split_buffers,
                buffers,
                buffer_metadata,
                buffer_id,
                theme,
                is_active,
                tab_scroll_offset,
            );

            if let Some(state) = buffers.get_mut(&buffer_id) {
                let _saved = Self::temporary_split_state(
                    state,
                    split_view_states.as_deref(),
                    split_id,
                    is_active,
                );
                Self::sync_viewport_to_content(state, layout.content_rect);
                let view_prefs =
                    Self::resolve_view_preferences(state, split_view_states.as_deref(), split_id);

                let mut layout_override: Option<Layout> = None;
                if let Some(view_states) = split_view_states.as_mut() {
                    if let Some(view_state) = view_states.get_mut(&split_id) {
                        view_state.viewport.width = state.viewport.width;
                        view_state.viewport.height = state.viewport.height;
                        view_state.cursors = state.cursors.clone();

                        let gutter_width = view_state.viewport.gutter_width(&state.buffer);
                        let wrap_params = Some((view_state.viewport.width as usize, gutter_width));
                        let layout = view_state
                            .ensure_layout(&mut state.buffer, estimated_line_length, wrap_params)
                            .clone();

                        let primary_cursor = *state.cursors.primary();
                        view_state.viewport.ensure_visible_in_layout(
                            &primary_cursor,
                            &layout,
                            gutter_width,
                        );

                        layout_override = Some(layout);
                        state.cursors = view_state.cursors.clone();
                        state.viewport = view_state.viewport.clone();
                    }
                }

                let layout_for_render = layout_override.unwrap_or_else(|| {
                    let tokens = Self::build_base_tokens_for_hook(
                        &mut state.buffer,
                        state.viewport.top_view_line,
                        estimated_line_length,
                        state.viewport.visible_line_count(),
                    );
                    Layout::from_tokens(&tokens, 0..state.buffer.len())
                });

                let view_data = ViewData {
                    lines: layout_for_render.lines.clone(),
                };

                let view_anchor = ViewAnchor {
                    start_line_idx: state.viewport.top_view_line,
                    start_line_skip: 0,
                };

                let gutter_width = state.viewport.gutter_width(&state.buffer);

                let (selection, decorations) =
                    Self::build_contexts(state, &layout_for_render, gutter_width);

                let line_render_input = LineRenderInput {
                    state,
                    theme,
                    view_lines: &view_data.lines,
                    view_anchor,
                    render_area: layout.content_rect,
                    gutter_width,
                    selection: &selection,
                    decorations: &decorations,
                    starting_line_num: view_anchor.start_line_idx,
                    visible_line_count: state.viewport.visible_line_count(),
                    lsp_waiting,
                    is_active,
                    line_wrap: state.viewport.line_wrap_enabled,
                    estimated_lines: estimated_line_length,
                };

                let line_output = Self::render_lines(line_render_input);

                // Render background ANSI if needed.
                if let Some(bg) = ansi_background {
                    bg.render_background(frame, layout.content_rect, background_fade);
                }

                // Render content lines.
                frame.render_widget(
                    Paragraph::new(line_output.lines.clone())
                        .block(Block::default().borders(Borders::NONE)),
                    layout.content_rect,
                );

                // Render cursor.
                if !hide_cursor {
                    if let Some((x, y)) = line_output.cursor {
                        frame.set_cursor_position((x, y));
                    } else if let Some(last_end) = line_output.last_line_end {
                        if !last_end.terminated_with_newline {
                            frame.set_cursor_position(last_end.pos);
                        }
                    }
                }

                split_areas.push((
                    split_id,
                    buffer_id,
                    layout.content_rect,
                    layout.scrollbar_rect,
                    0,
                    0,
                ));
            }
        }

        split_areas
    }

    fn split_layout(area: Rect) -> SplitLayout {
        let tabs_height = 1;
        let scrollbar_width = 1;
        let tabs_rect = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: tabs_height,
        };
        let content_rect = Rect {
            x: area.x,
            y: area.y + tabs_height,
            width: area.width.saturating_sub(scrollbar_width),
            height: area.height.saturating_sub(tabs_height),
        };
        let scrollbar_rect = Rect {
            x: area.x + area.width.saturating_sub(scrollbar_width),
            y: area.y + tabs_height,
            width: scrollbar_width,
            height: area.height.saturating_sub(tabs_height),
        };
        SplitLayout {
            tabs_rect,
            content_rect,
            scrollbar_rect,
        }
    }

    fn split_buffers_for_tabs(
        split_view_states: Option<&HashMap<SplitId, crate::split::SplitViewState>>,
        split_id: SplitId,
        buffer_id: BufferId,
    ) -> (Vec<BufferId>, usize) {
        if let Some(states) = split_view_states {
            if let Some(view_state) = states.get(&split_id) {
                return (view_state.open_buffers.clone(), view_state.tab_scroll_offset);
            }
        }
        (vec![buffer_id], 0)
    }

    /// Stub: Temporarily set split state for rendering
    /// TODO: Implement proper split state management
    fn temporary_split_state(
        _state: &mut EditorState,
        _split_view_states: Option<&HashMap<SplitId, crate::split::SplitViewState>>,
        _split_id: SplitId,
        _is_active: bool,
    ) -> () {
        // This is a stub - split state is already managed via SplitViewState
    }

    /// Stub: Apply wrapping transform to tokens
    /// TODO: Implement proper line wrapping
    pub fn apply_wrapping_transform(
        tokens: Vec<ViewTokenWire>,
        _content_width: usize,
        _gutter_width: usize,
    ) -> Vec<ViewTokenWire> {
        // Stub implementation - return tokens unchanged
        // Real implementation would apply line wrapping
        tokens
    }

    fn resolve_view_preferences(
        state: &EditorState,
        split_view_states: Option<&HashMap<SplitId, crate::split::SplitViewState>>,
        split_id: SplitId,
    ) -> ViewPreferences {
        if let Some(states) = split_view_states {
            if let Some(view_state) = states.get(&split_id) {
                return ViewPreferences {
                    view_mode: view_state.view_mode.clone(),
                    compose_width: view_state.compose_width,
                    compose_column_guides: view_state.compose_column_guides.clone(),
                    view_transform: view_state.view_transform.clone(),
                };
            }
        }

        ViewPreferences {
            view_mode: state.view_mode.clone(),
            compose_width: state.compose_width,
            compose_column_guides: state.compose_column_guides.clone(),
            view_transform: state.view_transform.clone(),
        }
    }

    fn sync_viewport_to_content(state: &mut EditorState, content_rect: Rect) {
        state.viewport.width = content_rect.width;
        state.viewport.height = content_rect.height;
    }

    /// Build base tokens for a viewport (simplified).
    pub fn build_base_tokens_for_hook(
        buffer: &mut Buffer,
        _top_view_line: usize,
        estimated_line_length: usize,
        visible_count: usize,
    ) -> Vec<crate::plugin_api::ViewTokenWire> {
        // Simplified: return the current visible window as a single token.
        let len = buffer.len();
        let text = buffer.get_text_range_mut(0, len).unwrap_or_default();
        let content = String::from_utf8_lossy(&text).into_owned();
        vec![crate::plugin_api::ViewTokenWire {
            source_offset: Some(0),
            kind: crate::plugin_api::ViewTokenWireKind::Text(content),
            style: None,
        }]
    }

    fn build_contexts(
        state: &EditorState,
        layout: &Layout,
        gutter_width: usize,
    ) -> (SelectionContext, DecorationContext) {
        let cursor_positions = state.cursor_positions(layout, gutter_width);
        let primary_cursor_position = cursor_positions
            .first()
            .copied()
            .unwrap_or((0, 0));

        let selection = SelectionContext {
            ranges: Vec::new(),
            block_rects: Vec::new(),
            cursor_positions,
            primary_cursor_position,
        };

        let decorations = DecorationContext {
            highlight_spans: Vec::new(),
            semantic_spans: Vec::new(),
            viewport_overlays: Vec::new(),
            virtual_text_lookup: HashMap::new(),
            diagnostic_lines: HashSet::new(),
        };

        (selection, decorations)
    }

    fn render_lines(input: LineRenderInput) -> LineRenderOutput {
        let mut lines_out = Vec::new();
        let mut cursor_pos: Option<(u16, u16)> = None;
        let mut last_line_end: Option<LastLineEnd> = None;

        let mut rendered = 0usize;
        for (idx, view_line) in input
            .view_lines
            .iter()
            .skip(input.view_anchor.start_line_idx)
            .take(input.visible_line_count)
            .enumerate()
        {
            let global_line_idx = input.view_anchor.start_line_idx + idx;
            let gutter = if should_show_line_number(view_line) {
                format!("{:>4} │ ", global_line_idx + 1)
            } else {
                "      │ ".to_string()
            };
            let gutter_len = gutter.len();  // Save length before moving
            let mut spans = vec![Span::styled(
                gutter,
                Style::default()
                    .fg(input.theme.line_number_fg)
                    .bg(input.theme.line_number_bg),
            )];

            spans.push(Span::styled(
                view_line.text.clone(),
                Style::default().fg(input.theme.editor_fg),
            ));

            let line = Line::from(spans);
            lines_out.push(line);

            if input.is_active && cursor_pos.is_none() {
                let primary = input.state.cursors.primary();
                if primary.position.view_line == global_line_idx {
                    cursor_pos = Some((
                        input.gutter_width as u16 + primary.position.column as u16,
                        idx as u16 + input.render_area.y,
                    ));
                }
            }

            last_line_end = Some(LastLineEnd {
                pos: (
                    (gutter_len + view_line.text.len()) as u16,
                    idx as u16 + input.render_area.y,
                ),
                terminated_with_newline: view_line.ends_with_newline,
            });

            rendered += 1;
        }

        LineRenderOutput {
            lines: lines_out,
            cursor: cursor_pos,
            last_line_end,
            content_lines_rendered: rendered,
        }
    }
}

/// Should this line show a line number in the gutter?
fn should_show_line_number(view_line: &ViewLine) -> bool {
    match view_line.line_start {
        LineStart::Beginning | LineStart::AfterSourceNewline => true,
        LineStart::AfterInjectedNewline => view_line
            .char_mappings
            .iter()
            .any(|m| m.is_some()),
        LineStart::AfterBreak => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::view_pipeline::{ViewTokenWire, ViewTokenWireKind};

    fn simple_layout(text: &str) -> Layout {
        let token = ViewTokenWire {
            source_offset: Some(0),
            kind: ViewTokenWireKind::Text(text.to_string()),
            style: None,
        };
        Layout::from_tokens(&[token], 0..text.len())
    }

    #[test]
    fn should_show_line_numbers_for_source_lines() {
        let layout = simple_layout("a\nb");
        assert!(should_show_line_number(&layout.lines[0]));
    }
}
