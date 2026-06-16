# Orchestration Backend — Design Doc (WIP)

> **Status:** discussion / brainstorm. **Nothing here is implemented.** Living
> artifact we iterate on until we're perfectly aligned on the infrastructure
> that powers the orchestration frontend (the `ui/` maquette).
>
> **v3 — two-plane architecture + exhaustive problem register.** v2 made the
> *state* path robust but assumed disk-tailing for everything; that is fatal for
> **live token streaming**. v3 splits the system into a **durable control plane**
> (disk, eventually-consistent, authoritative) and an **ephemeral stream plane**
> (in-memory over a Unix socket, near-millisecond, best-effort). It also adds a
> full **Problem Register (§20)** — every foreseeable failure, its severity, and
> its mitigation — because this is months of active development and a
> production v1 for clients, not a throwaway.

---

## 1. Problem statement

The frontend orchestrates **many agents**. Each agent is a single Rust loop
running inside its own folder (its realm) and does **not** know about other
agents — so no agent can own the orchestration backend. We need a **standalone
backend** between the frontend and the fleet.

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
   └─────────┘      └─────────┘      └─────────┘
```

---

## 2. Constraints & principles (from the captain)

1. **1 agent = 1 Rust loop = 1 folder.** Realm-confined.
2. **No agent owns the backend.**
3. **Backend is standalone.**
4. Design frontend ↔ backend comms.
5. Design backend ↔ agent comms.
6. Backend must **discover** agents.
7. **CRITICAL — minimal impact on agent Rust code.** Additive changes (expose a
   socket, write a heartbeat) are fine; **never change how the agent
   reasons/acts**. The agent must run identically whether or not the backend is
   watching.
8. Catalog **information to gather from** + **actions to perform on** agents.

**New, load-bearing (v3):**

9. **Live streaming must be FLUID.** Flow: *LLM provider → rust agent → backend
   → frontend*, with **near-millisecond added delay** end-to-end. A user
   watching the Cockpit sees tokens appear as if the model were in the browser.
   Every added millisecond costs users. This is a hard requirement, not a
   nice-to-have.
10. **Production-ready on v1.** No "we'll rewrite it three times." Foreseeable
    issues are tracked now (§20) and designed against now.

**Preferences:** simple, robust, leverage existing choices. Same machine for
now, but an **abstraction layer** over comms **and** discovery. Per-agent
connection managed internally.

---

## 3. The central idea: two planes

The single most important decision. We split all backend↔agent traffic into two
planes with **opposite** trade-offs, so neither requirement compromises the
other.

| | **Control plane (durable)** | **Stream plane (live)** |
|---|---|---|
| Carries | thread state, final messages, panels, cost, status, registry, **commands** | **live token deltas**, streaming tool-args, phase/typing transitions |
| Medium | **disk** (`.context-pilot/`) + file-watch + poll | **Unix domain socket** (UDS), in-memory push |
| Latency | 50–200 ms (tolerant) | **sub-millisecond** |
| Durability | **authoritative** — survives any crash | **ephemeral** — droppable |
| Consistency | snapshot-consistent (rev/manifest) | best-effort, ordered-per-message |
| On loss | N/A (it's the truth) | irrelevant — the **final** message still lands on the control plane |

**Why this is the keystone.** The stream plane is *allowed to be lossy* because
the control plane is authoritative: if a live token is dropped, the fully
assembled assistant message is persisted to disk and arrives via the control
plane within a tick. So we get **fluid streaming without sacrificing
robustness** — the fast path never has to be durable, and the durable path never
has to be fast. **Invariant I7** (below) makes this explicit: the live plane may
never apply backpressure to the agent's actual work; the durable plane is the
safety net.

---

## 4. Recommended architecture

```
React frontend
   │  REST (load + actions)   +   WebSocket (live: control deltas seq'd + token frames)   ← §9
   ▼
Orchestrator backend (standalone, Rust — reuses cp-base/cp-render types)
   ├── AgentRegistry   (discovery)   → watch ~/.context-pilot/agents/*.json
   ├── AgentChannel[]  (per-agent transport, internally managed)
   │      DURABLE read:   watch <folder>/.context-pilot/ + rev/manifest gating + poll sweep
   │      LIVE stream:    connect <folder>/.context-pilot/stream.sock (UDS) ← token tee
   │      command:        UDS frame (fast) with DURABLE inbox/ file FALLBACK + outbox ack
   ├── AgentSupervisor (lifecycle)   → spawn DETACHED `cp --headless`
   ├── StreamHub       (per-agent fan-out: 1 UDS in → N frontend WS out, bounded buffers)
   └── MaterializedView[]  (in-memory cache = the only backend "state"; rebuildable)
   │
   ▼  Backend ↔ Agent
Agent Rust loop (per folder):
   ├── [hardening]  atomic writes + commit-rev/manifest + boot flock     (read-safety, inert)
   └── [additive]   cp-mod-bridge:
         • boot:    flock agent.lock ; write ~/.context-pilot/agents/<id>.json ; bind stream.sock
         • heartbeat: on a DEDICATED thread (never the main loop)
         • stream TEE: on each StreamEvent → non-blocking publish to stream.sock subscribers
         • command:  read UDS frames + inbox/ files → spine notifications (existing user-msg path)
         • (already) persists all state to .context-pilot/ → backend reads it
```

---

## 5. Invariants (the robustness spine)

- **I1 — Single writer per folder.** `flock` on `.context-pilot/agent.lock`. A
  2nd instance refuses / goes passive. Backend never writes agent state (only
  `inbox/`). Cardinal rule.
- **I2 — Atomic file writes.** `tmp → fsync → rename(2)`. Readers see old-or-new,
  never torn. *(Required hardening — today it's a plain `fs::write`.)*
- **I3 — Snapshot consistency via a manifest.** Each persistence batch writes,
  **last**, a `manifest` = `{ rev, files: [{path, hash}] }` (atomic). The backend
  reads the manifest, then only files whose hash changed; a referenced file
  missing/with a stale hash ⇒ batch in-flight ⇒ wait for next rev. **No mtime,
  no clock dependence** (kills granularity/NTP-step races) and it doubles as an
  **incremental-read index** (only re-read changed files → no read
  amplification). *(Upgrades v2's mtime heuristic.)*
- **I4 — Commands idempotent + ordered + ack'd.** Unique id + sortable seq;
  agent keeps a persisted `seen` ledger; at-least-once delivery, **exactly-once
  effect**.
- **I5 — Backend view is a rebuildable cache.** Only durable truth = agents'
  disks + registry. In-memory views, stream buffers, supervised-process table
  all reconstructable on restart.
- **I6 — A command's effect and its `seen` mark are committed in the SAME rev
  batch (atomic).** This is what makes exactly-once-effect *real* across a crash:
  either both the spine-notification (effect) and the dedup mark persist, or
  neither does. Partial states are impossible. *(New in v3 — v2 hand-waved this.)*
- **I7 — The live plane is best-effort and MUST NOT backpressure the agent.**
  The stream tee is a non-blocking publish; if the UDS buffer is full (slow
  backend / slow frontend), frames are dropped, never queued against the agent's
  work. The durable plane (final message on disk) is the safety net. *(New in v3
  — the guarantee that lets streaming be fast *and* safe.)*

---

## 6. The three abstraction seams

```text
interface AgentRegistry {              // §10 discovery
    list() -> [AgentHandle]
    watch() -> stream<RegistryEvent>   // appeared / disappeared / heartbeat / stale
}

interface AgentChannel {               // per-agent transport (one connection, internally managed)
    snapshot() -> (rev, AgentState)    // consistent durable snapshot (read)
    subscribe_state() -> stream<StateDelta>   // durable deltas since a rev (control plane)
    subscribe_stream() -> stream<StreamFrame> // LIVE token/phase frames (stream plane, best-effort)
    send(Command) -> Future<Ack>       // ordered, idempotent; UDS-fast w/ inbox fallback
    health() -> Liveness
}

interface AgentSupervisor {            // lifecycle / process control
    spawn(folder, opts) -> Future<AgentHandle>   // detached; resolves on registration
    stop(id, mode) ; restart(id) ; adopt(handle)
}
```

- **v1 impls:** `LocalRegistry` (watch the registry dir), `LocalChannel`
  (durable = disk watch + manifest; live = `stream.sock` UDS; command = UDS frame
  + `inbox/` fallback + `outbox/` ack), `LocalSupervisor` (detached
  `cp --headless`, adopt via registry).
- **One transport-agnostic, versioned wire protocol** (`Command` / `StateDelta` /
  `StreamFrame` / `AgentState`). A `Command` is the same struct whether it travels
  as a UDS frame or an inbox file. The medium is swappable (UDS → TCP/QUIC for
  remote, or shared-memory ring for even-lower local latency) **without touching
  orchestration logic**.

---

## 7. Live streaming path (§9 requirement)

The hot path that must be fluid. Flow and latency budget:

```
LLM provider ──SSE──▶ agent (existing)        : network (unavoidable)
agent StreamEvent ──tee──▶ stream.sock (UDS)  : ~microseconds (mem + kernel UDS)
backend recv ──fan-out──▶ frontend WS         : ~microseconds (in-mem) + localhost WS flush
frontend ──render──▶ DOM                       : next animation frame
```

**The agent tee.** The agent already receives `StreamEvent::Chunk` deltas from
the provider and renders them via its typewriter buffer. The bridge adds a
**non-blocking observer** at that exact point: copy the delta into the
`stream.sock` broadcast. The agent renders/persists **identically** — the tee is
a pure side-channel (I7). This is the *only* new behavior on the hot path, and
it cannot affect agent reasoning.

**Frame schema** (small, ordered, attributable):
`StreamFrame { agent_id, worker_id, thread_id, message_id, seq, kind: Token|ToolArgs|Phase, payload }`.
The frontend routes each frame to the right open conversation by
`(thread_id, message_id)` and orders by `seq`; a `seq` gap ⇒ ignore live, fall
back to the final message from the control plane.

**Fan-out (StreamHub).** The backend is the hub: **one** UDS consumer per agent,
**N** frontend WS subscribers. The agent never scales connections (good for #7).
Fan-out is O(subscribers) direct writes — no global scan.

**Backpressure (critical, ties to I7).**
- *Agent → backend:* non-blocking; drop frames if the UDS send buffer is full.
- *Backend → frontend:* each WS connection has a **bounded** buffer; on overflow,
  **coalesce** pending token deltas into one (or drop + flag "catch up from final
  message"). A slow browser never stalls the agent or other viewers.

**Latency hygiene.** `TCP_NODELAY` (if any TCP hop), no output buffering,
**flush per frame**, never debounce tokens, never route tokens through disk.

**Crash mid-stream.** Agent dies → `stream.sock` closes → backend marks the
stream ended → frontend shows "interrupted". The partial live text is ephemeral
(acceptable). Durability is whatever the agent had persisted. *(Optional knob:
checkpoint the partial assistant message to disk every N tokens — trades write
amplification for partial-survival. Default off in v1.)*

**Don't firehose blobs.** Only small deltas go on the stream plane. Large tool
outputs / panels are referenced by `rev`; the frontend fetches them via REST.
Keeps the live channel hot.

---

## 8. Backend ↔ Agent (control plane)

### 8.1 Read — disk + manifest (safe by I2/I3, reliable by event+poll)
Watch `.context-pilot/`, gate on the manifest, incrementally read changed files.
Event-driven watcher for latency **+** a 2–3 s poll-reconcile sweep for
correctness (`notify` drops/coalesces under load — the search indexer already
uses this exact belt-and-suspenders). Co-existence: a human TUI can be open while
the backend reads (I1 keeps a single writer).

### 8.2 Write — command, UDS-fast with durable fallback
- **Socket up (normal):** send the `Command` as a UDS frame → agent applies it in
  its loop (same path as a typed user message; queued safely if mid-stream) →
  acks over UDS. Fast.
- **Socket down / agent busy-booting:** drop the command as a durable
  `inbox/<seq>-<id>.json` file (atomic). The agent processes it on reconnect/boot.
- **Either way:** the `Command` struct is identical; the agent's effect + `seen`
  mark commit atomically (I6); the result is observable via `outbox/<id>` and the
  resulting `rev`.
- **Lifecycle states** surfaced to the UI: `queued → delivered → processing →
  done | failed | expired`. A command has a **TTL**; past it the backend marks it
  `expired` and reissues with a **new** id. The `seen`-ledger window is kept
  **longer than the TTL** so an expired-then-reissued command can never be
  resurrected as a duplicate.
- **Two-phase semantics:** "send message to thread" **acks on acceptance** (fast,
  cheap) — the resulting LLM work is observed later via the stream + state planes.
  Mutations like "archive thread" ack on completion. Never block an ack for
  minutes.

---

## 9. Frontend ↔ Backend

- **REST** — initial load + point queries + non-streaming actions. Every response
  carries `rev`. Actions return a `command id`.
- **WebSocket** — the single live channel, carrying **two frame types**:
  - *control deltas* — `seq`-numbered, **replayable** (state changes, new
    messages, MY_TURN, cost).
  - *token/stream frames* — ephemeral, **not** replayed (final message covers any
    gap).
  Full-duplex so the frontend can send **instant** `stop` / `interrupt` /
  `answer` without a REST round-trip. Binary frames keep token overhead minimal.

  **Why WS over SSE here:** the streaming requirement wants the lowest-overhead,
  flush-per-frame, full-duplex channel; one WS unifies the firehose + the
  seq'd control deltas + instant client→server controls. (SSE remains a viable
  fallback for the control deltas alone if a WS proves troublesome behind a proxy
  — but localhost v1 has no proxy.)

- **Reconnect:** control plane replays the gap by `seq` (bounded ring buffer); if
  the gap exceeds the buffer → `resync` → REST refetch. Stream plane: missed
  tokens are simply dropped; the final message is already in state. **No
  permanent staleness after a sleep/blip.**
- **Client monotonic rev:** ignore any WS frame or REST response with `rev ≤` the
  applied rev (defeats event-before-snapshot races).

---

## 10. Discovery, heartbeat & single-instance

- On boot: take the **folder flock** (I1), bind `stream.sock`, then register
  `~/.context-pilot/agents/<id>.json` =
  `{ id, folder, pid, boot_id, model, protocol_version, binary_version,
  socket_path, started_at, last_heartbeat, status }` (atomic, refreshed).
- **Heartbeat runs on a DEDICATED thread**, never the main loop — a legitimately
  busy agent (long tool, big stream) must still look alive. Reuses the existing
  deadman dedicated-thread pattern.
- **Liveness:** fresh heartbeat **AND** live pid **AND** matching
  `boot_id`/start-time (defeats pid reuse). Else stale → down.
- **Spawn = try-lock-or-adopt:** if a live registry entry / held flock exists →
  adopt; else launch; the launched process's own flock arbitrates a race, the
  loser exits cleanly and the backend adopts the winner.
- **GC:** registry `*.tmp` from crashed registrations reaped by age; stale
  `stream.sock` unlinked by the bridge before re-binding on boot.
- **Unmanaged agents:** live lock, no registry entry (bridge off / old binary) →
  listed read-only via disk; no command/stream channel.

---

## 11. Agent-side delta (the entire footprint)

### 11.1 Additive module — `cp-mod-bridge` (behaviorally inert)
1. **Lock + register + heartbeat** (heartbeat on a dedicated thread).
2. **Stream tee** — non-blocking publish of each `StreamEvent` to `stream.sock`
   (I7). Pure observer.
3. **Command intake** — UDS frames + `inbox/` files → spine notifications (the
   existing user-message path) → ack via `outbox/`.

Disable the bridge ⇒ the agent runs **exactly** as today.

### 11.2 Required hardening to the existing persistence path (behavior-preserving)
- **H1 (I2):** atomic `tmp→fsync→rename` in `PersistenceWriter::write_file`.
- **H2 (I3):** stamp `rev` + write the `manifest` (hashes) **last** in each batch.
- **H3 (I1):** boot `flock` on `agent.lock`.
- **H4 (I6):** route a consumed command's effect + `seen` mark through one batch.

> Entire impact: one additive inert module + four behavior-preserving robustness
> upgrades. Nothing changes *how the agent reasons or acts*. The stream tee is
> the only new hot-path code and it is a non-blocking observer.

---

## 12. Identity & multi-worker

- **Stable id:** FNV-1a of the canonical folder path (reuses search's scheme) →
  stable across restarts. Folder move/rename ⇒ new id + old tombstone
  (acceptable; registry stores the canonical path so a future "rebind" is
  possible).
- **Multi-worker:** an agent may run N internal workers. The frontend speaks
  **threads**; the backend flattens threads across workers and carries
  `worker_id` as metadata (and in every `StreamFrame`). Per-worker lanes are a
  later option.

---

## 13. Failure modes & recovery (summary; full register in §20)

| Actor | Failure | Detection | Recovery |
|---|---|---|---|
| Agent | Hard crash | stale heartbeat + dead pid; `stream.sock` closes | mark down; last snapshot readable; offer restart |
| Agent | Mid-batch crash | manifest points at last committed rev | resume from last committed snapshot |
| Agent | Double-launch | flock contention (I1) | 2nd passive; truth uncorrupted |
| Agent | Re-run command post-crash | `seen` ledger + I6 atomic commit | duplicate skipped; exactly-once effect |
| Stream | Slow frontend / backend | bounded buffers | coalesce/drop frames (I7); agent unaffected |
| Stream | Agent dies mid-stream | socket close | "interrupted"; final message via state plane |
| Backend | Crash / restart | n/a | re-scan registry + re-read disk (manifest) + re-adopt detached agents + re-open UDS + frontends reconnect (I5) |
| Transport | `notify` event dropped | poll-reconcile sweep | converges within sweep |
| Transport | Torn file read | atomic rename (I2) | prior committed version |
| Frontend | WS disconnect | reconnect + `seq` replay / `resync` | gap replay or REST refetch |
| Protocol | Version skew | `protocol_version` + per-msg `schema_version` | tolerant decode; reject unknown major |
| Process | pid reuse | `boot_id`/start-time mismatch | treated as down |

Backend is **supervised** (systemd/launchd); spawned agents are **detached**
(`setsid`) so they survive a backend restart and are re-adopted.

---

## 14. Sequence diagrams

**Live streaming token (the fluid path):**
```
provider →(SSE)→ agent: StreamEvent::Chunk("Hel")
agent: render to typewriter  AND  tee → stream.sock  (non-blocking)
backend: recv frame {thread,message,seq,Token:"Hel"} → fan-out to N WS (bounded)
frontend: append to message_id buffer → paint next frame
… repeat per token, sub-ms added latency …
agent: StreamDone → persist final assistant message → manifest rev++
backend: state delta {message finalized@rev} → WS control frame (authoritative)
frontend: reconcile final text (covers any dropped live token)
```

**Send-message (two-phase ack):**
```
UI →(REST/WS) POST message {text}
backend: assign cmd-id+seq; UDS frame (or inbox/ if socket down)
backend → UI: accepted {cmd-id}  (fast)
agent: dedup → spine notification (queued if streaming); effect+seen in one batch (I6)
agent: outbox/<cmd-id> {ok, rev_after}; then streams the response (→ live path)
backend → UI: state delta @rev_after (durable)
```

**Backend restart recovery:**
```
scan registry → verify liveness (heartbeat+pid+boot_id) → adopt live / tombstone dead
per live agent: open AgentChannel → read snapshot@rev (manifest) → reconnect stream.sock
scan outbox/ for unacked → reconcile in-flight commands (idempotent)
accept frontend WS (resync) → UI refetches
```

---

## 15. Information to gather FROM agents

Identity/lifecycle; status/phase (idle·streaming·tooling·blocked·needs-input·
errored·down); threads (id, name, MY_TURN/ACTIVE/THEIR_TURN, unread, preview,
full conversation on demand, pending questions); conversation messages + **live
streaming deltas** (stream plane); economics (tokens/cost per agent+thread,
cache hit/miss/output, context budget); every context panel (todos, memories,
logs, entities, spine, queue, scratchpad, tools, callbacks, tree, radar);
fleet-level MY_TURN signals + total spend + current `rev`.

## 16. Actions to perform ON agents

Send-to-thread (primary driver); thread create/archive/restore/answer-question;
lifecycle spawn/stop/restart/pause/**interrupt-stream** (instant via WS→UDS);
manage rename/model/archive; toggles (auto-continuation/reverie/think);
thread-scoped coucou. All as idempotent, ack'd commands (§8.2).

---

## 17. Security & permissions

- **v1 (single user, localhost):** `0700` on `.context-pilot/`, registry dir, and
  `stream.sock`. Commands carry `agent_id` (a file/frame for the wrong agent is
  rejected). Backend HTTP/WS **binds to localhost only**, with a frontend auth
  token + locked-down CORS.
- **Future (network/multi-tenant):** auth (bearer/mTLS), signed commands at the
  transport seam — designed-in via the abstraction, no orchestration changes.
- **Secrets:** the registry holds **no** API keys; secrets stay in the agent's
  existing env/config.

## 18. Versioning & compatibility

`schema_version` on every file/frame; serde tolerant decode (ignore unknown,
default missing); registry advertises `protocol_version` + supported commands;
unknown **major** versions rejected with an explicit error (never silently
dropped). Agents are long-lived; you'll upgrade the binary while old ones run.

## 19. Observability & ops (production necessity)

The backend exports: per-agent **stream latency p50/p99**, dropped/coalesced
frame counts, command queue depth + lifecycle-state histogram, **rev lag**
(how far the materialized view trails the agent), heartbeat freshness, WS
subscriber counts, reconnect/resync rates. Structured logs with `agent_id` +
`cmd_id` correlation. Without this, "fluid as fuck" is unmeasurable and
regressions ship silently.

---

## 20. Problem Register (track ALL problems)

Severity: 🔴 critical · 🟠 high · 🟡 medium. Status: ✅ designed · 🔵 knob/optional.

### A. Streaming / latency
| | Problem | Sev | Mitigation | St |
|---|---|---|---|---|
|A1|Disk-tailing too slow for tokens|🔴|Two-plane split: UDS live plane (§3/§7)|✅|
|A2|Nagle / output buffering adds latency|🟠|`TCP_NODELAY`, no buffering, flush per frame|✅|
|A3|Fan-out O(N) scan per token|🟠|Per-agent subscriber list, O(subs) direct writes (StreamHub)|✅|
|A4|Slow frontend backpressures chain|🔴|Bounded per-WS buffer, coalesce/drop (I7)|✅|
|A5|Slow backend stalls agent via tee|🔴|Non-blocking tee, drop on full buffer (I7)|✅|
|A6|Token reorder / wrong-thread|🟠|Frame carries (agent,worker,thread,message,seq)|✅|
|A7|Background-tab throttling|🟡|Frontend coalesces; refetch final on focus|✅|
|A8|Thundering herd (many agents stream)|🟡|Async runtime, one task per UDS, cheap fan-out|✅|
|A9|Control msg stalls token flow (HoL)|🟠|Separate frame types / logical streams; chunk control|✅|

### B. Consistency / state
| | Problem | Sev | Mitigation | St |
|---|---|---|---|---|
|B1|Torn file reads|🔴|Atomic rename (I2)|✅|
|B2|Cross-file inconsistency|🔴|Manifest commit marker (I3)|✅|
|B3|Messages on non-batched write path escape rev|🟠|Append-only/content-addressed; resolve refs per-manifest; or route into batch|✅|
|B4|Delete vs not-yet-written ambiguity|🟠|Interpret refs per-rev manifest only|✅|
|B5|mtime granularity / clock steps|🟠|Manifest hashes, no clock dependence (I3)|✅|
|B6|Read amplification on big agents|🟡|Manifest per-file hashes → incremental re-read (I3)|✅|

### C. Command delivery
| | Problem | Sev | Mitigation | St |
|---|---|---|---|---|
|C1|Double-execution on crash|🔴|`seen` ledger + atomic effect+mark (I4/I6)|✅|
|C2|Ordering|🟠|Sortable seq|✅|
|C3|Lost command|🟠|Durable inbox fallback + outbox ack + retry by id|✅|
|C4|Command to a down agent|🟠|Liveness check; durable queue; lifecycle states + TTL surfaced|✅|
|C5|`seen`-GC vs late re-delivery|🟠|`seen`-window > command-TTL invariant|✅|
|C6|Ack semantics (accept vs done)|🟡|Two-phase: accept-ack fast, effect via stream/state|✅|
|C7|Ack lost (crash after effect)|🟡|Re-derive from rev / idempotent reissue|✅|

### D. Discovery / lifecycle / identity
| | Problem | Sev | Mitigation | St |
|---|---|---|---|---|
|D1|Ghost registry entries|🟠|Heartbeat+pid+boot_id liveness|✅|
|D2|Double-launch same folder|🔴|flock (I1)|✅|
|D3|Folder move/rename changes id|🟡|Canonical path in registry; tombstone; future rebind|✅|
|D4|Heartbeat on main loop ⇒ busy looks dead|🔴|Dedicated heartbeat thread|✅|
|D5|Spawn race|🟠|try-lock-or-adopt; flock arbitrates|✅|
|D6|Spawn failure (env/keys/folder)|🟠|Handshake w/ timeout; read boot-error file; surface|✅|
|D7|Backend restart kills child agents|🔴|Detached `setsid`; re-adopt|✅|
|D8|Registry `*.tmp` litter|🟡|Age-based GC|✅|
|D9|Stale `stream.sock` after crash|🟡|Unlink-before-bind on boot; graceful connect-refused|✅|

### E. Backend robustness
| | Problem | Sev | Mitigation | St |
|---|---|---|---|---|
|E1|Backend SPOF|🟠|Supervised + near-stateless rebuild (I5)|✅|
|E2|Restart loses in-flight commands|🟠|Durable inbox/outbox reconcile|✅|
|E3|Memory growth (views/buffers)|🟠|Bounded ring buffers, lazy materialization|✅|
|E4|Backend↔frontend version skew|🟡|schema_version + capability negotiation|✅|
|E5|Many agents scaling|🟡|Linear rebuild; lazy-materialize on access|🔵|

### F. Frontend consistency
| | Problem | Sev | Mitigation | St |
|---|---|---|---|---|
|F1|Event before snapshot|🟠|Client monotonic rev; ignore ≤ applied|✅|
|F2|Reconnect after sleep|🟠|seq replay / resync→refetch; drop missed tokens|✅|
|F3|Optimistic UI hangs|🟡|Command lifecycle + TTL surfaced|✅|
|F4|Duplicate/out-of-order tokens|🟡|seq gap → fall back to final message|✅|

### G. Security
| | Problem | Sev | Mitigation | St |
|---|---|---|---|---|
|G1|inbox/UDS command-injection surface|🟠|0700 perms; agent-id-stamped commands|✅|
|G2|Network/multi-tenant future|🟡|Auth/mTLS/signed cmds at transport seam|🔵|
|G3|Backend HTTP/WS exposed|🟠|Localhost-bind + auth token + CORS lock|✅|
|G4|Secrets leak via registry/disk|🟠|Registry holds no secrets|✅|

### H. Ops / versioning / observability
| | Problem | Sev | Mitigation | St |
|---|---|---|---|---|
|H1|Binary upgrade w/ live agents|🟠|schema/protocol versioning (§18)|✅|
|H2|No observability|🟠|Metrics + correlated logs (§19)|✅|
|H3|Multi-machine clock sync (future)|🟡|Relative timestamps on receipt|🔵|
|H4|Disk full on agent|🟡|`errored` status surfaced, no silent wedge|✅|

### I. Edge / correctness
| | Problem | Sev | Mitigation | St |
|---|---|---|---|---|
|I1|Multi-worker→thread mapping|🟡|Flatten threads; worker_id metadata|✅|
|I2|Message to actively-streaming thread|🟡|Spine safe-point queue (existing)|✅|
|I3|Partial assistant msg on crash|🟡|Stream ephemeral; optional disk checkpoint|🔵|
|I4|Giant blobs over stream plane|🟠|Reference by rev; REST fetch; keep stream small|✅|
|I5|UTF-8 split mid-token|🟡|Concatenate by message_id; agent buffer handles|✅|

---

## 21. Open questions (genuine choices)

1. **Stream-plane transport confirm:** UDS for v1 (my lean) — agree? (Future:
   shared-memory ring for absolute-lowest local latency, same abstraction.)
2. **Frontend live channel:** one WebSocket carrying control+token frames (my
   lean) vs SSE-control + separate WS-stream?
3. **Partial-message checkpointing (I3-stream):** off in v1 (my lean) or
   checkpoint partial assistant text to disk every N tokens for crash survival?
4. **Multi-worker exposure:** flatten to threads (my lean) vs surface worker
   lanes?
5. **Reaping:** tombstone `down` agents (my lean) vs auto-reap?
6. **Backend stack = Rust** (reuse serializable state/IR types verbatim) — confirm?

## 22. Decision log

Each entry: date · question · ruling · rationale.

- **2026-06-16 · Two-plane architecture** · *Provisional:* durable control plane
  (disk) + ephemeral stream plane (UDS) · the keystone that delivers fluid
  streaming *and* crash-proof state without compromise.
- **2026-06-16 · Stream transport** · *Provisional:* Unix domain socket, tee on
  existing StreamEvent flow · sub-ms, reuses cp-console-server UDS pattern,
  behaviorally inert.
- **2026-06-16 · Frontend live channel** · *Provisional:* single WebSocket
  (seq'd control + ephemeral token frames) · full-duplex instant stop, binary,
  one connection.
- **2026-06-16 · Read transport** · *Provisional:* disk + manifest, incremental ·
  zero new read behavior; survives crashes; co-exists with a human TUI.
- **2026-06-16 · Command transport** · *Provisional:* UDS-fast + durable inbox
  fallback, idempotent/ordered/ack'd · low latency normally, durable always.
- **2026-06-16 · Discovery** · *Provisional:* registry dir + heartbeat (dedicated
  thread) + flock · cheap, file-only, double-writer-proof.
- **2026-06-16 · Identity** · *Provisional:* FNV-1a of canonical folder path.
- **2026-06-16 · Backend stack** · *Provisional:* Rust · reuse serializable
  state/IR types verbatim.
- **2026-06-16 · Invariants** · *Locked candidates:* I1–I7 (I6 atomic
  effect+seen, I7 live-plane-never-backpressures are the v3 additions) · the
  non-negotiable robustness spine.

_(Promote "Provisional" → "Locked" as the captain confirms.)_
