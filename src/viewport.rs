use crate::cursor::Cursor;
use crate::line_wrapping::{char_position_to_segment, wrap_line, WrapConfig};
use crate::text_buffer::Buffer;
use crate::ui::view_pipeline::{Layout, ViewLine};

/// View-centric viewport (top_view_line is authoritative).
#[derive(Debug, Clone)]
pub struct Viewport {
    /// View line at the top of the viewport.
    pub top_view_line: usize,
    /// Optional anchor source byte (hint for layout stabilization).
    pub anchor_byte: usize,

    /// Left column offset (horizontal scroll position).
    pub left_column: usize,

    /// Terminal dimensions.
    pub width: u16,
    pub height: u16,

    /// Scroll offset (lines to keep visible above/below cursor).
    pub scroll_offset: usize,

    /// Horizontal scroll offset (columns to keep visible left/right of cursor).
    pub horizontal_scroll_offset: usize,

    /// Whether line wrapping is enabled. When true, horizontal scrolling is disabled.
    pub line_wrap_enabled: bool,

    /// Whether viewport needs synchronization with cursor positions.
    needs_sync: bool,
}

impl Viewport {
    /// Create a new viewport.
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            top_view_line: 0,
            anchor_byte: 0,
            left_column: 0,
            width,
            height,
            scroll_offset: 3,
            horizontal_scroll_offset: 5,
            line_wrap_enabled: false,
            needs_sync: false,
        }
    }

    /// Set the scroll offset.
    pub fn set_scroll_offset(&mut self, offset: usize) {
        self.scroll_offset = offset;
    }

    /// Update terminal dimensions.
    pub fn resize(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
    }

    /// Get the number of visible lines.
    pub fn visible_line_count(&self) -> usize {
        self.height as usize
    }

    /// Calculate gutter width based on buffer length (fallback heuristic).
    pub fn gutter_width(&self, buffer: &Buffer) -> usize {
        let buffer_len = buffer.len();
        let estimated_lines = (buffer_len / 80).max(1);
        let digits = if estimated_lines == 0 {
            1
        } else {
            ((estimated_lines as f64).log10().floor() as usize) + 1
        };
        1 + digits.max(4) + 3
    }

    /// Stabilize scroll position after a layout rebuild using anchor_byte if possible.
    pub fn stabilize_after_layout_change(&mut self, layout: &Layout) {
        if layout.lines.is_empty() {
            self.top_view_line = 0;
            self.anchor_byte = 0;
            return;
        }

        if let Some(line) = layout.view_line_for_byte(self.anchor_byte) {
            self.top_view_line = line;
        } else {
            self.top_view_line = layout.find_nearest_view_line(self.anchor_byte);
        }
    }

    /// Scroll by a relative line offset (view lines).
    pub fn scroll_layout(&mut self, offset: isize, layout: &Layout) {
        let max_top = layout.max_top_line(self.visible_line_count());
        let target = (self.top_view_line as isize + offset).max(0) as usize;
        self.top_view_line = target.min(max_top);
        if let Some(byte) = layout.get_source_byte_for_line(self.top_view_line) {
            self.anchor_byte = byte;
        }
    }

    /// Ensure a cursor is visible within the layout (view-based).
    /// If cursor view coordinates are not resolved, no scrolling is performed.
    pub fn ensure_visible_in_layout(
        &mut self,
        cursor: &Cursor,
        layout: &Layout,
        gutter_width: usize,
    ) {
        // Only scroll if view coordinates are resolved
        let Some(cursor_line) = cursor.position.view_line else {
            return;
        };

        let visible = self.visible_line_count();
        let top = self.top_view_line;
        let bottom = top.saturating_add(visible.saturating_sub(1));

        if cursor_line < top + self.scroll_offset {
            let new_top = cursor_line.saturating_sub(self.scroll_offset);
            self.top_view_line = new_top;
        } else if cursor_line > bottom.saturating_sub(self.scroll_offset) {
            // Scroll so cursor is at position (visible - 1 - scroll_offset) from top,
            // leaving scroll_offset lines visible below the cursor.
            let new_top = cursor_line
                .saturating_add(self.scroll_offset)
                .saturating_sub(visible.saturating_sub(1));
            self.top_view_line = new_top;
        }

        let max_top = layout.max_top_line(visible);
        self.top_view_line = self.top_view_line.min(max_top);

        if let Some(byte) = layout.get_source_byte_for_line(self.top_view_line) {
            self.anchor_byte = byte;
        }

        // Horizontal scroll for wrapped vs unwrapped is left as-is; reuse existing left_column.
        let Some(cursor_col) = cursor.position.column else {
            return;
        };
        let content_width = self.width as usize - gutter_width;

        if cursor_col < self.left_column + self.horizontal_scroll_offset {
            self.left_column = cursor_col.saturating_sub(self.horizontal_scroll_offset);
        } else if cursor_col
            > self
                .left_column
                .saturating_add(content_width)
                .saturating_sub(self.horizontal_scroll_offset + 1)
        {
            let over = cursor_col.saturating_sub(content_width.saturating_sub(1));
            self.left_column = over.saturating_add(self.horizontal_scroll_offset);
        }
    }

    /// Mark the viewport as needing sync with cursor.
    pub fn mark_needs_sync(&mut self) {
        self.needs_sync = true;
    }

    /// Check if sync is needed.
    pub fn needs_sync(&self) -> bool {
        self.needs_sync
    }

    /// Sync viewport with cursor using layout (clears the needs_sync flag).
    pub fn sync_with_cursor(&mut self, layout: &Layout, cursor: &Cursor, gutter_width: usize) {
        if self.needs_sync {
            self.ensure_visible_in_layout(cursor, layout, gutter_width);
            self.needs_sync = false;
        }
    }

    /// Compute cursor screen position from layout/view.
    /// Returns (0, 0) if view coordinates are not resolved.
    pub fn cursor_screen_position_layout(
        &self,
        _layout: &Layout,
        cursor: &Cursor,
        gutter_width: usize,
    ) -> (u16, u16) {
        let Some(view_line) = cursor.position.view_line else {
            return (0, 0);
        };
        let Some(column) = cursor.position.column else {
            return (0, 0);
        };
        let row = view_line.saturating_sub(self.top_view_line);
        let col = column
            .saturating_sub(self.left_column)
            .saturating_add(gutter_width);

        (col as u16, row as u16)
    }

    /// Basic cursor screen position using viewport offsets (no layout/gutter).
    /// Returns (0, 0) if view coordinates are not resolved.
    pub fn cursor_screen_position(&self, _buffer: &mut Buffer, cursor: &Cursor) -> (u16, u16) {
        let Some(view_line) = cursor.position.view_line else {
            return (0, 0);
        };
        let Some(column) = cursor.position.column else {
            return (0, 0);
        };
        let row = view_line.saturating_sub(self.top_view_line);
        let col = column.saturating_sub(self.left_column);
        (col as u16, row as u16)
    }

    /// Ensure a specific view line is visible.
    pub fn ensure_line_visible(&mut self, layout: &Layout, view_line: usize) {
        if view_line < self.top_view_line {
            self.top_view_line = view_line;
        } else {
            let bottom = self.top_view_line.saturating_add(self.visible_line_count().saturating_sub(1));
            if view_line > bottom {
                let delta = view_line.saturating_sub(bottom);
                self.scroll_layout(delta as isize, layout);
                return;
            }
        }
        let max_top = layout.max_top_line(self.visible_line_count());
        self.top_view_line = self.top_view_line.min(max_top);
        if let Some(byte) = layout.get_source_byte_for_line(self.top_view_line) {
            self.anchor_byte = byte;
        }
    }

    /// Compatibility: ensure visibility using view coords only (no layout).
    /// If cursor view coordinates are not resolved, no scrolling is performed.
    pub fn ensure_visible(&mut self, _buffer: &mut Buffer, cursor: &Cursor) {
        // Only scroll if view coordinates are resolved
        let Some(cursor_line) = cursor.position.view_line else {
            return;
        };

        let visible = self.visible_line_count();
        let top = self.top_view_line;
        let bottom = top.saturating_add(visible.saturating_sub(1));

        if cursor_line < top + self.scroll_offset {
            self.top_view_line = cursor_line.saturating_sub(self.scroll_offset);
        } else if cursor_line > bottom.saturating_sub(self.scroll_offset) {
            // Scroll so cursor is at position (visible - 1 - scroll_offset) from top,
            // leaving scroll_offset lines visible below the cursor.
            self.top_view_line = cursor_line
                .saturating_add(self.scroll_offset)
                .saturating_sub(visible.saturating_sub(1));
        }
        if let Some(src) = cursor.position.source_byte {
            self.anchor_byte = src;
        }
        // Horizontal scrolling based on column only.
        let Some(cursor_col) = cursor.position.column else {
            return;
        };
        if cursor_col < self.left_column + self.horizontal_scroll_offset {
            self.left_column = cursor_col.saturating_sub(self.horizontal_scroll_offset);
        } else if cursor_col
            > self
                .left_column
                .saturating_add(self.width as usize)
                .saturating_sub(self.horizontal_scroll_offset + 1)
        {
            let over = cursor_col.saturating_sub(self.width as usize).saturating_add(1);
            self.left_column = over.saturating_add(self.horizontal_scroll_offset);
        }
    }

    /// Ensure a column is visible horizontally.
    pub fn ensure_column_visible(&mut self, column: usize, content_width: usize) {
        if column < self.left_column {
            self.left_column = column;
        } else if column >= self.left_column.saturating_add(content_width) {
            self.left_column = column.saturating_sub(content_width.saturating_sub(1));
        }
    }

    /// Ensure all cursors are visible.
    pub fn ensure_cursors_visible<'a>(
        &mut self,
        layout: &Layout,
        cursors: impl Iterator<Item = &'a Cursor>,
        gutter_width: usize,
    ) {
        for cursor in cursors {
            self.ensure_visible_in_layout(cursor, layout, gutter_width);
        }
    }

    /// Scroll a prebuilt set of view lines (used in tests or simplified contexts).
    pub fn scroll_view_lines(&mut self, view_lines: &[ViewLine], line_offset: isize) {
        let max_top = view_lines
            .len()
            .saturating_sub(self.visible_line_count())
            .min(view_lines.len())
            .saturating_sub(1);
        let target = (self.top_view_line as isize + line_offset).max(0) as usize;
        self.top_view_line = target.min(max_top);
    }

    /// Scroll to a specific view line in the layout
    pub fn scroll_to(&mut self, layout: &Layout, view_line: usize) {
        // Clamp to valid range
        let max_line = layout.lines.len().saturating_sub(1);
        let target_line = view_line.min(max_line);

        // Ensure we don't scroll past the point where the last line is at the top
        let max_top = layout
            .lines
            .len()
            .saturating_sub(self.visible_line_count());
        self.top_view_line = target_line.min(max_top);

        // Update anchor_byte if we have a valid source mapping for this line
        if let Some(line) = layout.lines.get(target_line) {
            if let Some(byte) = line.char_mappings.iter().find_map(|m| *m) {
                self.anchor_byte = byte;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_api::{ViewTokenWire, ViewTokenWireKind};
    use crate::text_buffer::Buffer;

    fn simple_layout(text: &str) -> Layout {
        let token = ViewTokenWire {
            source_offset: Some(0),
            kind: ViewTokenWireKind::Text(text.to_string()),
            style: None,
        };
        Layout::from_tokens(&[token], 0..text.len())
    }

    fn cursor_at(line: usize, col: usize, src: Option<usize>) -> Cursor {
        Cursor::new(crate::cursor::ViewPosition {
            view_line: Some(line),
            column: Some(col),
            source_byte: src,
        })
    }

    #[test]
    fn ensures_cursor_visibility_vertical() {
        let layout = simple_layout("line1\nline2\nline3\nline4\nline5\nline6\nline7\n");
        let mut vp = Viewport::new(80, 3);
        vp.top_view_line = 0;
        let cursor = cursor_at(5, 0, Some(5));
        vp.ensure_visible_in_layout(&cursor, &layout, 0);
        assert!(vp.top_view_line <= 5);
        assert!(vp.top_view_line + vp.visible_line_count() > 5);
    }

    #[test]
    fn scroll_layout_respects_bounds() {
        let layout = simple_layout("line1\nline2\nline3\n");
        let mut vp = Viewport::new(80, 2);
        vp.scroll_layout(10, &layout);
        let max_top = layout.max_top_line(vp.visible_line_count());
        assert_eq!(vp.top_view_line, max_top);
        vp.scroll_layout(-10, &layout);
        assert_eq!(vp.top_view_line, 0);
    }

    #[test]
    fn cursor_screen_position_accounts_for_offsets() {
        let _layout = simple_layout("line1\nline2\n");
        let mut buffer = Buffer::from_str_test("line1\nline2\n");
        let mut vp = Viewport::new(10, 2);
        vp.top_view_line = 1;
        vp.left_column = 2;
        let cursor = cursor_at(1, 5, Some(5));
        let (x, y) = vp.cursor_screen_position(&mut buffer, &cursor);
        assert_eq!(y, 0); // second line now at top
        assert_eq!(x, (5usize.saturating_sub(2)) as u16);
    }
}
