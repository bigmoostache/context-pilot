//! Virtualized agent-list entries: only rows that fit the pane are built.

use super::{AgentsState, ListBuild, Rows, State, push_agent_entry, push_archive_confirm_bubble, worktree_line};

/// Push agent entries until the pane is full. Virtualized: entries past the
/// bottom would be clipped unpainted, so they are never built.
pub(super) fn push_visible_entries(lb: &mut ListBuild<'_>, state: &State, visible: &[usize], rows: &Rows) {
    let _g = crate::profile!("tv_list_entries");
    let agents = &AgentsState::get(state).agents;
    // `i` is the position in the visible slice; `real` indexes `agents`.
    for (i, &real) in visible.iter().enumerate() {
        if lb.blocks.len() >= rows.height {
            break;
        }
        let Some(agent) = agents.get(real) else {
            continue;
        };
        push_agent_entry(lb, agent, i == rows.selected, worktree_line(state, agent).as_ref());
        if rows.confirming && i == rows.selected {
            push_archive_confirm_bubble(lb.blocks, rows.viewing_archived);
        }
    }
}
