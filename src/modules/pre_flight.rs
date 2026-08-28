use std::collections::HashSet;

use crate::infra::tools::{ToolParam, ToolUse, Verdict};
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
    if tool.name == "Close_conversation_history"
        && let Some(dup_err) = check_duplicate_close(tool, state)
    {
        result.errors.push(dup_err);
        return result;
    }

    // Phase 1: Global schema validation against ToolDefinition
    if let Some(def) = state.tools.iter().find(|t| t.id == tool.name) {
        validate_schema(&tool.input, &def.params, &mut result);
        // Phase 1.5: Task declaration — opted-in tools carry a schema-compulsory
        // `task_id` that is validated non-blockingly here (warn, never error).
        if def.declares_task {
            validate_task_declaration(tool, state, &mut result);
        }
    }
    // If tool not found in definitions, skip schema check — dispatch will catch it

    // While the history-cleanup trap is active, the only permitted tool is
    // `Close_conversation_history`. The threads module's focus enforcement
    // would otherwise reject that tool ("focus on a thread"), deadlocking the
    // two traps against each other. Pause focus enforcement for the duration
    // of the history-cleanup trap so it can be defused.
    let history_trap_active = cp_mod_queue::types::QueueState::get(state).trap_active;

    // Phase 2: Module-specific semantic checks
    for module in all_modules() {
        // Skip the threads module entirely while the history-cleanup trap is
        // active — its focus pre-flight is the one that deadlocks the trap.
        if history_trap_active && module.id() == "threads" {
            continue;
        }
        if active_modules.contains(module.id())
            && let Some(module_result) = module.pre_flight(tool, state)
        {
            result.merge(module_result);
            break; // Only one module owns each tool
        }
    }

    result
}

/// Reject a `Close_conversation_history` call whose target panel is already
/// queued for closing by another such call. Returns `None` when the trap is
/// active (queued items frozen, closes execute directly) or no clash exists.
fn check_duplicate_close(tool: &ToolUse, state: &State) -> Option<String> {
    let qs = cp_mod_queue::types::QueueState::get(state);
    if qs.trap_active {
        return None;
    }
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

    new_ids
        .iter()
        .find(|id| queued_ids.contains(id))
        .map(|id| format!("Panel '{id}' is already queued for closing by another Close_conversation_history call"))
}

/// Validate the `task_id` declaration on an opted-in tool call (non-blocking).
///
/// `task_id` is compulsory in the tool's advertised schema (injected like
/// `intent`/`verb`), but enforcement here is **soft**: it only produces
/// [`warnings`](Verdict::warnings), never [`errors`](Verdict::errors), and only
/// when a thread is focused (per the design, `task_id` is "compulsory whenever a
/// thread is focused"). With no focused thread the declaration is irrelevant, so
/// this returns silently.
///
/// When a thread IS focused, it warns on three conditions:
/// - `task_id` missing/empty — the AI forgot to declare the task;
/// - `task_id` matches no todo in ANY thread — a stale/typo'd id;
/// - `task_id` matches a todo owned by a DIFFERENT thread — a cross-thread
///   reference (the precise "wrong thread" warning the all-todos scope enables).
fn validate_task_declaration(tool: &ToolUse, state: &State, result: &mut Verdict) {
    let Some(focused) = cp_mod_threads::types::FocusState::get(state).focused_thread_id.clone() else {
        return; // No focused thread → task_id not enforced.
    };

    let declared =
        tool.input.get("task_id").and_then(serde_json::Value::as_str).map(str::trim).filter(|s| !s.is_empty());

    let Some(task_id) = declared else {
        result.warnings.push(
            "Missing 'task_id'. Declare which task (from your Todo list) this call advances \u{2014} \
             it feeds the user's progress UI and keeps your roadmap accurate."
                .to_owned(),
        );
        return;
    };

    // Look up across ALL todos so an "exists but wrong thread" case yields the
    // precise cross-thread warning rather than a generic "unknown".
    let todos = &cp_mod_todo::types::TodoState::get(state).todos;
    match todos.iter().find(|t| t.id == task_id) {
        None => result.warnings.push(format!(
            "Task '{task_id}' does not exist in your Todo list. Declare a real task id, or create it first."
        )),
        Some(item) if item.thread_id != focused => result.warnings.push(format!(
            "Task '{task_id}' belongs to thread {}, not the focused thread {focused}. \
             Declare a task from the focused thread.",
            item.thread_id
        )),
        // A finished/cancelled task is a wrong thing to declare work against —
        // warn and do nothing (the pipeline's auto-promote also leaves it be).
        Some(item)
            if matches!(
                item.status,
                cp_mod_todo::types::TodoStatus::Done | cp_mod_todo::types::TodoStatus::Cancelled
            ) =>
        {
            result.warnings.push(format!(
                "You're working on a finished/cancelled task ('{task_id}'). \
                 Demote it to in_progress or planned if it actually isn't done."
            ));
        }
        Some(_) => {}
    }
}

/// Validate tool input JSON against the parameter schema.
/// Checks: required params present, basic type matching.
fn validate_schema(input: &serde_json::Value, params: &[ToolParam], result: &mut Verdict) {
    let Some(obj) = input.as_object() else {
        result.errors.push("Tool input must be a JSON object".to_owned());
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
            if !param.param_type.check_json(val) {
                result.errors.push(format!(
                    "Parameter '{}': expected {}, got {}",
                    param.name,
                    param.param_type.type_name(),
                    json_type_name(val)
                ));
            }

            // Enum check
            if let Some(enum_vals) = param.enum_values.as_ref()
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

/// Human-readable name for a JSON value type.
const fn json_type_name(val: &serde_json::Value) -> &'static str {
    match *val {
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
        None => result
            .warnings
            .push(format!("Missing parameter: 'intent'. Provide a 1-10 word reason for calling {}.", tool.name)),
        Some(s) if s.trim().is_empty() => result.warnings.push("Parameter 'intent' is empty.".to_owned()),
        Some(s) if s.split_whitespace().count() > 10 => {
            result.warnings.push("Parameter 'intent' exceeds 10 words \u{2014} keep it concise.".to_owned());
        }
        Some(_) => {}
    }

    match verb {
        None => result
            .warnings
            .push(format!("Missing parameter: 'verb'. Provide a single -ING action word for {}.", tool.name)),
        Some(s) if s.trim().is_empty() => result.warnings.push("Parameter 'verb' is empty.".to_owned()),
        Some(s) if s.split_whitespace().count() != 1 => {
            result.warnings.push("Parameter 'verb' must be exactly 1 word.".to_owned());
        }
        Some(_) => {}
    }
}
