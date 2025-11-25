use crate::cursor::ViewPosition;
use crate::ui::view_pipeline::Layout;
use std::ops::Range;

/// Map a view-position range to a buffer byte range, if both endpoints have source bytes.
pub fn view_range_to_buffer_range(
    layout: &Layout,
    start: &ViewPosition,
    end: &ViewPosition,
) -> Option<Range<usize>> {
    match (start.source_byte, end.source_byte) {
        (Some(s), Some(e)) => Some(s.min(e)..s.max(e)),
        _ => None,
    }
}

/// Map a single view position to a buffer byte, if available.
pub fn view_pos_to_buffer_byte(layout: &Layout, pos: &ViewPosition) -> Option<usize> {
    pos.source_byte.or_else(|| {
        let view_line = pos.view_line?;
        let column = pos.column?;
        layout.view_position_to_source_byte(view_line, column)
    })
}
