//! Settings state management
//!
//! Tracks the current state of the settings UI, pending changes,
//! and provides methods for reading/writing config values.

use super::items::{control_to_value, SettingControl, SettingItem, SettingsPage};
use super::layout::SettingsHit;
use super::schema::{parse_schema, SettingCategory};
use super::search::{search_settings, SearchResult};
use crate::config::Config;
use crate::view::controls::FocusState;
use crate::view::ui::ScrollablePanel;
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
    /// Current search results
    pub search_results: Vec<SearchResult>,
    /// Selected search result index
    pub selected_search_result: usize,
    /// Whether the unsaved changes confirmation dialog is showing
    pub showing_confirm_dialog: bool,
    /// Selected option in confirmation dialog (0=Save, 1=Discard, 2=Cancel)
    pub confirm_dialog_selection: usize,
    /// Whether the help overlay is showing
    pub showing_help: bool,
    /// Scrollable panel for settings items
    pub scroll_panel: ScrollablePanel,
    /// Sub-focus index within the selected item (for TextList/Map navigation)
    pub sub_focus: Option<usize>,
    /// Whether we're in text editing mode (for TextList controls)
    pub editing_text: bool,
    /// Current mouse hover position (for hover feedback)
    pub hover_position: Option<(u16, u16)>,
    /// Current hover hit result (computed from hover_position and cached layout)
    pub hover_hit: Option<SettingsHit>,
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
            search_results: Vec::new(),
            selected_search_result: 0,
            showing_confirm_dialog: false,
            confirm_dialog_selection: 0,
            showing_help: false,
            scroll_panel: ScrollablePanel::new(),
            sub_focus: None,
            editing_text: false,
            hover_position: None,
            hover_hit: None,
        })
    }

    /// Show the settings panel
    pub fn show(&mut self) {
        self.visible = true;
        self.category_focus = true;
        self.selected_category = 0;
        self.selected_item = 0;
        self.scroll_panel = ScrollablePanel::new();
        self.sub_focus = None;
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
                self.scroll_panel = ScrollablePanel::new();
                self.sub_focus = None;
            }
        } else if self.selected_item > 0 {
            self.selected_item -= 1;
            self.sub_focus = None;
            self.ensure_visible();
        }
    }

    /// Move selection down
    pub fn select_next(&mut self) {
        if self.category_focus {
            if self.selected_category + 1 < self.pages.len() {
                self.selected_category += 1;
                self.selected_item = 0;
                self.scroll_panel = ScrollablePanel::new();
                self.sub_focus = None;
            }
        } else if let Some(page) = self.current_page() {
            if self.selected_item + 1 < page.items.len() {
                self.selected_item += 1;
                self.sub_focus = None;
                self.ensure_visible();
            }
        }
    }

    /// Switch focus between category list and settings
    pub fn toggle_focus(&mut self) {
        self.category_focus = !self.category_focus;
        // Reset item selection when switching to settings
        if !self.category_focus
            && self.selected_item >= self.current_page().map_or(0, |p| p.items.len())
        {
            self.selected_item = 0;
        }
        self.sub_focus = None;
        self.ensure_visible();
    }

    /// Ensure the selected item is visible in the viewport
    pub fn ensure_visible(&mut self) {
        if self.category_focus {
            return;
        }

        // Need to avoid borrowing self for both page and scroll_panel
        let selected_item = self.selected_item;
        let sub_focus = self.sub_focus;
        if let Some(page) = self.pages.get(self.selected_category) {
            self.scroll_panel
                .ensure_focused_visible(&page.items, selected_item, sub_focus);
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
            item.default
                .as_ref()
                .map(|default| (item.path.clone(), default.clone()))
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
                    SettingControl::Map(state) => state.focus = focus,
                    SettingControl::Complex { .. } => {}
                }
            }
        }
    }

    /// Start search mode
    pub fn start_search(&mut self) {
        self.search_active = true;
        self.search_query.clear();
        self.search_results.clear();
        self.selected_search_result = 0;
    }

    /// Cancel search mode
    pub fn cancel_search(&mut self) {
        self.search_active = false;
        self.search_query.clear();
        self.search_results.clear();
        self.selected_search_result = 0;
    }

    /// Update search query and refresh results
    pub fn set_search_query(&mut self, query: String) {
        self.search_query = query;
        self.search_results = search_settings(&self.pages, &self.search_query);
        self.selected_search_result = 0;
    }

    /// Add a character to the search query
    pub fn search_push_char(&mut self, c: char) {
        self.search_query.push(c);
        self.search_results = search_settings(&self.pages, &self.search_query);
        self.selected_search_result = 0;
    }

    /// Remove the last character from the search query
    pub fn search_pop_char(&mut self) {
        self.search_query.pop();
        self.search_results = search_settings(&self.pages, &self.search_query);
        self.selected_search_result = 0;
    }

    /// Navigate to previous search result
    pub fn search_prev(&mut self) {
        if !self.search_results.is_empty() && self.selected_search_result > 0 {
            self.selected_search_result -= 1;
        }
    }

    /// Navigate to next search result
    pub fn search_next(&mut self) {
        if !self.search_results.is_empty()
            && self.selected_search_result + 1 < self.search_results.len()
        {
            self.selected_search_result += 1;
        }
    }

    /// Jump to the currently selected search result
    pub fn jump_to_search_result(&mut self) {
        if let Some(result) = self.search_results.get(self.selected_search_result) {
            self.selected_category = result.page_index;
            self.selected_item = result.item_index;
            self.category_focus = false;
            self.scroll_panel = ScrollablePanel::new(); // Reset scroll when jumping to new category
            self.sub_focus = None;
            self.ensure_visible();
            self.cancel_search();
        }
    }

    /// Get the currently selected search result
    pub fn current_search_result(&self) -> Option<&SearchResult> {
        self.search_results.get(self.selected_search_result)
    }

    /// Show the unsaved changes confirmation dialog
    pub fn show_confirm_dialog(&mut self) {
        self.showing_confirm_dialog = true;
        self.confirm_dialog_selection = 0; // Default to "Save and Exit"
    }

    /// Hide the confirmation dialog
    pub fn hide_confirm_dialog(&mut self) {
        self.showing_confirm_dialog = false;
        self.confirm_dialog_selection = 0;
    }

    /// Move to next option in confirmation dialog
    pub fn confirm_dialog_next(&mut self) {
        self.confirm_dialog_selection = (self.confirm_dialog_selection + 1) % 3;
    }

    /// Move to previous option in confirmation dialog
    pub fn confirm_dialog_prev(&mut self) {
        self.confirm_dialog_selection = if self.confirm_dialog_selection == 0 {
            2
        } else {
            self.confirm_dialog_selection - 1
        };
    }

    /// Toggle the help overlay
    pub fn toggle_help(&mut self) {
        self.showing_help = !self.showing_help;
    }

    /// Hide the help overlay
    pub fn hide_help(&mut self) {
        self.showing_help = false;
    }

    /// Get the maximum scroll offset for the current page (in rows)
    pub fn max_scroll(&self) -> u16 {
        self.scroll_panel.scroll.max_offset()
    }

    /// Scroll up by a given number of rows
    /// Returns true if the scroll offset changed
    pub fn scroll_up(&mut self, delta: usize) -> bool {
        let old = self.scroll_panel.scroll.offset;
        self.scroll_panel.scroll_up(delta as u16);
        old != self.scroll_panel.scroll.offset
    }

    /// Scroll down by a given number of rows
    /// Returns true if the scroll offset changed
    pub fn scroll_down(&mut self, delta: usize) -> bool {
        let old = self.scroll_panel.scroll.offset;
        self.scroll_panel.scroll_down(delta as u16);
        old != self.scroll_panel.scroll.offset
    }

    /// Scroll to a position based on a ratio (0.0 to 1.0)
    /// Returns true if the scroll offset changed
    pub fn scroll_to_ratio(&mut self, ratio: f32) -> bool {
        let old = self.scroll_panel.scroll.offset;
        self.scroll_panel.scroll_to_ratio(ratio);
        old != self.scroll_panel.scroll.offset
    }

    /// Start text editing mode for TextList, Text, or Map controls
    pub fn start_editing(&mut self) {
        if let Some(item) = self.current_item() {
            if matches!(
                item.control,
                SettingControl::TextList(_) | SettingControl::Text(_) | SettingControl::Map(_)
            ) {
                self.editing_text = true;
            }
        }
    }

    /// Stop text editing mode
    pub fn stop_editing(&mut self) {
        self.editing_text = false;
    }

    /// Check if the current item is editable (TextList, Text, or Map)
    pub fn is_editable_control(&self) -> bool {
        self.current_item().map_or(false, |item| {
            matches!(
                item.control,
                SettingControl::TextList(_) | SettingControl::Text(_) | SettingControl::Map(_)
            )
        })
    }

    /// Insert a character into the current editable control
    pub fn text_insert(&mut self, c: char) {
        if let Some(item) = self.current_item_mut() {
            match &mut item.control {
                SettingControl::TextList(state) => state.insert(c),
                SettingControl::Text(state) => {
                    state.value.insert(state.cursor, c);
                    state.cursor += c.len_utf8();
                }
                SettingControl::Map(state) => {
                    state.new_key_text.insert(state.cursor, c);
                    state.cursor += c.len_utf8();
                }
                _ => {}
            }
        }
    }

    /// Backspace in the current editable control
    pub fn text_backspace(&mut self) {
        if let Some(item) = self.current_item_mut() {
            match &mut item.control {
                SettingControl::TextList(state) => state.backspace(),
                SettingControl::Text(state) => {
                    if state.cursor > 0 {
                        let mut char_start = state.cursor - 1;
                        while char_start > 0 && !state.value.is_char_boundary(char_start) {
                            char_start -= 1;
                        }
                        state.value.remove(char_start);
                        state.cursor = char_start;
                    }
                }
                SettingControl::Map(state) => {
                    if state.cursor > 0 {
                        let mut char_start = state.cursor - 1;
                        while char_start > 0 && !state.new_key_text.is_char_boundary(char_start) {
                            char_start -= 1;
                        }
                        state.new_key_text.remove(char_start);
                        state.cursor = char_start;
                    }
                }
                _ => {}
            }
        }
    }

    /// Move cursor left in the current editable control
    pub fn text_move_left(&mut self) {
        if let Some(item) = self.current_item_mut() {
            match &mut item.control {
                SettingControl::TextList(state) => state.move_left(),
                SettingControl::Text(state) => {
                    if state.cursor > 0 {
                        let mut new_pos = state.cursor - 1;
                        while new_pos > 0 && !state.value.is_char_boundary(new_pos) {
                            new_pos -= 1;
                        }
                        state.cursor = new_pos;
                    }
                }
                SettingControl::Map(state) => {
                    if state.cursor > 0 {
                        let mut new_pos = state.cursor - 1;
                        while new_pos > 0 && !state.new_key_text.is_char_boundary(new_pos) {
                            new_pos -= 1;
                        }
                        state.cursor = new_pos;
                    }
                }
                _ => {}
            }
        }
    }

    /// Move cursor right in the current editable control
    pub fn text_move_right(&mut self) {
        if let Some(item) = self.current_item_mut() {
            match &mut item.control {
                SettingControl::TextList(state) => state.move_right(),
                SettingControl::Text(state) => {
                    if state.cursor < state.value.len() {
                        let mut new_pos = state.cursor + 1;
                        while new_pos < state.value.len() && !state.value.is_char_boundary(new_pos)
                        {
                            new_pos += 1;
                        }
                        state.cursor = new_pos;
                    }
                }
                SettingControl::Map(state) => {
                    if state.cursor < state.new_key_text.len() {
                        let mut new_pos = state.cursor + 1;
                        while new_pos < state.new_key_text.len()
                            && !state.new_key_text.is_char_boundary(new_pos)
                        {
                            new_pos += 1;
                        }
                        state.cursor = new_pos;
                    }
                }
                _ => {}
            }
        }
    }

    /// Move focus to previous item in TextList/Map
    pub fn text_focus_prev(&mut self) {
        if let Some(item) = self.current_item_mut() {
            match &mut item.control {
                SettingControl::TextList(state) => state.focus_prev(),
                SettingControl::Map(state) => {
                    // Move focus to previous entry (None = add-new field)
                    match state.focused_entry {
                        None if !state.entries.is_empty() => {
                            state.focused_entry = Some(state.entries.len() - 1);
                        }
                        Some(0) => {
                            state.focused_entry = None;
                        }
                        Some(idx) => {
                            state.focused_entry = Some(idx - 1);
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    /// Move focus to next item in TextList/Map
    pub fn text_focus_next(&mut self) {
        if let Some(item) = self.current_item_mut() {
            match &mut item.control {
                SettingControl::TextList(state) => state.focus_next(),
                SettingControl::Map(state) => {
                    // Move focus to next entry (None = add-new field)
                    match state.focused_entry {
                        None => {
                            if !state.entries.is_empty() {
                                state.focused_entry = Some(0);
                            }
                        }
                        Some(idx) if idx + 1 < state.entries.len() => {
                            state.focused_entry = Some(idx + 1);
                        }
                        Some(_) => {
                            state.focused_entry = None;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Add new item in TextList/Map (from the new item field)
    pub fn text_add_item(&mut self) {
        if let Some(item) = self.current_item_mut() {
            match &mut item.control {
                SettingControl::TextList(state) => state.add_item(),
                SettingControl::Map(state) => state.add_entry_from_input(),
                _ => {}
            }
        }
        // Record the change
        self.on_value_changed();
    }

    /// Remove the currently focused item in TextList/Map
    pub fn text_remove_focused(&mut self) {
        if let Some(item) = self.current_item_mut() {
            match &mut item.control {
                SettingControl::TextList(state) => {
                    if let Some(idx) = state.focused_item {
                        state.remove_item(idx);
                    }
                }
                SettingControl::Map(state) => {
                    if let Some(idx) = state.focused_entry {
                        state.remove_entry(idx);
                    }
                }
                _ => {}
            }
        }
        // Record the change
        self.on_value_changed();
    }

    // =========== Dropdown methods ===========

    /// Check if current item is a dropdown with menu open
    pub fn is_dropdown_open(&self) -> bool {
        self.current_item().map_or(false, |item| {
            if let SettingControl::Dropdown(ref d) = item.control {
                d.open
            } else {
                false
            }
        })
    }

    /// Toggle dropdown open/closed
    pub fn dropdown_toggle(&mut self) {
        let mut opened = false;
        if let Some(item) = self.current_item_mut() {
            if let SettingControl::Dropdown(ref mut d) = item.control {
                d.toggle_open();
                opened = d.open;
            }
        }

        // When dropdown opens, update content height and ensure it's visible
        if opened {
            // Update content height since item is now taller
            let selected_item = self.selected_item;
            if let Some(page) = self.pages.get(self.selected_category) {
                self.scroll_panel.update_content_height(&page.items);
                // Ensure the dropdown item is visible with its new expanded height
                self.scroll_panel
                    .ensure_focused_visible(&page.items, selected_item, None);
            }
        }
    }

    /// Select previous option in dropdown
    pub fn dropdown_prev(&mut self) {
        if let Some(item) = self.current_item_mut() {
            if let SettingControl::Dropdown(ref mut d) = item.control {
                d.select_prev();
            }
        }
    }

    /// Select next option in dropdown
    pub fn dropdown_next(&mut self) {
        if let Some(item) = self.current_item_mut() {
            if let SettingControl::Dropdown(ref mut d) = item.control {
                d.select_next();
            }
        }
    }

    /// Confirm dropdown selection (close and record change)
    pub fn dropdown_confirm(&mut self) {
        if let Some(item) = self.current_item_mut() {
            if let SettingControl::Dropdown(ref mut d) = item.control {
                d.confirm();
            }
        }
        self.on_value_changed();
    }

    /// Cancel dropdown (restore original value and close)
    pub fn dropdown_cancel(&mut self) {
        if let Some(item) = self.current_item_mut() {
            if let SettingControl::Dropdown(ref mut d) = item.control {
                d.cancel();
            }
        }
    }

    /// Get list of pending changes for display
    pub fn get_change_descriptions(&self) -> Vec<String> {
        self.pending_changes
            .iter()
            .map(|(path, value)| {
                let value_str = match value {
                    serde_json::Value::Bool(b) => b.to_string(),
                    serde_json::Value::Number(n) => n.to_string(),
                    serde_json::Value::String(s) => format!("\"{}\"", s),
                    _ => value.to_string(),
                };
                format!("{}: {}", path, value_str)
            })
            .collect()
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
        SettingControl::Map(state) => {
            if let Some(obj) = value.as_object() {
                state.entries = obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                state.entries.sort_by(|a, b| a.0.cmp(&b.0));
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
