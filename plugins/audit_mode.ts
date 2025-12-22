// Audit & Verification Plugin
// Provides a unified workflow for reviewing code changes (diffs, conflicts, AI outputs).

/// <reference path="./lib/fresh.d.ts" />
/// <reference path="./lib/types.ts" />
/// <reference path="./lib/virtual-buffer-factory.ts" />

import { VirtualBufferFactory } from "./lib/virtual-buffer-factory.ts";

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
  byteOffset: number; // Position in the virtual buffer
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
const STYLE_DISCARDED: [number, number, number] = [150, 50, 50];

// --- Helper Functions ---

async function getGitDiff(): Promise<Hunk[]> {
    editor.debug("AuditMode: Running git diff HEAD");
    const result = await editor.spawnProcess("git", ["diff", "HEAD", "--unified=3"]);
    if (result.exit_code !== 0) {
        editor.debug(`AuditMode: Git diff failed: ${result.stderr}`);
        return [];
    }

    const lines = result.stdout.split('\n');
    const hunks: Hunk[] = [];
    let currentFile = "";
    let currentHunk: Hunk | null = null;

    for (let i = 0; i < lines.length; i++) {
        const line = lines[i];

        if (line.startsWith('diff --git')) {
            const match = line.match(/diff --git a\/(.+) b\/(.+)/);
            if (match) {
                currentFile = match[2];
                currentHunk = null;
            }
        } else if (line.startsWith('@@')) {
            const match = line.match(/@@ -(\d+),?\d* \+(\d+),?\d* @@(.*)/);
            if (match && currentFile) {
                const start = parseInt(match[2]);
                currentHunk = {
                    id: `${currentFile}:${start}`,
                    file: currentFile,
                    range: { start, end: start },
                    type: 'modify',
                    lines: [],
                    status: 'pending',
                    contextHeader: match[3]?.trim() || "",
                    byteOffset: 0
                };
                hunks.push(currentHunk);
            }
        } else if (currentHunk && (line.startsWith('+') || line.startsWith('-') || line.startsWith(' '))) {
            if (!line.startsWith('---') && !line.startsWith('+++')) {
                 currentHunk.lines.push(line);
            }
        }
    }
    return hunks;
}

function renderReviewStream(): TextPropertyEntry[] {
  const entries: TextPropertyEntry[] = [];
  let currentFile = "";
  let currentByte = 0;

  state.hunks.forEach((hunk, index) => {
    if (hunk.file !== currentFile) {
      const bannerText = `\n📦 FILE: ${hunk.file}\n`;
      entries.push({
        text: bannerText,
        properties: { type: "banner", file: hunk.file, color: STYLE_FILE_BANNER, bold: true }
      });
      currentByte += bannerText.length;
      currentFile = hunk.file;
    }

    hunk.byteOffset = currentByte;

    const statusIcon = hunk.status === 'staged' ? '✓' : (hunk.status === 'discarded' ? '✗' : ' ');
    const headerText = `  ${statusIcon} @@ ${hunk.contextHeader}\n`;
    let hunkColor = STYLE_HUNK_HEADER;
    if (hunk.status === 'staged') hunkColor = STYLE_STAGED;
    else if (hunk.status === 'discarded') hunkColor = STYLE_DISCARDED;

    entries.push({
      text: headerText,
      properties: { type: "header", hunkId: hunk.id, index: index, color: hunkColor }
    });
    currentByte += headerText.length;

    hunk.lines.forEach((line) => {
        let lineStyle = hunkColor;
        if (hunk.status === 'pending') {
            if (line.startsWith('+')) lineStyle = STYLE_ADD;
            else if (line.startsWith('-')) lineStyle = STYLE_REMOVE;
        }
        const lineText = `    ${line}\n`;
        entries.push({
            text: lineText,
            properties: { type: "content", hunkId: hunk.id, color: lineStyle }
        });
        currentByte += lineText.length;
    });
  });

  if (entries.length === 0) {
      entries.push({ text: "No changes to review.\n", properties: {} });
  }

  return entries;
}

function refreshReviewStream() {
  if (state.reviewBufferId !== null) {
    const content = renderReviewStream();
    editor.setVirtualBufferContent(state.reviewBufferId, content);
  }
}

// --- Refresh Logic ---

let isUpdating = false;

async function updateHunks(): Promise<boolean> {
    const newHunks = await getGitDiff();
    
    // Merge status from existing hunks
    newHunks.forEach(hunk => {
        hunk.status = state.hunkStatus[hunk.id] || 'pending';
    });

    state.hunks = newHunks;
    return true;
}

async function refreshAuditStream() {
    if (isUpdating) return;
    isUpdating = true;
    editor.setStatus("Refreshing audit stream...");

    try {
        await updateHunks();
        refreshReviewStream();
        editor.setStatus(`Audit stream updated. Found ${state.hunks.length} hunks.`);
    } catch (e) {
        editor.debug(`AuditMode: Error updating: ${e}`);
    } finally {
        isUpdating = false;
    }
}

// --- Actions ---

globalThis.audit_stage_hunk = () => {
    const bufferId = editor.getActiveBufferId();
    const props = editor.getTextPropertiesAtCursor(bufferId);
    if (props.length > 0 && props[0].hunkId) {
        const hunkId = props[0].hunkId as string;
        state.hunkStatus[hunkId] = 'staged';
        const hunk = state.hunks.find(h => h.id === hunkId);
        if (hunk) hunk.status = 'staged';
        refreshReviewStream();
    }
};

globalThis.audit_discard_hunk = () => {
    const bufferId = editor.getActiveBufferId();
    const props = editor.getTextPropertiesAtCursor(bufferId);
    if (props.length > 0 && props[0].hunkId) {
        const hunkId = props[0].hunkId as string;
        state.hunkStatus[hunkId] = 'discarded';
        const hunk = state.hunks.find(h => h.id === hunkId);
        if (hunk) hunk.status = 'discarded';
        refreshReviewStream();
    }
};

globalThis.audit_undo_action = () => {
    const bufferId = editor.getActiveBufferId();
    const props = editor.getTextPropertiesAtCursor(bufferId);
    if (props.length > 0 && props[0].hunkId) {
        const hunkId = props[0].hunkId as string;
        state.hunkStatus[hunkId] = 'pending';
        const hunk = state.hunks.find(h => h.id === hunkId);
        if (hunk) hunk.status = 'pending';
        refreshReviewStream();
    }
};

globalThis.audit_next_hunk = () => {
    const bufferId = editor.getActiveBufferId();
    const props = editor.getTextPropertiesAtCursor(bufferId);
    let currentIndex = -1;
    if (props.length > 0 && props[0].index !== undefined) {
        currentIndex = props[0].index as number;
    }
    const nextIndex = currentIndex + 1;
    if (nextIndex < state.hunks.length) {
        const hunk = state.hunks[nextIndex];
        editor.setBufferCursor(bufferId, hunk.byteOffset);
    }
};

globalThis.audit_prev_hunk = () => {
    const bufferId = editor.getActiveBufferId();
    const props = editor.getTextPropertiesAtCursor(bufferId);
    let currentIndex = state.hunks.length;
    if (props.length > 0 && props[0].index !== undefined) {
        currentIndex = props[0].index as number;
    }
    const prevIndex = currentIndex - 1;
    if (prevIndex >= 0) {
        const hunk = state.hunks[prevIndex];
        editor.setBufferCursor(bufferId, hunk.byteOffset);
    }
};

globalThis.audit_refresh = () => {
    refreshAuditStream();
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
    if (data.split_id === activeDiffView.leftSplitId) {
        (editor as any).setSplitScroll(activeDiffView.rightSplitId, data.top_byte);
    } else if (data.split_id === activeDiffView.rightSplitId) {
        (editor as any).setSplitScroll(activeDiffView.leftSplitId, data.top_byte);
    }
};

globalThis.audit_drill_down = async () => {
    const bufferId = editor.getActiveBufferId();
    const props = editor.getTextPropertiesAtCursor(bufferId);
    if (props.length > 0 && props[0].hunkId) {
        const hunkId = props[0].hunkId as string;
        const hunk = state.hunks.find(h => h.id === hunkId);
        if (!hunk) return;

        const gitShow = await editor.spawnProcess("git", ["show", `HEAD:${hunk.file}`]);
        if (gitShow.exit_code !== 0) return;

        const leftBufferId = await editor.createVirtualBuffer({
            name: `HEAD:${hunk.file}`,
            mode: "special",
            read_only: true,
            entries: [{ text: gitShow.stdout, properties: {} }],
            show_line_numbers: true
        });

        editor.openFile(hunk.file, hunk.range.start, 0);
        const rightBufferId = editor.getActiveBufferId();
        const rightSplitId = (editor as any).getActiveSplitId();

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

        editor.on("viewport_changed", "on_viewport_changed");
    }
};

// --- Initialization ---

globalThis.start_audit_mode = async () => {
    editor.setStatus("Generating Audit Stream...");
    editor.setContext("audit-mode", true);

    await updateHunks();

    const bufferId = await VirtualBufferFactory.create({
        name: "*Audit Stream*",
        mode: "audit-mode",
        readOnly: true,
        entries: renderReviewStream(),
        showLineNumbers: false
    });

    state.reviewBufferId = bufferId;
    editor.setStatus(`Audit Mode Active. Found ${state.hunks.length} hunks. Press 'r' to refresh.`);

    editor.on("buffer_activated", "on_audit_buffer_activated");
    editor.on("buffer_closed", "on_audit_buffer_closed");
};

globalThis.stop_audit_mode = () => {
    state.reviewBufferId = null;
    editor.setContext("audit-mode", false);
    editor.off("buffer_activated", "on_audit_buffer_activated");
    editor.off("buffer_closed", "on_audit_buffer_closed");
    editor.setStatus("Audit Mode stopped.");
};

globalThis.on_audit_buffer_activated = (data: any) => {
    if (data.buffer_id === state.reviewBufferId) {
        refreshAuditStream();
    }
};

globalThis.on_audit_buffer_closed = (data: any) => {
    if (data.buffer_id === state.reviewBufferId) {
        stop_audit_mode();
    }
};

// Register Modes and Commands
editor.registerCommand("Start Audit Mode", "Start code review session", "start_audit_mode", "global");
editor.registerCommand("Stop Audit Mode", "Stop the audit session", "stop_audit_mode", "audit-mode");
editor.registerCommand("Refresh Audit Stream", "Refresh the list of changes", "audit_refresh", "audit-mode");

editor.on("buffer_closed", "on_buffer_closed");

editor.defineMode("audit-mode", "normal", [
    ["s", "audit_stage_hunk"],
    ["d", "audit_discard_hunk"],
    ["u", "audit_undo_action"],
    ["n", "audit_next_hunk"],
    ["p", "audit_prev_hunk"],
    ["r", "audit_refresh"],
    ["Enter", "audit_drill_down"],
    ["q", "close_buffer"],
], true);

editor.debug("Audit Mode plugin loaded");
