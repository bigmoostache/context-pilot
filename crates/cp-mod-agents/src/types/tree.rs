//! Agent hierarchy: `parent_agent_id` links form a forest (no cycles, bounded
//! depth). Root agents (created by the user) have no parent.

use super::Agent;

/// Default maximum depth of a sub-agent below its root (root = depth 0).
pub const DEFAULT_MAX_DEPTH: usize = 2;

/// Why a parent link was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkError {
    /// The parent id names no agent.
    UnknownAgent,
    /// The link would make an agent its own ancestor.
    Cycle,
    /// The child (or its subtree) would sit deeper than the cap.
    TooDeep {
        /// The configured cap.
        max: usize,
    },
}

impl std::fmt::Display for LinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::UnknownAgent => f.write_str("unknown parent agent"),
            Self::Cycle => f.write_str("an agent cannot be its own ancestor"),
            Self::TooDeep { max } => write!(f, "sub-agent depth is capped at {max}"),
        }
    }
}

/// The agent with this id.
fn find<'agents>(agents: &'agents [Agent], id: &str) -> Option<&'agents Agent> {
    agents.iter().find(|a| a.id == id)
}

/// Parent id of `id`, if both exist.
fn parent_of<'agents>(agents: &'agents [Agent], id: &str) -> Option<&'agents str> {
    find(agents, id)?.parent_agent_id.as_deref()
}

/// Depth of `id` below its root (root = 0). A dangling parent counts as a root.
#[must_use]
pub fn depth(agents: &[Agent], id: &str) -> usize {
    let mut d: usize = 0;
    let mut cur = parent_of(agents, id);
    // The `d <= len` bound stops on corrupted (cyclic) data.
    while let Some(p) = cur.filter(|_| d <= agents.len()) {
        d = d.saturating_add(1);
        cur = parent_of(agents, p);
    }
    d
}

/// Direct children of `id`, in list order.
#[must_use]
pub fn children<'agents>(agents: &'agents [Agent], id: &str) -> Vec<&'agents Agent> {
    agents.iter().filter(|a| a.parent_agent_id.as_deref() == Some(id)).collect()
}

/// Whether `ancestor` is `id` or one of its ancestors.
#[must_use]
pub fn is_ancestor_or_self(agents: &[Agent], ancestor: &str, id: &str) -> bool {
    let mut cur = Some(id);
    let mut steps: usize = 0;
    while let Some(c) = cur.filter(|_| steps <= agents.len()) {
        if c == ancestor {
            return true;
        }
        steps = steps.saturating_add(1);
        cur = parent_of(agents, c);
    }
    false
}

/// Height of the subtree under `id` (a leaf = 0); `budget` bounds recursion.
fn height(agents: &[Agent], id: &str, budget: usize) -> usize {
    let Some(left) = budget.checked_sub(1) else { return 0 };
    children(agents, id).iter().map(|c| height(agents, &c.id, left).saturating_add(1)).max().unwrap_or(0)
}

/// Check that `child` may hang under `parent` (`None` = make it a root).
///
/// # Errors
/// [`LinkError`] on an unknown id, a cycle, or a subtree exceeding `max_depth`.
pub fn check_parent(agents: &[Agent], child: &str, parent: Option<&str>, max_depth: usize) -> Result<(), LinkError> {
    let Some(parent_id) = parent else { return Ok(()) };
    if find(agents, parent_id).is_none() {
        return Err(LinkError::UnknownAgent);
    }
    if is_ancestor_or_self(agents, child, parent_id) {
        return Err(LinkError::Cycle);
    }
    let deepest = depth(agents, parent_id).saturating_add(1).saturating_add(height(agents, child, agents.len()));
    if deepest > max_depth {
        return Err(LinkError::TooDeep { max: max_depth });
    }
    Ok(())
}

/// Pre-order walk of the indices in `visible` (roots first, children under
/// their parent), paired with each agent's depth among the visible ones.
///
/// An agent whose parent is not visible is shown as a root.
#[must_use]
pub fn preorder(agents: &[Agent], visible: &[usize]) -> Vec<(usize, usize)> {
    let shown = |id: &str| visible.iter().any(|&i| agents.get(i).is_some_and(|a| a.id == id));
    let parent_at = |i: usize| agents.get(i)?.parent_agent_id.as_deref();
    let mut out = Vec::with_capacity(visible.len());
    let mut stack: Vec<(usize, usize)> =
        visible.iter().rev().copied().filter(|&i| parent_at(i).is_none_or(|p| !shown(p))).map(|i| (i, 0)).collect();
    while let Some((i, d)) = stack.pop() {
        if out.len() > visible.len() {
            break;
        }
        out.push((i, d));
        let Some(id) = agents.get(i).map(|a| a.id.as_str()) else { continue };
        let kids = visible.iter().rev().copied().filter(|&k| parent_at(k) == Some(id));
        stack.extend(kids.map(|k| (k, d.saturating_add(1))));
    }
    out
}

/// Tree-guide prefix per row of a [`preorder`] walk, 2 columns per depth level:
/// `│ ` while an ancestor level continues below, `├─`/`└─` on the row's own level.
/// Roots get an empty prefix.
#[must_use]
pub fn guides(depths: &[usize]) -> Vec<String> {
    // Does the level-`l` branch open at or above row `i` continue after it?
    let continues = |i: usize, l: usize| depths.iter().skip(i.saturating_add(1)).find(|&&x| x <= l) == Some(&l);
    depths
        .iter()
        .enumerate()
        .map(|(i, &d)| {
            (1..=d)
                .map(|l| match (l == d, continues(i, l)) {
                    (true, true) => "\u{251c}\u{2500}",
                    (true, false) => "\u{2514}\u{2500}",
                    (false, true) => "\u{2502} ",
                    (false, false) => "  ",
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Agents from `(id, parent)` pairs.
    fn forest(spec: &[(&str, Option<&str>)]) -> Vec<Agent> {
        spec.iter()
            .map(|&(id, p)| {
                let mut a = Agent::new(id.to_owned(), id.to_owned());
                a.parent_agent_id = p.map(str::to_owned);
                a
            })
            .collect()
    }

    /// A1 → A2 → A3, plus a lone root A4.
    fn chain() -> Vec<Agent> {
        forest(&[("A1", None), ("A2", Some("A1")), ("A3", Some("A2")), ("A4", None)])
    }

    #[test]
    fn depth_and_children() {
        let f = chain();
        assert_eq!(depth(&f, "A1"), 0);
        assert_eq!(depth(&f, "A3"), 2);
        assert_eq!(children(&f, "A1").len(), 1);
    }

    #[test]
    fn rejects_cycles() {
        let f = chain();
        assert_eq!(check_parent(&f, "A1", Some("A3"), 9), Err(LinkError::Cycle));
        assert_eq!(check_parent(&f, "A1", Some("A1"), 9), Err(LinkError::Cycle));
    }

    #[test]
    fn rejects_too_deep() {
        let f = chain();
        assert_eq!(check_parent(&f, "A4", Some("A3"), 2), Err(LinkError::TooDeep { max: 2 }));
        assert_eq!(check_parent(&f, "A1", Some("A4"), 2), Err(LinkError::TooDeep { max: 2 }));
    }

    #[test]
    fn accepts_valid_and_rejects_unknown() {
        let f = chain();
        assert_eq!(check_parent(&f, "A4", Some("A2"), 2), Ok(()));
        assert_eq!(check_parent(&f, "A3", None, 2), Ok(()));
        assert_eq!(check_parent(&f, "A4", Some("A9"), 2), Err(LinkError::UnknownAgent));
    }

    #[test]
    fn preorder_nests_and_orphans_become_roots() {
        let f = forest(&[("A1", None), ("A2", None), ("A3", Some("A1")), ("A4", Some("A3")), ("A5", Some("A2"))]);
        let all: Vec<usize> = (0..f.len()).collect();
        assert_eq!(preorder(&f, &all), vec![(0, 0), (2, 1), (3, 2), (1, 0), (4, 1)]);
        // A2 hidden (e.g. archived): its child A5 shows as a root.
        assert_eq!(preorder(&f, &[0, 2, 3, 4]), vec![(0, 0), (2, 1), (3, 2), (4, 0)]);
    }

    #[test]
    fn guides_draw_branches_and_pipes() {
        // A1 > (A3 > A4, A6), A2 > A5
        let g = guides(&[0, 1, 2, 1, 0, 1]);
        assert_eq!(
            g,
            vec!["", "\u{251c}\u{2500}", "\u{2502} \u{2514}\u{2500}", "\u{2514}\u{2500}", "", "\u{2514}\u{2500}"]
        );
    }
}
