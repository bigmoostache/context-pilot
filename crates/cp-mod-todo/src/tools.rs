use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

use crate::types::{TodoItem, TodoState, TodoStatus};
use std::fmt::Write as _;

/// Normalize a `parent_id` JSON value: treat `null`, `"none"`, `"null"`, `""`
/// as `None`. Returns the trimmed owned id otherwise.
fn normalize_parent_id(value: &serde_json::Value) -> Option<String> {
    value
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
        .map(str::to_owned)
}

/// Create a single todo from one JSON entry. Returns the `"id: name"` label on
/// success, or an error message (missing name, unknown parent) on failure.
fn create_one_todo(todo_value: &serde_json::Value, state: &mut State) -> Result<String, String> {
    let Some(name) = todo_value.get("name").and_then(|v| v.as_str()).map(str::to_owned) else {
        return Err("Missing 'name' in todo".to_owned());
    };
    let description = todo_value.get("description").and_then(|v| v.as_str()).unwrap_or("").to_owned();
    let parent_id = normalize_parent_id(todo_value);

    // Validate parent exists if specified.
    let ts = TodoState::get(state);
    if let Some(pid) = &parent_id
        && !ts.todos.iter().any(|t| t.id == *pid)
    {
        let available: Vec<&str> = ts.todos.iter().map(|t| t.id.as_str()).collect();
        let available_str = if available.is_empty() {
            "no todos exist yet".to_owned()
        } else {
            format!("available: {}", available.join(", "))
        };
        return Err(format!("Parent '{pid}' not found for '{name}' ({available_str})"));
    }

    let status =
        todo_value.get("status").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()).unwrap_or(TodoStatus::Pending);

    let ts_mut = TodoState::get_mut(state);
    let id = format!("X{}", ts_mut.next_todo_id);
    ts_mut.next_todo_id = ts_mut.next_todo_id.saturating_add(1);
    ts_mut.todos.push(TodoItem { id: id.clone(), parent_id, name: name.clone(), description, status });
    Ok(format!("{id}: {name}"))
}

/// Execute `todo_create` tool — add one or more todo items with optional nesting.
pub(crate) fn execute_create(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("todo_create");
    let Some(todos) = tool.input.get("todos").and_then(|v| v.as_array()) else {
        return ToolResult::new(tool.id.clone(), "Missing 'todos' array parameter".to_owned(), true);
    };

    if todos.is_empty() {
        return ToolResult::new(tool.id.clone(), "Empty 'todos' array".to_owned(), true);
    }

    let mut created: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for todo_value in todos {
        match create_one_todo(todo_value, state) {
            Ok(label) => created.push(label),
            Err(e) => errors.push(e),
        }
    }

    let mut output = String::new();

    if !created.is_empty() {
        let _r = write!(output, "Created {} todo(s):\n{}", created.len(), created.join("\n"));
        // Update Todo panel timestamp
        state.touch_panel(Kind::TODO);
    }

    if !errors.is_empty() {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        let _r = write!(output, "Errors ({}):\n{}", errors.len(), errors.join("\n"));
    }

    ToolResult::new(tool.id.clone(), output, created.is_empty())
}

/// Collect the ids of every todo being deleted in this batch (via `delete:true`
/// or `status:"deleted"`), used to validate no child is orphaned.
fn collect_delete_ids(updates: &[serde_json::Value]) -> std::collections::HashSet<String> {
    updates
        .iter()
        .filter(|u| {
            u.get("delete").and_then(serde_json::Value::as_bool).unwrap_or(false)
                || u.get("status").and_then(|v| v.as_str()) == Some("deleted")
        })
        .filter_map(|u| u.get("id").and_then(|v| v.as_str()).map(str::to_owned))
        .collect()
}

/// Recursively collect all descendant ids of `id`.
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

/// Whether an update entry requests deletion (`delete:true` or `status:"deleted"`).
fn is_delete_request(update_value: &serde_json::Value) -> bool {
    update_value.get("delete").and_then(serde_json::Value::as_bool).unwrap_or(false)
        || update_value.get("status").and_then(|v| v.as_str()) == Some("deleted")
}

/// Outcome of processing one deletion request.
enum DeleteOutcome {
    /// Todo removed — carries its id.
    Deleted(String),
    /// Todo id not present — carries its id.
    NotFound(String),
    /// Deletion rejected (would orphan children) — carries the error message.
    Rejected(String),
}

/// Handle one deletion request, rejecting it when a child would be orphaned
/// (i.e. a descendant not also being deleted in this batch).
fn delete_one(id: &str, delete_ids: &std::collections::HashSet<String>, state: &mut State) -> DeleteOutcome {
    let ts_check = TodoState::get(state);
    let descendants = collect_descendants(id, &ts_check.todos);
    let orphans: Vec<&String> = descendants.iter().filter(|d| !delete_ids.contains(d.as_str())).collect();
    if !orphans.is_empty() {
        return DeleteOutcome::Rejected(format!(
            "{}: cannot delete — children {} would be orphaned. Delete them too, or delete all at once.",
            id,
            orphans.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
        ));
    }

    let ts_del = TodoState::get_mut(state);
    let initial_len = ts_del.todos.len();
    ts_del.todos.retain(|t| t.id != id);
    if ts_del.todos.len() < initial_len {
        DeleteOutcome::Deleted(id.to_owned())
    } else {
        DeleteOutcome::NotFound(id.to_owned())
    }
}

/// A requested change to a todo's `parent_id`, pre-validated.
enum ParentUpdate {
    /// No `parent_id` field present — leave the parent unchanged.
    Unchanged,
    /// Detach: make the todo top-level (`parent_id = None`).
    Detach,
    /// Reparent under the carried (validated, existing) parent id.
    Reparent(String),
}

/// Resolve a `parent_id` change into a [`ParentUpdate`], or `Err(msg)` on a
/// validation failure (self-parent or unknown parent).
fn resolve_parent_update(update_value: &serde_json::Value, id: &str, state: &State) -> Result<ParentUpdate, String> {
    if update_value.get("parent_id").is_none() {
        return Ok(ParentUpdate::Unchanged);
    }
    let raw = update_value.get("parent_id");
    if raw.is_some_and(serde_json::Value::is_null) {
        return Ok(ParentUpdate::Detach);
    }
    let Some(pid) = raw.and_then(|v| v.as_str()) else {
        return Ok(ParentUpdate::Unchanged);
    };
    let lower = pid.to_lowercase();
    if pid.is_empty() || lower == "none" || lower == "null" {
        return Ok(ParentUpdate::Detach);
    }
    if pid == id {
        return Err(format!("{id}: cannot be its own parent"));
    }
    let ts = TodoState::get(state);
    if !ts.todos.iter().any(|other| other.id == pid) {
        let available: Vec<&str> = ts.todos.iter().filter(|t| t.id != id).map(|t| t.id.as_str()).collect();
        let available_str = if available.is_empty() {
            "no other todos exist".to_owned()
        } else {
            format!("available: {}", available.join(", "))
        };
        return Err(format!("{id}: parent '{pid}' not found ({available_str})"));
    }
    Ok(ParentUpdate::Reparent(pid.to_owned()))
}

/// Reject marking a todo `done` while any of its children are not done.
fn check_done_allowed(update_value: &serde_json::Value, id: &str, state: &State) -> Result<(), String> {
    let Some(s) = update_value.get("status").and_then(|v| v.as_str()) else {
        return Ok(());
    };
    if s.parse::<TodoStatus>().ok() != Some(TodoStatus::Done) {
        return Ok(());
    }
    let ts = TodoState::get(state);
    let undone: Vec<String> = ts
        .todos
        .iter()
        .filter(|c| c.parent_id.as_deref() == Some(id) && c.status != TodoStatus::Done)
        .map(|c| format!("{} ({})", c.id, c.name))
        .collect();
    if undone.is_empty() {
        Ok(())
    } else {
        Err(format!("{id}: cannot mark done — children not done: {}", undone.join(", ")))
    }
}

/// Apply the field changes from one update entry onto a todo. `parent` is the
/// pre-validated parent change from [`resolve_parent_update`]. Returns the list
/// of changed field labels.
fn apply_field_updates(t: &mut TodoItem, update_value: &serde_json::Value, parent: &ParentUpdate) -> Vec<&'static str> {
    let mut changes = Vec::new();
    if let Some(name) = update_value.get("name").and_then(|v| v.as_str()) {
        name.clone_into(&mut t.name);
        changes.push("name");
    }
    if let Some(desc) = update_value.get("description").and_then(|v| v.as_str()) {
        desc.clone_into(&mut t.description);
        changes.push("description");
    }
    match parent {
        ParentUpdate::Unchanged => {}
        ParentUpdate::Detach => {
            t.parent_id = None;
            changes.push("parent");
        }
        ParentUpdate::Reparent(pid) => {
            t.parent_id = Some(pid.clone());
            changes.push("parent");
        }
    }
    if let Some(status) = update_value.get("status").and_then(|v| v.as_str()).and_then(|s| s.parse::<TodoStatus>().ok())
    {
        t.status = status;
        changes.push("status");
    }
    changes
}

/// Walk up the parent chain of every `in_progress`-set todo, promoting pending
/// ancestors to `in_progress`. Returns the ids that were promoted.
fn propagate_in_progress(updates: &[serde_json::Value], state: &mut State) -> Vec<String> {
    let mut propagated: Vec<String> = Vec::new();
    for update_value in updates {
        let prop_status = update_value.get("status").and_then(|v| v.as_str());
        if (prop_status == Some("in_progress") || prop_status == Some("~"))
            && let Some(id) = update_value.get("id").and_then(|v| v.as_str())
        {
            let ts = TodoState::get_mut(state);
            let mut current_id = ts.todos.iter().find(|t| t.id == id).and_then(|t| t.parent_id.clone());
            while let Some(pid) = &current_id {
                let Some(parent) = ts.todos.iter_mut().find(|t| t.id == *pid) else {
                    break;
                };
                if parent.status == TodoStatus::Pending {
                    parent.status = TodoStatus::InProgress;
                    propagated.push(parent.id.clone());
                }
                current_id.clone_from(&parent.parent_id);
            }
        }
    }
    propagated
}

/// Tallies accumulated while applying a batch of todo updates.
#[derive(Default)]
struct UpdateTally {
    /// `"id: changed-fields"` lines for successfully modified todos.
    modified: Vec<String>,
    /// Ids of deleted todos.
    deleted: Vec<String>,
    /// Ids of propagated (auto-`in_progress`) ancestors.
    propagated: Vec<String>,
    /// Ids referenced by an update but not found.
    not_found: Vec<String>,
    /// Validation / parse error messages.
    errors: Vec<String>,
}

/// Format the accumulated update tally into the tool's output string.
fn build_update_output(tally: &UpdateTally) -> String {
    let mut output = String::new();
    let mut push_section = |body: String| {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        output.push_str(&body);
    };
    if !tally.modified.is_empty() {
        push_section(format!("Updated {}:\n{}", tally.modified.len(), tally.modified.join("\n")));
    }
    if !tally.propagated.is_empty() {
        push_section(format!("Auto-propagated in_progress to parents: {}", tally.propagated.join(", ")));
    }
    if !tally.deleted.is_empty() {
        push_section(format!("Deleted: {}", tally.deleted.join(", ")));
    }
    if !tally.not_found.is_empty() {
        push_section(format!("Not found: {}", tally.not_found.join(", ")));
    }
    if !tally.errors.is_empty() {
        push_section(format!("Errors:\n{}", tally.errors.join("\n")));
    }
    output
}

/// Apply one update entry, routing to delete / field-update and recording the
/// result into `tally`. Assumes propagation runs separately afterwards.
fn apply_one_update(
    update_value: &serde_json::Value,
    delete_ids: &std::collections::HashSet<String>,
    tally: &mut UpdateTally,
    state: &mut State,
) {
    let Some(id) = update_value.get("id").and_then(|v| v.as_str()) else {
        tally.errors.push("Missing 'id' in update".to_owned());
        return;
    };

    if is_delete_request(update_value) {
        match delete_one(id, delete_ids, state) {
            DeleteOutcome::Deleted(x) => tally.deleted.push(x),
            DeleteOutcome::NotFound(x) => tally.not_found.push(x),
            DeleteOutcome::Rejected(e) => tally.errors.push(e),
        }
        return;
    }

    let normalized_parent = match resolve_parent_update(update_value, id, state) {
        Ok(v) => v,
        Err(e) => {
            tally.errors.push(e);
            return;
        }
    };
    if let Err(e) = check_done_allowed(update_value, id, state) {
        tally.errors.push(e);
        return;
    }

    let ts = TodoState::get_mut(state);
    match ts.todos.iter_mut().find(|t| t.id == id) {
        Some(t) => {
            let changes = apply_field_updates(t, update_value, &normalized_parent);
            if !changes.is_empty() {
                tally.modified.push(format!("{}: {}", id, changes.join(", ")));
            }
        }
        None => tally.not_found.push(id.to_owned()),
    }
}

/// Execute `todo_update` tool — modify status, name, description, or delete todos.
pub(crate) fn execute_update(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("todo_update");
    let Some(updates) = tool.input.get("updates").and_then(|v| v.as_array()) else {
        return ToolResult::new(tool.id.clone(), "Missing 'updates' array parameter".to_owned(), true);
    };

    if updates.is_empty() {
        return ToolResult::new(tool.id.clone(), "Empty 'updates' array".to_owned(), true);
    }

    let updates = updates.clone();
    let delete_ids = collect_delete_ids(&updates);
    let mut tally = UpdateTally::default();

    for update_value in &updates {
        apply_one_update(update_value, &delete_ids, &mut tally, state);
    }

    tally.propagated = propagate_in_progress(&updates, state);

    if !tally.modified.is_empty() || !tally.deleted.is_empty() || !tally.propagated.is_empty() {
        state.touch_panel(Kind::TODO);
    }

    let is_error = tally.modified.is_empty() && tally.deleted.is_empty() && tally.propagated.is_empty();
    ToolResult::new(tool.id.clone(), build_update_output(&tally), is_error)
}

/// Execute `todo_move` tool — reorder a todo by placing it after another.
pub(crate) fn execute_move(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("todo_move");
    let Some(id) = tool.input.get("id").and_then(|v| v.as_str()) else {
        return ToolResult::new(tool.id.clone(), "Missing 'id' parameter".to_owned(), true);
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
    let ts_mut = TodoState::get_mut(state);
    let item = ts_mut.todos.remove(move_idx);

    // Insert at new position
    let insert_idx = after_id.map_or(0, |aid| {
        // Find the after_id position (may have shifted after remove)
        ts_mut.todos.iter().position(|t| t.id == aid).map_or(0, |idx| idx.saturating_add(1))
    });

    ts_mut.todos.insert(insert_idx, item);
    state.touch_panel(Kind::TODO);

    let position_desc = after_id.map_or_else(|| "top".to_owned(), |aid| format!("after {aid}"));

    ToolResult::new(tool.id.clone(), format!("Moved {id} to {position_desc}"), false)
}
