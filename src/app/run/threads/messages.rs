//! Live thread-message emission for the main event loop (Phase 2.2 — I13).
//!
//! Split from the sibling [`bridge`](super::bridge) module so each file stays
//! under the 500-line limit. This half owns exactly one responsibility: the
//! main-loop **observe-on-change chokepoint** that appends a `MessageCreated`
//! oplog delta the instant a new thread message appears, staging its body in
//! the content-addressed body store first (the I13 body-before-reference
//! barrier).

use cp_base::state::runtime::State;
use cp_mod_bridge::body::Stored;
use cp_mod_bridge::BridgeState;
use cp_mod_threads::types::{ThreadAuthor, ThreadMessage, ThreadsState};
use cp_wire::types::oplog::OpEntryKind;

use super::bridge::bridge_active;
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
    let seeded = app.state.get_ext::<BridgeState>().is_some_and(|bs| bs.msg_memo_seeded);
    if !seeded {
        let counts: Vec<(String, usize)> = ThreadsState::get(&app.state)
            .threads
            .iter()
            .map(|t| (t.id.clone(), t.messages.len()))
            .collect();
        let bs = app.state.ext_mut::<BridgeState>();
        for (id, len) in counts {
            let _prev = bs.thread_msg_counts.insert(id, len);
        }
        bs.msg_memo_seeded = true;
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
        let _prev = app
            .state
            .ext_mut::<BridgeState>()
            .thread_msg_counts
            .insert(p.thread_id, p.index.saturating_add(1));
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
    let author = match msg.author {
        ThreadAuthor::User => "user",
        ThreadAuthor::Assistant => "assistant",
    };
    let body = serde_json::json!({
        "id": message_id,
        "author": author,
        "text": msg.content,
        "ts": msg.timestamp,
        "question": msg.question,
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
    let inline_body = match stored {
        Stored::Inline { bytes, .. } => String::from_utf8(bytes).ok(),
        Stored::Spilled { .. } => None,
    };
    boot.oplog().submit_durable(OpEntryKind::MessageCreated {
        thread_id: thread_id.to_owned(),
        message_id: message_id.to_owned(),
        head,
        inline_body,
    });
}
