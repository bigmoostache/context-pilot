//! Live thread-message emission for the main event loop (Phase 2.2 — I13).
//!
//! Split from the sibling [`bridge`](super::bridge) module so each file stays
//! under the 500-line limit. This half owns exactly one responsibility: the
//! main-loop **observe-on-change chokepoint** that appends a `MessageCreated`
//! oplog delta the instant a new thread message appears, staging its body in
//! the content-addressed body store first (the I13 body-before-reference
//! barrier).

use std::cell::Cell;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash as _, Hasher as _};

use cp_base::state::runtime::State;
use cp_mod_bridge::BridgeState;
use cp_mod_bridge::body::Stored;
use cp_mod_scratchpad::types::ScratchpadState;
use cp_mod_threads::types::{ThreadAuthor, ThreadMessage, ThreadsState};
use cp_mod_todo::types::{TodoState, TodoStatus};
use cp_wire::types::oplog::OpEntryKind;
use cp_wire::types::snapshot::notes::WireNote;
use cp_wire::types::snapshot::todo::{WireTask, WireTaskStatus};

use super::bridge::{bridge_active, emit_roster_delta};
use crate::app::App;

/// Emit a [`MessageCreated`](OpEntryKind::MessageCreated) for every thread
/// message appended since the last pass, so a new chat message reaches the
/// backend view (and the web UI) in milliseconds instead of waiting on the
/// debounced tier-② disk write.
///
/// Like [`emit_vitals`](super::bridge::emit_vitals), this is a main-loop
/// **observe-on-change chokepoint** rather than a hook scattered across the
/// (many) message-append sites: it diffs each thread's live message vector
/// against the per-thread count memoised in [`BridgeState`], so it captures
/// messages from *every* source — the agent's `Send` tool, a TUI-typed reply,
/// or a web `SendMessage` command — with one uniform path.
///
/// Each new message's body (UTF-8 JSON: author, text, timestamp, optional
/// question / file-ref) is staged in the content-addressed body store **before**
/// the referencing `MessageCreated` is journaled (the I13 barrier): a small
/// body rides the delta inline (zero hydration round-trip — the common chat
/// case), a large one spills to a durable file the backend hydrates by hash.
/// The delta itself is journalled durably-but-non-blocking ([`submit_durable`]),
/// so a message can never be silently lost yet the loop never `fsync`s (I2).
///
/// The first pass after boot **seeds** the memo from the threads already on
/// disk without emitting, so a (re)started agent does not replay its whole
/// backlog onto the oplog — only post-boot messages become deltas.
///
/// No-op when the bridge is OFF.
///
/// [`submit_durable`]: cp_oplog::service::Service::submit_durable
pub(in crate::app::run) fn emit_messages(app: &mut App) {
    if !bridge_active(&app.state) {
        return;
    }

    // First pass: record existing message counts without emitting (the cold
    // backlog rides the frontend's initial tier-② load, not the delta stream).
    let seeded = app.state.get_ext::<BridgeState>().is_some_and(|bs| bs.seeded.messages());
    if !seeded {
        let counts: Vec<(String, usize)> =
            ThreadsState::get(&app.state).threads.iter().map(|t| (t.id.clone(), t.messages.len())).collect();
        let bs = app.state.ext_mut::<BridgeState>();
        for (id, len) in counts {
            let _prev = bs.thread_msg_counts.insert(id, len);
        }
        bs.seeded.seed_messages();
        return;
    }

    // Collect messages appended since the last pass (owned, so the borrows on
    // `ThreadsState` and `BridgeState` end before we mutate state below).
    let pending: Vec<PendingMessage> = {
        let ts = ThreadsState::get(&app.state);
        let memo = &app.state.ext::<BridgeState>().thread_msg_counts;
        let mut out = Vec::new();
        for thread in &ts.threads {
            let seen = memo.get(&thread.id).copied().unwrap_or(0);
            for (idx, msg) in thread.messages.iter().enumerate().skip(seen) {
                out.push(build_pending(&thread.id, msg, idx));
            }
        }
        out
    };
    if pending.is_empty() {
        return;
    }

    for p in pending {
        emit_one_message(&app.state, &p.thread_id, &p.message_id, &p.body);
        let _prev = app.state.ext_mut::<BridgeState>().thread_msg_counts.insert(p.thread_id, p.index.saturating_add(1));
    }
}

/// One thread message staged for emission.
struct PendingMessage {
    /// Owning thread id.
    thread_id: String,
    /// Synthesised stable message id (`"{thread_id}-m{index}"`).
    message_id: String,
    /// Storage index within the thread's message vector.
    index: usize,
    /// UTF-8 JSON body the observer renders the bubble from.
    body: String,
}

/// Build the [`PendingMessage`] for the message at `idx` in `thread_id`.
///
/// The body is the JSON the maquette thread view renders directly — author
/// (so the bubble lands on the right side), text, timestamp, and any embedded
/// question / file reference.
fn build_pending(thread_id: &str, msg: &ThreadMessage, idx: usize) -> PendingMessage {
    let message_id = format!("{thread_id}-m{idx}");
    let author = if matches!(msg.author, ThreadAuthor::Assistant) { "assistant" } else { "user" };
    let body = serde_json::json!({
        "id": message_id,
        "author": author,
        "text": msg.content,
        "ts": msg.timestamp,
        "fileRef": msg.file_path,
        "auto": msg.auto,
    })
    .to_string();
    PendingMessage { thread_id: thread_id.to_owned(), message_id, index: idx, body }
}

/// Stage `body` in the content-addressed store (I13 barrier) and journal the
/// referencing [`MessageCreated`](OpEntryKind::MessageCreated) delta.
///
/// A small body is carried inline in the delta (zero hydration round-trip); a
/// large one spills to a durable file (`inline_body = None`) the backend
/// hydrates by hash. No-op when the bridge is OFF or the store is unavailable.
fn emit_one_message(state: &State, thread_id: &str, message_id: &str, body: &str) {
    let Some(bs) = state.get_ext::<BridgeState>() else {
        return;
    };
    let (Some(store), Some(boot)) = (bs.store.as_ref(), bs.boot.as_ref()) else {
        return;
    };
    let stored = match store.put(body.as_bytes()) {
        Ok(s) => s,
        Err(e) => {
            log::error!("bridge: body store put failed for {message_id}: {e:?}");
            return;
        }
    };
    let head = stored.hash();
    let inline_body = if let Stored::Inline { bytes, .. } = stored { String::from_utf8(bytes).ok() } else { None };
    boot.oplog().submit_durable(OpEntryKind::MessageCreated {
        thread_id: thread_id.to_owned(),
        message_id: message_id.to_owned(),
        head,
        inline_body,
    });
}

// ── Task-list emission (thread-owned todos → frontend, M141) ─────────────

/// Map a [`TodoStatus`] to its wire projection, or `None` for the soft-deleted
/// [`Cancelled`](TodoStatus::Cancelled) state — cancelled tasks are excluded
/// from the projection entirely (the backend is the source of truth, the
/// frontend renders verbatim).
const fn wire_task_status(status: TodoStatus) -> Option<WireTaskStatus> {
    match status {
        TodoStatus::Planned => Some(WireTaskStatus::Planned),
        TodoStatus::InProgress => Some(WireTaskStatus::InProgress),
        TodoStatus::Done => Some(WireTaskStatus::Done),
        TodoStatus::Cancelled => None,
    }
}

/// Project a thread's todo items into the read-only [`WireTask`] list the web
/// aside renders: the thread's own items **sorted by sibling order** (YAML-diff
/// rework — the backend's `order` int is the single source of truth for sibling
/// order), cancelled excluded, nesting expressed via [`WireTask::parent_id`].
fn project_thread_tasks(mut items: Vec<&cp_mod_todo::types::TodoItem>) -> Vec<WireTask> {
    // Sort by (order, id): within each parent group this yields ascending order,
    // which is all the frontend needs to render siblings correctly (it groups by
    // parent_id and preserves encounter order).
    items.sort_by(|a, b| (a.order, a.id.as_str()).cmp(&(b.order, b.id.as_str())));
    items
        .into_iter()
        .filter_map(|t| {
            wire_task_status(t.status).map(|status| WireTask {
                id: t.id.clone(),
                parent_id: t.parent_id.clone(),
                name: t.name.clone(),
                description: t.description.clone(),
                status,
            })
        })
        .collect()
}

/// Replay the agent's oplog to recover the **last per-thread task list the log
/// recorded** — i.e. exactly what the backend's view has folded.
///
/// The correct seed for the task chokepoint on the first pass after a (re)boot:
/// comparing the live projection against this (not against the live list
/// itself) means any task change that landed on disk while the bridge was down
/// but was never journaled is emitted on the very first pass — self-healing
/// disk↔oplog divergence. Returns an empty map when the bridge is OFF or the
/// replay fails.
fn oplog_roster_tasks(state: &State) -> HashMap<String, Vec<WireTask>> {
    super::oplog_roster(state)
        .map(|r| r.iter().map(|t| (t.thread_id.clone(), t.tasks.clone())).collect())
        .unwrap_or_default()
}

/// First pass after (re)boot: seed the task memo from the oplog roster (what the
/// backend view has folded), then let the diff catch any change the oplog
/// missed. No-op once already seeded.
fn seed_tasks_memo_if_needed(app: &mut App) {
    let seeded = app.state.get_ext::<BridgeState>().is_some_and(|bs| bs.seeded.tasks());
    if seeded {
        return;
    }
    let oplog_tasks = oplog_roster_tasks(&app.state);
    let bs = app.state.ext_mut::<BridgeState>();
    bs.thread_tasks.extend(oplog_tasks);
    bs.seeded.seed_tasks();
}

/// Diff each thread's live task projection against the memo; collect
/// (`thread_id`, tasks) for every thread whose list changed. A thread absent
/// from the memo is treated as having an empty list, so a thread that never had
/// tasks (and still has none) yields no spurious emission.
fn collect_task_changes(app: &App) -> Vec<(String, Vec<WireTask>)> {
    let ts = ThreadsState::get(&app.state);
    let todos = TodoState::get(&app.state);
    let memo = &app.state.ext::<BridgeState>().thread_tasks;
    let empty: Vec<WireTask> = Vec::new();
    // One pass over the todos instead of one per thread.
    let mut by_thread: HashMap<&str, Vec<&cp_mod_todo::types::TodoItem>> = HashMap::new();
    for t in &todos.todos {
        by_thread.entry(t.thread_id.as_str()).or_default().push(t);
    }
    ts.threads
        .iter()
        .filter_map(|t| {
            let live = project_thread_tasks(by_thread.remove(t.id.as_str()).unwrap_or_default());
            (memo.get(&t.id).unwrap_or(&empty) != &live).then(|| (t.id.clone(), live))
        })
        .collect()
}

/// Emit a [`TaskListChanged`](OpEntryKind::TaskListChanged) the instant any
/// thread's projected task list changes, so the backend view (and the web todo
/// aside) reflect a `Think.todo` / `todo_mark` in milliseconds.
///
/// A main-loop **observe-on-change chokepoint** mirroring
/// [`emit_thread_archived`](super::archived::emit_thread_archived): it seeds the
/// per-thread task memo from the oplog roster on the first pass (what the view
/// has folded), then falls through to the diff so any change the oplog missed
/// while the bridge was down is emitted immediately. Each delta carries the
/// thread's **complete** cancelled-excluded list (whole-list snapshot), and
/// rides the **durable** path ([`emit_roster_delta`]) — task state is
/// user-visible roster state that must never be silently lost.
///
/// No-op when the bridge is OFF.
pub(in crate::app::run) fn emit_task_lists(app: &mut App) {
    if !bridge_active(&app.state) {
        return;
    }

    seed_tasks_memo_if_needed(app);

    // Inputs unchanged since the last full diff ⇒ memo already equals every
    // live projection: skip the O(threads × todos) rebuild entirely.
    let hash = tasks_input_hash(app);
    if TASKS_INPUT_HASH.get() == Some(hash) {
        return;
    }

    for (thread_id, tasks) in collect_task_changes(app) {
        emit_roster_delta(
            &app.state,
            OpEntryKind::TaskListChanged { thread_id: thread_id.clone(), tasks: tasks.clone() },
        );
        let _prev = app.state.ext_mut::<BridgeState>().thread_tasks.insert(thread_id, tasks);
    }
    TASKS_INPUT_HASH.set(Some(hash));
}

thread_local! {
    /// Input hash at the last full task diff (`None` until the first one).
    /// Main-loop only, so a thread-local is sound; a reload resets it, which
    /// just forces one full diff.
    static TASKS_INPUT_HASH: Cell<Option<u64>> = const { Cell::new(None) };
    /// Input hash at the last full note diff (see [`TASKS_INPUT_HASH`]).
    static NOTES_INPUT_HASH: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Skip key for [`collect_task_changes`]: the todo change stamp plus the thread
/// id set. Equal key ⇒ no thread's task list can differ. O(threads), never
/// touches todo content.
fn tasks_input_hash(app: &App) -> u64 {
    let mut h = DefaultHasher::new();
    TodoState::get(&app.state).revision.hash(&mut h);
    hash_thread_ids(app, &mut h);
    h.finish()
}

/// Skip key for [`collect_note_changes`]: the scratchpad change stamp plus the
/// thread id set. Equal key ⇒ no thread's note list can differ.
fn notes_input_hash(app: &App) -> u64 {
    let mut h = DefaultHasher::new();
    ScratchpadState::get(&app.state).revision.hash(&mut h);
    hash_thread_ids(app, &mut h);
    h.finish()
}

/// Feed every thread id into `h` (thread create/delete changes the diff set).
fn hash_thread_ids(app: &App, h: &mut DefaultHasher) {
    for t in &ThreadsState::get(&app.state).threads {
        t.id.hash(h);
    }
}

// ── Note-list emission (thread-owned scratchpad → frontend, T716) ─────────

/// Project a thread's scratchpad cells into the read-only [`WireNote`] list the
/// web Notes aside renders: the thread's own cells in creation order (the
/// `scratchpad_cells` Vec is already ordered by creation), each mapped to its
/// `id`/`title`/`content`. The twin of [`project_thread_tasks`].
fn project_thread_notes(cells: Vec<&cp_mod_scratchpad::types::ScratchpadCell>) -> Vec<WireNote> {
    cells
        .into_iter()
        .map(|c| WireNote { id: c.id.clone(), title: c.title.clone(), content: c.content.clone() })
        .collect()
}

/// Replay the agent's oplog to recover the **last per-thread note list the log
/// recorded** — the correct seed for the note chokepoint's first post-boot pass
/// (mirrors [`oplog_roster_tasks`]): comparing the live projection against this
/// (not the live list itself) emits any note change that landed on disk while
/// the bridge was down but was never journaled. Empty map when the bridge is OFF
/// or the replay fails.
fn oplog_roster_notes(state: &State) -> HashMap<String, Vec<WireNote>> {
    super::oplog_roster(state)
        .map(|r| r.iter().map(|t| (t.thread_id.clone(), t.notes.clone())).collect())
        .unwrap_or_default()
}

/// First pass after (re)boot: seed the note memo from the oplog roster (what the
/// backend view has folded), then let the diff catch any change the oplog
/// missed. No-op once already seeded. The twin of [`seed_tasks_memo_if_needed`].
fn seed_notes_memo_if_needed(app: &mut App) {
    let seeded = app.state.get_ext::<BridgeState>().is_some_and(|bs| bs.seeded.notes());
    if seeded {
        return;
    }
    let oplog_notes = oplog_roster_notes(&app.state);
    let bs = app.state.ext_mut::<BridgeState>();
    bs.thread_notes.extend(oplog_notes);
    bs.seeded.seed_notes();
}

/// Diff each thread's live note projection against the memo; collect
/// (`thread_id`, notes) for every thread whose list changed. A thread absent
/// from the memo is treated as having an empty list, so a thread that never had
/// notes (and still has none) yields no spurious emission. Twin of
/// [`collect_task_changes`].
fn collect_note_changes(app: &App) -> Vec<(String, Vec<WireNote>)> {
    let ts = ThreadsState::get(&app.state);
    let scratchpad = ScratchpadState::get(&app.state);
    let memo = &app.state.ext::<BridgeState>().thread_notes;
    let empty: Vec<WireNote> = Vec::new();
    let mut by_thread: HashMap<&str, Vec<&cp_mod_scratchpad::types::ScratchpadCell>> = HashMap::new();
    for c in &scratchpad.scratchpad_cells {
        by_thread.entry(c.thread_id.as_str()).or_default().push(c);
    }
    ts.threads
        .iter()
        .filter_map(|t| {
            let live = project_thread_notes(by_thread.remove(t.id.as_str()).unwrap_or_default());
            (memo.get(&t.id).unwrap_or(&empty) != &live).then(|| (t.id.clone(), live))
        })
        .collect()
}

/// Emit a [`NotesChanged`](OpEntryKind::NotesChanged) the instant any thread's
/// projected note list changes, so the backend view (and the web Notes aside)
/// reflect a `scratchpad_create_cell` / `scratchpad_edit_cell` /
/// `scratchpad_wipe` in milliseconds. The twin of [`emit_task_lists`]: it seeds
/// the per-thread note memo from the oplog roster on the first pass, then falls
/// through to the diff. Each delta carries the thread's **complete** current
/// note list (whole-list snapshot) and rides the **durable** path — note state
/// is user-visible roster state that must never be silently lost.
///
/// No-op when the bridge is OFF.
pub(in crate::app::run) fn emit_notes(app: &mut App) {
    if !bridge_active(&app.state) {
        return;
    }

    seed_notes_memo_if_needed(app);

    let hash = notes_input_hash(app);
    if NOTES_INPUT_HASH.get() == Some(hash) {
        return;
    }

    for (thread_id, notes) in collect_note_changes(app) {
        emit_roster_delta(&app.state, OpEntryKind::NotesChanged { thread_id: thread_id.clone(), notes: notes.clone() });
        let _prev = app.state.ext_mut::<BridgeState>().thread_notes.insert(thread_id, notes);
    }
    NOTES_INPUT_HASH.set(Some(hash));
}
