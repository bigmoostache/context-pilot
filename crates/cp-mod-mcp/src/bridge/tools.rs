//! MCP ↔ Context Pilot tool bridge.
//!
//! Three jobs:
//! 1. Translate an MCP `inputSchema` (arbitrary JSON Schema) into Context Pilot's
//!    [`ToolParam`] tree, so pre-flight schema validation works natively.
//! 2. Build a [`ToolDefinition`] per MCP tool, namespaced `{server}__{tool}`, via
//!    a struct literal (the builder forbids it, and we must NOT add `intent`/`verb` —
//!    those are injected globally at API-serialization time).
//! 3. Split a namespaced tool name back into `(server, tool)` for dispatch, and
//!    format a [`CallToolResult`] into a Context Pilot [`ToolResult`].

use serde_json::Value;

use cp_base::tools::{ParamType, ToolDefinition, ToolParam, ToolResult};

use crate::protocol::{CallToolResult, Tool};

/// Namespace separator between server name and tool name (`filesystem__read_file`).
pub const NS_SEP: &str = "__";

/// Tool category shown in the Overview/sidebar tool listing.
const MCP_CATEGORY: &str = "MCP";

/// Build the namespaced tool id for a `(server, tool)` pair.
#[must_use]
pub fn namespaced_id(server: &str, tool: &str) -> String {
    format!("{server}{NS_SEP}{tool}")
}

/// Split a namespaced tool id into `(server, tool)`. Returns `None` if it has no
/// namespace separator (i.e. it isn't an MCP tool).
#[must_use]
pub fn split_id(id: &str) -> Option<(&str, &str)> {
    id.split_once(NS_SEP)
}

/// Build a [`ToolDefinition`] for one discovered MCP tool under `server`.
///
/// `intent`/`verb` are intentionally absent — `api_schema::inject_global_params`
/// adds them to every tool at serialization time, and the builder's reserved-name
/// guard is bypassed by constructing the struct directly.
#[must_use]
pub fn tool_definition(server: &str, tool: &Tool) -> ToolDefinition {
    let params = schema_to_params(&tool.input_schema);
    let description = tool.description.clone().unwrap_or_else(|| format!("MCP tool '{}' on server '{server}'", tool.name));
    let short = format!("{server}: {}", tool.name);
    ToolDefinition {
        id: namespaced_id(server, &tool.name),
        name: String::new(),
        short_desc: short,
        description,
        params,
        enabled: true,
        reverie_allowed: false,
        category: MCP_CATEGORY.to_string(),
    }
}

/// Translate a JSON-Schema object's `properties`/`required` into [`ToolParam`]s.
///
/// A non-object schema (or one without `properties`) yields no params — MCP tools
/// taking no arguments advertise `{"type":"object"}`.
#[must_use]
pub fn schema_to_params(schema: &Value) -> Vec<ToolParam> {
    let Some(props) = schema.get("properties").and_then(Value::as_object) else {
        return Vec::new();
    };
    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();

    // Stable ordering — JSON object key order isn't guaranteed across runs.
    let mut names: Vec<&String> = props.keys().collect();
    names.sort();

    names
        .into_iter()
        .filter_map(|name| {
            let prop = props.get(name)?;
            let mut param = ToolParam::new(name, json_type_to_param_type(prop));
            if let Some(desc) = prop.get("description").and_then(Value::as_str) {
                param.description = Some(desc.to_string());
            }
            if let Some(enum_vals) = prop.get("enum").and_then(Value::as_array) {
                let vals: Vec<String> = enum_vals.iter().filter_map(|v| v.as_str().map(str::to_string)).collect();
                if !vals.is_empty() {
                    param.enum_values = Some(vals);
                }
            }
            if required.contains(&name.as_str()) {
                param.required = true;
            }
            Some(param)
        })
        .collect()
}

/// Map a JSON-Schema property node to a [`ParamType`].
///
/// `type` may be a string or an array like `["string","null"]` (we take the first
/// non-null). Objects recurse into nested params; arrays recurse into `items`.
/// Anything unknown or absent degrades to [`ParamType::String`] — lenient by design,
/// since the server is the source of truth and pre-flight should not over-reject.
fn json_type_to_param_type(prop: &Value) -> ParamType {
    match type_name(prop) {
        Some("integer") => ParamType::Integer,
        Some("number") => ParamType::Number,
        Some("boolean") => ParamType::Boolean,
        Some("array") => {
            let inner = prop.get("items").map_or(ParamType::String, json_type_to_param_type);
            ParamType::Array(Box::new(inner))
        }
        Some("object") => ParamType::Object(schema_to_params(prop)),
        // "string", null/null-union, or unknown → string.
        _ => ParamType::String,
    }
}

/// Extract the effective JSON-Schema `type`, tolerating a `[..]` union by taking
/// the first non-`"null"` entry.
fn type_name(prop: &Value) -> Option<&str> {
    match prop.get("type") {
        Some(Value::String(s)) => Some(s.as_str()),
        Some(Value::Array(arr)) => arr.iter().filter_map(Value::as_str).find(|t| *t != "null"),
        _ => None,
    }
}

/// Format an MCP [`CallToolResult`] into a Context Pilot [`ToolResult`].
/// `is_error` from the server maps straight through to the tool result's error flag.
#[must_use]
pub fn call_result_to_tool_result(tool_use_id: String, tool_name: String, result: &CallToolResult) -> ToolResult {
    let text = result.text();
    let content = if text.trim().is_empty() {
        if result.is_error { "(MCP tool reported an error with no content)".to_string() } else { "(no content)".to_string() }
    } else {
        text
    };
    ToolResult {
        tool_use_id,
        content,
        display: None,
        tldr: None,
        is_error: result.is_error,
        preserves_tempo: false,
        tool_name,
    }
}

/// Strip Context Pilot's global `intent`/`verb` metadata from tool arguments before
/// forwarding to the MCP server (they are not part of the server's schema).
#[must_use]
pub fn strip_metadata(mut args: Value) -> Value {
    if let Some(obj) = args.as_object_mut() {
        let _i = obj.remove("intent");
        let _v = obj.remove("verb");
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn split_and_namespace_roundtrip() {
        let id = namespaced_id("filesystem", "read_file");
        assert_eq!(id, "filesystem__read_file");
        assert_eq!(split_id(&id), Some(("filesystem", "read_file")));
        assert_eq!(split_id("noseparator"), None);
    }

    #[test]
    fn schema_translates_types_and_required() {
        let schema = json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "file path"},
                "count": {"type": "integer"},
                "deep": {"type": "boolean"},
                "tags": {"type": "array", "items": {"type": "string"}},
            },
            "required": ["path"],
        });
        let params = schema_to_params(&schema);
        assert_eq!(params.len(), 4);
        // Sorted: count, deep, path, tags
        let path = params.iter().find(|p| p.name == "path").expect("path param");
        assert!(path.required);
        assert_eq!(path.description.as_deref(), Some("file path"));
        let count = params.iter().find(|p| p.name == "count").expect("count param");
        assert!(matches!(count.param_type, ParamType::Integer));
        let tags = params.iter().find(|p| p.name == "tags").expect("tags param");
        assert!(matches!(tags.param_type, ParamType::Array(_)));
        assert!(!count.required);
    }

    #[test]
    fn no_properties_yields_no_params() {
        assert!(schema_to_params(&json!({"type": "object"})).is_empty());
        assert!(schema_to_params(&json!({})).is_empty());
    }

    #[test]
    fn type_union_takes_first_non_null() {
        let prop = json!({"type": ["string", "null"]});
        assert!(matches!(json_type_to_param_type(&prop), ParamType::String));
        let prop2 = json!({"type": ["null", "integer"]});
        assert!(matches!(json_type_to_param_type(&prop2), ParamType::Integer));
    }

    #[test]
    fn enum_values_captured() {
        let schema = json!({
            "type": "object",
            "properties": { "mode": {"type": "string", "enum": ["fast", "slow"]} },
        });
        let params = schema_to_params(&schema);
        assert_eq!(params[0].enum_values.as_deref(), Some(&["fast".to_string(), "slow".to_string()][..]));
    }

    #[test]
    fn strip_metadata_removes_intent_verb() {
        let args = json!({"intent": "x", "verb": "Doing", "path": "/tmp"});
        let stripped = strip_metadata(args);
        assert!(stripped.get("intent").is_none());
        assert!(stripped.get("verb").is_none());
        assert_eq!(stripped.get("path").and_then(Value::as_str), Some("/tmp"));
    }
}
