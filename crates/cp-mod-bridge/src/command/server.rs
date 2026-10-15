//! [`Server`] — runs the command intake on its own thread, off the agent's
//! main loop.
//!
//! Serving a connection used to happen inline on the main loop, where three
//! things could stall it: the socket reads, the `fdatasync` that
//! [`append_durable`] waits for on every fresh command (journal-then-ack, I11),
//! and the query responder's HTTP round-trip to Meilisearch. The server thread
//! now does all of that. The main loop only calls [`Server::drain`], a
//! non-blocking `try_recv` that hands back the commands already journaled and
//! acked, in arrival order, for it to apply.
//!
//! Exactly-once is unchanged: the thread owns the single [`Intake`] (and so the
//! single `SeenSet`), and a command reaches the channel only after its effect
//! is durable. Each connection is bounded by the intake's whole-connection
//! deadline, so one slow commander delays the next one by at most that budget.
//!
//! [`append_durable`]: cp_oplog::service::Service::append_durable

use std::fmt;
use std::io::{self, ErrorKind};
use std::os::fd::AsFd as _;
use std::os::unix::net::UnixListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

use cp_oplog::service::Service as OplogService;
use cp_wire::types::command::Command;
use cp_wire::types::payload::query::{Outcome, Query};
use nix::poll::{PollFd, PollFlags, poll};

use super::Intake;

/// How long one `poll` on the listener waits before re-checking the stop flag.
/// Bounds how long dropping the [`Server`] takes; an incoming connection wakes
/// the poll at once, so it adds no command latency.
const POLL_INTERVAL_MS: u16 = 100;

/// A query responder the thread can call: owned, shareable across threads.
pub type SharedResponder = Arc<dyn Fn(&Query) -> Outcome + Send + Sync>;

/// The installed responder plus the key it was built from, so the caller can
/// tell whether it needs replacing (for example after the search port changed).
type ResponderSlot = Mutex<Option<(String, SharedResponder)>>;

/// Handle to the running intake thread.
pub struct Server {
    /// Commands journaled and acked by the thread, waiting to be applied.
    commands: Mutex<Receiver<Command>>,
    /// The query responder, swappable while the thread runs.
    responder: Arc<ResponderSlot>,
    /// Set on drop to make the thread exit.
    stop: Arc<AtomicBool>,
    /// The thread, joined on drop.
    handle: Option<JoinHandle<()>>,
}

impl fmt::Debug for Server {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Server").field("running", &self.handle.is_some()).finish_non_exhaustive()
    }
}

impl Server {
    /// Start serving `listener` on a new thread with `intake`.
    ///
    /// `listener` should be a clone of the boot listener (`try_clone`); it is
    /// switched to non-blocking so the thread can accept every pending
    /// connection after a single poll wake-up.
    ///
    /// # Errors
    ///
    /// Returns an [`io::Error`] if the listener cannot be made non-blocking or
    /// the thread cannot be spawned.
    pub fn spawn(listener: UnixListener, oplog: Arc<OplogService>, intake: Intake) -> io::Result<Self> {
        listener.set_nonblocking(true)?;
        let (tx, rx) = channel();
        let responder: Arc<ResponderSlot> = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let worker = Worker { listener, oplog, intake, tx, responder: Arc::clone(&responder), stop: Arc::clone(&stop) };
        let handle = std::thread::Builder::new().name("bridge-intake".to_owned()).spawn(move || worker.run())?;
        Ok(Self { commands: Mutex::new(rx), responder, stop, handle: Some(handle) })
    }

    /// Every command accepted since the last call, in arrival order. Never
    /// blocks on I/O: the only lock is the receiver's, held by this call alone.
    pub fn drain(&self) -> Vec<Command> {
        let rx = self.commands.lock().unwrap_or_else(PoisonError::into_inner);
        let mut out = Vec::new();
        loop {
            match rx.try_recv() {
                Ok(cmd) => out.push(cmd),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return out,
            }
        }
    }

    /// The key of the installed responder, or `None` when there is none.
    pub fn responder_key(&self) -> Option<String> {
        let slot = self.responder.lock().unwrap_or_else(PoisonError::into_inner);
        slot.as_ref().map(|installed| installed.0.clone())
    }

    /// Install (or clear, with `None`) the query responder, tagged with `key`.
    pub fn set_responder(&self, installed: Option<(String, SharedResponder)>) {
        let mut slot = self.responder.lock().unwrap_or_else(PoisonError::into_inner);
        *slot = installed;
    }
}

impl Drop for Server {
    /// Stop the thread and wait for it. It notices within one poll interval,
    /// or after the connection it is serving ends (bounded by the deadline).
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            let _joined = handle.join();
        }
    }
}

/// Everything the intake thread owns.
struct Worker {
    /// Non-blocking clone of the boot listener.
    listener: UnixListener,
    /// The agent's oplog, for journal-then-ack.
    oplog: Arc<OplogService>,
    /// The single dedup authority.
    intake: Intake,
    /// Where accepted commands go.
    tx: Sender<Command>,
    /// The current query responder.
    responder: Arc<ResponderSlot>,
    /// Set when the [`Server`] is dropped.
    stop: Arc<AtomicBool>,
}

impl Worker {
    /// Poll, accept, serve, until stopped or the [`Server`] is gone.
    fn run(mut self) {
        while !self.stop.load(Ordering::Acquire) {
            if !self.wait_readable() {
                continue;
            }
            if !self.accept_pending() {
                return; // receiver dropped: nobody left to apply commands.
            }
        }
    }

    /// Wait up to [`POLL_INTERVAL_MS`] for a pending connection.
    fn wait_readable(&self) -> bool {
        let mut fds = [PollFd::new(self.listener.as_fd(), PollFlags::POLLIN)];
        poll(&mut fds, POLL_INTERVAL_MS).is_ok_and(|ready| ready > 0i32)
    }

    /// Serve every pending connection. `false` once the receiver is gone.
    fn accept_pending(&mut self) -> bool {
        loop {
            let mut stream = match self.listener.accept() {
                Ok((stream, _addr)) => stream,
                Err(e) if e.kind() == ErrorKind::WouldBlock => return true,
                Err(e) => {
                    log::error!("bridge: command accept failed: {e}");
                    return true;
                }
            };
            let responder = self.current_responder();
            let answer = |query: &Query| {
                responder.as_ref().map_or_else(
                    || Outcome::Error { reason: "search unavailable on this agent".to_owned() },
                    |respond| respond(query),
                )
            };
            let served = self.intake.serve_connection(&self.oplog, &mut stream, &answer);
            if let Some(e) = served.error {
                log::error!("bridge: command intake error: {e:?}");
            }
            for cmd in served.applied {
                if self.tx.send(cmd).is_err() {
                    return false;
                }
            }
        }
    }

    /// Snapshot the installed responder (the lock is released before use).
    fn current_responder(&self) -> Option<SharedResponder> {
        let slot = self.responder.lock().unwrap_or_else(PoisonError::into_inner);
        slot.as_ref().map(|installed| Arc::clone(&installed.1))
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read as _, Write as _};
    use std::os::unix::net::UnixStream;
    use std::time::{Duration, Instant};

    use cp_wire::framing;
    use cp_wire::types::ack::{Ack, Status};
    use cp_wire::types::command::{Frame as CommandFrame, Kind as CommandKind};

    use super::*;

    const TOKEN: &str = "cap-token-256bit";

    fn frame(dedup: &str) -> Vec<u8> {
        let cf = CommandFrame::new(
            TOKEN.to_owned(),
            Command::new(
                format!("cmd-{dedup}"),
                1,
                dedup.to_owned(),
                CommandKind::SendMessage { agent_id: "A1".to_owned(), content: "hi".to_owned() },
            ),
        );
        framing::encode_raw(&serde_json::to_vec(&cf).expect("ser")).expect("frame")
    }

    /// Send one command on a fresh connection and return its ack status.
    fn send(path: &std::path::Path, bytes: &[u8]) -> Status {
        let mut client = UnixStream::connect(path).expect("connect");
        client.write_all(bytes).expect("write");
        client.shutdown(std::net::Shutdown::Write).expect("half-close");
        let mut buf = Vec::new();
        let _n = client.read_to_end(&mut buf).expect("read ack");
        let (payload, _used) = framing::decode_raw(&buf).expect("ack frame");
        serde_json::from_slice::<Ack>(payload).expect("ack").status
    }

    /// Poll `drain` until it yields something or a second passes.
    fn drain_soon(server: &Server) -> Vec<Command> {
        let started = Instant::now();
        loop {
            let got = server.drain();
            if !got.is_empty() || started.elapsed() > Duration::from_secs(1) {
                return got;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn start(dir: &std::path::Path) -> (Server, std::path::PathBuf) {
        let path = dir.join("cmd.sock");
        let listener = UnixListener::bind(&path).expect("bind");
        let oplog = Arc::new(OplogService::spawn(dir.join("oplog")).expect("oplog"));
        let intake = Intake::new(&dir.join("oplog"), TOKEN.to_owned()).expect("intake");
        (Server::spawn(listener, oplog, intake).expect("spawn"), path)
    }

    #[test]
    fn accepted_commands_reach_drain_once() {
        let dir = tempfile::tempdir().expect("tmp");
        let (server, path) = start(dir.path());
        assert_eq!(send(&path, &frame("d1")), Status::Accepted);
        let first = drain_soon(&server);
        assert_eq!(first.len(), 1, "the fresh command is handed over");

        assert_eq!(send(&path, &frame("d1")), Status::Accepted, "a duplicate is still acked");
        std::thread::sleep(Duration::from_millis(50));
        assert!(server.drain().is_empty(), "a duplicate is never applied twice");
    }

    #[test]
    fn a_wedged_commander_does_not_block_drain_or_later_commanders() {
        let dir = tempfile::tempdir().expect("tmp");
        let (server, path) = start(dir.path());
        // Connects and never writes: the intake thread waits on it, the caller
        // of `drain` does not.
        let _wedged = UnixStream::connect(&path).expect("connect");
        let started = Instant::now();
        assert!(server.drain().is_empty());
        assert!(started.elapsed() < Duration::from_millis(50), "drain never waits on a socket");
        drop(server);
    }

    #[test]
    fn responder_key_tracks_installs() {
        let dir = tempfile::tempdir().expect("tmp");
        let (server, _path) = start(dir.path());
        assert_eq!(server.responder_key(), None);
        let respond: SharedResponder = Arc::new(|_q: &Query| Outcome::Hits { hits: Vec::new() });
        server.set_responder(Some(("7700".to_owned(), respond)));
        assert_eq!(server.responder_key().as_deref(), Some("7700"));
        server.set_responder(None);
        assert_eq!(server.responder_key(), None);
    }
}
