//! Ctrl+B debug reference: a compact, paste-ready text snapshot of one agent
//! (identity, turn/exec state, ⚠ reason, groups, runtime counters, recent
//! messages and tool traces) for pointing the assistant at a buggy agent.

use cp_fleet::AgentExecState;
use cp_groups::types::{GroupKind, GroupsState};
use cp_mod_agents::types::{Agent, AgentMessage};
use cp_mod_agents::view_state::FleetExecMirror;

use crate::state::State;

/// Hard cap on the whole reference (~2k tokens at ~4 chars/token).
const MAX_CHARS: usize = 8000;
/// Recent human-group messages included.
const LAST_MESSAGES: usize = 6;
/// Recent tool-call traces included.
const LAST_TRACES: usize = 10;
/// Per-message excerpt length.
const EXCERPT_CHARS: usize = 300;
/// Ellipsis appended to cut text.
const ELLIPSIS: char = '\u{2026}';
/// Marker prepended to every tool-trace message; noise in the reference.
const AUTO_PREFIX: &str = "/* auto */";

/// Char-boundary-safe excerpt, newlines flattened so each entry stays one line.
fn excerpt(text: &str, max: usize) -> String {
    let flat: String = text.chars().map(|c| if c == '\n' { ' ' } else { c }).collect();
    if flat.chars().count() <= max {
        return flat;
    }
    let mut cut: String = flat.chars().take(max).collect();
    cut.push(ELLIPSIS);
    cut
}

/// Stable label of a fleet execution state.
const fn exec_label(exec: AgentExecState) -> &'static str {
    match exec {
        AgentExecState::Idle => "Idle",
        AgentExecState::Runnable => "Runnable",
        AgentExecState::Streaming => "Streaming",
        AgentExecState::Errored => "Errored",
    }
}

/// Stable label of a group kind.
const fn kind_label(kind: GroupKind) -> &'static str {
    match kind {
        GroupKind::Human => "human",
        GroupKind::Coordination => "coordination",
        GroupKind::Custom => "custom",
    }
}

/// Header line plus one `- [ts] author: text` line per recent message.
fn push_messages(out: &mut Vec<String>, title: &str, msgs: &[AgentMessage], take: usize) {
    out.push(format!("{title} (last {} of {}):", take.min(msgs.len()), msgs.len()));
    for m in msgs.iter().skip(msgs.len().saturating_sub(take)) {
        let who = m.author_agent_id.as_deref().map_or_else(|| m.author.to_string(), str::to_owned);
        let raw = m.content.as_deref().or(m.file_path.as_deref()).unwrap_or("");
        let body = raw.trim_start().strip_prefix(AUTO_PREFIX).map_or(raw, str::trim_start);
        out.push(format!("- [{}] {who}: {}", local_time(m.timestamp), excerpt(body, EXCERPT_CHARS)));
    }
}

/// Epoch ms as local `YYYY-MM-DD HH:MM:SS`; the raw number when out of range.
fn local_time(ms: u64) -> String {
    i64::try_from(ms).ok().and_then(cp_mod_utilities::time::epoch_ms_to_local_ymd_hms).unwrap_or_else(|| ms.to_string())
}

/// Build the debug reference text for `agent`, capped at [`MAX_CHARS`].
pub(crate) fn agent_debug_ref(state: &State, agent: &Agent) -> String {
    let mut out = Vec::new();
    let cwd = std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default();
    out.push(format!("## Agent debug ref: {} \"{}\"", agent.id, agent.name));
    out.push(format!("fleet: pid={} project_root={cwd}", std::process::id()));
    if !agent.description.is_empty() {
        out.push(format!("description: {}", agent.description));
    }
    let origin = agent.origin.as_ref().map_or_else(|| "-".to_owned(), |o| format!("{}@{}", o.agent_id, o.message_ts));
    out.push(format!(
        "parent={} origin={origin} created_at={} paused={} archived={}",
        agent.parent_agent_id.as_deref().unwrap_or("-"),
        local_time(agent.created_at),
        agent.paused,
        agent.archived,
    ));
    let turn = cp_mod_agents::types::turn::status(state, &agent.id);
    let exec = exec_label(FleetExecMirror::get(state).exec_state_of(&agent.id));
    let focused = cp_mod_agents::types::FocusState::get(state).focused_agent_id.as_deref() == Some(agent.id.as_str());
    out.push(format!("turn={turn} exec={exec} focused={focused}"));
    out.push(format!("warning: {}", super::agent_warning(state, agent).unwrap_or_else(|| "none".to_owned())));
    push_runtime(&mut out, state, &agent.id);
    let groups = &GroupsState::get(state).groups;
    let holding = cp_mod_agents::types::turn::holding_groups(groups, &agent.id).join(",");
    out.push(format!("human_group={} turn_held_in=[{holding}]", agent.human_group_id));
    for g in groups.iter().filter(|g| g.member_ids.iter().any(|m| m == &agent.id)) {
        out.push(format!(
            "- group {} \"{}\" kind={} owner={} members=[{}] msgs={} substates={}",
            g.id,
            g.name,
            kind_label(g.kind),
            g.owner_agent_id,
            g.member_ids.join(","),
            g.messages.len(),
            g.social.len(),
        ));
    }
    let chat: Vec<AgentMessage> =
        cp_mod_agents::types::msgs::messages(state, &agent.id).iter().filter(|m| !m.auto).cloned().collect();
    push_messages(&mut out, "messages", &chat, LAST_MESSAGES);
    push_messages(&mut out, "tool traces", &agent.activity, LAST_TRACES);
    let mut text = out.join("\n");
    if text.chars().count() > MAX_CHARS {
        text = text.chars().take(MAX_CHARS).collect();
        text.push('\n');
        text.push(ELLIPSIS);
        text.push_str("[truncated]");
    }
    text
}

/// Per-agent runtime counters (workdir, stream, tokens, panel/message counts).
fn push_runtime(out: &mut Vec<String>, state: &State, agent_id: &str) {
    let Some(rt) = state.agent_store.get(agent_id) else {
        out.push("runtime: not resident".to_owned());
        return;
    };
    let workdir = rt.workdir.as_ref().map_or_else(|| "project root".to_owned(), |p| p.display().to_string());
    out.push(format!("workdir: {workdir}"));
    out.push(format!(
        "runtime: llm_messages={} panels={} last_stop_reason={} streaming_tool={} waiting_for_panels={}",
        rt.messages.len(),
        rt.context.len(),
        rt.last_stop_reason.as_deref().unwrap_or("-"),
        rt.streaming_tool.is_some(),
        rt.waiting_for_panels,
    ));
    out.push(format!(
        "tokens: cache_hit={} cache_miss={} uncached_in={} out={}",
        rt.cache_hit_tokens, rt.cache_miss_tokens, rt.uncached_input_tokens, rt.total_output_tokens
    ));
}
