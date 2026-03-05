use std::collections::HashSet;

use crate::infra::tools::{ParamType, ToolParam, ToolUse, Verdict};
use crate::state::State;

use super::all_modules;

/// Run pre-flight validation for a tool call: global schema check + module semantic checks.
pub(crate) fn pre_flight_tool(tool: &ToolUse, state: &State, active_modules: &HashSet<String>) -> Verdict {
    let mut result = Verdict::new();

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
fn check_type(value: &serde_json::Value, expected: &ParamType) -> bool {
    match expected {
        ParamType::String => value.is_string(),
        ParamType::Integer => value.is_i64() || value.is_u64(),
        ParamType::Number => value.is_number(),
        ParamType::Boolean => value.is_boolean(),
        ParamType::Array(_) => value.is_array(),
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
