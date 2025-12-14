//! Settings UI module
//!
//! Provides a built-in settings editor that reads from JSON Schema
//! and renders appropriate controls for each setting type.
//!
//! ## Architecture
//!
//! - `schema.rs` - Parse JSON Schema into setting definitions
//! - `items.rs` - Convert schema to renderable items with control states
//! - `state.rs` - Manage settings UI state and pending changes
//! - `render.rs` - Render the settings modal
//! - `layout.rs` - Hit testing for mouse interaction

pub mod items;
pub mod layout;
pub mod render;
pub mod schema;
pub mod state;

pub use layout::{SettingsHit, SettingsLayout};
pub use render::render_settings;
pub use state::SettingsState;
