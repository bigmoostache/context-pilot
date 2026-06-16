//! [`Tee`] — the agent-side stream tee.
//!
//! A non-blocking fan-out of live [`StreamFrame`]s from the hot loop to an
//! observing backend over the UDS stream plane (design doc tier ③ / roadmap
//! P3, MILESTONE M1).
//!
//! # The one rule: the loop is never perturbed
//!
//! The agent's streaming hot loop emits a [`StreamFrame`] per token / tool-arg
//! chunk / phase hint. Publishing those must cost the loop **one bounded,
//! non-blocking enqueue and nothing else** — no serialisation, no syscall, no
//! `fsync`, no blocking lock. [`Tee::publish`] is exactly that: a
//! [`SyncSender::try_send`] into a bounded channel. When the channel is full
//! the frame is **dropped in O(1)** and the tee marks itself *degraded*; the
//! loop continues at full speed. Stream frames are tier-③ traffic — disposable
//! by design, with the oplog as the durable safety net for anything that
//! matters (design doc I7/I10) — so dropping under pressure is correct, not a
//! failure.
//!
//! # Why a bounded channel rather than a hand-rolled lock-free ring
//!
//! The design sketch called for a lock-free SPSC ring. This workspace
//! **forbids `unsafe`** (`unsafe_code = "forbid"`), which rules out a
//! hand-rolled ring, so the tee uses a `std` bounded [`sync_channel`]. The
//! property that actually matters for the hot loop — **the producer never
//! blocks** — is preserved exactly: `try_send` fails fast on a full buffer. The
//! only lock involved is the channel's internal mutex, held by the consumer for
//! the microseconds of a single pop; while the consumer is *stalled* (blocked
//! on a slow UDS write) it holds **nothing**, so a producer `try_send` never
//! meaningfully contends. The V7 invariant — *a stalled consumer does not
//! affect loop tick* — therefore holds, and is asserted by
//! [`tests::stalled_consumer_never_blocks_the_producer`].
//!
//! # The publisher thread
//!
//! A dedicated thread owns the receiver and the bound [`UnixListener`]. It
//! accepts at most one observer at a time and, for each drained frame,
//! serialises it (JSON) and wraps it in the shared length-prefix + CRC framing
//! ([`cp_wire::framing::encode_raw`]) so the backend decodes stream frames with
//! the exact machinery it uses for the oplog. Writes are **non-blocking with a
//! bounded backoff**: a frame that cannot be written within the backoff budget
//! is dropped (degraded), never retried forever — so a wedged observer bounds
//! the publisher's per-frame time without ever touching the producer. With no
//! observer connected, drained frames are discarded, so a late-connecting
//! observer starts from live frames rather than a stale backlog.

use std::io::{self, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread::{self, sleep, JoinHandle};
use std::time::Duration;

use cp_wire::framing;
use cp_wire::types::stream::Frame as StreamFrame;

/// Default depth of the tee's bounded buffer, in frames.
///
/// Large enough to ride out a brief publisher hiccup, small enough that a
/// truly stalled observer is shedding load (dropping) within a few
/// milliseconds of token output.
pub const DEFAULT_TEE_CAPACITY: usize = 1024;

/// How long the publisher waits for the next frame before re-polling for a new
/// observer connection (so a client that connects mid-silence is still picked
/// up promptly).
const DRAIN_POLL: Duration = Duration::from_millis(50);

/// Per-frame write backoff: on `WouldBlock`, the publisher sleeps this long and
/// retries, up to [`MAX_WRITE_ATTEMPTS`] times, before giving up on the frame.
const WRITE_BACKOFF: Duration = Duration::from_micros(100);

/// Maximum non-blocking write attempts for a single frame before it is dropped.
/// `MAX_WRITE_ATTEMPTS * WRITE_BACKOFF` (~5 ms) bounds the publisher's time on
/// one wedged write; the producer is on another thread and is never affected.
const MAX_WRITE_ATTEMPTS: u32 = 50;

/// The fate of a [`Tee::publish`] call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The frame was enqueued for the publisher (not a delivery guarantee —
    /// tier-③ traffic is best-effort all the way down).
    Published,

    /// The buffer was full (or the publisher had stopped); the frame was
    /// dropped in O(1) and the tee is now degraded.
    Dropped,
}

/// Producer handle to the stream tee.
///
/// Cheap to hold on the hot loop: [`publish`](Self::publish) is a single
/// non-blocking channel send. Dropping the `Tee` stops and joins the publisher
/// thread.
#[derive(Debug)]
pub struct Tee {
    /// Bounded channel into the publisher. `try_send` is the O(1) enqueue.
    tx: SyncSender<StreamFrame>,

    /// Count of frames dropped because the buffer was full — observability for
    /// the degraded signal (design doc roadmap P3).
    dropped: Arc<AtomicU64>,

    /// Set once any drop has occurred; surfaced to the backend as a degraded
    /// stream so it can reconcile from the oplog snapshot.
    degraded: Arc<AtomicBool>,

    /// Signals the publisher thread to stop at its next wake.
    stop: Arc<AtomicBool>,

    /// The publisher thread, joined on [`shutdown`](Self::shutdown) / drop.
    handle: Option<JoinHandle<()>>,
}

impl Tee {
    /// Spawn the tee over a bound stream `listener` with the default capacity.
    #[must_use]
    pub fn spawn(listener: UnixListener) -> Self {
        Self::spawn_with_capacity(listener, DEFAULT_TEE_CAPACITY)
    }

    /// Spawn the tee with an explicit buffer `capacity` (tests use a tiny one to
    /// force the full-buffer drop path deterministically).
    #[must_use]
    pub fn spawn_with_capacity(listener: UnixListener, capacity: usize) -> Self {
        let (tx, rx) = sync_channel::<StreamFrame>(capacity);
        let dropped = Arc::new(AtomicU64::new(0));
        let degraded = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));

        let publisher = Publisher { rx, listener, stop: Arc::clone(&stop) };
        let handle = thread::spawn(move || publisher.run());

        Self { tx, dropped, degraded, stop, handle: Some(handle) }
    }

    /// Publish one frame: a single non-blocking enqueue.
    ///
    /// Returns [`Outcome::Dropped`] (and marks the tee degraded) when the
    /// buffer is full or the publisher has stopped — never blocks, never
    /// allocates beyond the move of `frame`.
    #[must_use]
    pub fn publish(&self, frame: StreamFrame) -> Outcome {
        match self.tx.try_send(frame) {
            Ok(()) => Outcome::Published,
            Err(TrySendError::Full(_dropped) | TrySendError::Disconnected(_dropped)) => {
                let _prev = self.dropped.fetch_add(1, Ordering::Relaxed);
                self.degraded.store(true, Ordering::Relaxed);
                Outcome::Dropped
            }
        }
    }

    /// Total frames dropped so far because the buffer was full.
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Whether the tee has dropped at least one frame (the degraded signal).
    #[must_use]
    pub fn is_degraded(&self) -> bool {
        self.degraded.load(Ordering::Relaxed)
    }

    /// Stop the publisher and join its thread.
    ///
    /// Idempotent with [`Drop`]: consuming the tee here makes the drop a no-op.
    pub fn shutdown(mut self) {
        self.signal_and_join();
    }

    /// Signal the publisher to stop and join it (shared by [`shutdown`] and
    /// [`Drop`]).
    ///
    /// [`shutdown`]: Self::shutdown
    fn signal_and_join(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _joined = handle.join();
        }
    }
}

impl Drop for Tee {
    fn drop(&mut self) {
        self.signal_and_join();
    }
}

/// The publisher thread's owned state: the frame receiver, the bound listener,
/// and the stop flag.
struct Publisher {
    /// Receiving end of the bounded tee channel.
    rx: Receiver<StreamFrame>,

    /// The agent's bound stream socket; observers connect here.
    listener: UnixListener,

    /// Set by [`Tee`] to request shutdown.
    stop: Arc<AtomicBool>,
}

impl Publisher {
    /// The publisher loop: maintain at most one observer, drain frames to it,
    /// shed load on a wedged write, and exit when asked to stop.
    fn run(self) {
        // Non-blocking accept so a missing observer never wedges the loop.
        let _ignored = self.listener.set_nonblocking(true);
        let mut client: Option<UnixStream> = None;

        while !self.stop.load(Ordering::Relaxed) {
            // Pick up a newly-connected observer if we have none.
            if client.is_none() {
                client = self.try_accept();
            }

            // Wait briefly for the next frame; a timeout re-polls accept/stop.
            let frame = match self.rx.recv_timeout(DRAIN_POLL) {
                Ok(frame) => frame,
                Err(_timeout_or_disconnect) => {
                    if self.stop.load(Ordering::Relaxed) {
                        break;
                    }
                    continue;
                }
            };

            // With no observer the frame is discarded (best-effort): a late
            // observer should see live frames, not a stale backlog.
            let Some(stream) = client.as_mut() else {
                continue;
            };

            if !write_frame(stream, &frame) {
                // Broken pipe / exhausted backoff: drop this observer and fall
                // back to accepting a fresh one.
                client = None;
            }
        }
    }

    /// Try to accept one observer without blocking; configure it non-blocking
    /// so writes can never wedge the publisher.
    fn try_accept(&self) -> Option<UnixStream> {
        match self.listener.accept() {
            Ok((stream, _addr)) => {
                let _ignored = stream.set_nonblocking(true);
                Some(stream)
            }
            Err(_would_block_or_error) => None,
        }
    }
}

/// Serialise `frame`, wrap it in the shared length+CRC framing, and write it to
/// `stream` with a bounded non-blocking backoff.
///
/// Returns `true` if the whole frame was written, `false` if the observer
/// should be dropped (a write error, broken pipe, or the backoff budget was
/// exhausted). A `false` return is never fatal — it only sheds one observer.
fn write_frame(stream: &mut UnixStream, frame: &StreamFrame) -> bool {
    let Ok(payload) = serde_json::to_vec(frame) else {
        // A frame that will not serialise is dropped, but the observer is fine.
        return true;
    };
    let Ok(framed) = framing::encode_raw(&payload) else {
        return true;
    };
    write_all_bounded(stream, &framed)
}

/// Write every byte of `bytes` to a non-blocking `stream`, backing off on
/// `WouldBlock` up to [`MAX_WRITE_ATTEMPTS`]. Returns `false` on a hard error or
/// once the backoff budget is spent (the observer is then dropped).
fn write_all_bounded(stream: &mut UnixStream, bytes: &[u8]) -> bool {
    let mut written = 0usize;
    let mut attempts = 0u32;
    while let Some(rest) = bytes.get(written..) {
        if rest.is_empty() {
            return true;
        }
        match stream.write(rest) {
            Ok(0) => return false,
            Ok(n) => {
                written = written.wrapping_add(n);
                attempts = 0;
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                attempts = attempts.wrapping_add(1);
                if attempts >= MAX_WRITE_ATTEMPTS {
                    return false;
                }
                sleep(WRITE_BACKOFF);
            }
            Err(_hard) => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use cp_wire::framing::decode_raw;
    use cp_wire::types::stream::Kind;
    use std::io::Read as _;
    use std::time::Instant;
    use tempfile::tempdir;

    /// A bound listener on a temp-dir socket, plus its path.
    fn bound_listener() -> (UnixListener, std::path::PathBuf) {
        let dir = tempdir().expect("tempdir");
        // Leak the tempdir so the socket path stays valid for the test body.
        let path = dir.keep().join("stream.sock");
        let listener = UnixListener::bind(&path).expect("bind");
        (listener, path)
    }

    fn token(seq: u64, text: &str) -> StreamFrame {
        StreamFrame {
            schema_version: 1,
            agent_id: "a".to_owned(),
            worker_id: "w".to_owned(),
            thread_id: "T1".to_owned(),
            message_id: "m1".to_owned(),
            seq,
            kind: Kind::Token { text: text.to_owned() },
        }
    }

    /// Read and decode exactly one framed [`StreamFrame`] from `stream`,
    /// retrying short reads until the frame is complete or `deadline` passes.
    fn read_one_frame(stream: &mut UnixStream, deadline: Instant) -> Option<StreamFrame> {
        let mut buf: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 256];
        while Instant::now() < deadline {
            match stream.read(&mut chunk) {
                Ok(0) => return None,
                Ok(n) => {
                    if let Some(got) = chunk.get(..n) {
                        buf.extend_from_slice(got);
                    }
                    if let Ok((payload, _consumed)) = decode_raw(&buf) {
                        return serde_json::from_slice(payload).ok();
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => sleep(WRITE_BACKOFF),
                Err(_) => return None,
            }
        }
        None
    }

    #[test]
    fn publish_delivers_frames_to_a_connected_observer() {
        let (listener, path) = bound_listener();
        let tee = Tee::spawn(listener);

        // Connect an observer and give the publisher a moment to accept it.
        let mut client = UnixStream::connect(&path).expect("connect");
        client.set_nonblocking(true).expect("nonblocking");
        sleep(Duration::from_millis(80));

        assert_eq!(tee.publish(token(0, "hello")), Outcome::Published);

        let deadline = Instant::now() + Duration::from_secs(2);
        let got = read_one_frame(&mut client, deadline).expect("a frame arrives");
        assert_eq!(got, token(0, "hello"), "the delivered frame round-trips");

        tee.shutdown();
    }

    #[test]
    fn stalled_consumer_never_blocks_the_producer() {
        // V7: an observer that connects but never reads fills the socket buffer,
        // which fills the tee buffer; the producer must still race through a
        // flood of publishes without blocking, shedding load via Dropped.
        let (listener, path) = bound_listener();
        let tee = Tee::spawn_with_capacity(listener, 8);

        let _stalled = UnixStream::connect(&path).expect("connect");
        sleep(Duration::from_millis(80)); // let the publisher take the client
        // Note: `_stalled` is never read from — the classic wedged observer.

        let start = Instant::now();
        let mut dropped_seen = false;
        for seq in 0..100_000u64 {
            if tee.publish(token(seq, "x")) == Outcome::Dropped {
                dropped_seen = true;
            }
        }
        let elapsed = start.elapsed();

        assert!(dropped_seen, "a stalled observer must force drops");
        assert!(tee.is_degraded(), "drops must raise the degraded signal");
        assert!(tee.dropped() > 0, "the dropped counter must advance");
        assert!(
            elapsed < Duration::from_secs(5),
            "100k non-blocking publishes must finish fast despite the stall, took {elapsed:?}",
        );

        tee.shutdown();
    }

    #[test]
    fn publishing_with_no_observer_is_best_effort() {
        let (listener, _path) = bound_listener();
        let tee = Tee::spawn_with_capacity(listener, 4);

        // No observer connected: the first few buffer, the rest drop — but no
        // call ever blocks or panics.
        for seq in 0..50u64 {
            let _outcome = tee.publish(token(seq, "x"));
        }
        tee.shutdown();
    }

    #[test]
    fn shutdown_joins_cleanly() {
        let (listener, _path) = bound_listener();
        let tee = Tee::spawn(listener);
        let _o = tee.publish(token(0, "x"));
        tee.shutdown(); // must not hang or panic.
    }

    #[test]
    fn drop_without_shutdown_also_joins() {
        let (listener, _path) = bound_listener();
        {
            let tee = Tee::spawn(listener);
            let _o = tee.publish(token(0, "x"));
        } // Drop must stop + join the publisher.
    }
}
