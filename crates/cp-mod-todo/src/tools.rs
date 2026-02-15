use cp_base::state::{ContextType, State, TodoItem, TodoStatus};
use cp_base::tools::{ToolResult, ToolUse};

pub fn execute_create(tool: &ToolUse, state: &mut State) -> ToolResult {
    let todos = match tool.input.get("todos").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing 'todos' array parameter".to_string(),
                is_error: true,
            };
        }
    };

    if todos.is_empty() {
        return ToolResult { tool_use_id: tool.id.clone(), content: "Empty 'todos' array".to_string(), is_error: true };
    }

    let mut created: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for todo_value in todos {
        let name = match todo_value.get("name").and_then(|v| v.as_str()) {
            Some(n) => n.to_string(),
            None => {
                errors.push("Missing 'name' in todo".to_string());
                continue;
            }
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
            .map(|s| s.to_string());

        // Validate parent exists if specified
        if let Some(ref pid) = parent_id
            && !state.todos.iter().any(|t| t.id == *pid)
        {
            let available: Vec<&str> = state.todos.iter().map(|t| t.id.as_str()).collect();
            let available_str = if available.is_empty() {
                "no todos exist yet".to_string()
            } else {
                format!("available: {}", available.join(", "))
            };
            errors.push(format!("Parent '{}' not found for '{}' ({})", pid, name, available_str));
            continue;
        }

        let status = todo_value
            .get("status")
            .and_then(|v| v.as_str())
            .and_then(TodoStatus::from_str)
            .unwrap_or(TodoStatus::Pending);

        let id = format!("X{}", state.next_todo_id);
        state.next_todo_id += 1;

        state.todos.push(TodoItem { id: id.clone(), parent_id, name: name.clone(), description, status });

        created.push(format!("{}: {}", id, name));
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

    ToolResult { tool_use_id: tool.id.clone(), content: output, is_error: created.is_empty() }
}

pub fn execute_update(tool: &ToolUse, state: &mut State) -> ToolResult {
    let updates = match tool.input.get("updates").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing 'updates' array parameter".to_string(),
                is_error: true,
            };
        }
    };

    if updates.is_empty() {
        return ToolResult {
            tool_use_id: tool.id.clone(),
            content: "Empty 'updates' array".to_string(),
            is_error: true,
        };
    }

    let mut updated: Vec<String> = Vec::new();
    let mut deleted: Vec<String> = Vec::new();
    let mut not_found: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    // Collect all IDs being deleted in this batch to validate no orphans are created
    let delete_ids: std::collections::HashSet<String> = updates
        .iter()
        .filter(|u| {
            u.get("delete").and_then(|v| v.as_bool()).unwrap_or(false)
                || u.get("status").and_then(|v| v.as_str()) == Some("deleted")
        })
        .filter_map(|u| u.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .collect();

    for update_value in updates {
        let id = match update_value.get("id").and_then(|v| v.as_str()) {
            Some(i) => i,
            None => {
                errors.push("Missing 'id' in update".to_string());
                continue;
            }
        };

        // Check for deletion (support both delete:true and status:"deleted")
        let should_delete = update_value.get("delete").and_then(|v| v.as_bool()).unwrap_or(false)
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

            let descendants = collect_descendants(id, &state.todos);
            let orphans: Vec<&String> = descendants.iter().filter(|d| !delete_ids.contains(d.as_str())).collect();

            if !orphans.is_empty() {
                errors.push(format!(
                    "{}: cannot delete — children {} would be orphaned. Delete them too, or delete all at once.",
                    id,
                    orphans.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
                ));
                continue;
            }

            let initial_len = state.todos.len();
            state.todos.retain(|t| t.id != id);
            if state.todos.len() < initial_len {
                deleted.push(id.to_string());
            } else {
                not_found.push(id.to_string());
            }
            continue;
        }

        // Pre-validate parent_id if specified (normalize "none", "null", "" to None)
        let normalized_parent = if update_value.get("parent_id").is_some() {
            let raw = update_value.get("parent_id");
            if raw.map(|v| v.is_null()).unwrap_or(false) {
                Some(None) // explicitly set to None
            } else if let Some(pid) = raw.and_then(|v| v.as_str()) {
                let lower = pid.to_lowercase();
                if pid.is_empty() || lower == "none" || lower == "null" {
                    Some(None) // normalize to None
                } else {
                    if pid == id {
                        errors.push(format!("{}: cannot be its own parent", id));
                        continue;
                    }
                    if !state.todos.iter().any(|other| other.id == pid) {
                        let available: Vec<&str> =
                            state.todos.iter().filter(|t| t.id != id).map(|t| t.id.as_str()).collect();
                        let available_str = if available.is_empty() {
                            "no other todos exist".to_string()
                        } else {
                            format!("available: {}", available.join(", "))
                        };
                        errors.push(format!("{}: parent '{}' not found ({})", id, pid, available_str));
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
            && let Some(status) = TodoStatus::from_str(s)
            && status == TodoStatus::Done
        {
            let undone_children: Vec<String> = state
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
        let todo = state.todos.iter_mut().find(|t| t.id == id);

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
                    t.parent_id = new_parent.clone();
                    changes.push("parent");
                }

                if let Some(status_str) = update_value.get("status").and_then(|v| v.as_str())
                    && let Some(status) = TodoStatus::from_str(status_str)
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
            // Walk up parent chain
            let mut current_id = state.todos.iter().find(|t| t.id == id).and_then(|t| t.parent_id.clone());
            while let Some(pid) = current_id {
                if let Some(parent) = state.todos.iter_mut().find(|t| t.id == pid) {
                    if parent.status == TodoStatus::Pending {
                        parent.status = TodoStatus::InProgress;
                        propagated.push(parent.id.clone());
                    }
                    current_id = parent.parent_id.clone();
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

    ToolResult {
        tool_use_id: tool.id.clone(),
        content: output,
        is_error: updated.is_empty() && deleted.is_empty() && propagated.is_empty(),
    }
}

pub fn execute_move(tool: &ToolUse, state: &mut State) -> ToolResult {
    let id = match tool.input.get("id").and_then(|v| v.as_str()) {
        Some(i) => i,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: "Missing 'id' parameter".to_string(),
                is_error: true,
            };
        }
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
    let move_idx = match state.todos.iter().position(|t| t.id == id) {
        Some(idx) => idx,
        None => {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Todo '{}' not found", id),
                is_error: true,
            };
        }
    };

    // Validate after_id exists if specified
    if let Some(aid) = after_id {
        if aid == id {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Cannot move '{}' after itself", id),
                is_error: true,
            };
        }
        if !state.todos.iter().any(|t| t.id == aid) {
            return ToolResult {
                tool_use_id: tool.id.clone(),
                content: format!("Target '{}' not found", aid),
                is_error: true,
            };
        }
    }

    // Remove the todo from its current position
    let item = state.todos.remove(move_idx);

    // Insert at new position
    let insert_idx = match after_id {
        None => 0, // move to top
        Some(aid) => {
            // Find the after_id position (may have shifted after remove)
            match state.todos.iter().position(|t| t.id == aid) {
                Some(idx) => idx + 1, // insert after it
                None => 0,            // shouldn't happen, we validated above
            }
        }
    };

    state.todos.insert(insert_idx, item);
    state.touch_panel(ContextType::new(ContextType::TODO));

    let position_desc = match after_id {
        None => "top".to_string(),
        Some(aid) => format!("after {}", aid),
    };

    ToolResult { tool_use_id: tool.id.clone(), content: format!("Moved {} to {}", id, position_desc), is_error: false }
}
