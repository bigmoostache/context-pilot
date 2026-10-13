//! The LLM-facing agent roster: one line per agent, in tree order.

use std::fmt::Write as _;

use cp_base::state::runtime::State;

use super::{Agent, AgentsState, tree};

/// Escape a string for YAML double-quoted context: `\` → `\\`, `"` → `\"`.
#[must_use]
pub fn yaml_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Worktree folder name an agent works in; `None` at the project root.
fn worktree_name(state: &State, agent_id: &str) -> Option<String> {
    let dir = state.agent_store.get(agent_id)?.workdir.as_deref()?;
    Some(dir.file_name()?.to_string_lossy().into_owned())
}

/// One roster line: `A7 "name" — description [⎇ wt] · STATUS · 12 msgs · …`.
fn agent_line(state: &State, agent: &Agent, focused_id: &str) -> String {
    let mut line = format!("{} \"{}\"", agent.id, yaml_escape(&agent.name));
    if !agent.description.is_empty() {
        _ = write!(line, " — {}", agent.description);
    }
    if let Some(wt) = worktree_name(state, &agent.id) {
        _ = write!(line, " [⎇ {wt}]");
    }
    _ = write!(line, " · {} · {} msgs", agent.status, agent.messages.len());
    let unack = agent.messages.iter().filter(|m| !m.acknowledged).count();
    if unack > 0 {
        _ = write!(line, " · {unack} unread");
    }
    if agent.id == focused_id {
        line.push_str(" \u{b7} focused");
    }
    if agent.paused {
        line.push_str(" \u{b7} paused");
    }
    if let Some(origin) = agent.origin.as_ref() {
        _ = write!(line, " · branched from {}", origin.agent_id);
    }
    line
}

/// Write the agent roster (active agents only) into `output`.
///
/// Archived agents are invisible to the LLM (T9). Paused agents are hidden
/// too (T663), except the focused one, whose conversation is rendered below.
/// Sub-agents sit under their parent, indented two spaces per depth level.
pub fn write_agent_list(output: &mut String, state: &State, focused_id: &str) {
    let agents = &AgentsState::get(state).agents;
    let shown: Vec<usize> = agents
        .iter()
        .enumerate()
        .filter(|&(_, a)| !a.archived && (!a.paused || a.id == focused_id))
        .map(|(i, _)| i)
        .collect();
    for (i, depth) in tree::preorder(agents, &shown) {
        let Some(agent) = agents.get(i) else { continue };
        let indent = "  ".repeat(depth.saturating_add(1));
        _ = writeln!(output, "{indent}- {}", agent_line(state, agent, focused_id));
    }
}
