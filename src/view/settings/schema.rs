//! JSON Schema parsing for settings UI
//!
//! Parses the config JSON Schema to build the settings UI structure.

use serde::Deserialize;
use std::collections::HashMap;

/// A property/setting from the schema
#[derive(Debug, Clone)]
pub struct SettingSchema {
    /// JSON pointer path (e.g., "/editor/tab_size")
    pub path: String,
    /// Human-readable name derived from property name
    pub name: String,
    /// Description from schema
    pub description: Option<String>,
    /// The type of this setting
    pub setting_type: SettingType,
    /// Default value (as JSON)
    pub default: Option<serde_json::Value>,
}

/// Type of a setting, determines which control to render
#[derive(Debug, Clone)]
pub enum SettingType {
    /// Boolean toggle
    Boolean,
    /// Integer number with optional min/max
    Integer { minimum: Option<i64>, maximum: Option<i64> },
    /// Floating point number
    Number { minimum: Option<f64>, maximum: Option<f64> },
    /// Free-form string
    String,
    /// String with enumerated options
    Enum { options: Vec<String> },
    /// Array of strings
    StringArray,
    /// Nested object (category)
    Object { properties: Vec<SettingSchema> },
    /// Map with string keys (for languages, lsp configs)
    Map { value_schema: Box<SettingSchema> },
    /// Complex type we can't edit directly
    Complex,
}

/// A category in the settings tree
#[derive(Debug, Clone)]
pub struct SettingCategory {
    /// Category name (e.g., "Editor", "File Explorer")
    pub name: String,
    /// JSON path prefix for this category
    pub path: String,
    /// Description of this category
    pub description: Option<String>,
    /// Settings in this category
    pub settings: Vec<SettingSchema>,
    /// Subcategories
    pub subcategories: Vec<SettingCategory>,
}

/// Raw JSON Schema structure for deserialization
#[derive(Debug, Deserialize)]
struct RawSchema {
    #[serde(rename = "type")]
    schema_type: Option<String>,
    description: Option<String>,
    default: Option<serde_json::Value>,
    properties: Option<HashMap<String, RawSchema>>,
    items: Option<Box<RawSchema>>,
    #[serde(rename = "enum")]
    enum_values: Option<Vec<serde_json::Value>>,
    minimum: Option<serde_json::Number>,
    maximum: Option<serde_json::Number>,
    #[serde(rename = "$ref")]
    ref_path: Option<String>,
    #[serde(rename = "$defs")]
    defs: Option<HashMap<String, RawSchema>>,
    #[serde(rename = "additionalProperties")]
    additional_properties: Option<Box<RawSchema>>,
}

/// Parse the JSON Schema and build the category tree
pub fn parse_schema(schema_json: &str) -> Result<Vec<SettingCategory>, serde_json::Error> {
    let raw: RawSchema = serde_json::from_str(schema_json)?;

    let defs = raw.defs.unwrap_or_default();
    let properties = raw.properties.unwrap_or_default();

    let mut categories = Vec::new();
    let mut top_level_settings = Vec::new();

    // Process each top-level property
    for (name, prop) in properties {
        let path = format!("/{}", name);
        let display_name = humanize_name(&name);

        // Resolve references
        let resolved = resolve_ref(&prop, &defs);

        // Check if this is a nested object (category) or a simple setting
        if let Some(ref inner_props) = resolved.properties {
            // This is a category with nested settings
            let settings = parse_properties(inner_props, &path, &defs);
            categories.push(SettingCategory {
                name: display_name,
                path: path.clone(),
                description: resolved.description.clone(),
                settings,
                subcategories: Vec::new(),
            });
        } else {
            // This is a top-level setting
            let setting = parse_setting(&name, &path, &resolved, &defs);
            top_level_settings.push(setting);
        }
    }

    // If there are top-level settings, create a "General" category for them
    if !top_level_settings.is_empty() {
        categories.insert(0, SettingCategory {
            name: "General".to_string(),
            path: String::new(),
            description: Some("General settings".to_string()),
            settings: top_level_settings,
            subcategories: Vec::new(),
        });
    }

    // Sort categories alphabetically, but keep General first
    categories.sort_by(|a, b| {
        match (a.name.as_str(), b.name.as_str()) {
            ("General", _) => std::cmp::Ordering::Less,
            (_, "General") => std::cmp::Ordering::Greater,
            (a, b) => a.cmp(b),
        }
    });

    Ok(categories)
}

/// Parse properties into settings
fn parse_properties(
    properties: &HashMap<String, RawSchema>,
    parent_path: &str,
    defs: &HashMap<String, RawSchema>,
) -> Vec<SettingSchema> {
    let mut settings = Vec::new();

    for (name, prop) in properties {
        let path = format!("{}/{}", parent_path, name);
        let resolved = resolve_ref(prop, defs);
        let setting = parse_setting(name, &path, &resolved, defs);
        settings.push(setting);
    }

    // Sort settings alphabetically by name
    settings.sort_by(|a, b| a.name.cmp(&b.name));

    settings
}

/// Parse a single setting from its schema
fn parse_setting(
    name: &str,
    path: &str,
    schema: &RawSchema,
    defs: &HashMap<String, RawSchema>,
) -> SettingSchema {
    let setting_type = determine_type(schema, defs);

    SettingSchema {
        path: path.to_string(),
        name: humanize_name(name),
        description: schema.description.clone(),
        setting_type,
        default: schema.default.clone(),
    }
}

/// Determine the SettingType from a schema
fn determine_type(schema: &RawSchema, defs: &HashMap<String, RawSchema>) -> SettingType {
    // Check for enum first
    if let Some(ref values) = schema.enum_values {
        let options: Vec<String> = values
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        if !options.is_empty() {
            return SettingType::Enum { options };
        }
    }

    // Check type field
    match schema.schema_type.as_deref() {
        Some("boolean") => SettingType::Boolean,
        Some("integer") => {
            let minimum = schema.minimum.as_ref().and_then(|n| n.as_i64());
            let maximum = schema.maximum.as_ref().and_then(|n| n.as_i64());
            SettingType::Integer { minimum, maximum }
        }
        Some("number") => {
            let minimum = schema.minimum.as_ref().and_then(|n| n.as_f64());
            let maximum = schema.maximum.as_ref().and_then(|n| n.as_f64());
            SettingType::Number { minimum, maximum }
        }
        Some("string") => SettingType::String,
        Some("array") => {
            // Check if it's an array of strings
            if let Some(ref items) = schema.items {
                let resolved = resolve_ref(items, defs);
                if resolved.schema_type.as_deref() == Some("string") {
                    return SettingType::StringArray;
                }
            }
            SettingType::Complex
        }
        Some("object") => {
            // Check for additionalProperties (map type)
            if let Some(ref add_props) = schema.additional_properties {
                let resolved = resolve_ref(add_props, defs);
                let value_schema = parse_setting("value", "", &resolved, defs);
                return SettingType::Map {
                    value_schema: Box::new(value_schema),
                };
            }
            // Regular object with fixed properties
            if let Some(ref props) = schema.properties {
                let properties = parse_properties(props, "", defs);
                return SettingType::Object { properties };
            }
            SettingType::Complex
        }
        _ => SettingType::Complex,
    }
}

/// Resolve a $ref to its definition
fn resolve_ref<'a>(schema: &'a RawSchema, defs: &'a HashMap<String, RawSchema>) -> &'a RawSchema {
    if let Some(ref ref_path) = schema.ref_path {
        // Parse ref path like "#/$defs/EditorConfig"
        if let Some(def_name) = ref_path.strip_prefix("#/$defs/") {
            if let Some(def) = defs.get(def_name) {
                return def;
            }
        }
    }
    schema
}

/// Convert snake_case to Title Case
fn humanize_name(name: &str) -> String {
    name.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_SCHEMA: &str = r##"
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "Config",
  "type": "object",
  "properties": {
    "theme": {
      "description": "Color theme name",
      "type": "string",
      "default": "high-contrast"
    },
    "check_for_updates": {
      "description": "Check for new versions on quit",
      "type": "boolean",
      "default": true
    },
    "editor": {
      "description": "Editor settings",
      "$ref": "#/$defs/EditorConfig"
    }
  },
  "$defs": {
    "EditorConfig": {
      "description": "Editor behavior configuration",
      "type": "object",
      "properties": {
        "tab_size": {
          "description": "Number of spaces per tab",
          "type": "integer",
          "minimum": 1,
          "maximum": 16,
          "default": 4
        },
        "line_numbers": {
          "description": "Show line numbers",
          "type": "boolean",
          "default": true
        }
      }
    }
  }
}
"##;

    #[test]
    fn test_parse_schema() {
        let categories = parse_schema(SAMPLE_SCHEMA).unwrap();

        // Should have General and Editor categories
        assert_eq!(categories.len(), 2);
        assert_eq!(categories[0].name, "General");
        assert_eq!(categories[1].name, "Editor");
    }

    #[test]
    fn test_general_category() {
        let categories = parse_schema(SAMPLE_SCHEMA).unwrap();
        let general = &categories[0];

        // General should have theme and check_for_updates
        assert_eq!(general.settings.len(), 2);

        let theme = general.settings.iter().find(|s| s.path == "/theme").unwrap();
        assert!(matches!(theme.setting_type, SettingType::String));

        let updates = general.settings.iter().find(|s| s.path == "/check_for_updates").unwrap();
        assert!(matches!(updates.setting_type, SettingType::Boolean));
    }

    #[test]
    fn test_editor_category() {
        let categories = parse_schema(SAMPLE_SCHEMA).unwrap();
        let editor = &categories[1];

        assert_eq!(editor.path, "/editor");
        assert_eq!(editor.settings.len(), 2);

        let tab_size = editor.settings.iter().find(|s| s.name == "Tab Size").unwrap();
        if let SettingType::Integer { minimum, maximum } = &tab_size.setting_type {
            assert_eq!(*minimum, Some(1));
            assert_eq!(*maximum, Some(16));
        } else {
            panic!("Expected integer type");
        }
    }

    #[test]
    fn test_humanize_name() {
        assert_eq!(humanize_name("tab_size"), "Tab Size");
        assert_eq!(humanize_name("line_numbers"), "Line Numbers");
        assert_eq!(humanize_name("check_for_updates"), "Check For Updates");
        assert_eq!(humanize_name("lsp"), "Lsp");
    }
}
