# Audit & Verification Tool Design

**Status**: Proposed
**Date**: 2025-12-22
**Context**: Design for a TUI-based "Audit & Verification" workflow within Fresh Editor, specifically tailored for reviewing and merging high-volume output from AI agents.

## 1. Executive Summary

This design introduces a dedicated "Audit Mode" to Fresh Editor. Unlike traditional code editing, which focuses on creation, Audit Mode focuses on **verification**. It assumes the code has already been generated (by an AI or colleague) and transforms the editor into a high-speed "decision engine" for reviewing, modifying, and accepting changes.

The system relies on three core view modes:
1.  **Unified Review**: A continuous, vertical stream of changes for rapid "Stage/Discard" triage.
2.  **Side-by-Side Diff**: A synchronized split-view for deep inspection of complex logic.
3.  **Conflict Merge**: A 3-pane layout (Local | Result | Remote) for resolving merge conflicts.

---

## 2. Architecture Overview

The feature will be implemented as a **Core TypeScript Plugin** (`plugins/audit_mode.ts`) backed by specialized Rust primitives.

### Rust Core Requirements
*   **Virtual Buffers**: Enhanced to support `TextPropertyEntry` with per-line metadata (already exists).
*   **Overlays**: Used extensively for diff highlighting (red/green backgrounds) and structural markers.
*   **Sync Scrolling**: A new capability in `SplitManager` to lock the viewports of two distinct splits based on a line-mapping algorithm.
*   **Structural Diffing**: Exposure of a `compute_structural_diff` API (potentially via `tree-sitter` and `dissimilar` crates) to the plugin runtime.

### Plugin Logic
*   **State Management**: Tracks the "Staged/Rejected" status of every hunk in memory.
*   **Diff Generation**: Orchestrates the comparison of files (Git vs Working, Base vs Remote).
*   **Render Loop**: Converts raw diff data into the `VirtualBuffer` "Audit Stream".

---

## 3. Feature I: Unified Diff Mode (The "Review Stream")

This is the default view when entering Audit Mode. It linearizes changes across multiple files into a single, scrollable document.

### Visual Layout
The view is a read-only `VirtualBuffer` containing a generated stream of "Hunks".

```text
┌──────────────────────────────────────────────────────────────┐
│ [FILE] src/lib.rs                                            │
│ ┌─ fn calculate_total() ───────────────────────────────────┐ │
│ │  10 |     let total = 0;                                 │ │
│ │  11 | -   for item in items {                            │ │
│ │  11 | +   for item in items.iter() {                     │ │
│ │  12 |         total += item.price;                       │ │
│ └──────────────────────────────────────────────────────────┘ │
│                                                              │
│ [FILE] src/utils.rs                                          │
│ ┌─ fn format_date() ───────────────────────────────────────┐ │
│ ...                                                          │
└──────────────────────────────────────────────────────────────┘
```

### UX Mechanics
*   **Hunk-Centric Navigation**: `n`/`p` jumps between hunk headers, skipping unchanged context.
*   **Granular Staging**:
    *   `s` (Stage): Marks the hunk as accepted. Visually, the hunk "dims" (opacity 50%) or gets a green checkmark gutter icon.
    *   `x` (Discard): Marks the hunk as rejected. Visually applies strikethrough.
*   **Drill Down**: Pressing `Enter` on any hunk instantly switches to **Side-by-Side Diff** view for that specific file/hunk context.

---

## 4. Feature II: Side-by-Side Diff UX

Designed for complex logic where context is key. This view coordinates two splits to ensure they always show the same semantic region of code.

### Visual Layout
A 2-column split.

```text
┌──────────────────────────────┬──────────────────────────────┐
│ src/main.rs (HEAD)           │ src/main.rs (Working Copy)   │
│   let x = 10;                │   let x = 20;                │
│   // removed line            │                              │
│                              │   new_function();            │
└──────────────────────────────┴──────────────────────────────┘
```

### UX Mechanics
*   **Synchronized Scrolling**: Moving the cursor in the Left pane automatically scrolls the Right pane. The alignment is not linear (1:1) but topological, based on the LCS (Longest Common Subsequence) diff map.
*   **Intraline Highlighting**:
    *   Changes are highlighted at the character level.
    *   Visual priority: `DiffText` (brightest) > `DiffAdd`/`DiffDelete` (background).
*   **Invocation**:
    *   **From Unified Mode**: via `Enter`.
    *   **Manual**: `Diff with Git` (HEAD vs Working) or `Diff Two Files` (File A vs File B).

---

## 5. Feature III: Conflict Resolution (3-Pane Merge)

Standard layout for resolving git merge conflicts (`<<<<<<<`, `=======`, `>>>>>>>`).

### Visual Layout
A 3-column layout, maximizing horizontal space for the crucial "Center" pane.

```text
┌──────────────────┬──────────────────────┬──────────────────┐
│ LOCAL (Left)     │   RESULT (Center)    │ REMOTE (Right)   │
│ (Read-Only)      │   (Editable)         │ (Read-Only)      │
│                  │                      │                  │
│ let timeout = 1; │ <<<<<<<              │ let timeout = 5; │
│                  │ let timeout = ?      │                  │
│                  │ >>>>>>>              │                  │
└──────────────────┴──────────────────────┴──────────────────┘
```

### UX Mechanics
*   **The Flow**: The user mentally (and functionally) pulls changes from the sides into the center.
*   **Key Actions**:
    *   `<Leader>gl` ("Get Left"): Replaces conflict block in Center with Left content.
    *   `<Leader>gr` ("Get Right"): Replaces conflict block in Center with Right content.
    *   `<Leader>gm` ("Merge"): Opens a manual edit prompt or attempts intelligent synthesis.
*   **Navigation**: Cursor is locked to the Center pane. `[c` and `]c` jump to the previous/next conflict marker.

---

## 6. Keyboard Shortcut Cheat Sheet (Audit Mode)

| Key | Command | Description |
| :--- | :--- | :--- |
| **Navigation** | | |
| `j` / `k` | `audit_next_line` / `prev` | Standard movement |
| `n` / `N` | `audit_next_hunk` / `prev` | **Jump** to next/previous hunk header |
| `}` / `{` | `audit_next_file` / `prev` | Jump to next/previous File Banner |
| `Enter` | `audit_drill_down` | Open focused hunk in Side-by-Side Diff |
| **Review** | | |
| `s` | `audit_stage_hunk` | **Keep** the change (Stage) |
| `d` | `audit_discard_hunk` | **Reject** the change (Discard) |
| `u` | `audit_undo` | Undo staging decision on this hunk |
| `c` | `audit_commit` | Finalize and apply staged changes to disk |
| **Conflict Resolution** | | |
| `<Leader>gl` | `merge_get_left` | Accept Local change |
| `<Leader>gr` | `merge_get_right` | Accept Remote change |
| `]c` | `merge_next_conflict` | Jump to next conflict marker |

## 7. Implementation Roadmap

1.  **Rust Core**:
    *   Implement `SplitManager::sync_scroll(group_id)`.
    *   Expose structural diffing to TS runtime.
2.  **Plugin (Phase 1)**:
    *   Build the `Unified Diff` generator.
    *   Implement the Hunk Staging state machine.
3.  **Plugin (Phase 2)**:
    *   Implement `Side-by-Side` view using two temporary read-only buffers.
    *   Implement the "Drill Down" transition.
4.  **Plugin (Phase 3)**:
    *   Implement the 3-Pane Conflict view.
    *   Add git conflict marker parsing.
