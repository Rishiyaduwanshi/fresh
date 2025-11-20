/// <reference path="../types/fresh.d.ts" />

import { VirtualBufferFactory } from "./lib/index.ts";

const MANUAL_MODE = "help-manual";
const SHORTCUTS_MODE = "help-keyboard";

// ANSI color codes for styling
const COLORS = {
  RESET: "\x1b[0m",
  BOLD: "\x1b[1m",
  DIM: "\x1b[2m",
  UNDERLINE: "\x1b[4m",

  // Foreground colors
  CYAN: "\x1b[36m",
  GREEN: "\x1b[32m",
  YELLOW: "\x1b[33m",
  BLUE: "\x1b[34m",
  MAGENTA: "\x1b[35m",
  WHITE: "\x1b[37m",
  BRIGHT_CYAN: "\x1b[96m",
  BRIGHT_GREEN: "\x1b[92m",
  BRIGHT_YELLOW: "\x1b[93m",
  BRIGHT_BLUE: "\x1b[94m",
  BRIGHT_MAGENTA: "\x1b[95m",
};

const createEntriesFromLines = (lines: string[]): TextPropertyEntry[] =>
  lines.map((line) => ({
    text: line.endsWith("\n") ? line : `${line}\n`,
    properties: {},
  }));

const buildManualEntries = (): TextPropertyEntry[] => {
  const C = COLORS;
  const manualText = [
    `${C.BOLD}${C.BRIGHT_CYAN}╔════════════════════════════════════════════════════════════╗${C.RESET}`,
    `${C.BOLD}${C.BRIGHT_CYAN}║${C.RESET}         ${C.BOLD}${C.BRIGHT_YELLOW}Welcome to Fresh - A Modern Code Editor${C.RESET}            ${C.BOLD}${C.BRIGHT_CYAN}║${C.RESET}`,
    `${C.BOLD}${C.BRIGHT_CYAN}╚════════════════════════════════════════════════════════════╝${C.RESET}`,
    "",
    `${C.BOLD}${C.BRIGHT_GREEN}⭐Getting Started${C.RESET}`,
    `${C.DIM}────────────────────────────────────────────────────────────${C.RESET}`,
    `  ${C.CYAN}•${C.RESET} ${C.BOLD}Open Files:${C.RESET} Press ${C.YELLOW}Ctrl+O${C.RESET} to browse and open any file`,
    `  ${C.CYAN}•${C.RESET} ${C.BOLD}Quick Actions:${C.RESET} Hit ${C.YELLOW}Ctrl+P${C.RESET} for the command palette - your Swiss Army knife!`,
    `  ${C.CYAN}•${C.RESET} ${C.BOLD}🖱 Use the Mouse!${C.RESET} Try the scroll bars, menus, and tabs.`,
    `  ${C.CYAN}•${C.RESET} ${C.BOLD}Use the Menu!${C.RESET} ${C.YELLOW}Alt+F${C.RESET} to open File menu, navigate with mouse or arrow keys.`,
    "",
    `${C.BOLD}${C.BRIGHT_MAGENTA}✨Navigation & Editing${C.RESET}`,
    `${C.DIM}────────────────────────────────────────────────────────────${C.RESET}`,
    `  ${C.MAGENTA}•${C.RESET} ${C.BOLD}Switch Tabs:${C.RESET} ${C.YELLOW}Ctrl+PageUp${C.RESET}/${C.YELLOW}Ctrl+PageDown${C.RESET} to navigate between open files`,
    `  ${C.MAGENTA}•${C.RESET} ${C.BOLD}Split Views:${C.RESET} Work on multiple files side-by-side (see View menu)`,
    `  ${C.MAGENTA}•${C.RESET} ${C.BOLD}File Explorer:${C.RESET} ${C.YELLOW}Ctrl+B${C.RESET} toggles the sidebar - your project at a glance`,
    `  ${C.MAGENTA}•${C.RESET} ${C.BOLD}Go to Line:${C.RESET} ${C.YELLOW}Ctrl+G${C.RESET} jumps you anywhere instantly`,
    "",
    `${C.BOLD}${C.BRIGHT_BLUE}⚙ Make It Yours${C.RESET}`,
    `${C.DIM}────────────────────────────────────────────────────────────${C.RESET}`,
    `  ${C.BLUE}•${C.RESET} ${C.BOLD}Keybindings:${C.RESET} Edit ${C.GREEN}config.json${C.RESET} to customize every shortcut`,
    `  ${C.BLUE}•${C.RESET} ${C.BOLD}Plugins:${C.RESET} Check out ${C.GREEN}plugins/${C.RESET} for TypeScript examples`,
    `  ${C.BLUE}•${C.RESET} ${C.BOLD}Extend It:${C.RESET} Read ${C.GREEN}docs/PLUGIN_DEVELOPMENT.md${C.RESET} to build your own features`,
    "",
    `${C.BOLD}${C.BRIGHT_YELLOW}⭐Pro Tips${C.RESET}`,
    `${C.DIM}────────────────────────────────────────────────────────────${C.RESET}`,
    `  ${C.YELLOW}•${C.RESET} ${C.BOLD}Multi-cursor:${C.RESET} ${C.YELLOW}Ctrl+D${C.RESET} selects next match - edit multiple places at once!`,
    `  ${C.YELLOW}•${C.RESET} ${C.BOLD}Search:${C.RESET} ${C.YELLOW}Ctrl+F${C.RESET} finds text, ${C.YELLOW}F3${C.RESET}/${C.YELLOW}Shift+F3${C.RESET} navigate matches`,
    `  ${C.YELLOW}•${C.RESET} ${C.BOLD}Undo/Redo:${C.RESET} ${C.YELLOW}Ctrl+Z${C.RESET}/${C.YELLOW}Ctrl+Y${C.RESET} - we've got your back!`,
    "",
    `${C.BOLD}${C.WHITE}📚 Learn More${C.RESET}`,
    `${C.DIM}────────────────────────────────────────────────────────────${C.RESET}`,
    `  • ${C.GREEN}README.md${C.RESET} - Quick start guide`,
    `  • ${C.GREEN}docs/USER_GUIDE.md${C.RESET} - Comprehensive documentation`,
    `  • ${C.GREEN}docs/ARCHITECTURE.md${C.RESET} - How Fresh works under the hood`,
    "",
    `${C.DIM}Press ${C.YELLOW}q${C.RESET}${C.DIM} or ${C.YELLOW}Esc${C.RESET}${C.DIM} to close this help | ${C.YELLOW}Shift+F1${C.RESET}${C.DIM} for keyboard shortcuts${C.RESET}`,
  ];
  return createEntriesFromLines(manualText);
};

const buildShortcutEntries = (bindings: { key: string; action: string }[]): TextPropertyEntry[] => {
  const C = COLORS;
  const header = [
    `${C.BOLD}${C.BRIGHT_CYAN}╔════════════════════════════════════════════════════════════╗${C.RESET}`,
    `${C.BOLD}${C.BRIGHT_CYAN}║${C.RESET}                 ${C.BOLD}${C.BRIGHT_YELLOW}⌨️  Keyboard Shortcuts${C.RESET}                      ${C.BOLD}${C.BRIGHT_CYAN}║${C.RESET}`,
    `${C.BOLD}${C.BRIGHT_CYAN}╚════════════════════════════════════════════════════════════╝${C.RESET}`,
    "",
  ];

  const lines: string[] = bindings.map((binding) => {
    const keyLabel = binding.key.padEnd(22);
    return `  ${C.CYAN}${keyLabel}${C.RESET} ${C.DIM}→${C.RESET} ${binding.action}`;
  });

  const paddedLines = lines.length ? lines : [`  ${C.DIM}(No bindings available)${C.RESET}`];

  const footer = [
    "",
    `${C.DIM}Press ${C.YELLOW}q${C.RESET}${C.DIM} or ${C.YELLOW}Esc${C.RESET}${C.DIM} to close | ${C.YELLOW}F1${C.RESET}${C.DIM} for the main help${C.RESET}`,
  ];

  return createEntriesFromLines([...header, ...paddedLines, ...footer]);
};

const openVirtualBuffer = async (
  name: string,
  entries: TextPropertyEntry[],
  mode: string,
): Promise<void> => {
  try {
    await VirtualBufferFactory.create({
      name,
      mode,
      entries,
      showLineNumbers: false,
      editingDisabled: true,
      readOnly: true,
    });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    editor.setStatus(`Failed to open ${name}: ${message}`);
  }
};

const openManual = async (): Promise<void> => {
  const entries = buildManualEntries();
  await openVirtualBuffer("*Fresh Manual*", entries, MANUAL_MODE);
};

const openShortcuts = async (bindings: { key: string; action: string }[]): Promise<void> => {
  const entries = buildShortcutEntries(bindings);
  await openVirtualBuffer("*Keyboard Shortcuts*", entries, SHORTCUTS_MODE);
};

editor.defineMode(
  MANUAL_MODE,
  null,
  [
    ["q", "manual_help_close"],
    ["Escape", "manual_help_close"],
  ],
  true,
);

editor.defineMode(
  SHORTCUTS_MODE,
  null,
  [
    ["q", "manual_help_close"],
    ["Escape", "manual_help_close"],
  ],
  true,
);

globalThis.manual_help_close = () => {
  const bufferId = editor.getActiveBufferId();
  editor.closeBuffer(bufferId);
};

globalThis.onManualPage = async (): Promise<boolean> => {
  await openManual();
  return true;
};

globalThis.onKeyboardShortcuts = async (args: {
  bindings: { key: string; action: string }[];
}): Promise<boolean> => {
  await openShortcuts(args.bindings);
  return true;
};

editor.on("manual_page", "onManualPage");
editor.on("keyboard_shortcuts", "onKeyboardShortcuts");

editor.registerCommand(
  "Show Fresh Manual",
  "Open the Fresh manual (virtual buffer)",
  "show_help",
  "normal",
);

editor.registerCommand(
  "Keyboard Shortcuts",
  "Show the keyboard shortcuts list (virtual buffer)",
  "keyboard_shortcuts",
  "normal",
);

editor.debug("Manual/help plugin initialized");
