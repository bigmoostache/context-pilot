//! Side effects of the group tools on other agents: spine notices, panel
//! cleanup, and the scoped notification clear behind `Read`.

use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_groups::panels::{GROUP_KIND, META_GROUP_ID};
use cp_mod_spine::types::{NotificationType, SpineState};

/// Source prefix of the notices this crate posts; unique per notice so the
/// spine's (kind, source) dedup never swallows a second one.
const NOTICE_SOURCE: &str = "groups";

/// Run `f` as agent `agent_id`, then restore the executing agent. Skipped
/// when the agent has no runtime yet: its next boot reads the groups anyway.
fn as_agent(state: &mut State, agent_id: &str, f: impl FnOnce(&mut State)) {
    if state.executing_agent_id() == Some(agent_id) {
        f(state);
        return;
    }
    if !state.agent_store.contains(agent_id) {
        return;
    }
    let prev = state.executing_agent_id().map(str::to_owned);
    state.agent_store.set_executing(Some(agent_id.to_owned()));
    f(state);
    state.agent_store.set_executing(prev);
}

/// Post `text` as a spine notice in `agent_id`'s inbox, bound to that agent
/// so its `Read` of `gid` clears it.
pub fn notify(state: &mut State, gid: &str, agent_id: &str, text: &str) {
    let source = format!("{NOTICE_SOURCE}:{gid}:{agent_id}:{}", cp_base::panels::now_ms());
    as_agent(state, agent_id, |st| {
        if st.get_ext::<SpineState>().is_none() {
            return;
        }
        let nid = SpineState::create_notification(st, NotificationType::Custom, source, text.to_owned());
        SpineState::set_notification_agent(st, &nid, Some(agent_id.to_owned()));
    });
}

/// Close the `group` panel showing `gid` in each listed agent's context
/// (agents without a runtime have no panel to close).
pub fn close_panels_of(state: &mut State, gid: &str, agents: &[String]) {
    let shows = |c: &cp_base::state::context::Entry| {
        c.context_type.as_str() == GROUP_KIND && c.metadata.get(META_GROUP_ID).and_then(|v| v.as_str()) == Some(gid)
    };
    for id in agents {
        if let Some(rt) = state.agent_store.get_mut(id) {
            rt.context.retain(|c| !shows(c));
            rt.selected_context = rt.selected_context.min(rt.context.len().saturating_sub(1));
        }
    }
}

/// What a `Read` of a group has made stale for the reader.
#[derive(Debug)]
pub struct ReadScope<'scope> {
    /// The reader (the executing agent).
    pub me: &'scope str,
    /// The group read.
    pub gid: &'scope str,
    /// The group is the reader's human group.
    pub human: bool,
    /// The group holds the reader's turn (its `MY_TURN` nudge points here).
    pub holds_turn: bool,
}

/// Clear the reader's notifications that `Read(gid)` answers.
///
/// Those are this group's notices and comeback / recurring firings, its
/// `MY_TURN` nudge when the group holds the turn, and the user-message pushes
/// when it is the human group. Coucous, watchers and other groups' notices
/// stay. Returns the number removed.
pub fn clear_read(state: &mut State, scope: &ReadScope<'_>) -> usize {
    let notice = format!("{NOTICE_SOURCE}:{}:", scope.gid);
    let social = format!("social:{}:", scope.gid);
    let pushes = [format!("thread_push:{}", scope.me), format!("thread_autoread:{}", scope.me)];
    let answered = |source: &str, kind: NotificationType| {
        source.starts_with(&notice)
            || source.starts_with(&social)
            || (scope.holds_turn && source == "agents")
            || (scope.human && (kind == NotificationType::UserMessage || pushes.iter().any(|p| p == source)))
    };
    let Some(ss) = state.get_ext_mut::<SpineState>() else { return 0 };
    let before = ss.notifications.len();
    ss.notifications.retain(|n| !(n.agent_id.as_deref() == Some(scope.me) && answered(&n.source, n.kind)));
    let removed = before.saturating_sub(ss.notifications.len());
    if removed > 0 {
        state.touch_panel(Kind::SPINE);
    }
    removed
}

/// Delete `gid`'s message file on a background thread (inline if the spawn
/// fails), keeping file I/O off the main loop.
pub fn remove_file_off_loop(gid: &str) {
    let bg = gid.to_owned();
    let spawned = std::thread::Builder::new()
        .name("group-file-rm".into())
        .spawn(move || cp_groups::persist::remove_group_file(&bg));
    if spawned.is_err() {
        cp_groups::persist::remove_group_file(gid);
    }
}
