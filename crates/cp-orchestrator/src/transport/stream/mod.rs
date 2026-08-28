//! The SSE producer loop and its tail-cadence tuning.
//!
//! Split out of [`transport`](super) so the acceptor/router in `mod.rs` stays
//! within the workspace's per-file line budget. [`run_stream`] is spawned by
//! `handle_stream` once per connected subscriber: it tails one agent's oplog
//! (rev-numbered durable deltas) plus its ephemeral stream-hub frames and
//! pushes both down the SSE [`sink`](super::sse::Sink) until the client
//! disconnects.
//!
//! The cold-connect vs reconnect seeding policy (the T123 fix — a fresh
//! subscriber rides the live tail instead of replaying the whole oplog) lives
//! in [`run_stream`]'s seeding block.

pub mod sse;
// Single-use SSE upgrade tickets (I9b). They exist solely to gate the SSE
// upgrade `GET`, so they live alongside the stream machinery they protect.
pub mod ticket;
// The `GET /api/stream` landing pad (ticket redemption + ACL + producer
// hand-off), split out of the transport router for its line budget.
pub mod upgrade;
// URL query-string parser — used by the `/api/stream` upgrade handler to read
// the `agent` / `ticket` / `last_rev` params. Lives here (its sole consumer is
// `upgrade`) to keep the `transport/` folder within the 8-entry cap.
mod query;

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use cp_wire::types::oplog::OpEntry;
use cp_wire::types::stream::Frame;

use super::Backend;
use crate::tailer::Tailer;

/// Tight tail re-poll cadence for the SSE producer.
///
/// The [`OplogWaiter`](sse::OplogWaiter) wakes the producer the instant the
/// agent appends — single-digit ms on Linux (inotify) — but macOS `FSEvents`
/// coalesces filesystem notifications with a ~300 ms latency window, which
/// would otherwise floor visible latency at hundreds of ms. Capping the wait at
/// this tight value makes the producer re-poll its tailer every few ms
/// regardless of the OS event latency, so a durable delta reaches the browser
/// within ~`TAIL_REPOLL` even on macOS. On Linux the waiter still returns early
/// on the inotify event (sub-ms), so this is purely a backstop there — the
/// design doc's "inotify primary, poll backstop" contract (I12/§8.1), just with
/// a backstop tight enough to be acceptable on every platform.
const TAIL_REPOLL: Duration = Duration::from_millis(5);

/// How often the SSE producer emits a keep-alive comment.
///
/// Decoupled from [`TAIL_REPOLL`] so the tight re-poll loop does not spam the
/// client with hundreds of keep-alive comments per second. The keep-alive
/// doubles as the idle disconnect probe; on a fully idle stream a dropped
/// connection is detected within this interval (a busy stream is detected
/// immediately by the failing delta/frame write).
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(10);

/// The SSE producer loop: replay-from-`rev`, then live oplog + stream tail.
///
/// Runs until a `send` fails (the client disconnected, dropping the body
/// reader). Unsubscribes its stream-hub slot on exit.
///
/// The four connection references are bundled into [`StreamCtx`] so the
/// producer stays within the argument budget; `last_rev` is the one per-run
/// scalar (the client's `Last-Event-ID`, `None` on a cold connect).
pub(crate) fn run_stream(ctx: &StreamCtx<'_>, last_rev: Option<u64>) {
    let mut tailer = Tailer::new(ctx.oplog_dir.to_path_buf());
    seed_tailer(&mut tailer, ctx.oplog_dir, last_rev);

    // Event-driven wakeup on oplog appends (design doc I12). If the watch can't
    // be established, `waiter` is None and the loop degrades to a pure backstop
    // poll at TAIL_REPOLL — correct, just less snappy.
    let waiter = sse::OplogWaiter::new(ctx.oplog_dir).ok();
    let sub_id = ctx.state.lock().ok().map(|mut b| b.hub.subscribe(ctx.agent_id));
    let mut gap_checked = last_rev.is_none();
    let mut last_keepalive = std::time::Instant::now();

    loop {
        // Oplog deltas (durable, rev-numbered).
        match tailer.poll() {
            Ok(entries) => {
                if !send_deltas(ctx, &entries, last_rev, &mut gap_checked) {
                    break;
                }
            }
            // A tailer read error means the client fell off the replayable
            // window — ask it to resync from a fresh REST load.
            Err(_) => {
                if ctx.sink.send(&sse::Message::resync()).is_err() {
                    break;
                }
            }
        }

        // Ephemeral stream frames (best-effort hints).
        if sub_id.is_some_and(|sub| !send_frames(ctx, sub)) {
            break;
        }

        // Tier-② state change — the driver loop or a command handler flagged
        // this agent's inspection-plane data as stale. Push an `invalidate`
        // event so connected frontends refetch immediately.
        let is_dirty = ctx.state.lock().ok().is_some_and(|mut b| b.take_dirty(ctx.agent_id));
        if is_dirty && ctx.sink.send(&sse::Message::invalidate()).is_err() {
            break;
        }

        // Keep-alive doubles as a disconnect probe on a slow cadence (see
        // [`keepalive`]) so the tight tail re-poll does not flood the client.
        if !keepalive(ctx, &mut last_keepalive) {
            break;
        }
        park(waiter.as_ref());
    }
    cleanup(ctx.state, ctx.agent_id, sub_id);
}

/// Borrowed connection context for one SSE producer run — the four references
/// [`run_stream`] needs, bundled so its signature stays within the argument
/// budget.
pub(crate) struct StreamCtx<'ctx> {
    /// The SSE sink the producer writes events into.
    pub sink: &'ctx sse::Sink,
    /// Shared backend state (stream hub + dirty flags).
    pub state: &'ctx Arc<Mutex<Backend>>,
    /// The agent whose oplog + stream frames this producer tails.
    pub agent_id: &'ctx str,
    /// That agent's oplog directory.
    pub oplog_dir: &'ctx Path,
}

/// Seed the tailer so the subscriber receives only the deltas it needs.
///
/// RECONNECT (`last_rev` present) → seed at the client's last-seen rev, so the
/// producer replays exactly the gap (`rev > last_seen`) the client missed while
/// disconnected (design doc §9 replay-by-rev).
///
/// COLD CONNECT (`None`) → seed at the CURRENT oplog head, so the subscriber
/// rides the LIVE tail and skips the history it just loaded over REST (T123).
/// But an EMPTY oplog has NO head: `oplog_head_rev` returns `None`, and seeding
/// a bogus `0` would silently DROP the agent's first append at rev 0 (T271
/// off-by-one). So an empty log is left UNSEEDED — delivering from rev 0 onward,
/// which is both correct and cheap (there is no backlog to replay).
fn seed_tailer(tailer: &mut Tailer, oplog_dir: &Path, last_rev: Option<u64>) {
    match last_rev {
        Some(rev) => tailer.seed(rev),
        None => {
            if let Some(head) = oplog_head_rev(oplog_dir) {
                tailer.seed(head);
            }
        }
    }
}

/// Forward one poll's durable deltas; returns `false` on client disconnect.
///
/// On the FIRST poll of a reconnect (`!gap_checked`), emits a `resync` when the
/// oldest replayable entry skips past the client's last rev — a gap the oplog
/// can no longer cover.
fn send_deltas(ctx: &StreamCtx<'_>, entries: &[OpEntry], last_rev: Option<u64>, gap_checked: &mut bool) -> bool {
    if !*gap_checked {
        // The oldest replayable entry skips past the client's last rev ⇒ a gap
        // the oplog can no longer cover ⇒ ask the client to resync.
        let gap = matches!(
            (last_rev, entries.first()),
            (Some(want), Some(first)) if first.rev > want.saturating_add(1)
        );
        if gap && ctx.sink.send(&sse::Message::resync()).is_err() {
            return false;
        }
        *gap_checked = true;
    }
    for entry in entries {
        let data = serde_json::to_string(entry).unwrap_or_default();
        if ctx.sink.send(&sse::Message::delta(entry.rev, data)).is_err() {
            return false;
        }
    }
    true
}

/// Drain and forward an agent's ephemeral stream frames; `false` on disconnect.
fn send_frames(ctx: &StreamCtx<'_>, sub: u64) -> bool {
    let frames = drain_frames(ctx.state, ctx.agent_id, sub);
    for frame in &frames {
        let data = serde_json::to_string(frame).unwrap_or_default();
        if ctx.sink.send(&sse::Message::stream(data)).is_err() {
            return false;
        }
    }
    true
}

/// Park until the agent appends to its oplog (woken in sub-ms on Linux inotify)
/// or the tight backstop elapses — so a delta surfaces within ~`TAIL_REPOLL`
/// even on macOS, where `FSEvents` notification latency is far higher.
fn park(waiter: Option<&sse::OplogWaiter>) {
    match waiter {
        Some(w) => w.wait(TAIL_REPOLL),
        None => thread::sleep(TAIL_REPOLL),
    }
}

/// Emit a keep-alive comment on a slow cadence (it doubles as the idle
/// disconnect probe); returns `false` if the write fails (client disconnected).
///
/// Kept off the tight tail re-poll so it does not flood the client — a busy
/// stream is already disconnect-probed by its failing delta/frame writes.
fn keepalive(ctx: &StreamCtx<'_>, last: &mut std::time::Instant) -> bool {
    if last.elapsed() >= KEEPALIVE_INTERVAL {
        if ctx.sink.keep_alive().is_err() {
            return false;
        }
        *last = std::time::Instant::now();
    }
    true
}

/// Drain an agent's stream-hub subscriber buffer under a brief lock.
fn drain_frames(state: &Arc<Mutex<Backend>>, agent_id: &str, sub: u64) -> Vec<Frame> {
    state.lock().ok().and_then(|mut b| b.hub.drain(agent_id, sub)).unwrap_or_default()
}

/// Release the stream-hub subscriber on producer exit.
fn cleanup(state: &Arc<Mutex<Backend>>, agent_id: &str, sub_id: Option<u64>) {
    if let (Ok(mut backend), Some(sub)) = (state.lock(), sub_id) {
        let _removed = backend.hub.unsubscribe(agent_id, sub);
    }
}

/// Read an agent oplog's current head `rev` for cold-connect SSE seeding.
///
/// Returns `None` when the oplog has no entries yet (or is absent/unreadable) —
/// the caller MUST then leave the tailer unseeded so the agent's first append
/// (`rev 0`) is delivered on the live tail, rather than seeding `0` (which is an
/// exclusive lower bound and would silently drop `rev 0`, the T271 off-by-one).
/// On a non-empty log it returns `Some(head)`.
///
/// Uses [`cp_oplog::replay`]'s bounded checkpoint fast-path: it reads only the
/// newest checkpoint-bearing segment to recover the head rev, so this is a cheap
/// read even for a long-lived log — it does NOT parse the whole history (which
/// is exactly the cost we are avoiding by not replaying it to the subscriber).
fn oplog_head_rev(oplog_dir: &Path) -> Option<u64> {
    let r = cp_oplog::replay::replay(oplog_dir).ok()?;
    r.rev_head
}

#[cfg(test)]
mod tests {
    use super::*;
    use cp_wire::types::Phase;
    use cp_wire::types::oplog::OpEntryKind;

    /// The keystone T271 regression: a subscriber cold-connecting to an EMPTY
    /// oplog must receive the agent's very first append (`rev 0`). The bug was
    /// `oplog_head_rev` returning `0` for an empty log, which seeded the tailer
    /// at `0` (exclusive) and silently dropped `rev 0`. The fix leaves the
    /// tailer unseeded on an empty log, so `rev 0` rides the live tail.
    #[test]
    fn cold_connect_on_empty_oplog_delivers_first_append() {
        let dir = tempfile::tempdir().expect("tempdir");
        let oplog = dir.path().to_path_buf();

        // Empty log ⇒ no head ⇒ the cold-connect path must NOT seed.
        assert!(oplog_head_rev(&oplog).is_none(), "empty oplog has no head");
        let mut tailer = Tailer::new(oplog.clone());
        if let Some(head) = oplog_head_rev(&oplog) {
            tailer.seed(head);
        }

        // The agent now appends its first entry (rev 0).
        let mut writer = cp_oplog::append::OplogWriter::open(&oplog).expect("open oplog");
        let _rev = writer.append(OpEntryKind::PhaseTransition { phase: Phase::Streaming }).expect("append");

        // The cold subscriber must see rev 0 on the live tail.
        let got = tailer.poll().expect("poll");
        assert_eq!(got.len(), 1, "first append delivered");
        assert_eq!(got[0].rev, 0, "rev 0 is not dropped");
    }

    /// The contrast case the T123 head-seed exists for: a subscriber cold-
    /// connecting to a NON-empty log seeds at the head and receives only
    /// FUTURE appends (it already loaded current state over REST), never a
    /// replay of the backlog.
    #[test]
    fn cold_connect_on_nonempty_oplog_skips_backlog() {
        let dir = tempfile::tempdir().expect("tempdir");
        let oplog = dir.path().to_path_buf();

        // Pre-existing backlog: one entry at rev 0.
        let mut writer = cp_oplog::append::OplogWriter::open(&oplog).expect("open oplog");
        let _rev0 = writer.append(OpEntryKind::PhaseTransition { phase: Phase::Streaming }).expect("append rev 0");

        // Cold connect now seeds at the head (Some), skipping the backlog.
        let head = oplog_head_rev(&oplog).expect("non-empty log has a head");
        let mut tailer = Tailer::new(oplog);
        tailer.seed(head);
        assert!(tailer.poll().expect("poll").is_empty(), "backlog is not replayed");

        // A future append is delivered live.
        let _rev1 = writer.append(OpEntryKind::PhaseTransition { phase: Phase::Idle }).expect("append rev 1");
        let got = tailer.poll().expect("poll");
        assert_eq!(got.len(), 1, "future append delivered");
        assert_eq!(got[0].rev, 1, "only the post-seed rev arrives");
    }
}
