//! Thread-related helpers for the main event loop.
//!
//! Extracted from `lifecycle.rs` to keep it under the 500-line limit.
//! Contains the bridge delta emission chokepoints and the per-tool-call
//! thread-activity trace; the idle `MY_TURN` detection now lives in a
//! standard `Watcher` (`cp_mod_threads::watcher::MyTurnWatcher`).

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
pub(super) use observers::{emit_behaviour, emit_thread_focus};
pub(super) use paused::emit_thread_paused;

use crate::app::App;
use cp_base::state::runtime::State;
use cp_base::tools::ToolUse;
use cp_mod_threads::types::{FocusState, ThreadMessage, ThreadsState};
use cp_wire::types::snapshot::RosterThread;

/// Branch A + B of the incoming-message behavior, run once per main-loop tick on
/// the **focused** thread (the resident at rest, per the resident=focused
/// invariant). Splits on `was_streaming` — the agent's stream phase snapshotted
/// by the caller BEFORE `finalize_stream` runs this tick (see the call site):
///
/// - **Streaming (Branch B):** do not interrupt. For each focused-thread message
///   that is unseen (`!acknowledged`) and not yet pushed (`!has_been_pushed`),
///   mark it pushed and raise ONE non-interrupting spine notification — which
///   [`SpineState::create_notification`] injects inline mid-stream (the same path
///   the Think reminder uses). The `has_been_pushed` flag makes this fire at most
///   once per message, so a long stream never re-pushes.
/// - **Idle (Branch A):** if the focused `MY_TURN` thread has unseen messages,
///   auto-read them (acknowledge + force-refresh the Threads panel) and raise a
///   continuation nudge whose content is already in-panel — skipping the explicit
///   `Read` round-trip. When everything is already seen, this is a no-op and the
///   `IdleMyTurnDetector` watcher fires the plain "please respond" notification.
///
/// Both notifications are bound to the thread (so `Read`-ing it clears them) and
/// deduplicated on their `(kind, source)` key by the spine. Focused-thread only
/// (background `MY_TURN` threads are the dispatcher's job), so this is
/// N=1-correct and a no-op whenever the focused thread has nothing new.
pub(super) fn handle_incoming_focused_messages(app: &mut App, was_streaming: bool) {
    use cp_mod_spine::types::{NotificationType, SpineState};

    // `was_streaming` is snapshotted by the caller BEFORE `finalize_stream` runs
    // this tick. We must NOT read `app.state.stream.phase.is_streaming()` here:
    // `finalize_stream` applies the turn's `pending_done` and transitions the
    // phase to `Idle` just before this hook, so the instantaneous phase is
    // almost always `Idle` mid-turn (e.g. between the micro-turns of a blocking
    // tool chain). Using the pre-finalize snapshot restores the intended
    // "was the agent mid-turn this tick?" semantics so Branch B fires.
    if was_streaming {
        // Branch B — inline push, non-interrupting.
        if let Some(push) = cp_mod_threads::incoming::take_streaming_push(&mut app.state) {
            let cp_mod_threads::incoming::StreamingPush { tid, name, messages, truncated } = push;
            let quoted: Vec<String> = messages.iter().map(|m| format!("---\n{m}")).collect();
            let tail = if truncated {
                "Finish your current step, then respond. Some content was truncated: the full text \
                 will be in the Threads panel once you are idle."
            } else {
                "Finish your current step, then respond (no need to call Read)."
            };
            let content = format!(
                "New message(s) from the user in the focused thread \"{name}\" while you were working:\n{}\n---\n{tail}",
                quoted.join("\n")
            );
            let nid = SpineState::create_notification(
                &mut app.state,
                NotificationType::Custom,
                format!("thread_push:{tid}"),
                content,
            );
            SpineState::set_notification_thread(&mut app.state, &nid, Some(tid));
        }
    } else {
        // Branch A — idle auto-read: content is already pulled into the panel.
        if let Some(push) = cp_mod_threads::incoming::take_idle_autoread(&mut app.state) {
            let cp_mod_threads::incoming::StreamingPush { tid, name, messages, truncated } = push;
            let quoted: Vec<String> = messages.iter().map(|m| format!("---\n{m}")).collect();
            let tail = if truncated {
                "Some content was truncated: the full text is in the Threads panel. Respond directly (no need to call Read)."
            } else {
                "Respond directly (no need to call Read)."
            };
            let content = format!(
                "New message(s) from the user in the focused thread \"{name}\":\n{}\n---\n{tail}",
                quoted.join("\n")
            );
            let nid = SpineState::create_notification(
                &mut app.state,
                NotificationType::Custom,
                format!("thread_autoread:{tid}"),
                content,
            );
            SpineState::set_notification_thread(&mut app.state, &nid, Some(tid));
        }
    }
}

/// Run every bridge live-emission chokepoint for one main-loop tick — the
/// vitals, message, roster-status, focus, behaviour, archived, and paused
/// observe-on-change emitters, grouped behind one call so the loop keeps a
/// single entry point (and `lifecycle.rs` stays under the 500-line cap). Each
/// emitter is a no-op when the bridge is OFF, so this whole barge is free at
/// anchor. Order mirrors the historical inline sequence.
pub(super) fn emit_bridge_deltas(app: &mut App) {
    let (skip_roster, hash) = {
        let _g = crate::profile!("roster_gate");
        roster_gate(app)
    };
    // Each emitter gets its own level-2 perf row (`loop.threads_emit.<name>`).
    // `true` = roster emitter, skipped while the roster fingerprint is unchanged.
    let emitters: [Emitter; 9] = [
        ("emit_vitals", emit_vitals, false),
        ("emit_messages", emit_messages, true),
        ("emit_task_lists", emit_task_lists, false),
        ("emit_notes", emit_notes, false),
        ("emit_thread_status", emit_thread_status, true),
        ("emit_thread_focus", emit_thread_focus, false),
        ("emit_behaviour", emit_behaviour, false),
        ("emit_thread_archived", emit_thread_archived, true),
        ("emit_thread_paused", emit_thread_paused, true),
    ];
    for (name, emit, roster) in emitters {
        if roster && skip_roster {
            continue;
        }
        let _guard = crate::profile!(name);
        emit(app);
    }
    // Emitters never mutate threads, so `hash` still describes the live roster
    // and every roster memo now matches it.
    ROSTER_HASH.set(hash);
    // Drop the tick's replay so a later reseed (bridge recovery) reads fresh.
    drop(OPLOG_ROSTER.with(|c| c.borrow_mut().take()));
}

/// Shared, cheaply clonable oplog roster snapshot.
type RosterRc = std::rc::Rc<[RosterThread]>;

thread_local! {
    /// Oplog roster replayed at most once per `emit_bridge_deltas` tick.
    static OPLOG_ROSTER: std::cell::RefCell<Option<RosterRc>> = const { std::cell::RefCell::new(None) };
}

/// The roster the oplog last recorded (what the backend view has folded),
/// shared by the five seeders (status, archived, paused, tasks, notes).
///
/// They all seed on the same first post-boot tick; each used to replay the
/// whole oplog itself (up to ~30 ms each). Cached for the current tick only.
/// `None` when the bridge is OFF or the replay fails (callers seed nothing).
pub(super) fn oplog_roster(state: &State) -> Option<RosterRc> {
    if let Some(hit) = OPLOG_ROSTER.with(|c| c.borrow().clone()) {
        return Some(hit);
    }
    let boot = state.get_ext::<cp_mod_bridge::BridgeState>()?.boot.as_ref()?;
    let roster: RosterRc = match cp_oplog::replay::replay(&boot.entry().oplog_path) {
        Ok(recovered) => recovered.roster.into(),
        Err(e) => {
            log::warn!("bridge: oplog replay for roster seed failed: {e:?}");
            return None;
        }
    };
    OPLOG_ROSTER.with(|c| *c.borrow_mut() = Some(std::rc::Rc::clone(&roster)));
    Some(roster)
}

/// One bridge emitter, its level-2 perf row name, and whether it is a roster
/// emitter (gated by [`roster_gate`]).
type Emitter = (&'static str, fn(&mut App), bool);

thread_local! {
    /// Roster fingerprint after the last full roster pass (`None` = must run).
    /// Main-loop only; a reload resets it, forcing one full pass.
    static ROSTER_HASH: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

/// Decide whether the four roster emitters (messages, status, archived,
/// paused) can be skipped this tick, and return the fingerprint to store.
///
/// They diff only `(id, message count, status, archived, paused)` per thread
/// against memos that equal the live values after every pass. So once all four
/// memos are seeded, an unchanged fingerprint guarantees an empty diff, saving
/// four O(threads) walks with hash lookups on every loop tick.
fn roster_gate(app: &App) -> (bool, Option<u64>) {
    use std::hash::{Hash as _, Hasher as _};
    if !bridge_active(&app.state) {
        return (false, None);
    }
    let seeded = app
        .state
        .get_ext::<cp_mod_bridge::BridgeState>()
        .is_some_and(|bs| bs.seeded.messages() && bs.seeded.statuses() && bs.seeded.archived() && bs.seeded.paused());
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for t in &ThreadsState::get(&app.state).threads {
        t.id.hash(&mut h);
        t.messages.len().hash(&mut h);
        (t.status == cp_mod_threads::types::ThreadStatus::MyTurn).hash(&mut h);
        t.archived.hash(&mut h);
        t.paused.hash(&mut h);
    }
    let hash = h.finish();
    // Unseeded: run everything and store nothing, so the first pass after
    // seeding still runs in full.
    if !seeded {
        return (false, None);
    }
    (ROSTER_HASH.get() == Some(hash), Some(hash))
}

/// Append an auto **tool-activity trace** to the owner (resident) thread, if any.
///
/// Every tool call leaves a lightweight `{verb · tool — intent}` breadcrumb in
/// the conversation of the thread that is actually executing it — the
/// **resident** thread (`State.resident_thread_id`), falling back to the focused
/// pointer when no thread is resident yet. So a human watching a thread sees
/// that thread's own live work without the agent having to narrate it, even when
/// several threads run concurrently and the human is looking at a different one.
/// The message is marked
/// [`auto`](ThreadMessage::auto): it is **hidden from the agent's own context**
/// (skipped in `build_panel_content`) and rendered as a **collapsible run** in
/// the web UI and TUI rather than as a normal bubble.
///
/// Invariants this upholds:
/// - **Never changes turn or focus.** The trace is `Assistant`-authored and
///   `acknowledged` (so it can't flip the thread to `MY_TURN` or count as
///   unread), and no spine notification / `on_user_message` hook fires.
/// - **No-op when no owner thread.** No resident/focused thread ⇒ nothing
///   happens.
/// - **Skips the thread-native tools** (`Send` / `Read`): `Send` already writes
///   a real bubble (a second auto trace would double it) and `Read` is the
///   focus mechanism itself — tracing them would be self-referential noise.
///
/// The live [`emit_messages`] chokepoint picks the appended message up on the
/// next loop tick and pushes it to the backend view (and the web UI) for free,
/// since an auto message is an ordinary thread message on the wire (carrying its
/// `auto` flag).
pub(in crate::app::run) fn maybe_append_tool_activity(state: &mut State, tool: &ToolUse) {
    // Attribute the trace to the OWNER thread — the one whose runtime is
    // currently resident in `state` (the thread actually executing this tool),
    // NOT the human's on-screen focus. With N>1 these differ: a background
    // thread can run tools while the human views another thread, and its
    // breadcrumbs must land in its own conversation. Falls back to the focused
    // pointer when no thread is resident yet (cold boot / N=1 before focus).
    let Some(tid) = state.resident_thread_id.clone().or_else(|| FocusState::get(state).focused_thread_id.clone())
    else {
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
