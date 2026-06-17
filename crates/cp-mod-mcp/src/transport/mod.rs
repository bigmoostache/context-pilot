//! Transport abstraction: how JSON-RPC frames travel to and from a server.
//!
//! Phase 1 ships [`pipe`] (stdio) only. A future HTTP/SSE transport will
//! implement the same [`Transport`] contract so [`crate::clients`] stays
//! transport-agnostic.

pub mod pipe;

use crate::errors::McpError;
use crate::protocol::Incoming;

/// A bidirectional JSON-RPC channel to a single MCP server.
///
/// Implementations own the underlying resource (subprocess, socket) and are
/// responsible for framing. [`send_line`](Transport::send_line) writes one
/// serialized message; [`recv`](Transport::recv) yields the next inbound
/// message or times out.
pub trait Transport: Send {
    /// Write one already-serialized JSON message (newline framing is added by
    /// the implementation as the transport requires).
    ///
    /// # Errors
    ///
    /// Returns [`McpError::Transport`] if the underlying channel is closed or
    /// the write fails.
    fn send_line(&mut self, json: &str) -> Result<(), McpError>;

    /// Block for the next inbound message, up to `timeout_ms`.
    ///
    /// # Errors
    ///
    /// Returns [`McpError::Timeout`] if no message arrives in time, or
    /// [`McpError::Transport`] if the channel closed.
    fn recv(&mut self, timeout_ms: u64) -> Result<Incoming, McpError>;
}
