//! QuickJS Runtime Backend
//!
//! This module provides a lightweight JavaScript runtime using QuickJS
//! with oxc for TypeScript transpilation.
//!
//! QuickJS is a small, embeddable JavaScript engine that supports ES2023.
//! It has a much smaller footprint than V8 but covers the ES features needed
//! by Fresh plugins.

use crate::input::commands::{Command, CommandSource};
use crate::input::keybindings::Action;
use crate::model::event::BufferId;
use crate::services::plugins::api::{EditorStateSnapshot, PluginCommand, PluginResponse};
use crate::services::plugins::backend::{JsBackend, PendingResponses};
use anyhow::{anyhow, Result};
use rquickjs::{Context, Function, Object, Runtime, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, RwLock};

/// Transpile TypeScript to JavaScript using oxc
///
/// Note: For now, only parses and regenerates the code.
/// TypeScript type stripping will be added when oxc API stabilizes.
#[allow(dead_code)]
fn transpile_typescript(source: &str, filename: &str) -> Result<String> {
    use oxc_allocator::Allocator;
    use oxc_codegen::Codegen;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    let allocator = Allocator::default();

    // Parse as TypeScript
    let source_type = SourceType::from_path(filename).unwrap_or_else(|_| SourceType::ts());

    let parser_ret = Parser::new(&allocator, source, source_type).parse();

    if !parser_ret.errors.is_empty() {
        let errors: Vec<String> = parser_ret.errors.iter().map(|e| e.to_string()).collect();
        return Err(anyhow!("TypeScript parse errors: {}", errors.join(", ")));
    }

    // Generate JavaScript output (oxc parser handles TS syntax)
    let codegen = Codegen::new();
    let output = codegen.build(&parser_ret.program);

    Ok(output.code)
}

/// Shared state for QuickJS ops
struct QuickJsState {
    /// Editor state snapshot (read-only access)
    state_snapshot: Arc<RwLock<EditorStateSnapshot>>,
    /// Command sender for write operations
    command_sender: std::sync::mpsc::Sender<PluginCommand>,
    /// Event handlers: event_name -> list of global JS function names
    event_handlers: Rc<RefCell<HashMap<String, Vec<String>>>>,
    /// Pending response senders for async operations
    pending_responses: PendingResponses,
    /// Next request ID for async operations
    #[allow(dead_code)]
    next_request_id: Rc<RefCell<u64>>,
    /// Current plugin source (for registerCommand)
    current_plugin_source: Rc<RefCell<Option<String>>>,
}

/// QuickJS backend - lightweight JavaScript runtime
pub struct QuickJsBackend {
    #[allow(dead_code)]
    runtime: Runtime,
    context: Context,
    /// Shared event handlers registry
    event_handlers: Rc<RefCell<HashMap<String, Vec<String>>>>,
    /// Pending response senders
    pending_responses: PendingResponses,
    /// State for ops
    state: Rc<RefCell<QuickJsState>>,
}

impl QuickJsBackend {
    /// Create a new QuickJS backend
    pub fn create(
        state_snapshot: Arc<RwLock<EditorStateSnapshot>>,
        command_sender: std::sync::mpsc::Sender<PluginCommand>,
        pending_responses: PendingResponses,
    ) -> Result<Self> {
        let runtime = Runtime::new().map_err(|e| anyhow!("Failed to create QuickJS runtime: {}", e))?;
        let context = Context::full(&runtime).map_err(|e| anyhow!("Failed to create QuickJS context: {}", e))?;

        let event_handlers = Rc::new(RefCell::new(HashMap::new()));

        let state = Rc::new(RefCell::new(QuickJsState {
            state_snapshot,
            command_sender,
            event_handlers: event_handlers.clone(),
            pending_responses: Arc::clone(&pending_responses),
            next_request_id: Rc::new(RefCell::new(1)),
            current_plugin_source: Rc::new(RefCell::new(None)),
        }));

        let mut backend = Self {
            runtime,
            context,
            event_handlers,
            pending_responses,
            state,
        };

        // Initialize the editor API
        backend.init_editor_api()?;

        Ok(backend)
    }

    /// Initialize the global editor API object
    fn init_editor_api(&mut self) -> Result<()> {
        let state = self.state.clone();

        self.context.with(|ctx| {
            let globals = ctx.globals();

            // Create the editor object
            let editor = Object::new(ctx.clone()).map_err(|e| anyhow!("Failed to create editor object: {}", e))?;

            // Add all editor methods
            Self::add_editor_methods(&ctx, &editor, &state)?;

            // Set editor as global
            globals.set("editor", editor).map_err(|e| anyhow!("Failed to set editor global: {}", e))?;

            // Create console object for logging
            let console = Object::new(ctx.clone()).map_err(|e| anyhow!("Failed to create console object: {}", e))?;

            let log_fn = Function::new(ctx.clone(), |msg: String| {
                tracing::info!("QuickJS console.log: {}", msg);
            }).map_err(|e| anyhow!("Failed to create console.log: {}", e))?;

            let warn_fn = Function::new(ctx.clone(), |msg: String| {
                tracing::warn!("QuickJS console.warn: {}", msg);
            }).map_err(|e| anyhow!("Failed to create console.warn: {}", e))?;

            let error_fn = Function::new(ctx.clone(), |msg: String| {
                tracing::error!("QuickJS console.error: {}", msg);
            }).map_err(|e| anyhow!("Failed to create console.error: {}", e))?;

            console.set("log", log_fn).map_err(|e| anyhow!("Failed to set console.log: {}", e))?;
            console.set("warn", warn_fn).map_err(|e| anyhow!("Failed to set console.warn: {}", e))?;
            console.set("error", error_fn).map_err(|e| anyhow!("Failed to set console.error: {}", e))?;

            globals.set("console", console).map_err(|e| anyhow!("Failed to set console global: {}", e))?;

            Ok::<(), anyhow::Error>(())
        })?;

        Ok(())
    }

    /// Add all editor.* methods to the editor object
    fn add_editor_methods(
        ctx: &rquickjs::Ctx<'_>,
        editor: &Object<'_>,
        state: &Rc<RefCell<QuickJsState>>,
    ) -> Result<()> {
        // === Status and logging ===

        // editor.setStatus(message)
        {
            let state = state.clone();
            let set_status = Function::new(ctx.clone(), move |msg: String| {
                let state = state.borrow();
                let _ = state.command_sender.send(PluginCommand::SetStatus {
                    message: msg.clone(),
                });
                tracing::info!("QuickJS plugin set_status: {}", msg);
            })
            .map_err(|e| anyhow!("Failed to create setStatus: {}", e))?;
            editor
                .set("setStatus", set_status)
                .map_err(|e| anyhow!("Failed to set setStatus: {}", e))?;
        }

        // editor.debug(message)
        {
            let debug_fn = Function::new(ctx.clone(), |msg: String| {
                tracing::debug!("QuickJS plugin: {}", msg);
            })
            .map_err(|e| anyhow!("Failed to create debug: {}", e))?;
            editor
                .set("debug", debug_fn)
                .map_err(|e| anyhow!("Failed to set debug: {}", e))?;
        }

        // === Clipboard ===

        // editor.copyToClipboard(text)
        {
            let state = state.clone();
            let copy_fn = Function::new(ctx.clone(), move |text: String| {
                let state = state.borrow();
                let _ = state
                    .command_sender
                    .send(PluginCommand::SetClipboard { text });
            })
            .map_err(|e| anyhow!("Failed to create copyToClipboard: {}", e))?;
            editor
                .set("copyToClipboard", copy_fn)
                .map_err(|e| anyhow!("Failed to set copyToClipboard: {}", e))?;
        }

        // === Buffer queries ===
        // Clone the Arc directly for use in closures
        let snapshot_arc = state.borrow().state_snapshot.clone();

        // editor.getActiveBufferId()
        {
            let snapshot = snapshot_arc.clone();
            let get_active_buffer = Function::new(ctx.clone(), move || -> usize {
                snapshot.read().map(|s| s.active_buffer_id.0).unwrap_or(0)
            })
            .map_err(|e| anyhow!("Failed to create getActiveBufferId: {}", e))?;
            editor
                .set("getActiveBufferId", get_active_buffer)
                .map_err(|e| anyhow!("Failed to set getActiveBufferId: {}", e))?;
        }

        // editor.getCursorPosition()
        {
            let snapshot = snapshot_arc.clone();
            let get_cursor = Function::new(ctx.clone(), move || -> Option<usize> {
                snapshot
                    .read()
                    .ok()
                    .and_then(|s| s.primary_cursor.as_ref().map(|c| c.position))
            })
            .map_err(|e| anyhow!("Failed to create getCursorPosition: {}", e))?;
            editor
                .set("getCursorPosition", get_cursor)
                .map_err(|e| anyhow!("Failed to set getCursorPosition: {}", e))?;
        }

        // editor.getBufferPath(bufferId)
        {
            let snapshot = snapshot_arc.clone();
            let get_path = Function::new(ctx.clone(), move |buffer_id: usize| -> Option<String> {
                let bid = BufferId(buffer_id);
                snapshot
                    .read()
                    .ok()
                    .and_then(|s| {
                        s.buffers
                            .get(&bid)
                            .and_then(|info| info.path.as_ref())
                            .map(|p| p.to_string_lossy().to_string())
                    })
            })
            .map_err(|e| anyhow!("Failed to create getBufferPath: {}", e))?;
            editor
                .set("getBufferPath", get_path)
                .map_err(|e| anyhow!("Failed to set getBufferPath: {}", e))?;
        }

        // editor.getBufferLength(bufferId)
        {
            let snapshot = snapshot_arc.clone();
            let get_len = Function::new(ctx.clone(), move |buffer_id: usize| -> Option<usize> {
                let bid = BufferId(buffer_id);
                snapshot
                    .read()
                    .ok()
                    .and_then(|s| s.buffers.get(&bid).map(|info| info.length))
            })
            .map_err(|e| anyhow!("Failed to create getBufferLength: {}", e))?;
            editor
                .set("getBufferLength", get_len)
                .map_err(|e| anyhow!("Failed to set getBufferLength: {}", e))?;
        }

        // editor.isBufferModified(bufferId)
        {
            let snapshot = snapshot_arc.clone();
            let is_modified = Function::new(ctx.clone(), move |buffer_id: usize| -> bool {
                let bid = BufferId(buffer_id);
                snapshot
                    .read()
                    .ok()
                    .and_then(|s| s.buffers.get(&bid).map(|info| info.modified))
                    .unwrap_or(false)
            })
            .map_err(|e| anyhow!("Failed to create isBufferModified: {}", e))?;
            editor
                .set("isBufferModified", is_modified)
                .map_err(|e| anyhow!("Failed to set isBufferModified: {}", e))?;
        }

        // === Buffer mutations ===

        // editor.insertText(bufferId, position, text)
        {
            let state = state.clone();
            let insert_text = Function::new(
                ctx.clone(),
                move |buffer_id: usize, position: usize, text: String| {
                    let state = state.borrow();
                    let bid = BufferId(buffer_id);
                    let _ = state.command_sender.send(PluginCommand::InsertText {
                        buffer_id: bid,
                        position,
                        text,
                    });
                },
            )
            .map_err(|e| anyhow!("Failed to create insertText: {}", e))?;
            editor
                .set("insertText", insert_text)
                .map_err(|e| anyhow!("Failed to set insertText: {}", e))?;
        }

        // editor.deleteRange(bufferId, start, end)
        {
            let state = state.clone();
            let delete_range =
                Function::new(ctx.clone(), move |buffer_id: usize, start: usize, end: usize| {
                    let state = state.borrow();
                    let bid = BufferId(buffer_id);
                    let _ = state.command_sender.send(PluginCommand::DeleteRange {
                        buffer_id: bid,
                        range: start..end,
                    });
                })
                .map_err(|e| anyhow!("Failed to create deleteRange: {}", e))?;
            editor
                .set("deleteRange", delete_range)
                .map_err(|e| anyhow!("Failed to set deleteRange: {}", e))?;
        }

        // editor.insertAtCursor(text)
        {
            let state = state.clone();
            let insert_at_cursor = Function::new(ctx.clone(), move |text: String| {
                let state = state.borrow();
                let _ = state
                    .command_sender
                    .send(PluginCommand::InsertAtCursor { text });
            })
            .map_err(|e| anyhow!("Failed to create insertAtCursor: {}", e))?;
            editor
                .set("insertAtCursor", insert_at_cursor)
                .map_err(|e| anyhow!("Failed to set insertAtCursor: {}", e))?;
        }

        // === Command registration ===

        // editor.registerCommand(name, description, action, contexts)
        {
            let state = state.clone();
            let register_cmd = Function::new(
                ctx.clone(),
                move |name: String, description: String, action: String, contexts: String| {
                    let state = state.borrow();
                    let source = state
                        .current_plugin_source
                        .borrow()
                        .clone()
                        .unwrap_or_default();

                    // Parse custom contexts from comma-separated string
                    let custom_contexts: Vec<String> = if contexts.is_empty() {
                        Vec::new()
                    } else {
                        contexts.split(',').map(|s| s.trim().to_string()).collect()
                    };

                    let command = Command {
                        name,
                        description,
                        action: Action::PluginAction(action),
                        contexts: Vec::new(), // No built-in key contexts
                        custom_contexts,
                        source: CommandSource::Plugin(source),
                    };

                    let _ = state
                        .command_sender
                        .send(PluginCommand::RegisterCommand { command });
                },
            )
            .map_err(|e| anyhow!("Failed to create registerCommand: {}", e))?;
            editor
                .set("registerCommand", register_cmd)
                .map_err(|e| anyhow!("Failed to set registerCommand: {}", e))?;
        }

        // === Context management ===

        // editor.setContext(name, active)
        {
            let state = state.clone();
            let set_context = Function::new(ctx.clone(), move |name: String, active: bool| {
                let state = state.borrow();
                let _ = state
                    .command_sender
                    .send(PluginCommand::SetContext { name, active });
            })
            .map_err(|e| anyhow!("Failed to create setContext: {}", e))?;
            editor
                .set("setContext", set_context)
                .map_err(|e| anyhow!("Failed to set setContext: {}", e))?;
        }

        // === File operations ===

        // editor.openFile(path) - Opens file in background
        {
            let state = state.clone();
            let open_file = Function::new(ctx.clone(), move |path: String| {
                let state = state.borrow();
                let _ = state.command_sender.send(PluginCommand::OpenFileInBackground {
                    path: std::path::PathBuf::from(path),
                });
            })
            .map_err(|e| anyhow!("Failed to create openFile: {}", e))?;
            editor
                .set("openFile", open_file)
                .map_err(|e| anyhow!("Failed to set openFile: {}", e))?;
        }

        // === Split operations ===

        // editor.getActiveSplitId()
        {
            let snapshot = snapshot_arc.clone();
            let get_active_split = Function::new(ctx.clone(), move || -> usize {
                snapshot.read().map(|s| s.active_split_id).unwrap_or(0)
            })
            .map_err(|e| anyhow!("Failed to create getActiveSplitId: {}", e))?;
            editor
                .set("getActiveSplitId", get_active_split)
                .map_err(|e| anyhow!("Failed to set getActiveSplitId: {}", e))?;
        }

        // === Cursor operations ===
        // Note: CursorInfo only has position and selection, no line field

        // === Event/Hook operations ===

        // editor.on(eventName, handlerName)
        {
            let state = state.clone();
            let on_fn = Function::new(ctx.clone(), move |event_name: String, handler_name: String| {
                let state = state.borrow();
                let mut handlers = state.event_handlers.borrow_mut();
                handlers
                    .entry(event_name)
                    .or_insert_with(Vec::new)
                    .push(handler_name);
            })
            .map_err(|e| anyhow!("Failed to create on: {}", e))?;
            editor
                .set("on", on_fn)
                .map_err(|e| anyhow!("Failed to set on: {}", e))?;
        }

        // editor.off(eventName, handlerName)
        {
            let state = state.clone();
            let off_fn = Function::new(ctx.clone(), move |event_name: String, handler_name: String| {
                let state = state.borrow();
                let mut handlers = state.event_handlers.borrow_mut();
                if let Some(list) = handlers.get_mut(&event_name) {
                    list.retain(|h| h != &handler_name);
                }
            })
            .map_err(|e| anyhow!("Failed to create off: {}", e))?;
            editor
                .set("off", off_fn)
                .map_err(|e| anyhow!("Failed to set off: {}", e))?;
        }

        // === Environment operations ===

        // editor.getEnv(name)
        {
            let get_env = Function::new(ctx.clone(), |name: String| -> Option<String> {
                std::env::var(&name).ok()
            })
            .map_err(|e| anyhow!("Failed to create getEnv: {}", e))?;
            editor
                .set("getEnv", get_env)
                .map_err(|e| anyhow!("Failed to set getEnv: {}", e))?;
        }

        // editor.getCwd()
        {
            let get_cwd = Function::new(ctx.clone(), || -> Option<String> {
                std::env::current_dir()
                    .ok()
                    .map(|p| p.to_string_lossy().to_string())
            })
            .map_err(|e| anyhow!("Failed to create getCwd: {}", e))?;
            editor
                .set("getCwd", get_cwd)
                .map_err(|e| anyhow!("Failed to set getCwd: {}", e))?;
        }

        // === Path operations ===

        // editor.pathDirname(path)
        {
            let path_dirname = Function::new(ctx.clone(), |path: String| -> Option<String> {
                Path::new(&path)
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
            })
            .map_err(|e| anyhow!("Failed to create pathDirname: {}", e))?;
            editor
                .set("pathDirname", path_dirname)
                .map_err(|e| anyhow!("Failed to set pathDirname: {}", e))?;
        }

        // editor.pathBasename(path)
        {
            let path_basename = Function::new(ctx.clone(), |path: String| -> Option<String> {
                Path::new(&path)
                    .file_name()
                    .map(|p| p.to_string_lossy().to_string())
            })
            .map_err(|e| anyhow!("Failed to create pathBasename: {}", e))?;
            editor
                .set("pathBasename", path_basename)
                .map_err(|e| anyhow!("Failed to set pathBasename: {}", e))?;
        }

        // editor.pathExtname(path)
        {
            let path_extname = Function::new(ctx.clone(), |path: String| -> Option<String> {
                Path::new(&path)
                    .extension()
                    .map(|p| p.to_string_lossy().to_string())
            })
            .map_err(|e| anyhow!("Failed to create pathExtname: {}", e))?;
            editor
                .set("pathExtname", path_extname)
                .map_err(|e| anyhow!("Failed to set pathExtname: {}", e))?;
        }

        // editor.pathIsAbsolute(path)
        {
            let path_is_absolute = Function::new(ctx.clone(), |path: String| -> bool {
                Path::new(&path).is_absolute()
            })
            .map_err(|e| anyhow!("Failed to create pathIsAbsolute: {}", e))?;
            editor
                .set("pathIsAbsolute", path_is_absolute)
                .map_err(|e| anyhow!("Failed to set pathIsAbsolute: {}", e))?;
        }

        // === File system operations ===

        // editor.fileExists(path)
        {
            let file_exists = Function::new(ctx.clone(), |path: String| -> bool {
                Path::new(&path).exists()
            })
            .map_err(|e| anyhow!("Failed to create fileExists: {}", e))?;
            editor
                .set("fileExists", file_exists)
                .map_err(|e| anyhow!("Failed to set fileExists: {}", e))?;
        }

        // editor.readFile(path)
        {
            let read_file = Function::new(ctx.clone(), |path: String| -> Option<String> {
                std::fs::read_to_string(&path).ok()
            })
            .map_err(|e| anyhow!("Failed to create readFile: {}", e))?;
            editor
                .set("readFile", read_file)
                .map_err(|e| anyhow!("Failed to set readFile: {}", e))?;
        }

        // editor.writeFile(path, content)
        {
            let write_file = Function::new(ctx.clone(), |path: String, content: String| -> bool {
                std::fs::write(&path, &content).is_ok()
            })
            .map_err(|e| anyhow!("Failed to create writeFile: {}", e))?;
            editor
                .set("writeFile", write_file)
                .map_err(|e| anyhow!("Failed to set writeFile: {}", e))?;
        }

        // editor.showBuffer(bufferId)
        {
            let state = state.clone();
            let show_buffer = Function::new(ctx.clone(), move |buffer_id: usize| {
                let state = state.borrow();
                let bid = BufferId(buffer_id);
                let _ = state
                    .command_sender
                    .send(PluginCommand::ShowBuffer { buffer_id: bid });
            })
            .map_err(|e| anyhow!("Failed to create showBuffer: {}", e))?;
            editor
                .set("showBuffer", show_buffer)
                .map_err(|e| anyhow!("Failed to set showBuffer: {}", e))?;
        }

        // editor.closeBuffer(bufferId)
        {
            let state = state.clone();
            let close_buffer = Function::new(ctx.clone(), move |buffer_id: usize| {
                let state = state.borrow();
                let bid = BufferId(buffer_id);
                let _ = state
                    .command_sender
                    .send(PluginCommand::CloseBuffer { buffer_id: bid });
            })
            .map_err(|e| anyhow!("Failed to create closeBuffer: {}", e))?;
            editor
                .set("closeBuffer", close_buffer)
                .map_err(|e| anyhow!("Failed to set closeBuffer: {}", e))?;
        }

        // editor.setBufferCursor(bufferId, position)
        {
            let state = state.clone();
            let set_cursor = Function::new(ctx.clone(), move |buffer_id: usize, position: usize| {
                let state = state.borrow();
                let bid = BufferId(buffer_id);
                let _ = state.command_sender.send(PluginCommand::SetBufferCursor {
                    buffer_id: bid,
                    position,
                });
            })
            .map_err(|e| anyhow!("Failed to create setBufferCursor: {}", e))?;
            editor
                .set("setBufferCursor", set_cursor)
                .map_err(|e| anyhow!("Failed to set setBufferCursor: {}", e))?;
        }

        Ok(())
    }

    /// Execute JavaScript code
    fn execute_script(&mut self, code: &str) -> Result<()> {
        self.context.with(|ctx| {
            ctx.eval::<Value, _>(code)
                .map_err(|e| anyhow!("Script execution error: {}", e))?;
            Ok(())
        })
    }
}

impl JsBackend for QuickJsBackend {
    fn new(
        state_snapshot: Arc<RwLock<EditorStateSnapshot>>,
        command_sender: std::sync::mpsc::Sender<PluginCommand>,
        pending_responses: PendingResponses,
    ) -> Result<Self> {
        Self::create(state_snapshot, command_sender, pending_responses)
    }

    async fn load_module(&mut self, path: &str, plugin_source: &str) -> Result<()> {
        // Set the plugin source for registerCommand
        {
            let state = self.state.borrow();
            *state.current_plugin_source.borrow_mut() = if plugin_source.is_empty() {
                None
            } else {
                Some(plugin_source.to_string())
            };
        }

        // Read the source file
        let source = std::fs::read_to_string(path)
            .map_err(|e| anyhow!("Failed to read module '{}': {}", path, e))?;

        // Transpile if TypeScript
        let code = if path.ends_with(".ts") {
            transpile_typescript(&source, path)?
        } else {
            source
        };

        // Execute the code
        self.execute_script(&code)?;

        // Clear plugin source
        {
            let state = self.state.borrow();
            *state.current_plugin_source.borrow_mut() = None;
        }

        Ok(())
    }

    async fn execute_action(&mut self, action_name: &str) -> Result<()> {
        let code = format!(
            r#"
            if (typeof globalThis.{} === 'function') {{
                globalThis.{}();
            }} else {{
                throw new Error('Action "{}" is not defined as a global function');
            }}
            "#,
            action_name, action_name, action_name
        );

        self.execute_script(&code)
    }

    async fn emit(&mut self, event_name: &str, event_data: &str) -> Result<bool> {
        let handlers = self.event_handlers.borrow().get(event_name).cloned();

        if let Some(handler_names) = handlers {
            if handler_names.is_empty() {
                return Ok(true);
            }

            for handler_name in &handler_names {
                let code = format!(
                    r#"
                    if (typeof globalThis.{} === 'function') {{
                        globalThis.{}({});
                    }}
                    "#,
                    handler_name, handler_name, event_data
                );

                if let Err(e) = self.execute_script(&code) {
                    tracing::error!(
                        "Failed to call event handler '{}' for '{}': {:?}",
                        handler_name,
                        event_name,
                        e
                    );
                }
            }
        }

        Ok(true)
    }

    fn has_handlers(&self, event_name: &str) -> bool {
        self.event_handlers
            .borrow()
            .get(event_name)
            .map(|v| !v.is_empty())
            .unwrap_or(false)
    }

    fn deliver_response(&self, response: PluginResponse) {
        let request_id = match &response {
            PluginResponse::VirtualBufferCreated { request_id, .. } => *request_id,
            PluginResponse::LspRequest { request_id, .. } => *request_id,
        };

        let sender = {
            let mut pending = self.pending_responses.lock().unwrap();
            pending.remove(&request_id)
        };

        if let Some(tx) = sender {
            let _ = tx.send(response);
        } else {
            tracing::warn!("No pending response sender for request_id {}", request_id);
        }
    }

    fn send_status(&mut self, message: String) {
        let state = self.state.borrow();
        let _ = state
            .command_sender
            .send(PluginCommand::SetStatus { message });
    }

    fn pending_responses(&self) -> &PendingResponses {
        &self.pending_responses
    }
}
