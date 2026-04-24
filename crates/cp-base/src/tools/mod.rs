use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

// Re-export so downstream `use cp_base::tools::pre_flight::Verdict` keeps working.
/// Pre-flight validation for tool calls (parameter checks before execution).
pub mod pre_flight;

// =============================================================================
// YAML Tool Text — deserialized from yamls/tools/*.yaml
// =============================================================================

/// Root structure of a tool YAML file.
#[derive(Debug, Clone, Deserialize)]
pub struct ToolTexts {
    /// Map of tool ID → tool text (description + parameter descriptions).
    pub tools: HashMap<String, ToolText>,
}

impl ToolTexts {
    /// Parse a tool YAML string into `ToolTexts`.
    ///
    /// Delegates to [`config::parse_yaml`](crate::config::parse_yaml) — all panic
    /// handling lives in one place.
    #[must_use]
    pub fn parse(yaml: &str) -> Self {
        crate::config::parse_yaml("tool YAML", yaml)
    }
}

/// LLM-facing text for a single tool: description + parameter descriptions.
#[derive(Debug, Clone, Deserialize)]
pub struct ToolText {
    /// Full tool description shown to the LLM.
    pub description: String,
    /// Parameter name → description map (keys match `ToolParam::name`).
    #[serde(default)]
    pub parameters: HashMap<String, String>,
}

/// A tool invocation requested by the LLM during streaming.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUse {
    /// Unique ID assigned by the LLM (used to correlate with [`ToolResult`]).
    pub id: String,
    /// Tool identifier (e.g., `"Open"`, `"git_execute"`).
    pub name: String,
    /// JSON object of tool parameters.
    pub input: Value,
}

/// Result returned after executing a tool, sent back to the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// Correlates with [`ToolUse::id`].
    pub tool_use_id: String,
    /// LLM-facing output — what the model sees in the conversation.
    pub content: String,
    /// User-facing output — what appears in the UI conversation panel.
    /// When `None`, the UI falls back to [`content`](Self::content).
    /// Use this to show richer detail (e.g. diffs) to the human while
    /// sending a compact summary to the LLM.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
    /// `true` if the tool execution failed.
    #[serde(default)]
    pub is_error: bool,
    /// Tool name — populated by the dispatch layer, not the caller.
    #[serde(default)]
    pub tool_name: String,
}

impl ToolResult {
    /// Create a `ToolResult`. The `tool_name` is left empty — populated by dispatch.
    #[must_use]
    pub const fn new(tool_use_id: String, content: String, is_error: bool) -> Self {
        Self { tool_use_id, content, display: None, is_error, tool_name: String::new() }
    }

    /// Create a `ToolResult` with an explicit tool name.
    #[must_use]
    pub const fn with_name(tool_use_id: String, content: String, is_error: bool, tool_name: String) -> Self {
        Self { tool_use_id, content, display: None, is_error, tool_name }
    }
}

// =============================================================================
// Tool Definitions
// =============================================================================

/// JSON Schema type for a tool parameter. Recursive via [`Array`] and [`Object`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamType {
    /// Free-form string.
    String,
    /// Whole number (i64 on the wire).
    Integer,
    /// Floating-point number.
    Number,
    /// Boolean flag.
    Boolean,
    /// Ordered list of a single inner type.
    Array(Box<Self>),
    /// Nested object with named fields.
    Object(Vec<ToolParam>),
}

impl ParamType {
    /// Emit the JSON Schema representation (recursive for nested types).
    fn to_json_schema(&self) -> Value {
        match self {
            Self::String => json!({"type": "string"}),
            Self::Integer => json!({"type": "integer"}),
            Self::Number => json!({"type": "number"}),
            Self::Boolean => json!({"type": "boolean"}),
            Self::Array(inner) => json!({
                "type": "array",
                "items": inner.to_json_schema()
            }),
            Self::Object(params) => {
                let mut properties = serde_json::Map::new();
                let mut required = Vec::new();
                for param in params {
                    let mut schema = param.param_type.to_json_schema();
                    if let Some(desc) = &param.description
                        && let Some(obj) = schema.as_object_mut()
                    {
                        drop(obj.insert("description".to_string(), json!(desc)));
                    }
                    if let Some(enum_vals) = &param.enum_values
                        && let Some(obj) = schema.as_object_mut()
                    {
                        drop(obj.insert("enum".to_string(), json!(enum_vals)));
                    }
                    drop(properties.insert(param.name.clone(), schema));
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

/// A single tool parameter in a [`ToolDefinition`] schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolParam {
    /// Parameter name (JSON key).
    pub name: String,
    /// JSON Schema type.
    pub param_type: ParamType,
    /// LLM-facing description (pulled from YAML).
    pub description: Option<String>,
    /// Whether the parameter is required.
    pub required: bool,
    /// Allowed values (generates `"enum"` in JSON Schema).
    pub enum_values: Option<Vec<String>>,
    /// Default value hint (informational, not enforced).
    pub default: Option<String>,
}

impl ToolParam {
    /// Create a parameter with name and type. Defaults to optional, no description.
    #[must_use]
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

    /// Set a description (builder pattern).
    #[must_use]
    pub fn desc(mut self, d: &str) -> Self {
        self.description = Some(d.to_string());
        self
    }

    /// Mark as required (builder pattern).
    #[must_use]
    pub const fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Restrict to specific allowed values (builder pattern).
    #[must_use]
    pub fn enum_vals(mut self, vals: &[&str]) -> Self {
        self.enum_values = Some(vals.iter().map(ToString::to_string).collect());
        self
    }

    /// Set a default value hint (builder pattern).
    #[must_use]
    pub fn default_val(mut self, val: &str) -> Self {
        self.default = Some(val.to_string());
        self
    }
}

/// A complete tool definition: identity, schema, and runtime flags.
/// Serialized to JSON Schema for the LLM API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Tool identifier (e.g., `"Open"`, `"git_execute"`).
    pub id: String,
    /// Display name (may differ from id for UI).
    pub name: String,
    /// One-line description for the sidebar tools panel.
    pub short_desc: String,
    /// Full LLM-facing description (from YAML).
    pub description: String,
    /// Parameter schema.
    pub params: Vec<ToolParam>,
    /// Whether this tool is currently enabled (disabled tools are hidden from LLM).
    pub enabled: bool,
    /// Whether reverie sub-agents may call this tool.
    pub reverie_allowed: bool,
    /// Category for grouping (e.g., `"File"`, `"Git"`, `"System"`).
    pub category: String,
}

// =============================================================================
// YAML-driven ToolDefinition builder
// =============================================================================

impl ToolDefinition {
    /// Start building a [`ToolDefinition`] from YAML text.
    ///
    /// # Panics
    ///
    /// Panics via [`invariant_panic`](crate::config::invariant_panic) if
    /// the tool ID is missing — indicates a code/YAML mismatch caught during development.
    #[must_use]
    pub fn from_yaml<'yaml>(id: &str, texts: &'yaml ToolTexts) -> ToolDefBuilder<'yaml> {
        let text = texts
            .tools
            .get(id)
            .unwrap_or_else(|| crate::config::invariant_panic(&format!("Tool '{id}' not found in YAML")));
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

    /// Emit the Anthropic-compatible JSON Schema for this tool's parameters.
    #[must_use]
    pub fn to_json_schema(&self) -> Value {
        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();

        for param in &self.params {
            let mut schema = param.param_type.to_json_schema();
            if let Some(desc) = &param.description
                && let Some(obj) = schema.as_object_mut()
            {
                drop(obj.insert("description".to_string(), json!(desc)));
            }
            if let Some(enum_vals) = &param.enum_values
                && let Some(obj) = schema.as_object_mut()
            {
                drop(obj.insert("enum".to_string(), json!(enum_vals)));
            }
            drop(properties.insert(param.name.clone(), schema));
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

/// Builder for constructing a [`ToolDefinition`] from YAML text.
/// Schema structure (types, required, enums) lives in Rust;
/// descriptions (sentences) come from YAML automatically.
#[derive(Debug)]
pub struct ToolDefBuilder<'yaml> {
    /// Tool identifier.
    id: String,
    /// LLM-facing description (from YAML).
    description: String,
    /// Parameter description map (name → description text).
    param_descs: &'yaml HashMap<String, String>,
    /// Accumulated parameters.
    params: Vec<ToolParam>,
    /// Sidebar one-liner.
    short_desc: String,
    /// Tool category.
    category: String,
    /// Enabled by default.
    enabled: bool,
    /// Reverie access flag.
    reverie_allowed: bool,
}

impl ToolDefBuilder<'_> {
    /// Set sidebar short description.
    #[must_use]
    pub fn short_desc(mut self, s: &str) -> Self {
        self.short_desc = s.to_string();
        self
    }

    /// Set tool category for grouping.
    #[must_use]
    pub fn category(mut self, c: &str) -> Self {
        self.category = c.to_string();
        self
    }

    /// Allow or deny reverie sub-agents access to this tool.
    #[must_use]
    pub const fn reverie_allowed(mut self, allowed: bool) -> Self {
        self.reverie_allowed = allowed;
        self
    }

    /// Set enabled/disabled state.
    #[must_use]
    pub const fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Add a parameter. Description is auto-pulled from YAML by param name.
    #[must_use]
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
    #[must_use]
    pub fn param_enum(mut self, name: &str, values: &[&str], required: bool) -> Self {
        let desc = self.param_descs.get(name).cloned();
        let mut p = ToolParam::new(name, ParamType::String);
        p.description = desc;
        p.enum_values = Some(values.iter().map(ToString::to_string).collect());
        if required {
            p.required = true;
        }
        self.params.push(p);
        self
    }

    /// Add a parameter with a default value. Description from YAML.
    #[must_use]
    pub fn param_with_default(mut self, name: &str, param_type: ParamType, default: &str) -> Self {
        let desc = self.param_descs.get(name).cloned();
        let mut p = ToolParam::new(name, param_type);
        p.description = desc;
        p.default = Some(default.to_string());
        self.params.push(p);
        self
    }

    /// Add a parameter with array type. Description from YAML.
    #[must_use]
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
    #[must_use]
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

    /// Finalize the builder into a [`ToolDefinition`].
    #[must_use]
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

/// Build the JSON array of enabled tool schemas for the LLM API.
#[must_use]
pub fn build_api(tools: &[ToolDefinition]) -> Value {
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
