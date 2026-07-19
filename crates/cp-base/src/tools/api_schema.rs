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
