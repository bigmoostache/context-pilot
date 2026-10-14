//! Agent-list ordering for the web sidebar tree (T860).
//!
//! The `/agents` response is returned already ordered: roots newest first,
//! each followed by its sub-agents in preorder (siblings newest first too),
//! every row tagged with its `depth` (0 = root). The frontend renders the list
//! as received.

use std::collections::{HashMap, HashSet};

/// Sort key of an agent: creation time, then the `<n>` of its `A<n>` id so
/// agents created in the same millisecond keep their creation order. Sorted
/// descending: newest first.
fn sort_key(detail: &serde_json::Value) -> (u64, u64, String) {
    let created = detail.get("createdAt").and_then(serde_json::Value::as_u64).unwrap_or(0);
    let id = id_of(detail);
    let number = id.get(1..).and_then(|n| n.parse::<u64>().ok()).unwrap_or(u64::MAX);
    (created, number, id.to_owned())
}

/// The agent's id, empty when missing.
fn id_of(detail: &serde_json::Value) -> &str {
    detail.get("id").and_then(serde_json::Value::as_str).unwrap_or("")
}

/// Whether the agent is archived.
fn archived(detail: &serde_json::Value) -> bool {
    detail.get("archived").and_then(serde_json::Value::as_bool).unwrap_or(false)
}

/// The parent a row nests under, or `None` for a root. A parent that is absent
/// from the list, or on the other side of the live/archived split, makes the
/// row a root: the sidebar shows one side at a time.
fn parent_of<'list>(detail: &'list serde_json::Value, by_id: &HashMap<&str, &serde_json::Value>) -> Option<&'list str> {
    let parent = detail.get("parentAgentId").and_then(serde_json::Value::as_str)?;
    let found = by_id.get(parent)?;
    (parent != id_of(detail) && archived(found) == archived(detail)).then_some(parent)
}

/// Reorder `details` into newest-first preorder and set each row's `depth`.
///
/// Rows caught in a parent loop (never reached from a root) are appended as
/// roots, so no agent is ever dropped.
pub(crate) fn order_tree(details: &mut Vec<serde_json::Value>) {
    let mut indices: Vec<usize> = (0..details.len()).collect();
    indices.sort_by_key(|&i| std::cmp::Reverse(details.get(i).map(sort_key)));
    let (roots, children) = link(details, &indices);
    let order = preorder(details, roots.into_iter().chain(indices), &children);
    apply(details, order);
}

/// Child row indices per parent id, in sort order.
type Children<'list> = HashMap<&'list str, Vec<usize>>;

/// Split sorted `indices` into roots and per-parent child lists, both keeping
/// the sorted order.
fn link<'list>(details: &'list [serde_json::Value], indices: &[usize]) -> (Vec<usize>, Children<'list>) {
    let by_id: HashMap<&str, &serde_json::Value> = details.iter().map(|d| (id_of(d), d)).collect();
    let mut children: Children<'list> = HashMap::new();
    let mut roots: Vec<usize> = Vec::new();
    for &i in indices {
        let Some(detail) = details.get(i) else { continue };
        match parent_of(detail, &by_id) {
            Some(parent) => children.entry(parent).or_default().push(i),
            None => roots.push(i),
        }
    }
    (roots, children)
}

/// Depth-first walk from each start in turn; every row is visited once and
/// returned as `(index, depth)`.
fn preorder(
    details: &[serde_json::Value],
    starts: impl Iterator<Item = usize>,
    children: &HashMap<&str, Vec<usize>>,
) -> Vec<(usize, u64)> {
    let mut order: Vec<(usize, u64)> = Vec::with_capacity(details.len());
    let mut seen: HashSet<usize> = HashSet::new();
    let mut stack: Vec<(usize, u64)> = starts.map(|i| (i, 0u64)).collect();
    stack.reverse();
    while let Some((i, depth)) = stack.pop() {
        if !seen.insert(i) {
            continue;
        }
        order.push((i, depth));
        let kids = details.get(i).and_then(|d| children.get(id_of(d)));
        stack.extend(kids.into_iter().flatten().rev().map(|&k| (k, depth.saturating_add(1))));
    }
    order
}

/// Rebuild `details` in `order`, tagging each row with its `depth`.
fn apply(details: &mut Vec<serde_json::Value>, order: Vec<(usize, u64)>) {
    let mut slots: Vec<Option<serde_json::Value>> = details.drain(..).map(Some).collect();
    for (i, depth) in order {
        let Some(mut detail) = slots.get_mut(i).and_then(Option::take) else { continue };
        if let Some(obj) = detail.as_object_mut() {
            drop(obj.insert("depth".to_owned(), serde_json::Value::from(depth)));
        }
        details.push(detail);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(id: &str, created: u64, parent: Option<&str>, is_archived: bool) -> serde_json::Value {
        serde_json::json!({"id": id, "createdAt": created, "parentAgentId": parent, "archived": is_archived})
    }

    fn rows(details: &[serde_json::Value]) -> Vec<(String, u64)> {
        details
            .iter()
            .map(|d| (id_of(d).to_owned(), d.get("depth").and_then(serde_json::Value::as_u64).unwrap_or(99)))
            .collect()
    }

    fn pairs(expected: &[(&str, u64)]) -> Vec<(String, u64)> {
        expected.iter().map(|&(id, depth)| (id.to_owned(), depth)).collect()
    }

    #[test]
    fn roots_newest_first_with_children_in_preorder() {
        let mut list = vec![
            agent("A5", 50, Some("A1"), false),
            agent("A2", 20, None, false),
            agent("A6", 60, Some("A5"), false),
            agent("A1", 10, None, false),
            agent("A3", 30, Some("A1"), false),
        ];
        order_tree(&mut list);
        assert_eq!(rows(&list), pairs(&[("A2", 0), ("A1", 0), ("A5", 1), ("A6", 2), ("A3", 1)]));
    }

    #[test]
    fn missing_or_archived_parent_makes_a_root() {
        let mut list = vec![
            agent("A4", 40, Some("A9"), false),
            agent("A1", 10, None, true),
            agent("A3", 30, Some("A1"), false),
            agent("A2", 20, Some("A1"), true),
        ];
        order_tree(&mut list);
        assert_eq!(rows(&list), pairs(&[("A4", 0), ("A3", 0), ("A1", 0), ("A2", 1)]));
    }

    #[test]
    fn same_millisecond_newest_id_first_and_loops_are_kept() {
        let mut list = vec![
            agent("A10", 5, None, false),
            agent("A9", 5, None, false),
            agent("A7", 1, Some("A8"), false),
            agent("A8", 2, Some("A7"), false),
        ];
        order_tree(&mut list);
        assert_eq!(rows(&list), pairs(&[("A10", 0), ("A9", 0), ("A8", 0), ("A7", 1)]));
    }
}
