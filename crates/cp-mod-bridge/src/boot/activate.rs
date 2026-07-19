//! Live [`BridgeState`] assembly + the background recovery and stream-publish
//! helpers, split from `lib.rs` to keep each file within the 500-line budget.
//!
//! Where [`super::Boot`] acquires the agent's OS resources (lock, oplog, stream
//! socket, heartbeat), this module wires the *live surfaces* around a booted
//! `Boot`: it binds the tee socket, seeds the command intake, opens the body
//! store, and installs the assembled [`BridgeState`]. It also owns the
//! self-healing [`try_recover`] retry path and the [`publish_frame`] stream-tee
//! helper the module's stream hooks call.

use cp_base::state::runtime::State;

use cp_wire::types::stream::{Frame as StreamFrame, Kind as StreamKind};

use crate::BridgeState;
use crate::body::Store;
use crate::command::Intake;
use crate::tee::Tee;

use super::Boot;

/// Name of the dedicated tee socket inside the agent folder.
///
/// Separate from `stream.sock` (which Boot binds for command intake in Phase
/// 10). The tee socket carries only outbound [`StreamFrame`]s to an observing
/// backend.
const TEE_SOCKET: &str = "tee.sock";

/// Bind `tee.sock` in the agent folder and spawn the [`Tee`] publisher.
fn setup_tee(entry: &cp_wire::types::registry::Entry) -> std::io::Result<Tee> {
    let tee_path = std::path::Path::new(&entry.folder).join(TEE_SOCKET);
    let _ignored = std::fs::remove_file(&tee_path);
    let listener = std::os::unix::net::UnixListener::bind(&tee_path)?;
    Ok(Tee::spawn(listener))
}

/// Assemble the live [`BridgeState`] around a freshly-booted [`Boot`] and store
/// it on `state`.
///
/// Binds the stream tee, sets the command listener non-blocking, seeds the
/// command intake (dedup `SeenSet` from oplog replay), and opens the
/// content-addressed body store — then installs the fully-live `BridgeState`.
/// Each auxiliary resource degrades independently to `None` on failure (a
/// missing tee/intake/store only disables that live surface; the durable disk
/// path still carries the data).
///
/// Called from both the startup `init_state` Ok-path and the background
/// [`try_recover`] success-path, so a mid-session recovery brings the bridge up
/// identically to a clean boot. The fresh `BridgeState` resets the
/// observe-on-change memos (`*_memo_seeded = false`), so a recovered bridge
/// re-seeds without replaying its entire message/status backlog onto the oplog.
/// Unwrap a fallible resource setup into `Option`, logging the error and
/// degrading to `None` on failure. Keeps each live surface independent —
/// a missing tee/intake/store only disables that surface, the durable disk
/// path still carries the data.
fn or_log<T, E>(what: &str, result: Result<T, E>) -> Option<T>
where
    E: std::fmt::Debug,
{
    match result {
        Ok(v) => Some(v),
        Err(e) => {
            log::error!("bridge: {what} setup failed: {e:?}");
            None
        }
    }
}

/// Wire the live bridge surfaces around a booted [`Boot`] and install the
/// assembled [`BridgeState`] on `state` (see the module-level docs).
pub(crate) fn activate(boot: Boot, state: &mut State) {
    log::info!("bridge: activated for {} ({})", boot.id(), boot.entry().folder);

    // Bind a dedicated tee socket for live token streaming (separate from the
    // command socket in Boot).
    let tee = or_log("tee", setup_tee(boot.entry()));

    // Set the command listener to non-blocking so the main-loop poll never
    // stalls when no commander is connected.
    let _nb = boot.listener().set_nonblocking(true);

    // Seed the command intake from the oplog replay (populates the SeenSet for
    // dedup across deadman re-exec).
    let intake =
        or_log("intake", Intake::new(std::path::Path::new(&boot.entry().oplog_path), boot.cap_token().to_owned()));

    // Open the content-addressed body store under the oplog dir, so the message
    // chokepoint can durably stage bodies before referencing them (I13).
    let store = or_log("body store", Store::open(std::path::Path::new(&boot.entry().oplog_path)));

    state.set_ext(BridgeState { boot: Some(boot), tee, intake, store, ..Default::default() });
}

/// Background bridge-boot recovery — the self-healing half of the
/// inert-bridge fix.
///
/// A no-op unless the bridge is in the [`pending`](BridgeState::pending) state
/// (i.e. `CP_BRIDGE=1` but the startup boot failed, typically an
/// `AlreadyRunning` `flock` race on a fast relaunch). When pending, it makes a
/// single **fail-fast, non-blocking** boot attempt via [`Boot::try_start`]: on
/// success the bridge comes up live mid-session (sockets bound, registry
/// rewritten, heartbeat beating — web sends start working within the
/// orchestrator's next scan); on failure (the predecessor still holds the lock)
/// it stays pending for the next retry tick.
///
/// The fail-fast attempt is essential: it must never sleep out the ~2s lock
/// retry window, because this runs **on the main loop thread** — a blocking
/// attempt would stutter the UI. The caller throttles invocation (every couple
/// of seconds); when not pending, the cost is a single `bool` check.
pub fn try_recover(state: &mut State) {
    let pending = state.get_ext::<BridgeState>().is_some_and(|bs| bs.pending);
    if !pending {
        return;
    }

    let model = state.get_ext::<BridgeState>().map(|bs| bs.pending_model.clone()).unwrap_or_default();

    let Ok(folder) = std::env::current_dir() else {
        // Can't determine the folder this tick; stay pending and retry later.
        return;
    };

    match Boot::try_start(&folder, &model) {
        Ok(boot) => {
            log::error!("bridge: RECOVERED — orchestration plane is live again ({})", folder.display());
            activate(boot, state);
        }
        // Still contended (predecessor not yet dead) — remain pending, the next
        // retry tick will try again. Logged at debug to avoid spamming.
        Err(e) => log::debug!("bridge: recovery attempt deferred: {e:?}"),
    }
}

/// Build a [`StreamFrame`] from the given `kind` and publish it to the tee.
///
/// Silently returns if the bridge is OFF or the tee is absent.
///
/// The frame is tagged with the **active streaming message's id** — the id of
/// the last message in the conversation, which is precisely the assistant
/// message being built while tokens stream (the streaming pipeline appends each
/// chunk to `state.messages.last_mut()`). Carrying that id lets the frontend
/// route live `Token` frames to the right conversation bubble and reconcile the
/// streamed text against the durable `MessageCreated` entry (which references
/// the same `Message::id`). `thread_id` stays empty: main-loop streaming is the
/// agent's own conversation, not a thread reply (thread replies go through the
/// `Send` tool, not the token stream).
pub(crate) fn publish_frame(state: &mut State, kind: StreamKind) {
    // Read the active streaming message id BEFORE the mutable `ext_mut` borrow
    // (a short clone, negligible against the LLM/network cost of a chunk).
    let message_id = state.messages.last().map(|m| m.id.clone()).unwrap_or_default();

    let bs = state.ext_mut::<BridgeState>();

    let active = bs.tee.is_some() && bs.boot.is_some();
    if !active {
        return;
    }

    let seq = bs.tee_seq;
    bs.tee_seq = seq.wrapping_add(1);

    let agent_id = bs.boot.as_ref().map(|b| b.id().to_owned()).unwrap_or_default();

    let frame = StreamFrame {
        schema_version: 1,
        agent_id,
        worker_id: String::new(),
        thread_id: String::new(),
        message_id,
        seq,
        kind,
    };

    if let Some(tee) = &bs.tee {
        let _outcome = tee.publish(frame);
    }
}
