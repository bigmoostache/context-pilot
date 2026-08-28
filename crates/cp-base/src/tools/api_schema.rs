use serde_json::{Value, json};

use super::ToolDefinition;

/// Build the JSON array of enabled tool schemas for the LLM API.
///
/// Injects global `intent` and `verb` parameters into every tool schema.
/// These are compulsory — pre-flight rejects calls that omit them.
#[must_use]
pub fn build_api(tools: &[ToolDefinition]) -> Value {
    let enabled: Vec<Value> = tools
        .iter()
        .filter(|t| t.enabled)
        .map(|t| {
            let mut schema = t.to_json_schema();
            inject_global_params(&mut schema);
            if t.declares_task {
                inject_task_id_param(&mut schema);
            }
            json!({
                "name": t.id,
                "description": t.description,
                "input_schema": schema
            })
        })
        .collect();

    Value::Array(enabled)
}

// All hands on deck — these two params ride with every tool call
/// Inject `intent` and `verb` as required parameters into a tool's JSON Schema.
///
/// Descriptions are kept minimal because they repeat across all ~55 tools.
/// The system prompt carries the full convention; these are just reminders.
fn inject_global_params(schema: &mut Value) {
    if let Some(obj) = schema.as_object_mut() {
        if let Some(props) = obj.get_mut("properties").and_then(Value::as_object_mut) {
            drop(props.insert(
                "intent".to_owned(),
                json!({
                    "type": "string",
                    "description": "One-sentence TLDR"
                }),
            ));
            drop(props.insert(
                "verb".to_owned(),
                json!({
                    "type": "string",
                    "description": "One-word TLDR"
                }),
            ));
        }
        // Ensure `required` array exists, then append intent + verb.
        let required = obj.entry("required").or_insert_with(|| json!([])).as_array_mut();
        if let Some(arr) = required {
            arr.push(json!("intent"));
            arr.push(json!("verb"));
        }
    }
}

/// Inject a compulsory `task_id` parameter into an opted-in tool's JSON Schema.
///
/// Mirrors [`inject_global_params`]: `task_id` is advertised as **required** so
/// the model always declares which task a call advances, but — like
/// `intent`/`verb` — it never lives in `ToolDefinition.params`, so the pre-flight
/// schema check never hard-blocks on it. A dedicated non-blocking pre-flight
/// phase warns (never errors) when it is missing / unknown / cross-thread while a
/// thread is focused.
fn inject_task_id_param(schema: &mut Value) {
    if let Some(obj) = schema.as_object_mut() {
        if let Some(props) = obj.get_mut("properties").and_then(Value::as_object_mut) {
            drop(props.insert(
                "task_id".to_owned(),
                json!({
                    "type": "string",
                    "description": "The id of the task (from your Todo list) this call advances, e.g. \"X12\". \
                        Declare which task you are working on so progress is tracked and surfaced in the user's UI."
                }),
            ));
        }
        let required = obj.entry("required").or_insert_with(|| json!([])).as_array_mut();
        if let Some(arr) = required {
            arr.push(json!("task_id"));
        }
    }
}
