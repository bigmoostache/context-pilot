//! Canonical YAML render of a thread's task tree.
//!
//! A **read-only projection**: [`render`] turns a thread's tasks into a
//! byte-stable YAML document with deterministic field order, 2-space indent and
//! `description: |` block scalars. It feeds the Todo panel — the human- and
//! model-facing view of the tree.
//!
//! Editing lives entirely in [`crate::upsert`] (structured, nested upsert): this
//! module never mutates state and never parses YAML back.
//!
//! The item shape is `- {id}: {status}` (the id is the mapping KEY, its value
//! the status) with `title` / `description` / `children` as sibling keys.

use std::fmt::Write as _;

use cp_base::state::runtime::State;

use crate::types::{TodoItem, TodoStatus};

/// The canonical YAML for `thread_id`'s task tree.
///
/// Empty string when the thread has no items. Siblings are ordered by
/// [`TodoItem::order`]; a cancelled item sorts **last** in its group
/// (soft-delete convention).
#[must_use]
pub fn render(state: &State, thread_id: &str) -> String {
    let ts = crate::types::TodoState::get(state);
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
    // `- {id}: {status}` opens the list entry — the id is the mapping KEY, so
    // the panel reads as a compact `X106: in_progress` line. Sibling keys align
    // under it.
    _ = writeln!(out, "{indent}- {}: {}", item.id, status_wire(item.status));
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

/// The canonical wire string for a status (matches the `Todo` tool's accepted
/// `status` values).
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
/// the common case stays clean and readable.
fn scalar(s: &str) -> String {
    let needs_quote = s.is_empty()
        || s != s.trim()
        || s.contains(['"', '\'', ':', '#', '\n', '\t'])
        || s.starts_with(['-', '?', '&', '*', '!', '|', '>', '%', '@', '`', '[', ']', '{', '}', ',']);
    if needs_quote { format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")) } else { s.to_owned() }
}
