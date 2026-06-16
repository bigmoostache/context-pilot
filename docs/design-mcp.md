# Design — Generic MCP Client Module (`cp-mod-mcp`)

Status: **DRAFT — pending sign-off**
Branch: `mcp-client`

## 1. Goal

A generic [Model Context Protocol](https://modelcontextprotocol.io) client module that connects to
**any** MCP server, discovers its tools at runtime, and exposes them to the LLM as native Context
Pilot tools. Notion's hosted MCP (`mcp.notion.com`) is the first real remote target; a local `npx`
server (e.g. filesystem) is the first stdio target.

Non-goals (for v1): being an MCP *server*, sampling/roots capabilities, full resources/prompts UI.

## 2. MCP in one breath

MCP is **JSON-RPC 2.0** over a transport. Minimal client flow:

1. `initialize` — capability handshake; server returns its info + capabilities.
2. `notifications/initialized` — client confirms.
3. `tools/list` — `[{ name, description, inputSchema (JSON Schema) }]`.
4. `tools/call { name, arguments }` → `{ content: [...], isError }`.
5. Notifications (`tools/list_changed`) trigger re-list.

Transports:
- **stdio** — spawn a subprocess, exchange newline-delimited JSON-RPC over stdin/stdout. Most common.
- **Streamable HTTP** — single endpoint, POST requests, responses optionally streamed via SSE. This is
  what Notion hosts. Requires **OAuth 2.1 + PKCE** for Notion's hosted server.

## 3. The load-bearing question — dynamic tools ⭐

CP's built-in tools are **static** (declared in `yamls/tools/`, validated at compile time in
`cp-base/src/lib.rs` tests). MCP tools are **dynamic** (discovered at runtime per server).

**Hypothesis:** `modules/mod.rs::rebuild_tools()` already rebuilds the toolset at runtime (that's how
`module_toggle` adds/removes tools). So the toolset is *not* frozen at compile time. The MCP module's
`tool_definitions()` would return runtime-discovered tools, cached after connection.

**Risk to validate in Phase 0 before any real code:** confirm that (a) `tool_definitions()` may return
runtime data (not pure/static), and (b) nothing in the pre-flight / schema-validation pipeline rejects
a tool whose schema wasn't known at compile time. If this fails, the whole architecture changes.

MCP `inputSchema` is JSON Schema, which maps ~1:1 to the Anthropic `input_schema` CP already sends —
conversion is a thin wrapper.

## 4. Concurrency model

Reuse the existing async-result pattern (`cp-base/src/state/watchers.rs`): `ChannelWatcher` polls a
`Mutex<Receiver<WatcherResult>>` — exactly how console/gh async results already flow back.

- **stdio server:** one long-lived thread per server. Reads newline-delimited JSON-RPC from stdout,
  matches responses to requests via a `pending: HashMap<RpcId, Sender>`. Writes requests to stdin.
- **HTTP server:** blocking `reqwest` in a dedicated thread (consistent with brave/firecrawl), SSE read
  via chunked response.

Tool-call lifecycle: LLM calls `notion__search` → module sends JSON-RPC, returns *pending* + registers
a `ChannelWatcher` → connection thread resolves it → watcher delivers the tool result.

## 5. Crate structure (respects ≤8 entries/folder, ≤500 lines/file)

```
crates/cp-mod-mcp/src/
├── lib.rs          # Module trait impl; server registry; lifecycle (connect on load)
├── protocol.rs     # JSON-RPC + MCP wire types (initialize, tools/list, tools/call)
├── client.rs       # Per-server client: handshake, list, call, response matching
├── transport/
│   ├── mod.rs      # Transport trait
│   ├── stdio.rs    # Subprocess transport
│   └── http.rs     # Streamable HTTP + SSE transport
├── oauth.rs        # OAuth 2.1 + PKCE (Notion remote)
├── config.rs       # .mcp.json parsing
├── tools.rs        # MCP tool <-> CP tool bridge; namespacing + dispatch routing
└── panel.rs        # Server status panel (connected servers, tool counts, errors)
```

## 6. Config & secrets

Server list — standard de-facto format, per-project, shareable:

```json
// .context-pilot/shared/mcp.json   (global fallback: ~/.context-pilot/mcp.json)
{
  "mcpServers": {
    "notion":     { "url": "https://mcp.notion.com/mcp" },
    "filesystem": { "command": "npx", "args": ["-y", "@modelcontextprotocol/server-filesystem", "."] }
  }
}
```

OAuth tokens / bearer secrets go in **global** config (like API keys via `config/global.rs`), **never**
committed. Tool namespacing: CP tool name = `{server}__{tool}`; dispatch splits on `__` to route.

## 7. Phasing

| Phase | Deliverable | Risk |
|-------|-------------|------|
| **0** | Validate the dynamic-tools hypothesis (§3) in code. Sign-off on this doc. | low |
| **1** | `protocol.rs` + stdio transport + `client.rs`. Handshake + `tools/list` + `tools/call` against an `npx` server. No auth. | med |
| **2** | Dynamic tool registration: bridge into `rebuild_tools()`, dispatch, config loading, status panel. | **high ⭐** |
| **3** | Streamable HTTP transport with a static bearer token. | med |
| **4** | OAuth 2.1 + PKCE for Notion hosted (browser auth, localhost callback, token store + refresh). | **high** |
| **5** | Polish: reconnection, `tools/list_changed`, resources/prompts, error UX. | low |

## 8. Open questions

- **§3 dynamic tools** — the gating risk; validate first.
- **Reconnection on reload** — re-establish servers on `system_reload` (cf. search module re-binding
  its port, M33). Confirm lifecycle hook.
- **OAuth callback** — spin a transient localhost HTTP listener to capture the auth code; port choice +
  cleanup.
- **Tool count explosion** — Notion exposes many tools; consider an allow/deny filter per server in
  config to keep the LLM tool list lean.
- **Error surfacing** — MCP `isError` results vs transport failures vs server crashes; how each appears
  in the panel and the tool result.
