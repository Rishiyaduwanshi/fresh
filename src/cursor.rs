use crate::event::CursorId;
use std::collections::HashMap;

/// Selection mode for cursors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionMode {
    /// Normal character-wise selection (stream)
    Normal,
    /// Block/rectangular selection (column-wise)
    Block,
}

impl Default for SelectionMode {
    fn default() -> Self {
        SelectionMode::Normal
    }
}

/// Position in view coordinates with optional source mapping
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewPosition {
    pub view_line: usize,
    pub column: usize,
    /// Optional source byte offset (None for injected/view-only content)
    pub source_byte: Option<usize>,
}

/// Position in 2D coordinates (for block selection)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position2D {
    pub line: usize,
    pub column: usize,
}

/// A cursor in the view with optional selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    /// Primary position in view coordinates
    pub position: ViewPosition,

    /// Selection anchor (if any) in view coordinates
    pub anchor: Option<ViewPosition>,

    /// Preferred visual column for vertical movement
    pub preferred_visual_column: Option<usize>,

    /// Legacy sticky column (kept during migration)
    pub sticky_column: Option<usize>,

    /// Selection mode (normal or block)
    pub selection_mode: SelectionMode,

    /// Block selection anchor position (line, column) for rectangular selections
    /// Only used when selection_mode is Block
    pub block_anchor: Option<Position2D>,

    /// Whether regular movement should clear the selection (default: true)
    /// When false (e.g., after set_mark in Emacs mode), movement preserves the anchor
    pub deselect_on_move: bool,
}

impl Cursor {
    /// Create a new cursor at a position
    pub fn new(position: ViewPosition) -> Self {
        Self {
            position,
            anchor: None,
            preferred_visual_column: None,
            sticky_column: None,
            selection_mode: SelectionMode::Normal,
            block_anchor: None,
            deselect_on_move: true,
        }
    }

    /// Create a cursor with a selection
    pub fn with_selection(start: ViewPosition, end: ViewPosition) -> Self {
        Self {
            position: end,
            anchor: Some(start),
            preferred_visual_column: None,
            sticky_column: None,
            selection_mode: SelectionMode::Normal,
            block_anchor: None,
            deselect_on_move: true,
        }
    }

    /// Is the cursor collapsed (no selection)?
    pub fn collapsed(&self) -> bool {
        self.anchor.is_none() && self.block_anchor.is_none()
    }

    /// Get the selection range, if any (for normal selection) in view coordinates
    pub fn selection_range(&self) -> Option<(ViewPosition, ViewPosition)> {
        self.anchor.map(|anchor| {
            if anchor.view_line < self.position.view_line
                || (anchor.view_line == self.position.view_line
                    && anchor.column <= self.position.column)
            {
                (anchor, self.position)
            } else {
                (self.position, anchor)
            }
        })
    }

    /// Get the start of the selection (min of position and anchor)
    pub fn selection_start(&self) -> ViewPosition {
        self.selection_range()
            .map(|(start, _)| start)
            .unwrap_or(self.position)
    }

    /// Get the end of the selection (max of position and anchor)
    pub fn selection_end(&self) -> ViewPosition {
        self.selection_range()
            .map(|(_, end)| end)
            .unwrap_or(self.position)
    }

    /// Clear the selection, keeping only the position
    pub fn clear_selection(&mut self) {
        self.anchor = None;
        self.block_anchor = None;
        self.selection_mode = SelectionMode::Normal;
    }

    /// Set the selection anchor
    pub fn set_anchor(&mut self, anchor: ViewPosition) {
        self.anchor = Some(anchor);
    }

    /// Start a block selection at the given 2D position
    pub fn start_block_selection(&mut self, line: usize, column: usize) {
        self.selection_mode = SelectionMode::Block;
        self.block_anchor = Some(Position2D { line, column });
    }

    /// Clear block selection and return to normal mode
    pub fn clear_block_selection(&mut self) {
        self.selection_mode = SelectionMode::Normal;
        self.block_anchor = None;
    }

    /// Move to a position, optionally extending selection
    pub fn move_to(&mut self, position: ViewPosition, extend_selection: bool) {
        if extend_selection {
            if self.anchor.is_none() {
                self.anchor = Some(self.position);
            }
        } else {
            self.anchor = None;
            if !extend_selection && self.selection_mode == SelectionMode::Block {
                self.selection_mode = SelectionMode::Normal;
                self.block_anchor = None;
            }
        }
        self.position = position;
    }

    /// Adjust cursor position after an edit (view-based mapping TODO)
    pub fn adjust_for_edit(&mut self, _edit_pos: usize, _old_len: usize, _new_len: usize) {
        // TODO: re-map view positions after edits once layout is rebuilt.
    }

    pub fn source_byte(&self) -> Option<usize> {
        self.position.source_byte
    }

    pub fn set_source_byte(&mut self, byte: Option<usize>) {
        self.position.source_byte = byte;
    }
}

impl From<crate::event::ViewEventPosition> for ViewPosition {
    fn from(v: crate::event::ViewEventPosition) -> Self {
        ViewPosition {
            view_line: v.view_line,
            column: v.column,
            source_byte: v.source_byte,
        }
    }
}

/// Collection of cursors with multi-cursor support
#[derive(Debug, Clone)]
pub struct Cursors {
    /// Map from cursor ID to cursor
    cursors: HashMap<CursorId, Cursor>,

    /// Next available cursor ID
    next_id: usize,

    /// Primary cursor ID (the most recently added/active one)
    primary_id: CursorId,
}

impl Cursors {
    /// Create a new cursor collection with one cursor at view (0,0)
    pub fn new() -> Self {
        let primary_id = CursorId(0);
        let mut cursors = HashMap::new();
        cursors.insert(
            primary_id,
            Cursor::new(ViewPosition {
                view_line: 0,
                column: 0,
                source_byte: Some(0),
            }),
        );

        Self {
            cursors,
            next_id: 1,
            primary_id,
        }
    }

    /// Get the primary cursor
    pub fn primary(&self) -> &Cursor {
        self.cursors
            .get(&self.primary_id)
            .expect("Primary cursor should always exist")
    }

    /// Get the primary cursor mutably
    pub fn primary_mut(&mut self) -> &mut Cursor {
        self.cursors
            .get_mut(&self.primary_id)
            .expect("Primary cursor should always exist")
    }

    /// Get the primary cursor ID
    pub fn primary_id(&self) -> CursorId {
        self.primary_id
    }

    /// Get a cursor by ID
    pub fn get(&self, id: CursorId) -> Option<&Cursor> {
        self.cursors.get(&id)
    }

    /// Get a cursor by ID mutably
    pub fn get_mut(&mut self, id: CursorId) -> Option<&mut Cursor> {
        self.cursors.get_mut(&id)
    }

    /// Get all cursors as a slice
    pub fn iter(&self) -> impl Iterator<Item = (CursorId, &Cursor)> {
        self.cursors.iter().map(|(id, c)| (*id, c))
    }

    /// Add a new cursor and return its ID
    pub fn add(&mut self, cursor: Cursor) -> CursorId {
        let id = CursorId(self.next_id);
        self.next_id += 1;
        self.cursors.insert(id, cursor);
        self.primary_id = id; // New cursor becomes primary
        id
    }

    /// Insert a cursor with a specific ID (for undo/redo)
    pub fn insert_with_id(&mut self, id: CursorId, cursor: Cursor) {
        self.cursors.insert(id, cursor);
        self.primary_id = id;
        self.next_id = self.next_id.max(id.0 + 1);
    }

    /// Remove a cursor by ID
    pub fn remove(&mut self, id: CursorId) {
        self.cursors.remove(&id);
        if self.primary_id == id {
            if let Some((&first_id, _)) = self.cursors.iter().next() {
                self.primary_id = first_id;
            } else {
                // Always keep one cursor
                let new_cursor = Cursor::new(ViewPosition {
                    view_line: 0,
                    column: 0,
                    source_byte: Some(0),
                });
                self.cursors.insert(id, new_cursor);
                self.primary_id = id;
                self.next_id = id.0 + 1;
            }
        }
    }

    /// Normalize cursor order (retain deterministic order)
    pub fn normalize(&mut self) {
        // No-op placeholder; view-based cursors require layout to sort meaningfully.
    }

    /// Adjust all cursors after an edit (view-based mapping TODO)
    pub fn adjust_for_edit(&mut self, edit_pos: usize, old_len: usize, new_len: usize) {
        for cursor in self.cursors.values_mut() {
            cursor.adjust_for_edit(edit_pos, old_len, new_len);
        }
    }
}
