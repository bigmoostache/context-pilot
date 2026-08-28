//! Virtual-YAML projection of the thread-owned task tree (YAML-diff rework).
//!
//! The `Todo` tool no longer takes a structured forest; it takes a list of
//! `{prev, new}` text diffs applied to a **virtual YAML file** that exists only
//! at apply time and in the panel. This module owns the three pieces that make
//! that work:
//!
//!   1. [`render`] — the **canonical, byte-stable** YAML of a thread's tree
//!      (feeds both the Todo panel and the tool's echo). Deterministic field
//!      order, 2-space indent, `description: |` block scalars — so the `prev`
//!      strings the model copies always match at apply time.
//!   2. [`apply_diffs`] — apply the ordered `{prev, new}` edits to the rendered
//!      buffer (search/replace, unique-`prev` like the `Edit` tool), then parse
//!      and reconcile the result, then re-render and return the fresh canonical
//!      YAML.
//!   3. `reconcile` (private) — fold a parsed tree back into [`TodoState`]
//!      **by id**: existing id → patch in place; absent id → create; a
//!      previously-present id now gone from the YAML → **soft-cancel** (sorted
//!      last in its parent group). `order` is reconstructed from sibling
//!      position here — it is never written in the YAML.

use std::collections::HashSet;
use std::fmt::Write as _;

use serde::Deserialize;

use cp_base::state::runtime::State;

use crate::types::{TodoItem, TodoState, TodoStatus};

// =============================================================================
// Parsed YAML node
// =============================================================================

/// One node of the edited virtual YAML, parsed by `serde_yaml`.
///
/// Every field is optional so a malformed-but-parseable node still loads: a
/// missing `title` on a *create* is the one hard error (caught in `reconcile`);
/// a missing `id` simply means "create". `serde_yaml` handles block scalars,
/// quoting and nesting per the YAML spec, so indentation determines nesting
/// deterministically (not heuristically) — the canonical re-render is the safety
/// net that surfaces any mis-nest to the model on the next turn.
#[derive(Debug, Default, Deserialize)]
struct YamlNode {
    /// Item id (`X{n}`); absent means "create a new item".
    #[serde(default)]
    id: Option<String>,
    /// Task title (required when creating).
    #[serde(default)]
    title: Option<String>,
    /// Status wire string (`planned`/`in_progress`/`done`/`cancelled`).
    #[serde(default)]
    status: Option<String>,
    /// Longer description (block scalar in the canonical render).
    #[serde(default)]
    description: Option<String>,
    /// Nested child nodes (the `children:` sub-list).
    #[serde(default)]
    children: Vec<Self>,
}

// =============================================================================
// Canonical render
// =============================================================================

/// The canonical YAML for `thread_id`'s task tree.
///
/// Empty string when the thread has no items. Siblings are ordered by
/// [`TodoItem::order`]; a cancelled item sorts **last** in its group
/// (soft-delete convention).
#[must_use]
pub fn render(state: &State, thread_id: &str) -> String {
    let ts = TodoState::get(state);
    let items: Vec<&TodoItem> = ts.todos.iter().filter(|t| t.thread_id == thread_id).collect();
    let mut out = String::new();
    render_group(&items, None, 0, &mut out);
    out
}

/// Sort key for one item within its sibling group: cancelled last, then by
/// `order`, then by id for a stable tie-break.
fn sibling_key(t: &TodoItem) -> (bool, i32, &str) {
    (t.status == TodoStatus::Cancelled, t.order, t.id.as_str())
}

/// Render every child of `parent` (root group when `parent` is `None`) at
/// `depth`, recursing into their own children.
fn render_group(items: &[&TodoItem], parent: Option<&str>, depth: usize, out: &mut String) {
    let mut group: Vec<&&TodoItem> = items.iter().filter(|t| t.parent_id.as_deref() == parent).collect();
    group.sort_by(|a, b| sibling_key(a).cmp(&sibling_key(b)));
    for item in group {
        render_item(items, item, depth, out);
    }
}

/// Render a single item block (`- id / status / title / description / children`)
/// then recurse into its children one level deeper.
fn render_item(items: &[&TodoItem], item: &TodoItem, depth: usize, out: &mut String) {
    let indent = "  ".repeat(depth);
    // `- ` opens the list entry; subsequent keys align under the first key.
    _ = writeln!(out, "{indent}- id: {}", item.id);
    _ = writeln!(out, "{indent}  status: {}", status_wire(item.status));
    _ = writeln!(out, "{indent}  title: {}", scalar(&item.name));
    if !item.description.is_empty() {
        _ = writeln!(out, "{indent}  description: |");
        for line in item.description.lines() {
            _ = writeln!(out, "{indent}    {line}");
        }
    }
    let has_children = items.iter().any(|c| c.parent_id.as_deref() == Some(item.id.as_str()));
    if has_children {
        _ = writeln!(out, "{indent}  children:");
        render_group(items, Some(&item.id), depth.saturating_add(1), out);
    }
}

/// The canonical wire string for a status (matches the parser's accepted forms).
const fn status_wire(status: TodoStatus) -> &'static str {
    match status {
        TodoStatus::Planned => "planned",
        TodoStatus::InProgress => "in_progress",
        TodoStatus::Done => "done",
        TodoStatus::Cancelled => "cancelled",
    }
}

/// Render a title as a canonical single-line YAML scalar.
///
/// Titles are single-line; we double-quote when the value could otherwise be
/// mis-parsed (leading/trailing space, or a character YAML treats specially in
/// plain scalars), escaping `\` and `"`. A plain-safe value is emitted bare so
/// the common case stays clean and diff-friendly.
fn scalar(s: &str) -> String {
    let needs_quote = s.is_empty()
        || s != s.trim()
        || s.contains(['"', '\'', ':', '#', '\n', '\t'])
        || s.starts_with(['-', '?', '&', '*', '!', '|', '>', '%', '@', '`', '[', ']', '{', '}', ',']);
    if needs_quote { format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")) } else { s.to_owned() }
}

// =============================================================================
// apply_diffs — the tool entry point
// =============================================================================

/// Apply the ordered `{prev, new}` text diffs to `thread_id`'s canonical YAML,
/// reconcile the result into [`TodoState`], and return the **fresh** canonical
/// YAML.
///
/// Each diff is a search/replace on the working buffer (applied in order, so a
/// later diff sees earlier results). `prev` must match **exactly once** (like
/// the `Edit` tool) — zero or multiple matches is an error naming the offending
/// diff. An **empty `prev` appends** `new` at the end of the buffer (handy for
/// adding a root item). A YAML parse failure of the final buffer is an error
/// (nothing is committed). On success the thread's tasks are updated and the
/// re-rendered canonical YAML is returned.
///
/// # Errors
/// Returns `Err` with a human-readable message when a `prev` is not found /
/// ambiguous, or the resulting buffer is not valid YAML. State is left
/// untouched on any error.
pub fn apply_diffs(state: &mut State, thread_id: &str, diffs: &[(String, String)]) -> Result<String, String> {
    let mut buffer = render(state, thread_id);
    for (idx, diff) in diffs.iter().enumerate() {
        apply_one_diff(&mut buffer, diff.0.as_str(), diff.1.as_str(), idx)?;
    }
    let nodes: Vec<YamlNode> = serde_yaml::from_str(&buffer).map_err(|e| format!("resulting YAML is invalid: {e}"))?;
    reconcile(state, thread_id, &nodes);
    Ok(render(state, thread_id))
}

/// Apply one search/replace diff to `buffer`. Empty `prev` appends `new`.
fn apply_one_diff(buffer: &mut String, prev: &str, new: &str, idx: usize) -> Result<(), String> {
    if prev.is_empty() {
        if !buffer.is_empty() && !buffer.ends_with('\n') {
            buffer.push('\n');
        }
        buffer.push_str(new);
        return Ok(());
    }
    let count = buffer.matches(prev).count();
    match count {
        0 => Err(format!("diff #{}: 'prev' not found in the current YAML", idx.saturating_add(1))),
        1 => {
            *buffer = buffer.replacen(prev, new, 1);
            Ok(())
        }
        n => Err(format!(
            "diff #{}: 'prev' matches {n} places — make it unique (include the item's `id:` line)",
            idx.saturating_add(1)
        )),
    }
}

// =============================================================================
// reconcile — fold parsed tree back into TodoState by id
// =============================================================================

/// A sibling group's location: owning thread + parent id (`None` = root group).
#[derive(Clone, Copy)]
struct Loc<'loc> {
    /// Owning thread id.
    thread_id: &'loc str,
    /// Parent item id, or `None` for the root group.
    parent: Option<&'loc str>,
}

/// One node's slot: its group location plus its positional order among siblings.
#[derive(Clone, Copy)]
struct Slot<'slot> {
    /// The group this node belongs to.
    loc: Loc<'slot>,
    /// Zero-based index among its siblings in the YAML (its `order`).
    order: i32,
}

/// Accumulator threaded through the reconcile walk.
struct ReconcileCtx {
    /// Ids present in the incoming YAML that resolved to an existing item
    /// (updated) or were freshly created — everything NOT here is soft-cancelled.
    seen: HashSet<String>,
}

/// Fold the parsed `nodes` tree back into `thread_id`'s tasks **by id**.
///
/// Existing id → patch (title/status/description/parent/order); absent id →
/// create (next `X{n}`); a previously-present id now missing from the YAML →
/// soft-cancel. `order` is the node's index among its siblings in the YAML.
fn reconcile(state: &mut State, thread_id: &str, nodes: &[YamlNode]) {
    let mut ctx = ReconcileCtx { seen: HashSet::new() };
    reconcile_group(state, nodes, Loc { thread_id, parent: None }, &mut ctx);
    cancel_unseen(state, thread_id, &ctx.seen);
    renumber_orders(state, thread_id);
}

/// Reconcile one sibling group (children of `loc.parent`), assigning each node
/// its positional `order`, then recursing into its own children.
fn reconcile_group(state: &mut State, nodes: &[YamlNode], loc: Loc<'_>, ctx: &mut ReconcileCtx) {
    for (idx, node) in nodes.iter().enumerate() {
        let order = i32::try_from(idx).unwrap_or(i32::MAX);
        let resolved = reconcile_node(state, node, Slot { loc, order }, ctx);
        if let Some(id) = resolved {
            reconcile_group(state, &node.children, Loc { thread_id: loc.thread_id, parent: Some(&id) }, ctx);
        }
    }
}

/// Reconcile one node: update an existing id in place, or create a new item.
/// Returns the item's id (so its children can attach), or `None` on a bad node.
fn reconcile_node(state: &mut State, node: &YamlNode, slot: Slot<'_>, ctx: &mut ReconcileCtx) -> Option<String> {
    let thread_id = slot.loc.thread_id;
    let existing_id = node
        .id
        .as_deref()
        .filter(|id| TodoState::get(state).todos.iter().any(|t| &t.id == id && t.thread_id == thread_id));

    if let Some(id) = existing_id {
        let owned_id = id.to_owned();
        patch_item(state, &owned_id, node, slot);
        let _inserted = ctx.seen.insert(owned_id.clone());
        return Some(owned_id);
    }

    // Create: title is required (an id that didn't resolve falls through here).
    let name = node.title.as_deref().map(str::trim).filter(|t| !t.is_empty())?;
    let status = node.status.as_deref().and_then(parse_status);
    let ts = TodoState::get_mut(state);
    let id = format!("X{}", ts.next_todo_id);
    ts.next_todo_id = ts.next_todo_id.saturating_add(1);
    ts.todos.push(TodoItem {
        id: id.clone(),
        thread_id: thread_id.to_owned(),
        parent_id: slot.loc.parent.map(str::to_owned),
        name: name.to_owned(),
        description: node.description.clone().unwrap_or_default(),
        status: status.unwrap_or(TodoStatus::Planned),
        order: slot.order,
    });
    let _inserted = ctx.seen.insert(id.clone());
    Some(id)
}

/// Apply an in-place field patch to an existing item (title/status/description/
/// parent/order). Status is derived from the node here.
fn patch_item(state: &mut State, id: &str, node: &YamlNode, slot: Slot<'_>) {
    let status = node.status.as_deref().and_then(parse_status);
    let ts = TodoState::get_mut(state);
    let Some(item) = ts.todos.iter_mut().find(|t| t.id == id) else {
        return;
    };
    if let Some(title) = node.title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        title.clone_into(&mut item.name);
    }
    // Description is authoritative from the YAML: an omitted key clears it (the
    // canonical render always emits a non-empty description, so omission is a
    // deliberate delete), a present block replaces it.
    item.description = node.description.clone().unwrap_or_default();
    item.parent_id = slot.loc.parent.map(str::to_owned);
    item.order = slot.order;
    if let Some(s) = status {
        item.status = s;
    }
}

/// Soft-cancel every non-cancelled item of `thread_id` whose id is absent from
/// `seen` (removed from the edited YAML). Cancelled items sort last in their
/// group via [`renumber_orders`].
fn cancel_unseen(state: &mut State, thread_id: &str, seen: &HashSet<String>) {
    let ts = TodoState::get_mut(state);
    for item in &mut ts.todos {
        if item.thread_id == thread_id && item.status != TodoStatus::Cancelled && !seen.contains(&item.id) {
            item.status = TodoStatus::Cancelled;
        }
    }
}

/// One row of the renumber snapshot: (id, `parent_id`, current order, cancelled).
type OrderRow = (String, Option<String>, i32, bool);

/// Renumber `order` densely (0..n) within every parent group of `thread_id`,
/// cancelled items placed last — the single source of truth both [`render`] and
/// the wire projection sort by.
fn renumber_orders(state: &mut State, thread_id: &str) {
    let ts = TodoState::get_mut(state);
    // Snapshot the sort inputs (id, parent, order, cancelled) so the borrow ends
    // before we mutate each item's order.
    let mut rows: Vec<OrderRow> = ts
        .todos
        .iter()
        .filter(|t| t.thread_id == thread_id)
        .map(|t| (t.id.clone(), t.parent_id.clone(), t.order, t.status == TodoStatus::Cancelled))
        .collect();
    rows.sort_by(|a, b| {
        // group by parent, then cancelled-last, then existing order, then id.
        (&a.1, a.3, a.2, &a.0).cmp(&(&b.1, b.3, b.2, &b.0))
    });
    // Walk groups in the sorted order, assigning 0..n per parent.
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

/// Parse a status string (canonical wire value or an ergonomics alias), or
/// `None` for an unrecognised value (the item keeps its prior status).
fn parse_status(raw: &str) -> Option<TodoStatus> {
    raw.trim().parse().ok()
}
