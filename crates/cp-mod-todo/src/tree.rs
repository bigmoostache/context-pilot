//! Synthetic task-tree rendering for tool results (T686).
//!
//! Two pieces, both read-only projections of [`TodoState`] scoped to one thread:
//!
//!   1. [`render`] — an ASCII box-drawing tree of the thread's live
//!      (non-cancelled) tasks: one line per item carrying its status glyph, id
//!      and title (never the description). This is what the `Todo` tool result
//!      now shows (replacing the raw canonical YAML echo, which is still visible
//!      in the Todo panel) and what an opted-in tool's result gets appended when
//!      its `task_id` flips a task to `in_progress`.
//!   2. [`in_progress_leaf_warning`] — a nudge emitted when **more than one**
//!      in-progress *leaf* exists, where "leaf" means an `in_progress` item with
//!      no `in_progress` descendant (a parent that is in progress but has an
//!      in-progress child does NOT count — the child is the real active front).
//!
//! [`result_annex`] bundles the two (tree, then the warning if any) into the
//! single **tagged recap block** emitted at both call sites — the `Todo` tool's
//! own result and the annex appended to an opted-in tool whose `task_id` flipped
//! a task. Because there is exactly ONE producer, the block is always framed by
//! [`RECAP_OPEN`] / [`RECAP_CLOSE`], which is what lets the conversation
//! stripper find and collapse superseded recaps.

use std::fmt::Write as _;

use cp_base::state::runtime::State;

use crate::types::{TodoItem, TodoState, TodoStatus};

/// Opening sentinel of a task-recap block.
///
/// Comment syntax so it reads as machine framing rather than content, and so a
/// task title can never collide with it. Paired with [`RECAP_CLOSE`]; the two
/// delimit the byte range the conversation stripper collapses.
pub const RECAP_OPEN: &str = "<!--TODO_RECAP-->";
/// Closing sentinel of a task-recap block (see [`RECAP_OPEN`]).
pub const RECAP_CLOSE: &str = "<!--/TODO_RECAP-->";

/// The box-drawing prefixes, escaped as code points (clippy forbids non-ASCII
/// string literals). `TEE`/`ELL` open a middle/last child line; `PIPE`/`GAP`
/// are the continuation columns drawn under a non-last / last parent.
const TEE: &str = "\u{251c}\u{2500}\u{2500} "; // "├── "
/// Last-child branch prefix (`└── `).
const ELL: &str = "\u{2514}\u{2500}\u{2500} "; // "└── "
/// Continuation column under a non-last parent (`│   `).
const PIPE: &str = "\u{2502}   "; // "│   "
/// Continuation column under a last parent (blank).
const GAP: &str = "    ";

/// The **complete, tagged task-recap block** for `thread_id`: header, tree, and
/// the optional multi-in-progress-leaf warning, framed by [`RECAP_OPEN`] /
/// [`RECAP_CLOSE`].
///
/// This is the single producer of a recap — both the `Todo` tool result and the
/// `task_id` annex splice exactly this string, so the two can never drift and
/// every recap in the conversation is uniformly delimited (and therefore
/// strippable). Callers add their own framing *outside* the block: whatever they
/// prepend survives stripping, everything inside does not.
#[must_use]
pub fn result_annex(state: &State, thread_id: &str) -> String {
    let tree = render(state, thread_id);
    let body = if tree.is_empty() { "(no tasks on this thread)" } else { tree.trim_end() };
    let mut out = format!("{RECAP_OPEN}\nCurrent tasks:\n\n{body}");
    if let Some(warning) = in_progress_leaf_warning(state, thread_id) {
        let _w = write!(out, "\n\n{warning}");
    }
    let _w = write!(out, "\n{RECAP_CLOSE}");
    out
}

/// The ASCII box-drawing tree of `thread_id`'s live (non-cancelled) tasks.
///
/// Empty string when the thread has no live tasks. Siblings follow the canonical
/// [`TodoItem::order`]; nesting is by `parent_id`.
#[must_use]
pub fn render(state: &State, thread_id: &str) -> String {
    let items: Vec<&TodoItem> = live_items(state, thread_id);
    let mut out = String::new();
    render_group(&items, None, "", &mut out);
    out
}

/// Every non-cancelled task of `thread_id`.
fn live_items<'item>(state: &'item State, thread_id: &str) -> Vec<&'item TodoItem> {
    TodoState::get(state)
        .todos
        .iter()
        .filter(|t| t.thread_id == thread_id && t.status != TodoStatus::Cancelled)
        .collect()
}

/// Children of `parent` (root group when `None`), ordered by `order` then id.
fn children_of<'item>(items: &[&'item TodoItem], parent: Option<&str>) -> Vec<&'item TodoItem> {
    let mut group: Vec<&'item TodoItem> = items.iter().copied().filter(|t| t.parent_id.as_deref() == parent).collect();
    group.sort_by(|a, b| (a.order, a.id.as_str()).cmp(&(b.order, b.id.as_str())));
    group
}

/// Render one sibling group at `prefix`, recursing into each child's subtree.
fn render_group(items: &[&TodoItem], parent: Option<&str>, prefix: &str, out: &mut String) {
    let group = children_of(items, parent);
    let last_idx = group.len().saturating_sub(1);
    for (idx, item) in group.iter().enumerate() {
        let last = idx == last_idx;
        let branch = if last { ELL } else { TEE };
        let _w = writeln!(out, "{prefix}{branch}{} {} {}", glyph(item.status), item.id, item.name);
        let child_prefix = format!("{prefix}{}", if last { GAP } else { PIPE });
        render_group(items, Some(item.id.as_str()), &child_prefix, out);
    }
}

/// Compact ASCII status marker for a tree line.
const fn glyph(status: TodoStatus) -> &'static str {
    match status {
        TodoStatus::Planned => "[ ]",
        TodoStatus::InProgress => "[~]",
        TodoStatus::Done => "[x]",
        TodoStatus::Cancelled => "[/]",
    }
}

/// The warning listing every in-progress *leaf* id when there is more than one,
/// or `None`. A "leaf" is an `in_progress` item with no `in_progress` descendant.
#[must_use]
pub fn in_progress_leaf_warning(state: &State, thread_id: &str) -> Option<String> {
    let items = live_items(state, thread_id);
    let leaves: Vec<&str> = items
        .iter()
        .filter(|t| t.status == TodoStatus::InProgress && !has_in_progress_descendant(&items, t.id.as_str()))
        .map(|t| t.id.as_str())
        .collect();
    if leaves.len() <= 1 {
        return None;
    }
    Some(format!(
        "I detected multiple simultaneous **in progress** tasks: {}. \
         Don't forget if applicable to mark done items as done.",
        leaves.join(", ")
    ))
}

/// Whether any descendant of `id` (child, grandchild, …) is `in_progress`.
fn has_in_progress_descendant(items: &[&TodoItem], id: &str) -> bool {
    children_of(items, Some(id))
        .iter()
        .any(|child| child.status == TodoStatus::InProgress || has_in_progress_descendant(items, child.id.as_str()))
}
