//! Structured task upsert — the `Todo` tool's core.
//!
//! The model hands in a list of items; each one either **updates** an existing
//! task (it carries an `id`) or **creates** a new one (it does not). Parenthood
//! is expressed *only* by physical nesting: an item's `children` are created
//! under it, and a nested item may never carry an `id` — there is no
//! `parent_id` anywhere in the surface.
//!
//! Two properties make this safe to drive blind:
//!
//!   * **Merge, never replace.** Tasks absent from the payload are left
//!     untouched. Removal is an explicit `status: cancelled`, never an omission
//!     — so a partial call can't silently wipe the roadmap.
//!   * **Validate-then-apply.** [`upsert`] collects *every* problem before
//!     mutating anything, so a bad call is a hard error that leaves the task
//!     list exactly as it was (atomic, like the diff-based tool it replaces).

use std::collections::HashSet;

use cp_base::state::runtime::State;

use crate::types::{TodoItem, TodoState, TodoStatus};

// =============================================================================
// Payload
// =============================================================================

/// One item of the `items` payload.
///
/// Every field is optional at parse time so validation (not deserialization)
/// owns the error messages. `id` present = update that task; `id` absent =
/// create. `children` are always creations parented to this item.
#[derive(Debug, Default)]
pub struct Item {
    /// Existing task id to update. Absent means "create a new task".
    pub id: Option<String>,
    /// Task title. Required when creating; when updating, absent = untouched.
    pub title: Option<String>,
    /// Longer detail. Absent = untouched; present (even empty) = replace.
    pub description: Option<String>,
    /// Status wire string (`planned`/`in_progress`/`done`/`cancelled`).
    pub status: Option<String>,
    /// Nested items, created as children of this one. Creation-only — a child
    /// carrying an `id` is a hard error.
    pub children: Vec<Self>,
}

/// Parse the tool's `items` JSON array into [`Item`]s.
///
/// # Errors
/// Returns `Err` when the value is not an array, or when any element (at any
/// nesting depth) is not an object.
pub fn parse_items(value: &serde_json::Value) -> Result<Vec<Item>, String> {
    let arr = value.as_array().ok_or_else(|| "'items' must be an array".to_owned())?;
    arr.iter().map(parse_item).collect()
}

/// Parse one payload element (must be a JSON object) into an [`Item`].
fn parse_item(value: &serde_json::Value) -> Result<Item, String> {
    let obj = value.as_object().ok_or_else(|| "each task item must be an object".to_owned())?;
    let children = match obj.get("children") {
        Some(nested) if !nested.is_null() => parse_items(nested)?,
        _ => Vec::new(),
    };
    Ok(Item {
        id: str_field(obj, "id"),
        title: str_field(obj, "title"),
        description: str_field(obj, "description"),
        status: str_field(obj, "status"),
        children,
    })
}

/// Read an optional string field from a payload object.
fn str_field(obj: &serde_json::Map<String, serde_json::Value>, key: &str) -> Option<String> {
    obj.get(key).and_then(serde_json::Value::as_str).map(str::to_owned)
}

// =============================================================================
// Entry point
// =============================================================================

/// Apply `items` to `thread_id`'s task list, returning the ids of tasks created.
///
/// Validation runs over the whole payload first; on any problem nothing is
/// mutated and the error names every issue at once.
///
/// # Errors
/// Returns `Err` when a nested item carries an `id` (children are
/// creation-only), an `id` does not resolve to a task of this thread, an `id`
/// repeats within the call, a creation is missing its `title`, or a `status`
/// string is not one of the four accepted values.
pub fn upsert(state: &mut State, thread_id: &str, items: &[Item]) -> Result<Vec<String>, String> {
    validate(state, thread_id, items)?;
    let mut created: Vec<String> = Vec::new();
    apply_group(state, items, Loc { thread_id, parent: None }, &mut created);
    renumber_orders(state, thread_id);
    Ok(created)
}

// =============================================================================
// Validation — reject bad payloads BEFORE mutating (atomic apply)
// =============================================================================

/// Accumulator threaded through the validation walk.
struct Ctx<'ctx> {
    /// Live state, read-only during validation.
    state: &'ctx State,
    /// Thread whose tasks the payload targets.
    thread_id: &'ctx str,
    /// Ids already claimed by an earlier item in this same call.
    seen: HashSet<String>,
    /// Every problem found so far (reported together).
    errors: Vec<String>,
}

/// Validate the whole payload, returning `Err` listing every problem at once.
fn validate(state: &State, thread_id: &str, items: &[Item]) -> Result<(), String> {
    let mut ctx = Ctx { state, thread_id, seen: HashSet::new(), errors: Vec::new() };
    validate_group(&mut ctx, items, false);
    if ctx.errors.is_empty() {
        return Ok(());
    }
    Err(format!("{} problem(s) in this Todo call:\n  - {}", ctx.errors.len(), ctx.errors.join("\n  - ")))
}

/// Validate one sibling group; `nested` marks everything below the top level.
fn validate_group(ctx: &mut Ctx<'_>, items: &[Item], nested: bool) {
    for item in items {
        validate_one(ctx, item, nested);
        validate_group(ctx, &item.children, true);
    }
}

/// Validate a single item's id/title/status triple.
fn validate_one(ctx: &mut Ctx<'_>, item: &Item, nested: bool) {
    match item.id.as_deref() {
        Some(id) if nested => ctx.errors.push(format!(
            "nested item `{id}`: children are creation-only \u{2014} drop its `id` (nesting alone sets the parent)"
        )),
        Some(id) => validate_existing(ctx, id),
        None => validate_create(ctx, item),
    }
    validate_status(ctx, item);
}

/// An `id` must resolve to a task of this thread, and appear only once.
fn validate_existing(ctx: &mut Ctx<'_>, id: &str) {
    let exists = TodoState::get(ctx.state).todos.iter().any(|t| t.id == id && t.thread_id == ctx.thread_id);
    if !exists {
        ctx.errors.push(format!(
            "item id `{id}` does not exist on this thread \u{2014} ids are backend-assigned; omit `id` to create"
        ));
        return;
    }
    if !ctx.seen.insert(id.to_owned()) {
        ctx.errors.push(format!("item id `{id}` appears more than once in this call"));
    }
}

/// A creation (no `id`) needs a non-empty `title`.
fn validate_create(ctx: &mut Ctx<'_>, item: &Item) {
    if item.title.as_deref().map(str::trim).is_none_or(str::is_empty) {
        ctx.errors.push("a new item (no `id`) is missing a non-empty `title`".to_owned());
    }
}

/// A provided `status` must be one of the four accepted wire values.
fn validate_status(ctx: &mut Ctx<'_>, item: &Item) {
    let Some(raw) = item.status.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
        return;
    };
    if parse_status(raw).is_none() {
        ctx.errors.push(format!("invalid status `{raw}` \u{2014} use one of: planned, in_progress, done, cancelled"));
    }
}

// =============================================================================
// Apply
// =============================================================================

/// A sibling group's location: owning thread + parent id (`None` = root group).
#[derive(Clone, Copy)]
struct Loc<'loc> {
    /// Owning thread id.
    thread_id: &'loc str,
    /// Parent task id, or `None` for the root group.
    parent: Option<&'loc str>,
}

/// Apply one sibling group, then recurse into each item's children.
fn apply_group(state: &mut State, items: &[Item], loc: Loc<'_>, created: &mut Vec<String>) {
    for item in items {
        let Some(id) = apply_one(state, item, loc, created) else {
            continue;
        };
        apply_group(state, &item.children, Loc { thread_id: loc.thread_id, parent: Some(&id) }, created);
    }
}

/// Update an identified task in place, or create a new one. Returns its id so
/// children can attach to it.
fn apply_one(state: &mut State, item: &Item, loc: Loc<'_>, created: &mut Vec<String>) -> Option<String> {
    if let Some(id) = item.id.as_deref() {
        patch(state, id, item);
        return Some(id.to_owned());
    }
    let id = create(state, item, loc)?;
    created.push(id.clone());
    Some(id)
}

/// Patch the provided fields of an existing task. Absent fields are untouched,
/// and neither parent nor sibling order ever moves (upsert never reparents).
fn patch(state: &mut State, id: &str, item: &Item) {
    let status = item.status.as_deref().and_then(parse_status);
    let ts = TodoState::get_mut(state);
    let Some(target) = ts.todos.iter_mut().find(|t| t.id == id) else {
        return;
    };
    if let Some(title) = item.title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        title.clone_into(&mut target.name);
    }
    if let Some(desc) = item.description.as_deref() {
        desc.clone_into(&mut target.description);
    }
    if let Some(s) = status {
        target.status = s;
    }
}

/// Create a task at the end of `loc`'s sibling group. Returns its fresh id.
fn create(state: &mut State, item: &Item, loc: Loc<'_>) -> Option<String> {
    let name = item.title.as_deref().map(str::trim).filter(|t| !t.is_empty())?.to_owned();
    let status = item.status.as_deref().and_then(parse_status).unwrap_or(TodoStatus::Planned);
    let description = item.description.clone().unwrap_or_default();
    let order = next_order(state, loc);
    let ts = TodoState::get_mut(state);
    let id = format!("X{}", ts.next_todo_id);
    ts.next_todo_id = ts.next_todo_id.saturating_add(1);
    ts.todos.push(TodoItem {
        id: id.clone(),
        thread_id: loc.thread_id.to_owned(),
        parent_id: loc.parent.map(str::to_owned),
        name,
        description,
        status,
        order,
    });
    Some(id)
}

/// The order a new task takes at the end of `loc`'s sibling group.
fn next_order(state: &State, loc: Loc<'_>) -> i32 {
    TodoState::get(state)
        .todos
        .iter()
        .filter(|t| t.thread_id == loc.thread_id && t.parent_id.as_deref() == loc.parent)
        .map(|t| t.order)
        .max()
        .map_or(0i32, |m| m.saturating_add(1))
}

// =============================================================================
// Order maintenance
// =============================================================================

/// One row of the renumber snapshot: (id, `parent_id`, current order, cancelled).
type OrderRow = (String, Option<String>, i32, bool);

/// Renumber `order` densely (0..n) within every parent group of `thread_id`,
/// cancelled tasks placed last — the single source of truth both the canonical
/// render and the wire projection sort by.
pub fn renumber_orders(state: &mut State, thread_id: &str) {
    let ts = TodoState::get_mut(state);
    // Snapshot the sort inputs so the borrow ends before we mutate each order.
    let mut rows: Vec<OrderRow> = ts
        .todos
        .iter()
        .filter(|t| t.thread_id == thread_id)
        .map(|t| (t.id.clone(), t.parent_id.clone(), t.order, t.status == TodoStatus::Cancelled))
        .collect();
    // Group by parent, then cancelled-last, then existing order, then id.
    rows.sort_by(|a, b| (&a.1, a.3, a.2, &a.0).cmp(&(&b.1, b.3, b.2, &b.0)));
    let mut current_parent: Option<Option<String>> = None;
    let mut next = 0i32;
    for (id, parent, _, _) in rows {
        if current_parent.as_ref() != Some(&parent) {
            current_parent = Some(parent.clone());
            next = 0i32;
        }
        if let Some(item) = ts.todos.iter_mut().find(|t| t.id == id) {
            item.order = next;
        }
        next = next.saturating_add(1);
    }
}

/// Parse a status string, or `None` when unrecognised.
fn parse_status(raw: &str) -> Option<TodoStatus> {
    raw.trim().parse().ok()
}
