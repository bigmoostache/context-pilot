use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

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
    /// Category for grouping (e.g., "File", "Git", "System")
    pub category: String,
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
