//! Tool-call activity: per-agent `auto` traces, kept out of groups (T860 H8).
//!
//! A tool call belongs to the agent that ran it, not to a conversation. Each
//! agent's traces live in [`Agent::activity`] and on disk in
//! `.context-pilot/activity/<agent_id>.json`, rewritten only when changed.
//! Views that show both (TUI, web) interleave them with the agent's human
//! group by timestamp ([`timeline`]).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

use cp_base::config::constants::STORE_DIR;
use cp_base::state::runtime::State;
use cp_groups::types::GroupsState;
use serde::Serialize;

use super::{Agent, AgentMessage, AgentsState, msgs};

/// Directory (under the store dir) holding one activity file per agent.
pub const ACTIVITY_DIR: &str = "activity";

/// Path of an agent's activity file.
#[must_use]
pub fn file_path(id: &str) -> PathBuf {
    PathBuf::from(STORE_DIR).join(ACTIVITY_DIR).join(format!("{id}.json"))
}

/// On-disk body of an activity file.
#[derive(Serialize)]
struct ActivityFile<'agent> {
    /// Owning agent id.
    id: &'agent str,
    /// Ordered traces.
    messages: &'agent [AgentMessage],
}

/// [`Agent::activity_rev`] of each agent's activity file as last written / loaded.
fn clean_map() -> &'static Mutex<HashMap<String, u64>> {
    static CLEAN: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    CLEAN.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Lock [`clean_map`], recovering from poison: the map only caches revisions,
/// so a panic mid-update at worst causes one redundant rewrite, never a lost one.
fn lock_clean() -> MutexGuard<'static, HashMap<String, u64>> {
    clean_map().lock().unwrap_or_else(PoisonError::into_inner)
}

/// Serialize the activity files of agents changed since their last write.
/// Marks them clean: hand every op to the durable writer lane.
///
/// O(1) per unchanged agent: compares [`Agent::activity_rev`], never the traces.
/// An agent never loaded nor written is clean at revision 0 (nothing to save).
#[must_use]
pub fn dirty_file_ops(agents: &[Agent]) -> Vec<(PathBuf, Vec<u8>)> {
    let dirty: Vec<&Agent> = {
        let clean = lock_clean();
        agents.iter().filter(|a| clean.get(&a.id).copied().unwrap_or(0) != a.activity_rev).collect()
    };
    let mut ops = Vec::with_capacity(dirty.len());
    let mut written = Vec::with_capacity(dirty.len());
    for a in dirty {
        let Ok(bytes) = serde_json::to_vec(&ActivityFile { id: &a.id, messages: &a.activity }) else { continue };
        written.push((a.id.clone(), a.activity_rev));
        ops.push((file_path(&a.id), bytes));
    }
    lock_clean().extend(written);
    ops
}

/// Record a change to `a`'s activity for [`dirty_file_ops`].
const fn touch(a: &mut Agent) {
    a.activity_rev = a.activity_rev.wrapping_add(1);
}

/// Fill each agent's [`Agent::activity`] from its file (absent = none).
pub fn load(agents: &mut [Agent]) {
    let mut loaded = Vec::new();
    for a in agents.iter_mut() {
        let Ok(bytes) = std::fs::read(file_path(&a.id)) else { continue };
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) else { continue };
        let Some(list) = v.get("messages").and_then(|m| serde_json::from_value(m.clone()).ok()) else { continue };
        a.activity = list;
        loaded.push((a.id.clone(), a.activity_rev));
    }
    lock_clean().extend(loaded);
}

/// Move `auto` traces still stored in human groups (pre-H8 data) into their
/// agent's activity, keeping timestamp order. Idempotent. Returns how many moved.
pub fn migrate_out_of_groups(agents: &mut [Agent], groups: &mut GroupsState) -> usize {
    let mut moved = 0usize;
    for a in agents.iter_mut() {
        let Some(g) = groups.find_mut(&a.human_group_id) else { continue };
        if !g.messages.iter().any(|m| m.auto) {
            continue;
        }
        let (traces, kept): (Vec<AgentMessage>, Vec<AgentMessage>) =
            std::mem::take(&mut g.messages).into_iter().partition(|m| m.auto);
        g.messages = kept;
        moved = moved.saturating_add(traces.len());
        a.activity.extend(traces);
        a.activity.sort_by_key(|m| m.timestamp);
        touch(a);
    }
    moved
}

/// Append a trace to `agent_id`'s activity (no-op for an unknown agent).
pub fn push(state: &mut State, agent_id: &str, trace: AgentMessage) {
    if let Some(a) = AgentsState::get_mut(state).agents.iter_mut().find(|a| a.id == agent_id) {
        a.activity.push(trace);
        touch(a);
    }
}

/// `agent_id`'s human-group messages and tool traces, interleaved by
/// timestamp (a message wins a tie, so a trace never jumps ahead of it).
#[must_use]
pub fn timeline<'store>(state: &'store State, agent_id: &str) -> Vec<&'store AgentMessage> {
    merge(msgs::messages(state, agent_id), traces(state, agent_id))
}

/// Keep only `agent_id`'s traces matching `keep`; returns how many remain
/// (0 for an unknown agent).
pub fn retain<F>(state: &mut State, agent_id: &str, keep: F) -> usize
where
    F: FnMut(&AgentMessage) -> bool,
{
    AgentsState::get_mut(state).agents.iter_mut().find(|a| a.id == agent_id).map_or(0, |a| {
        let before = a.activity.len();
        a.activity.retain(keep);
        if a.activity.len() != before {
            touch(a);
        }
        a.activity.len()
    })
}

/// `agent_id`'s tool traces (empty for an unknown agent).
#[must_use]
pub fn traces<'store>(state: &'store State, agent_id: &str) -> &'store [AgentMessage] {
    AgentsState::get(state).agents.iter().find(|a| a.id == agent_id).map_or(&[][..], |a| &a.activity)
}

/// Stable merge of two timestamp-ordered lists, `first` winning ties.
fn merge<'store>(first: &'store [AgentMessage], second: &'store [AgentMessage]) -> Vec<&'store AgentMessage> {
    let mut out = Vec::with_capacity(first.len().saturating_add(second.len()));
    let (mut lhs, mut rhs) = (first.iter().peekable(), second.iter().peekable());
    loop {
        let take_first = match (lhs.peek(), rhs.peek()) {
            (Some(left), Some(right)) => left.timestamp <= right.timestamp,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => break,
        };
        out.extend(if take_first { lhs.next() } else { rhs.next() });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(ts: u64, auto: bool) -> AgentMessage {
        let mut m = if auto { AgentMessage::auto_trace("t".into()) } else { AgentMessage::user("u".into()) };
        m.timestamp = ts;
        m
    }

    #[test]
    fn merge_interleaves_by_time_message_first_on_tie() {
        let chat = [at(1, false), at(5, false)];
        let traces = [at(2, true), at(5, true), at(9, true)];
        let ts: Vec<(u64, bool)> = merge(&chat, &traces).iter().map(|m| (m.timestamp, m.auto)).collect();
        assert_eq!(ts, vec![(1, false), (2, true), (5, false), (5, true), (9, true)]);
    }

    #[test]
    fn migration_moves_traces_once() {
        let mut groups = GroupsState::new();
        let gid = groups.create_human_group("A1", "a");
        if let Some(g) = groups.find_mut(&gid) {
            g.messages = vec![at(1, false), at(2, true), at(3, false)];
        }
        let mut agents = vec![Agent { human_group_id: gid.clone(), ..Agent::new("A1".into(), "a".into()) }];
        assert_eq!(migrate_out_of_groups(&mut agents, &mut groups), 1);
        assert_eq!(migrate_out_of_groups(&mut agents, &mut groups), 0);
        assert_eq!(groups.find(&gid).map(|g| g.messages.len()), Some(2));
        assert_eq!(agents.first().map(|a| a.activity.len()), Some(1));
    }

    /// `clean_map` is process-global: each test uses its own agent ids.
    fn agent(id: &str) -> Agent {
        Agent::new(id.into(), id.into())
    }

    #[test]
    fn dirty_ops_follow_revision_not_content() {
        let mut agents = vec![agent("A9001")];
        assert!(dirty_file_ops(&agents).is_empty(), "untouched agent has nothing to save");
        if let Some(a) = agents.first_mut() {
            a.activity.push(at(1, true));
            touch(a);
        }
        assert_eq!(dirty_file_ops(&agents).len(), 1);
        assert!(dirty_file_ops(&agents).is_empty(), "written once, then clean");
    }

    #[test]
    fn poisoned_lock_still_writes() {
        let poison = std::panic::catch_unwind(|| {
            let _guard = lock_clean();
            panic!("poison clean_map");
        });
        assert!(poison.is_err());
        assert!(clean_map().is_poisoned());
        let mut agents = vec![agent("A9002")];
        if let Some(a) = agents.first_mut() {
            touch(a);
        }
        assert_eq!(dirty_file_ops(&agents).len(), 1, "poison must not drop writes");
    }
}
