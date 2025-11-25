use crate::cursor::ViewPosition;
use crate::ui::view_pipeline::Layout;

/// Convert a view position to a source byte if available.
/// Returns None if view coordinates are not resolved.
pub fn view_pos_to_source(layout: &Layout, pos: &ViewPosition) -> Option<usize> {
    let view_line = pos.view_line?;
    let column = pos.column?;
    layout.view_position_to_source_byte(view_line, column)
}

/// Convert a source byte to the nearest view position (using optional preferred col).
pub fn source_to_view_pos(
    layout: &Layout,
    source_byte: usize,
    preferred_col: Option<usize>,
) -> ViewPosition {
    let (line, col) = layout
        .source_byte_to_view_position(source_byte)
        .unwrap_or((0, 0));
    let col = preferred_col.unwrap_or(col);
    ViewPosition {
        view_line: Some(line),
        column: Some(col),
        source_byte: Some(source_byte),
    }
}
