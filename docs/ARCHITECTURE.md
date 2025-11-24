# Fresh Architecture

Fresh is a high-performance, terminal-based text editor built in Rust. It's designed to be fast, responsive, and extensible, with a modern architecture that draws inspiration from the best ideas in the world of text editors.

## Core Design Principles

*   **Performance First:** Every architectural decision is made with performance in mind. This includes the choice of data structures, the design of the rendering pipeline, and the implementation of core features.
*   **Event-Driven:** All state changes are represented as events, which are processed by a central event loop. This makes the editor's state predictable and enables features like unlimited undo/redo.
*   **Asynchronous I/O:** All file and process I/O is handled asynchronously on a separate thread pool. This ensures that the editor's UI is never blocked by slow I/O operations.
*   **Extensible:** A powerful TypeScript-based plugin system allows for deep customization and extension of the editor's functionality.

## High-Level Architecture

```
┌─────────────────────────────────────────────────────────┐
│                   MAIN THREAD (Sync)                    │
│  ┌──────────────┐  ┌──────────────┐  ┌─────────────┐    │
│  │ Event Loop   │→ │  Editor      │→ │  Renderer   │    │
│  │ (crossterm)  │  │  (state)     │  │  (ratatui)  │    │
│  └──────────────┘  └──────────────┘  └─────────────┘    │
│         ↓                 ↑                             │
│    Input Queue      EventQueue (mpsc)                   │
└─────────────────────────────────────────────────────────┘
         ↑                      ↑
         │ send events          │ send messages
         │                      │
    ┌────┴──────────┐  ┌────────┴──────────┐
    │ LSP Tasks     │  │ File I/O Tasks    │
    │ (Tokio)       │  │ (Tokio)           │
    └───────────────┘  └───────────────────┘
```

## The Document Model

To provide a clean separation between the editor's UI and the underlying text buffer, Fresh uses a `DocumentModel` trait. This abstraction layer is responsible for all interactions with the text buffer and provides a consistent API for both small and large files.

### Dual Position System

To support multi-gigabyte files where line indexing may be unavailable, the `DocumentModel` uses a dual position system:

*   **`DocumentPosition::LineColumn`:** For small files, this provides precise line and column-based positioning.
*   **`DocumentPosition::ByteOffset`:** For large files, this provides byte-offset-based positioning, which is always available and precise.

### The `DocumentModel` Trait

The `DocumentModel` trait defines a set of methods for interacting with the document, including:

*   **`get_viewport_content`:** The core rendering primitive, which returns the content for the visible portion of the screen.
*   **`position_to_offset` and `offset_to_position`:** For converting between the two position types.
*   **`insert`, `delete`, and `replace`:** For modifying the document's content.

This abstraction allows the rest of the editor to be blissfully unaware of the details of the underlying text buffer, such as whether it's a small file with a full line index or a large file with lazy loading.

## The Buffer

The core of the editor is the text buffer, which is implemented as a **`PieceTree`**. A `PieceTree` is a balanced binary tree that represents the text as a sequence of "pieces," which are references to either the original, immutable file buffer or an in-memory buffer of user additions.

This data structure provides several key advantages:

*   **O(log n) Edits:** Inserts and deletes are O(log n), where n is the number of pieces. This makes text editing extremely fast, even in large files.
*   **Efficient Memory Usage:** The `PieceTree` only stores the changes to the file, not the entire file content. This makes it very memory-efficient, especially for large files.
*   **Lazy Loading:** For multi-gigabyte files, Fresh uses a lazy loading strategy. The file is not loaded into memory all at once. Instead, chunks of the file are loaded on demand as the user scrolls through the file.

## The Rendering Pipeline

The rendering pipeline is designed to be as efficient as possible, especially when it comes to overlays (visual decorations like highlights and squiggly lines).

### The Overlay Problem

A naive implementation of overlays, where each character on the screen is checked against a list of all active overlays, can lead to significant performance problems. This is an O(N*M) problem, where N is the number of characters on the screen and M is the number of overlays.

### The Solution: A High-Performance Overlay System

Fresh uses a multi-pronged approach to solve the overlay problem:

1.  **Line-Indexed Overlay Storage:** Instead of storing overlays in a flat list, they are stored in a `BTreeMap<usize, Vec<Overlay>>`, where the key is the starting line number of the overlay. This allows for a very fast lookup of all overlays on a given line.
2.  **Render-Time Overlay Cache:** During each frame, the editor creates a cache of all overlays that are visible in the current viewport. This cache is then used to apply the overlays to the text, avoiding the need to query the overlay manager for each character.
3.  **Diagnostic Hash Check:** For LSP diagnostics, which are a major source of overlays, Fresh uses a hash check to avoid redundant updates. If the set of diagnostics from the language server hasn't changed, no work is done.

This new architecture, which is currently being implemented, will provide a massive performance improvement over the old system and will allow Fresh to handle thousands of overlays without breaking a sweat.

## LSP Integration

Fresh has a deep and robust integration with the Language Server Protocol (LSP), providing features like code completion, diagnostics, and go-to-definition.

The LSP integration is built on a multi-threaded architecture that ensures the editor's UI is never blocked by the language server.

*   **`LspManager`:** A central coordinator that manages multiple language servers (one for each language).
*   **`LspHandle`:** A handle to a specific language server, providing a non-blocking API for sending commands and notifications.
*   **`AsyncBridge`:** An `mpsc` channel that bridges the asynchronous world of the LSP tasks with the synchronous world of the editor's main event loop.

This architecture allows Fresh to communicate with language servers in a highly efficient and non-blocking way, providing a smooth and responsive user experience.

## Annotated Views

Fresh supports **Annotated Views**, a powerful pattern for displaying file content with injected annotations (such as headers, metadata lines, or visual separators) while preserving core editor features like line numbers and syntax highlighting. This architecture enables features like git blame, code coverage overlays, and inline documentation.

### The Problem

Consider displaying git blame information: you want to show the file content with header lines above each block indicating the commit, author, and date. A naive approach creates several problems:

1. **Line Number Mismatch:** If headers are part of the buffer content, line numbers include them, making it impossible to show "line 42" next to the actual line 42 of the source file.

2. **Syntax Highlighting Loss:** If the buffer contains mixed content (headers + code), tree-sitter cannot parse it correctly, and syntax highlighting breaks.

3. **Historical Content:** For features like "blame at parent commit," you need to display historical file versions that differ from the current file.

### The Solution: View Transforms + Virtual Buffers

Fresh solves this with a hybrid architecture combining two mechanisms:

```
┌─────────────────────────────────────────────────────────────────┐
│                    ANNOTATED VIEW ARCHITECTURE                  │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│   ┌─────────────────────────────────────────────────────────┐  │
│   │              Virtual Buffer (Content Layer)              │  │
│   │  • Contains actual file content (current or historical) │  │
│   │  • Language detected from buffer name extension         │  │
│   │  • Syntax highlighting via tree-sitter                  │  │
│   │  • Text properties store annotation metadata            │  │
│   └─────────────────────────────────────────────────────────┘  │
│                              │                                  │
│                              ▼                                  │
│   ┌─────────────────────────────────────────────────────────┐  │
│   │             View Transform (Presentation Layer)          │  │
│   │  • Injects annotation headers between content blocks    │  │
│   │  • Headers: source_offset = None (no line number)       │  │
│   │  • Content: source_offset = Some(byte) (has line num)   │  │
│   │  • Styling via ViewTokenWire.style field                │  │
│   └─────────────────────────────────────────────────────────┘  │
│                              │                                  │
│                              ▼                                  │
│   ┌─────────────────────────────────────────────────────────┐  │
│   │                   Rendered Output                        │  │
│   │     ── abc123 (Alice, 2 days ago) "Fix bug" ──          │  │
│   │  42 │ fn main() {                                       │  │
│   │  43 │     println!("Hello");                            │  │
│   │     ── def456 (Bob, 1 week ago) "Add feature" ──        │  │
│   │  44 │     do_something();                               │  │
│   │  45 │ }                                                 │  │
│   └─────────────────────────────────────────────────────────┘  │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

### Key Components

#### 1. Virtual Buffer with Content

The first layer is a virtual buffer containing the actual file content:

```typescript
// Create buffer with historical or current content
const content = await editor.spawnProcess("git", ["show", `${commit}:${path}`]);
const bufferId = await editor.createVirtualBuffer({
  name: `*annotated:${basename(path)}*`,  // Extension enables syntax highlighting
  mode: "annotated-view",
  read_only: true,
  entries: [{ text: content.stdout, properties: { commit, path } }],
});
```

**Key properties:**
- Buffer name includes file extension (e.g., `*blame:main.rs*`) for automatic language detection
- Content is pure source code, enabling correct tree-sitter parsing
- Text properties can store metadata without affecting content

#### 2. ViewTokenWire with Source Mapping

The `ViewTokenWire` structure enables precise control over line numbering:

```rust
pub struct ViewTokenWire {
    /// Source byte offset - None for injected annotations
    pub source_offset: Option<usize>,
    pub kind: ViewTokenWireKind,
    /// Optional styling for injected content
    pub style: Option<ViewTokenStyle>,
}

pub struct ViewTokenStyle {
    pub fg: Option<(u8, u8, u8)>,
    pub bg: Option<(u8, u8, u8)>,
    pub bold: bool,
    pub italic: bool,
}
```

The `source_offset` field is the key mechanism:
- **`Some(byte_position)`:** Token maps to source content. The renderer:
  - Includes it in line number calculation
  - Applies syntax highlighting from that byte position
  - Enables cursor positioning and selection
- **`None`:** Token is injected annotation. The renderer:
  - Skips line number increment (shows blank in gutter)
  - Applies `style` field if present
  - Does not participate in source-based features

#### 3. View Transform Hook

Plugins register for the `view_transform_request` hook, called each render frame:

```typescript
editor.on("view_transform_request", "onViewTransform");

globalThis.onViewTransform = function(args: {
  buffer_id: number;
  split_id: number;
  viewport_start: number;
  viewport_end: number;
  tokens: ViewTokenWire[];
}) {
  // Only transform our annotated buffers
  if (!isAnnotatedBuffer(args.buffer_id)) return;

  const transformed: ViewTokenWire[] = [];

  for (const block of getAnnotationBlocks(args.buffer_id)) {
    // Inject header (no source mapping = no line number)
    injectHeader(transformed, block, {
      source_offset: null,
      style: { bg: [50, 50, 55], fg: [220, 220, 220], bold: true }
    });

    // Pass through content tokens (preserve source mapping)
    for (const token of args.tokens) {
      if (tokenInBlock(token, block)) {
        transformed.push(token);  // Unchanged - keeps line numbers & highlighting
      }
    }
  }

  editor.submitViewTransform(
    args.buffer_id,
    args.split_id,
    args.viewport_start,
    args.viewport_end,
    transformed,
    null
  );
};
```

### How Line Numbers Work

The renderer in `split_rendering.rs` handles line numbers via the `is_continuation` check:

```rust
// Check if previous character had no source mapping
let is_continuation = if line_view_offset > 0 {
    view_mapping.get(line_view_offset - 1) == Some(&None)
} else {
    false
};

// Only increment line number for content lines
if !is_continuation && lines_rendered > 0 {
    current_source_line_num += 1;
}
```

This means:
- Lines starting after a `source_offset: None` newline show blank in the line number gutter
- Lines starting after a `source_offset: Some(_)` newline increment and display the line number
- The result: annotation headers have no line numbers, content lines have correct source line numbers

### How Syntax Highlighting Works

Syntax highlighting is applied based on `source_offset`:

```rust
let highlight_color = byte_pos.and_then(|bp| {
    highlight_spans
        .iter()
        .find(|span| span.range.contains(&bp))
        .map(|span| span.color)
});
```

- Content tokens have `source_offset: Some(byte)` → looked up in highlight spans → colored
- Annotation tokens have `source_offset: None` → no highlight lookup → uses `style` field instead

### Use Cases

#### Git Blame

```
Buffer: Historical file content from `git show commit:path`
Annotations: Commit headers above each blame block
Line numbers: Match historical file lines
Highlighting: Based on file language
```

#### Code Coverage

```
Buffer: Current file content
Annotations: Coverage percentage headers above functions
Line numbers: Match current file
Highlighting: Normal syntax + coverage overlays
```

#### Inline Documentation

```
Buffer: Source code
Annotations: Doc comments rendered as styled blocks
Line numbers: Only for code lines
Highlighting: Code highlighted, docs styled differently
```

### Performance Considerations

1. **Viewport-Only Processing:** View transforms only process the visible viewport, not the entire file. For a 100K line file, only ~50 lines are transformed per frame.

2. **Efficient Block Lookup:** Annotation metadata should be stored in a sorted structure enabling O(log n) lookup of blocks overlapping the viewport.

3. **Frame-Rate Transform:** The `view_transform_request` hook is called every frame. Plugins must respond quickly. Pre-compute annotation positions; don't run git commands during the hook.

4. **Caching:** View transforms are cached per-split and reused until explicitly cleared or the viewport changes significantly.

### Implementation Checklist

To implement an annotated view feature:

1. **Define annotation structure:** What metadata accompanies each block? (commit info, coverage data, etc.)

2. **Create content buffer:** Use `createVirtualBuffer` with appropriate name for language detection

3. **Store block positions:** Track byte ranges for each annotation block in the content

4. **Implement view transform hook:** Inject headers with `source_offset: None` and `style`, pass through content tokens unchanged

5. **Handle navigation:** Map cursor positions in the view back to logical positions in your annotation model

6. **Support refresh:** When annotations change (e.g., blame at different commit), update buffer content and block positions; view transform auto-updates on next render

## Buffer vs View: State Separation

Fresh cleanly separates **Buffer state** (the document) from **View state** (how it's displayed). This enables multiple independent views of the same buffer, each with its own scroll position, cursor, and view transform.

### Why Separate?

Consider two splits showing the same file:
- Split A: Normal view, scrolled to line 100, cursor at line 105
- Split B: Git blame view, scrolled to line 1, cursor at line 10

Both views share the same underlying text, but have completely independent:
- Scroll positions (viewport)
- Cursor positions
- View transforms (blame vs normal)
- Computed layouts

### The State Model

```
┌─────────────────────────────────────────────────────────────┐
│                    BUFFER STATE (shared)                    │
│  EditorState per buffer_id                                  │
│  ┌────────────────────────────────────────────────────────┐ │
│  │ • buffer: PieceTree (actual text content)              │ │
│  │ • undo/redo history                                    │ │
│  │ • syntax highlighter (tokens derived from content)     │ │
│  │ • overlays (content-anchored decorations)              │ │
│  │ • markers (bookmarks, breakpoints)                     │ │
│  └────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
                              │
            ┌─────────────────┼─────────────────┐
            ▼                                   ▼
┌───────────────────────────┐     ┌───────────────────────────┐
│   VIEW STATE (Split A)    │     │   VIEW STATE (Split B)    │
│   SplitViewState          │     │   SplitViewState          │
│ ┌───────────────────────┐ │     │ ┌───────────────────────┐ │
│ │ • viewport (scroll)   │ │     │ │ • viewport (scroll)   │ │
│ │ • cursors             │ │     │ │ • cursors             │ │
│ │ • view_transform      │ │     │ │ • view_transform      │ │
│ │ • view_mode           │ │     │ │ • view_mode           │ │
│ │ • layout (computed)   │ │     │ │ • layout (computed)   │ │
│ └───────────────────────┘ │     │ └───────────────────────┘ │
└───────────────────────────┘     └───────────────────────────┘
```

### Event Routing

Events are routed based on whether they affect the buffer or the view:

**Buffer Events** → Applied to shared `EditorState`:
- `Insert`, `Delete` (text modifications)
- `AddOverlay`, `RemoveOverlay` (content decorations)
- Undo/Redo

**View Events** → Applied to `SplitViewState` for active split:
- `Scroll` (viewport movement)
- `MoveCursor` (cursor navigation)
- `SetViewMode` (normal/blame/etc)

When a buffer event modifies content, ALL views of that buffer must:
1. Adjust their cursor positions (if affected by the edit)
2. Mark their layout as dirty (needs rebuild)

### Syntax Highlighting: Buffer or View?

Base syntax highlighting is **buffer state** because:
- Tokens are derived from content (same content = same tokens)
- Expensive to compute, wasteful to duplicate per-view
- Most editors share highlighting across views

However, **style application** can be view-specific via view transforms. A git blame view transform can restyle tokens (dim old code, highlight recent changes) without affecting other views.

## The Layout Layer

The Layout is **View state**, not Buffer state. Each view (split) has its own Layout computed from its view_transform. This enables different views of the same buffer to have different layouts (e.g., blame view vs normal view).

### The Two Layers

```
┌─────────────────────────────────────────────────────────────┐
│              BUFFER LAYER (EditorState - shared)            │
│  • Buffer: source bytes (PieceTree)                         │
│  • Syntax tokens (shared across views)                      │
│  • Overlays, markers (content-anchored)                     │
└─────────────────────────────────────────────────────────────┘
                              │
          ┌───────────────────┼───────────────────┐
          ▼                                       ▼
┌─────────────────────────────┐     ┌─────────────────────────────┐
│  VIEW LAYER (Split A)       │     │  VIEW LAYER (Split B)       │
│  SplitViewState             │     │  SplitViewState             │
│ ┌─────────────────────────┐ │     │ ┌─────────────────────────┐ │
│ │ view_transform (plugin) │ │     │ │ view_transform (plugin) │ │
│ │         │               │ │     │ │         │               │ │
│ │         ▼               │ │     │ │         ▼               │ │
│ │ layout (ViewLines)      │ │     │ │ layout (ViewLines)      │ │
│ │         │               │ │     │ │         │               │ │
│ │         ▼               │ │     │ │         ▼               │ │
│ │ viewport (view line idx)│ │     │ │ viewport (view line idx)│ │
│ │ cursors (source bytes)  │ │     │ │ cursors (source bytes)  │ │
│ └─────────────────────────┘ │     │ └─────────────────────────┘ │
└─────────────────────────────┘     └─────────────────────────────┘
                │                                 │
                ▼                                 ▼
┌─────────────────────────────┐     ┌─────────────────────────────┐
│         RENDERING           │     │         RENDERING           │
│  ViewLines[top..top+height] │     │  ViewLines[top..top+height] │
└─────────────────────────────┘     └─────────────────────────────┘
```

### Why Two Layers?

When view transforms inject content (like git blame headers), the display has **more lines** than the source buffer:

```
Source Buffer (5 lines):     Display Layout (7 lines):
─────────────────────        ─────────────────────────
Line 1                       ── Header (injected) ──
Line 2                       Line 1
Line 3                       Line 2
Line 4                       ── Header (injected) ──
Line 5                       Line 3
                             Line 4
                             Line 5
```

If the viewport operates on source lines, scroll limits are wrong (can't scroll to show all 7 display lines). If cursor movement uses source lines, pressing ↓ skips over injected headers.

The solution: **viewport and visual navigation operate on the Layout Layer**.

### Core Data Structures

```rust
/// A single display line with source mapping
pub struct ViewLine {
    pub text: String,
    /// Maps each character to source byte (None = injected)
    pub char_mappings: Vec<Option<usize>>,
    pub char_styles: Vec<Option<ViewTokenStyle>>,
    pub line_start: LineStart,
    pub ends_with_newline: bool,
}

/// The complete layout for a view (computed from view_transform)
pub struct Layout {
    /// All display lines
    pub lines: Vec<ViewLine>,
    /// Fast lookup: source byte → view line index
    byte_to_view_line: BTreeMap<usize, usize>,
}

/// View state for a single split (independent of buffer)
pub struct SplitViewState {
    /// Cursor positions (source bytes, but per-view)
    pub cursors: Cursors,
    /// Scroll position in layout coordinates
    pub viewport: Viewport,
    /// Plugin-provided view transform (git blame, etc)
    pub view_transform: Option<ViewTransformPayload>,
    /// Computed layout (from view_transform or base tokens)
    pub layout: Option<Layout>,
    /// Display mode
    pub view_mode: ViewMode,
}

/// Viewport tracks position in layout coordinates
pub struct Viewport {
    /// Stable anchor in source bytes (survives layout rebuilds)
    pub anchor_byte: usize,
    /// Current top of viewport in view line index
    pub top_view_line: usize,
    /// Dimensions
    pub width: u16,
    pub height: u16,
}

/// Cursor stays in document coordinates (for editing)
pub struct Cursor {
    /// Position in source bytes
    pub position: usize,
    /// Preferred visual column (for ↑/↓ movement)
    pub preferred_visual_column: Option<usize>,
}
```

### Layout Building Strategy: Lazy but Required

Following VSCode's ViewModel pattern, the Layout is built **lazily but is never optional**:

```rust
impl SplitViewState {
    /// Get or build the layout. Always succeeds - no fallback to buffer.
    fn ensure_layout(&mut self, buffer: &Buffer) -> &Layout {
        if self.layout.is_none() || self.layout_dirty {
            self.rebuild_layout(buffer);
        }
        self.layout.as_ref().unwrap()
    }
}
```

**Key principle**: Every operation that needs Layout calls `ensure_layout()` first. There is no "fallback to buffer-based scrolling" - Layout always exists when needed.

**Layout becomes dirty when:**
- Buffer content changes (Insert/Delete events)
- View transform changes (plugin sends new tokens)
- Scroll would move past current layout's source_range

### The Frame Flow

```
┌─────────────────────────────────────────────────────────────┐
│ 1. ANY VIEW OPERATION (scroll, cursor move, render)         │
│    • Call ensure_layout() first                             │
│    • If layout is None or dirty:                            │
│      - Get view transform tokens (from plugin or base)      │
│      - Convert to ViewLines via ViewLineIterator            │
│      - Build byte→view_line index                           │
│      - Store in SplitViewState.layout                       │
│    • Proceed with operation using valid Layout              │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│ 2. PROCESS INPUT (routed by event type)                     │
│                                                             │
│    Scroll Event → SplitViewState:                           │
│    • ensure_layout()                                        │
│    • viewport.top_view_line += offset                       │
│    • Clamp to [0, layout.lines.len() - viewport.height]     │
│    • Update anchor_byte from layout for stability           │
│                                                             │
│    Cursor Move (↑/↓) → SplitViewState:                      │
│    • ensure_layout()                                        │
│    • Find cursor's (view_line, visual_col) in layout        │
│    • Move to adjacent view line, same visual column         │
│    • Translate back to source byte via char_mappings        │
│    • Update cursor.position in SplitViewState.cursors       │
│                                                             │
│    Cursor Move (←/→/word/etc) → SplitViewState:             │
│    • Operate directly on source bytes (document layer)      │
│    • ensure_layout(), then ensure_visible()                 │
│                                                             │
│    Edit (insert/delete) → EditorState (shared buffer):      │
│    • Modify buffer at cursor.position                       │
│    • Mark ALL views' layouts as dirty                       │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│ 3. ENSURE CURSOR VISIBLE                                    │
│    • Find cursor byte in layout → view_line_index           │
│    • If view_line_index outside [top, top+height]:          │
│      • Adjust top_view_line to center cursor                │
│    • Update anchor_byte for stability                       │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│ 4. RENDER                                                   │
│    • for line in layout.lines[top..top+height]:             │
│        • Render line.text with line.char_styles             │
│        • Show line number if line has source content        │
│        • Highlight cursor position if on this line          │
└─────────────────────────────────────────────────────────────┘
```

### Viewport Stability

When the layout changes (edit, view transform update), view line indices shift. To maintain stable scroll position:

1. **Store anchor_byte**: The source byte at the top of the viewport
2. **On layout rebuild**: Find anchor_byte in new layout → new top_view_line
3. **Clamp if needed**: If anchor_byte no longer exists, clamp to valid range

```rust
impl Viewport {
    fn stabilize_after_layout_change(&mut self, layout: &Layout) {
        // Find where our anchor landed in the new layout
        if let Some(&view_line) = layout.byte_to_view_line.get(&self.anchor_byte) {
            self.top_view_line = view_line;
        } else {
            // Anchor byte gone (deleted), find nearest
            self.top_view_line = layout.find_nearest_view_line(self.anchor_byte);
        }
        // Clamp to valid range
        let max_top = layout.lines.len().saturating_sub(self.height as usize);
        self.top_view_line = self.top_view_line.min(max_top);
        // Update anchor to current top line's source byte
        self.anchor_byte = layout.get_source_byte_for_line(self.top_view_line);
    }
}
```

### WYSIWYG Cursor Movement

For ↑/↓ to work correctly with injected lines:

```rust
impl Cursor {
    fn move_vertical(&mut self, direction: i32, layout: &Layout) {
        // 1. Find current position in layout
        let (current_view_line, current_visual_col) =
            layout.source_byte_to_view_position(self.position);

        // 2. Use preferred column if set (for consistent vertical movement)
        let target_col = self.preferred_visual_column
            .unwrap_or(current_visual_col);

        // 3. Move to target view line
        let target_view_line = (current_view_line as i32 + direction)
            .max(0) as usize;
        let target_view_line = target_view_line.min(layout.lines.len() - 1);

        // 4. Translate back to source byte
        if let Some(new_byte) = layout.view_position_to_source_byte(
            target_view_line,
            target_col
        ) {
            self.position = new_byte;
        }
        // If target is injected line, skip to next source line

        // 5. Remember preferred column for subsequent moves
        self.preferred_visual_column = Some(target_col);
    }
}
```

### Scroll Limits

With the Layout Layer, scroll limits are trivially correct:

```rust
impl Viewport {
    fn scroll(&mut self, offset: isize, layout: &Layout) {
        let new_top = (self.top_view_line as isize + offset)
            .max(0) as usize;
        let max_top = layout.lines.len().saturating_sub(self.height as usize);
        self.top_view_line = new_top.min(max_top);

        // Update anchor for stability
        self.anchor_byte = layout.get_source_byte_for_line(self.top_view_line);
    }
}
```

No special handling for injected lines - they're just more ViewLines.

### Scrolling Beyond the Current Layout

The Layout is built from viewport-scoped tokens (for performance). But what happens when the user scrolls past the current layout?

**The Problem:**
```
Layout covers view lines 0-50 (from viewport tokens)
User presses PageDown → wants to see lines 50-100
But we don't have ViewLines for 50-100 yet!
```

**The Solution: Layout Expansion**

When scrolling would move past the current layout, we:

1. **Request new tokens** from the view transform for the target range
2. **Rebuild layout** to cover the new viewport
3. **Complete the scroll** using the new layout

```rust
impl SplitViewState {
    fn scroll(&mut self, offset: isize, buffer: &Buffer) {
        let layout = self.layout.as_ref().expect("layout must exist");
        let target_top = (self.viewport.top_view_line as isize + offset)
            .max(0) as usize;

        // Check if target is beyond current layout
        if target_top + self.viewport.height as usize > layout.lines.len() {
            // Need to rebuild layout for new viewport position
            // First, estimate source byte for target view line
            let target_byte = self.estimate_byte_for_view_line(target_top);

            // Request new tokens from target_byte and rebuild layout
            self.rebuild_layout_from_byte(target_byte, buffer);
        }

        // Now scroll within the (possibly rebuilt) layout
        let layout = self.layout.as_ref().unwrap();
        self.viewport.scroll(offset, layout);
    }
}
```

**Estimating Target Byte:**

When scrolling to view lines we haven't built yet, we estimate the source byte:

```rust
impl SplitViewState {
    fn estimate_byte_for_view_line(&self, target_view_line: usize) -> usize {
        // Use the last known mapping from current layout
        if let Some(ref layout) = self.layout {
            if let Some(last_line) = layout.lines.last() {
                if let Some(last_byte) = last_line.char_mappings.iter().filter_map(|m| *m).last() {
                    // Estimate: target is N lines past our last known byte
                    let lines_past = target_view_line.saturating_sub(layout.lines.len());
                    // Rough estimate: 80 bytes per line average
                    return last_byte + (lines_past * 80);
                }
            }
        }
        0
    }
}
```

This estimate doesn't need to be perfect - the view transform will give us correct tokens for whatever range we request.

### Cursor Movement Beyond Layout

Similar handling for cursor movement (all operations happen on SplitViewState):

**PageDown/PageUp:**
```rust
impl SplitViewState {
    fn page_down(&mut self, buffer: &Buffer) {
        let layout = self.layout.as_ref().expect("layout must exist");
        let page_size = self.viewport.height as usize;
        let cursor = self.cursors.primary();

        // Move cursor down by page_size view lines
        let (current_view_line, visual_col) =
            layout.source_byte_to_view_position(cursor.position);
        let target_view_line = current_view_line + page_size;

        // If target is beyond layout, expand it first
        if target_view_line >= layout.lines.len() {
            let target_byte = self.estimate_byte_for_view_line(target_view_line);
            self.rebuild_layout_from_byte(target_byte, buffer);
        }

        // Now move cursor within the expanded layout
        let layout = self.layout.as_ref().unwrap();
        self.cursors.primary_mut().move_to_view_line(target_view_line, visual_col, layout);

        // Ensure cursor is visible (may scroll viewport)
        self.ensure_cursor_visible();
    }
}
```

**Cursor at End of Layout (↓ key):**
```rust
impl SplitViewState {
    fn cursor_down(&mut self, buffer: &Buffer) {
        let layout = self.layout.as_ref().expect("layout must exist");
        let cursor = self.cursors.primary();

        let (current_view_line, visual_col) =
            layout.source_byte_to_view_position(cursor.position);
        let target_view_line = current_view_line + 1;

        // At bottom of layout?
        if target_view_line >= layout.lines.len() {
            // Check if there's more content in the buffer
            if layout.has_content_below(buffer.len()) {
                // Expand layout downward
                self.expand_layout_down(buffer);
            } else {
                // At end of file - stay put or beep
                return;
            }
        }

        // Move cursor in layout
        let layout = self.layout.as_ref().unwrap();
        self.cursors.primary_mut().move_to_view_line(target_view_line, visual_col, layout);
        self.ensure_cursor_visible();
    }
}
```

### Tracking Total View Lines

To know scroll limits without building the entire layout, we track:

```rust
pub struct Layout {
    /// ViewLines for current viewport region
    pub lines: Vec<ViewLine>,

    /// Byte range this layout covers
    pub source_range: Range<usize>,

    /// Total view lines in entire document (estimated or exact)
    pub total_view_lines: usize,

    /// How many injected lines exist in entire document
    /// (reported by view transform or computed)
    pub total_injected_lines: usize,
}
```

**Computing total_view_lines:**

Without view transform:
```rust
total_view_lines = buffer.line_count()
```

With view transform (plugin reports it):
```rust
total_view_lines = buffer.line_count() + total_injected_lines
```

The plugin knows how many headers it injects (e.g., git blame knows number of commit blocks). It reports this in the view transform response:

```typescript
editor.submitViewTransform(
    buffer_id,
    split_id,
    viewport_start,
    viewport_end,
    tokens,
    {
        compose_width: null,
        total_injected_lines: blameBlocks.length  // NEW
    }
);
```

This gives correct scroll limits without building full-document layout.

### Integration with View Transforms

View transforms remain viewport-scoped for performance (only tokenize visible range). The Layout is built from:

1. **With view transform**: Use cached `ViewTransformPayload.tokens`
2. **Without view transform**: Build tokens directly from buffer

The key insight: we don't need full-file tokens for correct scroll limits. We need to know **how many view lines exist total**. This can be:

- Computed incrementally as user scrolls through file
- Estimated from buffer line count + known injected line count
- Reported by plugin as metadata

For most cases, building layout from current viewport tokens + tracking total injected lines is sufficient.