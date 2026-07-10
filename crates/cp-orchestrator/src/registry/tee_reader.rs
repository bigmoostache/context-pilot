//! [`TeeReader`] — the backend's consumer of an agent's **live stream plane**
//! (design doc tier ③ / §7).
//!
//! Where the [`Tailer`](crate::registry::tailer::Tailer) consumes an agent's
//! *durable* oplog, the [`TeeReader`] consumes its *ephemeral* token stream: it
//! connects to the agent's `tee.sock`, reads length-prefixed
//! [`StreamFrame`]s (the same `len + CRC` framing the oplog uses, written by the
//! agent's `Tee` publisher), and republishes each one into the shared
//! [`StreamHub`](crate::services::StreamHub) so every connected SSE subscriber
//! fans it out to the browser.
//!
//! # One reader per agent, not per subscriber
//!
//! The agent's `Tee` publisher accepts **at most one observer** at a time. So
//! exactly one `TeeReader` connects per agent — owned by the runtime driver,
//! spawned on [`Appeared`](crate::registry::Event::Appeared) and stopped on
//! [`Disappeared`](crate::registry::Event::Disappeared). The
//! [`StreamHub`] is what fans a single inbound frame out to N browser tabs; the
//! reader never needs to know how many subscribers exist.
//!
//! # Lossy by design
//!
//! Stream frames are tier-③ traffic: best-effort and droppable, with the oplog
//! as the durable safety net (design doc I7/I10). The reader therefore favours
//! liveness over completeness — a corrupt or torn frame resyncs the buffer
//! rather than tearing down the connection, a missing socket retries rather
//! than erroring, and EOF (agent restart) simply reconnects.

use std::io::{ErrorKind, Read as _};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle, sleep};
use std::time::Duration;

use cp_wire::framing::{FRAME_HEADER_SIZE, FrameError, MAX_PAYLOAD_SIZE, decode_raw};
use cp_wire::types::stream::Frame as StreamFrame;

use crate::transport::Backend;

/// How long to wait before retrying a failed connect to a not-yet-bound (or
/// vanished) `tee.sock`. Short enough that a just-booted agent's stream is
/// picked up promptly, long enough to avoid a busy-spin while it is absent.
const RECONNECT_DELAY: Duration = Duration::from_millis(200);

/// Read timeout on the connected socket. Bounds how long a blocking `read`
/// parks before returning so the loop can observe the stop flag — the reader
/// reacts to a [`Disappeared`](crate::registry::Event::Disappeared) within this
/// window even when no frames are flowing.
const READ_TIMEOUT: Duration = Duration::from_millis(200);

/// Size of each socket read into the accumulation buffer.
const READ_CHUNK: usize = 4096;

/// Hard ceiling on the accumulation buffer. A buffer that grows past one
/// maximum frame without yielding a decodable record is treated as desynced and
/// reset — defends against a corrupt `len` header wedging the reader.
const MAX_BUFFER: usize = MAX_PAYLOAD_SIZE as usize;

/// Tee socket file name inside an agent's folder (mirrors `cp-mod-bridge`'s
/// `TEE_SOCKET`). Defined here rather than imported to keep the backend's
/// dependency on the agent crate test-only.
const TEE_SOCKET: &str = "tee.sock";

/// The backend-side reader of one agent's live stream socket.
///
/// Construct with [`spawn`](TeeReader::spawn); drop (or call
/// [`stop`](TeeReader::stop)) to signal the reader thread to exit and join it.
#[derive(Debug)]
pub struct TeeReader {
    /// Set to request the reader thread stop at its next wake.
    stop: Arc<AtomicBool>,

    /// The reader thread, joined on [`stop`](Self::stop) / drop.
    handle: Option<JoinHandle<()>>,
}

impl TeeReader {
    /// Spawn a reader for the agent at `folder`, republishing its stream frames
    /// into `backend`'s [`StreamHub`](crate::services::StreamHub) under
    /// `agent_id`.
    ///
    /// The reader connects to `<folder>/tee.sock`, retrying while the socket is
    /// absent, and runs until [`stop`](Self::stop) (or drop) is invoked.
    #[must_use]
    pub fn spawn(agent_id: String, folder: &std::path::Path, backend: Arc<Mutex<Backend>>) -> Self {
        let tee_path = folder.join(TEE_SOCKET);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        let handle = thread::spawn(move || read_loop(&agent_id, &tee_path, &backend, &stop_thread));
        Self { stop, handle: Some(handle) }
    }

    /// Signal the reader thread to stop and join it. Idempotent with [`Drop`].
    pub fn stop(mut self) {
        self.signal_and_join();
    }

    /// Shared stop+join used by [`stop`](Self::stop) and [`Drop`].
    fn signal_and_join(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _joined = handle.join();
        }
    }
}

impl Drop for TeeReader {
    fn drop(&mut self) {
        self.signal_and_join();
    }
}

/// The reader thread body: (re)connect to the tee socket and pump frames into
/// the hub until asked to stop.
fn read_loop(agent_id: &str, tee_path: &PathBuf, backend: &Arc<Mutex<Backend>>, stop: &AtomicBool) {
    while !stop.load(Ordering::Relaxed) {
        match UnixStream::connect(tee_path) {
            Ok(stream) => {
                let _ignored = stream.set_read_timeout(Some(READ_TIMEOUT));
                pump_connection(agent_id, stream, backend, stop);
                // Connection ended (EOF / error / stop) — loop reconnects
                // unless we are stopping, so an agent restart is picked up.
            }
            Err(_absent) => sleep(RECONNECT_DELAY),
        }
    }
}

/// Drain one connected socket frame-by-frame into the hub. Returns when the
/// connection ends (EOF / hard error) or `stop` is set.
fn pump_connection(agent_id: &str, mut stream: UnixStream, backend: &Arc<Mutex<Backend>>, stop: &AtomicBool) {
    let mut buf: Vec<u8> = Vec::with_capacity(READ_CHUNK);
    let mut chunk = [0u8; READ_CHUNK];

    while !stop.load(Ordering::Relaxed) {
        match stream.read(&mut chunk) {
            Ok(0) => return, // EOF — the agent closed the stream (e.g. restart).
            Ok(n) => {
                if let Some(got) = chunk.get(..n) {
                    buf.extend_from_slice(got);
                }
                drain_frames(agent_id, &mut buf, backend);
                if buf.len() > MAX_BUFFER {
                    // Desynced past a full frame without decoding — reset.
                    buf.clear();
                }
            }
            // A timeout is the expected idle path: it just lets us re-check
            // the stop flag. WouldBlock is the same on non-timeout platforms.
            Err(ref e) if e.kind() == ErrorKind::TimedOut || e.kind() == ErrorKind::WouldBlock => {}
            Err(_hard) => return, // broken pipe / reset — reconnect.
        }
    }
}

/// Decode and publish every complete frame at the front of `buf`, retaining any
/// trailing partial frame for the next read.
fn drain_frames(agent_id: &str, buf: &mut Vec<u8>, backend: &Arc<Mutex<Backend>>) {
    loop {
        match decode_raw(buf) {
            Ok((payload, consumed)) => {
                if let Ok(frame) = serde_json::from_slice::<StreamFrame>(payload) {
                    publish(agent_id, &frame, backend);
                }
                // Drop the consumed frame; keep the trailing partial bytes.
                let _drained = buf.drain(..consumed);
            }
            // Not enough bytes yet — wait for the next read.
            Err(FrameError::Incomplete) => return,
            // A CRC mismatch means this frame's payload is corrupt, but its
            // length header is intact — skip exactly this frame and keep
            // decoding, so a good frame that arrived in the same read still
            // gets delivered (tier-③ is lossy per frame, not per buffer).
            Err(FrameError::CrcMismatch { .. }) => match corrupt_frame_span(buf) {
                Some(span) if span <= buf.len() => {
                    let _resynced = buf.drain(..span);
                }
                // The declared length is not yet fully buffered or is
                // untrustworthy — drop everything and resync from scratch.
                _ => {
                    buf.clear();
                    return;
                }
            },
            // An oversized/implausible length header is unrecoverable in place;
            // resync by discarding the buffer.
            Err(_corrupt) => {
                buf.clear();
                return;
            }
        }
    }
}

/// Byte span (`header + payload`) that the frame at the front of `buf` *claims*
/// to occupy, read from its intact little-endian length header. Returns `None`
/// when the header is not yet fully buffered or the declared length exceeds the
/// safety cap (so the caller falls back to a full-buffer resync).
fn corrupt_frame_span(buf: &[u8]) -> Option<usize> {
    let len_bytes: [u8; 4] = buf.get(0..4)?.try_into().ok()?;
    let len = u32::from_le_bytes(len_bytes);
    if len > MAX_PAYLOAD_SIZE {
        return None;
    }
    Some(FRAME_HEADER_SIZE.wrapping_add(usize::try_from(len).ok()?))
}

/// Publish one frame into the hub under a brief lock.
fn publish(agent_id: &str, frame: &StreamFrame, backend: &Arc<Mutex<Backend>>) {
    if let Ok(mut b) = backend.lock() {
        let _delivered = b.hub_mut().publish(agent_id, frame);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cp_wire::framing::encode_raw;
    use cp_wire::types::stream::Kind;
    use std::io::Write as _;
    use std::os::unix::net::UnixListener;
    use tempfile::tempdir;

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

    /// A backend with one subscriber so published frames are retained for the
    /// assertion (the hub drops frames for agents with no subscribers).
    fn backend_with_subscriber(agent_id: &str) -> (Arc<Mutex<Backend>>, u64) {
        let dir = tempdir().expect("dir");
        let backend = Arc::new(Mutex::new(Backend::new(
            dir.path().to_path_buf(),
            PathBuf::from("/tmp/cp-test-realms"),
            PathBuf::from("/tmp/cp-test-bin"),
            None,
            Duration::from_secs(3600),
        )));
        let sub = backend.lock().expect("lock").hub_mut().subscribe(agent_id);
        (backend, sub)
    }

    #[test]
    fn reader_republishes_framed_tokens_into_the_hub() {
        let dir = tempdir().expect("dir");
        let folder = dir.path().to_path_buf();
        let tee_path = folder.join(TEE_SOCKET);
        let listener = UnixListener::bind(&tee_path).expect("bind");

        let (backend, sub) = backend_with_subscriber("agentX");
        let reader = TeeReader::spawn("agentX".to_owned(), &folder, Arc::clone(&backend));

        // Accept the reader's connection and write three framed tokens.
        let (mut conn, _addr) = listener.accept().expect("accept");
        for seq in 0..3u64 {
            let payload = serde_json::to_vec(&token(seq, "hi")).expect("ser");
            let framed = encode_raw(&payload).expect("frame");
            conn.write_all(&framed).expect("write");
        }
        conn.flush().expect("flush");

        // Poll the subscriber buffer until the three frames arrive.
        let mut got = Vec::new();
        for _ in 0..50 {
            if let Some(frames) = backend.lock().expect("lock").hub_mut().drain("agentX", sub) {
                got.extend(frames);
            }
            if got.len() >= 3 {
                break;
            }
            sleep(Duration::from_millis(20));
        }
        reader.stop();

        assert_eq!(got.len(), 3, "all three framed tokens republished");
        assert_eq!(got.first(), Some(&token(0, "hi")), "frame round-trips intact");
    }

    #[test]
    fn reader_tolerates_absent_socket_then_connects() {
        let dir = tempdir().expect("dir");
        let folder = dir.path().to_path_buf();
        let (backend, sub) = backend_with_subscriber("late");

        // Spawn BEFORE the socket exists — the reader must retry, not die.
        let reader = TeeReader::spawn("late".to_owned(), &folder, Arc::clone(&backend));
        sleep(Duration::from_millis(80));

        // Now bind + serve a frame; the reader should connect and deliver it.
        let listener = UnixListener::bind(folder.join(TEE_SOCKET)).expect("bind");
        let (mut conn, _addr) = listener.accept().expect("accept");
        let framed = encode_raw(&serde_json::to_vec(&token(7, "x")).expect("ser")).expect("frame");
        conn.write_all(&framed).expect("write");
        conn.flush().expect("flush");

        let mut delivered = false;
        for _ in 0..50 {
            if let Some(frames) = backend.lock().expect("lock").hub_mut().drain("late", sub) {
                if !frames.is_empty() {
                    delivered = true;
                    break;
                }
            }
            sleep(Duration::from_millis(20));
        }
        reader.stop();
        assert!(delivered, "reader connected once the socket appeared and delivered a frame");
    }

    #[test]
    fn corrupt_prefix_resyncs_without_killing_the_reader() {
        let dir = tempdir().expect("dir");
        let folder = dir.path().to_path_buf();
        let tee_path = folder.join(TEE_SOCKET);
        let listener = UnixListener::bind(&tee_path).expect("bind");

        let (backend, sub) = backend_with_subscriber("noisy");
        let reader = TeeReader::spawn("noisy".to_owned(), &folder, Arc::clone(&backend));
        let (mut conn, _addr) = listener.accept().expect("accept");

        // Garbage bytes that look like a frame header with a CRC mismatch, then
        // a clean frame: the reader must resync and still deliver the good one.
        let mut corrupt = encode_raw(b"junk-payload").expect("frame");
        if let Some(byte) = corrupt.get_mut(8) {
            *byte ^= 0xFF; // flip a payload byte → CRC mismatch on decode
        }
        conn.write_all(&corrupt).expect("write corrupt");
        let good = encode_raw(&serde_json::to_vec(&token(1, "ok")).expect("ser")).expect("frame");
        conn.write_all(&good).expect("write good");
        conn.flush().expect("flush");

        let mut got = Vec::new();
        for _ in 0..50 {
            if let Some(frames) = backend.lock().expect("lock").hub_mut().drain("noisy", sub) {
                got.extend(frames);
            }
            if !got.is_empty() {
                break;
            }
            sleep(Duration::from_millis(20));
        }
        reader.stop();
        // The corrupt bytes are discarded; the subsequent clean frame survives.
        assert_eq!(got.first(), Some(&token(1, "ok")), "clean frame delivered after resync");
    }
}
