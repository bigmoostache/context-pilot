use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

// =============================================================================
// YAML Tool Text — deserialized from yamls/tools/*.yaml
// =============================================================================

/// Root structure of a tool YAML file.
#[derive(Debug, Clone, Deserialize)]
pub struct ToolTexts {
    pub tools: HashMap<String, ToolText>,
}

/// LLM-facing text for a single tool: description + parameter descriptions.
#[derive(Debug, Clone, Deserialize)]
pub struct ToolText {
    pub description: String,
    #[serde(default)]
    pub parameters: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUse {
    pub id: String,
    pub name: String,
    pub input: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_use_id: String,
    pub content: String,
    #[serde(default)]
    pub is_error: bool,
    #[serde(default)]
    pub tool_name: String,
}

impl ToolResult {
    /// Create a new ToolResult. The tool_name will be populated by dispatch_tool.
    pub fn new(tool_use_id: String, content: String, is_error: bool) -> Self {
        Self { tool_use_id, content, is_error, tool_name: String::new() }
    }

    /// Create a new ToolResult with tool_name specified.
    pub fn with_name(tool_use_id: String, content: String, is_error: bool, tool_name: String) -> Self {
        Self { tool_use_id, content, is_error, tool_name }
    }
}

// =============================================================================
// Tool Definitions
// =============================================================================

/// Parameter type for tool inputs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamType {
    String,
    Integer,
    Number,
    Boolean,
    Array(Box<ParamType>),
    Object(Vec<ToolParam>),
}

impl ParamType {
    fn to_json_schema(&self) -> Value {
        match self {
            ParamType::String => json!({"type": "string"}),
            ParamType::Integer => json!({"type": "integer"}),
            ParamType::Number => json!({"type": "number"}),
            ParamType::Boolean => json!({"type": "boolean"}),
            ParamType::Array(inner) => json!({
                "type": "array",
                "items": inner.to_json_schema()
            }),
            ParamType::Object(params) => {
                let mut properties = serde_json::Map::new();
                let mut required = Vec::new();
                for param in params {
                    let mut schema = param.param_type.to_json_schema();
                    if let Some(desc) = &param.description {
                        schema["description"] = json!(desc);
                    }
                    if let Some(enum_vals) = &param.enum_values {
                        schema["enum"] = json!(enum_vals);
                    }
                    properties.insert(param.name.clone(), schema);
                    if param.required {
                        required.push(param.name.clone());
                    }
                }
                json!({
                    "type": "object",
                    "properties": properties,
                    "required": required
                })
            }
        }
    }
}

/// A single tool parameter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolParam {
    pub name: String,
    pub param_type: ParamType,
    pub description: Option<String>,
    pub required: bool,
    pub enum_values: Option<Vec<String>>,
    pub default: Option<String>,
}

impl ToolParam {
    pub fn new(name: &str, param_type: ParamType) -> Self {
        Self {
            name: name.to_string(),
            param_type,
            description: None,
            required: false,
            enum_values: None,
            default: None,
        }
    }

    pub fn desc(mut self, d: &str) -> Self {
        self.description = Some(d.to_string());
        self
    }

    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    pub fn enum_vals(mut self, vals: &[&str]) -> Self {
        self.enum_values = Some(vals.iter().map(|s| s.to_string()).collect());
        self
    }

    pub fn default_val(mut self, val: &str) -> Self {
        self.default = Some(val.to_string());
        self
    }
}

/// A tool definition with its schema and prompt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Tool identifier (e.g., "open_file")
    pub id: String,
    /// Display name
    pub name: String,
    /// Short description for the sidebar
    pub short_desc: String,
    /// Full description for LLM prompt
    pub description: String,
    /// Structured parameters
    pub params: Vec<ToolParam>,
    /// Whether this tool is currently enabled
    pub enabled: bool,
    /// Whether this tool is available to reverie sub-agents (context optimizers)
    pub reverie_allowed: bool,
    /// Category for grouping (e.g., "File", "Git", "System")
    pub category: String,
}

// =============================================================================
// YAML-driven ToolDefinition builder
// =============================================================================

impl ToolDefinition {
    /// Create a ToolDefinition from YAML text, pulling description + param descs
    /// from the `ToolTexts` map. Panics if the tool ID is missing from YAML.
    pub fn from_yaml<'a>(id: &str, texts: &'a ToolTexts) -> ToolDefBuilder<'a> {
        let text = texts.tools.get(id).unwrap_or_else(|| {
            panic!("Tool '{}' not found in YAML", id);
        });
        ToolDefBuilder {
            id: id.to_string(),
            description: text.description.trim().to_string(),
            param_descs: &text.parameters,
            params: Vec::new(),
            short_desc: String::new(),
            category: String::new(),
            enabled: true,
            reverie_allowed: false,
        }
    }
}

/// Builder for constructing a ToolDefinition from YAML text.
/// Schema structure (types, required, enums) lives in Rust.
/// Descriptions (sentences) come from YAML automatically.
pub struct ToolDefBuilder<'a> {
    id: String,
    description: String,
    param_descs: &'a HashMap<String, String>,
    params: Vec<ToolParam>,
    short_desc: String,
    category: String,
    enabled: bool,
    reverie_allowed: bool,
}

impl<'a> ToolDefBuilder<'a> {
    pub fn short_desc(mut self, s: &str) -> Self {
        self.short_desc = s.to_string();
        self
    }

    pub fn category(mut self, c: &str) -> Self {
        self.category = c.to_string();
        self
    }

    pub fn reverie_allowed(mut self, allowed: bool) -> Self {
        self.reverie_allowed = allowed;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Add a parameter. Description is auto-pulled from YAML by param name.
    pub fn param(mut self, name: &str, param_type: ParamType, required: bool) -> Self {
        let desc = self.param_descs.get(name).cloned();
        let mut p = ToolParam::new(name, param_type);
        p.description = desc;
        if required {
            p.required = true;
        }
        self.params.push(p);
        self
    }

    /// Add a parameter with enum values. Description from YAML.
    pub fn param_enum(mut self, name: &str, values: &[&str], required: bool) -> Self {
        let desc = self.param_descs.get(name).cloned();
        let mut p = ToolParam::new(name, ParamType::String);
        p.description = desc;
        p.enum_values = Some(values.iter().map(|s| s.to_string()).collect());
        if required {
            p.required = true;
        }
        self.params.push(p);
        self
    }

    /// Add a parameter with a default value. Description from YAML.
    pub fn param_with_default(mut self, name: &str, param_type: ParamType, default: &str) -> Self {
        let desc = self.param_descs.get(name).cloned();
        let mut p = ToolParam::new(name, param_type);
        p.description = desc;
        p.default = Some(default.to_string());
        self.params.push(p);
        self
    }

    /// Add a parameter with array type. Description from YAML.
    pub fn param_array(mut self, name: &str, items: ParamType, required: bool) -> Self {
        let desc = self.param_descs.get(name).cloned();
        let mut p = ToolParam::new(name, ParamType::Array(Box::new(items)));
        p.description = desc;
        if required {
            p.required = true;
        }
        self.params.push(p);
        self
    }

    /// Add a parameter with object type (nested params). Description from YAML.
    pub fn param_object(mut self, name: &str, fields: Vec<ToolParam>, required: bool) -> Self {
        let desc = self.param_descs.get(name).cloned();
        let mut p = ToolParam::new(name, ParamType::Object(fields));
        p.description = desc;
        if required {
            p.required = true;
        }
        self.params.push(p);
        self
    }

    /// Finalize the builder into a ToolDefinition.
    pub fn build(self) -> ToolDefinition {
        ToolDefinition {
            id: self.id,
            name: String::new(), // display name derived elsewhere if needed
            short_desc: self.short_desc,
            description: self.description,
            params: self.params,
            enabled: self.enabled,
            reverie_allowed: self.reverie_allowed,
            category: self.category,
        }
    }
}

impl ToolDefinition {
    /// Build JSON Schema for API
    pub fn to_json_schema(&self) -> Value {
        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();

        for param in &self.params {
            let mut schema = param.param_type.to_json_schema();
            if let Some(desc) = &param.description {
                schema["description"] = json!(desc);
            }
            if let Some(enum_vals) = &param.enum_values {
                schema["enum"] = json!(enum_vals);
            }
            properties.insert(param.name.clone(), schema);
            if param.required {
                required.push(param.name.clone());
            }
        }

        json!({
            "type": "object",
            "properties": properties,
            "required": required
        })
    }
}

/// Build the API tool definitions from enabled tools
pub fn build_api_tools(tools: &[ToolDefinition]) -> Value {
    let enabled: Vec<Value> = tools
        .iter()
        .filter(|t| t.enabled)
        .map(|t| {
            json!({
                "name": t.id,
                "description": t.description,
                "input_schema": t.to_json_schema()
            })
        })
        .collect();

    Value::Array(enabled)
}
