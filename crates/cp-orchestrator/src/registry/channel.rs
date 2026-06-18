//! Per-agent **channel** — the backend's read/write handle to one agent.
//!
//! An [`AgentChannel`] is constructed from a registry [`Entry`] and exposes
//! the backend's two runtime operations against that agent:
//!
//! * [`hydrate`](AgentChannel::hydrate) — on-demand, content-addressed body
//!   fetch from the agent's body store (`oplog/bodies/{hash}`). The oplog tail
//!   delivers heads (content hashes); hydrate resolves them to bytes, verifying
//!   integrity.
//! * [`send`](AgentChannel::send) — deliver a [`Command`] to the agent over its
//!   UDS stream socket, returning the durable [`Ack`] (journal-then-ack, I11).
//!
//! A [`Tailer`] is the incremental, gap-free consumer of an agent's oplog
//! directory. Each [`poll`](Tailer::poll) returns only the entries appended
//! since the previous call, in `rev` order. The `rev` is monotonic and no
//! entry is skipped, so the consumer sees a complete, ordered event stream.
//! The tailer is a **pure poll primitive** — no kernel watch, no thread. The
//! live driver (inotify + backstop timer) belongs to the runtime loop, exactly
//! as [`AgentRegistry`](crate::registry::AgentRegistry) is a pure scan+diff
//! driven by an external cadence.

use std::io::{self, Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{fs, thread};

use cp_oplog::segment;
use cp_wire::framing;
use cp_wire::types::ack::Ack;
use cp_wire::types::command::{Command, Frame as CommandFrame};
use cp_wire::types::oplog::OpEntry;
use cp_wire::types::registry::Entry;
use cp_wire::types::ContentHash;

/// Schema version stamped onto command frames this channel sends.
const FRAME_SCHEMA_VERSION: u32 = 1;

/// Subdirectory of the oplog directory holding spilled body files.
const BODIES_DIR: &str = "bodies";

/// Maximum bytes to accumulate when reading an ack before giving up (a
/// safety bound against a peer that sends an endless un-decodable stream).
const MAX_ACK_BUFFER: usize = 64 * 1024;

/// Read timeout applied to the UDS socket when awaiting an ack.
const ACK_READ_TIMEOUT: Duration = Duration::from_secs(5);
/// Chunk size for reading ack bytes off the socket.
const READ_CHUNK: usize = 4096;

/// Delay between connection retries when the agent's socket is not yet
/// ready (used only by [`send_with_retry`](AgentChannel::send_with_retry)).
const RETRY_DELAY: Duration = Duration::from_millis(100);

// ── Tailer ─────────────────────────────────────────────────────────────

/// Incremental, gap-free consumer of an agent's oplog directory.
///
/// Remembers the highest delivered `rev` and the segment index it was in, so
/// each [`poll`](Tailer::poll) reads only the newest segment(s) and returns
/// only entries the consumer has not yet seen. Correct across segment rolls,
/// compaction (which deletes only segments the tailer has already passed), and
/// a missing directory (yields an empty poll, not an error).
#[derive(Debug)]
pub struct Tailer {
    /// The agent's oplog directory (`<folder>/oplog`).
    dir: PathBuf,

    /// The segment index we last read entries from. On the next poll we start
    /// scanning from this index (skipping older segments entirely).
    last_index: Option<u64>,

    /// The highest `rev` delivered to the consumer. Entries at or below this
    /// rev are filtered out, ensuring gap-free, exactly-once delivery.
    last_rev: Option<u64>,
}

impl Tailer {
    /// Create a tailer over `oplog_dir`. The first [`poll`](Tailer::poll)
    /// returns every entry in the log (a full catch-up); use
    /// [`seed`](Tailer::seed) first to skip already-processed history.
    #[must_use]
    pub fn new(oplog_dir: PathBuf) -> Self {
        Self { dir: oplog_dir, last_index: None, last_rev: None }
    }

    /// Advance the cursor to `rev` so the next poll skips everything at or
    /// below it. Call after replaying the log to a known point.
    pub fn seed(&mut self, rev: u64) {
        self.last_rev = Some(rev);
    }

    /// Read new entries since the last poll, advancing the cursor.
    ///
    /// Returns entries in ascending `rev` order. An empty `Vec` means no new
    /// entries were appended since the last call. The method is idempotent: two
    /// consecutive calls with no intervening agent writes yield `[]` then `[]`.
    ///
    /// # Errors
    ///
    /// Returns [`io::Error`] if a segment file cannot be listed or read.
    pub fn poll(&mut self) -> io::Result<Vec<OpEntry>> {
        let indices = segment::indices(&self.dir)?;
        let start_from = self.last_index.unwrap_or(0);
        let mut new_entries: Vec<OpEntry> = Vec::new();

        for &index in &indices {
            if index < start_from {
                continue;
            }
            let scan = segment::read(&segment::path(&self.dir, index))
                .map_err(|e| io::Error::other(e.to_string()))?;
            for entry in scan.entries {
                let dominated = self.last_rev.is_some_and(|lr| entry.rev <= lr);
                if !dominated {
                    new_entries.push(entry);
                }
            }
        }

        if let Some(last) = new_entries.last() {
            self.last_rev = Some(last.rev);
        }
        if let Some(&newest_index) = indices.last() {
            self.last_index = Some(newest_index);
        }
        Ok(new_entries)
    }

    /// The highest `rev` delivered so far, or `None` if no entries have been
    /// polled yet.
    #[must_use]
    pub const fn last_rev(&self) -> Option<u64> {
        self.last_rev
    }
}

// ── AgentChannel ───────────────────────────────────────────────────────

/// The backend's read/write handle to one agent (hydrate bodies + send
/// commands).
///
/// Constructed from a registry [`Entry`]; holds the paths and credential
/// needed to reach the agent's body store and command socket.
#[derive(Debug)]
pub struct AgentChannel {
    /// Path to the agent's UDS stream socket.
    socket_path: PathBuf,

    /// The bearer token the agent requires (from the registry record).
    cap_token: String,

    /// Directory holding spilled body files (`<oplog>/bodies`).
    bodies_dir: PathBuf,
}

impl AgentChannel {
    /// Build a channel from a registry record.
    #[must_use]
    pub fn from_entry(entry: &Entry) -> Self {
        Self {
            socket_path: PathBuf::from(&entry.socket_path),
            cap_token: entry.cap_token.clone(),
            bodies_dir: Path::new(&entry.oplog_path).join(BODIES_DIR),
        }
    }

    /// Fetch a spilled body by content hash, verifying integrity.
    ///
    /// Returns `Ok(None)` if no body file exists (the body was inlined in its
    /// oplog entry, or was garbage-collected). The read-back bytes are
    /// re-hashed and compared; a mismatch is an [`io::Error`] (bit-rot or a
    /// wrong file), never silently-corrupt data.
    ///
    /// # Errors
    ///
    /// Returns [`io::Error`] on a read fault or a content-hash mismatch.
    pub fn hydrate(&self, hash: ContentHash) -> io::Result<Option<Vec<u8>>> {
        let path = self.bodies_dir.join(hash.to_hex());
        match fs::read(&path) {
            Ok(bytes) => {
                if ContentHash::of(&bytes) == hash {
                    Ok(Some(bytes))
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("body {} failed integrity check", hash.to_hex()),
                    ))
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Send a [`Command`] to the agent, returning its durable [`Ack`].
    ///
    /// Connects to the agent's UDS socket, writes a framed
    /// [`CommandFrame`] carrying the registry `cap_token`, and reads the
    /// framed [`Ack`] the agent returns (journal-then-ack: the ack is sent
    /// only after the command's effect is `fdatasync`'d, design doc I11).
    ///
    /// # Errors
    ///
    /// Returns [`io::Error`] if the agent is unreachable (connection refused),
    /// the socket write fails, the ack is not received within
    /// [`ACK_READ_TIMEOUT`], or the response cannot be decoded.
    pub fn send(&self, command: Command) -> io::Result<Ack> {
        let frame = CommandFrame {
            schema_version: FRAME_SCHEMA_VERSION,
            auth: self.cap_token.clone(),
            command,
        };
        let payload = serde_json::to_vec(&frame)
            .map_err(|e| io::Error::other(format!("serialize command frame: {e}")))?;
        let frame_bytes = framing::encode_raw(&payload)
            .map_err(|e| io::Error::other(format!("frame command: {e}")))?;

        let mut stream = UnixStream::connect(&self.socket_path)?;
        let _timeout = stream.set_read_timeout(Some(ACK_READ_TIMEOUT));
        stream.write_all(&frame_bytes)?;
        // Half-close the write side so the agent's read loop sees EOF after
        // this single command (the v1 protocol sends one command per
        // connection; a long-lived multiplexed connection is a later phase).
        stream.shutdown(std::net::Shutdown::Write)?;

        read_ack(&mut stream)
    }

    /// Like [`send`](Self::send), but retries up to `retries` times on
    /// connection-refused (the agent's socket may not be ready yet).
    ///
    /// # Errors
    ///
    /// Returns the last [`io::Error`] if all attempts fail.
    pub fn send_with_retry(&self, command: Command, retries: u32) -> io::Result<Ack> {
        let mut last_err = None;
        for attempt in 0..=retries {
            match self.send(command.clone()) {
                Ok(ack) => return Ok(ack),
                Err(e) if e.kind() == io::ErrorKind::ConnectionRefused && attempt < retries => {
                    last_err = Some(e);
                    thread::sleep(RETRY_DELAY);
                }
                Err(e) => return Err(e),
            }
        }
        Err(last_err.unwrap_or_else(|| io::Error::other("send_with_retry: no attempts")))
    }
}

/// Read a framed [`Ack`] from `stream`, bounded by [`MAX_ACK_BUFFER`].
fn read_ack(stream: &mut UnixStream) -> io::Result<Ack> {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; READ_CHUNK];

    loop {
        let read = stream.read(&mut chunk)?;
        if read == 0 && buf.is_empty() {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "agent closed before ack"));
        }
        if let Some(got) = chunk.get(..read) {
            buf.extend_from_slice(got);
        }
        // Try to decode a complete framed Ack.
        if let Ok((payload, _consumed)) = framing::decode_raw(&buf) {
            let ack: Ack = serde_json::from_slice(payload)
                .map_err(|e| io::Error::other(format!("decode ack: {e}")))?;
            return Ok(ack);
        }
        if read == 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "incomplete ack frame"));
        }
        if buf.len() > MAX_ACK_BUFFER {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "ack buffer overflow"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cp_oplog::append::OplogWriter;
    use cp_wire::types::ack::Status;
    use cp_wire::types::oplog::OpEntryKind;
    use cp_wire::types::Phase;
    use std::os::unix::net::UnixListener;
    use tempfile::tempdir;

    fn phase_kind() -> OpEntryKind {
        OpEntryKind::PhaseTransition { phase: Phase::Streaming }
    }

    fn msg(thread: &str, byte: u8) -> OpEntryKind {
        OpEntryKind::MessageCreated {
            thread_id: thread.to_owned(),
            message_id: format!("m{byte}"),
            head: ContentHash::new([byte; 32]),
            inline_body: None,
        }
    }

    // ── Tailer tests ───────────────────────────────────────────────────

    #[test]
    fn poll_returns_new_entries() {
        let dir = tempdir().expect("dir");
        let mut writer = OplogWriter::open(dir.path()).expect("open");
        for byte in 0..4u8 {
            let _r = writer.append(msg("T1", byte)).expect("append");
        }

        let mut tailer = Tailer::new(dir.path().to_path_buf());
        let first = tailer.poll().expect("poll");
        assert_eq!(first.len(), 4, "first poll catches up the entire log");
        let revs: Vec<u64> = first.iter().map(|e| e.rev).collect();
        assert_eq!(revs, vec![0, 1, 2, 3]);
        assert_eq!(tailer.last_rev(), Some(3));
    }

    #[test]
    fn poll_is_idempotent_then_delivers_new() {
        let dir = tempdir().expect("dir");
        let mut writer = OplogWriter::open(dir.path()).expect("open");
        let _r = writer.append(phase_kind()).expect("append");

        let mut tailer = Tailer::new(dir.path().to_path_buf());
        let _catch_up = tailer.poll().expect("poll");
        let empty = tailer.poll().expect("poll");
        assert!(empty.is_empty(), "no new writes → empty poll");

        let _r = writer.append(msg("T1", 0xAA)).expect("append");
        let new = tailer.poll().expect("poll");
        assert_eq!(new.len(), 1);
        assert_eq!(new.first().expect("entry").rev, 1);
    }

    #[test]
    fn poll_across_segment_roll_is_gap_free() {
        let dir = tempdir().expect("dir");
        let mut writer = OplogWriter::open_with_segment_limit(dir.path(), 16).expect("open");

        let mut tailer = Tailer::new(dir.path().to_path_buf());
        // Append enough to force several segment rolls.
        for byte in 0..10u8 {
            let _r = writer.append(msg("T1", byte)).expect("append");
        }
        let all = tailer.poll().expect("poll");
        // Revs must be strictly increasing and contain every user record +
        // every checkpoint the rolls injected. The exact count depends on
        // frame sizes, but monotonicity is the invariant.
        for window in all.windows(2) {
            assert!(
                window.get(1).expect("w1").rev > window.first().expect("w0").rev,
                "revs must strictly increase across segment rolls",
            );
        }
        assert!(all.len() >= 10, "at least 10 user records (plus checkpoints)");
    }

    #[test]
    fn seed_skips_known_history() {
        let dir = tempdir().expect("dir");
        let mut writer = OplogWriter::open(dir.path()).expect("open");
        for byte in 0..5u8 {
            let _r = writer.append(msg("T1", byte)).expect("append");
        }

        let mut tailer = Tailer::new(dir.path().to_path_buf());
        tailer.seed(2); // skip revs 0, 1, 2
        let after_seed = tailer.poll().expect("poll");
        let revs: Vec<u64> = after_seed.iter().map(|e| e.rev).collect();
        assert_eq!(revs, vec![3, 4], "seed(2) skips revs 0-2");
    }

    #[test]
    fn poll_on_missing_dir_returns_empty() {
        let dir = tempdir().expect("dir");
        let mut tailer = Tailer::new(dir.path().join("nonexistent"));
        assert!(tailer.poll().expect("poll").is_empty());
    }

    // ── hydrate tests ──────────────────────────────────────────────────

    fn make_channel(bodies_dir: &Path) -> AgentChannel {
        AgentChannel {
            socket_path: PathBuf::from("/tmp/unused.sock"),
            cap_token: "tok".to_owned(),
            bodies_dir: bodies_dir.to_path_buf(),
        }
    }

    #[test]
    fn hydrate_reads_back_verified_body() {
        let dir = tempdir().expect("dir");
        let bodies = dir.path().join(BODIES_DIR);
        fs::create_dir_all(&bodies).expect("mkdir");

        let content = b"the quick brown fox";
        let hash = ContentHash::of(content);
        fs::write(bodies.join(hash.to_hex()), content).expect("write");

        let ch = make_channel(&bodies);
        let fetched = ch.hydrate(hash).expect("hydrate");
        assert_eq!(fetched, Some(content.to_vec()));
    }

    #[test]
    fn hydrate_returns_none_for_missing() {
        let dir = tempdir().expect("dir");
        let bodies = dir.path().join(BODIES_DIR);
        fs::create_dir_all(&bodies).expect("mkdir");

        let ch = make_channel(&bodies);
        assert_eq!(ch.hydrate(ContentHash::new([0xFF; 32])).expect("hydrate"), None);
    }

    #[test]
    fn hydrate_detects_corruption() {
        let dir = tempdir().expect("dir");
        let bodies = dir.path().join(BODIES_DIR);
        fs::create_dir_all(&bodies).expect("mkdir");

        let hash = ContentHash::of(b"original");
        fs::write(bodies.join(hash.to_hex()), b"tampered").expect("write");

        let ch = make_channel(&bodies);
        assert!(ch.hydrate(hash).is_err(), "a hash mismatch must error");
    }

    // ── send tests ─────────────────────────────────────────────────────

    /// Build a minimal Command for testing.
    fn test_command(dedup: &str) -> Command {
        Command {
            schema_version: 1,
            id: format!("cmd-{dedup}"),
            seq: 1,
            dedup_token: dedup.to_owned(),
            kind: cp_wire::types::command::Kind::Stop,
        }
    }

    /// A minimal echo-ack server: reads one framed CommandFrame, writes back
    /// a canned Accepted Ack with rev=42, then closes.
    fn echo_ack_server(listener: UnixListener) {
        let (mut conn, _addr) = listener.accept().expect("accept");
        let mut buf: Vec<u8> = Vec::new();
        let mut chunk = [0u8; READ_CHUNK];
        loop {
            let n = conn.read(&mut chunk).expect("read");
            if let Some(got) = chunk.get(..n) {
                buf.extend_from_slice(got);
            }
            if n == 0 {
                break;
            }
        }
        // We don't need to parse the command — just ack it.
        let ack = Ack {
            schema_version: 1,
            cmd_id: "cmd-echo".to_owned(),
            status: Status::Accepted,
            rev: Some(42),
        };
        let payload = serde_json::to_vec(&ack).expect("ser");
        let frame = framing::encode_raw(&payload).expect("frame");
        conn.write_all(&frame).expect("write");
    }

    #[test]
    fn send_receives_ack_from_agent() {
        let dir = tempdir().expect("dir");
        let sock = dir.path().join("test.sock");
        let listener = UnixListener::bind(&sock).expect("bind");

        let server = thread::spawn(move || echo_ack_server(listener));

        let ch = AgentChannel {
            socket_path: sock,
            cap_token: "tok".to_owned(),
            bodies_dir: PathBuf::new(),
        };
        let ack = ch.send(test_command("echo")).expect("send");
        assert_eq!(ack.status, Status::Accepted);
        assert_eq!(ack.rev, Some(42));

        server.join().expect("join");
    }

    #[test]
    fn send_returns_error_on_connection_refused() {
        let ch = AgentChannel {
            socket_path: PathBuf::from("/tmp/nonexistent_socket_for_test.sock"),
            cap_token: "tok".to_owned(),
            bodies_dir: PathBuf::new(),
        };
        let result = ch.send(test_command("fail"));
        assert!(result.is_err(), "connection refused must surface as an error");
    }
}
