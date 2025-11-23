/// <reference path="../types/fresh.d.ts" />

/**
 * Minimal test plugin for view transform debugging.
 *
 * This plugin simply injects a header line at the very start of a buffer
 * to test if view transforms work correctly when startByte === 0.
 */

let activeBufferId: number | null = null;

// Define a simple mode for testing
editor.defineMode(
  "test-view-marker",
  "normal",
  [
    ["q", "test_view_marker_close"],
  ],
  true // read-only
);

/**
 * View transform hook - injects a header at byte 0
 */
globalThis.onTestViewMarkerTransform = function(args: {
  buffer_id: number;
  split_id: number;
  viewport_start: number;
  viewport_end: number;
  tokens: ViewTokenWire[];
}): void {
  // Only transform our test buffer
  if (args.buffer_id !== activeBufferId || activeBufferId === null) {
    return;
  }

  editor.debug(`[test_view_marker] transform request: viewport=${args.viewport_start}-${args.viewport_end}, tokens=${args.tokens.length}`);

  // Log first few tokens for debugging
  for (let i = 0; i < Math.min(3, args.tokens.length); i++) {
    const t = args.tokens[i];
    editor.debug(`[test_view_marker] IN[${i}]: offset=${t.source_offset}, kind=${JSON.stringify(t.kind)}`);
  }

  const transformed: ViewTokenWire[] = [];
  let headerInjected = false;

  // Process tokens and inject header at byte 0
  for (const token of args.tokens) {
    const byteOffset = token.source_offset;

    // Inject header before the first token at byte 0
    if (byteOffset === 0 && !headerInjected) {
      const headerText = "== HEADER AT BYTE 0 ==";
      editor.debug(`[test_view_marker] INJECTING header: "${headerText}"`);

      // Add header token (source_offset: null = no line number)
      transformed.push({
        source_offset: null,
        kind: { Text: headerText },
        style: {
          fg: [255, 255, 0],   // Yellow
          bg: [50, 50, 50],    // Dark gray
          bold: true,
          italic: false,
        },
      });

      // Add newline after header
      transformed.push({
        source_offset: null,
        kind: "Newline",
        style: {
          fg: [255, 255, 255],
          bg: null,
          bold: false,
          italic: false,
        },
      });

      headerInjected = true;
    }

    // Pass through the original token
    transformed.push(token);
  }

  // Log first few output tokens
  for (let i = 0; i < Math.min(5, transformed.length); i++) {
    const t = transformed[i];
    const kindStr = typeof t.kind === 'string' ? t.kind : `Text:"${(t.kind as {Text:string}).Text.substring(0, 30)}"`;
    editor.debug(`[test_view_marker] OUT[${i}]: offset=${t.source_offset}, kind=${kindStr}`);
  }

  // Submit the transformed view
  editor.debug(`[test_view_marker] submitting: ${args.tokens.length} -> ${transformed.length} tokens`);
  editor.submitViewTransform(
    args.buffer_id,
    args.split_id,
    args.viewport_start,
    args.viewport_end,
    transformed,
    null
  );
};

// Register for the view transform hook
editor.on("view_transform_request", "onTestViewMarkerTransform");

/**
 * Open the test view marker with simple hardcoded content
 */
globalThis.show_test_view_marker = async function(): Promise<void> {
  const splitId = editor.getActiveSplitId();

  editor.debug(`[test_view_marker] opening view marker in split ${splitId}`);

  // Create virtual buffer with simple hardcoded content
  // This is the simplest possible test case
  const entries: TextPropertyEntry[] = [
    { text: "Line 1\n", properties: { type: "content", line: 1 } },
    { text: "Line 2\n", properties: { type: "content", line: 2 } },
    { text: "Line 3\n", properties: { type: "content", line: 3 } },
  ];

  const bufferId = await editor.createVirtualBufferInExistingSplit({
    name: "*test-view-marker*",
    mode: "test-view-marker",
    read_only: true,
    entries: entries,
    split_id: splitId,
    show_line_numbers: true,
    show_cursors: true,
    editing_disabled: true,
  });

  if (bufferId !== null) {
    activeBufferId = bufferId;
    editor.debug(`[test_view_marker] buffer created with id ${bufferId}`);
    editor.setStatus("Test view marker active - press q to close");
  } else {
    editor.debug(`[test_view_marker] failed to create buffer`);
    editor.setStatus("Failed to create test view marker buffer");
  }
};

/**
 * Close the test view marker
 */
globalThis.test_view_marker_close = function(): void {
  if (activeBufferId !== null) {
    editor.closeBuffer(activeBufferId);
    activeBufferId = null;
    editor.setStatus("Test view marker closed");
  }
};

// Register command
editor.registerCommand(
  "Test View Marker",
  "Test view transform with header at byte 0",
  "show_test_view_marker",
  "normal"
);

editor.setStatus("Test View Marker plugin loaded");
