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

                        // Sync cursor view positions from source_byte using the layout.
                        // After edits, source_byte is updated but view_line/column may be stale.
                        // We must sync before ensure_visible_in_layout so scrolling goes to the right place.
                        // Note: We sync view_state.cursors since it gets copied back to state at the end.
                        //
                        // If cursor is outside layout's source_range (e.g., jumped to EOF),
                        // we need to rebuild layout centered on the cursor first.
                        // May need multiple iterations for small files where layout doesn't
                        // extend far enough from anchor_byte.
                        let mut layout = layout;
                        let primary_byte = view_state
                            .cursors
                            .primary()
                            .position
                            .source_byte
                            .unwrap_or(0);
                        let mut rebuild_attempts = 0;
                        while !layout.source_range.contains(&primary_byte)
                            && primary_byte != layout.source_range.end
                            && rebuild_attempts < 3
                        {
                            // Cursor is outside layout range - rebuild starting closer to cursor
                            // Use cursor position minus a small buffer so cursor ends up in middle/end of layout
                            let buffer_offset = estimated_line_length
                                * view_state.viewport.visible_line_count()
                                / 2;
                            // For subsequent attempts, start closer to cursor
                            let multiplier = rebuild_attempts + 1;
                            view_state.viewport.anchor_byte = primary_byte.saturating_sub(
                                buffer_offset / multiplier,
                            );
                            view_state.invalidate_layout();
                            layout = view_state
                                .ensure_layout(&mut state.buffer, estimated_line_length, wrap_params)
                                .clone();
                            rebuild_attempts += 1;
                        }

                        let cursor_ids: Vec<_> =
                            view_state.cursors.iter().map(|(id, _)| id).collect();
                        for cursor_id in cursor_ids {
                            if let Some(cursor) = view_state.cursors.get_mut(cursor_id) {
                                if let Some(byte) = cursor.position.source_byte {
                                    if let Some((view_line, column)) =
                                        layout.source_byte_to_view_position(byte)
                                    {
                                        cursor.position.view_line = Some(view_line);
                                        cursor.position.column = Some(column);
                                    }
                                }
                            }
                        }

                        let primary_cursor = *view_state.cursors.primary();
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

                // Render scrollbar
                let (thumb_start, thumb_end) = Self::render_scrollbar(
                    frame,
                    layout.scrollbar_rect,
                    &layout_for_render,
                    state.viewport.top_view_line,
                    state.viewport.visible_line_count(),
                    theme,
                );

                split_areas.push((
                    split_id,
                    buffer_id,
                    layout.content_rect,
                    layout.scrollbar_rect,
                    thumb_start,
                    thumb_end,
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

    /// Render the scrollbar and return (thumb_start, thumb_end) for mouse handling
    fn render_scrollbar(
        frame: &mut Frame,
        scrollbar_rect: Rect,
        layout: &crate::ui::view_pipeline::Layout,
        top_view_line: usize,
        visible_lines: usize,
        theme: &crate::theme::Theme,
    ) -> (usize, usize) {
        let total_lines = layout.lines.len();
        let track_height = scrollbar_rect.height as usize;

        if track_height == 0 || total_lines == 0 {
            return (0, 0);
        }

        // Calculate thumb size (minimum 1 row)
        let thumb_size = if total_lines <= visible_lines {
            track_height // Full height if content fits
        } else {
            ((visible_lines as f64 / total_lines as f64) * track_height as f64)
                .ceil()
                .max(1.0) as usize
        };

        // Calculate thumb position
        let max_scroll = total_lines.saturating_sub(visible_lines);
        let thumb_start = if max_scroll == 0 {
            0
        } else {
            let scroll_ratio = top_view_line as f64 / max_scroll as f64;
            let max_thumb_start = track_height.saturating_sub(thumb_size);
            (scroll_ratio * max_thumb_start as f64).round() as usize
        };
        let thumb_end = (thumb_start + thumb_size).min(track_height);

        // Render the scrollbar
        let buffer = frame.buffer_mut();
        for row in 0..track_height {
            let y = scrollbar_rect.y + row as u16;
            let x = scrollbar_rect.x;

            if row >= thumb_start && row < thumb_end {
                // Thumb character
                buffer[(x, y)].set_char('█');
                buffer[(x, y)].set_fg(theme.scrollbar_thumb_fg);
            } else {
                // Track character
                buffer[(x, y)].set_char('│');
                buffer[(x, y)].set_fg(theme.scrollbar_track_fg);
            }
        }

        (thumb_start, thumb_end)
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

    /// Placeholder for split state setup during rendering.
    /// Split state is managed via SplitViewState - this is kept for potential
    /// future extension of per-split rendering configuration.
    #[allow(dead_code)]
    fn temporary_split_state(
        _state: &mut EditorState,
        _split_view_states: Option<&HashMap<SplitId, crate::split::SplitViewState>>,
        _split_id: SplitId,
        _is_active: bool,
    ) {
        // No-op: split state is managed via SplitViewState
    }

    /// Apply line wrapping transform to tokens
    ///
    /// Breaks long lines into multiple visual lines by inserting Break tokens.
    /// Accounts for gutter width when calculating available space.
    pub fn apply_wrapping_transform(
        tokens: Vec<ViewTokenWire>,
        content_width: usize,
        gutter_width: usize,
    ) -> Vec<ViewTokenWire> {
        use crate::ansi::visible_char_count;
        use crate::plugin_api::ViewTokenWireKind;

        let mut wrapped = Vec::new();
        let mut current_line_width = 0;

        // Calculate available width (accounting for gutter on first line only)
        let available_width = content_width.saturating_sub(gutter_width);

        if available_width == 0 {
            return tokens;
        }

        for token in tokens {
            match &token.kind {
                ViewTokenWireKind::Newline => {
                    // Real newlines always break the line
                    wrapped.push(token);
                    current_line_width = 0;
                }
                ViewTokenWireKind::Break => {
                    // Preserve existing breaks
                    wrapped.push(token);
                    current_line_width = 0;
                }
                ViewTokenWireKind::Space => {
                    // Spaces are treated as single-width characters
                    if current_line_width >= available_width {
                        wrapped.push(ViewTokenWire {
                            source_offset: None,
                            kind: ViewTokenWireKind::Break,
                            style: None,
                        });
                        current_line_width = 0;
                    }
                    wrapped.push(token);
                    current_line_width += 1;
                }
                ViewTokenWireKind::Text(text) => {
                    // Use visible character count (excludes ANSI escape sequences)
                    let text_len = visible_char_count(text);

                    // If this token would exceed line width, insert Break before it
                    if current_line_width > 0 && current_line_width + text_len > available_width {
                        wrapped.push(ViewTokenWire {
                            source_offset: None,
                            kind: ViewTokenWireKind::Break,
                            style: None,
                        });
                        current_line_width = 0;
                    }

                    // If token is longer than available width and doesn't contain ANSI codes, split it
                    if text_len > available_width && !crate::ansi::contains_ansi_codes(text) {
                        let chars: Vec<char> = text.chars().collect();
                        let mut char_idx = 0;
                        let source_base = token.source_offset;

                        while char_idx < chars.len() {
                            let remaining = chars.len() - char_idx;
                            let chunk_size =
                                remaining.min(available_width.saturating_sub(current_line_width));

                            if chunk_size == 0 {
                                // Need to break to next line
                                wrapped.push(ViewTokenWire {
                                    source_offset: None,
                                    kind: ViewTokenWireKind::Break,
                                    style: None,
                                });
                                current_line_width = 0;
                                continue;
                            }

                            let chunk: String =
                                chars[char_idx..char_idx + chunk_size].iter().collect();
                            let chunk_source = source_base.map(|b| b + char_idx);

                            wrapped.push(ViewTokenWire {
                                source_offset: chunk_source,
                                kind: ViewTokenWireKind::Text(chunk),
                                style: token.style.clone(),
                            });

                            current_line_width += chunk_size;
                            char_idx += chunk_size;

                            // If we filled the line, break
                            if current_line_width >= available_width {
                                wrapped.push(ViewTokenWire {
                                    source_offset: None,
                                    kind: ViewTokenWireKind::Break,
                                    style: None,
                                });
                                current_line_width = 0;
                            }
                        }
                    } else {
                        wrapped.push(token);
                        current_line_width += text_len;
                    }
                }
            }
        }

        wrapped
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

    fn build_base_tokens(
        buffer: &mut Buffer,
        top_byte: usize,
        estimated_line_length: usize,
        visible_count: usize,
    ) -> Vec<crate::plugin_api::ViewTokenWire> {
        use crate::plugin_api::{ViewTokenWire, ViewTokenWireKind};

        let mut tokens = Vec::new();
        let mut iter = buffer.line_iterator(top_byte, estimated_line_length);
        let mut lines_seen = 0usize;
        let max_lines = visible_count.saturating_add(4);

        while lines_seen < max_lines {
            if let Some((line_start, line_content)) = iter.next() {
                let mut byte_offset = 0usize;
                for ch in line_content.chars() {
                    let ch_len = ch.len_utf8();
                    let source_offset = Some(line_start + byte_offset);

                    match ch {
                        '\n' => {
                            tokens.push(ViewTokenWire {
                                source_offset,
                                kind: ViewTokenWireKind::Newline,
                                style: None,
                            });
                        }
                        ' ' => {
                            tokens.push(ViewTokenWire {
                                source_offset,
                                kind: ViewTokenWireKind::Space,
                                style: None,
                            });
                        }
                        _ => {
                            // Accumulate consecutive non-space/non-newline chars into Text tokens
                            if let Some(last) = tokens.last_mut() {
                                if let ViewTokenWireKind::Text(ref mut s) = last.kind {
                                    // Extend existing Text token if contiguous
                                    let expected_offset = last.source_offset.map(|o| o + s.len());
                                    if expected_offset == Some(line_start + byte_offset) {
                                        s.push(ch);
                                        byte_offset += ch_len;
                                        continue;
                                    }
                                }
                            }
                            tokens.push(ViewTokenWire {
                                source_offset,
                                kind: ViewTokenWireKind::Text(ch.to_string()),
                                style: None,
                            });
                        }
                    }
                    byte_offset += ch_len;
                }
                lines_seen += 1;
            } else {
                break;
            }
        }

        // Handle empty buffer
        if tokens.is_empty() {
            tokens.push(ViewTokenWire {
                source_offset: Some(top_byte),
                kind: ViewTokenWireKind::Text(String::new()),
                style: None,
            });
        }

        tokens
    }

    /// Public wrapper for building base tokens - used by render.rs for the view_transform_request hook
    pub fn build_base_tokens_for_hook(
        buffer: &mut Buffer,
        top_byte: usize,
        estimated_line_length: usize,
        visible_count: usize,
    ) -> Vec<crate::plugin_api::ViewTokenWire> {
        Self::build_base_tokens(buffer, top_byte, estimated_line_length, visible_count)
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

        // Build selection ranges from all cursors
        let mut ranges = Vec::new();
        for (_cursor_id, cursor) in state.cursors.iter() {
            if let Some(selection) = cursor.selection_range() {
                // Convert selection view positions to source byte ranges
                if let (Some(start_byte), Some(end_byte)) = (
                    selection.start.source_byte,
                    selection.end.source_byte,
                ) {
                    ranges.push(start_byte..end_byte);
                }
            }
        }

        let selection = SelectionContext {
            ranges,
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

        // Helper to check if a source byte is within any selection range
        let is_selected = |byte: usize| -> bool {
            input.selection.ranges.iter().any(|range| range.contains(&byte))
        };

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
            let gutter_len = gutter.chars().count();  // Use char count, not byte length (│ is multi-byte)
            let mut spans = vec![Span::styled(
                gutter,
                Style::default()
                    .fg(input.theme.line_number_fg)
                    .bg(input.theme.line_number_bg),
            )];

            // Build content spans with selection highlighting
            let text_chars: Vec<char> = view_line.text.chars().collect();
            let mut char_idx = 0;
            let mut current_span_start = 0;
            let mut current_is_selected = false;

            // Determine initial selection state
            if !text_chars.is_empty() && char_idx < view_line.char_mappings.len() {
                if let Some(byte) = view_line.char_mappings[char_idx] {
                    current_is_selected = is_selected(byte);
                }
            }

            for (i, _ch) in text_chars.iter().enumerate() {
                let this_selected = if i < view_line.char_mappings.len() {
                    view_line.char_mappings[i]
                        .map(|b| is_selected(b))
                        .unwrap_or(false)
                } else {
                    false
                };

                // If selection state changed, emit a span for the previous segment
                if this_selected != current_is_selected && i > current_span_start {
                    let segment: String = text_chars[current_span_start..i].iter().collect();
                    let style = if current_is_selected {
                        Style::default()
                            .fg(input.theme.editor_fg)
                            .bg(input.theme.selection_bg)
                    } else {
                        Style::default().fg(input.theme.editor_fg)
                    };
                    spans.push(Span::styled(segment, style));
                    current_span_start = i;
                    current_is_selected = this_selected;
                }
                char_idx = i + 1;
            }

            // Emit final segment
            if current_span_start < text_chars.len() {
                let segment: String = text_chars[current_span_start..].iter().collect();
                let style = if current_is_selected {
                    Style::default()
                        .fg(input.theme.editor_fg)
                        .bg(input.theme.selection_bg)
                } else {
                    Style::default().fg(input.theme.editor_fg)
                };
                spans.push(Span::styled(segment, style));
            }

            let line = Line::from(spans);
            lines_out.push(line);

            if input.is_active && cursor_pos.is_none() {
                let primary = input.state.cursors.primary();
                if let (Some(view_line), Some(column)) = (primary.position.view_line, primary.position.column) {
                    if view_line == global_line_idx {
                        // Account for horizontal scroll (left_column) when calculating screen x
                        let left_col = input.state.viewport.left_column;
                        let content_width = input.render_area.width.saturating_sub(gutter_len as u16);

                        // Only show cursor if it's within the visible horizontal range
                        if column >= left_col && column < left_col + content_width as usize {
                            let adjusted_col = column.saturating_sub(left_col);
                            cursor_pos = Some((
                                input.render_area.x + gutter_len as u16 + adjusted_col as u16,
                                idx as u16 + input.render_area.y,
                            ));
                        }
                    }
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
    use crate::plugin_api::{ViewTokenWire, ViewTokenWireKind};

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
