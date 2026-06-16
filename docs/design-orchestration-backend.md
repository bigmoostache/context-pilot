# Orchestration Backend — Design Doc (WIP)

> **Status:** discussion / brainstorm. **Nothing here is implemented.** Living
> artifact we iterate on until we're perfectly aligned on the infrastructure
> that powers the orchestration frontend (the `ui/` maquette).
>
> **v4 — hardened against the adversarial review.** v3 introduced the two-plane
> architecture (durable control plane + ephemeral stream plane) and the Problem
> Register. v4 closes **every** issue raised in
> [`architecture-adversarial-analysis.md`](./architecture-adversarial-analysis.md):
> three new invariants (**I8** single-writer commit transaction, **I9**
> capability-token command auth, **I10** cross-plane causal ordering), phase
> moved off the lossy plane onto the durable plane, a real WS handshake auth, a
> semantic dedup token, lazy cold-rebuild, an N-1 version-compat window, a global
> cost circuit-breaker + spawn allow-list, and an explicit fsync barrier. §23
> maps each of the 18 adversarial issues to its resolution.

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
9. **Live streaming must be FLUID.** Flow: *LLM provider → rust agent → backend →
   frontend*, **near-millisecond added delay** end-to-end. Every added
   millisecond costs users. Hard requirement.
10. **Production-ready on v1.** No "rewrite it three times." Foreseeable issues
    are tracked (§20) and designed against now.

**Preferences:** simple, robust, leverage existing choices. Same machine for now,
but an **abstraction layer** over comms **and** discovery. Per-agent connection
managed internally.

---

## 3. The central idea: two planes

We split all backend↔agent traffic into two planes with **opposite** trade-offs,
so neither requirement compromises the other.

| | **Control plane (durable)** | **Stream plane (live)** |
|---|---|---|
| Carries | thread state, final messages, panels, cost, registry, **commands**, **phase/status transitions**, **command-lifecycle** | **live token deltas**, streaming tool-arg deltas, **message-start frames** |
| Medium | **disk** (`.context-pilot/`) + file-watch + poll | **Unix domain socket** (UDS), in-memory push |
| Latency | 50–200 ms (tolerant) | **sub-millisecond** |
| Durability | **authoritative** — survives any crash | **ephemeral** — droppable |
| Consistency | snapshot-consistent (rev/manifest) | best-effort, causally ordered per message (I10) |
| On loss | N/A (it's the truth) | irrelevant — the **final** message + the **phase** still land on the control plane |

**Why this is the keystone.** The stream plane is *allowed to be lossy* because
the control plane is authoritative: drop a live token and the assembled message
arrives via the control plane within a tick. **Streaming is fast; state is safe;
neither compromises the other.**

**v4 correction (adversarial #6).** *Sticky/latched* state — phase (idle ·
streaming · tooling), status — is **never** on the lossy plane. A dropped "stream
ended" frame must not strand the UI as "streaming forever." Phase rides the
**durable control plane** with a `rev`. The stream plane carries **only**
ephemeral deltas that are safe to drop (tokens, tool-arg deltas) plus the
self-describing **message-start** frame (I10).

---

## 4. Recommended architecture

```
React frontend
   │  REST (load + actions)  +  WebSocket (auth'd: seq'd control deltas + ephemeral token frames)  ← §9
   ▼
Orchestrator backend (standalone, Rust — reuses cp-base/cp-render types)
   ├── AgentRegistry   (discovery)   → watch ~/.context-pilot/agents/*.json (rare writes)
   ├── AgentChannel[]  (per-agent transport, internally managed)
   │      DURABLE read:   watch <folder>/.context-pilot/ + rev/manifest gating + poll sweep
   │      LIVE stream:    connect <folder>/.context-pilot/stream.sock (UDS) ← token tee
   │      liveness:       UDS connected  +  polled heartbeat file (NOT a watched rename)
   │      command:        UDS frame (fast) + cap-token + dedup-token, DURABLE inbox/ fallback + outbox ack
   ├── AgentSupervisor (lifecycle)   → spawn DETACHED `cp --headless` (ALLOW-LIST gated)
   ├── StreamHub       (per-agent fan-out: 1 UDS in → N frontend WS out, bounded buffers, degraded flag)
   ├── CostBreaker     (global aggregate-spend circuit-breaker)
   └── MaterializedView[]  (in-memory cache = backend's only "state"; LAZY rebuild)
   │
   ▼  Backend ↔ Agent
Agent Rust loop (per folder):
   ├── [hardening]  commit-transaction writer + manifest + boot flock(FD-inherited)   (read-safety, inert)
   └── [additive]   cp-mod-bridge:
         • boot:    flock agent.lock (FD-inheritable) ; write registry entry + cap_token ; bind stream.sock
         • heartbeat: DEDICATED thread → polled heartbeat file (decoupled from registry)
         • stream TEE: StreamEvent → lock-free SPSC enqueue → DEDICATED publisher thread → stream.sock
         • command:  verify cap_token + dedup_token → spine notification (existing user-msg path) → outbox ack
         • (already) persists all state to .context-pilot/ → backend reads it
```

---

## 5. Invariants (the robustness spine)

- **I1 — Single writer per folder.** `flock` on `.context-pilot/agent.lock`. A 2nd
  instance refuses / goes passive. Backend never writes agent state (only
  `inbox/`). Cardinal rule. *(See H5 for the deadman-re-exec interaction.)*
- **I2 — Atomic, power-safe file writes.** `tmp → write → fsync(file) → rename(2)`,
  and for a batch: **fsync every data file, then rename the manifest, then fsync
  the containing directory**. Readers see old-or-new, never torn; a durable
  manifest can never reference content lost from the page cache. *(Required
  hardening — today it's a plain `fs::write`.)*
- **I3 — Snapshot consistency via a manifest.** Each batch writes, **last**, a
  `manifest = { rev, files: [{path, hash}] }` (atomic). The backend reads the
  manifest, then only files whose hash changed; a referenced file missing / with a
  stale hash ⇒ batch in-flight ⇒ wait for next rev. No mtime, no clock dependence;
  doubles as an incremental-read index.
- **I4 — Commands idempotent + ordered + ack'd, by SEMANTIC key.** Each command
  carries a unique transport id + sortable seq **and** a client-supplied
  **`dedup_token`** (semantic key). The agent's `seen` ledger keys on the
  `dedup_token`, so a TTL-reissue with a *new transport id but the same
  dedup_token* is still deduped. At-least-once delivery, **exactly-once effect**.
- **I5 — Backend view is a LAZILY-rebuildable cache.** Only durable truth =
  agents' disks + registry. On restart the backend eagerly rebuilds **only** the
  registry + each agent's `rev`/manifest head; message bodies and large panels are
  **hydrated on demand**. Restart latency is bounded, independent of fleet disk
  size.
- **I6 — A command's effect and its `seen` mark commit in the SAME transaction**
  (subsumed by I8). Either both the spine-notification (effect) and the dedup mark
  persist, or neither. No partial states across a crash.
- **I7 — The live plane is best-effort and MUST NOT backpressure the agent.** The
  tee is a **lock-free SPSC enqueue** on the hot loop; a **dedicated publisher
  thread** serializes + writes the socket. If a buffer is full, frames are
  dropped/coalesced, never queued against the agent's work — and the affected
  stream is flagged **degraded** so the UI can show it (not silent jank). The
  durable plane is the safety net.
- **I8 — Single-writer commit transaction (NEW v4).** Data files, **message
  files**, `seen`-ledger marks, and the `rev`/manifest bump all flow through **one
  ordered durable transaction on one channel**. The manifest enumerates **every**
  file in the batch *including messages* (closing the v3 "message path escapes the
  rev" hole, B3). The single persistence actor assigns `rev` and atomically
  snapshots **all workers** per batch (closing the multi-worker rev question). The
  v3 dual `Batch`/`Message` channel split is **removed for command-effect and
  state writes**; message writes participate in the manifest.
- **I9 — Every command is authenticated by a capability token (NEW v4).** At boot
  the agent mints a random 256-bit `cap_token`, stored in its `0600` registry
  entry. **Every** command — UDS frame *and* `inbox/` file — must carry the
  matching `cap_token`; the agent rejects mismatches. Raises the bar from "any
  same-user process may blindly inject into `inbox/`" to "must first read the
  `0700` registry" and makes the backend the sole practical command author.
  *(True defense against same-user malware needs OS sandboxing — §17, future.)*
- **I10 — Cross-plane causal ordering (NEW v4).** A token frame may arrive before
  the control-plane "message created" delta. Resolution is twofold and composes:
  (a) the **first** `StreamFrame` for a `message_id` is a self-describing
  **`MessageStart`** carrying `{thread_id, worker_id, author, base_rev}` — enough
  to lazily create the buffer; (b) the frontend additionally **buffers orphan
  tokens** by `message_id` until the start frame or control delta lands. A `seq`
  gap on the stream ⇒ ignore live, reconcile from the final message on the control
  plane.

---

## 6. The three abstraction seams

```text
interface AgentRegistry {              // §10 discovery
    list() -> [AgentHandle]
    watch() -> stream<RegistryEvent>   // appeared / disappeared / status / stale
}

interface AgentChannel {               // per-agent transport (one connection, internally managed)
    snapshot() -> (rev, AgentState)         // consistent durable snapshot (read)
    subscribe_state() -> stream<StateDelta> // durable deltas since a rev (control plane; incl phase + cmd-lifecycle)
    subscribe_stream() -> stream<StreamFrame> // LIVE token/MessageStart frames (stream plane, best-effort)
    send(Command) -> Future<Ack>        // cap-token + dedup-token; ordered, idempotent; UDS-fast w/ inbox fallback
    health() -> Liveness                // UDS connected + polled heartbeat
}

interface AgentSupervisor {            // lifecycle / process control
    spawn(folder, opts) -> Future<AgentHandle>   // ALLOW-LIST gated; detached; resolves on registration
    stop(id, mode) ; restart(id) ; adopt(handle)
}
```

- **v1 impls:** `LocalRegistry` (watch the registry dir), `LocalChannel` (durable
  = disk watch + manifest; live = `stream.sock` UDS; command = UDS frame + cap/dedup
  tokens + `inbox/` fallback + `outbox/` ack), `LocalSupervisor` (detached
  `cp --headless`, adopt via registry).
- **One transport-agnostic, versioned wire protocol** (`Command` / `StateDelta` /
  `StreamFrame` / `AgentState`). The medium is swappable (UDS → TCP/QUIC remote, or
  shared-memory ring for lower local latency) **without touching orchestration
  logic**.

---

## 7. Live streaming path (§9 requirement)

The hot path that must be fluid. Flow and latency budget:

```
LLM provider ──SSE──▶ agent (existing)              : network (unavoidable)
agent StreamEvent ──SPSC enqueue──▶ publisher thread : ~nanoseconds (lock-free, hot loop)
publisher thread ──serialize+write──▶ stream.sock    : ~microseconds (mem + kernel UDS)
backend recv ──fan-out──▶ frontend WS                : ~microseconds (in-mem) + localhost WS flush
frontend ──rAF batch──▶ DOM                          : next animation frame
```

**The agent tee (adversarial #5).** The agent already receives
`StreamEvent::Chunk` deltas and renders them. The bridge adds a **lock-free SPSC
enqueue** at that exact point — **one atomic push, no serialization on the hot
loop**. A **dedicated publisher thread** drains the ring, serializes `StreamFrame`s
and writes the socket. The agent renders/persists **identically**; the tee can
never steal CPU from or backpressure the main loop (I7). This is the only new
hot-path code and it is a single enqueue.

**Frame schema** (small, ordered, attributable):
`StreamFrame { agent_id, worker_id, thread_id, message_id, seq, kind, payload }`
with `kind ∈ { MessageStart, Token, ToolArgs }`. **Phase is NOT here** — it is a
durable control-plane delta (adversarial #6). The **first** frame per `message_id`
is `MessageStart` (I10), self-describing so the frontend can create the buffer
before the control delta lands.

**Fan-out (StreamHub).** One UDS consumer per agent → N frontend WS subscribers.
The agent never scales connections (good for #7). Fan-out is O(subscribers) direct
writes.

**Backpressure (I7, adversarial #11).**
- *Agent → backend:* non-blocking publisher; drop/coalesce frames if the UDS send
  buffer is full.
- *Backend → frontend:* each WS connection has a **bounded** buffer; on overflow,
  coalesce pending token deltas (or drop) **and set a `degraded` flag** on that
  stream. The flag is pushed to the UI ("stream degraded — catching up") and the
  backend emits a control-plane "reconcile from final message" hint. A slow browser
  never stalls the agent or other viewers, and degradation is **never silent**.

**Frontend rendering contract (adversarial #10).** The browser is the real latency
floor. Mandatory frontend rule (documented here because it determines the
end-to-end UX): incoming tokens accumulate into a per-message buffer and are
flushed to the DOM **once per `requestAnimationFrame`** — **never** `setState`
per token. This is a first-class requirement of "fluid," not an implementation
detail.

**Latency hygiene.** `TCP_NODELAY` on any TCP hop, no output buffering, **flush
per frame**, never debounce tokens, never route tokens through disk.

**Crash mid-stream.** Agent dies → `stream.sock` closes → backend marks the
stream ended **on the durable control plane** (phase → `down`/`interrupted`, so
the UI can never get stuck). Partial live text is ephemeral. *(Optional knob:
checkpoint partial assistant text to disk every N tokens — default off in v1.)*

**Don't firehose blobs.** Only small deltas on the stream plane. Large tool
outputs / panels are referenced by `rev`; the frontend fetches them via REST.

---

## 8. Backend ↔ Agent (control plane)

### 8.1 Read — disk + manifest (safe by I2/I3, reliable by event+poll)
Watch `.context-pilot/`, gate on the manifest, incrementally read changed files.
Event-driven watcher for latency **+** a 2–3 s poll-reconcile sweep for
correctness (`notify` drops/coalesces under load — the search indexer already uses
this belt-and-suspenders). A human TUI can be open while the backend reads (I1
single writer).

### 8.2 Write — command, UDS-fast with durable fallback
- **Authn (I9):** every command carries the agent's `cap_token`. Mismatch ⇒
  rejected, logged.
- **Idempotency (I4):** every command carries a client-supplied **`dedup_token`**.
  The `seen` ledger keys on it, so retries/reissues never double-execute even with
  a fresh transport id.
- **Socket up (normal):** UDS frame → agent applies it in its loop (same path as a
  typed user message; queued safely if mid-stream) → acks over UDS. Fast.
- **Socket down / busy-booting:** drop a durable `inbox/<seq>-<id>.json` (atomic,
  carries cap+dedup tokens). Processed on reconnect/boot.
- **Atomicity (I8):** the command's effect (message + spine notification) + its
  `seen` mark + the `rev`/manifest bump commit in **one transaction**; observable
  via `outbox/<id>` and the resulting `rev`.
- **Lifecycle states**, pushed as **control-plane deltas** so the UI shows real
  progress (adversarial #12): `queued → delivered → processing → done | failed |
  expired`. A TTL bounds the wait; on expiry the backend marks `expired` and
  reissues with a **new transport id but the SAME `dedup_token`** (so the reissue
  can never become a semantic duplicate). The `seen`-window is kept longer than the
  TTL.
- **Two-phase semantics:** "send message" **acks on acceptance** (fast); the LLM
  work is observed later via the stream + state planes. Mutations like "archive
  thread" ack on completion. Never block an ack for minutes.

---

## 9. Frontend ↔ Backend

- **REST** — initial load + point queries + non-streaming actions. Every response
  carries `rev`. Actions return a `command id` + echo the `dedup_token`.
- **WebSocket** — the single live channel, **authenticated** (adversarial #3): the
  backend mints a per-session **bearer token** delivered to the frontend
  out-of-band (the backend serves the frontend and injects it, or prints it for
  paste). The WS **handshake requires the token** (via `Sec-WebSocket-Protocol` or
  a mandatory first auth frame); connections without it are rejected. **CORS /
  `Origin` are NOT relied upon** — they don't protect WS. The channel carries:
  - *control deltas* — `seq`-numbered, **replayable** (state, new messages, phase,
    MY_TURN, cost, command-lifecycle).
  - *token/stream frames* — ephemeral, **not** replayed (final message covers any
    gap).
  Full-duplex → instant `stop`/`interrupt`/`answer`. Binary frames keep token
  overhead minimal.
- **Reconnect:** control plane replays the gap by `seq` (bounded ring); gap >
  buffer ⇒ `resync` → REST refetch. Stream plane: missed tokens dropped; the final
  message is already in state.
- **Backend-down resilience (adversarial #14):** the frontend **queues user
  actions client-side** while the backend is unreachable and **replays them on
  reconnect**. Replay is safe because each action carries its `dedup_token` (I4) —
  a replayed-but-already-applied action is deduped. In-flight intent is therefore
  **not** silently lost during a backend blip. *(Future: a thin local helper could
  write `inbox/` directly when co-located; the browser itself cannot.)*
- **Client monotonic rev:** ignore any WS frame or REST response with `rev ≤` the
  applied rev (defeats event-before-snapshot races).

---

## 10. Discovery, heartbeat & single-instance

- On boot: take the **folder flock** (I1, FD-inheritable — see H5), bind
  `stream.sock`, mint `cap_token`, then register `~/.context-pilot/agents/<id>.json`
  (`0600`) = `{ id, folder, pid, boot_id, model, protocol_version, binary_version,
  socket_path, heartbeat_path, cap_token, started_at, status }` (atomic). The
  registry entry is written **rarely** (boot + status change), **not** per
  heartbeat.
- **Liveness — decoupled from the registry (adversarial #16).** Two signals, neither
  of which churns the manifest/registry rename path: (1) the **UDS being connected**
  (primary, instant), and (2) a **dedicated `heartbeat` file the backend POLLS** (not
  watches) at its own cadence, refreshed by the agent's dedicated heartbeat thread.
  No mtime dependence (I3-safe), no watcher storm.
- **Heartbeat thread.** Dedicated thread, never the main loop — a busy agent (long
  tool, big stream) must still look alive. Reuses the deadman dedicated-thread
  pattern.
- **Liveness verdict:** fresh polled heartbeat **AND** live pid **AND** matching
  `boot_id`/start-time (defeats pid reuse). Else stale → down.
- **Spawn = try-lock-or-adopt:** a live registry entry / held flock ⇒ adopt; else
  launch; the launched process's flock arbitrates a race, loser exits, backend
  adopts the winner. **Spawn is allow-list gated** (§17).
- **GC:** registry `*.tmp` reaped by age; stale `stream.sock` unlinked before
  re-binding on boot.
- **Unmanaged agents:** live lock, no registry entry (bridge off / old binary) →
  listed read-only via disk; no command/stream channel.

---

## 11. Agent-side delta (the entire footprint)

### 11.1 Additive module — `cp-mod-bridge` (behaviorally inert)
1. **Lock + register + heartbeat** (heartbeat + polled file on a dedicated thread).
2. **Stream tee** — **lock-free SPSC enqueue** of each `StreamEvent`; a dedicated
   publisher thread serializes + writes `stream.sock` (I7). Pure observer, zero hot-
   loop serialization.
3. **Command intake** — verify `cap_token` + `dedup_token` (I9/I4) on UDS frames +
   `inbox/` files → spine notifications (existing user-message path) → ack via
   `outbox/`.

Disable the bridge ⇒ the agent runs **exactly** as today.

### 11.2 Required hardening to the existing persistence path (behavior-preserving)
- **H1 (I2):** atomic, power-safe writes — `fsync(file) → rename`, and per batch
  **fsync all data files → rename manifest → fsync directory**.
- **H2 (I3/I8):** stamp `rev` + write the `manifest` (hashes of **all** batch files,
  *including messages*) **last**.
- **H3 (I8):** collapse the dual `Batch`/`Message` writer channels into **one commit
  transaction** for state + command-effect writes (so messages participate in the
  rev; no escape path).
- **H4 (I9):** mint + persist the `cap_token`; verify it on every command.
- **H5 (I1 × deadman re-exec).** The deadman watchdog re-execs the process
  (`CommandExt::exec`). To avoid an unlocked window or a self-deadlock, the agent
  **clears `FD_CLOEXEC` on `agent.lock`** and passes `CP_AGENT_LOCK_FD=<n>` across
  the exec; the re-exec'd image detects the env and **adopts the inherited lock**
  (no re-lock). Absent the env (fresh launch) ⇒ normal `flock`.

> Entire impact: one additive inert module + five behavior-preserving robustness
> upgrades. Nothing changes *how the agent reasons or acts*. The tee is a single
> lock-free enqueue.

---

## 12. Identity & multi-worker

- **Stable id:** FNV-1a of the canonical folder path (reuses search's scheme) →
  stable across restarts. Folder move/rename ⇒ new id + old tombstone (registry
  stores the canonical path so a future "rebind" is possible).
- **Multi-worker:** an agent may run N internal workers. The frontend speaks
  **threads**; the backend flattens threads across workers, carrying `worker_id` as
  metadata (and in every `StreamFrame`). **The single persistence actor assigns
  `rev` and snapshots all workers atomically per batch** (I8) — so the per-agent
  `rev` is well-defined despite concurrent workers.

---

## 13. Failure modes & recovery (summary; full register in §20)

| Actor | Failure | Detection | Recovery |
|---|---|---|---|
| Agent | Hard crash | stale heartbeat + dead pid; `stream.sock` closes | mark down (phase on control plane); last snapshot readable; offer restart |
| Agent | Mid-batch crash | manifest points at last committed rev | resume from last committed snapshot (commit txn I8) |
| Agent | Double-launch | flock contention (I1) | 2nd passive; truth uncorrupted |
| Agent | Deadman re-exec | inherited lock FD (H5) | no unlocked window; same single writer |
| Agent | Re-run command post-crash | `seen` ledger keyed on `dedup_token` (I4) + I8 | duplicate skipped; exactly-once effect |
| Stream | Slow frontend / backend | bounded buffers | coalesce/drop + **degraded flag** (I7); agent unaffected |
| Stream | Agent dies mid-stream | socket close | phase→interrupted on control plane; final message via state |
| Backend | Crash / restart | n/a | **lazy** rebuild: registry + rev heads eager, bodies on demand (I5); re-adopt detached agents; re-open UDS; frontends reconnect + replay queued actions |
| Transport | `notify` event dropped | poll-reconcile sweep | converges within sweep |
| Transport | Torn / power-lost write | atomic rename + dir fsync (I2) | prior committed version |
| Frontend | WS disconnect | reconnect + `seq` replay / `resync` | gap replay or REST refetch; client action queue replays |
| Security | Forged command | `cap_token` mismatch (I9) | rejected + logged |
| Protocol | Version skew | `protocol_version` + per-msg `schema_version` | tolerant decode within **N-1 major** (§18) |
| Process | pid reuse | `boot_id`/start-time mismatch | treated as down |
| Fleet | Runaway spend | `CostBreaker` aggregate ceiling (§17) | stop issuing commands/spawns; surface |

Backend is **supervised** (systemd/launchd); spawned agents are **detached**
(`setsid`) so they survive a backend restart and are re-adopted.

---

## 14. Sequence diagrams

**Live streaming token (the fluid path):**
```
provider →(SSE)→ agent: StreamEvent::Chunk("Hel")
agent: render to typewriter  AND  SPSC enqueue (hot loop, one atomic push)
publisher thread: serialize → stream.sock {MessageStart once, then Token seq, "Hel"}
backend: recv frame → fan-out to N WS (bounded; degraded flag on overflow)
frontend: route by (thread,message); rAF-batch append → paint next frame
… repeat per token, sub-ms added latency …
agent: StreamDone → commit txn: persist final message + manifest rev++ (I8)
backend: control delta {message finalized@rev, phase→idle} → WS (authoritative)
frontend: reconcile final text (covers any dropped live token)
```

**Send-message (auth'd, idempotent, two-phase ack):**
```
UI →(REST/WS auth'd) POST message {text, dedup_token}
backend: assign cmd-id+seq; UDS frame {cap_token, dedup_token} (or inbox/ if socket down)
agent: verify cap_token; dedup on dedup_token; → backend: accepted {cmd-id}  (fast)
agent: spine notification (queued if streaming); effect+seen+rev in one txn (I8)
agent: outbox/<cmd-id> {ok, rev_after}; control delta {lifecycle: processing→done}; then streams (→ live path)
```

**Backend restart recovery (lazy):**
```
scan registry → verify liveness (heartbeat+pid+boot_id) → adopt live / tombstone dead
per live agent: open AgentChannel → read rev/manifest HEAD only (bodies lazy) → reconnect stream.sock
scan outbox/ for unacked → reconcile in-flight commands (dedup-safe)
accept frontend WS (auth'd; resync) → UI refetches + replays its queued actions
```

---

## 15. Information to gather FROM agents

Identity/lifecycle; **phase/status** (idle·streaming·tooling·blocked·needs-input·
errored·down — **durable control plane**); threads (id, name, MY_TURN/ACTIVE/
THEIR_TURN, unread, preview, full conversation on demand, pending questions);
conversation messages + **live streaming deltas** (stream plane); command
lifecycle state; economics (tokens/cost per agent+thread, cache hit/miss/output,
context budget); every context panel (todos, memories, logs, entities, spine,
queue, scratchpad, tools, callbacks, tree, radar); fleet-level MY_TURN signals +
total spend + current `rev` + degraded-stream flags.

## 16. Actions to perform ON agents

Send-to-thread (primary driver); thread create/archive/restore/answer-question;
lifecycle spawn (**allow-list gated**)/stop/restart/pause/**interrupt-stream**
(instant via WS→UDS); manage rename/model/archive; toggles
(auto-continuation/reverie/think); thread-scoped coucou. All as **cap-token-auth'd,
dedup-token-idempotent**, ack'd commands (§8.2).

---

## 17. Security & permissions

- **Command authn (I9):** per-agent `cap_token` (256-bit, minted at boot, in the
  `0600` registry entry), required on **every** UDS frame and `inbox/` file. The
  backend is the sole practical command author. *(Same-user malware that reads the
  `0700` registry can still forge — true defense needs OS sandboxing, future.)*
- **Frontend WS (adversarial #3):** localhost-bind **plus** a per-session bearer
  token required in the WS handshake. **No reliance on CORS/`Origin`.**
- **Spawn blast radius (adversarial #18):** the supervisor refuses any folder **not
  on a configured allow-list** — a compromised/buggy backend cannot spawn agents in
  arbitrary paths. Spawned agents inherit the user's keys and run user tools (RCE
  blast radius is inherent to running agents); the allow-list + cost breaker bound
  it; sandboxing is future.
- **Global cost circuit-breaker (`CostBreaker`):** the backend tracks **aggregate
  fleet spend**; past a configured ceiling it **stops issuing commands and
  spawns** and surfaces the trip. Per-worker guard rails do **not** bound a backend
  issuing commands in a loop — this does.
- **Permissions:** `0700` on `.context-pilot/`, registry dir, and `stream.sock`;
  `0600` on the registry entry (holds the cap token).
- **Secrets:** the registry holds **no** API keys; secrets stay in the agent's env/
  config. The `cap_token` is a capability, not a credential to an external service.
- **Future (network/multi-tenant):** auth (bearer/mTLS), signed commands at the
  transport seam — designed-in via the abstraction, no orchestration changes.

## 18. Versioning & compatibility (adversarial #17)

`schema_version` on every file/frame; serde tolerant decode (ignore unknown,
default missing); registry advertises `protocol_version` + supported commands.
**Compatibility window: the backend supports the current AND previous major
(N-1)**; it rejects only majors older than N-1 or newer-than-known, with an
explicit error. This makes the rolling binary upgrade the design explicitly
supports (long-lived agents, upgrade-in-place) **safe** — a new backend never
hard-orphans a one-major-old live agent.

## 19. Observability & ops (production necessity)

The backend exports: per-agent **stream latency p50/p99**, **dropped/coalesced
frame counts + degraded-stream events** (adversarial #11), command queue depth +
lifecycle-state histogram, **rev lag** (how far the view trails the agent),
heartbeat freshness, WS subscriber counts, reconnect/resync rates, **CostBreaker
state + aggregate spend**, rejected-command (auth-fail) counts. Structured logs
with `agent_id` + `cmd_id` correlation. Without this, "fluid as fuck" is
unmeasurable and regressions ship silently.

---

## 20. Problem Register (track ALL problems)

Severity: 🔴 critical · 🟠 high · 🟡 medium. Status: ✅ designed · 🔵 knob/optional.

### A. Streaming / latency
| | Problem | Sev | Mitigation | St |
|---|---|---|---|---|
|A1|Disk-tailing too slow for tokens|🔴|Two-plane split: UDS live plane (§3/§7)|✅|
|A2|Nagle / output buffering adds latency|🟠|`TCP_NODELAY`, no buffering, flush per frame|✅|
|A3|Fan-out O(N) scan per token|🟠|Per-agent subscriber list, O(subs) direct writes|✅|
|A4|Slow frontend backpressures chain|🔴|Bounded per-WS buffer, coalesce/drop + degraded flag (I7)|✅|
|A5|Slow backend stalls agent via tee|🔴|Lock-free SPSC enqueue + dedicated publisher thread (I7)|✅|
|A6|Token reorder / wrong-thread / pre-message|🔴|MessageStart self-describing + orphan buffering + seq (I10)|✅|
|A7|Background-tab throttling|🟡|Frontend coalesces; refetch final on focus|✅|
|A8|Thundering herd (many agents stream)|🟡|Async runtime, one task per UDS, cheap fan-out|✅|
|A9|Control msg stalls token flow (HoL)|🟠|Separate frame types / logical streams|✅|
|A10|Per-token serialize steals hot-loop CPU|🟠|Tee = one SPSC enqueue; publisher thread serializes (I7)|✅|
|A11|Coalescing = silent UX regression|🟡|`degraded` flag surfaced + reconcile-from-final hint|✅|
|A12|Browser setState-per-token janks|🟠|Mandatory rAF token-batching contract (§7)|✅|

### B. Consistency / state
| | Problem | Sev | Mitigation | St |
|---|---|---|---|---|
|B1|Torn file reads|🔴|Atomic rename (I2)|✅|
|B2|Cross-file inconsistency|🔴|Manifest commit marker (I3)|✅|
|B3|Messages on non-batched path escape rev|🟠|Single commit txn; manifest enumerates messages (I8)|✅|
|B4|Delete vs not-yet-written ambiguity|🟠|Interpret refs per-rev manifest only|✅|
|B5|mtime granularity / clock steps|🟠|Manifest hashes, no clock dependence (I3)|✅|
|B6|Read amplification on big agents|🟡|Manifest per-file hashes → incremental re-read (I3)|✅|
|B7|Power-loss after rename, content in page cache|🟡|fsync data → rename manifest → **fsync dir** (I2)|✅|
|B8|Multi-worker rev race|🟡|Single persistence actor assigns rev + atomic snapshot (I8)|✅|

### C. Command delivery
| | Problem | Sev | Mitigation | St |
|---|---|---|---|---|
|C1|Double-execution on crash|🔴|`seen` ledger (dedup_token) + atomic effect+mark (I4/I8)|✅|
|C2|Ordering|🟠|Sortable seq|✅|
|C3|Lost command|🟠|Durable inbox fallback + outbox ack + retry by dedup_token|✅|
|C4|Command to a down agent|🟠|Liveness check; durable queue; lifecycle + TTL surfaced|✅|
|C5|TTL-reissue creates a semantic duplicate|🟠|Reissue keeps SAME `dedup_token`; seen keys on it (I4)|✅|
|C6|Ack semantics (accept vs done) / hidden delay|🟡|Lifecycle states pushed as control deltas to the UI|✅|
|C7|Ack lost (crash after effect)|🟡|Re-derive from rev / idempotent reissue|✅|
|C8|Forged / unauthenticated command|🔴|`cap_token` required + verified on every command (I9)|✅|

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
|D10|flock lost/double across deadman re-exec|🟠|Inherit lock FD (clear CLOEXEC, pass fd) (H5)|✅|
|D11|Heartbeat rename storms the watcher (vs I3)|🟡|Liveness = UDS + POLLED heartbeat file, not watched rename|✅|

### E. Backend robustness
| | Problem | Sev | Mitigation | St |
|---|---|---|---|---|
|E1|Backend SPOF|🟠|Supervised + near-stateless rebuild (I5)|✅|
|E2|Restart loses in-flight commands|🟠|Durable inbox/outbox reconcile (dedup-safe)|✅|
|E3|Memory growth (views/buffers)|🟠|Bounded ring buffers, lazy materialization|✅|
|E4|Backend↔frontend version skew|🟡|schema_version + capability negotiation|✅|
|E5|Eager cold rebuild scales with fleet disk|🟠|**Lazy materialization in v1** (I5)|✅|
|E6|Backend-down = action blackout|🟡|Client action queue + replay on reconnect (dedup-safe)|✅|

### F. Frontend consistency
| | Problem | Sev | Mitigation | St |
|---|---|---|---|---|
|F1|Event before snapshot|🟠|Client monotonic rev; ignore ≤ applied|✅|
|F2|Reconnect after sleep|🟠|seq replay / resync→refetch; drop missed tokens|✅|
|F3|Optimistic UI hangs|🟡|Command lifecycle + TTL surfaced|✅|
|F4|Duplicate/out-of-order/orphan tokens|🟡|seq gap → final message; MessageStart + orphan buffer (I10)|✅|

### G. Security
| | Problem | Sev | Mitigation | St |
|---|---|---|---|---|
|G1|Command injection via inbox/UDS|🔴|`cap_token` auth on every command (I9) + 0600 entry|✅|
|G2|WS unauthenticated (CORS ≠ WS auth)|🔴|Bearer token in WS handshake; no Origin reliance (§9/§17)|✅|
|G3|Spawn RCE amplifier|🟠|Spawn allow-list (§17)|✅|
|G4|Runaway fleet spend|🟠|Global CostBreaker circuit-breaker (§17)|✅|
|G5|Network/multi-tenant future|🟡|Auth/mTLS/signed cmds at transport seam|🔵|
|G6|Secrets leak via registry/disk|🟠|Registry holds no API keys; cap_token ≠ external credential|✅|

### H. Ops / versioning / observability
| | Problem | Sev | Mitigation | St |
|---|---|---|---|---|
|H1|Binary upgrade w/ live agents (hard-reject)|🟠|**N-1 major** compatibility window (§18)|✅|
|H2|No observability|🟠|Metrics + correlated logs + degraded/cost-breaker events (§19)|✅|
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
|I6|Phase (sticky) dropped on lossy plane|🟠|Phase → durable control plane (§3/§6)|✅|

---

## 21. Open questions (genuine choices)

1. **Stream-plane transport confirm:** UDS for v1 (my lean) — agree? (Future:
   shared-memory ring, same abstraction.)
2. **Partial-message checkpointing (I3-stream):** off in v1 (my lean) or checkpoint
   partial assistant text every N tokens for crash survival?
3. **Multi-worker exposure:** flatten to threads (my lean) vs surface worker lanes?
4. **Reaping:** tombstone `down` agents (my lean) vs auto-reap?
5. **Backend stack = Rust** (reuse serializable state/IR types verbatim) — confirm?
6. **Spawn allow-list source:** explicit config file (my lean) vs a "register this
   folder" UI gesture vs both?

*(Resolved since v3: frontend live channel = one authenticated WebSocket; command
auth = capability token; idempotency = semantic dedup token; cold rebuild =
lazy.)*

## 22. Decision log

Each entry: date · question · ruling · rationale.

- **2026-06-16 · Two-plane architecture** · *Provisional:* durable control plane
  (disk) + ephemeral stream plane (UDS) · fluid streaming *and* crash-proof state
  without compromise.
- **2026-06-16 · Phase on which plane** · *Locked (v4):* phase/status on the
  **durable** plane, never the lossy stream · sticky state on a droppable channel
  strands the UI (adversarial #6).
- **2026-06-16 · Stream tee mechanism** · *Locked (v4):* lock-free SPSC enqueue +
  dedicated publisher thread · zero hot-loop serialization / CPU steal
  (adversarial #5).
- **2026-06-16 · Cross-plane ordering** · *Locked (v4):* self-describing
  `MessageStart` + frontend orphan-buffering (I10) · tokens can precede the control
  delta (adversarial #4).
- **2026-06-16 · Commit atomicity** · *Locked (v4):* single-writer commit
  transaction; manifest enumerates messages (I8) · closes the dual-channel escape
  (adversarial #1) + multi-worker rev (adversarial #15).
- **2026-06-16 · Command auth** · *Locked (v4):* per-agent capability token on
  every command (I9) · perms-only is not authentication (adversarial #2).
- **2026-06-16 · Frontend WS auth** · *Locked (v4):* bearer token in the WS
  handshake; no CORS reliance · CORS doesn't protect WS (adversarial #3).
- **2026-06-16 · Idempotency** · *Locked (v4):* client-supplied semantic
  `dedup_token`; reissue keeps the same token (I4) · TTL-reissue otherwise
  double-executes (adversarial #13).
- **2026-06-16 · Cold rebuild** · *Locked (v4):* lazy materialization in v1 (I5) ·
  eager rebuild scales with fleet disk (adversarial #9).
- **2026-06-16 · Power-loss durability** · *Locked (v4):* data fsync → manifest
  rename → dir fsync (I2) · a renamed manifest can else reference lost content
  (adversarial #8).
- **2026-06-16 · flock × deadman re-exec** · *Locked (v4):* inherit the lock FD
  across exec (H5) · avoids unlocked window / self-deadlock (adversarial #7).
- **2026-06-16 · Heartbeat mechanism** · *Locked (v4):* UDS + polled heartbeat
  file, decoupled from registry rename · avoids watcher storm vs I3 mtime ban
  (adversarial #16).
- **2026-06-16 · Version compatibility** · *Locked (v4):* N-1 major window (§18) ·
  resolves the tolerant-decode-vs-hard-reject contradiction (adversarial #17).
- **2026-06-16 · Spawn + cost safety** · *Locked (v4):* spawn allow-list + global
  CostBreaker (§17) · bounds RCE blast radius + runaway spend (adversarial #18).
- **2026-06-16 · Read transport** · *Provisional:* disk + manifest, incremental.
- **2026-06-16 · Command transport** · *Provisional:* UDS-fast + durable inbox
  fallback.
- **2026-06-16 · Discovery** · *Provisional:* registry dir + heartbeat + flock.
- **2026-06-16 · Identity** · *Provisional:* FNV-1a of canonical folder path.
- **2026-06-16 · Backend stack** · *Provisional:* Rust.
- **2026-06-16 · Invariants** · *Locked candidates:* I1–I10 — the non-negotiable
  robustness + security spine.

_(Promote "Provisional" → "Locked" as the captain confirms.)_

---

## 23. Adversarial review resolution

Direct mapping of every issue in
[`architecture-adversarial-analysis.md`](./architecture-adversarial-analysis.md)
to its v4 resolution. **All 18 accepted as relevant; none dismissed.**

| # | Issue (sev) | Resolution in v4 | Where |
|---|---|---|---|
| 1 | Effect+seen+rev not one transaction 🔴 | **I8** single-writer commit transaction; manifest enumerates messages; dual Batch/Message channel collapsed | I8, H2/H3, §8.2, B3 |
| 2 | Command auth = perms only 🔴 | **I9** per-agent capability token, required + verified on every command | I9, §8.2, §17, G1 |
| 3 | WS auth = localhost+CORS 🔴 | Per-session **bearer token in the WS handshake**; CORS not relied upon | §9, §17, G2 |
| 4 | Cross-plane token/message ordering 🔴 | **I10** self-describing `MessageStart` + frontend orphan buffering | I10, §7, A6/F4 |
| 5 | Tee CPU steal on hot loop 🟠 | Lock-free **SPSC enqueue** + dedicated publisher thread | I7, §7, §11.1, A10 |
| 6 | Phase (sticky) on lossy plane 🟠 | Phase moved to the **durable control plane** | §3, §6, §15, I6-row |
| 7 | flock × deadman re-exec 🟠 | **H5** inherit lock FD (clear CLOEXEC, pass fd) across exec | H5, §10, D10 |
| 8 | Power-loss: no dir fsync 🟡 | **I2** full barrier: data fsync → manifest rename → **dir fsync** | I2, H1, B7 |
| 9 | Eager cold rebuild cost 🟡 | **I5** lazy materialization in v1 (heads eager, bodies on demand) | I5, §13, E5 |
| 10 | Browser render floor out of scope 🟠 | Mandatory **rAF token-batching** frontend contract | §7, A12 |
| 11 | Coalescing = silent UX regression 🟡 | **`degraded` flag** surfaced + reconcile hint + metric | §7, §19, A11 |
| 12 | "Accepted" hides effect delay 🟡 | Command **lifecycle states pushed as control deltas** to the UI | §8.2, §9, C6 |
| 13 | No semantic idempotency 🟠 | **I4** client-supplied `dedup_token`; reissue keeps same token | I4, §8.2, C5 |
| 14 | Backend-down action blackout 🟡 | **Client action queue + replay on reconnect** (dedup-safe) | §9, E6 |
| 15 | Multi-worker rev serialization 🟡 | **I8** single persistence actor assigns rev + atomic cross-worker snapshot | I8, §12, B8 |
| 16 | Heartbeat vs I3 (mtime ban) 🟡 | Liveness = **UDS + polled heartbeat file**, decoupled from registry rename | §10, D11 |
| 17 | Upgrade hard-reject vs tolerant 🟠 | **N-1 major** compatibility window | §18, H1 |
| 18 | Spawn RCE + no cost breaker 🟠 | **Spawn allow-list + global CostBreaker** circuit-breaker | §17, G3/G4 |

**Residual honesty.** Two limits are inherent, not hand-waved: (a) `cap_token`
(I9) defends against *blind* same-user injection but not against malware that
reads the `0700` registry — true defense needs OS sandboxing (future); (b) a
spawned agent runs the user's tools, so the RCE blast radius is intrinsic to
*running agents at all* — the allow-list + CostBreaker **bound** it; sandboxing
is future. Both are flagged 🔵 where future work, ✅ where bounded today.
