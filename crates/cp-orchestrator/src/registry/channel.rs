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
//! The incremental oplog consumer that feeds the materialized view lives in the
//! sibling [`tailer`](crate::registry::tailer) module; [`Tailer`] is re-exported
//! here so it remains reachable at the stable `channel::Tailer` path.

use std::io::{self, Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{fs, thread};

use cp_wire::framing;
use cp_wire::types::ack::Ack;
use cp_wire::types::command::{Command, Frame as CommandFrame};
use cp_wire::types::registry::Entry;
use cp_wire::types::ContentHash;

/// Re-export so the tailer stays reachable at the historical `channel::Tailer`
/// path despite living in its own file (call sites and rustdoc links are
/// unchanged by the split).
pub use crate::registry::tailer::Tailer;

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
    use cp_wire::types::ack::Status;
    use std::os::unix::net::UnixListener;
    use tempfile::tempdir;

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
