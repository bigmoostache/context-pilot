//! The SSE producer loop and its tail-cadence tuning.
//!
//! Split out of [`transport`](super) so the acceptor/router in `mod.rs` stays
//! within the workspace's per-file line budget. [`run_stream`] is spawned by
//! `handle_stream` once per connected subscriber: it tails one agent's oplog
//! (rev-numbered durable deltas) plus its ephemeral stream-hub frames and
//! pushes both down the SSE [`sink`](super::sse::SseSink) until the client
//! disconnects.
//!
//! The cold-connect vs reconnect seeding policy (the T123 fix — a fresh
//! subscriber rides the live tail instead of replaying the whole oplog) lives
//! in [`run_stream`]'s seeding block.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use cp_wire::types::stream::Frame;

use super::sse;
use super::Backend;
use crate::channel::Tailer;

/// Tight tail re-poll cadence for the SSE producer.
///
/// The [`OplogWaiter`](sse::OplogWaiter) wakes the producer the instant the
/// agent appends — single-digit ms on Linux (inotify) — but macOS FSEvents
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
pub(crate) fn run_stream(
    sink: &sse::SseSink,
    state: &Arc<Mutex<Backend>>,
    agent_id: &str,
    oplog_dir: &PathBuf,
    last_rev: Option<u64>,
) {
    let mut tailer = Tailer::new(oplog_dir.clone());
    // Seed the tailer so the subscriber receives only the deltas it needs:
    //   * RECONNECT (`Last-Event-ID` present) → seed at the client's last-seen
    //     rev, so the producer replays exactly the gap (`rev > last_seen`) the
    //     client missed while disconnected (design doc §9 replay-by-rev).
    //   * COLD CONNECT (no `Last-Event-ID`) → seed at the CURRENT oplog head, so
    //     the subscriber rides the LIVE tail and receives only deltas appended
    //     from now on. The client just loaded full current state over REST, so
    //     replaying the entire oplog history would be both redundant and — for a
    //     long-lived agent with thousands of entries — catastrophically slow:
    //     the browser would chew through the whole backlog (seconds) before the
    //     user's just-sent message delta, sitting at the live head, ever paints
    //     (T123). Seeding at the head keeps a fresh connection's first live
    //     delta sub-ms instead of gated behind a full-history drain.
    match last_rev {
        Some(rev) => tailer.seed(rev),
        None => tailer.seed(oplog_head_rev(oplog_dir)),
    }
    // Event-driven wakeup on oplog appends (design doc I12). If the watch can't
    // be established, `waiter` is None and the loop degrades to a pure backstop
    // poll at TAIL_REPOLL — correct, just less snappy.
    let waiter = sse::OplogWaiter::new(oplog_dir).ok();
    let sub_id = state.lock().ok().map(|mut b| b.hub.subscribe(agent_id));
    let mut gap_checked = last_rev.is_none();
    let mut last_keepalive = std::time::Instant::now();

    loop {
        // Oplog deltas (durable, rev-numbered).
        match tailer.poll() {
            Ok(entries) => {
                if !gap_checked {
                    if let (Some(want), Some(first)) = (last_rev, entries.first()) {
                        // The oldest replayable entry skips past the client's
                        // last rev ⇒ a gap the oplog can't cover ⇒ resync.
                        if first.rev > want.saturating_add(1) && sink.send(&sse::SseMessage::resync()).is_err() {
                            break;
                        }
                    }
                    gap_checked = true;
                }
                for entry in &entries {
                    let data = serde_json::to_string(entry).unwrap_or_default();
                    if sink.send(&sse::SseMessage::delta(entry.rev, data)).is_err() {
                        return cleanup(state, agent_id, sub_id);
                    }
                }
            }
            Err(_) => {
                if sink.send(&sse::SseMessage::resync()).is_err() {
                    return cleanup(state, agent_id, sub_id);
                }
            }
        }

        // Ephemeral stream frames (best-effort hints).
        if let Some(sub) = sub_id {
            let frames = drain_frames(state, agent_id, sub);
            for frame in &frames {
                let data = serde_json::to_string(frame).unwrap_or_default();
                if sink.send(&sse::SseMessage::stream(data)).is_err() {
                    return cleanup(state, agent_id, sub_id);
                }
            }
        }

        // Tier-② state change — the driver loop or a command handler flagged
        // this agent's inspection-plane data as stale. Push an `invalidate`
        // event so connected frontends refetch immediately.
        {
            let is_dirty = state.lock().ok().map_or(false, |mut b| b.take_dirty(agent_id));
            if is_dirty && sink.send(&sse::SseMessage::invalidate()).is_err() {
                return cleanup(state, agent_id, sub_id);
            }
        }

        // Keep-alive doubles as a disconnect probe, but only on a slow cadence
        // so the tight tail re-poll below does not flood the client with
        // comments. A busy stream is already disconnect-probed by its failing
        // delta/frame writes above.
        if last_keepalive.elapsed() >= KEEPALIVE_INTERVAL {
            if sink.keep_alive().is_err() {
                return cleanup(state, agent_id, sub_id);
            }
            last_keepalive = std::time::Instant::now();
        }
        // Park until the agent appends to its oplog (woken in sub-ms on Linux
        // inotify) or the tight backstop elapses — so a delta surfaces within
        // ~TAIL_REPOLL even on macOS, where FSEvents notification latency is
        // far higher than the target.
        match &waiter {
            Some(w) => w.wait(TAIL_REPOLL),
            None => thread::sleep(TAIL_REPOLL),
        }
    }
    cleanup(state, agent_id, sub_id);
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
/// Returns `0` when the oplog is absent or unreadable — a fresh log whose first
/// real append is `rev 0`, so seeding at `0` (exclusive) still delivers it.
/// Uses [`cp_oplog::replay`]'s bounded checkpoint fast-path: it reads only the
/// newest checkpoint-bearing segment to recover the head rev, so this is a cheap
/// read even for a long-lived log — it does NOT parse the whole history (which
/// is exactly the cost we are avoiding by not replaying it to the subscriber).
fn oplog_head_rev(oplog_dir: &std::path::Path) -> u64 {
    cp_oplog::replay::replay(oplog_dir).ok().and_then(|r| r.rev_head).unwrap_or(0)
}
