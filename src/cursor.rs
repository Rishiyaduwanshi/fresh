use crate::event::CursorId;
use crate::selection::Selection;
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
///
/// `view_line` and `column` are None when position hasn't been resolved against a Layout.
/// `source_byte` is None for injected/view-only content (annotations).
///
/// For code buffers: `source_byte` is always known, view coords need Layout to resolve.
/// For annotations: `source_byte` is None, view coords are set directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewPosition {
    /// View line index (None = needs resolution from source_byte via Layout)
    pub view_line: Option<usize>,
    /// Column in the view line (None = needs resolution from source_byte via Layout)
    pub column: Option<usize>,
    /// Source byte offset (None for injected/view-only content)
    pub source_byte: Option<usize>,
}

impl std::fmt::Display for ViewPosition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let line = self.view_line.map(|l| l.to_string()).unwrap_or_else(|| "?".to_string());
        let col = self.column.map(|c| c.to_string()).unwrap_or_else(|| "?".to_string());
        match self.source_byte {
            Some(byte) => write!(f, "{}:{} (@{})", line, col, byte),
            None => write!(f, "{}:{}", line, col),
        }
    }
}

impl PartialOrd for ViewPosition {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        // Compare by source_byte if both have it, otherwise by view coords
        match (self.source_byte, other.source_byte) {
            (Some(a), Some(b)) => Some(a.cmp(&b)),
            _ => match (self.view_line, other.view_line) {
                (Some(a), Some(b)) => match a.cmp(&b) {
                    std::cmp::Ordering::Equal => self.column.partial_cmp(&other.column),
                    ord => Some(ord),
                },
                _ => None, // Can't compare unresolved positions
            },
        }
    }
}

impl Ord for ViewPosition {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other).unwrap_or(std::cmp::Ordering::Equal)
    }
}

impl ViewPosition {
    /// Create a source-backed position (view coords unresolved)
    pub fn from_source(source_byte: usize) -> Self {
        Self {
            view_line: None,
            column: None,
            source_byte: Some(source_byte),
        }
    }

    /// Create a view-only position (for injected content)
    pub fn from_view(view_line: usize, column: usize) -> Self {
        Self {
            view_line: Some(view_line),
            column: Some(column),
            source_byte: None,
        }
    }

    /// Create a fully resolved position
    pub fn resolved(view_line: usize, column: usize, source_byte: usize) -> Self {
        Self {
            view_line: Some(view_line),
            column: Some(column),
            source_byte: Some(source_byte),
        }
    }
}

impl ViewPosition {
    /// Construct a view position from a source byte (view coordinates unknown)
    /// Alias for from_source() kept for backward compatibility
    pub fn from_source_byte(byte: usize) -> Self {
        Self::from_source(byte)
    }
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
    pub fn selection_range(&self) -> Option<Selection> {
        self.anchor.map(|anchor| {
            if anchor.view_line < self.position.view_line
                || (anchor.view_line == self.position.view_line
                    && anchor.column <= self.position.column)
            {
                Selection::new(anchor, self.position)
            } else {
                Selection::new(self.position, anchor)
            }
        })
    }

    /// Get the start of the selection (min of position and anchor)
    pub fn selection_start(&self) -> ViewPosition {
        self.selection_range()
            .map(|sel| sel.start)
            .unwrap_or(self.position)
    }

    /// Get the end of the selection (max of position and anchor)
    pub fn selection_end(&self) -> ViewPosition {
        self.selection_range()
            .map(|sel| sel.end)
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

    /// Adjust cursor position after an edit
    ///
    /// This adjusts the source_byte and column based on edits to maintain
    /// proper cursor tracking. The view_line will be re-computed when the
    /// layout is rebuilt.
    ///
    /// # Arguments
    /// * `edit_pos` - Byte position where the edit occurred
    /// * `old_len` - Length of text that was removed (0 for pure insert)
    /// * `new_len` - Length of text that was inserted (0 for pure delete)
    pub fn adjust_for_edit(&mut self, edit_pos: usize, old_len: usize, new_len: usize) {
        // Adjust position
        if let Some(byte) = self.position.source_byte {
            self.position.source_byte = Some(Self::adjust_byte(byte, edit_pos, old_len, new_len));
            // Invalidate view coordinates - they'll be recalculated when layout is rebuilt
            self.position.view_line = None;
            self.position.column = None;
        }

        // Adjust anchor if present
        if let Some(ref mut anchor) = self.anchor {
            if let Some(byte) = anchor.source_byte {
                anchor.source_byte = Some(Self::adjust_byte(byte, edit_pos, old_len, new_len));
                // Invalidate view coordinates
                anchor.view_line = None;
                anchor.column = None;
            }
        }
    }

    /// Helper to adjust a byte position for an edit
    fn adjust_byte(byte: usize, edit_pos: usize, old_len: usize, new_len: usize) -> usize {
        if byte < edit_pos {
            // Cursor is before the edit - no change
            byte
        } else if byte < edit_pos + old_len {
            // Cursor is inside the deleted region - clamp to edit position
            edit_pos
        } else {
            // Cursor is after the edit - shift by the delta
            let delta = new_len as isize - old_len as isize;
            ((byte as isize) + delta).max(0) as usize
        }
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
            view_line: Some(v.view_line),
            column: Some(v.column),
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
    /// Create a new cursor collection with one cursor at position (0,0)
    pub fn new() -> Self {
        let primary_id = CursorId(0);
        let mut cursors = HashMap::new();
        cursors.insert(
            primary_id,
            Cursor::new(ViewPosition::resolved(0, 0, 0)),
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

    /// Number of cursors.
    pub fn len(&self) -> usize {
        self.cursors.len()
    }

    /// True if no cursors (should not happen in practice).
    pub fn is_empty(&self) -> bool {
        self.cursors.is_empty()
    }

    /// Alias for len() for callers expecting count.
    pub fn count(&self) -> usize {
        self.len()
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
                // Always keep one cursor - create one at document start
                let new_cursor = Cursor::new(ViewPosition::resolved(0, 0, 0));
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
