use crate::cursor::ViewPosition;

/// A selection in view coordinates (start and end positions)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub start: ViewPosition,
    pub end: ViewPosition,
}

impl Selection {
    /// Create a new selection
    pub fn new(start: ViewPosition, end: ViewPosition) -> Self {
        Self { start, end }
    }

    /// Create from a tuple (for compatibility with existing code)
    pub fn from_tuple(tuple: (ViewPosition, ViewPosition)) -> Self {
        Self {
            start: tuple.0,
            end: tuple.1,
        }
    }

    /// Convert to tuple (for compatibility with existing code)
    pub fn to_tuple(self) -> (ViewPosition, ViewPosition) {
        (self.start, self.end)
    }

    /// Return a normalized selection where start <= end
    pub fn normalized(&self) -> Self {
        if self.start <= self.end {
            *self
        } else {
            Self {
                start: self.end,
                end: self.start,
            }
        }
    }

    /// Check if the selection is empty (start == end)
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Get the length in view coordinates (approximation)
    /// Returns 0 if view coordinates are not resolved.
    pub fn len(&self) -> usize {
        let (Some(start_line), Some(start_col)) = (self.start.view_line, self.start.column) else {
            return 0;
        };
        let (Some(end_line), Some(end_col)) = (self.end.view_line, self.end.column) else {
            return 0;
        };
        if start_line == end_line {
            end_col.saturating_sub(start_col)
        } else {
            // Multi-line selection: approximate
            let line_diff = end_line.saturating_sub(start_line);
            line_diff * 80 + end_col
        }
    }
}
