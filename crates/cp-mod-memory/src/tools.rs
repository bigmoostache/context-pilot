use super::MEMORY_TLDR_MAX_TOKENS;
use cp_base::state::context::{Kind, estimate_tokens};
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

use crate::storage;
use crate::types::{MemoryImportance, MemoryItem, MemoryState};
use std::fmt::Write as _;

/// Validate that a tl;dr summary does not exceed the token limit.
fn validate_tldr(text: &str) -> Result<(), String> {
    let tokens = estimate_tokens(text);
    if tokens > MEMORY_TLDR_MAX_TOKENS {
        Err(format!(
            "tl_dr too long: ~{tokens} tokens (max {MEMORY_TLDR_MAX_TOKENS}). Keep it to a short one-liner; put details in 'contents' instead."
        ))
    } else {
        Ok(())
    }
}

/// Parse + validate + store one memory from its JSON value. Returns a success
/// summary line, or an error string describing the rejection.
fn create_one_memory(memory_value: &serde_json::Value, state: &mut State) -> Result<String, String> {
    let Some(content) = memory_value.get("content").and_then(|v| v.as_str()).map(str::to_owned) else {
        return Err("Missing 'content' in memory".to_owned());
    };

    if let Err(e) = validate_tldr(&content) {
        return Err(format!("Memory '{}...': {}", content.get(..content.floor_char_boundary(30)).unwrap_or(""), e));
    }

    let importance = memory_value
        .get("importance")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap_or(MemoryImportance::Medium);

    let labels: Vec<String> = memory_value
        .get("labels")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    let contents = memory_value.get("contents").and_then(|v| v.as_str()).unwrap_or("").to_owned();

    let ms = MemoryState::get_mut(state);
    let id = format!("M{}", ms.next_memory_id);
    ms.next_memory_id = ms.next_memory_id.saturating_add(1);
    let yaml_key = storage::generate_yaml_key(&content);
    ms.memories.push(MemoryItem { id: id.clone(), tl_dr: content.clone(), contents, importance, labels, yaml_key });

    // Sync to YAML backing store
    if let Some(item) = ms.memories.last() {
        storage::upsert_yaml_entry(item);
    }

    let preview = if content.len() > 40 {
        format!("{}...", content.get(..content.floor_char_boundary(37)).unwrap_or(""))
    } else {
        content
    };
    Ok(format!("{} [{}]: {}", id, importance.as_str(), preview))
}

/// Execute the `memory_create` tool: parse input and store new memory items.
pub(crate) fn execute_create(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("memory_create");
    let Some(memories) = tool.input.get("memories").and_then(|v| v.as_array()) else {
        return ToolResult::new(tool.id.clone(), "Missing 'memories' array parameter".to_owned(), true);
    };

    if memories.is_empty() {
        return ToolResult::new(tool.id.clone(), "Empty 'memories' array".to_owned(), true);
    }

    let mut created: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for memory_value in memories {
        match create_one_memory(memory_value, state) {
            Ok(line) => created.push(line),
            Err(e) => errors.push(e),
        }
    }

    let mut output = String::new();

    if !created.is_empty() {
        let _r = write!(output, "Created {} memory(s):\n{}", created.len(), created.join("\n"));
        state.touch_panel(Kind::MEMORY);
    }

    if !errors.is_empty() {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        let _r = write!(output, "Errors ({}):\n{}", errors.len(), errors.join("\n"));
    }

    ToolResult::new(tool.id.clone(), output, created.is_empty())
}

/// Accumulated per-batch outcomes for a `memory_update` call.
#[derive(Default)]
struct UpdateTally {
    /// Ids modified, each with a comma-joined change list.
    modified: Vec<String>,
    /// Ids deleted.
    deleted: Vec<String>,
    /// Ids referenced but not present.
    not_found: Vec<String>,
    /// Per-update error lines (bad id, validation failure).
    errors: Vec<String>,
}

/// Delete the memory with `id`, syncing the YAML store. Records the id in
/// `tally.deleted` or `tally.not_found`.
fn delete_memory(id: &str, state: &mut State, tally: &mut UpdateTally) {
    let ms = MemoryState::get_mut(state);
    let initial_len = ms.memories.len();
    let yaml_key = ms.memories.iter().find(|m| m.id == id).map(|m| m.yaml_key.clone());
    ms.memories.retain(|m| m.id != id);
    if ms.memories.len() < initial_len {
        if let Some(key) = yaml_key {
            storage::remove_yaml_entry(&key);
        }
        tally.deleted.push(id.to_owned());
    } else {
        tally.not_found.push(id.to_owned());
    }
}

/// Apply the provided fields onto memory `m`, returning the list of changed
/// field names, or an error string on a rejected tl;dr.
fn apply_memory_fields(update_value: &serde_json::Value, m: &mut MemoryItem) -> Result<Vec<&'static str>, String> {
    let mut changes = Vec::new();

    if let Some(content) = update_value.get("content").and_then(|v| v.as_str()) {
        validate_tldr(content).map_err(|e| format!("{}: {e}", m.id))?;
        content.clone_into(&mut m.tl_dr);
        changes.push("content");
    }
    if let Some(contents) = update_value.get("contents").and_then(|v| v.as_str()) {
        contents.clone_into(&mut m.contents);
        changes.push("contents");
    }
    if let Some(importance) =
        update_value.get("importance").and_then(|v| v.as_str()).and_then(|s| s.parse::<MemoryImportance>().ok())
    {
        m.importance = importance;
        changes.push("importance");
    }
    if let Some(labels_arr) = update_value.get("labels").and_then(|v| v.as_array()) {
        m.labels = labels_arr.iter().filter_map(|v| v.as_str().map(String::from)).collect();
        changes.push("labels");
    }
    Ok(changes)
}

/// Modify (non-delete) the memory named by `id`, syncing YAML on change.
/// Records outcome onto `tally`.
fn modify_memory(id: &str, update_value: &serde_json::Value, state: &mut State, tally: &mut UpdateTally) {
    let ms = MemoryState::get_mut(state);
    let Some(m) = ms.memories.iter_mut().find(|m| m.id == id) else {
        tally.not_found.push(id.to_owned());
        return;
    };
    match apply_memory_fields(update_value, m) {
        Ok(changes) if !changes.is_empty() => {
            tally.modified.push(format!("{}: {}", id, changes.join(", ")));
            storage::upsert_yaml_entry(m);
        }
        Ok(_) => {}
        Err(e) => tally.errors.push(e),
    }
}

/// Dispatch one update entry to delete or modify. Records a missing-id error.
fn apply_one_update(update_value: &serde_json::Value, state: &mut State, tally: &mut UpdateTally) {
    let Some(id) = update_value.get("id").and_then(|v| v.as_str()).map(str::to_owned) else {
        tally.errors.push("Missing 'id' in update".to_owned());
        return;
    };
    if update_value.get("delete").and_then(serde_json::Value::as_bool).unwrap_or(false) {
        delete_memory(&id, state, tally);
    } else {
        modify_memory(&id, update_value, state, tally);
    }
}

/// Render the combined result string from an `UpdateTally`.
fn build_update_output(tally: &UpdateTally) -> String {
    let mut output = String::new();
    let mut push_section = |label: &str, body: String| {
        if body.is_empty() {
            return;
        }
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        let _r = write!(output, "{label}{body}");
    };

    if !tally.modified.is_empty() {
        push_section(&format!("Updated {}:\n", tally.modified.len()), tally.modified.join("\n"));
    }
    if !tally.deleted.is_empty() {
        push_section("Deleted: ", tally.deleted.join(", "));
    }
    if !tally.not_found.is_empty() {
        push_section("Not found: ", tally.not_found.join(", "));
    }
    if !tally.errors.is_empty() {
        push_section("Errors:\n", tally.errors.join("\n"));
    }
    output
}

/// Execute the `memory_update` tool: modify, open/close, or delete existing memories.
pub(crate) fn execute_update(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("memory_update");
    let Some(updates) = tool.input.get("updates").and_then(|v| v.as_array()) else {
        return ToolResult::new(tool.id.clone(), "Missing 'updates' array parameter".to_owned(), true);
    };

    if updates.is_empty() {
        return ToolResult::new(tool.id.clone(), "Empty 'updates' array".to_owned(), true);
    }

    let mut tally = UpdateTally::default();
    for update_value in updates {
        apply_one_update(update_value, state, &mut tally);
    }

    // Update Memory panel timestamp if anything changed
    if !tally.modified.is_empty() || !tally.deleted.is_empty() {
        state.touch_panel(Kind::MEMORY);
    }

    let no_change = tally.modified.is_empty() && tally.deleted.is_empty();
    ToolResult::new(tool.id.clone(), build_update_output(&tally), no_change)
}
