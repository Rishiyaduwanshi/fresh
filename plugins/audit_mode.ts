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

async function getGitDiff(): Promise<Hunk[]> {
    editor.debug("AuditMode: Running git diff HEAD");
    const result = await editor.spawnProcess("git", ["diff", "HEAD", "--unified=3"]);
    if (result.exit_code !== 0) {
        editor.debug(`AuditMode: Git diff failed: ${result.stderr}`);
        return [];
    }
    editor.debug(`AuditMode: Git diff output:\n${result.stdout}`);

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
                editor.debug(`AuditMode: Parsing diff for file: ${currentFile}`);
            }
        } else if (line.startsWith('--- a/')) {
            if (currentFile === "") {
                const path = line.substring(6);
                if (path !== '/dev/null') {
                    currentFile = path;
                }
            }
        } else if (line.startsWith('+++ b/')) {
            if (currentFile === "" || currentFile === "/dev/null") {
                currentFile = line.substring(6);
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
                    contextHeader: match[3]?.trim() || ""
                };
                hunks.push(currentHunk);
            }
        } else if (currentHunk && (line.startsWith('+') || line.startsWith('-') || line.startsWith(' '))) {
            if (!line.startsWith('---') && !line.startsWith('+++')) {
                 currentHunk.lines.push(line);
            }
        }
    }
    editor.debug(`AuditMode: Parsed ${hunks.length} hunks.`);
    return hunks;
}

function renderReviewStream(): TextPropertyEntry[] {
  const entries: TextPropertyEntry[] = [];
  let currentFile = "";

  state.hunks.forEach((hunk, index) => {
    if (hunk.file !== currentFile) {
      entries.push({
        text: `\n📦 FILE: ${hunk.file}\n`,
        properties: { type: "banner", file: hunk.file }
      });
      currentFile = hunk.file;
    }

    const statusIcon = hunk.status === 'staged' ? '✓' : (hunk.status === 'discarded' ? '✗' : ' ');
    entries.push({
      text: `  ${statusIcon} @@ ${hunk.contextHeader}\n`,
      properties: { type: "header", hunkId: hunk.id, index: index }
    });

    hunk.lines.forEach((line) => {
        entries.push({
            text: `    ${line}\n`,
            properties: { type: "content", hunkId: hunk.id }
        });
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
    editor.debug("AuditMode: Refreshed review stream.");
  }
}

// --- On-demand Update Logic ---

let isUpdating = false;

async function updateHunks(): Promise<boolean> {
    const newHunks = await getGitDiff();
    
    const hasChanged = newHunks.length !== state.hunks.length || 
        !newHunks.every((hunk, i) => hunk.id === state.hunks[i]?.id && hunk.lines.join() === state.hunks[i]?.lines.join());

    if (hasChanged) {
        editor.debug("AuditMode: Changes detected.");
        state.hunks = newHunks;
        state.hunks.forEach(hunk => {
            hunk.status = state.hunkStatus[hunk.id] || 'pending';
        });
        return true;
    }

    editor.debug("AuditMode: No changes detected.");
    return false;
}

async function refreshAuditStream() {
    editor.debug("AuditMode: Refresh triggered.");
    if (isUpdating) {
        editor.debug("AuditMode: Update already in progress, skipping.");
        return;
    }
    isUpdating = true;
    editor.setStatus("Refreshing audit stream...");

    try {
        if (await updateHunks()) {
            refreshReviewStream();
            editor.setStatus(`Audit stream updated. Found ${state.hunks.length} hunks.`);
        } else {
            editor.setStatus("Audit stream is up-to-date.");
        }
    } catch (e) {
        editor.debug(`AuditMode: Error updating audit stream: ${e}`);
        editor.setStatus(`Error refreshing audit stream: ${e}`);
    } finally {
        isUpdating = false;
        editor.debug("AuditMode: Update cycle finished.");
    }
}

// --- Actions ---

globalThis.audit_stage_hunk = () => {
    // implementation...
};
globalThis.audit_discard_hunk = () => {
    // implementation...
};
globalThis.audit_undo_action = () => {
    // implementation...
};
// ... (Side-by-side and conflict logic remains the same)

// --- Initialization ---

globalThis.start_audit_mode = async () => {
    editor.setStatus("Generating Audit Stream...");
    editor.setContext("audit-mode", true);

    await refreshAuditStream();

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
    editor.debug("AuditMode: Registered session hooks.");
};

globalThis.stop_audit_mode = () => {
    state.reviewBufferId = null;
    editor.setContext("audit-mode", false);
    editor.off("buffer_activated", "on_audit_buffer_activated");
    editor.off("buffer_closed", "on_audit_buffer_closed");
    editor.setStatus("Audit Mode stopped.");
    editor.debug("AuditMode: Stopped and cleaned up hooks.");
};

globalThis.on_audit_buffer_activated = (data: any) => {
    if (data.buffer_id === state.reviewBufferId) {
        editor.debug("AuditMode: Review Stream focused, refreshing.");
        refreshAuditStream();
    }
};

globalThis.on_audit_buffer_closed = (data: any) => {
    if (data.buffer_id === state.reviewBufferId) {
        stop_audit_mode();
    }
};

globalThis.audit_refresh = () => {
    refreshAuditStream();
};

// Register Modes and Commands
editor.registerCommand("Start Audit Mode", "Start code review session", "start_audit_mode", "global");
editor.registerCommand("Stop Audit Mode", "Stop the audit session", "stop_audit_mode", "audit-mode");
editor.registerCommand("Refresh Audit Stream", "Manually refresh the list of changes", "audit_refresh", "audit-mode");

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