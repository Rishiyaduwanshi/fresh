## 1. Executive Summary

**Implementation Status**: Initial Version Complete
*   **Rust Core**: Sync scrolling and viewport hooks implemented.
*   **Plugin**: Unified Review stream and Side-by-Side drill-down implemented.
*   **Conflict Mode**: 3-pane layout scaffolded.

This design introduces a dedicated "Audit Mode" to Fresh Editor. Unlike traditional code editing, which focuses on creation, Audit Mode focuses on **verification**. It assumes the code has already been generated (by an AI or colleague) and transforms the editor into a high-speed "decision engine" for reviewing, modifying, and accepting changes.

The system relies on three core view modes:
1.  **Unified Review**: A continuous, vertical stream of changes for rapid "Stage/Discard" triage. (Implemented via `git diff HEAD`)
2.  **Side-by-Side Diff**: A synchronized split-view for deep inspection of complex logic. (Implemented via `ViewportChanged` hook)
3.  **Conflict Merge**: A 3-pane layout (Local | Result | Remote) for resolving merge conflicts. (Scaffolded)

---

## 2. Architecture Overview

The feature is implemented as a **Core TypeScript Plugin** (`plugins/audit_mode.ts`) backed by specialized Rust primitives.

### Rust Core
*   **Virtual Buffers**: Support `TextPropertyEntry` with per-line metadata.
*   **Overlays**: Used for diff highlighting (red/green backgrounds).
*   **Sync Scrolling**: Implemented in `SplitManager` via `sync_group`.
*   **Viewport Hook**: `ViewportChanged` hook notifies plugins of scroll events.
*   **Diff API**: `diffLines` exposed to TS runtime.

### Plugin Logic
*   **State Management**: Tracks "Staged/Rejected" status of every hunk in memory.
*   **Diff Generation**: Uses `git diff HEAD` to populate the review stream.
*   **Render Loop**: Converts diff output into `VirtualBuffer` entries with metadata.

---

## 3. Feature I: Unified Diff Mode (The "Review Stream")

This is the default view when entering Audit Mode. It linearizes changes across multiple files into a single, scrollable document.

### Visual Layout
The view is a read-only `VirtualBuffer` containing a stream of "Hunks" parsed from git.

```text
┌──────────────────────────────────────────────────────────────┐
│ 📦 FILE: src/lib.rs                                          │
│   @@ fn calculate_total()                                    │
│      let total = 0;                                          │
│ -    for item in items {                                     │
│ +    for item in items.iter() {                              │
│      total += item.price;                                    │
└──────────────────────────────────────────────────────────────┘
```

### UX Mechanics
*   **Hunk-Centric Navigation**: `n`/`p` jumps between hunk headers.
*   **Granular Staging**:
    *   `s` (Stage): Marks hunk as accepted (dimmed/gray style).
    *   `d` (Discard): Marks hunk as rejected (red/strike style).
*   **Drill Down**: Pressing `Enter` on any hunk switches to **Side-by-Side Diff** view.

---

## 4. Feature II: Side-by-Side Diff UX

Designed for complex logic where context is key. This view coordinates two splits to ensure they always show the same semantic region of code.

### Visual Layout
A 2-column vertical split.

```text
┌──────────────────────────────┬──────────────────────────────┐
│ HEAD:src/main.rs             │ src/main.rs (Working)        │
│   let x = 10;                │   let x = 20;                │
│   // removed line            │                              │
│                              │   new_function();            │
└──────────────────────────────┴──────────────────────────────┘
```

### UX Mechanics
*   **Synchronized Scrolling**: Moving the cursor or scrolling in one pane automatically scrolls the other.
*   **Implementation**: Uses the `ViewportChanged` hook to propagate `top_byte` position across splits.

---

## 5. Feature III: Conflict Resolution (3-Pane Merge)

Standard layout for resolving git merge conflicts.

### Visual Layout
A 3-column layout (33% / 34% / 33%).

```text
┌──────────────┬────────────────────┬──────────────┐
│ LOCAL (Left) │   RESULT (Center)  │ REMOTE (Right│
│              │                    │              │
│ let a = 1;   │ <<<<<<<            │ let a = 2;   │
│              │ let a = ?          │              │
│              │ >>>>>>>            │              │
└──────────────┴────────────────────┴──────────────┘
```

### UX Mechanics
*   **Key Actions**:
    *   `l`: Pick change from Left (Local).
    *   `r`: Pick change from Right (Remote).
*   **Navigation**: Cursor stays in the **Center** pane (the editable Result).

---

## 6. Keyboard Shortcut Cheat Sheet (Audit Mode)

| Key | Command | Description |
| :--- | :--- | :--- |
| **Navigation** | | |
| `j` / `k` | `move_up` / `down` | Standard movement |
| `n` / `p` | `audit_next_hunk` / `prev` | **Jump** to next/previous hunk header |
| `Enter` | `audit_drill_down` | Open focused hunk in Side-by-Side Diff |
| **Review** | | |
| `s` | `audit_stage_hunk` | **Keep** the change (Stage) |
| `d` | `audit_discard_hunk` | **Reject** the change (Discard) |
| `u` | `audit_undo` | Undo staging decision on this hunk |
| **Conflict Resolution** | | |
| `l` | `merge_get_left` | Accept Local change |
| `r` | `merge_get_right` | Accept Remote change |

