//! `.mcp.json` discovery and parsing.
//!
//! Server list lives in `.context-pilot/shared/mcp.json` (per-project, shareable),
//! falling back to `~/.context-pilot/mcp.json` (global). De-facto format:
//!
//! ```json
//! {
//!   "mcpServers": {
//!     "filesystem": { "command": "npx", "args": ["-y", "@modelcontextprotocol/server-filesystem", "."] },
//!     "notion":     { "url": "https://mcp.notion.com/mcp" }
//!   }
//! }
//! ```
//!
//! Phase 2 supports `command`/`args` (stdio) only. A `url` entry is parsed but
//! flagged unsupported until the HTTP transport lands (Phase 3).

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;

/// Project-relative location of the shared MCP config.
const PROJECT_CONFIG: &str = ".context-pilot/shared/mcp.json";

/// Root of the config file.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Manifest {
    /// Map of server name → launch spec.
    #[serde(default, rename = "mcpServers")]
    pub servers: HashMap<String, ServerSpec>,
}

/// One server's launch specification. Either a stdio command or a remote url.
#[derive(Debug, Clone, Deserialize)]
pub struct ServerSpec {
    /// Executable to spawn for a stdio server (e.g. `"npx"`).
    #[serde(default)]
    pub command: Option<String>,
    /// Arguments passed to `command`.
    #[serde(default)]
    pub args: Vec<String>,
    /// Remote endpoint for an HTTP/SSE server (Phase 3+).
    #[serde(default)]
    pub url: Option<String>,
}

impl ServerSpec {
    /// Stdio launch pair `(command, args)` when this is a stdio server.
    #[must_use]
    pub fn stdio(&self) -> Option<(&str, &[String])> {
        self.command.as_deref().map(|c| (c, self.args.as_slice()))
    }
}

/// Locate the active MCP config file: project first, then global.
/// Returns `None` if neither exists.
#[must_use]
pub fn resolve() -> Option<PathBuf> {
    let project = PathBuf::from(PROJECT_CONFIG);
    if project.is_file() {
        return Some(project);
    }
    let home = std::env::var("HOME").ok()?;
    let global = PathBuf::from(home).join(".context-pilot").join("mcp.json");
    global.is_file().then_some(global)
}

/// Load and parse the MCP config, or `Ok(default/empty)` when no file exists.
///
/// # Errors
///
/// Returns a human-readable message on read or JSON-parse failure.
pub fn load() -> Result<Manifest, String> {
    let Some(path) = resolve() else {
        return Ok(Manifest::default());
    };
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_str(&raw).map_err(|e| format!("parse {}: {e}", path.display()))
}
