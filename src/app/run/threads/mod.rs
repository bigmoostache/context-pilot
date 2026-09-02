//! Thread-related helpers for the main event loop.
//!
//! Extracted from `lifecycle.rs` to keep it under the 500-line limit.
//! Contains auto-`Read` injection and `MY_TURN` thread detection here; the
//! bridge command intake/application + live-vitals emission live in the
//! sibling [`bridge`] submodule (the two halves split a single file that had
//! outgrown the 500-line limit).

mod archived;
mod bridge;
mod commands;
mod messages;
mod observers;
mod paused;
mod query;
pub(super) use archived::emit_thread_archived;
pub(super) use bridge::{bridge_active, emit_thread_status, emit_vitals, poll_bridge_commands};
pub(super) use messages::{emit_messages, emit_notes, emit_task_lists};
pub(super) use observers::{emit_behaviour, emit_identity, emit_thread_focus};
pub(super) use paused::emit_thread_paused;

use crate::app::App;
use crate::app::PendingDone;
use cp_base::tools::ToolUse;
use cp_mod_threads::types::{FocusState, ThreadMessage, ThreadStatus, ThreadsState};

/// Run every bridge live-emission chokepoint for one main-loop tick — the
/// vitals, message, roster-status, focus, behaviour, archived, and paused
/// observe-on-change emitters, grouped behind one call so the loop keeps a
/// single entry point (and `lifecycle.rs` stays under the 500-line cap). Each
/// emitter is a no-op when the bridge is OFF, so this whole barge is free at
/// anchor. Order mirrors the historical inline sequence.
pub(super) fn emit_bridge_deltas(app: &mut App) {
    emit_vitals(app);
    emit_messages(app);
    emit_task_lists(app);
    emit_notes(app);
    emit_thread_status(app);
    emit_thread_focus(app);
    emit_behaviour(app);
    emit_identity(app);
    emit_thread_archived(app);
    emit_thread_paused(app);
}

/// Inject a synthetic `Read` tool call when auto-continuation fires for a
/// thread notification while the AI is unfocused.
///
/// The Read is 100% deterministic in this scenario — the AI would always call
/// it anyway. Injecting it saves a full round-trip: the AI starts streaming
/// with focus already set and the thread content visible, so it can immediately
/// `Send` its response.
///
/// # Transparency (the load-bearing property — T322)
///
/// This does **not** execute the Read itself or hand-build any messages. It
/// hands a `Read` [`ToolUse`] to the **normal tool pipeline** via
/// [`inject_tool_call`], so the injected call is processed by *exactly* the same
/// code path as an LLM-emitted Read: pre-flight, `execute_tool` dispatch,
/// callbacks, the **tempo break** (`tempo = false`), message pairing, and the
/// follow-up `continue_streaming`. The *only* difference from a real LLM Read is
/// its **origin** (the harness placed it on `pending_tools` instead of the LLM
/// stream parsing it out).
///
/// This is what fixes the stale-Threads-panel freeze: because the Read now runs
/// through the pipeline, it breaks `tempo` just like a real Read would, so the
/// panel it refreshes is no longer frozen by the idle-tick `tempo` guard.
///
/// Thread selection priority:
/// 1. Extract thread IDs from the synthetic continuation message and pick the
///    first one that's still `MY_TURN`.
/// 2. Fall back to any `MY_TURN` thread if no notification thread ID matches.
///
/// Returns `true` if a Read was injected. When `true`, the caller MUST NOT start
/// its own LLM stream — the pipeline will execute the Read on the next loop tick
/// and drive the follow-up stream itself (via `continue_streaming`). Returns
/// `false` (no injection) when the agent is already focused or no eligible
/// `MY_TURN` thread exists, in which case the caller starts the stream normally.
pub(super) fn maybe_inject_auto_read(app: &mut App) -> bool {
    // Only inject when unfocused + a MY_TURN thread exists.
    let fs = FocusState::get(&app.state);
    if fs.focused_thread_id.is_some() {
        return false;
    }

    // Extract thread IDs from the synthetic message that triggered this
    // continuation. The synthetic is the second-to-last message (before
    // the empty assistant streaming target pushed by `apply_continuation`).
    let candidate_ids: Vec<String> = app
        .state
        .messages
        .len()
        .checked_sub(2)
        .and_then(|idx| app.state.messages.get(idx))
        .filter(|m| m.role == "user" && m.content.starts_with("/* Auto-continuation:"))
        .map(|m| extract_thread_ids(&m.content))
        .unwrap_or_default();

    let ts = ThreadsState::get(&app.state);

    // Prefer the thread the notification is about; fall back to any MY_TURN.
    // Archived threads are LLM-invisible (T9) and never auto-read.
    let my_turn = candidate_ids
        .iter()
        .find_map(|tid| {
            ts.threads.iter().find(|t| t.id == *tid && !t.archived && !t.paused && t.status == ThreadStatus::MyTurn)
        })
        .or_else(|| ts.threads.iter().find(|t| !t.archived && !t.paused && t.status == ThreadStatus::MyTurn));

    let Some(thread) = my_turn else {
        return false;
    };
    let tid = thread.id.clone();

    // Build the Read ToolUse exactly as the LLM would, then hand it to the
    // normal pipeline. The pipeline does ALL the rest (focus, panel refresh,
    // tempo break, message pairing, follow-up stream) — see `inject_tool_call`.
    let tool_use = ToolUse::new(
        format!("auto_read_{tid}"),
        "Read".into(),
        serde_json::json!({
            "thread_id": tid,
            "intent": "Focus on thread",
            "verb": "Reading",
        }),
    );

    inject_tool_call(app, tool_use);
    true
}

/// Inject a **harness-originated tool call** into the normal execution pipeline,
/// making it behaviorally identical to an LLM-emitted call — the only difference
/// is the *origin*.
///
/// An LLM tool call reaches execution by two facts being true when the main loop
/// runs [`handle_tool_execution`](crate::app::run::tools::pipeline): the stream
/// is in a streaming phase, and there is a finished stream
/// ([`App::pending_done`] is `Some`) carrying one or more parsed tool calls in
/// [`App::pending_tools`]. This helper reproduces precisely that situation for a
/// harness-supplied [`ToolUse`]:
///
/// * the `tool` is pushed onto [`App::pending_tools`], and
/// * a **synthetic, all-zero** [`PendingDone`] is set (`stop_reason =
///   "tool_use"`, no token counts, no breakpoint hashes), so the pipeline's
///   guard passes and it proceeds to run the tool.
///
/// The zeroed token/cost figures are correct: a harness-injected call is **not**
/// a billed LLM turn, so `accumulate_pending_token_stats` adds nothing, and the
/// empty breakpoint-hash list means the cache engine is left untouched.
///
/// # Caller contract
///
/// The caller must ensure the stream is already in a streaming phase (e.g. via
/// [`begin_streaming`](cp_base::state::runtime::State::begin_streaming), which
/// `apply_continuation` calls) and must **not** start its own LLM stream — the
/// pipeline will execute this tool on the next loop tick and then call
/// `continue_streaming` itself, so the follow-up LLM stream begins with the
/// tool's result already in context. Starting a stream as well would race two
/// streams against one set of pending tools.
///
/// Because the call rides the *real* pipeline, it inherits every pipeline
/// behaviour transparently — pre-flight, queue interception (an injected call is
/// enqueued if a queue is active, exactly as an LLM call would be), callbacks,
/// the tempo break, and persisted `tool_call`/`tool_result` message pairing.
pub(super) fn inject_tool_call(app: &mut App, tool: ToolUse) {
    app.pending_tools.push(tool);
    // Synthetic "stream finished with a tool_use" receipt: zero tokens/cost,
    // no breakpoint hashes — see the doc comment for why each field is zero.
    let synthetic_done: PendingDone = (0, 0, 0, 0, Some("tool_use".to_owned()), Vec::new(), Vec::new(), 0, Vec::new());
    app.pending_done = Some(synthetic_done);
}

/// Handle an idle thread that has `MY_TURN` status (T692, extended T697).
///
/// Reaching the body means the agent is **idle** (the top `is_streaming` guard
/// returns otherwise), so it directly auto-injects a `Read` on the thread that
/// needs a response — the natural next tool call — and creates **no** spine
/// notification. Two cases, in priority order:
///
///  1. **Focused + idle on a `MY_TURN` thread** — a new user message landed on
///     the thread I'm parked on (T697). Auto-read *that* thread. Debounced via
///     `FocusState::notified_my_turn_id` so re-reading the focused thread (which
///     keeps focus on it) can't loop on a kept-turn / no-response idle tick; the
///     debounce is cleared on hand-back (`execute_send` `still_my_turn=false`) and
///     re-armed when a fresh user message arrives (`apply_send_message`).
///  2. **Free** (unfocused, or focused on a `THEIR_TURN` thread) with a
///     different `MY_TURN` thread waiting — auto-read it. Not debounced: the
///     injected Read re-focuses onto it (next tick it becomes case 1), which
///     prevents re-entry, so a stale/persisted `notified_my_turn_id` never
///     suppresses it.
///
/// Previously the focused-thread case fired a `focused_thread_input`
/// notification (a "go Read + ack" nudge) instead of reading directly; making
/// the detector own the focused case removes that redundant round-trip.
pub(super) fn check_my_turn_threads(app: &mut App) {
    if app.state.flags.stream.phase.is_streaming() {
        return;
    }

    let threads = ThreadsState::get(&app.state);
    let focus = FocusState::get(&app.state).focused_thread_id.clone();

    // The waiting thread to auto-read, chosen with the FOCUSED thread PREFERRED:
    // if the thread I'm parked on is itself MY_TURN (the user just messaged the
    // thread I'm sitting on), that's the one to read first (T697). Otherwise fall
    // back to any other eligible MY_TURN thread. Reaching this point means we are
    // NOT streaming (top guard), i.e. idle — so "focused on a MY_TURN thread" is
    // not "busy mid-work", it's "parked on a thread that now needs my response".
    // Archived/paused threads are LLM-invisible (T9/T371) and never count.
    let eligible = |t: &&cp_mod_threads::types::Thread| !t.archived && !t.paused && t.status == ThreadStatus::MyTurn;
    let waiting = focus
        .as_deref()
        .and_then(|f| threads.threads.iter().find(|t| t.id == f && eligible(t)))
        .or_else(|| threads.threads.iter().find(eligible));

    let Some(thread) = waiting else {
        // No MY_TURN thread — clear debounce.
        FocusState::get_mut(&mut app.state).notified_my_turn_id = None;
        return;
    };

    let tid = thread.id.clone();
    let tname = thread.name.clone();
    let is_focused_thread = Some(tid.as_str()) == focus.as_deref();

    // Case 1 — the waiting thread IS the one I'm focused on (focused + idle +
    // MY_TURN): auto-read it directly (T697). This is the case a new message on
    // the thread I'm parked on used to fire a redundant "go Read + ack"
    // notification for. It is DEBOUNCED on `notified_my_turn_id`: re-reading the
    // focused thread keeps focus on it, so without the guard a kept-turn or
    // no-response idle tick would re-read it forever. The debounce is cleared
    // when I hand the thread back (`execute_send` still_my_turn=false) and
    // re-armed when a new user message lands (`apply_send_message`), so each
    // fresh user message triggers exactly one auto-read.
    if is_focused_thread {
        if FocusState::get(&app.state).notified_my_turn_id.as_deref() == Some(tid.as_str()) {
            return;
        }
        FocusState::get_mut(&mut app.state).notified_my_turn_id = Some(tid.clone());
        inject_direct_auto_read(app, &tid, &tname);
        return;
    }

    // Case 2 — I'm free (focused thread is not MY_TURN, or unfocused) and a
    // DIFFERENT thread is MY_TURN → auto-Read it directly, no notification. The
    // debounce is deliberately NOT read as a guard here: re-entry is prevented by
    // the `is_streaming` check at the top (this begins streaming) and by the
    // injected Read re-focusing onto that thread (so next tick it is the focused
    // thread, handled by Case 1's debounce), so a stale/persisted
    // `notified_my_turn_id` (e.g. carried across a reload) must never suppress it.
    FocusState::get_mut(&mut app.state).notified_my_turn_id = Some(tid.clone());
    inject_direct_auto_read(app, &tid, &tname);
}

/// Directly drive an auto-Read on `tid` from the idle `MY_TURN` detector (T692).
///
/// Mirrors [`apply_continuation`](cp_mod_spine::engine)'s `SyntheticMessage`
/// sequence so the injected `Read` attaches to a valid user→assistant turn: a
/// short synthetic user anchor, an empty assistant streaming target, then
/// `begin_streaming`. The `Read` is then handed to the **normal** tool pipeline
/// via [`inject_tool_call`] — pre-flight, execution, tempo break, message
/// pairing and the follow-up `continue_streaming` all run exactly as for an
/// LLM-emitted Read. No spine notification is created: the tool call IS the
/// response. Begin-streaming-then-inject with no real LLM stream is the same
/// proven shape [`maybe_inject_auto_read`] rides on the continuation path.
fn inject_direct_auto_read(app: &mut App, tid: &str, tname: &str) {
    let anchor = format!("/* auto-read */ Thread \"{tname}\" ({tid}) is now MY_TURN — reading it to respond.");
    let _idx = app.state.push_user_message(anchor);
    let _ai = app.state.push_empty_assistant();
    app.state.begin_streaming();

    let tool_use = ToolUse::new(
        format!("auto_read_{tid}"),
        "Read".into(),
        serde_json::json!({
            "thread_id": tid,
            "intent": "Focus on thread",
            "verb": "Reading",
        }),
    );
    inject_tool_call(app, tool_use);
    app.state.flags.ui.dirty = true;
}

/// Extract thread IDs from notification content embedded in a synthetic message.
///
/// Looks for `thread_id="T..."` patterns (produced by thread input routing
/// and `check_my_turn_threads`). Returns all matches in order so the caller
/// can pick the first one that's still `MY_TURN`.
fn extract_thread_ids(content: &str) -> Vec<String> {
    let marker = "thread_id=\"";
    let mut ids = Vec::new();
    let mut search_from: usize = 0;
    while let Some(pos) = content.get(search_from..).and_then(|s| s.find(marker)) {
        let Some(start) = search_from.checked_add(pos).and_then(|v| v.checked_add(marker.len())) else {
            break;
        };
        if let Some(end_offset) = content.get(start..).and_then(|s| s.find('"')) {
            if let Some(id_str) = start.checked_add(end_offset).and_then(|end| content.get(start..end)) {
                ids.push(id_str.to_owned());
            }
            search_from = start.saturating_add(end_offset).saturating_add(1);
        } else {
            break;
        }
    }
    ids
}

/// Append an auto **tool-activity trace** to the focused thread, if any.
///
/// When the AI is focused on a thread (`FocusState.focused_thread_id`), every
/// tool call leaves a lightweight `{verb · tool — intent}` breadcrumb in that
/// thread's conversation — so a human watching the thread sees the agent's live
/// work without the agent having to narrate it. The message is marked
/// [`auto`](ThreadMessage::auto): it is **hidden from the agent's own context**
/// (skipped in `build_panel_content`) and rendered as a **collapsible run** in
/// the web UI and TUI rather than as a normal bubble.
///
/// Invariants this upholds:
/// - **Never changes turn or focus.** The trace is `Assistant`-authored and
///   `acknowledged` (so it can't flip the thread to `MY_TURN` or count as
///   unread), and no spine notification / `on_user_message` hook fires.
/// - **No-op when unfocused.** No focus ⇒ nothing happens (the user's explicit
///   contract).
/// - **Skips the thread-native tools** (`Send` / `Read`): `Send` already writes
///   a real bubble (a second auto trace would double it) and `Read` is the
///   focus mechanism itself — tracing them would be self-referential noise.
///
/// The live [`emit_messages`] chokepoint picks the appended message up on the
/// next loop tick and pushes it to the backend view (and the web UI) for free,
/// since an auto message is an ordinary thread message on the wire (carrying its
/// `auto` flag).
pub(in crate::app::run) fn maybe_append_tool_activity(state: &mut cp_base::state::runtime::State, tool: &ToolUse) {
    let Some(tid) = FocusState::get(state).focused_thread_id.clone() else {
        return;
    };
    // Thread-native tools are excluded — see the doc comment.
    if matches!(tool.name.as_str(), "Send" | "Read") {
        return;
    }

    let verb = tool.input.get("verb").and_then(serde_json::Value::as_str).unwrap_or("");
    let intent = tool.input.get("intent").and_then(serde_json::Value::as_str).unwrap_or("");
    let line = format!("/* auto */ {verb} · {tool_name} — {intent}", tool_name = tool.name);

    let ts = ThreadsState::get_mut(state);
    if let Some(thread) = ts.threads.iter_mut().find(|t| t.id == tid) {
        thread.messages.push(ThreadMessage::auto_trace(line));
    }
}
