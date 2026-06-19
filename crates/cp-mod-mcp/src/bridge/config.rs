//! `.mcp.json` discovery, parsing, and merge.
//!
//! Two config layers are merged (global base + local override):
//!
//! 1. **Global** — `~/.context-pilot/mcp.json`  (user-wide defaults)
//! 2. **Local**  — `.context-pilot/shared/mcp.json` (per-project overrides)
//!
//! Servers with the same key in the local file replace the global entry.
//! Both files are optional; an empty manifest is used when neither exists.
//!
//! ```json
//! {
//!   "mcpServers": {
//!     "filesystem": { "command": "npx", "args": ["-y", "@modelcontextprotocol/server-filesystem", "."] },
//!     "notion":     { "url": "https://mcp.notion.com/mcp", "bearer_token": "ntn_xxx" }
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
    /// Static bearer token for remote servers (sent as `Authorization: Bearer`).
    /// Omit for unauthenticated servers or when using OAuth.
    #[serde(default)]
    pub bearer_token: Option<String>,
    /// Whitelist: only expose tools whose name appears in this list.
    /// When set, `deny_tools` is ignored.
    #[serde(default)]
    pub allow_tools: Option<Vec<String>>,
    /// Blacklist: hide tools whose name appears in this list.
    /// Ignored when `allow_tools` is set.
    #[serde(default)]
    pub deny_tools: Option<Vec<String>>,
}

impl ServerSpec {
    /// Stdio launch pair `(command, args)` when this is a stdio server.
    #[must_use]
    pub fn stdio(&self) -> Option<(&str, &[String])> {
        self.command.as_deref().map(|c| (c, self.args.as_slice()))
    }
}

/// Global user-wide config: `~/.context-pilot/mcp.json`.
fn global_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let path = PathBuf::from(home).join(".context-pilot").join("mcp.json");
    path.is_file().then_some(path)
}

/// Project-local config: `.context-pilot/shared/mcp.json`.
fn project_path() -> Option<PathBuf> {
    let path = PathBuf::from(PROJECT_CONFIG);
    path.is_file().then_some(path)
}

/// Parse a single config file into a [`Manifest`].
fn load_file(path: &std::path::Path) -> Result<Manifest, String> {
    let raw =
        std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_str(&raw).map_err(|e| format!("parse {}: {e}", path.display()))
}

/// Load and merge MCP config from both layers.
///
/// Global (`~/.context-pilot/mcp.json`) provides the base set of servers.
/// Project-local (`.context-pilot/shared/mcp.json`) overrides or extends it —
/// servers with the same key replace the global entry entirely.
///
/// Returns `Ok(empty)` when neither file exists.
///
/// # Errors
///
/// Returns a human-readable message on read or JSON-parse failure.
pub fn load() -> Result<Manifest, String> {
    let mut servers = HashMap::new();

    // Layer 1: global (base)
    if let Some(path) = global_path() {
        servers.extend(load_file(&path)?.servers);
    }

    // Layer 2: project-local (override)
    if let Some(path) = project_path() {
        servers.extend(load_file(&path)?.servers);
    }

    Ok(Manifest { servers })
}

/// Config sources that are currently present on disk — for diagnostics.
#[must_use]
pub fn active_sources() -> Vec<PathBuf> {
    let mut sources = Vec::new();
    if let Some(p) = global_path() {
        sources.push(p);
    }
    if let Some(p) = project_path() {
        sources.push(p);
    }
    sources
}
