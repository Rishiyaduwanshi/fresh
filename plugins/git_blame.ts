/// <reference path="../types/fresh.d.ts" />

/**
 * Git Blame Plugin - Magit-style Git Blame Interface
 *
 * Provides an interactive git blame view using the Annotated Views architecture:
 * - Virtual buffer contains pure file content (for syntax highlighting)
 * - View transform injects styled header lines above each blame block
 * - Headers have dark gray background and no line numbers
 * - Content lines preserve source line numbers and syntax highlighting
 *
 * Features:
 * - 'b' to go back in history (show blame at parent commit)
 * - 'q' to close the blame view
 * - 'y' to yank (copy) the commit hash at cursor
 *
 * Inspired by magit's git-blame-additions feature.
 */

// =============================================================================
// Types and Interfaces
// =============================================================================

interface BlameLine {
  hash: string;
  shortHash: string;
  author: string;
  authorTime: string;      // Unix timestamp
  relativeDate: string;
  summary: string;
  lineNumber: number;      // Original line number
  finalLineNumber: number; // Final line number in the file
  content: string;
}

interface BlameBlock {
  hash: string;
  shortHash: string;
  author: string;
  relativeDate: string;
  summary: string;
  lines: BlameLine[];
  startLine: number;       // First line number in block (1-indexed)
  endLine: number;         // Last line number in block (1-indexed)
  startByte: number;       // Start byte offset in the buffer
  endByte: number;         // End byte offset in the buffer
}

interface BlameState {
  isOpen: boolean;
  bufferId: number | null;
  splitId: number | null;
  sourceBufferId: number | null;  // The buffer that was open before blame
  sourceFilePath: string | null;  // Path to the file being blamed
  currentCommit: string | null;   // Current commit being viewed (null = HEAD)
  commitStack: string[];          // Stack of commits for navigation
  blocks: BlameBlock[];           // Blame blocks with byte offsets
  fileContent: string;            // Pure file content (for virtual buffer)
  lineByteOffsets: number[];      // Byte offset of each line start
}

// =============================================================================
// State Management
// =============================================================================

const blameState: BlameState = {
  isOpen: false,
  bufferId: null,
  splitId: null,
  sourceBufferId: null,
  sourceFilePath: null,
  currentCommit: null,
  commitStack: [],
  blocks: [],
  fileContent: "",
  lineByteOffsets: [],
};

// =============================================================================
// Color Definitions for Header Styling
// =============================================================================

const colors = {
  headerFg: [220, 220, 220] as [number, number, number],   // Light gray text
  headerBg: [50, 50, 55] as [number, number, number],       // Dark gray background
};

// =============================================================================
// Mode Definition
// =============================================================================

editor.defineMode(
  "git-blame",
  "normal", // inherit from normal mode for cursor movement
  [
    ["b", "git_blame_go_back"],
    ["q", "git_blame_close"],
    ["Escape", "git_blame_close"],
    ["y", "git_blame_copy_hash"],
  ],
  true // read-only
);

// =============================================================================
// Git Blame Parsing
// =============================================================================

/**
 * Parse git blame --porcelain output
 */
async function fetchGitBlame(filePath: string, commit: string | null): Promise<BlameLine[]> {
  const args = ["blame", "--porcelain"];

  if (commit) {
    args.push(commit);
  }

  args.push("--", filePath);

  const result = await editor.spawnProcess("git", args);

  if (result.exit_code !== 0) {
    editor.setStatus(`Git blame error: ${result.stderr}`);
    return [];
  }

  const lines: BlameLine[] = [];
  const output = result.stdout;
  const outputLines = output.split("\n");

  let currentHash = "";
  let currentAuthor = "";
  let currentAuthorTime = "";
  let currentSummary = "";
  let currentOrigLine = 0;
  let currentFinalLine = 0;

  // Cache for commit info to avoid redundant parsing
  const commitInfo: Map<string, { author: string; authorTime: string; summary: string }> = new Map();

  for (let i = 0; i < outputLines.length; i++) {
    const line = outputLines[i];

    // Check for commit line: <hash> <orig-line> <final-line> [num-lines]
    const commitMatch = line.match(/^([a-f0-9]{40}) (\d+) (\d+)/);
    if (commitMatch) {
      currentHash = commitMatch[1];
      currentOrigLine = parseInt(commitMatch[2], 10);
      currentFinalLine = parseInt(commitMatch[3], 10);

      // Check cache for this commit's info
      const cached = commitInfo.get(currentHash);
      if (cached) {
        currentAuthor = cached.author;
        currentAuthorTime = cached.authorTime;
        currentSummary = cached.summary;
      }
      continue;
    }

    // Parse header fields
    if (line.startsWith("author ")) {
      currentAuthor = line.slice(7);
      continue;
    }
    if (line.startsWith("author-time ")) {
      currentAuthorTime = line.slice(12);
      continue;
    }
    if (line.startsWith("summary ")) {
      currentSummary = line.slice(8);
      // Cache this commit's info
      commitInfo.set(currentHash, {
        author: currentAuthor,
        authorTime: currentAuthorTime,
        summary: currentSummary,
      });
      continue;
    }

    // Content line (starts with tab)
    if (line.startsWith("\t")) {
      const content = line.slice(1);

      // Calculate relative date from author-time
      const relativeDate = formatRelativeDate(parseInt(currentAuthorTime, 10));

      lines.push({
        hash: currentHash,
        shortHash: currentHash.slice(0, 7),
        author: currentAuthor,
        authorTime: currentAuthorTime,
        relativeDate: relativeDate,
        summary: currentSummary,
        lineNumber: currentOrigLine,
        finalLineNumber: currentFinalLine,
        content: content,
      });
    }
  }

  return lines;
}

/**
 * Format a unix timestamp as a relative date string
 */
function formatRelativeDate(timestamp: number): string {
  const now = Math.floor(Date.now() / 1000);
  const diff = now - timestamp;

  if (diff < 60) {
    return "just now";
  } else if (diff < 3600) {
    const mins = Math.floor(diff / 60);
    return `${mins} minute${mins > 1 ? "s" : ""} ago`;
  } else if (diff < 86400) {
    const hours = Math.floor(diff / 3600);
    return `${hours} hour${hours > 1 ? "s" : ""} ago`;
  } else if (diff < 604800) {
    const days = Math.floor(diff / 86400);
    return `${days} day${days > 1 ? "s" : ""} ago`;
  } else if (diff < 2592000) {
    const weeks = Math.floor(diff / 604800);
    return `${weeks} week${weeks > 1 ? "s" : ""} ago`;
  } else if (diff < 31536000) {
    const months = Math.floor(diff / 2592000);
    return `${months} month${months > 1 ? "s" : ""} ago`;
  } else {
    const years = Math.floor(diff / 31536000);
    return `${years} year${years > 1 ? "s" : ""} ago`;
  }
}

/**
 * Fetch file content at a specific commit (or HEAD)
 */
async function fetchFileContent(filePath: string, commit: string | null): Promise<string> {
  if (commit) {
    // Get historical file content
    const result = await editor.spawnProcess("git", ["show", `${commit}:${filePath}`]);
    if (result.exit_code === 0) {
      return result.stdout;
    }
  }

  // Get current file content
  const result = await editor.spawnProcess("cat", [filePath]);
  return result.exit_code === 0 ? result.stdout : "";
}

/**
 * Build line byte offset lookup table
 */
function buildLineByteOffsets(content: string): number[] {
  const offsets: number[] = [0]; // Line 1 starts at byte 0
  let byteOffset = 0;

  for (const char of content) {
    byteOffset += char.length; // In JS strings, each char is at least 1
    if (char === '\n') {
      offsets.push(byteOffset);
    }
  }

  return offsets;
}

/**
 * Get byte offset for a given line number (1-indexed)
 */
function getLineByteOffset(lineNum: number): number {
  if (lineNum <= 0) return 0;
  const idx = lineNum - 1;
  if (idx < blameState.lineByteOffsets.length) {
    return blameState.lineByteOffsets[idx];
  }
  // Return end of file if line number is out of range
  return blameState.fileContent.length;
}

/**
 * Group blame lines into blocks by commit, with byte offset information
 */
function groupIntoBlocks(lines: BlameLine[]): BlameBlock[] {
  const blocks: BlameBlock[] = [];
  let currentBlock: BlameBlock | null = null;

  for (const line of lines) {
    // Check if we need to start a new block
    if (!currentBlock || currentBlock.hash !== line.hash) {
      // Save previous block
      if (currentBlock && currentBlock.lines.length > 0) {
        currentBlock.endByte = getLineByteOffset(currentBlock.endLine + 1);
        blocks.push(currentBlock);
      }

      // Start new block
      currentBlock = {
        hash: line.hash,
        shortHash: line.shortHash,
        author: line.author,
        relativeDate: line.relativeDate,
        summary: line.summary,
        lines: [],
        startLine: line.finalLineNumber,
        endLine: line.finalLineNumber,
        startByte: getLineByteOffset(line.finalLineNumber),
        endByte: 0, // Will be set when block is complete
      };
    }

    currentBlock.lines.push(line);
    currentBlock.endLine = line.finalLineNumber;
  }

  // Don't forget the last block
  if (currentBlock && currentBlock.lines.length > 0) {
    currentBlock.endByte = getLineByteOffset(currentBlock.endLine + 1);
    blocks.push(currentBlock);
  }

  return blocks;
}

// =============================================================================
// View Transform Hook
// =============================================================================

/**
 * Format a header line for a blame block
 */
function formatBlockHeader(block: BlameBlock): string {
  // Truncate summary if too long
  const maxSummaryLen = 50;
  const summary = block.summary.length > maxSummaryLen
    ? block.summary.slice(0, maxSummaryLen - 3) + "..."
    : block.summary;

  return `── ${block.shortHash} (${block.author}, ${block.relativeDate}) "${summary}" ──`;
}

/**
 * Find which block (if any) starts at or before the given byte offset
 */
function findBlockForByteOffset(byteOffset: number): BlameBlock | null {
  for (const block of blameState.blocks) {
    if (byteOffset >= block.startByte && byteOffset < block.endByte) {
      return block;
    }
  }
  return null;
}

/**
 * View transform hook - injects styled header lines above blame blocks
 */
globalThis.onViewTransformRequest = function(args: {
  buffer_id: number;
  split_id: number;
  viewport_start: number;
  viewport_end: number;
  tokens: ViewTokenWire[];
}): void {
  editor.debug(`view_transform_request: buffer_id=${args.buffer_id}, blameState.bufferId=${blameState.bufferId}, isOpen=${blameState.isOpen}`);

  // Only transform our blame buffer
  if (args.buffer_id !== blameState.bufferId || !blameState.isOpen) {
    editor.debug(`view_transform_request: skipping (buffer mismatch or not open)`);
    return;
  }

  editor.debug(`view_transform_request: processing viewport ${args.viewport_start}-${args.viewport_end}, ${blameState.blocks.length} blocks`);

  const transformed: ViewTokenWire[] = [];
  const processedBlocks = new Set<string>();

  // Track which blocks we need headers for based on viewport
  const blocksInViewport: BlameBlock[] = [];
  for (const block of blameState.blocks) {
    // Check if block overlaps with viewport
    if (block.endByte > args.viewport_start && block.startByte < args.viewport_end) {
      blocksInViewport.push(block);
      editor.debug(`Block in viewport: hash=${block.shortHash}, startByte=${block.startByte}, endByte=${block.endByte}`);
    }
  }
  editor.debug(`Total blocks in viewport: ${blocksInViewport.length}, tokens: ${args.tokens.length}`);

  // Process tokens and inject headers
  // We need to inject a header when:
  // 1. We encounter a token at the exact start of a block (byteOffset === block.startByte)
  // 2. OR the block starts before the viewport but extends into it (first visible token of that block)

  let lastByteOffset: number | null = null;

  // Debug: log first few tokens
  for (let i = 0; i < Math.min(5, args.tokens.length); i++) {
    const t = args.tokens[i];
    editor.debug(`Token[${i}]: source_offset=${t.source_offset}, kind=${JSON.stringify(t.kind)}`);
  }

  for (const token of args.tokens) {
    const byteOffset = token.source_offset;

    // Check if we need to inject headers before this token
    if (byteOffset !== null && byteOffset !== lastByteOffset) {
      for (const block of blocksInViewport) {
        const blockKey = block.hash + block.startByte;

        // Inject header if:
        // 1. This token is at the exact start of the block, OR
        // 2. This token is the first visible token within this block
        //    (block started before viewport, so header wasn't shown yet)
        const isBlockStart = byteOffset === block.startByte;
        const isFirstVisibleInBlock = byteOffset >= block.startByte &&
                                       byteOffset < block.endByte &&
                                       block.startByte < args.viewport_start;

        if ((isBlockStart || isFirstVisibleInBlock) && !processedBlocks.has(blockKey)) {
          const headerText = formatBlockHeader(block);
          editor.debug(`INJECT: block=${block.shortHash}, byteOffset=${byteOffset}, headerText="${headerText.substring(0, 40)}..."`);
          processedBlocks.add(blockKey);

          // Add header token (source_offset: null = no line number)
          transformed.push({
            source_offset: null,
            kind: { Text: headerText },
            style: {
              fg: colors.headerFg,
              bg: colors.headerBg,
              bold: true,
              italic: false,
            },
          });

          // Add newline after header (also with no source mapping)
          transformed.push({
            source_offset: null,
            kind: "Newline",
            style: {
              fg: colors.headerFg,
              bg: colors.headerBg,
              bold: false,
              italic: false,
            },
          });
        }
      }
    }

    // Pass through the original token (preserves source mapping for line numbers + syntax highlighting)
    transformed.push(token);
    lastByteOffset = byteOffset;
  }

  // Submit the transformed view
  editor.debug(`Submitting view transform: original=${args.tokens.length} tokens, transformed=${transformed.length} tokens`);
  // Log first 5 transformed tokens
  for (let i = 0; i < Math.min(5, transformed.length); i++) {
    const t = transformed[i];
    const kindStr = typeof t.kind === 'string' ? t.kind : `Text:"${(t.kind as {Text:string}).Text.substring(0, 30)}"`;
    editor.debug(`OUT[${i}]: source_offset=${t.source_offset}, kind=${kindStr}`);
  }
  const result = editor.submitViewTransform(
    args.buffer_id,
    args.split_id,
    args.viewport_start,
    args.viewport_end,
    transformed,
    null // no layout hints
  );
  editor.debug(`submitViewTransform result: ${result}`);
};

// Register for the view transform hook
editor.on("view_transform_request", "onViewTransformRequest");

// =============================================================================
// Public Commands
// =============================================================================

/**
 * Show git blame for the current file
 */
globalThis.show_git_blame = async function(): Promise<void> {
  if (blameState.isOpen) {
    editor.setStatus("Git blame already open");
    return;
  }

  // Get current file path
  const activeBufferId = editor.getActiveBufferId();
  const filePath = editor.getBufferPath(activeBufferId);
  if (!filePath || filePath === "") {
    editor.setStatus("No file open to blame");
    return;
  }

  editor.setStatus("Loading git blame...");

  // Store state before opening blame
  blameState.splitId = editor.getActiveSplitId();
  blameState.sourceBufferId = activeBufferId;
  blameState.sourceFilePath = filePath;
  blameState.currentCommit = null;
  blameState.commitStack = [];

  // Fetch file content and blame data in parallel
  const [fileContent, blameLines] = await Promise.all([
    fetchFileContent(filePath, null),
    fetchGitBlame(filePath, null),
  ]);

  if (blameLines.length === 0) {
    editor.setStatus("No blame information available (not a git file or error)");
    resetState();
    return;
  }

  // Store file content and build line offset table
  blameState.fileContent = fileContent;
  blameState.lineByteOffsets = buildLineByteOffsets(fileContent);

  // Group into blocks with byte offsets
  blameState.blocks = groupIntoBlocks(blameLines);

  // Get file extension for language detection
  const ext = filePath.includes('.') ? filePath.split('.').pop() : '';
  const bufferName = `*blame:${editor.pathBasename(filePath)}*`;

  // Create virtual buffer with PURE file content (for syntax highlighting)
  // The view transform hook will inject headers during rendering
  const entries: TextPropertyEntry[] = [];

  // We need to track which line belongs to which block for text properties
  let lineNum = 1;
  const contentLines = fileContent.split('\n');
  let byteOffset = 0;

  for (const line of contentLines) {
    // Find the block for this line
    const block = findBlockForByteOffset(byteOffset);

    entries.push({
      text: line + (lineNum < contentLines.length || fileContent.endsWith('\n') ? '\n' : ''),
      properties: {
        type: "content",
        hash: block?.hash ?? null,
        shortHash: block?.shortHash ?? null,
        lineNumber: lineNum,
      },
    });

    byteOffset += line.length + 1; // +1 for newline
    lineNum++;
  }

  // Create virtual buffer with the file content
  const bufferId = await editor.createVirtualBufferInExistingSplit({
    name: bufferName,
    mode: "git-blame",
    read_only: true,
    entries: entries,
    split_id: blameState.splitId!,
    show_line_numbers: true,  // We DO want line numbers (headers won't have them due to source_offset: null)
    show_cursors: true,
    editing_disabled: true,
  });

  if (bufferId !== null) {
    blameState.isOpen = true;
    blameState.bufferId = bufferId;

    editor.setStatus(`Git blame: ${blameState.blocks.length} blocks | b: blame at parent | q: close`);
    editor.debug("Git blame panel opened with view transform architecture");
  } else {
    resetState();
    editor.setStatus("Failed to open git blame panel");
  }
};

/**
 * Reset blame state
 */
function resetState(): void {
  blameState.splitId = null;
  blameState.sourceBufferId = null;
  blameState.sourceFilePath = null;
  blameState.currentCommit = null;
  blameState.commitStack = [];
  blameState.blocks = [];
  blameState.fileContent = "";
  blameState.lineByteOffsets = [];
}

/**
 * Close the git blame view
 */
globalThis.git_blame_close = function(): void {
  if (!blameState.isOpen) {
    return;
  }

  // Restore the original buffer in the split
  if (blameState.splitId !== null && blameState.sourceBufferId !== null) {
    editor.setSplitBuffer(blameState.splitId, blameState.sourceBufferId);
  }

  // Close the blame buffer
  if (blameState.bufferId !== null) {
    editor.closeBuffer(blameState.bufferId);
  }

  blameState.isOpen = false;
  blameState.bufferId = null;
  resetState();

  editor.setStatus("Git blame closed");
};

/**
 * Get the commit hash at the current cursor position
 */
function getCommitAtCursor(): string | null {
  if (blameState.bufferId === null) return null;

  const props = editor.getTextPropertiesAtCursor(blameState.bufferId);

  if (props.length > 0) {
    const hash = props[0].hash as string | undefined;
    if (hash) {
      return hash;
    }
  }

  return null;
}

/**
 * Navigate to blame at the parent commit of the current line's commit
 */
globalThis.git_blame_go_back = async function(): Promise<void> {
  if (!blameState.isOpen || !blameState.sourceFilePath) {
    return;
  }

  const currentHash = getCommitAtCursor();
  if (!currentHash) {
    editor.setStatus("Move cursor to a blame line first");
    return;
  }

  // Skip if this is the "not committed yet" hash (all zeros)
  if (currentHash === "0000000000000000000000000000000000000000") {
    editor.setStatus("This line is not yet committed");
    return;
  }

  editor.setStatus(`Loading blame at ${currentHash.slice(0, 7)}^...`);

  // Get the parent commit
  const parentCommit = `${currentHash}^`;

  // Push current state to stack for potential future navigation
  if (blameState.currentCommit) {
    blameState.commitStack.push(blameState.currentCommit);
  } else {
    blameState.commitStack.push("HEAD");
  }

  // Fetch file content and blame at parent commit
  const [fileContent, blameLines] = await Promise.all([
    fetchFileContent(blameState.sourceFilePath, parentCommit),
    fetchGitBlame(blameState.sourceFilePath, parentCommit),
  ]);

  if (blameLines.length === 0) {
    // Pop the stack since we couldn't navigate
    blameState.commitStack.pop();
    editor.setStatus(`Cannot get blame at ${currentHash.slice(0, 7)}^ (may be initial commit or file didn't exist)`);
    return;
  }

  // Update state
  blameState.currentCommit = parentCommit;
  blameState.fileContent = fileContent;
  blameState.lineByteOffsets = buildLineByteOffsets(fileContent);
  blameState.blocks = groupIntoBlocks(blameLines);

  // Update virtual buffer content
  if (blameState.bufferId !== null) {
    const entries: TextPropertyEntry[] = [];
    let lineNum = 1;
    const contentLines = fileContent.split('\n');
    let byteOffset = 0;

    for (const line of contentLines) {
      const block = findBlockForByteOffset(byteOffset);

      entries.push({
        text: line + (lineNum < contentLines.length || fileContent.endsWith('\n') ? '\n' : ''),
        properties: {
          type: "content",
          hash: block?.hash ?? null,
          shortHash: block?.shortHash ?? null,
          lineNumber: lineNum,
        },
      });

      byteOffset += line.length + 1;
      lineNum++;
    }

    editor.setVirtualBufferContent(blameState.bufferId, entries);
  }

  const depth = blameState.commitStack.length;
  editor.setStatus(`Git blame at ${currentHash.slice(0, 7)}^ | depth: ${depth} | b: go deeper | q: close`);
};

/**
 * Copy the commit hash at cursor to clipboard
 */
globalThis.git_blame_copy_hash = function(): void {
  if (!blameState.isOpen) return;

  const hash = getCommitAtCursor();
  if (!hash) {
    editor.setStatus("Move cursor to a blame line first");
    return;
  }

  // Skip if this is the "not committed yet" hash
  if (hash === "0000000000000000000000000000000000000000") {
    editor.setStatus("This line is not yet committed");
    return;
  }

  // Use spawn to copy to clipboard
  editor.spawnProcess("sh", ["-c", `echo -n "${hash}" | xclip -selection clipboard 2>/dev/null || echo -n "${hash}" | pbcopy 2>/dev/null || echo -n "${hash}" | xsel --clipboard 2>/dev/null`])
    .then(() => {
      editor.setStatus(`Copied: ${hash.slice(0, 7)} (${hash})`);
    })
    .catch(() => {
      editor.setStatus(`Hash: ${hash}`);
    });
};

// =============================================================================
// Command Registration
// =============================================================================

editor.registerCommand(
  "Git Blame",
  "Show git blame for current file (magit-style)",
  "show_git_blame",
  "normal"
);

editor.registerCommand(
  "Git Blame: Close",
  "Close the git blame panel",
  "git_blame_close",
  "normal"
);

editor.registerCommand(
  "Git Blame: Go Back",
  "Show blame at parent commit of current line",
  "git_blame_go_back",
  "normal"
);

// =============================================================================
// Plugin Initialization
// =============================================================================

editor.setStatus("Git Blame plugin loaded (view transform architecture)");
editor.debug("Git Blame plugin initialized - Use 'Git Blame' command to open");
