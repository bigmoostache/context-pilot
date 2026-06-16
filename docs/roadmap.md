# Orchestration Backend — Implementation Roadmap

> **Source of truth:** [`design-orchestration-backend.md`](./design-orchestration-backend.md) (v5).
> This roadmap turns that design into a buildable plan. It is organized as
> **Goals → Tasks → Subtasks**. Every **Task** carries a single
> **Objective (testable)** — a concrete, assertable acceptance criterion that
> proves the task is *done*, not just *written*.
>
> **Traceability.** Tasks reference the design's invariants (`I1`–`I12`),
> killers (`K1`–`K9`), round-2 issues (`R2-*`), and fault-injection rows
> (`V1`–`V10`, design §25). The §25 matrix **is** the production-v1 acceptance
> gate; its rows are embedded as task objectives and re-run as a whole in Phase 11.
>
> **Convention.** Task IDs are `P{phase}-T{n}`. Subtasks are checkboxes.
> "Additive" / "core" change types follow design §24 — **the shared
> `PersistenceWriter` and the autonomy spine are never modified.**

---

## Milestones (critical path)

| ID | Milestone | Definition of reached |
|----|-----------|-----------------------|
| **M0** | Walking skeleton | Wire protocol + oplog library exist and pass unit/fault tests in isolation (Phases 0–1). |
| **M1** | One agent streams live | A single agent with the bridge tees tokens to a UDS; a trivial consumer renders them sub-ms (Phases 2–3). |
| **M2** | Commands round-trip | Backend sends an HMAC'd command; agent applies it exactly-once and survives a deadman re-exec (Phase 4). |
| **M3** | Fleet visible | Standalone backend discovers N agents, tails oplogs, hydrates bodies, fans out streams to a browser (Phases 5–7). |
| **M4** | Hardened v1 | Full V1–V10 fault-injection matrix passes; security + observability gates green (Phases 8–11). |

**Dependency order:** P0 → P1 → {P2 → P3, P2 → P4} → P5 → P6 → P7 → P8 → {P9, P10} → P11.
P1 (oplog) is the keystone; nothing durable proceeds until it passes its fault tests.

---

## Phase 0 — Foundations, decisions & the wire protocol

**Goal.** Lock the open design choices and build the versioned wire protocol both
sides depend on. Nothing else can compile against a moving seam.

### P0-T1 — Lock the §21 open questions
- [ ] Q1 oplog record format → **decide framed + per-record CRC** (torn-tail detection) vs JSON-lines.
- [ ] Q2 compaction trigger → **decide ack-rev barrier + size threshold (both)**.
- [ ] Q3 stream transport → **confirm UDS v1**, shared-memory ring deferred behind the same seam.
- [ ] Q4 partial-message checkpoint cadence (degraded stream) → pick `N` tokens / `M` ms.
- [ ] Q5 backend stack → **confirm Rust** (reuse `cp-base`/`cp-render` serializable types).
- [ ] Q6 spawn allow-list source → config file vs "register folder" UI vs both.
- **Objective (testable):** `docs/roadmap-decisions.md` records a ruling for all six Qs; each downstream task referencing a Q cites the locked value. CI lint: no task may start while its gating Q is `UNDECIDED`.

### P0-T2 — Create the `cp-wire` protocol crate
- [ ] New crate `crates/cp-wire`: `Command`, `OpEntry`, `StreamFrame`, `Heads`, `Body`, `Ack`, `RegistryEntry`.
- [ ] Every type carries `schema_version`; `serde` tolerant decode (ignore unknown fields).
- [ ] `protocol_version` constant + N-1 compatibility helper (`accepts(major)`).
- [ ] Round-trip serde tests for every type, incl. a recorded N-1 fixture.
- **Objective (testable):** `cargo test -p cp-wire` is green; a fixture encoded at version `N-1` decodes without error under version `N`, and an `N+1`-major frame is rejected with a typed error (not a panic).

### P0-T3 — Scaffold the two new crates
- [ ] `crates/cp-mod-bridge` (agent-side, additive module) — empty `Module` impl, registered **after** existing modules, behaviorally inert.
- [ ] `crates/cp-orchestrator` (standalone backend binary) — boots, parses config, exits clean.
- [ ] Workspace `Cargo.toml` members updated; folder-size + file-length callbacks green.
- **Objective (testable):** `cargo build --workspace` succeeds; running the agent with `cp-mod-bridge` active produces byte-identical trajectory/state to running without it on a fixed scripted session (proves "behaviorally inert," constraint #7).

### P0-T4 — Spike: `flock` × deadman re-exec FD inheritance (H5, core-adjacent)
- [ ] Prototype: acquire `flock` on `agent.lock`, clear `FD_CLOEXEC`, pass `CP_AGENT_LOCK_FD`, re-exec via `CommandExt::exec`.
- [ ] Confirm the lock is *held continuously* across the re-exec (no double-writer window, no self-deadlock).
- **Objective (testable):** an automated test re-execs a child holding the lock; a concurrent second acquirer is blocked **before, during, and after** the re-exec (zero-width window verified by timing log).

---

## Phase 1 — The oplog (tier ①): the keystone

**Goal.** An append-only, `fsync`'d, gap-free, replay-safe operation log as a
standalone library. This is the durability heart; it must be perfect before
anything reads it.

### P1-T1 — Append + fsync + framing
- [ ] `OplogWriter::append(entry) -> rev`: framed record (len + payload + CRC), `write → fsync(file) → fsync(dir)`.
- [ ] `rev` = monotonic append offset (the fsync'd position **is** the rev — announce-after-durable, K9).
- **Objective (testable):** **V1** — `kill -9` injected between `write` and `fsync` (fault hook); on reopen+replay the torn tail record is discarded via CRC mismatch and no half-effect is observed. Property test: `rev` strictly increases, never reused.

### P1-T2 — Replay + torn-tail recovery
- [ ] `OplogReader::replay() -> (rev_head, Heads, SeenSet)` — stops at first CRC failure (torn tail), reports clean head.
- [ ] Bodies are content-addressed (filename = content hash); replay rebuilds **heads** (per-thread/per-panel last hash), not full history (I3).
- **Objective (testable):** **V4** — simulate power-cut (drop un-fsync'd dir entry); a committed `rev` is fully readable, an uncommitted one is entirely absent. Replay of a 100k-entry log rebuilds heads in O(entries) with bounded memory (heads set, not full enumeration).

### P1-T3 — Dedup `seen`-set, evicted by ack-rev
- [ ] `seen` keyed by `dedup_token`; `mark_seen(token, rev)`; `is_seen(token)`.
- [ ] Eviction **only** past the acknowledged-`rev` barrier (not time, R2-1/I4).
- **Objective (testable):** **V3** — replay the same `dedup_token` after a simulated 2-hour gap (no time-based expiry); the second apply is a no-op. A token is evicted *only* after its `rev` is confirmed ack'd.

### P1-T4 — Compaction
- [ ] `compact()` past the ack-rev barrier + size threshold (P0-T1 Q2); preserves `seen` semantics for un-acked tokens.
- **Objective (testable):** after compaction, replay yields an identical `(rev_head, Heads, SeenSet)` to pre-compaction; log file size drops below threshold; no un-acked token is lost.

---

## Phase 2 — Agent-side bridge: boot, lock, registry, heartbeat (additive)

**Goal.** The agent exposes itself to the fleet with the minimal additive
footprint of §24. **`writer.rs` is not touched.**

### P2-T1 — Boot sequence
- [ ] On boot: take folder `flock` (FD-inheritable, P0-T4), bind `stream.sock` (unlink stale first), open/create oplog, mint 256-bit `cap_token`.
- [ ] Write registry entry `~/.context-pilot/agents/<id>.json` (`0600`, atomic tmp+rename) with full schema (id, folder, pid, boot_id, model, versions, paths, cap_token, started_at, status).
- **Objective (testable):** after boot, the registry entry exists with `0600` perms, `stream.sock` accepts a connection, the oplog is appendable, and a second instance in the same folder refuses to start (flock contention, I1/D2).

### P2-T2 — Heartbeat thread (decoupled)
- [ ] Dedicated thread writes a fixed-size, single-word, aligned **in-place** value to `heartbeat` file at a documented cadence (no rename churn, no mtime dependence, R2-18/D11).
- **Objective (testable):** killing the agent (`SIGKILL`) makes the heartbeat stale within `cadence × k`; liveness verdict = fresh heartbeat **AND** live pid **AND** matching `boot_id`. A reused pid with a different `boot_id` is correctly judged *down*.

### P2-T3 — Unmanaged-agent fallback
- [ ] An agent with the bridge **off** (or old binary): live flock, no registry entry → discoverable read-only via tier-② files; no command/stream channel.
- **Objective (testable):** with `cp-mod-bridge` disabled, the backend lists the agent read-only and never offers a command action for it.

---

## Phase 3 — Stream tee (tier ③): the fluid path

**Goal.** Live token streaming with sub-ms added latency that can **never**
backpressure the agent loop (I7).

### P3-T1 — SPSC enqueue at the drain point
- [ ] At `streaming.rs::process_stream_events` (additive read), push each `StreamEvent` to a lock-free SPSC ring — **one atomic enqueue, no serialization on the loop**.
- [ ] Ring-full ⇒ **O(1) fail-fast drop** (never block, never allocate) + set `degraded` (R2-13).
- **Objective (testable):** microbenchmark — the enqueue adds < 1 µs p99 to the loop tick; under a force-full ring, the loop tick time is statistically unchanged vs baseline (no backpressure).

### P3-T2 — Publisher thread + UDS write
- [ ] Dedicated publisher drains the ring, serializes `StreamFrame` (`MessageStartHint`/`Token`/`ToolArgs`/`PhaseHint`), writes `stream.sock`.
- [ ] Non-blocking write + bounded backoff on slow/dead UDS — never spins, never wedges (R2-14).
- [ ] First hint per `message_id` is self-describing; `seq` is per-`message_id`.
- **Objective (testable):** **V7** — stall the consumer and fill the ring; agent loop latency is unaffected and `degraded` is set. **R2-14** — kill the consumer; the publisher's CPU stays ~0% (no spin) and it recovers on reconnect.

### P3-T3 — End-to-end fluid path (M1)
- [ ] Trivial UDS consumer renders tokens; measure provider→consumer added latency.
- **Objective (testable):** added latency (agent enqueue → consumer receive), excluding provider network, is sub-millisecond at p99 over a 10k-token stream.

---

## Phase 4 — Command intake, injection & lifecycle

**Goal.** A command is authenticated, durable-before-ack, applied exactly-once,
and injected as *user input* — never through the autonomy spine (K7).

### P4-T1 — Authn + freshness (I9)
- [ ] Verify HMAC over `{seq, dedup_token, body}` keyed by `cap_token`; reject bad MAC.
- [ ] Monotonic nonce; reject stale/replayed nonces; log rejections.
- **Objective (testable):** **V6** — a replayed captured frame and a body-tampered frame are both rejected (stale nonce / bad MAC); a fresh valid frame is accepted.

### P4-T2 — Journal-then-ack (I11)
- [ ] Append the command to the oplog (`fsync`) **before** returning `accepted`; UDS = low-latency wake only.
- **Objective (testable):** **V2** — trigger a deadman re-exec after `accepted` but before the stream starts; on resume the effect is replayed exactly once (deduped by `seen`), with no false-accept loss and no double-apply.

### P4-T3 — Injection via the user-message entry (K7, additive call)
- [ ] Apply the effect through `src/app/actions/input.rs` (the human-typing path; clears `user_stopped`) — **not** `engine.rs::check_spine`.
- **Objective (testable):** a command delivered while the agent is in spine error-backoff or "no two synthetic in a row" still applies promptly (proves it bypasses the anti-loop guards). `engine.rs` shows zero diff.

### P4-T4 — Lifecycle as oplog appends
- [ ] `queued → delivered → processing → done | failed | expired` each an oplog append (never coalesced); TTL bounds the wait; expiry reissues with the **same** `dedup_token`.
- **Objective (testable):** **V10** — under coalescing load on tier-②, the oplog lifecycle stream shows a distinct `processing` state (no coalesced jump straight to `done`); the rev stream has no gaps (K5).

---

## Phase 5 — Standalone backend skeleton (the three seams)

**Goal.** The `cp-orchestrator` backend discovers agents, tails their oplogs, and
manages lifecycle — behind the swappable `Registry/Channel/Supervisor` seams.

### P5-T1 — `AgentRegistry` (discovery)
- [ ] `LocalRegistry`: watch `~/.context-pilot/agents/*.json`; emit `appeared/disappeared/status/stale`.
- [ ] Liveness verdict per P2-T2 (heartbeat + pid + boot_id); reap `*.tmp` by age.
- **Objective (testable):** booting/killing agents produces matching `appeared`/`disappeared` events within one poll cadence; a pid-reused stale entry is reported `stale`, not `live`.

### P5-T2 — `AgentChannel` read path (I12, I5)
- [ ] One inotify watch on the append-only oplog; tail `OpEntry` **gap-free**; 2–3 s poll backstop.
- [ ] `hydrate(hash)`: on-demand, content-addressed, **rev-pinned** body fetch (I5/R2-9).
- **Objective (testable):** **V8** — with 10k simulated agents, total inotify watch count ≈ agent count (one each), no `max_user_watches` exhaustion. **E7** — a lazy hydrate during a concurrent newer write returns the body pinned to the requested rev's head hash, never a newer body.

### P5-T3 — `AgentChannel` write path
- [ ] `send(Command)`: build HMAC+nonce frame, append to the agent's oplog (the durable inbox), UDS wake; return `Ack` on durable acceptance.
- **Objective (testable):** a command sent through the channel appears in the agent's oplog and is applied; killing the agent between append and wake still results in the effect on restart (durable inbox).

### P5-T4 — `AgentSupervisor` (lifecycle)
- [ ] `spawn`: detached (`setsid`) `cp --headless`, **allow-list gated with realpath canonicalization** (reject symlink/`..` escape, R2-15); resolve on registration.
- [ ] `stop/restart/adopt`.
- **Objective (testable):** **R2-15** — a spawn targeting a symlink that escapes an allow-listed root is rejected; a legitimate in-root folder spawns and self-registers. A backend restart re-adopts a still-live spawned agent (it keeps running, D7).

---

## Phase 6 — Backend services: fan-out, cost, materialized view

**Goal.** Turn a single agent channel into a fleet-scale, cost-bounded,
queryable view.

### P6-T1 — `StreamHub` fan-out
- [ ] One UDS consumer per agent → N frontend WS subscribers; bounded per-WS buffers; on overflow coalesce/drop + `degraded` flag.
- [ ] On long degraded stream: fall back to periodic oplog phase/partial snapshots as reconcile target (R2-17).
- **Objective (testable):** with N=100 subscribers on one agent, every subscriber receives frames; a deliberately stalled subscriber gets `degraded` and is reconciled from an oplog snapshot without affecting the others.

### P6-T2 — `CostBreaker` (durable)
- [ ] Aggregate fleet spend; the counter is **oplog-backed** (R2-8); trip ⇒ stop issuing commands/spawns; fail-closed on a missing counter.
- **Objective (testable):** **V9** — crash-loop the backend at the spend ceiling; the breaker remains tripped across every restart (durable counter), and no command/spawn is issued while tripped.

### P6-T3 — `MaterializedView` (lazy)
- [ ] In-memory view rebuilt from oplog **heads** on restart (eager heads, lazy bodies); serves REST queries.
- **Objective (testable):** backend restart latency scales with agent **count**, not fleet **disk** (measured: constant per-agent cost regardless of message-log size); every REST response carries the current `rev`.

---

## Phase 7 — Frontend ↔ Backend transport

**Goal.** REST for load/actions, one authenticated WebSocket for deltas + hints,
with reconnect, replay, and client-side action queueing.

### P7-T1 — REST surface
- [ ] Initial load, point queries, non-streaming actions; every response carries `rev`; actions return `cmd-id` + echo `dedup_token`.
- **Objective (testable):** a load returns a consistent snapshot at a single `rev`; an action returns a `cmd-id` and the client can correlate the resulting oplog delta to it.

### P7-T2 — Authenticated WebSocket (R2-10)
- [ ] Backend mints a **short-lived, single-use upgrade ticket** (out-of-band); WS handshake exchanges it for a session bound to that one connection. **No CORS/`Origin` reliance.**
- [ ] Channel carries rev-numbered oplog deltas (replayable) + ephemeral stream hints (not replayed).
- **Objective (testable):** a WS connect without a valid ticket is rejected; a replayed (already-used) ticket is rejected; a valid first-use ticket establishes a bound session.

### P7-T3 — Reconnect, replay & client action queue (R2-1)
- [ ] Reconnect replays oplog deltas by `rev`; gap beyond buffer ⇒ `resync` → REST refetch of heads + lazy hydrate.
- [ ] Frontend queues actions during backend downtime and replays on reconnect (dedup-safe via ack-rev eviction).
- [ ] Client ignores any frame/response with `rev ≤` applied rev (monotonic).
- **Objective (testable):** kill+restart the backend mid-session; the client replays its queued actions, each applied exactly once (ack-rev dedup), and the view converges to the correct `rev` with no duplicate effects.

---

## Phase 8 — Frontend rendering contract (fluidity)

**Goal.** The browser side that determines whether streaming feels fluid.

### P8-T1 — rAF token batching
- [ ] Tokens accumulate into a per-message buffer flushed **once per `requestAnimationFrame`** — never `setState` per token.
- **Objective (testable):** at a high token rate, React commit count ≈ frame count (not token count); no dropped frames in a profiler trace over a sustained stream.

### P8-T2 — Orphan-token buffer (I10, R2-3)
- [ ] Buffer tokens by `message_id` until the oplog "message created" arrives; bounded (per-message byte cap + global cap + TTL); drop-and-refetch on overflow.
- **Objective (testable):** **V5** — inject token frames *before* their `MessageStart` and drop a `MessageStartHint`; the UI reconstructs the message from the oplog header with no orphan leak (buffer stays within bounds) and no stuck/duplicate message.

### P8-T3 — Degraded indicator
- [ ] Surface the `degraded` flag as a visible "stream degraded — catching up" state.
- **Objective (testable):** forcing backend→frontend overflow shows the indicator; clearing the stall removes it after reconcile.

---

## Phase 9 — Security hardening & audit

**Goal.** Consolidate and verify every security mechanism end-to-end.

### P9-T1 — Authn/anti-replay audit (I9)
- [ ] Verify HMAC coverage, monotonic nonce, `cap_token` **rotation each boot**, compaction of consumed command entries (R2-11).
- **Objective (testable):** captured frames are non-replayable across a boot (token rotated); at-rest exposure window is bounded (consumed entries compacted within one compaction cycle).

### P9-T2 — Transport & filesystem perms
- [ ] `0700` on `.context-pilot/`, oplog dir, registry dir, `stream.sock`; `0600` on registry entries; registry holds **no** API keys.
- **Objective (testable):** an automated perms check asserts the exact mode bits on every path; a grep proves no API key is written to any registry entry.

### P9-T3 — Spawn blast-radius bound
- [ ] Allow-list + canonicalization (R2-15) verified; document the residual (same-user malware / intrinsic RCE) honestly.
- **Objective (testable):** the allow-list test suite (legit / symlink-escape / `..`-escape / non-listed) passes; `SECURITY.md` records the bounded-not-eliminated residual.

---

## Phase 10 — Observability, ops & versioning

**Goal.** Make the running system legible and upgradable.

### P10-T1 — Metrics
- [ ] Per-agent stream latency p50/p99, dropped/coalesced frames + degraded events, command queue depth + lifecycle histogram, `rev` lag, oplog append/`fsync` latency, heartbeat freshness, WS subscriber counts, reconnect/resync rates, durable CostBreaker state, rejected-command (auth/MAC/nonce) counts, inotify watch count vs limit.
- **Objective (testable):** a metrics endpoint exposes every listed series; an integration test asserts each series moves under a synthetic load that should move it.

### P10-T2 — Structured logging
- [ ] Logs keyed by `agent_id` + `cmd_id` + `rev`.
- **Objective (testable):** a single command's full lifecycle is reconstructable by filtering logs on its `cmd_id`.

### P10-T3 — Versioning & rolling upgrade (R2-16)
- [ ] N-1 major compatibility window; **backend upgrades first** ordering invariant; backend tolerates N+1 oplog entries read-only.
- **Objective (testable):** a backend at version `N` drives an agent at `N-1` through a full command + stream cycle with no orphaning; the documented upgrade order is enforced by a version-check gate.

---

## Phase 11 — Fault-injection validation gate (production-v1)

**Goal.** Promote every 🔴/🟠 mechanism from "designed" to "validated." This is the
**v1 ship gate** — until all rows pass, the design §20 statuses are
"designed, test-pending," not ✅ (R2-19 / M1).

### P11-T1 — Run the full §25 matrix
- [ ] **V1** oplog torn-tail · **V2** journal-then-ack under re-exec · **V3** dedup after 2-h outage · **V4** power-cut durability · **V5** frame drop/reorder + dropped `MessageStartHint` · **V6** auth replay/tamper · **V7** backpressure · **V8** 10k-agent watch budget · **V9** CostBreaker crash-loop · **V10** gap-free revs under coalescing.
- **Objective (testable):** all ten rows pass in CI as automated fault-injection tests; each maps to its originating task's objective and to a design §25 row. A red row blocks the v1 tag.

### P11-T2 — End-to-end soak under load
- [ ] Multi-agent fleet, sustained streaming + commands + reconnect churn, for a documented duration; assert no leaks (orphan buffers, inotify watches, publisher threads), bounded memory, stable `rev` lag.
- **Objective (testable):** over the soak window, memory/FD/thread counts are flat, no oplog gap is observed, and p99 stream latency stays within budget.

### P11-T3 — Sign-off
- [ ] Flip each design §20 register row from "designed, test-pending" to ✅ only when its validating V-test is green.
- **Objective (testable):** every 🔴/🟠 row in design §20 cites a passing Phase-11 test; the v1 release tag is gated on a fully-green register.

---

## Out of scope for v1 (tracked)

- Shared-memory ring stream transport (UDS is v1; seam is swappable).
- Network / multi-tenant transport (mTLS, signed commands) — design §17 G7 🔵.
- OS-level sandboxing of spawned agents (RCE residual is *bounded*, not eliminated).
- The unmerged multi-worker model: per-worker SPSC ring + publisher thread is
  designed (§12) but lands when multi-worker merges.
