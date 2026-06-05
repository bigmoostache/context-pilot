# Headless Mode — Design Document

**Status:** Draft — Round 4 (implementation-ready)
**Branch:** `headless`  
**Author:** Salt (Lieutenant)  
**Date:** 2026-06-05

---

## 1. Problem Statement

Context Pilot currently requires an active terminal to run. If the terminal closes, the session dies. This creates several pain points:

1. **Fragility** — accidental terminal close kills a mid-conversation session
2. **Mobility** — can't start a task on one machine/terminal and continue on another
3. **Background work** — LLM can't continue executing tools (reverie, auto-continuation) while the user is away from the terminal
4. **Resource efficiency** — the TUI rendering consumes CPU even when nobody is watching

### Goal

Run Context Pilot as a **background daemon** that persists independently of any terminal. Users attach/detach at will, like tmux — but at the semantic IR level, not the character grid level.

---

## 2. Architecture Overview

```
┌─────────────────────────────────────────────┐
│              DAEMON (cpilot)                 │
│                                             │
│  ┌─────────┐  ┌──────────┐  ┌───────────┐  │
│  │   App   │  │  LLM     │  │  Tools    │  │
│  │  State  │  │ Streaming│  │ Execution │  │
│  └────┬────┘  └────┬─────┘  └─────┬─────┘  │
│       │            │              │         │
│       ▼            ▼              ▼         │
│  ┌─────────────────────────────────────┐    │
│  │         IR Frame Builder            │    │
│  │  (builds Frame every render tick)   │    │
│  └──────────────┬──────────────────────┘    │
│                 │                            │
│                 ▼                            │
│  ┌─────────────────────────────────────┐    │
│  │        Socket Server                │    │
│  │  (Unix socket, JSON-line protocol)  │    │
│  └──────────────┬──────────────────────┘    │
│                 │                            │
└─────────────────┼───────────────────────────┘
                  │  Unix Socket
┌─────────────────┼───────────────────────────┐
│                 ▼                            │
│  ┌─────────────────────────────────────┐    │
│  │        Socket Client                │    │
│  │  (receives IR, sends input events)  │    │
│  └──────────────┬──────────────────────┘    │
│                 │                            │
│                 ▼                            │
│  ┌─────────────────────────────────────┐    │
│  │      TUI Renderer (ratatui)         │    │
│  │  (existing IR → ratatui adapters)   │    │
│  └─────────────────────────────────────┘    │
│                                             │
│              CLIENT (cpilot --attach)       │
└─────────────────────────────────────────────┘
```

### Key Principle

The daemon builds IR frames. The client renders them. All existing `ui/ir/` adapters (`render_sidebar_from_ir`, `render_conversation_from_ir`, etc.) are reused unchanged in the client.

---

## 3. Execution Modes

### 3.1 Integrated Mode — REMOVED

There is no separate "integrated" mode. `cpilot` always runs the daemon/client architecture. See §3.2.

### 3.2 Standard Launch

```
cpilot
```

Does two things in sequence:
1. **Launches the daemon** — same startup logic as today, including the existing suicide-and-replace mechanism (if another daemon is running for this folder, it suicides and the new one takes over). This logic is **untouched**.
2. **Auto-attaches** — once the daemon is ready, runs the equivalent of `cpilot --attach` to connect to it.

From the user's perspective, behavior is identical to today. Under the hood, the daemon and TUI client are separate processes communicating over a Unix socket.

### 3.3 Attach Mode

```
cpilot --attach
```

Thin client that connects to an already-running daemon:
- Receives IR frames and renders them via ratatui
- Captures terminal input and forwards to daemon
- Detaches cleanly on Ctrl+Q (daemon keeps running)
- **Multiple attach clients can coexist** — each renders independently at its own terminal size
- Session ID is auto-detected from the current working directory (one session per project folder)

### 3.4 List Mode

```
cpilot --list
```

Lists all active daemon sessions:
```
  PATH                                STATUS
  /home/user/my-project               running (2h 15m)
  /home/user/other-project            running (45m)
```

### 3.5 Stop Mode

```
cpilot --stop
```

Gracefully stops the daemon for the current project directory.

---

## 4. Socket Protocol

### 4.1 Transport

**Unix domain socket** at a well-known path derived from the project directory:
```
~/.context-pilot/sessions/<path-hash>/daemon.sock
```

Where `<path-hash>` is a deterministic hash of the canonical project path. The full path is stored in a `project_path` file alongside the socket for `--list` to read.

**DECIDED:** Session ID = project folder path. One session per project folder, enforced by existing suicide-and-replace logic.

### 4.2 Message Format

JSON-line protocol (one JSON object per line, newline-delimited). Same pattern as `cp-console-server`.

**DECIDED:** JSON serialization. Human-readable, debuggable. Local Unix sockets have effectively unlimited bandwidth — no need for binary serialization in v1.

### 4.3 Client → Daemon Messages

```rust
enum ClientMessage {
    /// Terminal key/mouse/resize event
    Input {
        event: SerializableEvent,  // crossterm Event, serialized
    },
    /// Client is attaching — send full state snapshot
    Attach {
        cols: u16,
        rows: u16,
    },
    /// Client is detaching gracefully
    Detach,
    /// Ping (keepalive)
    Ping,
}
```

### 4.4 Daemon → Client Messages

```rust
enum DaemonMessage {
    /// Full IR frame update (sent on every render tick)
    FrameUpdate {
        frame: Frame,
    },
    /// Pong (keepalive response)
    Pong,
    /// Daemon is shutting down
    Shutdown,
}
```

Note: No separate `Snapshot` message. On attach, the daemon simply sends its next `FrameUpdate` — since we always send full frames, the first frame IS the snapshot.

---

## 5. Frame Transport Strategy

**DECIDED:** Full frame on every render tick (Option A). Simplicity first.

The daemon serializes the entire `Frame` and sends it to all connected clients on every dirty render tick. The Frame is terminal-size-independent (see §6), so one build serves all clients.

### Why This Works

- A full Frame is ~50-100KB JSON. At 28fps during streaming, that's ~1.5-3MB/s per client.
- Unix domain sockets handle 1+ GB/s easily. We're at <0.3% capacity.
- During idle periods, no dirty tick = no frame sent. Bandwidth drops to zero.
- Client is **stateless** — no sync bugs, no missed deltas, no recovery logic.

### Future Enhancement Paths (Not Implemented)

If profiling reveals a problem (unlikely for local sockets):
1. **Dirty-flag sending** — only send when frame actually changed (trivial to add)
2. **Section-level deltas** — track dirty per section (sidebar/conversation/panel), send only changed sections
3. **Binary serialization** — switch JSON → bincode/MessagePack for ~10x size reduction
4. **Compression** — zstd frame compression for large panels

---

## 6. IR Is the Right Boundary

### Why IR, Not Raw State?

A natural question: shouldn't the daemon just send the relevant parts of State, and let each client build its own visual representation?

The answer is that **IR already IS that** — it's a semantic data contract, not a pre-rendered pixel grid:

```
Daemon:  State → IR Frame    (semantic: "here's a table", "here's a progress bar at 73%")
Client:  IR Frame → ratatui  (physical: "at terminal column 42, draw these characters")
```

The IR Frame is **terminal-size-independent**. It contains *what* to display (blocks, spans, tables, entries), not *how* to position it. The ratatui adapters — which run entirely **client-side** — handle terminal width, text wrapping, column sizing, scroll viewport. Each attached client renders the same Frame at its own terminal size.

Sending raw State instead would be:
- **Larger** — full message history, panel text, JSON tool calls are more verbose than pre-formatted blocks
- **Duplicative** — each client would redo markdown parsing, table formatting, tool visualizer logic
- **Fragile** — State contains non-serializable parts (file handles, watchers, thread channels)

The Frame is the **rendering ViewModel** — the minimal, serializable, platform-agnostic representation of what needs to be displayed.

### What Happens When No Client Is Attached?

When no client is connected:
- The daemon **skips frame building entirely** — no render ticks, no IR construction
- LLM streaming, tools, spine, persistence all continue normally
- When a client attaches, the daemon resumes building frames (first frame acts as full snapshot)

---

## 7. Input Abstraction

### Current State

`lifecycle.rs` calls `crossterm::event::poll()` / `crossterm::event::read()` directly. These are terminal-specific.

### Proposed Change

Abstract input behind a trait or enum:

```rust
enum InputSource {
    /// Direct terminal (integrated mode — not used in headless, but keeps code paths clean)
    Terminal,
    /// Remote client(s) via socket (daemon mode)  
    Socket(SocketServer),
}
```

The daemon's event loop polls the socket server for client messages. Input events from any connected client are injected into the same event processing pipeline (actions → state mutations → frame rebuild).

### Multi-Client Input

When multiple clients are attached, inputs from ALL clients are processed. This is the same as having multiple keyboards — last input wins. No conflict resolution needed for v1.

### Terminal Size

Each client sends its terminal size on `Attach`. The daemon uses the **most recently attached** client's size for any size-dependent logic. Each client independently handles physical layout via its own ratatui adapters.

### Process Model — Spawn-and-Become

**DECIDED:** Option A — spawn-and-become.

When user types `cpilot`:

```
cpilot (original process)
  │
  ├─ spawns:  cpilot --daemon-internal  (background, detached, no terminal)
  │           └─ boots → socket server → accepts clients → runs forever
  │
  └─ becomes: cpilot --attach  (foreground TUI)
              └─ connects to daemon socket → renders frames → forwards input
```

`--daemon-internal` is a hidden flag (not in `--help`). The daemon gets fresh file descriptors and a clean process environment. The original process simply transitions into client mode once the daemon's socket appears.

### Early Socket — Boot Progress in TUI

**DECIDED:** The daemon starts its socket server as the **very first thing** (Phase 0), before any module loading or Meilisearch startup.

```
Phase 0: Create socket, accept connections       ← clients can connect HERE
Phase 1: Load config, persistence
Phase 2: Init modules
Phase 3: Start Meilisearch
Phase 4: Load module data
Phase 5: Module init (file watchers, etc.)
Phase 6: Ready — switch to normal frames
```

During phases 0-5, the daemon sends "boot progress" IR frames — a simple progress bar and status text, using the same `Frame` type. From the client's perspective, it's seamless: boot frames transition into normal frames without any protocol change.

This means `cpilot` feels exactly like today — ye see the boot sequence in the terminal. Under the hood, it's a daemon sending frames to a client.

---

## 8. Boot Sequence

### Standard Launch (`cpilot`)

Two-phase boot:

**Phase A — Daemon Launch:**
1. Compute session path from canonical project directory
2. Check if daemon already running (PID file + liveness check)
3. If running: existing suicide-and-replace logic fires (untouched)
4. Run current 6-phase boot (LLM init, module loading, persistence, etc.)
5. No terminal init (no crossterm enable_raw_mode, no alternate screen)
6. Progress logged to stderr / log file instead of rendered
7. Socket server starts after boot completes
8. PID file + project_path file written to session directory
9. Process daemonizes

**Phase B — Auto-Attach:**
Once daemon is ready, the parent process transitions into attach mode (same as `cpilot --attach`).

### Attach (`cpilot --attach`)

Minimal boot:
1. Derive session path from current working directory
2. Check daemon exists + PID is alive
3. Connect to Unix socket
4. Send `Attach { cols, rows }`
5. Init terminal (crossterm + ratatui)
6. Receive first `FrameUpdate` — render it
7. Enter event loop (forward input, render frames)

---

## 9. Session Management

### Session Directory

```
~/.context-pilot/sessions/
  <path-hash>/
    daemon.sock       # Unix socket
    daemon.pid        # PID file
    project_path      # Plain text: canonical project path (for --list)
    daemon.log        # Stdout/stderr redirect
```

`<path-hash>` is a short deterministic hash of the canonical project path (e.g., FNV-1a or SHA256 truncated). One session per project folder — enforced by the existing instance detection logic.

### Session Discovery

`cpilot --list` scans session directories, reads `project_path` files, checks PID liveness:
```
  PATH                                STATUS
  /home/user/my-project               running (2h 15m, 2 clients)
  /home/user/other-project            running (45m, 0 clients)
```

### Session Cleanup

Stale sessions (dead PID) are cleaned up automatically on `--list`, `--attach`, or `cpilot` launch.

---

## 10. Detach / Reattach Lifecycle

```
User runs cpilot:       cpilot
                        → daemon boots, socket ready
                        → auto-attaches (TUI appears as normal)

User works normally     (keystrokes flow through socket, frames stream back)

User detaches:          Ctrl+Q (or terminal closes)
                        → client sends Detach, disconnects
                        → daemon keeps running (LLM, tools, spine all alive)

User reattaches:        cpilot --attach
                        → client connects, gets first frame
                        → seamless resume

Another terminal:       cpilot --attach (in same project dir)
                        → second client connects simultaneously
                        → both see same session, both can send input
```

### What Happens During Detach?

While no client is attached:
- LLM streaming continues (if active)
- Tool execution continues
- Spine auto-continuation fires (if configured)
- State is persisted normally
- **No frames are built** — render loop is paused
- Existing guard rails (max_cost, max_auto_retries) remain the safety net

**DECIDED:** No special detached-mode guard rails. Existing spine guard rails are sufficient.

---

## 11. Guard Rails

**DECIDED:** No new guard rails for headless mode. The existing spine guard rails are sufficient:

| Guard Rail | Purpose |
|---|---|
| `max_cost` | Hard $ cap (stream/burst-based) |
| `max_auto_retries` | Max consecutive auto-continuations |
| `max_duration_secs` | Max autonomous duration |
| `max_messages` | Max conversation messages |
| `max_output_tokens` | Max total output tokens |

These apply identically whether a client is attached or not.

---

## 12. Signal Handling

**DECIDED:** Industry-standard daemon signal handling.

| Signal | Action | Rationale |
|---|---|---|
| **SIGTERM** | Graceful shutdown (save state, notify clients, cleanup socket/PID, exit) | Standard "please stop." What `kill`, systemd, Docker send. |
| **SIGINT** | Same as SIGTERM | Ctrl+C on the daemon's original terminal (if any) |
| **SIGHUP** | **Ignore** | Terminal hangup — irrelevant for a daemon. Prevents accidental kills. |
| **SIGPIPE** | Ignore | Disconnected client socket. Rust ignores by default. |

---

## 13. Keybindings

| Key | Action | Description |
|---|---|---|
| **Ctrl+Q** | **Quit** (unchanged) | Client sends `Quit` to daemon → daemon shuts down gracefully → client exits. Same behavior as today. |
| **Ctrl+Z** | **Detach** (new) | Client disconnects from daemon → daemon keeps running → client exits. Daemon continues LLM/tools/spine. |

In the current non-headless build, Ctrl+Z has no effect (or behaves as Quit since there's no daemon to detach from).

---

## 14. Relationship to Existing Infrastructure

### Console Server

The console server (`cp-console-server`) already runs as a daemon with a Unix socket. The headless daemon would be a separate process. They coexist — the daemon talks to the console server the same way the integrated mode does.

### Persistence

State persistence (`PersistenceWriter`) works identically. The daemon writes state to disk. If the daemon crashes and restarts, it loads from disk — same as today's integrated mode restart.

### Reload (`system_reload`)

In the current architecture, reload uses `exec()` to replace the process. In headless mode, reload should:
1. Save state
2. Send `Shutdown` to all attached clients
3. `exec()` to restart the daemon process
4. **Clients auto-reconnect** — brief "reconnecting..." display, then resume

**DECIDED:** Clients auto-reconnect after daemon reload. The client polls the socket path until the new daemon is ready, then reattaches.

### File Watchers / Meilisearch / Console Server

All run in the daemon process, same as integrated mode. No changes needed.

---

## 15. CLI Interface

```
cpilot                  # Launch daemon (with suicide-and-replace) + auto-attach
cpilot --attach         # Attach to running daemon for current project dir
cpilot --list           # List all running daemon sessions (paths + uptime)
cpilot --stop           # Gracefully stop daemon for current project dir
```

No session-id argument needed — the project directory is always derived from `pwd`.

---

## 16. What Changes in Existing Code

### Minimal Touch Points

| File/Area | Change | Scope |
|---|---|---|
| `main.rs` | CLI flag parsing, fork into daemon vs. client | Medium |
| `lifecycle.rs` | Input from socket instead of terminal, conditional frame sending | Medium |
| `ui/mod.rs` | Frame building gated on client presence | Small |
| `events.rs` | Accept events from socket (deserialized `ClientMessage::Input`) | Medium |

### New Code

| Component | Purpose | Est. Lines |
|---|---|---|
| `src/headless/server.rs` | Unix socket server, multi-client management, frame broadcasting | ~250 |
| `src/headless/client.rs` | Thin attach client (socket → IR → ratatui, terminal → socket) | ~300 |
| `src/headless/protocol.rs` | `ClientMessage` / `DaemonMessage` types, serialization | ~80 |
| `src/headless/session.rs` | Session directory, PID management, `--list`, stale cleanup | ~150 |
| `src/headless/mod.rs` | Module glue | ~30 |
| **Total new code** | | **~810 lines** |

### Zero Changes Required

- All module crates (`cp-mod-*`) — headless-agnostic
- `cp-render` — already serializable
- `cp-console-server` — independent daemon
- All tool implementations — headless-agnostic
- State/persistence — headless-agnostic
- LLM providers — headless-agnostic
- IR builders (`ui/ir/sidebar.rs`, `ui/ir/conversation.rs`, etc.) — daemon-side, unchanged
- IR adapters (`ui/ir/render_sidebar.rs`, etc.) — client-side, unchanged

---

## 17. Phase Plan

### Phase 1 — Serialization Foundation

Prerequisite for all headless work. The daemon serializes IR Frames; the client deserializes them.

- Add `Deserialize` to all `cp-render` types (`lib.rs`, `frame.rs`, `conversation.rs`, `overlay_ir.rs`)
- Handle `#[non_exhaustive]` enums (`Semantic`, `Block`, `Overlay`) with `#[serde(other)]` on a fallback variant
- Enable crossterm `serde` feature (`features = ["serde"]` in workspace `Cargo.toml`) for `Event` Serialize/Deserialize
- Verify round-trip: build Frame → serialize JSON → deserialize → fields intact

### Phase 2 — Protocol + Session Management

- Define `ClientMessage` / `DaemonMessage` types in `src/headless/protocol.rs`
- Session directory structure (path-hash, PID, project_path) in `src/headless/session.rs`
- Stale session cleanup and `list_sessions()`
- Module glue in `src/headless/mod.rs`

### Phase 3 — Daemon Socket Server

- Unix socket server with multi-client support in `src/headless/server.rs`
- Per-client reader thread → mpsc channel for `ClientMessage`
- Frame serialization + broadcast to all connected client writers
- Integrate into `lifecycle.rs`: poll server for client input, replace `terminal.draw()` with frame broadcast
- Skip frame building when `client_count() == 0`

### Phase 4 — Thin Client (`--attach`)

- `HeadlessClient` in `src/headless/client.rs`: socket connection + terminal
- Frame deserialization → client-side rendering (see §18 for adapter reuse details)
- Terminal input capture → `ClientMessage::Input` → socket
- Local scroll state management (client-owned, no round-trip for scroll)
- Ctrl+Z detach, Ctrl+Q quit

### Phase 5 — Daemon Boot + Standard Launch

- CLI arg parsing in `main.rs`: `--daemon-internal` (hidden), `--attach`, `--list`, `--stop`
- Daemon boot path: skip terminal init, socket server at Phase 0, `setsid()`, PID/project_path files, stderr → `daemon.log`
- Standard launch (`cpilot`): spawn `cpilot --daemon-internal`, wait for socket (geometric backoff), transition to attach mode
- Boot progress IR frames sent to early-connecting clients during boot phases 0–5

### Phase 6 — Keybindings + Signals

- `Action::Detach` variant + Ctrl+Z keybinding in `events.rs`
- Client: Ctrl+Z sends `Detach`, disconnects gracefully. Ctrl+Q sends `Quit`.
- Daemon: `Quit` message → graceful shutdown (save state, notify clients, cleanup)
- Signal handling: SIGTERM/SIGINT → graceful shutdown, SIGHUP → ignore (`signal-hook` crate, same as `cp-console-server`)

### Phase 7 — Polish + CLI Commands

- `--list`: scan session dirs, PID liveness, print table with paths + uptime + client count
- `--stop`: send shutdown via socket, wait for daemon exit, cleanup stale files
- Auto-reconnect on daemon reload: client detects `Shutdown` → "reconnecting…" display → poll socket with backoff → reattach (30s timeout)
- Error handling: daemon crash → client shows error + exits, socket errors → graceful degradation
- Multi-client: frame broadcast to all, input from any, terminal resize forwarding

---

## 18. Implementation Notes (Codebase Exploration)

Key findings from deep codebase analysis that inform the implementation:

### Serialization Gap

`cp-render` types derive `Serialize` but **not** `Deserialize`. The client needs to deserialize `Frame` — this is the first thing to fix. `cp-render/Cargo.toml` already depends on `serde` with `derive` feature, so adding `Deserialize` is mechanical.

### crossterm Event Serialization

The workspace dependency `crossterm = "0.29"` has no features enabled. Adding `features = ["serde"]` gives `Serialize`/`Deserialize` on `Event`, `KeyEvent`, `KeyCode`, `KeyModifiers`, etc. — required for the `ClientMessage::Input` wire type.

### `#[non_exhaustive]` Enums

Three enums use `#[non_exhaustive]`: `Semantic`, `Block`, `Overlay`. For `Deserialize` to work, each needs a catch-all variant with `#[serde(other)]`. This ensures forward compatibility — if the daemon sends a variant the client doesn't know, it deserializes to the fallback instead of failing.

### Client Rendering — Partial Adapter Reuse

| Adapter | Takes | Client Reuse |
|---|---|---|
| `render_sidebar_from_ir()` | IR `Sidebar` only | ✅ Direct reuse |
| `render_status_bar_from_ir()` | IR `StatusBar` only | ✅ Direct reuse |
| `render_panel_from_ir()` | `&mut State` (scroll) | ❌ Own renderer needed |
| `render_conversation_from_ir()` | `&mut State` (scroll + cache) | ❌ Own renderer needed |

The panel and conversation adapters depend on `State` for scroll management. The client has no `State` — it manages scroll locally. The client implements its own panel/conversation renderers (~100 lines each) that:
1. Convert Frame IR blocks to ratatui `Line`s via the existing `blocks_to_lines()` helper
2. Maintain local `scroll_offset` / `max_scroll`
3. Handle scroll keys locally (no round-trip to daemon)

### Conversation IR Data Completeness

The `Frame.conversation` field **is** fully populated by `build_frame()` — messages with content blocks, tool use/result previews, streaming tools, and input area. The current TUI renderer ignores this field and calls `build_content_cached(state)` directly (artifact of incremental IR migration, Phase 6+). The headless client renders from the IR data instead, which is the architecturally correct path.

### Existing Mechanisms That Work Unchanged

- **Suicide-and-replace**: `check_ownership()` PID-based ownership detection. New daemon writes PID → old daemon detects it's no longer owner → exits gracefully. Old daemon's clients see socket close → reconnect to new daemon.
- **Console server**: Independent daemon. Coexists with headless daemon unchanged.
- **Persistence**: `PersistenceWriter` works identically in daemon mode.
- **File watchers / Meilisearch**: Run in daemon process, same as integrated mode.

## 19. Decision Summary

All 12 open questions from Round 1 have been resolved:

| # | Question | Decision |
|---|---|---|
| 1 | `cpilot --list`? | **Yes** — lists active project paths |
| 2 | Bare `cpilot` behavior? | **Launch daemon + auto-attach** |
| 3 | Session ID format? | **Project folder path** (hashed for directory name) |
| 4 | Full frame vs delta? | **Full frame every tick** |
| 5 | JSON vs binary? | **JSON** |
| 6 | Transport strategy? | **Option A — full frame, simplest possible** |
| 7 | Frame building with no client? | **Skip entirely — resume on attach** |
| 8 | One session per project? | **Yes** — enforced by existing instance detection |
| 9 | Detached guard rails? | **No — existing spine guard rails sufficient** |
| 10 | Extra guard rails? | **No** |
| 11 | Auto-reconnect after reload? | **Yes — client polls + "reconnecting..." display** |
| 12 | Bare `cpilot` with running daemon? | **Suicide-and-replace (untouched) + auto-attach** |
| 13 | Process model? | **Spawn-and-become — `cpilot` spawns `--daemon-internal`, becomes client** |
| 14 | Boot progress display? | **Socket starts Phase 0, boot progress sent as IR frames** |
| 15 | Signal handling? | **SIGTERM/SIGINT→graceful shutdown, SIGHUP→ignore** |
| 16 | Suicide reconnect? | **Yes — old clients auto-reconnect to new daemon** |
| — | Quit vs. Detach? | **Ctrl+Q = quit (daemon+client), Ctrl+Z = detach (client only)** |

---

## 20. Open Questions (Round 4)

No open questions remain. All design decisions have been made. Ready for implementation.

---

## Decision Log

### Round 1 (2026-06-05)
- Initial design drafted
- Architecture: daemon/client split over Unix socket with IR frame streaming
- Key insight: existing IR layer + console server pattern make this naturally feasible
- Estimated ~780 lines of new code, ~5-8 days implementation
- 12 open questions flagged for discussion

### Round 2 (2026-06-05)
- All 12 open questions resolved
- Integrated mode removed — `cpilot` always runs daemon + auto-attach
- Session ID = project folder path (one session per project)
- Full frames, JSON, Option A (maximum simplicity)
- Clarified IR responsibility: IR is the semantic ViewModel, adapters handle physical rendering
- No frames built when no client attached — render loop pauses
- No special detached guard rails — existing spine guard rails suffice
- Clients auto-reconnect after daemon reload
- Multi-client support confirmed (multiple `--attach` coexist)
- 4 new questions raised (process model, boot UX, signals, suicide reconnect)

### Round 3 (2026-06-05)
- All 4 Round 2 questions resolved
- Process model: spawn-and-become (Option A) — `cpilot` spawns `--daemon-internal`, becomes client
- Boot UX: socket starts at Phase 0, boot progress sent as IR frames — seamless transition
- Signal handling: SIGTERM/SIGINT→graceful shutdown, SIGHUP→ignore (industry standard)
- Suicide reconnect: old daemon's clients auto-reconnect to new daemon
- **Ctrl+Q** = Quit (kills daemon + client, unchanged)
- **Ctrl+Z** = Detach (disconnect only, daemon lives on) — new keybinding
- **All questions resolved — design complete, ready for implementation**

### Round 4 (2026-06-05)
- Deep codebase exploration completed — all touch points analyzed
- Phase Plan rewritten: 6 phases → 7 phases (new Phase 1: Serialization Foundation)
- Added §18 "Implementation Notes" with concrete findings:
  - cp-render needs Deserialize (only has Serialize)
  - crossterm needs `serde` feature for Event serialization
  - `#[non_exhaustive]` enums need `#[serde(other)]` fallback variants
  - Client can reuse sidebar/status_bar adapters but needs own panel/conversation renderers (State dependency)
  - Conversation IR data is fully populated in Frame — client renders from it directly
- Fixed section numbering (duplicate §11, §13)
- **Design document complete — implementation starting**
