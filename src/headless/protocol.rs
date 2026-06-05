//! JSON-line wire protocol between daemon and client.
//!
//! One JSON object per line, newline-delimited — same pattern as `cp-console-server`.
//! Full frames on every dirty render tick (no deltas). See design doc §4–§5.

use cp_render::frame::Frame;
use crossterm::event::Event;
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, Write};

// ── Client → Daemon ──────────────────────────────────────────────

/// Messages sent from an attached client to the daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) enum ClientMessage {
    /// Terminal key/mouse/resize event forwarded to the daemon's event loop.
    Input { event: Event },

    /// Client is attaching — includes terminal dimensions so the daemon
    /// can adapt size-dependent logic to the most recently attached client.
    Attach { cols: u16, rows: u16 },

    /// Client is detaching gracefully (Ctrl+Z). Daemon keeps running.
    Detach,

    /// Client requests full daemon shutdown (Ctrl+Q).
    Quit,

    /// Keepalive ping.
    Ping,
}

// ── Daemon → Client ──────────────────────────────────────────────

/// Messages sent from the daemon to all connected clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) enum DaemonMessage {
    /// Full IR frame snapshot. Sent on every dirty render tick.
    /// The first frame after `Attach` acts as the initial state snapshot.
    FrameUpdate { frame: Frame },

    /// Response to a client `Ping`.
    Pong,

    /// Daemon is shutting down — clients should disconnect and exit.
    Shutdown,
}

// ── Protocol errors ──────────────────────────────────────────────

/// Errors that can occur during message read/write.
#[derive(Debug)]
pub(crate) enum ProtocolError {
    /// Underlying I/O failure (broken pipe, socket reset, etc.).
    Io(io::Error),
    /// JSON serialization or deserialization failure.
    Json(serde_json::Error),
    /// The other end closed the connection (EOF on read).
    ConnectionClosed,
}

impl From<io::Error> for ProtocolError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for ProtocolError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "protocol I/O error: {e}"),
            Self::Json(e) => write!(f, "protocol JSON error: {e}"),
            Self::ConnectionClosed => write!(f, "connection closed by remote end"),
        }
    }
}

// ── Read / Write helpers ─────────────────────────────────────────

/// Serialize a message as a single JSON line (newline-terminated) and flush.
///
/// Each message is atomic — one JSON object per line, flushed immediately
/// so the reader sees it without buffering delay.
pub(crate) fn write_message<W: Write, M: Serialize>(writer: &mut W, msg: &M) -> Result<(), ProtocolError> {
    serde_json::to_writer(&mut *writer, msg)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

/// Read one JSON-line message from a buffered reader.
///
/// Blocks until a full line is available. Returns [`ProtocolError::ConnectionClosed`]
/// on EOF (the other end disconnected).
pub(crate) fn read_message<R: BufRead, M: for<'de> Deserialize<'de>>(reader: &mut R) -> Result<M, ProtocolError> {
    let mut line = String::new();
    let bytes_read = reader.read_line(&mut line)?;
    if bytes_read == 0 {
        return Err(ProtocolError::ConnectionClosed);
    }
    Ok(serde_json::from_str(&line)?)
}
