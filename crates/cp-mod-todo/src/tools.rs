//! Pure task operations for the thread-owned todo model.
//!
//! Every entry point takes an explicit `thread_id` and is **free of any
//! `cp-mod-threads` dependency** (the focused thread is resolved by the main
//! crate and passed down — see the design doc §8 dependency-injection rule).
//! These functions never touch panels: the panel-refresh asymmetry (structural
//! `Think.todo` deprecates the panel, `todo_mark` does not) is the caller's
//! responsibility.

use serde::Deserialize;

use cp_base::state::runtime::State;

use crate::types::{TodoItem, TodoState, TodoStatus};

// =============================================================================
// Input / output types
// =============================================================================

/// One node of the recursive upsert forest accepted by `Think.todo`.
///
/// `id` absent → **create**; present → **update** (partial patch). `children`
/// are descendants of this node. `status` is a raw string so tool ergonomics
/// aliases (`~`, `x`, `/`) parse alongside the canonical wire values.
#[derive(Debug, Default, Deserialize)]
pub struct TodoNode {
    /// Existing id → update; absent → create.
    #[serde(default)]
    pub id: Option<String>,
    /// Title. Required on create, optional on update.
    #[serde(default)]
    pub name: Option<String>,
    /// Detailed description.
    #[serde(default)]
    pub description: Option<String>,
    /// Status (canonical wire value or ergonomics alias).
    #[serde(default)]
    pub status: Option<String>,
    /// Reparent an existing item under an already-existing id (rare).
    #[serde(default)]
    pub parent_id: Option<String>,
    /// Nodes living under this node.
    #[serde(default)]
    pub children: Vec<Self>,
}

/// Result of an `upsert_task_forest` call.
#[derive(Debug, Default)]
pub struct UpsertOutcome {
    /// Ids of newly created items.
    pub created: Vec<String>,
    /// Ids of updated items.
    pub updated: Vec<String>,
    /// Per-node error messages (a failed node skips its subtree).
    pub errors: Vec<String>,
}

impl UpsertOutcome {
    /// Whether anything was actually created or updated.
    #[must_use]
    pub const fn changed(&self) -> bool {
        !self.created.is_empty() || !self.updated.is_empty()
    }
}

/// Result of a `mark_tasks` call.
#[derive(Debug, Default)]
pub struct MarkOutcome {
    /// Ids whose status was changed.
    pub marked: Vec<String>,
    /// Per-mark error messages.
    pub errors: Vec<String>,
}

impl MarkOutcome {
    /// Whether any status was changed.
    #[must_use]
    pub const fn changed(&self) -> bool {
        !self.marked.is_empty()
    }
}

// =============================================================================
// Shared pure invariants (thread-scoped)
// =============================================================================

/// Recursively collect all descendant ids of `id` (within the same thread —
/// nesting is thread-local, so a plain `parent_id` walk stays scoped).
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

/// Reject marking `id` `Done` while any of its **non-cancelled** children are
/// not done. Cancelled children are ignored (soft-deleted).
fn check_done_allowed(state: &State, id: &str) -> Result<(), String> {
    let ts = TodoState::get(state);
    let undone: Vec<String> = ts
        .todos
        .iter()
        .filter(|c| {
            c.parent_id.as_deref() == Some(id) && c.status != TodoStatus::Done && c.status != TodoStatus::Cancelled
        })
        .map(|c| format!("{} ({})", c.id, c.name))
        .collect();
    if undone.is_empty() {
        Ok(())
    } else {
        Err(format!("{id}: cannot mark done — children not done: {}", undone.join(", ")))
    }
}

/// Bubble `Planned` ancestors of `id` up to `InProgress`. Returns nothing —
/// mutates in place. Used after a child is set `InProgress`.
fn propagate_in_progress(state: &mut State, id: &str) {
    let ts = TodoState::get_mut(state);
    let mut current = ts.todos.iter().find(|t| t.id == id).and_then(|t| t.parent_id.clone());
    while let Some(pid) = current.as_ref() {
        let Some(parent) = ts.todos.iter_mut().find(|t| t.id == *pid) else {
            break;
        };
        if parent.status == TodoStatus::Planned {
            parent.status = TodoStatus::InProgress;
        }
        current.clone_from(&parent.parent_id);
    }
}

/// Validate that `parent` is a legal parent for `id` within `thread_id`:
/// exists, same thread, not self, and not a descendant of `id` (no cycle).
fn validate_parent(state: &State, thread_id: &str, id: &str, parent: &str) -> Result<(), String> {
    if parent == id {
        return Err(format!("{id}: cannot be its own parent"));
    }
    let ts = TodoState::get(state);
    let exists = ts.todos.iter().any(|t| t.id == parent && t.thread_id == thread_id);
    if !exists {
        return Err(format!("{id}: parent '{parent}' not found in this thread"));
    }
    if collect_descendants(id, &ts.todos).iter().any(|d| d == parent) {
        return Err(format!("{id}: '{parent}' is a descendant — would create a cycle"));
    }
    Ok(())
}

/// Parse a status string (canonical wire value or ergonomics alias).
fn parse_status(raw: &str) -> Result<TodoStatus, String> {
    raw.parse().map_err(|()| format!("unknown status '{raw}'"))
}

// =============================================================================
// upsert_task_forest
// =============================================================================

/// Threaded accumulators for one `upsert_task_forest` traversal — bundled so
/// the recursive helpers stay within the argument budget. `state` is passed
/// separately (a disjoint `&mut` borrow).
struct UpsertCtx<'ctx> {
    /// The focused thread every node belongs to.
    thread_id: &'ctx str,
    /// Created/updated ids + per-node errors.
    outcome: &'ctx mut UpsertOutcome,
    /// Ids set `InProgress` this call (ancestors bubbled afterwards).
    newly_in_progress: &'ctx mut Vec<String>,
}

/// Apply a recursive upsert forest to the focused thread's tasks.
///
/// Pre-order DFS (outer→inner): a node is created/updated and its id resolved
/// **before** its children are processed, so a new parent already exists by the
/// time its children attach. Best-effort per node — a failed node skips its
/// subtree.
pub fn upsert_task_forest(state: &mut State, thread_id: &str, nodes: &[TodoNode]) -> UpsertOutcome {
    let mut outcome = UpsertOutcome::default();
    let mut newly_in_progress: Vec<String> = Vec::new();
    {
        let mut ctx = UpsertCtx { thread_id, outcome: &mut outcome, newly_in_progress: &mut newly_in_progress };
        for node in nodes {
            apply_node(state, &mut ctx, node, None);
        }
    }
    for id in &newly_in_progress {
        propagate_in_progress(state, id);
    }
    outcome
}

/// Apply one node (create or update), then recurse into its children with this
/// node's resolved id as their parent. Records results into `ctx.outcome`.
fn apply_node(state: &mut State, ctx: &mut UpsertCtx<'_>, node: &TodoNode, enclosing_parent: Option<&str>) {
    let resolved = match node.id.as_deref() {
        None => create_node(state, ctx, node, enclosing_parent),
        Some(_) => update_node(state, ctx, node, enclosing_parent),
    };
    // Only recurse when this node resolved to a real id (create/update ok).
    if let Some(parent_id) = resolved {
        for child in &node.children {
            apply_node(state, ctx, child, Some(&parent_id));
        }
    }
}

/// Create a new item. Returns its id on success (so children can attach).
fn create_node(
    state: &mut State,
    ctx: &mut UpsertCtx<'_>,
    node: &TodoNode,
    enclosing_parent: Option<&str>,
) -> Option<String> {
    let Some(name) = node.name.as_deref().filter(|n| !n.trim().is_empty()) else {
        ctx.outcome.errors.push("create: missing 'name'".to_owned());
        return None;
    };
    // Parent precedence: explicit parent_id, else the enclosing node.
    let parent_id = node.parent_id.as_deref().or(enclosing_parent);
    if let Some(pid) = parent_id
        && let Err(e) = validate_parent_for_create(state, ctx.thread_id, pid)
    {
        ctx.outcome.errors.push(format!("create '{name}': {e}"));
        return None;
    }
    let status = match node.status.as_deref() {
        None => TodoStatus::Planned,
        Some(raw) => match parse_status(raw) {
            Ok(s) => s,
            Err(e) => {
                ctx.outcome.errors.push(format!("create '{name}': {e}"));
                return None;
            }
        },
    };

    let ts = TodoState::get_mut(state);
    let id = format!("X{}", ts.next_todo_id);
    ts.next_todo_id = ts.next_todo_id.saturating_add(1);
    ts.todos.push(TodoItem {
        id: id.clone(),
        thread_id: ctx.thread_id.to_owned(),
        parent_id: parent_id.map(str::to_owned),
        name: name.to_owned(),
        description: node.description.clone().unwrap_or_default(),
        status,
    });
    if status == TodoStatus::InProgress {
        ctx.newly_in_progress.push(id.clone());
    }
    ctx.outcome.created.push(id.clone());
    Some(id)
}

/// Validate a parent id for a *create* (exists + same thread; no cycle check
/// needed since the new item has no descendants yet).
fn validate_parent_for_create(state: &State, thread_id: &str, parent: &str) -> Result<(), String> {
    let ts = TodoState::get(state);
    if ts.todos.iter().any(|t| t.id == parent && t.thread_id == thread_id) {
        Ok(())
    } else {
        Err(format!("parent '{parent}' not found in this thread"))
    }
}

/// Update an existing item (partial patch). Returns its id on success.
///
/// The caller (`apply_node`) only dispatches here for a node carrying an `id`,
/// so `node.id` is always `Some`.
fn update_node(
    state: &mut State,
    ctx: &mut UpsertCtx<'_>,
    node: &TodoNode,
    enclosing_parent: Option<&str>,
) -> Option<String> {
    let id = node.id.as_deref()?;
    let thread_id = ctx.thread_id;

    // Item must exist AND belong to the focused thread.
    if !TodoState::get(state).todos.iter().any(|t| t.id == id && t.thread_id == thread_id) {
        ctx.outcome.errors.push(format!("update '{id}': not found in this thread"));
        return None;
    }

    // Resolve reparent target + new status up front so a bad value aborts
    // before any mutation.
    let reparent_to = node.parent_id.as_deref().or(enclosing_parent);
    if let Some(pid) = reparent_to
        && let Err(e) = validate_parent(state, thread_id, id, pid)
    {
        ctx.outcome.errors.push(format!("update '{id}': {e}"));
        return None;
    }
    let new_status = match resolve_status(node) {
        Ok(s) => s,
        Err(e) => {
            ctx.outcome.errors.push(format!("update '{id}': {e}"));
            return None;
        }
    };
    if new_status == Some(TodoStatus::Done)
        && let Err(e) = check_done_allowed(state, id)
    {
        ctx.outcome.errors.push(format!("update '{id}': {e}"));
        return None;
    }

    write_update_fields(state, id, &UpdatePatch { node, reparent_to, new_status });
    if new_status == Some(TodoStatus::InProgress) {
        ctx.newly_in_progress.push(id.to_owned());
    }
    ctx.outcome.updated.push(id.to_owned());
    Some(id.to_owned())
}

/// Parse an optional status string into `Option<TodoStatus>` (None = untouched).
fn resolve_status(node: &TodoNode) -> Result<Option<TodoStatus>, String> {
    node.status.as_deref().map_or(Ok(None), |raw| parse_status(raw).map(Some))
}

/// A validated field patch for `write_update_fields` — bundled to keep the
/// helper within the argument budget.
struct UpdatePatch<'patch> {
    /// The source node carrying name/description edits.
    node: &'patch TodoNode,
    /// New parent id, if reparenting.
    reparent_to: Option<&'patch str>,
    /// New status, if changing.
    new_status: Option<TodoStatus>,
}

/// Apply the validated field patch to an existing item (name/description/parent/
/// status). Assumes all validation already passed.
fn write_update_fields(state: &mut State, id: &str, patch: &UpdatePatch<'_>) {
    let UpdatePatch { node, reparent_to, new_status } = *patch;
    let ts = TodoState::get_mut(state);
    let Some(item) = ts.todos.iter_mut().find(|t| t.id == id) else {
        return;
    };
    if let Some(name) = node.name.as_deref() {
        name.clone_into(&mut item.name);
    }
    if let Some(desc) = node.description.as_deref() {
        desc.clone_into(&mut item.description);
    }
    if let Some(pid) = reparent_to {
        item.parent_id = Some(pid.to_owned());
    }
    if let Some(status) = new_status {
        item.status = status;
    }
}

// =============================================================================
// mark_tasks
// =============================================================================

/// Flip the status of a batch of items, all belonging to `thread_id`.
///
/// Enforces `check_done_allowed` and bubbles `Planned` ancestors on
/// `InProgress`. `Cancelled` is the soft-delete. Best-effort per mark.
pub fn mark_tasks(state: &mut State, thread_id: &str, marks: &[(String, TodoStatus)]) -> MarkOutcome {
    let mut outcome = MarkOutcome::default();
    let mut newly_in_progress: Vec<String> = Vec::new();
    for mark in marks {
        let id = &mark.0;
        let status = mark.1;
        let exists = TodoState::get(state).todos.iter().any(|t| t.id == *id && t.thread_id == thread_id);
        if !exists {
            outcome.errors.push(format!("'{id}': not found in this thread"));
            continue;
        }
        if status == TodoStatus::Done
            && let Err(e) = check_done_allowed(state, id)
        {
            outcome.errors.push(e);
            continue;
        }
        if let Some(item) = TodoState::get_mut(state).todos.iter_mut().find(|t| t.id == *id) {
            item.status = status;
            if status == TodoStatus::InProgress {
                newly_in_progress.push(id.clone());
            }
            outcome.marked.push(id.clone());
        }
    }
    for id in &newly_in_progress {
        propagate_in_progress(state, id);
    }
    outcome
}

// =============================================================================
// purge_threadless / set_focus_filter
// =============================================================================

/// Drop every item lacking a `thread_id` (the legacy, pre-rework backlog).
/// Called once on load — a permanent, forever purge (FR4).
pub fn purge_threadless(state: &mut State) {
    TodoState::get_mut(state).todos.retain(|t| !t.thread_id.is_empty());
}

/// Set the injected focused-thread filter used by the panel. Returns whether it
/// changed (which drives the caller's forced panel refresh).
pub fn set_focus_filter(state: &mut State, thread_id: Option<String>) -> bool {
    let ts = TodoState::get_mut(state);
    if ts.focus_filter == thread_id {
        false
    } else {
        ts.focus_filter = thread_id;
        true
    }
}
