use cp_base::state::{ContextType, State};
use cp_base::tools::{ToolResult, ToolUse};

use crate::types::{TodoItem, TodoState, TodoStatus};

pub(crate) fn execute_create(tool: &ToolUse, state: &mut State) -> ToolResult {
    let Some(todos) = tool.input.get("todos").and_then(|v| v.as_array()) else {
        return ToolResult::new(tool.id.clone(), "Missing 'todos' array parameter".to_string(), true);
    };

    if todos.is_empty() {
        return ToolResult::new(tool.id.clone(), "Empty 'todos' array".to_string(), true);
    }

    let mut created: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for todo_value in todos {
        let name = if let Some(n) = todo_value.get("name").and_then(|v| v.as_str()) {
            n.to_string()
        } else {
            errors.push("Missing 'name' in todo".to_string());
            continue;
        };

        let description = todo_value.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();

        // Normalize parent_id: treat "none", "null", "" as None
        let parent_id = todo_value
            .get("parent_id")
            .and_then(|v| {
                if v.is_null() {
                    return None;
                }
                v.as_str()
            })
            .filter(|s| {
                let lower = s.to_lowercase();
                !s.is_empty() && lower != "none" && lower != "null"
            })
            .map(ToString::to_string);

        // Validate parent exists if specified
        let ts = TodoState::get(state);
        if let Some(ref pid) = parent_id
            && !ts.todos.iter().any(|t| t.id == *pid)
        {
            let available: Vec<&str> = ts.todos.iter().map(|t| t.id.as_str()).collect();
            let available_str = if available.is_empty() {
                "no todos exist yet".to_string()
            } else {
                format!("available: {}", available.join(", "))
            };
            errors.push(format!("Parent '{pid}' not found for '{name}' ({available_str})"));
            continue;
        }

        let status = todo_value
            .get("status")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(TodoStatus::Pending);

        let ts = TodoState::get_mut(state);
        let id = format!("X{}", ts.next_todo_id);
        ts.next_todo_id += 1;

        ts.todos.push(TodoItem { id: id.clone(), parent_id, name: name.clone(), description, status });

        created.push(format!("{id}: {name}"));
    }

    let mut output = String::new();

    if !created.is_empty() {
        output.push_str(&format!("Created {} todo(s):\n{}", created.len(), created.join("\n")));
        // Update Todo panel timestamp
        state.touch_panel(ContextType::new(ContextType::TODO));
    }

    if !errors.is_empty() {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        output.push_str(&format!("Errors ({}):\n{}", errors.len(), errors.join("\n")));
    }

    ToolResult::new(tool.id.clone(), output, created.is_empty())
}

pub(crate) fn execute_update(tool: &ToolUse, state: &mut State) -> ToolResult {
    let Some(updates) = tool.input.get("updates").and_then(|v| v.as_array()) else {
        return ToolResult::new(tool.id.clone(), "Missing 'updates' array parameter".to_string(), true);
    };

    if updates.is_empty() {
        return ToolResult::new(tool.id.clone(), "Empty 'updates' array".to_string(), true);
    }

    let mut updated: Vec<String> = Vec::new();
    let mut deleted: Vec<String> = Vec::new();
    let mut not_found: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    // Collect all IDs being deleted in this batch to validate no orphans are created
    let delete_ids: std::collections::HashSet<String> = updates
        .iter()
        .filter(|u| {
            u.get("delete").and_then(serde_json::Value::as_bool).unwrap_or(false)
                || u.get("status").and_then(|v| v.as_str()) == Some("deleted")
        })
        .filter_map(|u| u.get("id").and_then(|v| v.as_str()).map(ToString::to_string))
        .collect();

    for update_value in updates {
        let Some(id) = update_value.get("id").and_then(|v| v.as_str()) else {
            errors.push("Missing 'id' in update".to_string());
            continue;
        };

        // Check for deletion (support both delete:true and status:"deleted")
        let should_delete = update_value.get("delete").and_then(serde_json::Value::as_bool).unwrap_or(false)
            || update_value.get("status").and_then(|v| v.as_str()) == Some("deleted");

        if should_delete {
            // Check that all children are also being deleted in this batch
            fn collect_descendants(id: &str, todos: &[TodoItem]) -> Vec<String> {
                let mut desc = Vec::new();
                for t in todos {
                    if t.parent_id.as_deref() == Some(id) {
                        desc.push(t.id.clone());
                        desc.extend(collect_descendants(&t.id, todos));
                    }
                }
                desc
            }

            let ts = TodoState::get(state);
            let descendants = collect_descendants(id, &ts.todos);
            let orphans: Vec<&String> = descendants.iter().filter(|d| !delete_ids.contains(d.as_str())).collect();

            if !orphans.is_empty() {
                errors.push(format!(
                    "{}: cannot delete — children {} would be orphaned. Delete them too, or delete all at once.",
                    id,
                    orphans.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
                ));
                continue;
            }

            let ts = TodoState::get_mut(state);
            let initial_len = ts.todos.len();
            ts.todos.retain(|t| t.id != id);
            if ts.todos.len() < initial_len {
                deleted.push(id.to_string());
            } else {
                not_found.push(id.to_string());
            }
            continue;
        }

        // Pre-validate parent_id if specified (normalize "none", "null", "" to None)
        let normalized_parent = if update_value.get("parent_id").is_some() {
            let raw = update_value.get("parent_id");
            if raw.is_some_and(serde_json::Value::is_null) {
                Some(None) // explicitly set to None
            } else if let Some(pid) = raw.and_then(|v| v.as_str()) {
                let lower = pid.to_lowercase();
                if pid.is_empty() || lower == "none" || lower == "null" {
                    Some(None) // normalize to None
                } else {
                    if pid == id {
                        errors.push(format!("{id}: cannot be its own parent"));
                        continue;
                    }
                    let ts = TodoState::get(state);
                    if !ts.todos.iter().any(|other| other.id == pid) {
                        let available: Vec<&str> =
                            ts.todos.iter().filter(|t| t.id != id).map(|t| t.id.as_str()).collect();
                        let available_str = if available.is_empty() {
                            "no other todos exist".to_string()
                        } else {
                            format!("available: {}", available.join(", "))
                        };
                        errors.push(format!("{id}: parent '{pid}' not found ({available_str})"));
                        continue;
                    }
                    Some(Some(pid.to_string()))
                }
            } else {
                None // no change
            }
        } else {
            None // no change
        };

        // Pre-check: if setting status to done, verify all children are done
        let status_str = update_value.get("status").and_then(|v| v.as_str());
        if let Some(s) = status_str
            && let Some(status) = s.parse::<TodoStatus>().ok()
            && status == TodoStatus::Done
        {
            let ts = TodoState::get(state);
            let undone_children: Vec<String> = ts
                .todos
                .iter()
                .filter(|c| c.parent_id.as_deref() == Some(id) && c.status != TodoStatus::Done)
                .map(|c| format!("{} ({})", c.id, c.name))
                .collect();
            if !undone_children.is_empty() {
                errors.push(format!("{}: cannot mark done — children not done: {}", id, undone_children.join(", ")));
                continue;
            }
        }

        // Find and update the todo
        let ts = TodoState::get_mut(state);
        let todo = ts.todos.iter_mut().find(|t| t.id == id);

        match todo {
            Some(t) => {
                let mut changes = Vec::new();

                if let Some(name) = update_value.get("name").and_then(|v| v.as_str()) {
                    t.name = name.to_string();
                    changes.push("name");
                }

                if let Some(desc) = update_value.get("description").and_then(|v| v.as_str()) {
                    t.description = desc.to_string();
                    changes.push("description");
                }

                // Handle parent_id - use normalized value (already validated above)
                if let Some(new_parent) = &normalized_parent {
                    t.parent_id.clone_from(new_parent);
                    changes.push("parent");
                }

                if let Some(status_str) = update_value.get("status").and_then(|v| v.as_str())
                    && let Some(status) = status_str.parse::<TodoStatus>().ok()
                {
                    t.status = status;
                    changes.push("status");
                }

                if !changes.is_empty() {
                    updated.push(format!("{}: {}", id, changes.join(", ")));
                }
            }
            None => {
                not_found.push(id.to_string());
            }
        }
    }

    // Auto-propagate in_progress to parent chain
    // If any todo was set to in_progress, walk up its parent chain and mark pending parents as in_progress
    let mut propagated: Vec<String> = Vec::new();
    for update_value in updates {
        let status_str = update_value.get("status").and_then(|v| v.as_str());
        if (status_str == Some("in_progress") || status_str == Some("~"))
            && let Some(id) = update_value.get("id").and_then(|v| v.as_str())
        {
            let ts = TodoState::get_mut(state);
            // Walk up parent chain
            let mut current_id = ts.todos.iter().find(|t| t.id == id).and_then(|t| t.parent_id.clone());
            while let Some(ref pid) = current_id {
                if let Some(parent) = ts.todos.iter_mut().find(|t| t.id == *pid) {
                    if parent.status == TodoStatus::Pending {
                        parent.status = TodoStatus::InProgress;
                        propagated.push(parent.id.clone());
                    }
                    current_id.clone_from(&parent.parent_id);
                } else {
                    break;
                }
            }
        }
    }

    // Update Todo panel timestamp if anything changed
    if !updated.is_empty() || !deleted.is_empty() || !propagated.is_empty() {
        state.touch_panel(ContextType::new(ContextType::TODO));
    }

    let mut output = String::new();

    if !updated.is_empty() {
        output.push_str(&format!("Updated {}:\n{}", updated.len(), updated.join("\n")));
    }

    if !propagated.is_empty() {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        output.push_str(&format!("Auto-propagated in_progress to parents: {}", propagated.join(", ")));
    }

    if !deleted.is_empty() {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        output.push_str(&format!("Deleted: {}", deleted.join(", ")));
    }

    if !not_found.is_empty() {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        output.push_str(&format!("Not found: {}", not_found.join(", ")));
    }

    if !errors.is_empty() {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        output.push_str(&format!("Errors:\n{}", errors.join("\n")));
    }

    ToolResult::new(tool.id.clone(), output, updated.is_empty() && deleted.is_empty() && propagated.is_empty())
}

pub(crate) fn execute_move(tool: &ToolUse, state: &mut State) -> ToolResult {
    let Some(id) = tool.input.get("id").and_then(|v| v.as_str()) else {
        return ToolResult::new(tool.id.clone(), "Missing 'id' parameter".to_string(), true);
    };

    // Normalize after_id: treat null, "none", "null", "" as None (move to top)
    let after_id = tool
        .input
        .get("after_id")
        .and_then(|v| {
            if v.is_null() {
                return None;
            }
            v.as_str()
        })
        .filter(|s| {
            let lower = s.to_lowercase();
            !s.is_empty() && lower != "none" && lower != "null"
        });

    // Find the todo to move
    let ts = TodoState::get(state);
    let Some(move_idx) = ts.todos.iter().position(|t| t.id == id) else {
        return ToolResult::new(tool.id.clone(), format!("Todo '{id}' not found"), true);
    };

    // Validate after_id exists if specified
    if let Some(aid) = after_id {
        if aid == id {
            return ToolResult::new(tool.id.clone(), format!("Cannot move '{id}' after itself"), true);
        }
        if !ts.todos.iter().any(|t| t.id == aid) {
            return ToolResult::new(tool.id.clone(), format!("Target '{aid}' not found"), true);
        }
    }

    // Remove the todo from its current position
    let ts = TodoState::get_mut(state);
    let item = ts.todos.remove(move_idx);

    // Insert at new position
    let insert_idx = after_id.map_or(0, |aid| {
        // Find the after_id position (may have shifted after remove)
        ts.todos.iter().position(|t| t.id == aid).map_or(0, |idx| idx + 1)
    });

    ts.todos.insert(insert_idx, item);
    state.touch_panel(ContextType::new(ContextType::TODO));

    let position_desc = after_id.map_or_else(|| "top".to_string(), |aid| format!("after {aid}"));

    ToolResult::new(tool.id.clone(), format!("Moved {id} to {position_desc}"), false)
}
