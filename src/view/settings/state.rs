//! Settings state management
//!
//! Tracks the current state of the settings UI, pending changes,
//! and provides methods for reading/writing config values.

use super::items::{control_to_value, SettingControl, SettingItem, SettingsPage};
use super::schema::{parse_schema, SettingCategory};
use crate::config::Config;
use crate::view::controls::FocusState;
use std::collections::HashMap;

/// The state of the settings UI
#[derive(Debug)]
pub struct SettingsState {
    /// Parsed schema categories
    categories: Vec<SettingCategory>,
    /// Pages built from categories
    pub pages: Vec<SettingsPage>,
    /// Currently selected category index
    pub selected_category: usize,
    /// Currently selected item index within the category
    pub selected_item: usize,
    /// Whether we're focused on the category list (left panel)
    pub category_focus: bool,
    /// Pending changes (path -> new value)
    pub pending_changes: HashMap<String, serde_json::Value>,
    /// The original config value (for detecting changes)
    original_config: serde_json::Value,
    /// Whether the settings panel is visible
    pub visible: bool,
    /// Current search query
    pub search_query: String,
    /// Whether search is active
    pub search_active: bool,
}

impl SettingsState {
    /// Create a new settings state from schema and current config
    pub fn new(schema_json: &str, config: &Config) -> Result<Self, serde_json::Error> {
        let categories = parse_schema(schema_json)?;
        let config_value = serde_json::to_value(config)?;
        let pages = super::items::build_pages(&categories, &config_value);

        Ok(Self {
            categories,
            pages,
            selected_category: 0,
            selected_item: 0,
            category_focus: true,
            pending_changes: HashMap::new(),
            original_config: config_value,
            visible: false,
            search_query: String::new(),
            search_active: false,
        })
    }

    /// Show the settings panel
    pub fn show(&mut self) {
        self.visible = true;
        self.category_focus = true;
        self.selected_category = 0;
        self.selected_item = 0;
    }

    /// Hide the settings panel
    pub fn hide(&mut self) {
        self.visible = false;
        self.search_active = false;
        self.search_query.clear();
    }

    /// Get the currently selected page
    pub fn current_page(&self) -> Option<&SettingsPage> {
        self.pages.get(self.selected_category)
    }

    /// Get the currently selected page mutably
    pub fn current_page_mut(&mut self) -> Option<&mut SettingsPage> {
        self.pages.get_mut(self.selected_category)
    }

    /// Get the currently selected item
    pub fn current_item(&self) -> Option<&SettingItem> {
        self.current_page()
            .and_then(|page| page.items.get(self.selected_item))
    }

    /// Get the currently selected item mutably
    pub fn current_item_mut(&mut self) -> Option<&mut SettingItem> {
        self.pages
            .get_mut(self.selected_category)
            .and_then(|page| page.items.get_mut(self.selected_item))
    }

    /// Move selection up
    pub fn select_prev(&mut self) {
        if self.category_focus {
            if self.selected_category > 0 {
                self.selected_category -= 1;
                self.selected_item = 0;
            }
        } else if self.selected_item > 0 {
            self.selected_item -= 1;
        }
    }

    /// Move selection down
    pub fn select_next(&mut self) {
        if self.category_focus {
            if self.selected_category + 1 < self.pages.len() {
                self.selected_category += 1;
                self.selected_item = 0;
            }
        } else if let Some(page) = self.current_page() {
            if self.selected_item + 1 < page.items.len() {
                self.selected_item += 1;
            }
        }
    }

    /// Switch focus between category list and settings
    pub fn toggle_focus(&mut self) {
        self.category_focus = !self.category_focus;
        // Reset item selection when switching to settings
        if !self.category_focus && self.selected_item >= self.current_page().map_or(0, |p| p.items.len()) {
            self.selected_item = 0;
        }
    }

    /// Record a pending change for a setting
    pub fn set_pending_change(&mut self, path: &str, value: serde_json::Value) {
        // Check if this is the same as the original value
        let original = self.original_config.pointer(path);
        if original == Some(&value) {
            self.pending_changes.remove(path);
        } else {
            self.pending_changes.insert(path.to_string(), value);
        }
    }

    /// Check if there are unsaved changes
    pub fn has_changes(&self) -> bool {
        !self.pending_changes.is_empty()
    }

    /// Apply pending changes to a config
    pub fn apply_changes(&self, config: &Config) -> Result<Config, serde_json::Error> {
        let mut config_value = serde_json::to_value(config)?;

        for (path, value) in &self.pending_changes {
            if let Some(target) = config_value.pointer_mut(path) {
                *target = value.clone();
            }
        }

        serde_json::from_value(config_value)
    }

    /// Discard all pending changes
    pub fn discard_changes(&mut self) {
        self.pending_changes.clear();
        // Rebuild pages from original config
        self.pages = super::items::build_pages(&self.categories, &self.original_config);
    }

    /// Reset the current item to its default value
    pub fn reset_current_to_default(&mut self) {
        // Get the info we need first, then release the borrow
        let reset_info = self.current_item().and_then(|item| {
            item.default.as_ref().map(|default| {
                (item.path.clone(), default.clone())
            })
        });

        if let Some((path, default)) = reset_info {
            self.set_pending_change(&path, default.clone());

            // Now update the control state
            if let Some(item) = self.current_item_mut() {
                update_control_from_value(&mut item.control, &default);
                item.modified = false;
            }
        }
    }

    /// Handle a value change from user interaction
    pub fn on_value_changed(&mut self) {
        // Get value and path first, then release borrow
        let change_info = self.current_item().map(|item| {
            let value = control_to_value(&item.control);
            let modified = match &item.default {
                Some(default) => &value != default,
                None => true,
            };
            (item.path.clone(), value, modified)
        });

        if let Some((path, value, modified)) = change_info {
            // Update modified flag
            if let Some(item) = self.current_item_mut() {
                item.modified = modified;
            }
            self.set_pending_change(&path, value);
        }
    }

    /// Update focus states for rendering
    pub fn update_focus_states(&mut self) {
        for (page_idx, page) in self.pages.iter_mut().enumerate() {
            for (item_idx, item) in page.items.iter_mut().enumerate() {
                let is_focused = !self.category_focus
                    && page_idx == self.selected_category
                    && item_idx == self.selected_item;

                let focus = if is_focused {
                    FocusState::Focused
                } else {
                    FocusState::Normal
                };

                match &mut item.control {
                    SettingControl::Toggle(state) => state.focus = focus,
                    SettingControl::Number(state) => state.focus = focus,
                    SettingControl::Dropdown(state) => state.focus = focus,
                    SettingControl::Text(state) => state.focus = focus,
                    SettingControl::TextList(state) => state.focus = focus,
                    SettingControl::Complex { .. } => {}
                }
            }
        }
    }

    /// Start search mode
    pub fn start_search(&mut self) {
        self.search_active = true;
        self.search_query.clear();
    }

    /// Cancel search mode
    pub fn cancel_search(&mut self) {
        self.search_active = false;
        self.search_query.clear();
    }
}

/// Update a control's state from a JSON value
fn update_control_from_value(control: &mut SettingControl, value: &serde_json::Value) {
    match control {
        SettingControl::Toggle(state) => {
            if let Some(b) = value.as_bool() {
                state.checked = b;
            }
        }
        SettingControl::Number(state) => {
            if let Some(n) = value.as_i64() {
                state.value = n;
            }
        }
        SettingControl::Dropdown(state) => {
            if let Some(s) = value.as_str() {
                if let Some(idx) = state.options.iter().position(|o| o == s) {
                    state.selected = idx;
                }
            }
        }
        SettingControl::Text(state) => {
            if let Some(s) = value.as_str() {
                state.value = s.to_string();
                state.cursor = state.value.len();
            }
        }
        SettingControl::TextList(state) => {
            if let Some(arr) = value.as_array() {
                state.items = arr
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
            }
        }
        SettingControl::Complex { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SCHEMA: &str = r#"
{
  "type": "object",
  "properties": {
    "theme": {
      "type": "string",
      "default": "dark"
    },
    "line_numbers": {
      "type": "boolean",
      "default": true
    }
  },
  "$defs": {}
}
"#;

    fn test_config() -> Config {
        Config::default()
    }

    #[test]
    fn test_settings_state_creation() {
        let config = test_config();
        let state = SettingsState::new(TEST_SCHEMA, &config).unwrap();

        assert!(!state.visible);
        assert_eq!(state.selected_category, 0);
        assert!(!state.has_changes());
    }

    #[test]
    fn test_navigation() {
        let config = test_config();
        let mut state = SettingsState::new(TEST_SCHEMA, &config).unwrap();

        // Start in category focus
        assert!(state.category_focus);

        // Toggle to settings
        state.toggle_focus();
        assert!(!state.category_focus);

        // Navigate items
        state.select_next();
        assert_eq!(state.selected_item, 1);

        state.select_prev();
        assert_eq!(state.selected_item, 0);
    }

    #[test]
    fn test_pending_changes() {
        let config = test_config();
        let mut state = SettingsState::new(TEST_SCHEMA, &config).unwrap();

        assert!(!state.has_changes());

        state.set_pending_change("/theme", serde_json::Value::String("light".to_string()));
        assert!(state.has_changes());

        state.discard_changes();
        assert!(!state.has_changes());
    }

    #[test]
    fn test_show_hide() {
        let config = test_config();
        let mut state = SettingsState::new(TEST_SCHEMA, &config).unwrap();

        assert!(!state.visible);

        state.show();
        assert!(state.visible);
        assert!(state.category_focus);

        state.hide();
        assert!(!state.visible);
    }
}
