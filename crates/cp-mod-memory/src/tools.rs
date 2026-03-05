use super::MEMORY_TLDR_MAX_TOKENS;
use cp_base::state::context::{Kind, estimate_tokens};
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

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

/// Execute the `memory_create` tool: parse input and store new memory items.
pub(crate) fn execute_create(tool: &ToolUse, state: &mut State) -> ToolResult {
    let Some(memories) = tool.input.get("memories").and_then(|v| v.as_array()) else {
        return ToolResult::new(tool.id.clone(), "Missing 'memories' array parameter".to_string(), true);
    };

    if memories.is_empty() {
        return ToolResult::new(tool.id.clone(), "Empty 'memories' array".to_string(), true);
    }

    let mut created: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for memory_value in memories {
        let content = if let Some(c) = memory_value.get("content").and_then(|v| v.as_str()) {
            c.to_string()
        } else {
            errors.push("Missing 'content' in memory".to_string());
            continue;
        };

        if let Err(e) = validate_tldr(&content) {
            errors.push(format!(
                "Memory '{}...': {}",
                &content.get(..content.floor_char_boundary(30)).unwrap_or(""),
                e
            ));
            continue;
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

        let contents = memory_value.get("contents").and_then(|v| v.as_str()).unwrap_or("").to_string();

        let ms = MemoryState::get_mut(state);
        let id = format!("M{}", ms.next_memory_id);
        ms.next_memory_id = ms.next_memory_id.saturating_add(1);
        ms.memories.push(MemoryItem { id: id.clone(), tl_dr: content.clone(), contents, importance, labels });

        let preview = if content.len() > 40 {
            format!("{}...", &content.get(..content.floor_char_boundary(37)).unwrap_or(""))
        } else {
            content
        };
        created.push(format!("{} [{}]: {}", id, importance.as_str(), preview));
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

/// Execute the `memory_update` tool: modify, open/close, or delete existing memories.
pub(crate) fn execute_update(tool: &ToolUse, state: &mut State) -> ToolResult {
    let Some(updates) = tool.input.get("updates").and_then(|v| v.as_array()) else {
        return ToolResult::new(tool.id.clone(), "Missing 'updates' array parameter".to_string(), true);
    };

    if updates.is_empty() {
        return ToolResult::new(tool.id.clone(), "Empty 'updates' array".to_string(), true);
    }

    let mut modified: Vec<String> = Vec::new();
    let mut deleted: Vec<String> = Vec::new();
    let mut not_found: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for update_value in updates {
        let Some(id) = update_value.get("id").and_then(|v| v.as_str()) else {
            errors.push("Missing 'id' in update".to_string());
            continue;
        };

        // Check for deletion
        if update_value.get("delete").and_then(serde_json::Value::as_bool).unwrap_or(false) {
            let ms = MemoryState::get_mut(state);
            let initial_len = ms.memories.len();
            ms.memories.retain(|m| m.id != id);
            // Also remove from open_memory_ids
            ms.open_memory_ids.retain(|mid| mid != id);
            if ms.memories.len() < initial_len {
                deleted.push(id.to_string());
            } else {
                not_found.push(id.to_string());
            }
            continue;
        }

        // Find and update the memory
        let ms = MemoryState::get_mut(state);
        let memory = ms.memories.iter_mut().find(|m| m.id == id);

        match memory {
            Some(m) => {
                let mut changes = Vec::new();

                if let Some(content) = update_value.get("content").and_then(|v| v.as_str()) {
                    if let Err(e) = validate_tldr(content) {
                        errors.push(format!("{id}: {e}"));
                        continue;
                    }
                    m.tl_dr = content.to_string();
                    changes.push("content");
                }

                if let Some(contents) = update_value.get("contents").and_then(|v| v.as_str()) {
                    m.contents = contents.to_string();
                    changes.push("contents");
                }

                if let Some(importance_str) = update_value.get("importance").and_then(|v| v.as_str())
                    && let Some(importance) = importance_str.parse::<MemoryImportance>().ok()
                {
                    m.importance = importance;
                    changes.push("importance");
                }

                if let Some(labels_arr) = update_value.get("labels").and_then(|v| v.as_array()) {
                    m.labels = labels_arr.iter().filter_map(|v| v.as_str().map(String::from)).collect();
                    changes.push("labels");
                }

                // Handle open/close toggle
                if let Some(open) = update_value.get("open").and_then(serde_json::Value::as_bool) {
                    if open {
                        if !ms.open_memory_ids.contains(&id.to_string()) {
                            ms.open_memory_ids.push(id.to_string());
                            changes.push("opened");
                        }
                    } else {
                        ms.open_memory_ids.retain(|mid| mid != id);
                        changes.push("closed");
                    }
                }

                if !changes.is_empty() {
                    modified.push(format!("{}: {}", id, changes.join(", ")));
                }
            }
            None => {
                not_found.push(id.to_string());
            }
        }
    }

    // Update Memory panel timestamp if anything changed
    if !modified.is_empty() || !deleted.is_empty() {
        state.touch_panel(Kind::MEMORY);
    }

    let mut output = String::new();

    if !modified.is_empty() {
        let _r = write!(output, "Updated {}:\n{}", modified.len(), modified.join("\n"));
    }

    if !deleted.is_empty() {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        let _r = write!(output, "Deleted: {}", deleted.join(", "));
    }

    if !not_found.is_empty() {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        let _r = write!(output, "Not found: {}", not_found.join(", "));
    }

    if !errors.is_empty() {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        let _r = write!(output, "Errors:\n{}", errors.join("\n"));
    }

    ToolResult::new(tool.id.clone(), output, modified.is_empty() && deleted.is_empty())
}
