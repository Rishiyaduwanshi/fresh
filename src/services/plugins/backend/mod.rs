//! JavaScript Runtime Backend
//!
//! This module provides the JavaScript runtime for TypeScript plugins using QuickJS
//! with oxc for TypeScript transpilation.
//!
//! QuickJS is a lightweight embedded JavaScript engine (~700KB) that supports ES2023.
//! oxc provides fast TypeScript transpilation.

use crate::services::plugins::api::{EditorStateSnapshot, PluginCommand, PluginResponse};
use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

pub mod quickjs_backend;

/// Information about a loaded plugin
#[derive(Debug, Clone)]
pub struct PluginInfo {
    /// Plugin name
    pub name: String,
    /// Plugin file path
    pub path: PathBuf,
    /// Whether the plugin is enabled
    pub enabled: bool,
}

/// Pending response senders type alias for convenience
pub type PendingResponses =
    Arc<std::sync::Mutex<HashMap<u64, tokio::sync::oneshot::Sender<PluginResponse>>>>;

/// JavaScript Runtime Backend Trait
///
/// This trait abstracts the JavaScript runtime interface.
///
/// Note: This trait does NOT require `Send` because JavaScript runtimes
/// are typically not thread-safe. The runtime is designed to run on a
/// dedicated plugin thread.
#[allow(async_fn_in_trait)]
pub trait JsBackend {
    /// Create a new backend instance with the given configuration
    fn new(
        state_snapshot: Arc<RwLock<EditorStateSnapshot>>,
        command_sender: std::sync::mpsc::Sender<PluginCommand>,
        pending_responses: PendingResponses,
    ) -> Result<Self>
    where
        Self: Sized;

    /// Load and execute a TypeScript/JavaScript module file
    async fn load_module(&mut self, path: &str, plugin_source: &str) -> Result<()>;

    /// Execute a global function by name (for plugin actions)
    async fn execute_action(&mut self, action_name: &str) -> Result<()>;

    /// Emit an event to all registered handlers
    ///
    /// Returns `Ok(true)` if all handlers returned true, `Ok(false)` if any returned false.
    async fn emit(&mut self, event_name: &str, event_data: &str) -> Result<bool>;

    /// Check if any handlers are registered for an event
    fn has_handlers(&self, event_name: &str) -> bool;

    /// Deliver a response to a pending async operation
    fn deliver_response(&self, response: PluginResponse);

    /// Send a status message to the editor UI
    fn send_status(&mut self, message: String);

    /// Get the pending responses handle
    fn pending_responses(&self) -> &PendingResponses;
}

// Re-export the QuickJS backend
pub use quickjs_backend::QuickJsBackend;

/// The backend type used for plugins
pub type SelectedBackend = QuickJsBackend;

/// Get the name of the JS backend
pub fn backend_name() -> &'static str {
    "QuickJS + oxc"
}

/// Check if the runtime is available (always true for embedded QuickJS)
pub fn check_runtime_available() -> Result<()> {
    Ok(())
}

/// Create a new backend instance
pub fn create_backend(
    state_snapshot: Arc<RwLock<EditorStateSnapshot>>,
    command_sender: std::sync::mpsc::Sender<PluginCommand>,
    pending_responses: PendingResponses,
) -> Result<SelectedBackend> {
    SelectedBackend::new(state_snapshot, command_sender, pending_responses)
}
