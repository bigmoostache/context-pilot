# Context Pilot

**An AI coding agent that lives in your terminal — and a fleet orchestrator and web cockpit to run many of them at once.**

Context Pilot is a ~47,000-line Rust project (18 module crates + a main binary + an orchestration backend) plus a React web client. It started as a single self-hosting TUI in which an AI agent develops the very tool it runs inside, and grew an orchestration layer so that a *fleet* of such agents can be discovered, observed, commanded, and supervised — from the terminal or from a browser.

This document describes how the whole thing fits together: the agent's main loop, the module system, the three-tier durability model, the orchestrator, the bridge that joins an agent to a fleet, the web frontend, and the sidecar services (console server, Meilisearch, SQLite entities).

---

## 1. System overview

Context Pilot is not one process. It is a small constellation of cooperating surfaces:

```
                         ┌───────────────────────────────────────────┐
                         │              Web frontend (web/)           │
                         │  React 19 · TanStack Query · Vite · SSE    │
                         └───────────────┬───────────────────────────┘
                                         │  REST + Server-Sent Events
                                         │  (HTTP :7878)
                         ┌───────────────▼───────────────────────────┐
                         │        Orchestrator  (cp-orchestrator)     │
                         │  registry · materialized view · breaker ·  │
                         │  stream hub · supervisor · REST/SSE        │
                         └───┬───────────────┬───────────────┬────────┘
            oplog tail       │   commands     │   stream tap  │   spawn / signals
            (inotify)        │  (Unix socket) │  (Unix socket)│   (pty)
                         ┌───▼───────────────▼───────────────▼────────┐
                         │            Agent  (the TUI, src/)          │
                         │  event loop · modules · LLM streaming ·    │
                         │  bridge (CP_BRIDGE=1) · tier-② state       │
                         └───┬───────────────┬───────────────┬────────┘
                             │               │               │
                     ┌───────▼──────┐ ┌──────▼──────┐ ┌──────▼───────┐
                     │ Console server│ │ Meilisearch │ │ SQLite        │
                     │ (long-running │ │ (full-text  │ │ entities      │
                     │  processes)   │ │  search)    │ │ (structured)  │
                     └───────────────┘ └─────────────┘ └───────────────┘
```

**One agent = one folder.** Every agent owns a *realm* (a working directory) and stores all its state under `<folder>/.context-pilot/`. Switching agents is switching folders. The orchestrator manages many such realms; the web cockpit renders them.

| Surface | Crate / dir | Role |
|---|---|---|
| **Agent (TUI)** | `src/` (`tui` binary) | The interactive coding agent: event loop, tools, LLM streaming, panels, persistence. |
| **Orchestrator** | `crates/cp-orchestrator` | Fleet control plane: discover, observe, command, supervise; serves REST + SSE on `:7878`. |
| **Web frontend** | `web/` | Browser cockpit: fleet dashboard, per-agent threads / panels / file manager. |
| **Console server** | `crates/cp-console-server` | Per-realm daemon running child processes that survive TUI restarts. |
| **Meilisearch** | external process | Full-text index of project files + logs. |

---

## 2. The agent (TUI)

The agent is a blocking, single-threaded **Elm/Redux-style** application built on Ratatui + Crossterm. No async runtime — everything is driven by one event loop.

### Main loop

`src/app/run/lifecycle.rs` is the heart. Each iteration is **input-first** (a non-blocking poll), then it advances background work: stream events, the tool pipeline, cache updates, file/GitHub watchers, the spine (notifications), and reverie (the context optimizer). The loop runs on an **adaptive cadence** — ~8 ms while streaming or when panels are dirty, ~2 ms when an orchestrator is connected (so a web command applies within a tick), ~50 ms when idle — and renders at ~28 fps.

State changes flow through a central **action dispatcher** (`src/app/actions/`), Redux-style: an action mutates `State`, the next render reflects it. Key pipelines:

- **Streaming** (`src/app/run/streaming.rs`) — LLM chunks drive a typewriter buffer; `ToolUse` blocks queue pending tool calls; `Done` finalizes the message and reconciles token/cost accounting. API errors retry with backoff.
- **Tool pipeline** (`src/app/run/tools/pipeline.rs`) — pre-flight validation → queue interception → execution → file-edit callbacks → tempo break → sentinel deferral. Each tool is its own flame-graph span.
- **Prompt assembly** (`src/app/prompt_builder.rs`) — rebuilds the LLM prompt in three phases: context panels injected as synthetic `tool_use`/`tool_result` pairs, then the conversation history, then strict role alternation.

### Main-loop watchdog

Because the loop is **single-threaded**, every step runs inline on the one thread — so any synchronous block (a tool hitting a hung dependency, a slow panel rehash, a lock wait, a stalled socket read) freezes the *entire* UI for its full duration, historically with no trace of *which* step wedged. The watchdog (`src/app/run/tools/watchdog.rs`) ends that blind spot. It is the interactive sibling of the headless deadman, but **purely observational**: it never terminates, re-execs, or signals the process (a human is at the keyboard) — it only writes a diagnostic so a freeze that "had no apparent reason" becomes "the log says it wedged in `<step>` for `<N>s`."

Two cooperating detectors run off cheap atomic markers the loop updates each pass:

- **Heartbeat** — *is the loop alive?* The loop stamps a timestamp at the top of every iteration; since it ticks at least every ~50 ms even when idle, a timestamp gone **>15 s stale** means a genuine wedge.
- **Activity marker** — *which step wedged?* Before each phase (input, bridge, stream drain, cache, watchers, tool execution, panel refresh, spine, reverie, render, save) the loop sets a one-byte marker; a single step in flight **>12 s** is named as the culprit.

A detached monitor thread polls every 2 s and, on a trip, dumps `.context-pilot/errors/watchdog-<timestamp>.log` with the wedged step + duration, the process **CPU%** (high ≈ a busy-loop, low ≈ a blocked syscall or deadlock), and per-thread states (Linux `/proc/self/task` wait-channels, or a macOS `sample` backtrace). The heartbeat/marker writes are single relaxed atomic stores (a few nanoseconds), so the happy path is untouched and the monitor merely sleeps until something actually freezes.

### Modules — the plugin system

Functionality is a set of **modules** implementing a common `Module` trait (defined in `cp-base/src/modules.rs`): each can hold state, declare and execute tools, create panels, and hook into lifecycle events (`on_user_message`, `on_stream_chunk`, `on_tool_complete`, …). Twenty-one modules are registered via `all_modules()` (`src/modules/mod.rs`), one crate each:

`core`/overview · `threads` · `memory` · `todo` · `queue` · `spine` · `scratchpad` · `callback` · `console` · `git` · `github` · `search` · `entities` · `tree` · `files` · `logs` · `brave` · `firecrawl` · `ocr` · `prompt` · `questions` · `bridge`.

Modules are either **global** (shared across all workers in a realm — files, memory, tree, logs, entities, callbacks, …) or **per-worker** (queue, git, spine, scratchpad, todo, console, github, search). A single realm can run several **workers** (independent agent loops sharing model + theme); the orchestrated world typically runs one worker per realm and scales out by running many realms.

### Panels & context

Everything the agent "sees" is a **panel** (`cp-base/src/panels.rs`): files, tool results, memories, the directory tree, search results, etc. Panels are content-hashed (FNV-1a), cacheable, freezable, and paginated. The set of open panels *is* the working context; the agent manages its own context budget by opening and closing them.

### LLM providers

`cp-base/src/config/models.rs` defines seven provider backends — Anthropic, Grok, Groq, DeepSeek, MiniMax, Claude Code (OAuth and API-key variants) — each with its model roster, pricing, context windows, and wire API names. The provider/model is switchable at runtime (and persisted) via the config overlay.

### Persistence (tier-② state)

All agent state lives under `<folder>/.context-pilot/`:

- `config.json` — global module data (the full thread list + message logs, memory, tree descriptions, logs), theme, view mode.
- `states/<worker>.json` — per-worker module data (todo, queue, scratchpad, spine, git, console, search, …) + cost counters.
- `shared/` — `memories.yaml`, `callbacks.yaml`, `tree-descriptions.yaml`, `entities/entities.db`.
- `messages/*.yaml`, `panels/*.json` — conversation messages and panel snapshots.

These files are the **tier-②** cache (see §3): durable but disposable, never the live read path.

---

## 3. Durability & the two planes

When an agent joins a fleet, its observable state rides a **three-tier durability model** (design doc `docs/design-orchestration-backend.md`):

- **Tier ① — the oplog** (`crates/cp-oplog`): an append-only write-ahead log, one per agent. A single writer appends a **rev-numbered delta** for every user-visible mutation (thread created, message finalized, phase change, cost update, lifecycle). Writes are framed (length + CRC32C) and committed off the main loop by a group-commit thread (one `fdatasync` per batch, so the hot loop never blocks on durability). Periodic **checkpoints** carry a snapshot of heads + dedup set + thread roster, so a restart rebuilds bounded state without replaying all of history; compaction then drops superseded segments.
- **Tier ② — the state cache** (`config.json` / `states` / `messages`): a lazily-rebuildable, disposable mirror. After alignment it is read only for cold-start hydration and for inspection panels that have no delta — never for live state.
- **Tier ③ — the stream plane** (a Unix-domain `tee.sock`): lossy and ephemeral. Carries live tokens and phase hints for sub-millisecond "typing" feedback. Dropped frames are fine — the oplog is the authority that self-heals them.

These tiers feed **two planes** that the rest of the system is built around:

- **The push plane (live).** `oplog → backend Tailer (one inotify watch per agent) → in-memory MaterializedView → rev-numbered SSE deltas → the frontend applies the delta in place.` This is the fast path: command-to-visible is ~14 ms median. No polling, no disk re-read.
- **The inspection plane (read-only).** For state that has no oplog delta (memory cards, todos, the file tree, the tools catalog), the backend reads the agent's tier-② files on demand and reshapes them to JSON, with mtime memoization. Lower-churn, pull-based, explicitly second-class.

A single resource is owned by exactly one plane, so the two never fight over freshness.

---

## 4. The orchestrator (backend)

`crates/cp-orchestrator` is a standalone binary that controls a fleet. It uses **no async runtime**: a blocking, thread-per-connection HTTP server (`tiny_http`) mirrors the rest of the codebase.

- **Registry** (`registry/`) — `AgentRegistry` poll-scans the agents directory (`~/.context-pilot/agents/<id>.json`) and diffs it into events (Appeared / Disappeared / StatusChanged / Stale). Liveness is a three-factor verdict (registry record + heartbeat freshness + lock ownership). `Tailer` is the incremental, gap-free oplog consumer; `AgentChannel` hydrates and sends; `TeeReader` taps an agent's stream plane.
- **Services** (`services/`) — `MaterializedView` is the in-memory fleet projection: one `AgentView` per agent (rev, heads, thread roster, focused thread, phase, lifecycle, cost), folded purely from `OpEntry`. `CostBreaker` is a durable per-agent spend breaker (high-water mark, fail-closed, survives crash-loops). `StreamHub` fans an agent's stream out to N bounded subscribers. `RetiredStore` records stopped-but-kept agents.
- **Supervisor** (`supervisor/`) — `AgentSupervisor` owns process lifecycle: spawn an agent on a real **pty** (the TUI needs a tty), stop (SIGTERM → grace → SIGKILL → reap), restart, and adopt externally-launched agents. Spawns are gated by a binary allow-list.
- **Transport** (`transport/`) — REST + SSE. The driver loop runs two cadences: a slow scan (~2 s: registry diff + a `config.json` mtime backstop) and a fast tail (~100 ms: poll each Tailer → fold into the view → observe cost). Notable routes:
  - `GET /api/fleet/meta`, `/api/fleet/retired`, `/api/metrics`
  - `GET /api/agent/{id}/{meta,threads,panels,memory,todos,tree,callbacks,tools,radar,entities,conversation,metrics,vitals}`
  - `GET /api/agent/{id}/fs[...]` — the realm file manager (`fs`, `fs/preview`, `fs/download`)
  - `POST /api/agent/{id}/{command,fs/upload,fs/move,restart,retire,unretire}`
  - `POST /api/fleet/create` — spawn a new agent
  - `GET /api/stream?agent={id}&ticket={t}` — the SSE channel
  - SSE uses single-use **tickets** for auth, emits rev-numbered `OpEntry` deltas plus stream-hint frames, supports `Last-Event-ID` replay-by-rev, and is woken sub-millisecond by an inotify watch on the oplog directory.

---

## 5. The bridge (how an agent joins a fleet)

The agent ↔ orchestrator coupling lives in `crates/cp-mod-bridge`, an **additive, gated** module: with `CP_BRIDGE=1` it activates at boot; otherwise it is behaviorally inert. On boot it:

1. takes an exclusive lock on the realm (`bridge.lock`),
2. spawns the oplog writer service,
3. binds the command-intake Unix socket (`stream.sock`),
4. mints its identity — `folder_id` (an FNV-1a hash of the canonical path, the stable naming key), a `boot_id`, and a `cap_token` bearer secret,
5. writes its registry record (`<id>.json`),
6. starts a heartbeat beacon (a fixed 60-byte liveness record refreshed every second),
7. emits `Lifecycle::Running`.

Thereafter it **emits a durable oplog delta** for every user-visible mutation (so the view stays current), **accepts commands** over the socket with authentication + journal-then-ack + dedup (a command is durably journaled *before* it is acknowledged, exactly-once), and **tees** live tokens onto the stream plane.

---

## 6. The web frontend

`web/` is a Vite + React 19 + TypeScript + Tailwind v4 (shadcn) app. State management is **TanStack Query v5**, but freshness is owned by the push plane, not by polling:

- `lib/queryClient.ts` — a single `QueryClient` with `staleTime: Infinity` and refetch-on-focus/mount/reconnect disabled. The only time-based refresh is a slow (15 s) last-resort backstop.
- `lib/sse.ts` — a reconnecting `EventSource` (one singleton per agent), ticket-authenticated, resuming by `last_rev`.
- `lib/sync.ts` — the SSE→cache bridge: each rev-numbered delta is *folded* into the cache with a functional `setQueryData` updater (`applyThreadDelta` / `applyAgentDelta`), guarded by a monotonic rev high-water mark. No refetch on a delta.
- `lib/live.ts` / `lib/api.ts` — the typed hooks (`useFleet`, `useThreads`, `useAgentMeta`, `usePanels`, `useFs`, …) and the REST client.
- `lib/markdown.tsx` — a themed GFM renderer for conversation messages.

Four top-level views: **fleet** (the mission-control dashboard — the only place agents are created/managed), **threads** (the thread-centered conversation surface), **cockpit** (a panel-centered view mirroring the TUI's panels), and **finder** (a per-realm macOS-style file manager with live preview, upload, and internal drag-and-drop). Live assistant tokens are painted with a `requestAnimationFrame`-batched buffer so streaming never thrashes React.

---

## 7. Sidecar services

### Console server (`crates/cp-console-server`)

A standalone daemon, one per realm, reachable at `<folder>/.context-pilot/console/server.sock` (with a `server.pid`). It speaks a JSON-line protocol over a Unix socket (`create` / `send` / `kill` / `status` / `list`) and manages **child processes that outlive TUI restarts** — builds, dev servers, interactive bash. Thread-per-connection, signal-driven graceful shutdown. The `console` module talks to it; the agent never blocks on a long-running command.

### Meilisearch (full-text search)

An external Meilisearch process (its port discovered from `~/.context-pilot/meilisearch/port`) indexes the project. The `search` module chunks source files via tree-sitter (semantic units — functions, structs, classes — with a character fallback) and indexes logs with tags + importance. It powers the `search` tool and the recency-weighted **Context Radar**.

### Entities (structured knowledge)

The `entities` module gives each agent a private SQLite database (`shared/entities/entities.db`) with the full power of SQLite — JOINs, CTEs, window functions, triggers, views — exposed through one `entity_sql` tool, for structured domain data with relationships.

---

## 8. The wire protocol (`cp-wire`)

`cp-wire` is the I/O-free, transport-agnostic contract shared by the agent, the oplog, and the orchestrator. It carries `PROTOCOL_VERSION` with N-1 compatibility and tolerant decoding (unknown variants degrade gracefully). Core types:

- **`Command`** — `SendMessage`, `CreateThread`, `ArchiveThread`, `RestoreThread`, `InterruptStream`, `Stop`, `Configure`.
- **`OpEntry` / `OpEntryKind`** — the oplog deltas: `CommandEffect`, `MessageCreated`, `ThreadCreated` / `Archived` / `Restored` / `StatusChanged`, `ThreadFocusChanged`, `PhaseTransition`, `CostAggregate`, `Lifecycle`, `Checkpoint`.
- **`StreamFrame`** — ephemeral hints: `MessageStartHint`, `Token`, `ToolArgs`, `PhaseHint`.
- **`Heartbeat`** — the fixed 60-byte liveness record.
- **`Snapshot` / `RosterThread`**, **`Entry`** (registry record), **`ContentHash`** (SHA-256, for content-addressed message bodies).

---

## 9. Repository layout

```
src/                     The agent (TUI) — the `tui` binary
  app/                   Event loop, actions, streaming, tool pipeline, prompt builder
  modules/               Built-in panel modules + the module registry
  state/                 Tier-② persistence
  llms/                  LLM provider implementations
  ui/                    Ratatui rendering

crates/
  cp-base/               Foundation: Module + Panel traits, config, themes, casts
  cp-wire/               Shared wire protocol (I/O-free)
  cp-oplog/              Tier-① write-ahead log (append, replay, compaction, checkpoints)
  cp-mod-bridge/         Agent-side orchestration bridge (oplog, intake, tee, heartbeat)
  cp-orchestrator/       The fleet backend (registry, services, supervisor, transport)
  cp-console-server/     Long-running-process daemon
  cp-mod-*/              The 20+ feature modules (threads, memory, search, git, …)
  cp-render/             IR-based rendering primitives

web/                     React web cockpit (Vite + TanStack Query + SSE)
docs/                    Design docs (notably design-orchestration-backend.md)
```

---

## 10. Engineering constraints

The codebase is maintained under unusually strict static-analysis discipline, and those constraints shape the architecture as much as any design decision:

- **Structure caps** — every Rust file ≤ 500 lines, every directory ≤ 8 entries, enforced by CI. Growth forces decomposition rather than accretion (this is why so many modules are split into `mod.rs` + siblings).
- **Lints** — ~961 active clippy/rustc lints (the vast majority at `forbid`). `#[allow]` is banned; only a handful of individually-justified `#[expect]` annotations remain.
- **A cryptographic hash chain** guards the lint config, CI scripts, and exception registry — the agent that develops this project can write code and fix lints, but cannot lower the bar.

### Flame-graph telemetry

The agent ships built-in flame-graph instrumentation (~60 spans, zero cost when disabled). Run with `./run.sh --telemetry`, use the app normally (spans persist across reloads), then render:

```bash
cargo install inferno   # one-time
inferno-flamegraph --title "Context Pilot" \
  < .context-pilot/logs/flame-folded.txt > flame.svg
```

Self-time accounting (total minus children) keeps nested spans from double-counting.

---

<p align="center">
  <i>One agent per folder. A fleet in a browser. Built by an AI, inside itself.</i>
</p>
