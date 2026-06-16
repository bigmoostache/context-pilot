# Orchestration Backend — Design Doc (WIP)

> **Status:** discussion / brainstorm. **Nothing here is implemented.** This is
> the living artifact we iterate on until we're perfectly aligned on the
> infrastructure that powers the orchestration frontend (the `ui/` maquette).
>
> Branch context: the maquette lives in `ui/` (design-only). This doc designs
> the *real* backend + the minimal agent-side seam that would feed it.

---

## 1. Problem statement

The frontend orchestrates **many agents**. Each agent is a single Rust loop
running inside its own folder (its realm). An agent does **not** know about
other agents — so no agent can own the orchestration backend. We therefore need
a **standalone backend** sitting between the frontend and the fleet of agent
loops.

```
            ┌─────────────────────────┐
            │     React frontend      │   (ui/, the maquette)
            └────────────┬────────────┘
                         │  Frontend ↔ Backend  (§7)
            ┌────────────▼────────────┐
            │   Orchestrator backend  │   standalone, owns the fleet view
            └────────────┬────────────┘
                         │  Backend ↔ Agents  (§6)
        ┌────────────────┼────────────────┐
        ▼                ▼                ▼
   ┌─────────┐      ┌─────────┐      ┌─────────┐
   │ agent A │      │ agent B │      │ agent C │   one Rust loop / folder
   │ folder  │      │ folder  │      │ folder  │
   └─────────┘      └─────────┘      └─────────┘
```

---

## 2. Constraints & principles (from the captain)

1. **1 agent = 1 Rust loop = 1 folder.** Agents are realm-confined.
2. **No agent owns the backend** — agents are unaware of each other.
3. **Backend is standalone.**
4. Design frontend ↔ backend comms.
5. Design backend ↔ agent comms.
6. Backend must **discover** which agents exist.
7. **CRITICAL — minimal impact on agent Rust code.** Small additive changes are
   fine (expose a socket/server, write a heartbeat file). But we must **not**
   change *how the agent behaves/reasons*. The agent must run identically
   whether or not the backend is watching.
8. Catalog what **information to gather from** agents and what **actions to
   perform on** them.

**Preferences:** simple, robust, leverage existing design choices. Assume
backend + agents share **one machine** for now — but put an **abstraction layer**
over (a) comms and (b) discovery, so either can change later. Per-agent
connection is managed internally (uniform today, pluggable tomorrow).

---

## 3. Existing design choices we can lean on

These already exist in the codebase and shape the cheapest, most robust path:

- **Everything is already persisted to disk**, continuously, by a debounced
  (50 ms) background persistence writer, under each agent's
  `.context-pilot/`:
  - `shared/` — `memories.yaml`, `tree-descriptions.yaml`, logs, `config.json`
  - `states/` — per-worker state JSON (multi-worker)
  - `panels/{uid}.json` — every panel
  - conversation messages as YAML files
  - `logs/` — perf/tool-times/errors; headless `trajectory.jsonl`
- **`cp-console-server`** — a precedent standalone daemon: Unix socket +
  JSON-line protocol, manages child processes that survive restarts. The exact
  pattern we'd reuse for a socket transport.
- **Headless mode** (`tui --headless`) — boots terminal-free, drives the loop to
  quiescence, with a deadman watchdog. The agent can already run as a
  background daemon with no UI.
- **Spine notifications** — the agent already models *"a user message"* as a
  notification (`NotificationType::UserMessage`) that drives the loop. Injecting
  an action from outside = injecting a notification. **This is the key seam.**
- **File watchers** (`notify`, inotify/kqueue) are already wired per-module — so
  watching an inbox directory costs the agent almost nothing.
- **Search "projects registry"** — there is already a notion of a global
  registry of known project paths (`~/.context-pilot/`).

**Consequence:** *reading* agent state needs **zero** agent changes (tail the
disk). The only genuinely new agent behavior is (a) announce itself for
discovery and (b) accept commands. Both are additive and behaviorally inert.

---

## 4. Recommended architecture

> **"Disk-as-truth (read) + Inbox-for-actions (write) + Global-registry
> (discover)", behind three swappable interfaces.**

```
React frontend
   │  REST (actions/queries) + SSE (live push)            ← §7
   ▼
Orchestrator backend (standalone)
   ├── AgentRegistry   (discovery)   → watch ~/.context-pilot/agents/*.json
   ├── AgentChannel[]  (per-agent transport)
   │       read:  watch <folder>/.context-pilot/   (NO agent change)
   │       write: drop JSON cmd into <folder>/.context-pilot/inbox/
   └── AgentSupervisor (lifecycle)   → spawn `cp --headless` in a folder
   │
   ▼  Backend ↔ Agent — file-based now, socket/net later (same protocol)  ← §6
Agent Rust loop (per folder)  +  tiny additive `cp-mod-bridge`:
   • boot:  write ~/.context-pilot/agents/<id>.json   (registry + heartbeat)
   • watch  .context-pilot/inbox/ → command files → spine notifications
   • (already) persists all state to .context-pilot/  → backend just reads it
```

**Why this fits the constraints**

- *Minimal impact (#7):* reads are pure disk-tailing → no agent change. The only
  new agent code is a heartbeat write + an inbox watcher that turns a command
  file into the **same spine notification a typed user message already
  produces**. The agent's reasoning/acting path is untouched. The bridge can be
  compiled out / disabled and the agent runs exactly as today.
- *Robust:* disk is the durable source of truth → the backend is effectively
  **rebuildable/stateless** (re-scan + reconnect after a crash). Agents are
  independent (one dying never touches another or the backend — it just goes
  stale). Reuses already-debounced persistence + battle-tested watchers.
- *Simple:* no new network stack on the agent side for v1; commands are JSON
  files; discovery is a directory of JSON files.
- *Leverages existing choices:* headless mode, spine-notification-as-input,
  cp-console-server's daemon pattern, the projects registry, continuous disk
  persistence.

---

## 5. The three abstraction seams (his explicit asks)

Transport-, discovery-, and lifecycle-agnostic interfaces. One local impl today;
swap freely later (remote machine, mDNS, k8s, …) without touching orchestration
logic. Sketch (language-agnostic):

```text
interface AgentRegistry {            // §8 discovery
    list() -> [AgentHandle]          // current known agents
    watch() -> stream<RegistryEvent> // appeared / disappeared / heartbeat
}

interface AgentChannel {             // per-agent transport (one connection)
    snapshot() -> AgentState         // full current state (read)
    subscribe() -> stream<AgentEvent>// live deltas (read)
    send(Command) -> Ack             // perform an action (write)
}

interface AgentSupervisor {          // lifecycle / process control
    spawn(folder, opts) -> AgentHandle
    stop(id, mode)                   // graceful | kill
    restart(id)
}
```

- **v1 impls:** `LocalRegistry` (watch `~/.context-pilot/agents/`),
  `LocalFsChannel` (read = watch `.context-pilot/`, write = `inbox/`),
  `LocalSupervisor` (`std::process` spawn of `cp --headless`).
- **The wire protocol is defined once, transport-agnostic** (versioned JSON
  `Command` / `Event` / `AgentState` schemas). Whether a `Command` travels as a
  file, a Unix-socket line, or a TCP/WS frame, its shape is identical — that's
  the layer that lets the medium change.

---

## 6. Backend ↔ Agent

### 6.1 Read path — disk tailing (zero agent change)

Backend watches `<folder>/.context-pilot/` and reads the same files the agent
already writes. Gives a full, durable, already-debounced view. Survives agent
crashes (last good state remains on disk). **Co-existence bonus:** a human can
have the TUI open on an agent while the backend observes it read-only.

### 6.2 Write path — command inbox (one tiny agent addition)

Backend writes a JSON command file into `<folder>/.context-pilot/inbox/`. The
agent's bridge module watches that dir, parses each command, converts it into a
spine notification / action **inside its own loop** (so it serializes with
everything else — no race with the persistence writer), then deletes the file.
Optionally writes an ack/result file the backend can watch.

> Why not poke state files directly? Because we'd race the agent's own writer
> (last-write-wins clobbering). Routing through the loop keeps a single writer.

### 6.3 Tradeoff captured: file vs socket

| | File-based (inbox/disk) | Unix socket (cp-console-server style) |
|---|---|---|
| Agent code | watcher + heartbeat only | listener thread + protocol |
| Latency | ~watcher debounce (≈50–100 ms) | ~1 ms, true req/resp |
| Robustness | durable, survives crash, replayable | needs live process; reconnect logic |
| Minimal-impact (#7) | **best** | good |

**Lean:** ship file-based v1 (lowest impact, most robust), keep `AgentChannel`
abstract so a socket impl is a drop-in upgrade if latency ever bites.

---

## 7. Frontend ↔ Backend

Backend exposes an HTTP API to the React app. Two sub-channels:

- **REST** for actions & point queries: `GET /agents`, `GET /agents/:id`,
  `GET /agents/:id/threads`, `POST /agents/:id/threads/:tid/messages`,
  `POST /agents` (spawn), `POST /agents/:id/stop`, …
- **Live push** for streaming updates (status changes, new messages, token
  deltas, MY_TURN signals): **SSE** (one-way server→client) or **WebSocket**
  (full-duplex).

**Lean:** **REST + SSE**. The frontend only needs server→client push for live
updates (REST already covers client→server actions); SSE is simpler & more
robust than full-duplex WS, and the project already understands SSE deeply from
the LLM streaming side. WS stays an option if we later want client→server
streaming (e.g. live typing indicators).

---

## 8. Discovery & heartbeat

- On boot, each agent (via the bridge) registers in a **global registry dir**:
  `~/.context-pilot/agents/<agent_id>.json` = `{ id, folder, pid, model,
  transport, started_at, last_heartbeat }`, refreshed on a heartbeat interval.
- The backend's `LocalRegistry` watches that dir → agents appearing/disappearing.
- **Liveness:** stale heartbeat *or* dead pid ⇒ agent considered down (backend
  can offer "restart"). No agent code needed to detect its own death — the
  backend infers it.
- Behind `AgentRegistry` so we can later swap to mDNS / a discovery service /
  remote inventory.

---

## 9. Agent-side delta (the entire footprint)

A single **optional, additive** module — call it `cp-mod-bridge` (or fold into
spine):

1. **Registry + heartbeat:** write/refresh `~/.context-pilot/agents/<id>.json`.
2. **Inbox watcher:** watch `.context-pilot/inbox/`, turn command files into
   spine notifications/actions, ack, delete.
3. *(optional, later)* **Live event channel:** low-latency push of deltas if
   disk-tailing latency proves insufficient.

Properties: `is_global`, additive, **behaviorally inert** (only adds an input
source + a status file). Disable it ⇒ the agent runs exactly as today. This is
the whole "impact on agent Rust code" — and it changes *inputs*, never
*behavior*.

---

## 10. Information to gather FROM agents

(Everything the cockpit/threads/usage views render — all already on disk.)

- **Identity & lifecycle:** id, name, folder/realm, model, pid, uptime, version.
- **Status / phase:** idle · streaming · tooling · blocked · needs-input ·
  errored (maps to the multi-worker WORKING / NEEDS-ATTENTION buckets).
- **Threads:** list (id, name, status `MY_TURN`/`ACTIVE`/`THEIR_TURN`, unread,
  last activity, preview); full conversation on demand; pending question forms.
- **Conversation:** user / assistant / tool messages; streaming deltas.
- **Economics:** tokens & cost per agent + per thread; cache hit/miss/output;
  context budget; cache stats.
- **Context panels:** todos, memories, logs, entities, spine notifications,
  queue, scratchpad, tools, callbacks, tree, radar.
- **Fleet-level signals:** which agents are `MY_TURN` (need the human), total
  spend, counts.

## 11. Actions to perform ON agents

- **Send a message to a thread** (the primary driver).
- **Threads:** create, archive/restore, answer a thread question.
- **Lifecycle:** spawn a new agent (process in a folder), stop / restart / pause,
  stop streaming (`user_stopped`), delete.
- **Manage:** rename, switch model, archive.
- **Settings/toggles:** auto-continuation, reverie, think reminders (API keys are
  org-managed — out of scope for per-agent action).
- **Scheduling:** thread-scoped coucou reminders.
- *(multi-worker, if exposed):* switch/create/delete worker.

---

## 12. Lifecycle & supervision

The backend is also a **process supervisor**: since the frontend is now the UI,
agents most naturally run **headless** (`cp --headless`) under backend control.
`AgentSupervisor.spawn(folder)` launches one; the deadman/lifecycle watchdog
already guards against wedged loops. Crash handling: stale heartbeat ⇒ surface
"down" + offer restart. (Agents may *also* be launched by hand with a TUI; the
disk-read path means the backend still observes them — see §6.1 co-existence.)

---

## 13. Identity model (open)

- **Stable id:** derive `agent_id` from the canonical folder path (e.g. FNV-1a
  hash, the scheme search already uses) so it's stable across restarts and lets
  the backend reconnect to "the same agent." (vs a random per-process id.)
- **Worker mapping:** an agent (folder) may run N internal workers (subworkers
  design). The frontend speaks in **threads**, not workers — we need to decide
  how workers map to the frontend's view (collapse to one? expose as sub-lanes?).

---

## 14. Failure modes & robustness

- Backend crash → re-scan registry + re-read disk → full recovery (near-stateless).
- Agent crash → stale heartbeat → marked down; its last state still readable.
- Command lost → inbox files are durable + ack'd; safe to retry.
- Backend/agent version skew → versioned protocol; negotiate/reject politely.
- Two writers → avoided: only the agent loop writes its own state; backend never
  pokes state files directly (commands go through the loop).

---

## 15. Open questions (let's iterate)

1. **Headless vs attached:** do agents always run headless under the backend, or
   must the backend also observe agents a human launched in a terminal TUI? (The
   disk-read path supports both — confirm we want that co-existence.)
2. **Action latency:** is ~50–100 ms (file inbox) acceptable for v1, or do you
   want socket-level (~1 ms) req/resp from day one?
3. **Live token streaming:** does the web UI need true token-by-token streaming,
   or is near-real-time (per-message, via disk tailing) enough for v1? (This is
   the main thing that would force a live channel vs pure disk.)
4. **Identity:** folder-path-derived stable id (my lean) vs random per-process id?
5. **Multi-worker exposure:** collapse an agent's N workers into one frontend
   lane, or surface them?
6. **Frontend push:** REST + **SSE** (my lean) vs WebSocket?
7. **Backend language/stack:** Rust (reuse the crates + protocol types directly,
   share `cp-render` IR) vs something else? (Strong lean: Rust, to reuse the
   existing serializable IR/state types verbatim.)

## 16. Decision log

_(to be filled as we align — each entry: date · question · ruling · rationale)_
