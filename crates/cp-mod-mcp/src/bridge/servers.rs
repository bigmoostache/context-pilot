//! Live MCP host state — connected servers, their discovered tools, and status.
//!
//! Stored in Context Pilot's `State` extension map (`set_ext`/`ext`/`ext_mut`),
//! which requires `Send + Sync`. [`McpClient`] owns a subprocess and an mpsc
//! `Receiver` (`Send` but `!Sync`), so each client lives behind a [`Mutex`] —
//! that makes [`McpServerEntry`] (and thus [`McpState`]) `Sync`.

use std::collections::HashMap;
use std::sync::Mutex;

use cp_base::state::runtime::State;

use crate::clients::McpClient;
use crate::protocol::Tool;
use crate::transport::pipe::SubprocessTransport;

/// Connection outcome for a single configured server.
#[derive(Debug, Clone)]
pub enum ConnStatus {
    /// Handshake succeeded; the server is serving `n` discovered tools.
    Connected {
        /// Number of tools advertised by the server.
        tool_count: usize,
    },
    /// Spawn, handshake, or `tools/list` failed. Carries the error message.
    Failed(String),
    /// Configured with a `url` (remote transport) — not yet supported (Phase 3).
    Unsupported(String),
}

impl ConnStatus {
    /// Short label for the status panel.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Connected { tool_count } => format!("connected ({tool_count} tools)"),
            Self::Failed(e) => format!("failed: {e}"),
            Self::Unsupported(reason) => format!("unsupported: {reason}"),
        }
    }

    /// Whether the server is live and usable.
    #[must_use]
    pub const fn is_connected(&self) -> bool {
        matches!(self, Self::Connected { .. })
    }
}

/// A single connected (or failed) MCP server.
#[derive(Debug)]
pub struct McpServerEntry {
    /// Live client, behind a `Mutex` to satisfy the `Sync` bound of the state map.
    /// `None` when the server failed to connect or is unsupported.
    pub client: Option<Mutex<McpClient<SubprocessTransport>>>,
    /// Snapshot of the tools advertised at connection time.
    pub tools: Vec<Tool>,
    /// Connection outcome (drives the status panel).
    pub status: ConnStatus,
}

impl McpServerEntry {
    /// A connected entry wrapping a live client and its discovered tools.
    #[must_use]
    pub const fn connected(client: McpClient<SubprocessTransport>, tools: Vec<Tool>) -> Self {
        let status = ConnStatus::Connected { tool_count: tools.len() };
        Self { client: Some(Mutex::new(client)), tools, status }
    }

    /// A failed entry — no client, carries the error for display.
    #[must_use]
    pub fn failed<S: Into<String>>(error: S) -> Self {
        Self { client: None, tools: Vec::new(), status: ConnStatus::Failed(error.into()) }
    }

    /// An unsupported entry (e.g. a remote `url` server before Phase 3).
    #[must_use]
    pub fn unsupported<S: Into<String>>(reason: S) -> Self {
        Self { client: None, tools: Vec::new(), status: ConnStatus::Unsupported(reason.into()) }
    }
}

/// Host-side registry of all configured MCP servers, keyed by server name.
#[derive(Debug, Default)]
pub struct McpState {
    /// Configured servers (connected, failed, or unsupported).
    pub servers: HashMap<String, McpServerEntry>,
}

impl McpState {
    /// Shared ref from the `State` extension map.
    ///
    /// # Panics
    ///
    /// Panics if the module's `init_state` never ran (extension absent).
    #[must_use]
    pub fn get(state: &State) -> &Self {
        state.ext::<Self>()
    }

    /// Mutable ref from the `State` extension map.
    ///
    /// # Panics
    ///
    /// Panics if the module's `init_state` never ran (extension absent).
    pub fn get_mut(state: &mut State) -> &mut Self {
        state.ext_mut::<Self>()
    }

    /// Server names in stable sorted order (deterministic panel + tool listing).
    #[must_use]
    pub fn sorted_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.servers.keys().cloned().collect();
        names.sort();
        names
    }

    /// Total tools across all connected servers.
    #[must_use]
    pub fn total_tools(&self) -> usize {
        self.servers.values().map(|s| s.tools.len()).sum()
    }
}
