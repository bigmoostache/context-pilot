# Orchestration Backend — Design Doc (WIP)

> **Status:** discussion / brainstorm. **Nothing here is implemented.** This is
> the living artifact we iterate on until we're perfectly aligned on the
> infrastructure that powers the orchestration frontend (the `ui/` maquette).
>
> Branch context: the maquette lives in `ui/` (design-only). This doc designs
> the *real* backend + the minimal agent-side seam that would feed it.
>
> **v2 — robustness pass.** Closes the consistency races, delivery-semantics
> gaps, ghost-agent / double-writer hazards, backend-restart recovery, and
> version-skew handling that v1 left as grey areas. The load-bearing guarantees
> are now stated explicitly as **Invariants (§5)** and every failure mode has a
> detection + recovery path (§13).

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
                         │  Frontend ↔ Backend  (§9)
            ┌────────────▼────────────┐
            │   Orchestrator backend  │   standalone, owns the fleet view
            └────────────┬────────────┘
                         │  Backend ↔ Agents  (§8)
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

> **On constraint #7, stated honestly.** The agent footprint is two buckets:
> (a) a small **additive** module (the *bridge*: heartbeat + inbox/outbox
> watcher) that is **behaviorally inert** — it only adds an input source and a
> status file; disable it and the agent runs exactly as today; and
> (b) three **read-safety hardenings** to the *existing* persistence path
> (atomic file writes, a commit-revision marker, a single-instance lock). These
> touch existing code but **do not change behavior** — the same bytes land in
> the same files; they only make concurrent *reading* safe. They are good
> hygiene even with no backend present. We call these out transparently in §11
> rather than pretending the footprint is literally zero.

---

## 3. Existing design choices we can lean on

These already exist in the codebase and shape the cheapest, most robust path:

- **Everything is already persisted to disk**, continuously, by a debounced
  (50 ms) background persistence writer (`src/state/persistence/writer.rs`),
  under each agent's `.context-pilot/`:
  - `shared/` — `memories.yaml`, `tree-descriptions.yaml`, logs, `config.json`
  - `states/` — per-worker state JSON (multi-worker)
  - `panels/{uid}.json` — every panel
  - conversation messages as YAML files (`messages/`)
  - `logs/` — perf/tool-times/errors; headless `trajectory.jsonl`
- **`cp-console-server`** — a precedent standalone daemon: Unix socket +
  JSON-line protocol, manages child processes that survive restarts. The exact
  pattern we'd reuse for a socket transport, and for *detached* child spawning.
- **Headless mode** (`tui --headless`) — boots terminal-free, drives the loop to
  quiescence, with a **deadman watchdog** (re-exec / abort on wedge). The agent
  can already run as a background daemon with no UI.
- **Spine notifications** — the agent already models *"a user message"* as a
  notification (`NotificationType::UserMessage`) that drives the loop, and it
  already **queues** such input safely while mid-stream, applying it at a safe
  point. Injecting an action from outside = injecting a notification. **This is
  the key seam, and it already has the right concurrency semantics.**
- **File watchers** (`notify`, inotify/kqueue/FSEvents) are already wired
  per-module; the search indexer (`indexer.rs`) already pairs an event watcher
  with a **3 s poll fallback** — the precise belt-and-suspenders pattern we need
  for reliable disk tailing (§7).
- **Search "projects registry"** — there is already a notion of a global
  registry of known project paths under `~/.context-pilot/`, plus FNV-1a path
  hashing (a ready-made stable id scheme — §12).

**Consequence:** *reading* agent state is mostly free (tail the disk), once the
read is made **safe** (§5/§7). The only genuinely new agent *behavior* is
(a) announce itself for discovery and (b) accept commands. Both are additive.

---

## 4. Recommended architecture

> **"Disk-as-truth (read) + Inbox/Outbox-for-actions (write) + Global-registry
> (discover)", behind three swappable interfaces, resting on five invariants.**

```
React frontend
   │  REST (actions/queries) + SSE (live push, replayable via Last-Event-ID)  ← §9
   ▼
Orchestrator backend (standalone, Rust — reuses cp-base/cp-render types)
   ├── AgentRegistry   (discovery)   → watch ~/.context-pilot/agents/*.json
   ├── AgentChannel[]  (per-agent transport, internally managed)
   │       read:  watch <folder>/.context-pilot/  + commit-rev gating + poll sweep
   │       write: drop ordered+idempotent JSON cmd into <folder>/.context-pilot/inbox/
   │       ack:   watch <folder>/.context-pilot/outbox/<cmd-id>.result.json
   ├── AgentSupervisor (lifecycle)   → spawn DETACHED `cp --headless` in a folder
   └── MaterializedView[]  (per-agent in-memory cache = the only backend "state")
   │
   ▼  Backend ↔ Agent — file-based now, socket/net later (same versioned protocol)  ← §8
Agent Rust loop (per folder):
   ├── [hardening] atomic writes + commit-rev marker + boot flock   (read-safety, inert)
   └── [additive]  cp-mod-bridge:
         • boot:  flock .context-pilot/agent.lock ; write ~/.context-pilot/agents/<id>.json
         • watch  .context-pilot/inbox/  → ordered, idempotent → spine notifications
         • write  .context-pilot/outbox/<cmd-id>.result.json   (ack)
         • (already) persists all state to .context-pilot/  → backend just reads it
```

**Why this fits the constraints** — minimal impact (reads are disk-tailing; the
only new behavior is an inbox→spine path identical to a typed user message);
robust (disk is durable truth, backend is a rebuildable cache, agents are
independent); simple (commands are files, discovery is a directory); and it
leans on headless mode, spine-as-input, the console-server daemon pattern, the
projects registry, the poll-fallback watcher, and continuous persistence.

---

## 5. Invariants (load-bearing — everything rests on these)

These are the guarantees that make the system non-breakable. Each has an
enforcement mechanism.

- **I1 — Single writer per folder.** Exactly one process ever writes a given
  `.context-pilot/` state. Enforced by an **exclusive boot lock** (`flock` on
  `.context-pilot/agent.lock`). A second `cp` instance in the same folder
  refuses to start (or runs passive/read-only). The backend **never** writes
  agent state — it writes *only* into `inbox/` (a region whose lifecycle the
  agent owns). This is the cardinal rule; double-writers are the one thing that
  can corrupt truth.
- **I2 — Atomic file writes.** Every state file is written `tmp → fsync →
  rename(2)` (atomic within a filesystem). A reader therefore sees *either* the
  old *or* the new file, never a torn/truncated one. (Required hardening — today
  the writer uses a plain `fs::write`; see §11.)
- **I3 — Snapshot consistency via a commit marker.** The writer stamps a
  monotonically increasing **revision** and writes the marker (`config.json`'s
  `rev`, or a dedicated `rev` file) **last** in each batch. The backend reads the
  rev first, then the files it references; if any referenced file is newer than
  the marker, a newer batch is mid-flight → the backend waits for the next rev
  bump. This turns N independent files into a consistent snapshot at a single
  commit point. (Required hardening — today there is no ordered commit marker.)
- **I4 — Commands are idempotent, ordered, and acknowledged.** Each command has
  a unique id and a lexically-sortable sequence; the agent processes in order,
  records processed ids (persisted ledger), skips duplicates on restart, and
  writes a result/ack. Delivery is at-least-once; *effect* is exactly-once.
- **I5 — The backend's view is a rebuildable cache.** The backend's only durable
  truth is on the agents' disks + the registry. Its in-memory materialized view,
  SSE buffers, and supervised-process table are all reconstructable by re-scan +
  re-read + re-adopt on restart. The backend is therefore disposable/restartable
  without data loss.

---

## 6. The three abstraction seams (your explicit asks)

Transport-, discovery-, and lifecycle-agnostic interfaces. One local impl today;
swap freely later (remote machine, mDNS, k8s…) without touching orchestration
logic.

```text
interface AgentRegistry {              // §10 discovery
    list() -> [AgentHandle]            // current known agents
    watch() -> stream<RegistryEvent>   // appeared / disappeared / heartbeat / stale
}

interface AgentChannel {               // per-agent transport (one connection)
    snapshot() -> (rev, AgentState)    // consistent snapshot at a revision (read)
    subscribe() -> stream<AgentEvent>  // live deltas since a rev (read)
    send(Command) -> Future<Ack>       // ordered, idempotent action (write)
    health() -> Liveness               // alive / stale / down
}

interface AgentSupervisor {            // lifecycle / process control
    spawn(folder, opts) -> Future<AgentHandle>   // detached; resolves on registration
    stop(id, mode)                               // graceful | kill
    restart(id)
    adopt(handle)                                // re-attach to an already-running agent
}
```

- **v1 impls:** `LocalRegistry` (watch `~/.context-pilot/agents/`),
  `LocalFsChannel` (read = watch `.context-pilot/` + rev-gating + poll sweep;
  write = `inbox/`; ack = `outbox/`), `LocalSupervisor` (detached `std::process`
  spawn of `cp --headless`, re-adopt via registry).
- **The wire protocol is defined once, transport-agnostic** (versioned JSON
  `Command` / `Event` / `AgentState` schemas, §18). Whether a `Command` travels
  as a file, a Unix-socket line, or a TCP/WS frame, its shape is identical —
  that's the layer that lets the medium change.

---

## 7. Consistency & freshness model

Two mechanisms, combined (belt + suspenders, mirroring `indexer.rs`):

1. **Event-driven (low latency):** a `notify` watcher on `.context-pilot/` wakes
   the backend on change. On a rev-marker change, the backend reads the new
   consistent snapshot (per **I3**) and emits deltas to subscribers.
2. **Poll-reconcile (correctness):** a periodic sweep (e.g. every 2–3 s)
   re-stats the marker + key files. `notify` can *miss* or *coalesce* events
   (network/overlay filesystems, load); the sweep guarantees eventual
   convergence even if an event is dropped. The watcher is the optimization; the
   sweep is the guarantee.

**Read protocol per agent:** read `rev`; if `rev` unchanged since last view →
nothing to do. If bumped → read referenced files; if any file's mtime > marker's
mtime, treat the snapshot as in-flight and retry on the next tick (bounded
retries, then accept best-effort + flag `degraded`). Materialize into the typed
`AgentState`, diff against the previous view, emit deltas.

---

## 8. Backend ↔ Agent

### 8.1 Read path — disk tailing (made safe by I2/I3, reliable by §7)

Backend watches `<folder>/.context-pilot/` and reads the same files the agent
already writes, gated by the commit revision. Durable, already-debounced,
survives agent crashes (last good snapshot remains). **Co-existence bonus:** a
human can have the TUI open on an agent while the backend observes read-only —
the lock (**I1**) ensures there's still only one writer.

### 8.2 Write path — command inbox/outbox (the one new agent behavior)

```
backend                                   agent (bridge, in its own loop)
   │  write inbox/0001-<cmd-id>.json.tmp        │
   │  rename → inbox/0001-<cmd-id>.json  (I2)   │
   │ ───────────────────────────────────────▶  │  notify/poll picks it up (sorted)
   │                                            │  seen[cmd-id]? → skip (I4)
   │                                            │  else → spine notification
   │                                            │        (same path as a user msg;
   │                                            │         queued safely if streaming)
   │                                            │  record seen[cmd-id]; delete inbox file
   │  watch outbox/<cmd-id>.result.json         │  write outbox/<cmd-id>.result.json (I2)
   │ ◀───────────────────────────────────────  │   {status: ok|err, rev_after, payload}
   │  resolve Ack; GC result                    │
```

- **Ordering:** filenames carry a zero-padded/ULID sequence; the agent applies
  in sorted order.
- **Idempotency (I4):** the `seen` ledger (bounded, persisted in worker/bridge
  state) makes re-delivery harmless across crashes.
- **Ack/result:** gives true request/response over files. The `rev_after` lets
  the backend know which snapshot reflects the command's effect.
- **Atomic (I2):** both inbox and outbox files are tmp+rename, so neither side
  ever reads a half-written file.
- **GC:** agent deletes consumed inbox files immediately; results GC'd by
  age/count; backend deletes results once acked. Dirs stay bounded.
- **Backpressure / disk-full:** if the agent can't persist (disk full), it
  surfaces an `errored` status (which the backend reads) rather than wedging.

### 8.3 Tradeoff captured: file vs socket

| | File (inbox/outbox + disk) | Unix socket (console-server style) |
|---|---|---|
| Agent code | watcher + heartbeat (+ inert hardening) | listener thread + protocol |
| Latency | ~watcher debounce (≈50–100 ms) | ~1 ms, true req/resp |
| Robustness | durable, survives crash, replayable, ack'd | needs live process; reconnect logic |
| Minimal-impact (#7) | **best** | good |

**Lean:** file-based v1 (lowest impact, most robust, naturally durable +
replayable). Keep `AgentChannel` abstract so a socket impl is a drop-in upgrade
if action latency ever bites — *reads can stay on disk even then.*

---

## 9. Frontend ↔ Backend

- **REST** for actions & point queries: `GET /agents`, `GET /agents/:id`,
  `GET /agents/:id/threads`, `POST /agents/:id/threads/:tid/messages`,
  `POST /agents` (spawn), `POST /agents/:id/stop`, … Actions return the assigned
  `command id` so the UI can correlate the eventual effect.
- **SSE** for live server→client push (status, new messages, token deltas,
  MY_TURN). **Replayable:** every event carries a monotonic `id`; the backend
  keeps a bounded per-stream ring buffer; on reconnect the browser sends
  `Last-Event-ID` and the backend replays the gap. If the gap exceeds the
  buffer → emit a `resync` event → frontend does a full REST refetch. No
  permanent staleness after a blip/sleep.

**Lean:** **REST + SSE**. The frontend only needs server→client push for live
updates (REST covers client→server actions); SSE is simpler & more robust than
full-duplex WS, has built-in reconnect + `Last-Event-ID` replay, and we already
understand SSE deeply from the LLM streaming side. WS stays an option if we ever
want client→server streaming (e.g. live typing indicators).

---

## 10. Discovery, heartbeat & single-instance

- On boot, each agent (via the bridge) takes the **folder lock** (**I1**) then
  registers in a **global registry dir**:
  `~/.context-pilot/agents/<agent_id>.json` =
  `{ id, folder, pid, boot_id, model, protocol_version, binary_version,
  transport, started_at, last_heartbeat, status }`, refreshed on a heartbeat
  interval (atomic write, **I2**).
- The backend's `LocalRegistry` watches that dir (+ poll sweep) → agents
  appearing / disappearing / heartbeating.
- **Liveness (robust against pid reuse):** an agent is *alive* iff heartbeat is
  fresh **and** `pid` is alive **and** the process start-time / `boot_id`
  matches the registry entry (defeats pid recycling). Otherwise *stale* → *down*.
  No agent code is needed to detect its own death — the backend infers it.
- **Ghost cleanup:** a `down` entry whose folder lock is free can be reaped by
  the backend (or left as a tombstone the UI shows as "down — restart?").
- **Legacy / unmanaged agents:** a folder with a live lock but **no** registry
  entry (bridge disabled or old binary) is still **disk-observable** read-only;
  the backend lists it as `unmanaged` (no command channel).
- Behind `AgentRegistry` so we can later swap to mDNS / a discovery service /
  remote inventory.

---

## 11. Agent-side delta (the entire footprint)

### 11.1 Additive module — `cp-mod-bridge` (behaviorally inert)

1. **Lock + register + heartbeat:** flock `agent.lock`; write/refresh
   `~/.context-pilot/agents/<id>.json`.
2. **Inbox watcher:** watch `.context-pilot/inbox/`, apply commands in order,
   idempotently, as spine notifications/actions (the existing user-message
   path), ack via `outbox/`, delete consumed files.
3. *(optional, later)* **Live event channel:** low-latency push of deltas if
   disk-tailing latency proves insufficient.

Properties: `is_global`, additive, **behaviorally inert** (only adds an input
source + a status file). Disable it ⇒ the agent runs exactly as today.

### 11.2 Required hardening to the existing persistence path (read-safety)

These touch existing code but are **behavior-preserving** — same bytes, same
files, only safe to read concurrently. Good hygiene regardless of the backend.

- **H1 (I2):** `PersistenceWriter::write_file` → write `*.tmp`, `fsync`,
  atomic `rename`. (Today: plain `fs::write` → torn reads possible.)
- **H2 (I3):** stamp a monotonic `rev` and write the commit marker **last** in
  each batch (extend `build_save_batch` ordering + add `rev` to `config.json`).
- **H3 (I1):** acquire `flock` on `.context-pilot/agent.lock` at boot; refuse /
  go passive on contention.

> This is the whole "impact on agent Rust code": one additive inert module + three
> behavior-preserving robustness upgrades. Nothing changes *how the agent
> reasons or acts*.

---

## 12. Identity & multi-worker

- **Stable id:** derive `agent_id` from the canonical folder path via the
  **FNV-1a hash already used by search** for project hashing → stable across
  restarts, lets the backend reconnect to "the same agent", and is collision-
  resistant enough for a single user's fleet. (vs a random per-process id, which
  breaks reconnection.)
- **Worker mapping:** an agent (folder) may run N internal workers (subworkers
  design). The frontend speaks in **threads**, not workers. **Lean:** the backend
  exposes the agent's threads (flattened across workers) and hides the worker
  abstraction in v1; the worker id rides along as metadata for later. Revisit if
  we want per-worker lanes.

---

## 13. Failure modes & recovery (the derisking table)

| Actor | Failure | Detection | Recovery |
|---|---|---|---|
| Agent | Hard crash (SIGKILL) | stale heartbeat + dead pid (I1 lock freed) | mark `down`; last snapshot still readable; offer restart |
| Agent | Mid-batch crash | next boot: partial files, but **I2/I3** mean last *committed* rev is intact | resume from last committed snapshot |
| Agent | Double-launch in same folder | **I1** flock contention | 2nd instance refuses/goes passive; truth uncorrupted |
| Agent | Re-processes a command after crash | **I4** `seen` ledger | duplicate skipped; exactly-once effect |
| Agent | Disk full | write errors → `errored` status surfaced | backend shows error; no silent wedge |
| Backend | Crash / restart | n/a | re-scan registry + re-read disks (rev-gated) + **adopt** detached agents + frontends reconnect SSE → full recovery (**I5**) |
| Backend | Crash with in-flight commands | unacked outbox results on restart | re-issue by same id (**I4** idempotent) or read existing result |
| Transport | `notify` event dropped | poll-reconcile sweep (§7) | converges within sweep interval |
| Transport | Torn file read | **I2** atomic rename | reader sees prior committed version |
| Frontend | SSE disconnect (sleep/blip) | reconnect w/ `Last-Event-ID` | replay gap from ring buffer, or `resync` → REST refetch |
| Protocol | Version skew | `protocol_version` in registry + per-message `schema_version` | tolerant decode; reject unknown major-version commands with an error result (§18) |
| Process | pid reuse after crash | `boot_id`/start-time mismatch (§10) | treated as down, not a false-alive |

**Backend supervision:** the backend should itself run under `systemd` / `launchd`
(or a tiny watchdog) so its own crash auto-restarts into the recovery sequence
above. The agents it spawned are **detached** (`setsid`), so they survive a
backend restart and are re-adopted via the registry.

---

## 14. Sequence diagrams

**Send-message round trip (with ack):**
```
UI → POST /agents/A/threads/T/messages {text}
backend: assign cmd-id, seq; write inbox/<seq>-<cmd-id>.json (atomic)
backend → UI 202 {cmd-id}                       (optimistic; UI shows "sending")
agent: pick up (sorted), dedup, → spine notification (queued if streaming)
agent: process in loop → new ThreadMessage persisted → rev bump
agent: write outbox/<cmd-id>.result.json {ok, rev_after}
backend: watch sees result → resolve cmd-id; read snapshot@rev_after
backend → UI  SSE {thread T updated, message appended}
```

**Spawn handshake:**
```
UI → POST /agents {folder, model}
backend: verify folder, no live lock; spawn detached `cp --headless` (env injected)
backend: await registry entry with fresh heartbeat (timeout Ts)
  ├─ appears → 201 {agent-id}; begin watching folder
  └─ timeout → 500; read agent boot-error file; surface logs
```

**Backend restart recovery:**
```
boot → scan ~/.context-pilot/agents/*.json
     → for each: verify liveness (heartbeat+pid+boot_id); adopt live, tombstone dead
     → for each live: open AgentChannel, read snapshot@rev, materialize view
     → scan outbox/ for unacked results → reconcile in-flight commands
     → accept frontend SSE (resync events) → UI refetches
```

---

## 15. Information to gather FROM agents

(Everything the cockpit/threads/usage views render — all already on disk,
read as a consistent snapshot per §7.)

- **Identity & lifecycle:** id, name, folder/realm, model, pid, boot_id, uptime,
  binary/protocol version.
- **Status / phase:** idle · streaming · tooling · blocked · needs-input ·
  errored · down (maps to the multi-worker WORKING / NEEDS-ATTENTION buckets).
- **Threads:** list (id, name, status `MY_TURN`/`ACTIVE`/`THEIR_TURN`, unread,
  last activity, preview); full conversation on demand; pending question forms.
- **Conversation:** user / assistant / tool messages; streaming deltas.
- **Economics:** tokens & cost per agent + per thread; cache hit/miss/output;
  context budget; cache stats.
- **Context panels:** todos, memories, logs, entities, spine notifications,
  queue, scratchpad, tools, callbacks, tree, radar.
- **Fleet-level signals:** which agents are `MY_TURN` (need the human), total
  spend, counts, current `rev` (freshness).

## 16. Actions to perform ON agents

- **Send a message to a thread** (the primary driver).
- **Threads:** create, archive/restore, answer a thread question.
- **Lifecycle:** spawn (process in a folder), stop / restart / pause, stop
  streaming (`user_stopped`), delete.
- **Manage:** rename, switch model, archive.
- **Settings/toggles:** auto-continuation, reverie, think reminders (API keys are
  org-managed — out of scope for per-agent action).
- **Scheduling:** thread-scoped coucou reminders.
- *(multi-worker, if surfaced later):* switch/create/delete worker.

All actions are issued as **idempotent, ack'd commands** (§8.2).

---

## 17. Security & permissions

- **v1 (single user, single machine):** `inbox/` is a command surface — whoever
  can write the folder can drive the agent. On a single-user box that's the user
  (acceptable). Set `0700` on `.context-pilot/` and the registry dir; commands
  carry the agent-id so a file dropped in the wrong folder is rejected.
- **Future (network/multi-tenant):** the transport seam (`AgentChannel`) is where
  auth lands — bearer tokens / mTLS on the socket/HTTP transport, signed
  commands. Designing the command schema as transport-agnostic now means adding
  auth later doesn't touch orchestration logic.

---

## 18. Versioning & compatibility

- Every persisted state file and every `Command`/`Event` carries a
  `schema_version`. Decoders **ignore unknown fields** and **default missing
  ones** (serde) → forward/backward tolerant for minor changes.
- The registry entry advertises `protocol_version` + supported command set →
  the backend only sends commands an agent understands; unknown **major**
  versions are rejected with an explicit error result (never silently dropped).
- Agents are long-lived; you will upgrade the binary while old agents run. This
  section is what makes that safe.

---

## 19. Open questions (trimmed — genuine choices only)

1. **Action latency:** is ~50–100 ms (file inbox) acceptable for v1, or do you
   want socket-level (~1 ms) req/resp from day one? *(Lean: file v1, socket later
   behind the same seam.)*
2. **Live token streaming:** does the web UI need true token-by-token streaming,
   or is per-message near-real-time (disk tailing) enough for v1? *(This is the
   main thing that would force a live event channel from the agent.)*
3. **Multi-worker exposure:** collapse an agent's N workers into one frontend
   lane (my lean), or surface them as sub-lanes?
4. **Reaping policy:** auto-reap `down` agents from the registry, or keep
   tombstones the UI shows as "down — restart?" *(Lean: tombstone, manual reap.)*
5. **Backend stack confirmation:** Rust (reuse crates + serializable IR/state
   types verbatim, share `cp-render` IR) — confirm? *(Strong lean: yes.)*

## 20. Decision log

Each entry: date · question · ruling · rationale.

- **2026-06-16 · Read transport** · *Provisional:* disk-tailing, no agent read
  API · agent already persists everything; zero new read behavior; survives
  crashes; human-TUI co-existence.
- **2026-06-16 · Write transport** · *Provisional:* file inbox/outbox,
  idempotent + ordered + ack'd · most minimal-impact + durable; socket is a
  drop-in upgrade behind `AgentChannel`.
- **2026-06-16 · Discovery** · *Provisional:* global registry dir
  `~/.context-pilot/agents/*.json` + heartbeat + folder flock · cheap, file-only,
  reuses projects-registry idea; flock kills the double-writer hazard.
- **2026-06-16 · Frontend push** · *Provisional:* REST + SSE w/ `Last-Event-ID`
  replay · simpler/robuster than WS; reuses SSE expertise; no permanent staleness.
- **2026-06-16 · Identity** · *Provisional:* FNV-1a hash of canonical folder path
  · stable across restarts; reuses search's scheme.
- **2026-06-16 · Backend stack** · *Provisional:* Rust · reuse serializable
  state/IR types verbatim, share the protocol definitions with agents.
- **2026-06-16 · Invariants** · *Locked-in candidates:* I1 single-writer (flock),
  I2 atomic writes, I3 commit-rev snapshot, I4 idempotent ordered ack'd commands,
  I5 backend-view-is-a-cache · these are the non-negotiable robustness spine.

_(Promote "Provisional" → "Locked" as the captain confirms.)_
