//! Generic Model Context Protocol (MCP) client.
//!
//! Connects to MCP servers, discovers their tools at runtime, and bridges those
//! tools into Context Pilot's tool pipeline. The [`clients`]/[`protocol`]/[`transport`]
//! layers are a standalone, transport-tested MCP client (spawn a stdio server,
//! `initialize`, `tools/list`, `tools/call`). The [`bridge`] layer wires that
//! client into Context Pilot as a [`Module`](cp_base::modules::Module): config
//! discovery, dynamic tool registration, dispatch, and a status panel.
//!
//! # Example
//!
//! ```no_run
//! use cp_mod_mcp::clients::McpClient;
//!
//! let mut client = McpClient::connect_stdio(
//!     "npx",
//!     &["-y".to_owned(), "@modelcontextprotocol/server-everything".to_owned()],
//! )?;
//! let tools = client.list_tools()?;
//! println!("{} tools", tools.len());
//! # Ok::<(), cp_mod_mcp::errors::McpError>(())
//! ```

pub mod bridge;
pub mod clients;
pub mod errors;
pub mod oauth;
pub mod protocol;
pub mod transport;
