// Audit & Verification Plugin
// Provides a unified workflow for reviewing code changes (diffs, conflicts, AI outputs).

/// <reference path="./lib/fresh.d.ts" />
/// <reference path="./lib/types.ts" />
/// <reference path="./lib/virtual-buffer-factory.ts" />

import { VirtualBufferFactory, VirtualBufferOptions } from "./lib/virtual-buffer-factory.ts";

/**
 * Hunk status for staging
 */
type HunkStatus = 'pending' | 'staged' | 'discarded';

/**
 * A diff hunk (block of changes)
 */
interface Hunk {
  id: string;
  file: string;
  range: { start: number; end: number }; // Line numbers in original file
  type: 'add' | 'remove' | 'modify';
  lines: string[];
  status: HunkStatus;
  contextHeader: string; // e.g., "fn process_data()"
}

/**
 * Audit Session State
 */
interface AuditState {
  hunks: Hunk[];
  // Mapping from hunk ID to status (persisted across re-renders)
  hunkStatus: Record<string, HunkStatus>;
  // The buffer ID of the main "Review Stream"
  reviewBufferId: number | null;
  // Currently focused hunk index
  focusedHunkIndex: number;
}

const state: AuditState = {
  hunks: [],
  hunkStatus: {},
  reviewBufferId: null,
  focusedHunkIndex: -1,
};

// --- Colors & Styles ---
const STYLE_HUNK_HEADER: [number, number, number] = [100, 100, 255]; // Blueish
const STYLE_FILE_BANNER: [number, number, number] = [200, 200, 100]; // Yellowish
const STYLE_ADD: [number, number, number] = [50, 200, 50]; // Green
const STYLE_REMOVE: [number, number, number] = [200, 50, 50]; // Red
const STYLE_STAGED: [number, number, number] = [100, 100, 100]; // Dimmed/Grey
const STYLE_DISCARDED: [number, number, number] = [100, 50, 50]; // Dimmed Red (strikethrough logic handled by content gen if needed)

// --- Helper Functions ---

/**
 * Generate a unique ID for a hunk
 */
function generateHunkId(file: string, range: { start: number; end: number }): string {
  return `${file}:${range.start}-${range.end}`;
}

/**
 * Calculate structural diff between two strings
 */
async function computeDiff(original: string, modified: string): Promise<Hunk[]> {
  const result = (editor as any).diffLines(original, modified);
  if (result.equal) return [];

  // TODO: Convert diff results to Hunk objects properly.
  // For now, still returns mock but would use 'result.changed_lines'
  return []; 
}

/**
 * Run git diff to get pending changes
 */
async function getGitDiff(): Promise<Hunk[]> {
    const result = await editor.spawnProcess("git", ["diff", "HEAD"]);
    if (result.exit_code !== 0) {
        editor.debug(`Git diff failed: ${result.stderr}`);
        return [];
    }

    // Basic parser for git unified diff format
    const lines = result.stdout.split('\n');
    const hunks: Hunk[] = [];
    let currentFile = "";
    let currentHunk: Hunk | null = null;

    for (let i = 0; i < lines.length; i++) {
        const line = lines[i];
        if (line.startsWith('--- a/')) {
            currentFile = line.substring(6);
        } else if (line.startsWith('+++ b/')) {
            // currentFile already set
        } else if (line.startsWith('@@')) {
            const match = line.match(/@@ -(\d+),?\d* \+(\d+),?\d* @@/);
            if (match) {
                const start = parseInt(match[2]);
                currentHunk = {
                    id: `${currentFile}:${start}`,
                    file: currentFile,
                    range: { start, end: start }, // Simplified
                    type: 'modify',
                    lines: [],
                    status: 'pending',
                    contextHeader: line.split('@@')[2]?.trim() || ""
                };
                hunks.push(currentHunk);
            }
        } else if (currentHunk && (line.startsWith('+') || line.startsWith('-') || line.startsWith(' '))) {
            currentHunk.lines.push(line);
        }
    }

    return hunks;
}

/**
 * Render the Review Stream buffer content
 */
function renderReviewStream(): TextPropertyEntry[] {
  const entries: TextPropertyEntry[] = [];

  let currentFile = "";

  state.hunks.forEach((hunk, index) => {
    // File Banner (only if file changes)
    if (hunk.file !== currentFile) {
      entries.push({
        text: `\n📦 FILE: ${hunk.file}\n`,
        properties: {
          type: "banner",
          file: hunk.file,
          color: STYLE_FILE_BANNER,
          bold: true,
        },
      });
      currentFile = hunk.file;
    }

    // Hunk Status Style
    let hunkColor = STYLE_HUNK_HEADER;
    let contentColor: [number, number, number] | undefined = undefined; // Default
    
    if (hunk.status === 'staged') {
        hunkColor = STYLE_STAGED;
    } else if (hunk.status === 'discarded') {
        hunkColor = STYLE_DISCARDED;
    }

    // Hunk Header
    const statusIcon = hunk.status === 'staged' ? '✓' : (hunk.status === 'discarded' ? '✗' : ' ');
    entries.push({
      text: `  ${statusIcon} @@ ${hunk.contextHeader}\n`,
      properties: {
        type: "header",
        hunkId: hunk.id,
        index: index,
        color: hunkColor,
      },
    });

    // Hunk Content
    hunk.lines.forEach((line) => {
        let lineStyle = hunkColor;
        if (hunk.status === 'pending') {
            if (line.startsWith('+')) lineStyle = STYLE_ADD;
            else if (line.startsWith('-')) lineStyle = STYLE_REMOVE;
        }

        entries.push({
            text: `    ${line}\n`,
            properties: {
                type: "content",
                hunkId: hunk.id,
                color: lineStyle,
            }
        });
    });
  });

  if (entries.length === 0) {
      entries.push({
          text: "No changes to review.\n",
          properties: {}
      });
  }

  return entries;
}

/**
 * Refresh the Review Stream buffer
 */
function refreshReviewStream() {
  if (state.reviewBufferId !== null) {
    const content = renderReviewStream();
    editor.setVirtualBufferContent(state.reviewBufferId, content);
  }
}

// --- Actions ---

globalThis.audit_stage_hunk = () => {
    const bufferId = editor.getActiveBufferId();
    if (bufferId !== state.reviewBufferId) return;

    const props = editor.getTextPropertiesAtCursor(bufferId);
    if (props.length > 0 && props[0].hunkId) {
        const hunkId = props[0].hunkId as string;
        state.hunkStatus[hunkId] = 'staged';
        
        // Update local hunk object status
        const hunk = state.hunks.find(h => h.id === hunkId);
        if (hunk) hunk.status = 'staged';

        refreshReviewStream();
        editor.setStatus(`Staged hunk ${hunkId}`);
    }
};

globalThis.audit_discard_hunk = () => {
    const bufferId = editor.getActiveBufferId();
    if (bufferId !== state.reviewBufferId) return;

    const props = editor.getTextPropertiesAtCursor(bufferId);
    if (props.length > 0 && props[0].hunkId) {
        const hunkId = props[0].hunkId as string;
        state.hunkStatus[hunkId] = 'discarded';

        // Update local hunk object status
        const hunk = state.hunks.find(h => h.id === hunkId);
        if (hunk) hunk.status = 'discarded';

        refreshReviewStream();
        editor.setStatus(`Discarded hunk ${hunkId}`);
    }
};

globalThis.audit_undo_action = () => {
    const bufferId = editor.getActiveBufferId();
    if (bufferId !== state.reviewBufferId) return;

    const props = editor.getTextPropertiesAtCursor(bufferId);
    if (props.length > 0 && props[0].hunkId) {
        const hunkId = props[0].hunkId as string;
        state.hunkStatus[hunkId] = 'pending';

        // Update local hunk object status
        const hunk = state.hunks.find(h => h.id === hunkId);
        if (hunk) hunk.status = 'pending';

        refreshReviewStream();
        editor.setStatus(`Reset hunk ${hunkId}`);
    }
};

/**
 * Side-by-Side Diff State
 */
interface DiffViewState {
    leftBufferId: number;
    rightBufferId: number;
    leftSplitId: number;
    rightSplitId: number;
}

let activeDiffView: DiffViewState | null = null;

globalThis.on_viewport_changed = (data: any) => {
    if (!activeDiffView) return;

    // Synchronize scrolling between left and right panes
    if (data.split_id === activeDiffView.leftSplitId) {
        (editor as any).setSplitScroll(activeDiffView.rightSplitId, data.top_byte);
    } else if (data.split_id === activeDiffView.rightSplitId) {
        (editor as any).setSplitScroll(activeDiffView.leftSplitId, data.top_byte);
    }
};

globalThis.audit_drill_down = async () => {
    const bufferId = editor.getActiveBufferId();
    if (bufferId !== state.reviewBufferId) return;

    const props = editor.getTextPropertiesAtCursor(bufferId);
    if (props.length > 0 && props[0].hunkId) {
        const hunkId = props[0].hunkId as string;
        const hunk = state.hunks.find(h => h.id === hunkId);
        if (!hunk) return;

        editor.setStatus(`Opening Side-by-Side for ${hunk.file}`);

        // 1. Get original content from git
        const gitShow = await editor.spawnProcess("git", ["show", `HEAD:${hunk.file}`]);
        if (gitShow.exit_code !== 0) {
            editor.setStatus(`Error getting original file: ${gitShow.stderr}`);
            return;
        }

        // 2. Create Left Buffer (Original)
        const leftBufferId = await editor.createVirtualBuffer({
            name: `HEAD:${hunk.file}`,
            mode: "special",
            read_only: true,
            entries: [{ text: gitShow.stdout, properties: {} }],
            show_line_numbers: true
        });

        // 3. Open Right Buffer (Working Copy)
        // For now, we'll just open the real file. 
        // In a real implementation, we might use a temporary buffer to avoid edits during audit.
        editor.openFile(hunk.file, hunk.range.start, 0);
        const rightBufferId = editor.getActiveBufferId();
        const rightSplitId = (editor as any).getActiveSplitId();

        // 4. Create Split for Left Buffer
        // We want Side-by-Side, so vertical split.
        const leftResult = await editor.createVirtualBufferInSplit({
            name: `HEAD:${hunk.file}`,
            mode: "special",
            read_only: true,
            entries: [{ text: gitShow.stdout, properties: {} }],
            ratio: 0.5,
            direction: "vertical",
            show_line_numbers: true
        });

        activeDiffView = {
            leftBufferId: leftResult.buffer_id,
            rightBufferId: rightBufferId,
            leftSplitId: leftResult.split_id!,
            rightSplitId: rightSplitId
        };

        // 5. Register scroll listener
        editor.on("viewport_changed", "on_viewport_changed");

        // 6. Scroll both to the hunk position
        // We need a way to get byte offset from line number
        // For now, we'll assume 80 chars per line as a guess or just not scroll.
        // (Better: editor.getLineByteOffset(line))
    }
};

/**
 * Conflict Merge State
 */
interface ConflictMergeState {
    file: string;
    localBufferId: number;
    remoteBufferId: number;
    resultBufferId: number;
    localSplitId: number;
    remoteSplitId: number;
    resultSplitId: number;
}

let activeMergeView: ConflictMergeState | null = null;
let auditUpdateInterval: number | null = null;
let isUpdating = false;

/**
 * Fetch latest git diff, parse it, and update hunks state.
 * Returns true if changes were detected.
 */
async function updateHunks(): Promise<boolean> {
    const newHunks = await getGitDiff();
    
    // Simple change detection: compare hunk IDs and content
    if (newHunks.length !== state.hunks.length || 
        !newHunks.every((hunk, i) => hunk.id === state.hunks[i]?.id && hunk.lines.join() === state.hunks[i]?.lines.join())) {
        
        state.hunks = newHunks;
        // Preserve status of hunks that still exist
        state.hunks.forEach(hunk => {
            hunk.status = state.hunkStatus[hunk.id] || 'pending';
        });

        return true;
    }

    return false;
}

/**
 * Periodically checks for changes and updates the audit stream.
 * Debounced to prevent overlapping updates.
 */
async function scheduleAuditUpdate() {
    if (isUpdating) return;
    isUpdating = true;

    try {
        if (await updateHunks()) {
            refreshReviewStream();
            editor.debug("Audit Stream updated with new changes.");
        }
    } catch (e) {
        editor.debug(`Error updating audit stream: ${e}`);
    } finally {
        isUpdating = false;
    }
}

globalThis.merge_get_left = () => {
    if (!activeMergeView) return;
    // TODO: Implement "Get Left" - copy current hunk from local to result
    editor.setStatus("Merge: Pulling from LOCAL (Not fully implemented)");
};

globalThis.merge_get_right = () => {
    if (!activeMergeView) return;
    // TODO: Implement "Get Right" - copy current hunk from remote to result
    editor.setStatus("Merge: Pulling from REMOTE (Not fully implemented)");
};

globalThis.start_conflict_merge = async (file: string) => {
    editor.setStatus(`Starting Conflict Merge for ${file}`);

    // 1. Get components (simplified - in real git would be from index)
    // For demo, we'll assume the file has conflict markers.
    const content = await editor.readFile(file);
    
    // 2. Create 3 buffers
    // In real implementation, we'd extract <<<<<, =====, >>>>> sections.
    const resultBufferId = await editor.openFile(file, 0, 0); // Editable
    const localBufferId = await editor.createVirtualBuffer({
        name: `LOCAL:${file}`,
        mode: "special",
        read_only: true,
        entries: [{ text: "Local changes...\n", properties: {} }]
    });
    const remoteBufferId = await editor.createVirtualBuffer({
        name: `REMOTE:${file}`,
        mode: "special",
        read_only: true,
        entries: [{ text: "Remote changes...\n", properties: {} }]
    });

    // 3. Setup 3-pane layout
    // Current layout: [Result]
    const resultSplitId = editor.getActiveSplitId();
    
    // Split vertically to get [Local | Result]
    const localResult = await editor.createVirtualBufferInSplit({
        name: `LOCAL:${file}`,
        mode: "special",
        read_only: true,
        entries: [{ text: "Local changes...\n", properties: {} }],
        ratio: 0.33,
        direction: "vertical"
    });

    // Split result split again to get [Local | Result | Remote]
    editor.focusSplit(resultSplitId);
    const remoteResult = await editor.createVirtualBufferInSplit({
        name: `REMOTE:${file}`,
        mode: "special",
        read_only: true,
        entries: [{ text: "Remote changes...\n", properties: {} }],
        ratio: 0.5, // 50% of the remaining 66%
        direction: "vertical"
    });

    activeMergeView = {
        file,
        localBufferId,
        remoteBufferId,
        resultBufferId,
        localSplitId: localResult.split_id!,
        remoteSplitId: remoteResult.split_id!,
        resultSplitId: resultSplitId
    };

    editor.focusSplit(resultSplitId);
    editor.setContext("merge-mode", true);
    editor.setStatus("Conflict Merge Active. Use <Leader>gl or <Leader>gr to pick changes.");
};

// Register Merge Commands
editor.registerCommand("Accept Local", "Pick change from Left pane", "merge_get_left", "merge-mode");
editor.registerCommand("Accept Remote", "Pick change from Right pane", "merge_get_right", "merge-mode");

editor.defineMode("merge-mode", "normal", [
    ["l", "merge_get_left"],
    ["r", "merge_get_right"],
], false);

globalThis.stop_audit_mode = () => {
    if (auditUpdateInterval) {
        clearInterval(auditUpdateInterval);
        auditUpdateInterval = null;
    }
    state.reviewBufferId = null;
    editor.setContext("audit-mode", false);
    editor.setStatus("Audit Mode stopped.");
};

// --- Initialization ---

globalThis.start_audit_mode = async () => {
    editor.setStatus("Generating Audit Stream...");
    editor.setContext("audit-mode", true);

    await scheduleAuditUpdate();

    // 2. Create Virtual Buffer
    const bufferId = await VirtualBufferFactory.create({
        name: "*Audit Stream*",
        mode: "audit-mode",
        readOnly: true,
        entries: renderReviewStream(),
        showLineNumbers: false
    });

    state.reviewBufferId = bufferId;
    editor.setStatus(`Audit Mode Active. Found ${state.hunks.length} hunks. Auto-updating...`);

    // Start auto-update interval
    if (auditUpdateInterval) clearInterval(auditUpdateInterval);
    auditUpdateInterval = setInterval(scheduleAuditUpdate, 3000); // Check every 3 seconds
};

globalThis.on_buffer_closed = (data: any) => {
    if (data.buffer_id === state.reviewBufferId) {
        stop_audit_mode();
    }
};

// Register Modes and Commands
editor.registerCommand("Start Audit Mode", "Code review session", "start_audit_mode", "global");
editor.registerCommand("Stop Audit Mode", "Stop the audit session", "stop_audit_mode", "audit-mode");

editor.on("buffer_closed", "on_buffer_closed");

editor.defineMode("audit-mode", "normal", [
    ["s", "audit_stage_hunk"],
    ["d", "audit_discard_hunk"],
    ["u", "audit_undo_action"],
    ["n", "audit_next_hunk"],
    ["p", "audit_prev_hunk"],
    ["Enter", "audit_drill_down"],
    ["q", "close_buffer"], // Allow closing the audit buffer
], true);

editor.debug("Audit Mode plugin loaded");
