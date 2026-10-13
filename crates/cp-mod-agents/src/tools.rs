//! Tool execution handlers for the agents module.
//!
//! Two tools: [`execute_send`] posts a message to an agent and
//! [`execute_read`] retrieves agent messages and sets focus.

use std::fmt::Write as _;
use std::time::{SystemTime, UNIX_EPOCH};

use cp_base::cast::Safe as _;
use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

use crate::types::roster::yaml_escape;
use crate::types::{AgentAuthor, AgentMessage, AgentStatus, AgentsState, msgs};

/// Truncate `s` to at most `max` bytes on a char boundary (no ellipsis).
fn clamp_bytes(s: &str, max: usize) -> String {
    if s.len() > max { s.get(..s.floor_char_boundary(max)).unwrap_or(s).to_owned() } else { s.to_owned() }
}

/// Truncate `s` to at most `max` chars, appending `…` when shortened.
fn preview_ellipsis(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}…", s.get(..s.floor_char_boundary(max.saturating_sub(3))).unwrap_or(""))
    } else {
        s.to_owned()
    }
}

/// Push `msg` onto agent `tid`, flipping status when the turn is handed back.
/// Returns whether an archived agent was resurrected by this send.
fn push_send_message(state: &mut State, tid: &str, msg: AgentMessage, still_my_turn: bool) -> bool {
    let ts = AgentsState::get_mut(state);
    let Some(agent) = ts.agents.iter_mut().find(|t| t.id == tid) else {
        return false;
    };
    // Sending to an archived agent resurrects it (matches frontend on user send).
    let unarchived = agent.archived;
    if agent.archived {
        agent.archived = false;
    }
    if !still_my_turn {
        agent.status = AgentStatus::TheirTurn;
    }
    if let Some(history) = msgs::messages_mut(state, tid) {
        history.push(msg);
    }
    unarchived
}

/// Post a message to an agent.
///
/// Creates a `AgentMessage(author=Assistant)` and appends it to the agent.
/// With `still_my_turn = false` the agent flips to `TheirTurn`; focus stays on
/// the agent either way (T683).
pub(crate) fn execute_send(tool: &ToolUse, state: &mut State) -> ToolResult {
    /// Maximum markdown content length (bytes) to prevent state/disk bloat.
    const MAX_CONTENT_BYTES: usize = 100_000;

    // Always the caller's own thread: Send has no target parameter.
    let owned_tid = executing_agent_id_or_empty(state);
    let tid = owned_tid.as_str();

    let markdown =
        tool.input.get("markdown").and_then(serde_json::Value::as_str).map(|s| clamp_bytes(s, MAX_CONTENT_BYTES));

    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis().to_u64());

    // Default true: agent keeps its turn (progress update). Set false to
    // hand the agent back to the user (delivery complete).
    let still_my_turn = tool.input.get("still_my_turn").and_then(serde_json::Value::as_bool).unwrap_or(true);

    let msg = AgentMessage {
        author: AgentAuthor::Assistant,
        author_agent_id: None,
        content: markdown,
        // Attachments ride inline ```file-upload``` blocks in the markdown.
        file_path: None,
        timestamp: now,
        acknowledged: true,
        auto: false,
        has_been_pushed: false,
    };

    // Build result message before mutating — need agent name.
    let (agent_name, msg_preview) = {
        let ts = AgentsState::get(state);
        let agent = ts.agents.iter().find(|t| t.id == tid);
        let name = agent.map_or_else(|| tid.to_owned(), |t| t.name.clone());
        let preview = msg.content.as_deref().unwrap_or_default();
        (name, preview_ellipsis(preview, 80))
    };

    // Mutate agent state: push message + conditionally flip status.
    // `unarchived` records whether this Send resurrected an archived agent,
    // surfaced in the result so the AI knows the agent is live again (T353).
    let unarchived = push_send_message(state, tid, msg, still_my_turn);

    // Refresh the static Agents panel so the just-sent message appears in the
    // LLM-facing panel content without needing a follow-up `Read`. Guarded on
    // the target existing so an invalid id can't clobber the current panel with
    // a conversation-less list view. This deliberately does NOT break tempo
    // (`preserves_tempo` stays true below): it updates the panel source + marks
    // it fresh, but never forces a full context refresh the way `Read` does.
    if AgentsState::get(state).agents.iter().any(|t| t.id == tid) {
        rebuild_agents_panel(state, tid, now);
    }

    // NOTE: `Send` deliberately does NOT move `focused_agent_id`. That pointer
    // is the HUMAN's on-screen view selection (UI-global, shared across the
    // fleet, never per-agent). A background agent finishing its turn with
    // `still_my_turn=false` would otherwise yank the human's view onto itself —
    // the "intempestive agent switch" bug. The focused agent is already the
    // focus, so for the on-screen agent this is a no-op; for a background
    // agent it must not steal focus. Focus changes only on explicit human nav.
    let suffix = if still_my_turn { " (still your turn)" } else { "" };
    let unarchived_note = if unarchived { " [agent was archived \u{2014} automatically unarchived]" } else { "" };
    let mut result = ToolResult::new(
        tool.id.clone(),
        format!("Sent to {tid} \"{agent_name}\": {msg_preview}{suffix}{unarchived_note}"),
        false,
    );
    result.preserves_tempo = true;
    result
}

/// Per-thread unacknowledged summary lines (active agents with new messages).
/// The focused agent is marked. Archived agents are LLM-invisible (T9).
fn collect_agent_summaries(state: &State, focused_tid: &str) -> Vec<String> {
    let ts = AgentsState::get(state);
    let mut summaries = Vec::new();
    for t in ts.agents.iter().filter(|t| !t.archived) {
        let unack = msgs::messages(state, &t.id).iter().filter(|m| !m.acknowledged).count();
        if unack == 0 {
            continue;
        }
        let marker = if t.id == focused_tid { " \u{2190} focused" } else { "" };
        summaries.push(format!(
            "  {id} \"{name}\" [{status}]: {unack} new{marker}",
            id = t.id,
            name = t.name,
            status = t.status,
        ));
    }
    summaries
}

/// Force the Agents panel to emit fresh THIS tick after a Read.
///
/// Deprecating the cache alone is insufficient: the freeze pass would restore
/// the previous snapshot while breath budget lasts. Setting `freeze_count` to
/// `u8::MAX` forces the Fresh branch (one-shot; Fresh resets it to 0). `u8::MAX`
/// is also the sanctioned "not frozen" sentinel (excluded by the freeze indicator).
fn force_refresh_agents_panel(state: &mut State) {
    for ctx in &mut state.agent_mut().context {
        if ctx.context_type.as_str() == Kind::AGENTS {
            ctx.cache_deprecated = true;
            ctx.freeze_count = u8::MAX;
            break;
        }
    }
}

/// Rebuild the static Agents `panel_content` for `focused_tid` and mark the
/// panel to emit fresh this tick.
///
/// `panel_content` is the LLM-facing snapshot; it is (re)generated ONLY here.
/// Shared by [`execute_read`] (read + focus) and [`execute_send`] (so a message
/// the agent just posted appears in the panel without a follow-up `Read`).
///
/// This updates the panel's SOURCE and force-refreshes it, but does NOT touch
/// `state.tempo` — a caller that preserves tempo (Send) keeps a full cache
/// refresh from firing; the new content then emits on the next tick the freeze
/// pass runs fresh (never lost, since `panel_content` is durable).
pub(crate) fn rebuild_agents_panel(state: &mut State, focused_tid: &str, now_ms: u64) {
    let panel_content = build_panel_content(state, focused_tid, now_ms);
    AgentsState::get_mut(state).panel_content = panel_content;
    force_refresh_agents_panel(state);
}

/// Refresh the Threads panel for the CALLER's own thread.
///
/// Read takes no `agent_id`: it always targets the **executing** agent (the
/// agent whose context is executing this tool), exactly like `Send`. It must
/// NOT use `FocusState::focused_agent_id` — that is the human's UI-global view
/// pointer, so at N>1 a background agent would read, acknowledge, and clear
/// notifications of whatever agent is on screen (T828). For the same reason
/// Read never writes focus. Marks the agent's messages acknowledged, rebuilds
/// the panel (agent list + own conversation), and returns a summary.
pub fn execute_read(tool: &ToolUse, state: &mut State) -> ToolResult {
    let owned_tid = executing_agent_id_or_empty(state);
    let tid = owned_tid.as_str();

    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis().to_u64());

    // --- Phase 1: Collect summary across ALL agents (before marking) ---
    let ts = AgentsState::get(state);
    let Some(target_agent) = ts.agents.iter().find(|t| t.id == tid) else {
        return ToolResult::new(tool.id.clone(), format!("Agent '{tid}' not found"), true);
    };

    let agent_name = target_agent.name.clone();
    let agent_status = target_agent.status;

    let agent_summaries = collect_agent_summaries(state, tid);

    // Count + previews of newly acknowledged messages in a single pass.
    let mut new_count: usize = 0;
    let new_msg_previews: Vec<String> = msgs::messages(state, tid)
        .iter()
        .filter(|m| !m.acknowledged)
        .inspect(|_| new_count = new_count.saturating_add(1))
        .map(|m| format!("[{}] {}", m.author, preview_ellipsis(m.content.as_deref().unwrap_or("[no text]"), 60)))
        .collect();

    // --- Phase 2: Mark all messages in target agent as acknowledged ---
    if let Some(history) = msgs::messages_mut(state, tid) {
        for msg in history.iter_mut() {
            msg.acknowledged = true;
        }
    }

    // Reading an agent clears any spine notifications bound to it — the agent
    // has now read its own thread, so its pending nudges are moot.
    let _cleared = cp_mod_spine::types::SpineState::delete_notifications_by_agent(state, tid);

    // --- Phase 4: Rebuild the roster panel and open the human group's panel,
    // which now carries the conversation (closeable, re-opened by each Read).
    rebuild_agents_panel(state, tid, now_ms);
    let human_gid = msgs::human_group_id(AgentsState::get(state), tid).unwrap_or_default().to_owned();
    if !human_gid.is_empty() {
        let _panel = cp_groups::panels::open_group_panel(state, &human_gid);
    }

    // --- Phase 5: Build lightweight tool result ---
    let result_body = build_read_result(
        state,
        &ReadResult {
            tid,
            agent_name: &agent_name,
            agent_status,
            agent_summaries: &agent_summaries,
            new_count,
            new_msg_previews: &new_msg_previews,
        },
    );

    let mut result = ToolResult::new(tool.id.clone(), result_body, false);
    // Read must NOT preserve tempo. Preserving it keeps state.tempo = true, which
    // makes the next freeze pass freeze EVERY panel (tempo short-circuits the
    // freeze decision) — including the Agents panel we just force-refreshed.
    result.preserves_tempo = false;
    result
}

/// Inputs for [`build_read_result`], bundled to keep the arg count in check.
struct ReadResult<'read> {
    /// Focused agent id.
    tid: &'read str,
    /// Focused agent display name.
    agent_name: &'read str,
    /// Focused agent turn status.
    agent_status: AgentStatus,
    /// Cross-thread unacknowledged summary lines.
    agent_summaries: &'read [String],
    /// Count of newly acknowledged messages in the focused agent.
    new_count: usize,
    /// Previews of the newly acknowledged messages.
    new_msg_previews: &'read [String],
}

/// Assemble the lightweight `execute_read` result string: focus line,
/// cross-thread unacknowledged summary, newly acknowledged previews, and a
/// pointer at the force-refreshed Agents panel with its last message.
fn build_read_result(state: &State, r: &ReadResult<'_>) -> String {
    // Point the LLM at the human group's panel, which holds the conversation.
    let gid = msgs::human_group_id(AgentsState::get(state), r.tid).unwrap_or_default();
    let group_panel_id = state
        .agent()
        .context
        .iter()
        .find(|c| {
            c.context_type.as_str() == cp_groups::panels::GROUP_KIND
                && c.metadata.get(cp_groups::panels::META_GROUP_ID).and_then(serde_json::Value::as_str) == Some(gid)
        })
        .map_or_else(|| "??".to_owned(), |c| c.id.clone());

    let last_msg_preview = msgs::messages(state, r.tid)
        .last()
        .and_then(|m| m.content.as_deref())
        .map_or_else(|| "[no messages]".to_owned(), |c| preview_ellipsis(c, 80));

    let (tid, agent_name, agent_status) = (r.tid, r.agent_name, r.agent_status);
    let mut lines = vec![format!("Agent {tid} \"{agent_name}\" [{agent_status}] — your agent, refreshed.\n")];
    if r.agent_summaries.is_empty() {
        lines.push("No unacknowledged messages across active agents.".to_owned());
    } else {
        lines.push("Unacknowledged messages:".to_owned());
        lines.extend(r.agent_summaries.iter().cloned());
    }

    if r.new_count > 0 {
        lines.push(format!("\n{} new message(s) acknowledged in {tid}:", r.new_count));
        lines.extend(r.new_msg_previews.iter().map(|p| format!("  • {p}")));
    } else {
        lines.push(format!("\nNo new messages in {tid}."));
    }

    lines.push(format!(
        "\n⟳ The group panel ({group_panel_id}, human group {gid}) is open and \
         contains the MOST RECENT conversation for {tid}. Last message: \"{last_msg_preview}\""
    ));
    lines.join("\n")
}

/// Write the focused agent's `branched_from` block: the parent id plus a note
/// telling the agent the branch's history stops at the branch point.
///
/// The agent's own LLM conversation is global (not per-agent), so it may still
/// remember work done in the parent AFTER the branch point; the note asks it to
/// treat that as belonging to the parent only.
fn write_origin(output: &mut String, agent: &crate::types::Agent) {
    let Some(origin) = agent.origin.as_ref() else {
        return;
    };
    let parent = &origin.agent_id;
    _ = writeln!(output, "  branched_from: {parent}");
    _ = writeln!(
        output,
        "  branch_note: \"Branched out of {parent} — the messages up to the branch point are copied below. \
         Anything that happened in {parent} after that point belongs to {parent} only: do not assume it \
         here unless the user brings it up.\""
    );
}

/// The id of the agent whose context currently lives in `state` — the
/// **executing** agent (the focused agent at rest, or a background agent while
/// it is being stepped). Empty before the first tick places an agent.
///
/// This is the identity the Agents panel must render for: the panel instance is
/// per-agent (its `Entry` rides the agent's runtime), so each agent's
/// panel shows the roster + *its own* conversation — a per-agent **view** over
/// the shared roster (design doc §3). Without this, every agent rendered the
/// single shared `panel_content` baked for the focused agent, so a background
/// agent's Agents panel showed the focused agent's conversation.
pub(crate) fn executing_agent_id_or_empty(state: &State) -> String {
    state.executing_agent_id().unwrap_or_default().to_owned()
}

/// Build the Agents panel content for the **executing** agent (roster list +
/// a `you:` block). This is the per-agent render source used by
/// [`AgentsPanel`](crate::panel::AgentsPanel) in place of the shared, focused-
/// thread-baked `AgentsState::panel_content`.
pub(crate) fn executing_panel_content(state: &State) -> String {
    let tid = executing_agent_id_or_empty(state);
    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis().to_u64());
    build_panel_content(state, &tid, now_ms)
}

/// Build the Agents panel content: roster plus a `you:` block for
/// `focused_tid` (id, status, human group). The conversation itself is shown
/// by the human group's `group` panel (cp-groups).
///
/// Emits **YAML-structured** output (T372) so the LLM can parse agent state
/// cleanly — matching the style of the Search result panels.
pub(crate) fn build_panel_content(state: &State, focused_tid: &str, _now_ms: u64) -> String {
    let ts = AgentsState::get(state);
    let mut output = String::from("agents:\n");
    crate::types::roster::write_agent_list(&mut output, state, focused_tid);

    // Who "you" are; the conversation itself lives in the human group's `group` panel.
    let Some(agent) = ts.agents.iter().find(|t| t.id == focused_tid) else {
        return output;
    };
    _ = writeln!(output, "\nyou:");
    _ = writeln!(output, "  agent_id: {}", agent.id);
    _ = writeln!(output, "  name: \"{}\"", yaml_escape(&agent.name));
    _ = writeln!(output, "  status: {}", agent.status);
    _ = writeln!(output, "  human_group_id: {}", agent.human_group_id);
    write_origin(&mut output, agent);
    output
}
