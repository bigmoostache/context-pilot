//! Multi-client Unix socket server for the headless daemon.
//!
//! Accepts client connections, spawns a reader thread per client, and
//! broadcasts serialized IR frames to all connected writers. The main
//! daemon loop calls [`SocketServer::accept_pending`] and [`SocketServer::poll_events`]
//! on each tick, keeping everything on a single thread except for the
//! per-client readers (which feed an mpsc channel).

use super::protocol::{self, ClientMessage, DaemonMessage, ProtocolError};
use cp_render::frame::Frame;
use std::collections::HashMap;
use std::io::{self, BufReader, BufWriter, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

// ── Server events (internal) ────────────────────────────────────

/// Internal events produced by per-client reader threads.
enum ServerEvent {
    /// A client sent a protocol message.
    Message { client_id: u64, msg: ClientMessage },
    /// A client's reader thread detected disconnection (EOF or I/O error).
    Disconnected { client_id: u64 },
}

// ── Per-client writer handle ────────────────────────────────────

/// Writer half of a connected client, held by the main thread.
struct ClientWriter {
    writer: BufWriter<UnixStream>,
    /// Terminal dimensions reported on Attach.
    cols: u16,
    rows: u16,
}

// ── Public API ──────────────────────────────────────────────────

/// A client message paired with the client that sent it.
pub(crate) struct ClientEvent {
    pub client_id: u64,
    pub message: ClientMessage,
}

/// Multi-client Unix socket server.
///
/// The server owns a [`UnixListener`] and a set of connected client writers.
/// Reader threads run in the background, feeding messages into an mpsc channel
/// that the main loop drains via [`poll_events`](SocketServer::poll_events).
pub(crate) struct SocketServer {
    listener: UnixListener,
    clients: HashMap<u64, ClientWriter>,
    rx: Receiver<ServerEvent>,
    tx: Sender<ServerEvent>,
    next_client_id: u64,
    socket_path: PathBuf,
}

impl SocketServer {
    /// Bind a new server to the given Unix socket path.
    ///
    /// Removes any stale socket file from a previous run before binding.
    /// The listener is set to non-blocking so [`accept_pending`](Self::accept_pending)
    /// returns immediately when no client is waiting.
    pub(crate) fn bind(socket_path: &Path) -> io::Result<Self> {
        // Clean up stale socket from a previous daemon
        if socket_path.exists() {
            std::fs::remove_file(socket_path)?;
        }

        let listener = UnixListener::bind(socket_path)?;
        listener.set_nonblocking(true)?;

        let (tx, rx) = mpsc::channel();

        Ok(Self {
            listener,
            clients: HashMap::new(),
            rx,
            tx,
            next_client_id: 0,
            socket_path: socket_path.to_path_buf(),
        })
    }

    /// Accept any pending client connections (non-blocking).
    ///
    /// For each new connection, clones the stream — the read half goes to a
    /// background reader thread, the write half stays in the `clients` map.
    /// Call this on every main-loop tick.
    pub(crate) fn accept_pending(&mut self) {
        loop {
            match self.listener.accept() {
                Ok((stream, _addr)) => {
                    let client_id = self.next_client_id;
                    self.next_client_id += 1;

                    // Clone stream: reader thread gets one handle, writer stays here
                    let reader_stream = match stream.try_clone() {
                        Ok(s) => s,
                        Err(e) => {
                            log::warn!("headless: failed to clone stream for client {client_id}: {e}");
                            continue;
                        }
                    };

                    // Writer stays on the main thread — set blocking for writes
                    if let Err(e) = stream.set_nonblocking(false) {
                        log::warn!("headless: failed to set writer blocking for client {client_id}: {e}");
                    }

                    drop(
                        self.clients
                            .insert(client_id, ClientWriter { writer: BufWriter::new(stream), cols: 80, rows: 24 }),
                    );

                    // Spawn reader thread
                    let tx = self.tx.clone();
                    if let Err(e) =
                        thread::Builder::new().name(format!("headless-reader-{client_id}")).spawn(move || {
                            Self::client_reader_loop(client_id, reader_stream, tx);
                        })
                    {
                        log::error!("headless: failed to spawn reader thread for client {client_id}: {e}");
                    }

                    log::info!("headless: client {client_id} connected ({} total)", self.clients.len());
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => {
                    log::warn!("headless: accept error: {e}");
                    break;
                }
            }
        }
    }

    /// Drain all pending events from client reader threads.
    ///
    /// Processes disconnections internally (removes dead clients) and
    /// returns only actionable client messages. Call on every main-loop tick.
    pub(crate) fn poll_events(&mut self) -> Vec<ClientEvent> {
        let mut events = Vec::new();

        while let Ok(event) = self.rx.try_recv() {
            match event {
                ServerEvent::Message { client_id, msg } => {
                    // Update terminal dimensions on Attach
                    if let ClientMessage::Attach { cols, rows } = &msg {
                        if let Some(client) = self.clients.get_mut(&client_id) {
                            client.cols = *cols;
                            client.rows = *rows;
                        }
                    }

                    events.push(ClientEvent { client_id, message: msg });
                }
                ServerEvent::Disconnected { client_id } => {
                    drop(self.clients.remove(&client_id));
                    log::info!("headless: client {client_id} disconnected ({} remaining)", self.clients.len());
                }
            }
        }

        events
    }

    /// Serialize an IR frame once and broadcast to all connected clients.
    ///
    /// Clients that fail to receive are disconnected silently. Returns
    /// the number of clients that successfully received the frame.
    pub(crate) fn broadcast_frame(&mut self, frame: &Frame) -> usize {
        // Serialize once — all clients get the same bytes
        let msg = DaemonMessage::FrameUpdate { frame: frame.clone() };
        self.broadcast_message(&msg)
    }

    /// Broadcast an arbitrary daemon message to all connected clients.
    ///
    /// Returns the number of clients that successfully received it.
    pub(crate) fn broadcast_message(&mut self, msg: &DaemonMessage) -> usize {
        let json = match serde_json::to_string(msg) {
            Ok(j) => j,
            Err(e) => {
                log::error!("headless: failed to serialize message: {e}");
                return 0;
            }
        };

        let line = format!("{json}\n");
        let bytes = line.as_bytes();
        let mut dead = Vec::new();

        for (&id, client) in &mut self.clients {
            if client.writer.write_all(bytes).is_err() || client.writer.flush().is_err() {
                dead.push(id);
            }
        }

        for id in &dead {
            drop(self.clients.remove(id));
            log::info!("headless: client {id} dropped (write failed)");
        }

        self.clients.len()
    }

    /// Number of currently connected clients.
    pub(crate) fn client_count(&self) -> usize {
        self.clients.len()
    }

    /// Terminal dimensions of the most recently attached client.
    ///
    /// Falls back to 80×24 if no clients are connected.
    pub(crate) fn latest_terminal_size(&self) -> (u16, u16) {
        self.clients.values().last().map_or((80, 24), |c| (c.cols, c.rows))
    }

    /// Send a shutdown message to all clients and clean up.
    pub(crate) fn shutdown(&mut self) {
        let _ = self.broadcast_message(&DaemonMessage::Shutdown);
        self.clients.clear();
        // Socket file cleaned up in Drop
    }

    // ── Private ──────────────────────────────────────────────────

    /// Reader loop running on a dedicated thread per client.
    ///
    /// Reads JSON-line messages from the client socket and forwards them
    /// through the mpsc channel. Exits on EOF or I/O error, sending a
    /// `Disconnected` event so the main thread can clean up the writer.
    fn client_reader_loop(client_id: u64, stream: UnixStream, tx: Sender<ServerEvent>) {
        // Reader side should be blocking — we want to park this thread
        if let Err(e) = stream.set_nonblocking(false) {
            log::warn!("headless: reader {client_id} failed to set blocking: {e}");
        }

        let mut reader = BufReader::new(stream);

        loop {
            match protocol::read_message::<_, ClientMessage>(&mut reader) {
                Ok(msg) => {
                    if tx.send(ServerEvent::Message { client_id, msg }).is_err() {
                        break; // Server dropped — exit quietly
                    }
                }
                Err(ProtocolError::ConnectionClosed) => {
                    log::debug!("headless: reader {client_id} got EOF");
                    break;
                }
                Err(e) => {
                    log::warn!("headless: reader {client_id} error: {e}");
                    break;
                }
            }
        }

        // Notify main thread that this client is gone
        drop(tx.send(ServerEvent::Disconnected { client_id }));
    }
}

impl Drop for SocketServer {
    fn drop(&mut self) {
        // Clean up the socket file so the next daemon can bind
        drop(std::fs::remove_file(&self.socket_path));
    }
}
