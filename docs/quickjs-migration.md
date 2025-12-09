# QuickJS Backend Migration

## Overview

This document tracks the migration from deno_core (V8) to QuickJS for the Fresh editor's JavaScript plugin runtime.

## Goals

1. **Reduce dependencies** - From ~315 crates with deno_core/V8 to ~183 with QuickJS
2. **Simplify build** - No more V8 snapshot generation, faster compilation
3. **Lighter runtime** - QuickJS is ~700KB vs V8's multi-MB footprint
4. **Single backend** - No feature flags, just QuickJS + oxc

## Technology Stack

- **QuickJS**: Embedded JavaScript engine supporting ES2023 via `rquickjs` crate (v0.9)
- **oxc**: Fast TypeScript transpilation via `oxc_transformer` (v0.102)
- **oxc_semantic**: Scoping analysis for the transformer

## Completed Tasks

1. **[DONE] Remove deno_core dependencies from Cargo.toml**
   - Removed: `deno_core`, `deno_ast`, `deno_error`, `v8`
   - Added: `rquickjs`, `oxc_transformer`, `oxc_allocator`, `oxc_parser`, `oxc_span`, `oxc_codegen`, `oxc_semantic`

2. **[DONE] Remove deno_core backend files**
   - Deleted: `src/services/plugins/backend/deno_core_backend.rs`
   - Deleted: `src/services/plugins/runtime.rs`
   - Deleted: `src/v8_init.rs`

3. **[DONE] Simplify backend/mod.rs**
   - Removed conditional compilation feature flags
   - Only exports QuickJS backend
   - Added `backend_name()` returning "QuickJS + oxc"

4. **[DONE] Update thread.rs**
   - Changed imports from `runtime` to `backend` module
   - Uses `QuickJsBackend::new()` instead of `TypeScriptRuntime`

5. **[DONE] Update test harness**
   - Removed V8 initialization from `tests/common/harness.rs`
   - QuickJS doesn't need early initialization like V8

6. **[DONE] Implement TypeScript transpilation**
   - Uses `oxc_parser` to parse TypeScript
   - Uses `oxc_semantic::SemanticBuilder` for scoping
   - Uses `oxc_transformer::Transformer` to strip types
   - Uses `oxc_codegen::Codegen` to generate JavaScript

7. **[DONE] Implement QuickJS backend**
   - File: `src/services/plugins/backend/quickjs_backend.rs`
   - Implements the `JsBackend` trait
   - Creates `editor.*` global API for plugins
   - IIFE wrapping for plugin scope isolation

## Current Status

### Working
- Build compiles successfully
- TypeScript transpilation works for most plugins
- 18 out of 19 plugins load successfully

### Remaining Issue
**1 plugin fails to load: `clangd_support.ts`**

Error: `Unexpected token '{'` at line 10

This appears to be a syntax error in the transpiled output. The oxc transformer may be producing output that QuickJS doesn't accept, possibly related to:
- Object shorthand syntax
- Arrow function syntax
- Async/await syntax (QuickJS ES2023 should support this though)

## Editor API Implementation Status

### Fully Implemented
- `editor.setStatus(message)` - Show status message
- `editor.debug(message)` - Debug logging
- `editor.copyToClipboard(text)` - Set clipboard
- `editor.getActiveBufferId()` - Get active buffer
- `editor.getCursorPosition()` - Get cursor pos
- `editor.getBufferPath(bufferId)` - Get file path
- `editor.getBufferLength(bufferId)` - Get buffer size
- `editor.isBufferModified(bufferId)` - Check modified
- `editor.insertText(bufferId, pos, text)` - Insert text
- `editor.deleteRange(bufferId, start, end)` - Delete text
- `editor.insertAtCursor(text)` - Insert at cursor
- `editor.registerCommand(name, desc, action, ctx)` - Register command
- `editor.setContext(name, active)` - Set custom context
- `editor.openFile(path)` - Open file
- `editor.getActiveSplitId()` - Get active split
- `editor.on(event, handler)` - Register event handler
- `editor.off(event, handler)` - Remove event handler
- `editor.getEnv(name)` - Get environment variable
- `editor.getCwd()` - Get working directory
- `editor.pathDirname(path)` - Get directory name
- `editor.pathBasename(path)` - Get file name
- `editor.pathExtname(path)` - Get extension
- `editor.pathIsAbsolute(path)` - Check absolute
- `editor.pathJoin(...parts)` - Join paths
- `editor.fileExists(path)` - Check file exists
- `editor.readFile(path)` - Read file contents
- `editor.writeFile(path, content)` - Write file
- `editor.showBuffer(bufferId)` - Show buffer
- `editor.closeBuffer(bufferId)` - Close buffer
- `editor.setBufferCursor(bufferId, pos)` - Set cursor

### Stub Implementations (log warning but don't crash)
- `editor.defineMode(name, parent, bindings)` - Modal keybindings
- `editor.addOverlay(...)` - Syntax highlighting overlays
- `editor.clearNamespace(bufferId, ns)` - Clear overlays
- `editor.spawnProcess(cmd, args)` - Run external process
- `editor.setPromptSuggestions(...)` - Autocomplete suggestions
- `editor.startPrompt(prefix, mode)` - Start input prompt
- `editor.refreshLines(...)` - Force line refresh
- `editor.getTextPropertiesAtCursor()` - LSP info at cursor
- `editor.getBufferInfo(bufferId)` - Buffer metadata
- `editor.createVirtualBufferInSplit(...)` - Create virtual buffer
- `editor.setVirtualBufferContent(...)` - Update virtual buffer
- `editor.closeSplit(splitId)` - Close split
- `editor.setSplitBuffer(...)` - Set split buffer
- `editor.clearLineIndicators(...)` - Clear gutter indicators
- `editor.setLineIndicator(...)` - Set gutter indicator
- `editor.getBufferSavedDiff(...)` - Get diff from saved

## Next Steps

### Immediate (to fix the last failing plugin)

1. **Debug clangd_support.ts transpilation**
   - Write the transpiled output to a temp file
   - Manually inspect what syntax QuickJS doesn't accept
   - May need to adjust oxc transformer options

2. **Alternative: Skip problematic plugins**
   - Could add a list of plugins to skip loading
   - Allow editor to start even with plugin failures

### Short-term (complete the migration)

3. **Implement critical stub methods**
   - `spawnProcess` - Important for git plugins, grep, etc.
   - `addOverlay`/`clearNamespace` - For syntax highlighting
   - `defineMode` - For modal keybindings

4. **Add error handling improvements**
   - Better error messages for plugin failures
   - Option to continue loading editor when plugins fail

### Medium-term (full functionality)

5. **Implement remaining stub methods**
   - Virtual buffer support
   - Split management
   - Line indicators

6. **Performance testing**
   - Compare plugin execution speed vs deno_core
   - Measure memory usage

## File Structure

```
src/services/plugins/
├── backend/
│   ├── mod.rs              # Exports QuickJsBackend as SelectedBackend
│   └── quickjs_backend.rs  # QuickJS implementation
├── api.rs                  # EditorStateSnapshot, PluginCommand, etc.
├── thread.rs               # Plugin thread runner
├── hooks.rs                # Hook definitions
├── event_hooks.rs          # Event hook system
└── process.rs              # Process spawning (not yet integrated)
```

## Dependencies Added

```toml
# QuickJS JavaScript runtime with oxc for TypeScript transpilation
rquickjs = { version = "0.9", features = ["bindgen", "futures", "macro"] }
oxc_transformer = "0.102"
oxc_allocator = "0.102"
oxc_parser = "0.102"
oxc_span = "0.102"
oxc_codegen = "0.102"
oxc_semantic = "0.102"
```

## Known Limitations

1. **No ES module imports** - Plugins with `import ... from` are skipped (e.g., clangd_support.ts)
   - Workaround: Inline dependencies or use global state
   - Future: Implement module bundling at transpilation time
2. **No async/await in plugin API** - QuickJS supports async, but our API is synchronous
3. **Limited TypeScript features** - Only type stripping, no enum transforms etc.
4. **IIFE scope isolation** - Uses IIFE wrapping instead of true ES module system
5. **Stub implementations** - Many APIs just log warnings (see list above)

## References

- [rquickjs crate](https://docs.rs/rquickjs/)
- [QuickJS engine](https://bellard.org/quickjs/)
- [oxc project](https://oxc-project.github.io/)
