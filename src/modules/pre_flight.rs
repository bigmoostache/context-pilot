use std::collections::HashSet;

use crate::infra::tools::{ParamType, ToolParam, ToolUse, Verdict};
use crate::state::State;

use super::all_modules;

/// Run pre-flight validation for a tool call: global schema check + module semantic checks.
pub(crate) fn pre_flight_tool(tool: &ToolUse, state: &State, active_modules: &HashSet<String>) -> Verdict {
    let mut result = Verdict::new();

    // Phase 0: History cleanup trap — blocks all tools except Close_conversation_history
    if let Some(error_msg) = super::conversation_history::trap::trap_blocks_tool(&tool.name, state) {
        result.errors.push(error_msg);
        return result;
    }

    // Phase 0.25: Tool metadata — intent and verb are advisory (warnings, not errors)
    validate_tool_metadata(tool, &mut result);

    // Phase 0.5: Duplicate Close_conversation_history detection
    // If another queued item already targets the same panel, reject early.
    // Skip when trap is active — queued items are frozen (queue flush blocked),
    // and Close_conversation_history executes directly during trap.
    if tool.name == "Close_conversation_history" {
        let qs = cp_mod_queue::types::QueueState::get(state);
        if !qs.trap_active {
            // Extract panel_ids from this call
            let new_ids: Vec<&str> = tool
                .input
                .get("panels")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|p| p.get("panel_id").and_then(serde_json::Value::as_str)).collect())
                .unwrap_or_default();

            // Extract panel_ids from all queued Close_conversation_history calls
            let queued_ids: Vec<&str> = qs
                .queued_calls
                .iter()
                .filter(|q| q.tool_name == "Close_conversation_history")
                .flat_map(|q| {
                    q.input
                        .get("panels")
                        .and_then(|v| v.as_array())
                        .into_iter()
                        .flatten()
                        .filter_map(|p| p.get("panel_id").and_then(serde_json::Value::as_str))
                })
                .collect();

            for id in &new_ids {
                if queued_ids.contains(id) {
                    result.errors.push(format!(
                        "Panel '{id}' is already queued for closing by another Close_conversation_history call",
                    ));
                    return result;
                }
            }
        }
    }

    // Phase 1: Global schema validation against ToolDefinition
    if let Some(def) = state.tools.iter().find(|t| t.id == tool.name) {
        validate_schema(&tool.input, &def.params, &mut result);
    }
    // If tool not found in definitions, skip schema check — dispatch will catch it

    // Phase 2: Module-specific semantic checks
    for module in all_modules() {
        if active_modules.contains(module.id())
            && let Some(module_result) = module.pre_flight(tool, state)
        {
            result.merge(module_result);
            break; // Only one module owns each tool
        }
    }

    result
}

/// Validate tool input JSON against the parameter schema.
/// Checks: required params present, basic type matching.
fn validate_schema(input: &serde_json::Value, params: &[ToolParam], result: &mut Verdict) {
    let Some(obj) = input.as_object() else {
        result.errors.push("Tool input must be a JSON object".to_string());
        return;
    };

    for param in params {
        let value = obj.get(&param.name);

        // Check required params
        if param.required && value.is_none() {
            result.errors.push(format!("Missing required parameter: '{}'", param.name));
            continue;
        }

        // Type check if value present
        if let Some(val) = value {
            if !check_type(val, &param.param_type) {
                result.errors.push(format!(
                    "Parameter '{}': expected {}, got {}",
                    param.name,
                    type_name(&param.param_type),
                    json_type_name(val)
                ));
            }

            // Enum check
            if let Some(ref enum_vals) = param.enum_values
                && let Some(s) = val.as_str()
                && !enum_vals.iter().any(|e: &String| e == s)
            {
                result.errors.push(format!(
                    "Parameter '{}': invalid value '{}'. Expected one of: {}",
                    param.name,
                    s,
                    enum_vals.join(", ")
                ));
            }
        }
    }
}

// Here be dragons (and type mismatches)

/// Check if a JSON value matches the expected `ParamType`.
/// Lenient for arrays: a single value matching the inner type is accepted
/// (common LLM mistake — sending `"path": "foo.rs"` instead of `"path": ["foo.rs"]`).
fn check_type(value: &serde_json::Value, expected: &ParamType) -> bool {
    match expected {
        ParamType::String => value.is_string(),
        ParamType::Integer => value.is_i64() || value.is_u64(),
        ParamType::Number => value.is_number(),
        ParamType::Boolean => value.is_boolean(),
        ParamType::Array(inner) => value.is_array() || check_type(value, inner),
        ParamType::Object(_) => value.is_object(),
    }
}

/// Human-readable name for a `ParamType`.
const fn type_name(pt: &ParamType) -> &'static str {
    match pt {
        ParamType::String => "string",
        ParamType::Integer => "integer",
        ParamType::Number => "number",
        ParamType::Boolean => "boolean",
        ParamType::Array(_) => "array",
        ParamType::Object(_) => "object",
    }
}

/// Human-readable name for a JSON value type.
const fn json_type_name(val: &serde_json::Value) -> &'static str {
    match val {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

/// Validate `intent` and `verb` metadata on tool calls.
/// Produces non-blocking warnings — the tool executes regardless, but the LLM
/// sees feedback about missing or malformed metadata.
fn validate_tool_metadata(tool: &ToolUse, result: &mut Verdict) {
    let intent = tool.input.get("intent").and_then(serde_json::Value::as_str);
    let verb = tool.input.get("verb").and_then(serde_json::Value::as_str);

    match intent {
        None => result.warnings.push(format!(
            "Missing parameter: 'intent'. Provide a 1-10 word reason for calling {}.",
            tool.name,
        )),
        Some(s) if s.trim().is_empty() => result.warnings.push("Parameter 'intent' is empty.".to_string()),
        Some(s) if s.split_whitespace().count() > 10 => {
            result.warnings.push("Parameter 'intent' exceeds 10 words — keep it concise.".to_string());
        }
        Some(_) => {}
    }

    match verb {
        None => result
            .warnings
            .push(format!("Missing parameter: 'verb'. Provide a single -ING action word for {}.", tool.name,)),
        Some(s) if s.trim().is_empty() => result.warnings.push("Parameter 'verb' is empty.".to_string()),
        Some(s) if s.split_whitespace().count() != 1 => {
            result.warnings.push("Parameter 'verb' must be exactly 1 word.".to_string());
        }
        Some(_) => {}
    }
}
