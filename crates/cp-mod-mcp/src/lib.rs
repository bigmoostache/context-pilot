//! Generic Model Context Protocol (MCP) client.
//!
//! Connects to MCP servers, discovers their tools at runtime, and (in later
//! phases) bridges those tools into Context Pilot's tool pipeline. Phase 1 is a
//! standalone, transport-tested client: spawn a stdio server, `initialize`,
//! `tools/list`, `tools/call`. No auth, no host wiring yet.
//!
//! # Example
//!
//! ```no_run
//! use cp_mod_mcp::McpClient;
//!
//! let mut client = McpClient::connect_stdio(
//!     "npx",
//!     &["-y".to_owned(), "@modelcontextprotocol/server-everything".to_owned()],
//! )?;
//! let tools = client.list_tools()?;
//! println!("{} tools", tools.len());
//! # Ok::<(), cp_mod_mcp::McpError>(())
//! ```

pub mod client;
pub mod error;
pub mod protocol;
pub mod transport;

pub use client::McpClient;
pub use error::McpError;
pub use protocol::{CallToolResult, ServerInfo, Tool};
