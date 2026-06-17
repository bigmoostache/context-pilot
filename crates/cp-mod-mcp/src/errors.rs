//! Error type for the MCP client.
//!
//! A self-contained `Debug + Display` enum (no `thiserror` dependency) covering
//! the failure modes of subprocess spawning, transport I/O, request timeouts,
//! and server-reported JSON-RPC errors. It deliberately does NOT implement
//! [`std::error::Error`] — nothing in the crate needs `dyn Error`, and the
//! trait's defaulted methods (`provide`, etc.) cannot be satisfied on stable
//! under the workspace's `missing_trait_methods` lint.

use crate::protocol::RpcError;

/// Failure modes of an MCP client operation.
#[derive(Debug)]
pub enum McpError {
    /// The server subprocess could not be spawned.
    Spawn(String),
    /// A transport-level I/O failure (write/read/closed channel).
    Transport(String),
    /// No response arrived within the configured timeout.
    Timeout,
    /// The server returned a JSON-RPC error object.
    Rpc(RpcError),
    /// A response was structurally valid JSON-RPC but its payload did not match
    /// the expected shape (e.g. missing `result`, undecodable result).
    Protocol(String),
}

impl core::fmt::Display for McpError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Spawn(msg) => write!(f, "failed to spawn MCP server: {msg}"),
            Self::Transport(msg) => write!(f, "MCP transport error: {msg}"),
            Self::Timeout => write!(f, "MCP request timed out"),
            Self::Rpc(err) => write!(f, "{err}"),
            Self::Protocol(msg) => write!(f, "MCP protocol error: {msg}"),
        }
    }
}
