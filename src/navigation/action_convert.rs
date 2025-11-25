use crate::cursor::{Cursor, ViewPosition};
use crate::event::{Event, ViewEventPosition, ViewEventRange};
use crate::keybindings::Action;
use crate::navigation::edit_map::{view_pos_to_buffer_byte, view_range_to_buffer_range};
use crate::navigation::layout_nav;
use crate::text_buffer::Buffer;
use crate::ui::view_pipeline::Layout;
use crate::viewport::Viewport;

/// Convert an Action into Events using view-centric cursors and layout.
/// Returns None for actions that do nothing in the current state.
pub fn action_to_events(
    cursors: &mut crate::cursor::Cursors,
    layout: &Layout,
    viewport: &mut Viewport,
    buffer: &Buffer,
    action: Action,
) -> Option<Vec<Event>> {
    let mut events = Vec::new();

    match action {
        Action::MoveLeft => {
            for (id, cursor) in cursors.iter() {
                let new_pos = layout_nav::move_horizontal(layout, &cursor.position, -1);
                events.extend(move_cursor_event(id, cursor, new_pos, None));
            }
        }
        Action::MoveRight => {
            for (id, cursor) in cursors.iter() {
                let new_pos = layout_nav::move_horizontal(layout, &cursor.position, 1);
                events.extend(move_cursor_event(id, cursor, new_pos, None));
            }
        }
        Action::MoveUp => {
            for (id, cursor) in cursors.iter() {
                let pref = cursor.preferred_visual_column.or(cursor.position.column);
                let new_pos = layout_nav::move_vertical(layout, &cursor.position, pref, -1);
                events.extend(move_cursor_event(id, cursor, new_pos, pref));
            }
        }
        Action::MoveDown => {
            for (id, cursor) in cursors.iter() {
                let pref = cursor.preferred_visual_column.or(cursor.position.column);
                let new_pos = layout_nav::move_vertical(layout, &cursor.position, pref, 1);
                events.extend(move_cursor_event(id, cursor, new_pos, pref));
            }
        }
        Action::MoveLineStart => {
            for (id, cursor) in cursors.iter() {
                let new_pos = layout_nav::move_line_start(layout, &cursor.position);
                events.extend(move_cursor_event(id, cursor, new_pos, Some(0)));
            }
        }
        Action::MoveLineEnd => {
            for (id, cursor) in cursors.iter() {
                let new_pos = layout_nav::move_line_end(layout, &cursor.position);
                events.extend(move_cursor_event(id, cursor, new_pos, new_pos.column));
            }
        }
        Action::MovePageUp => {
            for (id, cursor) in cursors.iter() {
                let pref = cursor.preferred_visual_column.or(cursor.position.column);
                let new_pos = layout_nav::move_page(layout, &cursor.position, viewport, -1);
                events.extend(move_cursor_event(id, cursor, new_pos, pref));
            }
        }
        Action::MovePageDown => {
            for (id, cursor) in cursors.iter() {
                let pref = cursor.preferred_visual_column.or(cursor.position.column);
                let new_pos = layout_nav::move_page(layout, &cursor.position, viewport, 1);
                events.extend(move_cursor_event(id, cursor, new_pos, pref));
            }
        }
        Action::MoveDocumentStart => {
            // Position (0, 0, 0) is always resolved - it's the start of the document
            for (id, cursor) in cursors.iter() {
                let new_pos = ViewPosition::resolved(0, 0, 0);
                events.extend(move_cursor_event(id, cursor, new_pos, Some(0)));
            }
        }
        Action::MoveDocumentEnd => {
            // source_byte is known; view coords need resolution from layout
            let end_byte = buffer.len();
            for (id, cursor) in cursors.iter() {
                let new_pos = ViewPosition::from_source(end_byte);
                events.extend(move_cursor_event(id, cursor, new_pos, Some(0)));
            }
        }
        Action::SelectLeft => {
            for (id, cursor) in cursors.iter() {
                let new_pos = layout_nav::move_horizontal(layout, &cursor.position, -1);
                events.extend(move_cursor_event_with_anchor(id, cursor, new_pos));
            }
        }
        Action::SelectRight => {
            for (id, cursor) in cursors.iter() {
                let new_pos = layout_nav::move_horizontal(layout, &cursor.position, 1);
                events.extend(move_cursor_event_with_anchor(id, cursor, new_pos));
            }
        }
        Action::SelectUp => {
            for (id, cursor) in cursors.iter() {
                let pref = cursor.preferred_visual_column.or(cursor.position.column);
                let new_pos = layout_nav::move_vertical(layout, &cursor.position, pref, -1);
                events.extend(move_cursor_event_with_anchor(id, cursor, new_pos));
            }
        }
        Action::SelectDown => {
            for (id, cursor) in cursors.iter() {
                let pref = cursor.preferred_visual_column.or(cursor.position.column);
                let new_pos = layout_nav::move_vertical(layout, &cursor.position, pref, 1);
                events.extend(move_cursor_event_with_anchor(id, cursor, new_pos));
            }
        }
        Action::SelectLineStart => {
            for (id, cursor) in cursors.iter() {
                let new_pos = layout_nav::move_line_start(layout, &cursor.position);
                events.extend(move_cursor_event_with_anchor(id, cursor, new_pos));
            }
        }
        Action::SelectLineEnd => {
            for (id, cursor) in cursors.iter() {
                let new_pos = layout_nav::move_line_end(layout, &cursor.position);
                events.extend(move_cursor_event_with_anchor(id, cursor, new_pos));
            }
        }
        Action::SelectDocumentStart => {
            // Position (0, 0, 0) is always resolved
            for (id, cursor) in cursors.iter() {
                let new_pos = ViewPosition::resolved(0, 0, 0);
                events.extend(move_cursor_event_with_anchor(id, cursor, new_pos));
            }
        }
        Action::SelectDocumentEnd => {
            // source_byte is known; view coords need resolution from layout
            let end_byte = buffer.len();
            for (id, cursor) in cursors.iter() {
                let new_pos = ViewPosition::from_source(end_byte);
                events.extend(move_cursor_event_with_anchor(id, cursor, new_pos));
            }
        }
        Action::SelectPageUp => {
            for (id, cursor) in cursors.iter() {
                let pref = cursor.preferred_visual_column.or(cursor.position.column);
                let new_pos = layout_nav::move_page(layout, &cursor.position, viewport, -1);
                events.extend(move_cursor_event_with_anchor(id, cursor, new_pos));
            }
        }
        Action::SelectPageDown => {
            for (id, cursor) in cursors.iter() {
                let pref = cursor.preferred_visual_column.or(cursor.position.column);
                let new_pos = layout_nav::move_page(layout, &cursor.position, viewport, 1);
                events.extend(move_cursor_event_with_anchor(id, cursor, new_pos));
            }
        }
        Action::SelectAll => {
            // Select from start to end of document
            let first_pos = ViewPosition {
                view_line: Some(0),
                column: Some(0),
                source_byte: layout.get_source_byte_for_line(0),
            };
            let last_line = layout.lines.len().saturating_sub(1);
            let last_col = layout.lines.get(last_line).map(|l| l.char_mappings.len()).unwrap_or(0);
            let last_pos = ViewPosition {
                view_line: Some(last_line),
                column: Some(last_col),
                source_byte: layout.get_source_byte_for_line(last_line),
            };
            // Move all cursors to start, with anchor at end
            for (id, _cursor) in cursors.iter() {
                if let (Some(first_ev), Some(last_ev)) = (view_pos_to_event(first_pos), view_pos_to_event(last_pos)) {
                    events.push(Event::MoveCursor {
                        cursor_id: id,
                        old_position: first_ev,
                        new_position: first_ev,
                        old_anchor: None,
                        new_anchor: Some(last_ev),
                        old_sticky_column: None,
                        new_sticky_column: Some(0),
                    });
                }
            }
        }
        Action::SelectLine => {
            for (id, cursor) in cursors.iter() {
                let Some(line_idx) = cursor.position.view_line else { continue };
                let line_start = ViewPosition {
                    view_line: Some(line_idx),
                    column: Some(0),
                    source_byte: layout.view_position_to_source_byte(line_idx, 0),
                };
                let line_len = layout.lines.get(line_idx).map(|l| l.char_mappings.len()).unwrap_or(0);
                let line_end = ViewPosition {
                    view_line: Some(line_idx),
                    column: Some(line_len),
                    source_byte: layout.view_position_to_source_byte(line_idx, line_len),
                };
                if let (Some(old_pos), Some(new_pos), Some(new_anc)) = (
                    view_pos_to_event(cursor.position),
                    view_pos_to_event(line_start),
                    view_pos_to_event(line_end),
                ) {
                    events.push(Event::MoveCursor {
                        cursor_id: id,
                        old_position: old_pos,
                        new_position: new_pos,
                        old_anchor: cursor.anchor.and_then(view_pos_to_event),
                        new_anchor: Some(new_anc),
                        old_sticky_column: cursor.preferred_visual_column,
                        new_sticky_column: Some(0),
                    });
                }
            }
        }
        Action::DeleteLine => {
            for (id, cursor) in cursors.iter() {
                let Some(line_idx) = cursor.position.view_line else { continue };
                let line_start = ViewPosition {
                    view_line: Some(line_idx),
                    column: Some(0),
                    source_byte: layout.view_position_to_source_byte(line_idx, 0),
                };
                // Include newline if present
                let next_line_start = if line_idx + 1 < layout.lines.len() {
                    ViewPosition {
                        view_line: Some(line_idx + 1),
                        column: Some(0),
                        source_byte: layout.view_position_to_source_byte(line_idx + 1, 0),
                    }
                } else {
                    // Last line - delete to end
                    let line_len = layout.lines.get(line_idx).map(|l| l.char_mappings.len()).unwrap_or(0);
                    ViewPosition {
                        view_line: Some(line_idx),
                        column: Some(line_len),
                        source_byte: layout.view_position_to_source_byte(line_idx, line_len),
                    }
                };
                if let (Some(start_ev), Some(end_ev)) = (view_pos_to_event(line_start), view_pos_to_event(next_line_start)) {
                    let view_range = ViewEventRange::new(start_ev, end_ev);
                    let source_range = view_range_to_buffer_range(layout, &line_start, &next_line_start);
                    if source_range.is_some() {
                        events.push(Event::Delete {
                            range: view_range,
                            source_range,
                            deleted_text: String::new(),
                            cursor_id: id,
                        });
                    }
                }
            }
        }
        Action::DeleteToLineEnd => {
            for (id, cursor) in cursors.iter() {
                let Some(line_idx) = cursor.position.view_line else { continue };
                let line_len = layout.lines.get(line_idx).map(|l| l.char_mappings.len()).unwrap_or(0);
                let line_end = ViewPosition {
                    view_line: Some(line_idx),
                    column: Some(line_len),
                    source_byte: layout.view_position_to_source_byte(line_idx, line_len),
                };
                if let (Some(cur_ev), Some(end_ev)) = (view_pos_to_event(cursor.position), view_pos_to_event(line_end)) {
                    let view_range = ViewEventRange::normalized(cur_ev, end_ev);
                    let source_range = view_range_to_buffer_range(layout, &cursor.position, &line_end);
                    if source_range.is_some() {
                        events.push(Event::Delete {
                            range: view_range,
                            source_range,
                            deleted_text: String::new(),
                            cursor_id: id,
                        });
                    }
                }
            }
        }
        Action::ScrollUp => {
            layout_nav::scroll_view(layout, viewport, -1);
        }
        Action::ScrollDown => {
            layout_nav::scroll_view(layout, viewport, 1);
        }
        Action::DeleteBackward => {
            for (id, cursor) in cursors.iter() {
                if let Some(anchor) = cursor.anchor {
                    if let (Some(anc_ev), Some(cur_ev)) = (view_pos_to_event(anchor), view_pos_to_event(cursor.position)) {
                        let view_range = ViewEventRange::normalized(anc_ev, cur_ev);
                        let source_range =
                            view_range_to_buffer_range(layout, &anchor, &cursor.position);
                        if source_range.is_some() {
                            events.push(Event::Delete {
                                range: view_range,
                                source_range,
                                deleted_text: String::new(),
                                cursor_id: id,
                            });
                        }
                    }
                } else if let Some(prev_byte) = cursor.source_byte().and_then(|b| b.checked_sub(1)) {
                    let start = layout_nav::move_horizontal(layout, &cursor.position, -1);
                    if let (Some(start_ev), Some(cur_ev)) = (view_pos_to_event(start), view_pos_to_event(cursor.position)) {
                        let view_range = ViewEventRange::normalized(start_ev, cur_ev);
                        events.push(Event::Delete {
                            range: view_range,
                            source_range: Some(prev_byte..cursor.source_byte().unwrap()),
                            deleted_text: String::new(),
                            cursor_id: id,
                        });
                    }
                }
            }
        }
        Action::DeleteForward => {
            for (id, cursor) in cursors.iter() {
                if let Some(anchor) = cursor.anchor {
                    if let (Some(anc_ev), Some(cur_ev)) = (view_pos_to_event(anchor), view_pos_to_event(cursor.position)) {
                        let view_range = ViewEventRange::normalized(anc_ev, cur_ev);
                        let source_range =
                            view_range_to_buffer_range(layout, &anchor, &cursor.position);
                        if source_range.is_some() {
                            events.push(Event::Delete {
                                range: view_range,
                                source_range,
                                deleted_text: String::new(),
                                cursor_id: id,
                            });
                        }
                    }
                } else if let Some(start) = cursor.source_byte() {
                    let end_view = layout_nav::move_horizontal(layout, &cursor.position, 1);
                    let end_byte = end_view.view_line.and_then(|vl| end_view.column.and_then(|col| layout.view_position_to_source_byte(vl, col)))
                        .or_else(|| cursor.position.source_byte.map(|b| b.saturating_add(1)));
                    if let (Some(cur_ev), Some(end_ev)) = (view_pos_to_event(cursor.position), view_pos_to_event(end_view)) {
                        let view_range = ViewEventRange::normalized(cur_ev, end_ev);
                        events.push(Event::Delete {
                            range: view_range,
                            source_range: end_byte.map(|end| start..end),
                            deleted_text: String::new(),
                            cursor_id: id,
                        });
                    }
                }
            }
        }
        Action::InsertChar(ch) => {
            let text = ch.to_string();
            for (id, cursor) in cursors.iter() {
                if let Some(pos) = view_pos_to_buffer_byte(layout, &cursor.position) {
                    if let Some(mut event_pos) = view_pos_to_event(cursor.position) {
                        event_pos.source_byte = Some(pos);
                        events.push(Event::Insert {
                            position: event_pos,
                            text: text.clone(),
                            cursor_id: id,
                        });
                    }
                }
            }
        }
        Action::InsertNewline => {
            for (id, cursor) in cursors.iter() {
                if let Some(pos) = view_pos_to_buffer_byte(layout, &cursor.position) {
                    if let Some(mut event_pos) = view_pos_to_event(cursor.position) {
                        event_pos.source_byte = Some(pos);
                        events.push(Event::Insert {
                            position: event_pos,
                            text: "\n".to_string(),
                            cursor_id: id,
                        });
                    }
                }
            }
        }
        Action::MoveWordLeft => {
            for (id, cursor) in cursors.iter() {
                let new_pos = layout_nav::move_word_left(layout, &cursor.position, buffer);
                events.extend(move_cursor_event(id, cursor, new_pos, new_pos.column));
            }
        }
        Action::MoveWordRight => {
            for (id, cursor) in cursors.iter() {
                let new_pos = layout_nav::move_word_right(layout, &cursor.position, buffer);
                events.extend(move_cursor_event(id, cursor, new_pos, new_pos.column));
            }
        }
        Action::SelectWordLeft => {
            for (id, cursor) in cursors.iter() {
                let new_pos = layout_nav::move_word_left(layout, &cursor.position, buffer);
                events.extend(move_cursor_event_with_anchor(id, cursor, new_pos));
            }
        }
        Action::SelectWordRight => {
            for (id, cursor) in cursors.iter() {
                let new_pos = layout_nav::move_word_right(layout, &cursor.position, buffer);
                events.extend(move_cursor_event_with_anchor(id, cursor, new_pos));
            }
        }
        Action::DeleteWordBackward => {
            for (id, cursor) in cursors.iter() {
                let word_start = layout_nav::move_word_left(layout, &cursor.position, buffer);
                if let (Some(ws_ev), Some(cur_ev)) = (view_pos_to_event(word_start), view_pos_to_event(cursor.position)) {
                    let view_range = ViewEventRange::normalized(ws_ev, cur_ev);
                    let source_range = view_range_to_buffer_range(layout, &word_start, &cursor.position);
                    if source_range.is_some() {
                        events.push(Event::Delete {
                            range: view_range,
                            source_range,
                            deleted_text: String::new(),
                            cursor_id: id,
                        });
                    }
                }
            }
        }
        Action::DeleteWordForward => {
            for (id, cursor) in cursors.iter() {
                let word_end = layout_nav::move_word_right(layout, &cursor.position, buffer);
                if let (Some(cur_ev), Some(we_ev)) = (view_pos_to_event(cursor.position), view_pos_to_event(word_end)) {
                    let view_range = ViewEventRange::normalized(cur_ev, we_ev);
                    let source_range = view_range_to_buffer_range(layout, &cursor.position, &word_end);
                    if source_range.is_some() {
                        events.push(Event::Delete {
                            range: view_range,
                            source_range,
                            deleted_text: String::new(),
                            cursor_id: id,
                        });
                    }
                }
            }
        }
        _ => {}
    }

    if events.is_empty() {
        None
    } else {
        Some(events)
    }
}

fn move_cursor_event(
    cursor_id: crate::event::CursorId,
    cursor: &Cursor,
    new_pos: ViewPosition,
    new_pref_col: Option<usize>,
) -> Option<Event> {
    Some(Event::MoveCursor {
        cursor_id,
        old_position: view_pos_to_event(cursor.position)?,
        new_position: view_pos_to_event(new_pos)?,
        old_anchor: cursor.anchor.and_then(view_pos_to_event),
        new_anchor: None,
        old_sticky_column: cursor.preferred_visual_column,
        new_sticky_column: new_pref_col,
    })
}

fn move_cursor_event_with_anchor(
    cursor_id: crate::event::CursorId,
    cursor: &Cursor,
    new_pos: ViewPosition,
) -> Option<Event> {
    let anchor = cursor.anchor.unwrap_or(cursor.position);
    Some(Event::MoveCursor {
        cursor_id,
        old_position: view_pos_to_event(cursor.position)?,
        new_position: view_pos_to_event(new_pos)?,
        old_anchor: cursor.anchor.and_then(view_pos_to_event),
        new_anchor: view_pos_to_event(anchor),
        old_sticky_column: cursor.preferred_visual_column,
        new_sticky_column: cursor.preferred_visual_column,
    })
}

/// Convert ViewPosition to ViewEventPosition.
/// Returns None if view coordinates are not resolved.
fn view_pos_to_event(pos: ViewPosition) -> Option<ViewEventPosition> {
    ViewEventPosition::try_from_view_position(pos)
}
