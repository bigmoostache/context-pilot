# Orchestration Backend — Design Doc (WIP)

> **Status:** discussion / brainstorm. **Nothing here is implemented.** Living
> artifact we iterate on until we're perfectly aligned on the infrastructure
> that powers the orchestration frontend (the `ui/` maquette).
>
> **v5 — grounded in the actual codebase.** v4 hardened the design against an
> adversarial review but was authored against an *imagined* persistence layer. A
> code-grounded round of attack (reading `src/state/persistence/writer.rs`,
> `save.rs`, `crates/cp-mod-spine/src/engine.rs`, `src/app/run/streaming.rs`, and
> the deadman re-exec) showed that the agent's **real** writer is *async,
> 50 ms-debounced, coalescing, and never `fsync`s*; its command path is the
> *autonomy-safety spine*; and its crash-recovery is a *process-replacing
> re-exec*. v4's I8/I2/I3 each assumed a substrate that doesn't exist and partly
> *can't* coexist with the one that does.
>
> **v5's keystone move:** stop trying to make the agent's shared
> `PersistenceWriter` transactional. Introduce a separate, bridge-owned
> **append-only, `fsync`'d operation log (oplog)** as the agent's authoritative
> cross-process interface; treat the existing state files as a *coalesced,
> reconstructible cache* of replaying it. This leaves `writer.rs` **untouched**
> (honoring the prime directive, constraint #7), makes commits **O(1)
> append+fsync** instead of O(total-files), makes "accepted" mean *durable*, and
> never skips a `rev`. New §24 maps **every invariant to the exact code it
> touches**; new §25 is a fault-injection acceptance matrix. §23 retains the v4
> adversarial map.
>
> **v6 — honesty pass (senior-dev review).** A second reviewer accepted the
> keystone but flagged three things the v5 formalism papered over, now fixed:
> (1) **"behaviorally inert" was overclaimed** — the *logic* is unchanged, but
> turning the bridge ON adds per-event durability latency the agent didn't pay
> before; v6 moves that cost **off the hot path** (dedicated oplog thread +
> group commit) and restates the claim honestly (only *bridge OFF* = "exactly as
> today"). (2) **`fsync(file)+fsync(dir)` per append was overkill** — v6 uses
> `fdatasync` per commit-group and `fsync(dir)` **only on segment creation**.
> (3) **The body-storage model was internally contradictory** — v5 listed message
> bodies as the *mutable* tier-② cache *and* called them immutable & content-
> addressed; those can't both be true of the same files. v6 resolves ownership
> explicitly (see §3.1): tier-② `messages/{uid}.yaml` is the agent's **mutable
> self-reload cache**; the **bridge** owns a separate **immutable, content-
> addressed body store** (`oplog/bodies/`, small bodies inlined in the oplog,
> large bodies spilled) — a *named* second write path, not a free property. v6
> also **locks Open-Q1** (framed + per-record CRC) because the 🔴 torn-tail
> invariant V1 depends on it.
>
> **Read §20 through §25.** Every ✅ in the problem register means **"designed,"
> not "validated."** v4 was fully ✅'d and was wrong because it wasn't grounded in
> code; a register where every problem is solved *on paper before any code* is
> argument-completeness, not correctness. **§25 is the real status.** The first
> code written should be the isolated oplog append + crash-replay harness
> (V1/V2/V10) against `kill -9` and a simulated deadman re-exec — that tiny
> prototype tests the load-bearing durability claim better than another doc
> revision can.
>
> **v7 — two load-bearing answers + one subtraction (round-2 senior review).**
> The review accepted the architecture (WAL + materialized view) but found that
> v6 left two questions under the keystone unanswered, and that the v1 security
> machinery was over-built. v7 answers both and subtracts the bloat:
> (1) **Who guarantees the body is durable before the oplog entry that names
> it?** ⇒ new **I13 body-before-reference barrier**: a content hash may appear in
> a *durable* oplog entry only after that body is itself durable. Small bodies are
> inlined into the same fsync'd append (trivially satisfied); a spilled large body
> is `fdatasync`'d **before** its referencing entry commits. Content-addressing
> makes a crash in the gap a harmless *orphan body* (GC'd), never a dangling
> reference. So **"accepted = durable" now covers the contents, not just the
> envelope.** The tier-② mutable `messages/{uid}.yaml` copy is explicitly **not**
> load-bearing for durability — the authoritative body is the bridge's fsync'd
> immutable one. (2) **On which thread do oplog appends run?** ⇒ explicit
> execution model (I2): **the main loop never fsyncs.** It does one lock-free,
> non-blocking *enqueue* per record (like the tee); the **dedicated oplog thread**
> group-commits — N phase transitions in a window cost **one** `fdatasync`,
> off-loop. The only durability-gated ack (command-accept, I11) runs on the
> intake/oplog thread, never the hot loop. Phase rides the stream as a sub-ms hint
> and lands durably a group-commit later. (3) **Subtraction:** HMAC + monotonic
> nonce + per-boot rotation (old I9) guarded frame-replay on a `0700`/`0600` local
> UDS — a *strictly weaker* attacker than the same-user malware the perms can't
> stop anyway. v7 makes **filesystem perms + a presence-checked bearer `cap_token`
> the v1 command authn**, and moves HMAC/nonce/mTLS to the **remote-transport seam
> (future)** where they actually earn their keep. The frontend WS ticket is
> **kept** but re-justified: its real threat is a malicious *website* in the
> user's browser reaching `ws://localhost` (a confused-deputy / DNS-rebind that
> perms don't cover and `Origin` can't be trusted for) — not a local process.

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
   socket, append to a log, write a heartbeat) are fine; **never change how the
   agent reasons/acts**, and **never rewrite the shared persistence path the 22
   modules depend on**. The agent must run identically whether or not the backend
   is watching. *(v5 takes this literally: see §24 — the only agent-side change
   is one additive module; `writer.rs` is not modified.)*
8. Catalog **information to gather from** + **actions to perform on** agents.
9. **Live streaming must be FLUID.** Flow: *LLM provider → rust agent → backend →
   frontend*, **near-millisecond added delay** end-to-end. Every added
   millisecond costs users. Hard requirement.
10. **Production-ready on v1.** No "rewrite it three times." Foreseeable issues
    are tracked (§20), grounded in the real code (§24), and validated (§25).

**Preferences:** simple, robust, leverage existing choices. Same machine for now,
but an **abstraction layer** over comms **and** discovery. Per-agent connection
managed internally.

---

## 3. The keystone: three durability tiers + two planes

v4 split traffic into two planes (durable control / ephemeral stream). v5 keeps
that, but **grounds the "durable" side in how the agent actually persists** by
naming **three durability tiers** with sharply different guarantees:

| Tier | What | Medium | Guarantee | Coalesced? | `fsync`? |
|---|---|---|---|---|---|
| **① Oplog** (NEW v5) | command effects, `rev` assignment, `seen`-marks, phase transitions, lifecycle, cost aggregate, **+ immutable body store** (§3.1) | **append-only file** `.context-pilot/oplog/` + `oplog/bodies/` (bridge-owned) | **authoritative, durable, exactly-once** | **never** | **yes — `fdatasync` per commit-group; `fsync(dir)` per segment** |
| **② State cache** | panel snapshots, worker state, **mutable** message bodies (`messages/{uid}.yaml`) | existing `.context-pilot/` files via the **untouched** `PersistenceWriter` | **best-effort, reconstructible** by replaying ① | yes (50 ms debounce) | no (as today) |
| **③ Stream** | live token deltas, tool-arg deltas, **latency hints** for phase/message-start | **Unix domain socket** (UDS) | **ephemeral, droppable** | n/a | no |

**Why three, not two.** The code-grounded attack showed v4 conflated two very
different needs onto one "control plane": (a) a *handful* of events that must be
**exactly-once and observed in order** (a command's effect, the `rev`), and (b) a
*large, churny* body of **state** that is fine to lose and rebuild (panels,
message bodies). Tier ① is tiny, append-only, `fsync`'d — cheap to make perfect.
Tier ② is large and churny — left exactly as the real `writer.rs` already does it
(async, debounced, coalescing), because for state, *last-write-wins is correct*
(it's a cache of replaying ①, per I5). **You only pay for durability where you
need it.**

**The relationship:** ② is a *materialized view* of ①. The agent appends an
effect to ① (`fsync`), then lets its normal best-effort save update ②. On crash,
replay ① to rebuild what ② lost. The backend **tails ①** for truth and
**hydrates bodies** from the oplog's immutable store on demand (§3.1, I5).

### 3.1 Who owns message bodies — the two stores are different files

v5 contradicted itself: it listed message bodies under the *mutable, overwritten*
tier-② cache **and** called them *immutable, content-addressed, never rewritten*.
Those cannot both hold for the same file. If `writer.rs` overwrites
`messages/{uid}.yaml` in place, the bytes for an *older* `rev` are clobbered, and
rev-pinning can only stop you reading a *newer* body — it cannot resurrect an
older one that no longer exists. v6 resolves this by splitting ownership into
**two physically distinct stores**:

| Store | Path | Owner | Mutability | Reader |
|---|---|---|---|---|
| **Agent self-reload cache** | `messages/{uid}.yaml` | the **untouched** `PersistenceWriter` (tier ②) | **mutable**, overwritten in place, uid-named | the agent on its own reload; unmanaged-agent read-only listing |
| **Immutable body store** | `oplog/bodies/{hash}` *(or inlined in the oplog entry)* | the **bridge** (tier ①) | **immutable**, content-addressed, write-once | the backend's rev-pinned hydration (I3/I5) |

So "content-addressed immutable bodies" is **real** — but it is a property of the
**bridge-written** `oplog/bodies/` store, *not* of the agent's mutable cache. This
costs an honest, **named** trade:

- **A second write path.** For a finalized message, the bridge writes an immutable
  hash-named body (in addition to the writer's mutable `{uid}.yaml`). To keep this
  cheap, **small bodies are inlined directly into the oplog entry** (already
  immutable, already in the fsync'd append — *zero* extra file, *zero* double
  write); **only large bodies spill** to `oplog/bodies/{hash}`. Spilled bodies are
  the only genuine double-write, and they are rare.
- **`writer.rs` is still untouched.** The bridge *adds* a write path; it does not
  modify the writer. Constraint #7 ("never rewrite the shared persistence path")
  holds — the shared path is unchanged; an *additional, disjoint* one exists.
- **This is not a second writer in the I1 sense.** I1 forbids two **processes**
  writing a folder (enforced by `flock`). Within the *one* agent process, the
  `PersistenceWriter` thread and the bridge's oplog thread are two threads writing
  **disjoint paths** (`messages/` vs `oplog/`); there is no same-file race. I1 is
  about cross-process exclusion, not intra-process thread count.

**The tier-② copy is NOT load-bearing for durability (v7).** The
unsynced/coalescing `messages/{uid}.yaml` is a *convenience cache* for the agent's
own reload + unmanaged-agent listing. If it is stale or lost after a crash, replay
of the oplog + the immutable body store **rebuilds it**. The *authoritative* body
is the bridge's fsync'd immutable one — so the fact that `writer.rs` writes the
cache copy without `fsync` is **correct**, not a durability hole: it isn't the
authority.

**Body-before-reference barrier (v7, I13) — what makes "accepted = durable" true
for *contents*, not just the envelope.** A durable oplog entry that says "message
M, head = `H`" is only honest if body `H` is already on disk. The rule:

- A content hash `H` may appear in a **durable** oplog entry **only after** the
  body for `H` is itself durable.
- **Inlined small bodies** are *in* the same fsync'd append — the barrier is
  trivially satisfied (the body *is* the entry).
- A **spilled large body** is written `tmp → fdatasync → atomic rename`, and that
  rename must complete **before** the referencing entry joins a commit group.
- A crash *in the gap* (body written, entry not yet durable) leaves an **orphan
  body** — unreferenced, idempotent to rewrite, GC'd at compaction. A crash *after*
  the entry is durable means its body provably preceded it. Either way: **no
  durable entry ever references a missing body** (validated by V12).

**Sticky state never rides ③ alone.** Phase (idle·streaming·tooling) has its
**authoritative** record in ① and a **latency hint** on ③ — fast to show, but ①
wins and self-heals a dropped hint (resolves the v4 #6 fix's latency regression,
K6, and I10's dropped-`MessageStart`, R2-7).

---

## 4. Recommended architecture

```
React frontend
   │  REST (load + actions)  +  ONE auth'd WebSocket (oplog deltas + ephemeral stream hints)  ← §9
   ▼
Orchestrator backend (standalone, Rust — reuses cp-base/cp-render types)
   ├── AgentRegistry   (discovery)   → watch ~/.context-pilot/agents/*.json (rare writes)
   ├── AgentChannel[]  (per-agent transport, internally managed)
   │      OPLOG tail:    ONE inotify watch on <folder>/.context-pilot/oplog (append-only)   ← truth (I12)
   │      body hydrate:  on-demand reads of content-addressed bodies referenced by the oplog (I5)
   │      LIVE stream:   connect <folder>/.context-pilot/stream.sock (UDS) ← token tee + hints
   │      liveness:      UDS connected  +  polled heartbeat file (NOT a watched rename)
   │      command:       append to oplog (fsync) → ack "committed"; UDS = low-latency wake
   ├── AgentSupervisor (lifecycle)   → spawn DETACHED `cp --headless` (ALLOW-LIST gated, canonicalized)
   ├── StreamHub       (per-agent fan-out: 1 UDS in → N frontend WS out, bounded buffers, degraded flag)
   ├── CostBreaker     (global aggregate-spend circuit-breaker; counter is oplog-backed = durable)
   └── MaterializedView[]  (in-memory cache, rebuilt by replaying oplog heads; LAZY body hydration)
   │
   ▼  Backend ↔ Agent
Agent Rust loop (per folder):
   ├── [UNCHANGED]  PersistenceWriter (writer.rs) — async/debounced/coalescing state cache (tier ②)
   └── [additive]   cp-mod-bridge (the ENTIRE agent-side footprint — see §24):
         • boot:    flock agent.lock (FD-inheritable, H5) ; write registry + cap_token ; bind stream.sock ; open oplog
         • oplog:   append-only WAL; command effect + rev + seen-mark + phase + lifecycle = ONE fsync'd append (I8/I11)
         • heartbeat: DEDICATED thread → polled heartbeat file (decoupled from registry)
         • stream TEE: StreamEvent → lock-free SPSC enqueue → DEDICATED publisher thread → stream.sock
         • command:  journal-to-oplog-THEN-ack ; inject via the existing USER-MESSAGE entry (NOT the spine, K7)
```

---

## 5. Invariants (the robustness spine)

- **I1 — Single writer *process* per folder.** `flock` on
  `.context-pilot/agent.lock`. A 2nd *instance* refuses / goes passive. Backend
  never writes agent state; it only *appends commands to the oplog*. This is
  **cross-process** exclusion: within the one live agent process, the existing
  `PersistenceWriter` thread (tier ② `messages/`/panels) and the bridge's oplog
  thread (tier ① `oplog/`) write **disjoint paths** and do not race (§3.1) — I1
  does not forbid that. *(See H5 for the deadman-re-exec interaction.)*
- **I2 — Durable writes off the hot path; the main loop never `fsync`s.**
  **Execution model (v7, explicit):** every oplog append — including frequent
  **phase transitions** during streaming — is a **lock-free, non-blocking enqueue**
  on the main loop (one atomic push, exactly like the stream tee). A **dedicated
  oplog thread** drains the queue and **group-commits**: it writes all pending
  records and issues **one `fdatasync` per group**, so N phase transitions in a
  commit window cost **one** sync, entirely off-loop. `fsync(dir)` happens **only
  on new-segment creation**, not per append (`fdatasync` covers appended data + the
  size needed to read it back). The **one** durability-gated wait — command-accept
  (journal-then-ack, I11) — runs on the **intake/oplog thread**, *not* the main
  loop: the loop is never blocked on a sync. *Tier ②* state files keep the existing
  best-effort `fs::write` (no sync) — a reconstructible cache (I5); a periodic
  coalesced **checkpoint** bounds replay length. *(v4 wrongly demanded a per-write
  fsync barrier on the shared writer — K1/K4; v5 confined durability to the oplog;
  v6 confined its cost to `fdatasync`-per-group; v7 pins down **where it runs** —
  the answer the round-2 review demanded: nowhere on the hot loop. Phase rides the
  stream plane as a sub-ms hint and its authoritative record lands a group-commit
  later; a crash in that window replays to the last durable phase, which self-heals,
  I10/K6. **Honesty:** bridge-ON still adds bounded per-event durability work vs a
  no-bridge run — off-loop, group-amortized, and measured by V11 — so "exactly as
  today" holds only bridge-OFF.)*
- **I3 — Snapshot consistency via bounded heads, not full enumeration.** The oplog
  carries a monotonic `rev`. Message/panel **bodies are content-addressed in the
  bridge-owned immutable store** (filename = content hash → write-once, never
  rewritten; small bodies inlined in the oplog entry, large bodies in
  `oplog/bodies/{hash}` — §3.1). This is the **bridge's** store, distinct from the
  mutable tier-② `messages/{uid}.yaml` cache. The snapshot reference is a
  **bounded set of current heads** (per-thread last-message hash, per-panel hash),
  not an enumeration of all history. Reading a `rev` means reading its heads +
  hydrating referenced bodies on demand. *(v4's "manifest enumerates every file
  including messages" was O(total-files) rewritten every commit → O(S²)
  amplification, K3. Content-addressing + heads makes it O(threads+panels),
  bounded.)*
- **I4 — Commands idempotent + ordered + ack'd, by SEMANTIC key.** Each command
  carries a transport id + sortable seq **and** a client-supplied **`dedup_token`**
  (semantic key). The oplog's `seen`-set keys on `dedup_token`; a TTL-reissue with
  the *same* `dedup_token` is deduped. At-least-once delivery, **exactly-once
  effect**. The `seen`-set is **evicted by acknowledged-`rev`, not by time** — a
  token retires only once its effect's `rev` is durably confirmed consumed, so a
  replay after *any* outage duration is still deduped (resolves R2-1: dedup-window
  vs long-outage replay).
- **I5 — Tier ② is a LAZILY-rebuildable cache of the oplog.** Only durable truth =
  the oplog + its **immutable content-addressed body store** (§3.1) + registry. On
  restart the backend rebuilds **only** registry + each agent's oplog **head**
  (`rev` + heads); bodies hydrate on demand from the bridge's immutable store,
  pinned to the requested `rev`'s head hash — and because that store is write-once,
  the pinned hash **always still exists** (a lazy read can neither return a *newer*
  body nor find an older one clobbered, resolving R2-9). Restart latency is bounded
  by agent **count**, not fleet **disk**.
- **I6 — A command's effect and its `seen`-mark are the SAME oplog append.** One
  `fsync`'d append contains `{cmd_id, dedup_token, rev, effect}`. Either the append
  is durable (effect happened, token seen) or it isn't (neither) — there is no
  partial state, by the atomicity of append-then-fsync. Subsumed into I8.
- **I7 — The stream plane (tier ③) is best-effort and MUST NOT backpressure the agent.** The
  tee is a **lock-free SPSC enqueue** on the loop; a **dedicated publisher thread**
  serializes + writes the socket. Ring-full ⇒ **O(1) fail-fast drop** (never block,
  never allocate) + a `degraded` mark; the publisher uses **non-blocking writes +
  bounded backoff** on a slow/dead UDS (never spins, never wedges — R2-13/R2-14).
  The oplog is the safety net. *(Today the agent drains a single `StreamEvent`
  channel on the main loop — `streaming.rs::process_stream_events` — so the tee is
  genuinely single-producer; see §24 note on future multi-worker.)*
- **I8 — The oplog is the authoritative, append-only, `fsync`'d event log (NEW
  v5).** Command effects, `rev` assignment, `seen`-marks, phase transitions,
  lifecycle states, and the cost aggregate commit as **append-only oplog entries**
  — **O(1) append + fsync, never coalesced.** The agent's existing
  `PersistenceWriter` (tier ②) is **not modified**: it remains the best-effort,
  debounced, coalescing state cache. The single main loop assigns `rev` (it's the
  oplog append offset) — inherently serialized, no cross-worker race (the v4
  "atomic cross-worker snapshot" worry is moot: `build_save_batch` already
  snapshots synchronously on one thread — retraction noted in §22).
- **I9 — Command authn is the filesystem for v1; crypto is the remote seam
  (v7 subtraction).** On the same-machine v1, the command channel (UDS + oplog
  inbox) lives under `0700`/`0600`: anything that can write it is **already running
  as the same user**. So v1 authn = **perms + a presence-checked bearer
  `cap_token`** (minted per boot in the `0600` registry; rejects blind/accidental
  cross-talk; cheap). HMAC over the frame + a monotonic nonce + per-boot rotation
  defend against frame *replay/forgery* — but that attacker is **strictly weaker**
  than the same-user malware the perms already can't stop (it can read the
  `cap_token` and forge freely, or just write `inbox/` directly). So the crypto
  guards a non-threat for v1 and is **moved to the remote-transport seam** (the
  wire protocol keeps an `auth` field: local impl = perms+bearer; remote impl =
  HMAC/mTLS/signed commands — §17 G7 🔵). The honest v1 truth: against same-user
  malware, perms, bearer, and crypto are *all* defeated; only OS sandboxing helps,
  and it's out of scope. (The **frontend WS** ticket is a *different* threat model
  and is **kept** — see I9b/§17.)
- **I9b — The frontend WebSocket ticket defends the browser, not the loopback
  (v7).** `ws://localhost` is reachable by **any website** the user opens — a
  malicious page can attempt to drive the fleet (confused-deputy / DNS-rebinding).
  `Origin`/CORS is advisory for WS and can't be trusted; perms don't help because
  the *browser is the user's own process*. So the backend mints a **short-lived,
  single-use upgrade ticket** out-of-band; the WS handshake exchanges it for a
  session bound to that one connection. This earns its keep precisely because the
  attacker (a website) is **not** a same-user process.
- **I10 — Cross-plane causal ordering (NEW v4, hardened v5).** The **durable**
  "message created" record lives in the oplog; the stream plane's `MessageStart`
  is a *latency hint* only. A token frame may beat the hint — the frontend buffers
  orphan tokens by `message_id` (bounded: per-message byte cap + global cap + TTL,
  drop-and-refetch on overflow — R2-3), and the oplog "message created" entry is
  the *guaranteed* arrival of the header (bounded by commit cadence, not the 2–3 s
  poll). A dropped `MessageStart` self-heals from the oplog (resolves R2-7). `seq`
  is **per-`message_id`** so gaps are unambiguous.
- **I11 — "Accepted" means durable (NEW v5).** A command is appended to the oplog
  (`fsync`) **before** the "accepted" ack is sent. The UDS-fast path is *delivery +
  wake*, not durability; the oplog **is** the durable inbox. So a deadman re-exec
  (which fires precisely on a hung stream — the agent's own recovery path) replays
  the oplog and re-derives the effect, deduped by `seen`. No lost effect, no false
  ack, no double-apply (resolves K2). The two-phase ack's "accepted" is honest.
- **I12 — One watch per agent (NEW v5).** The backend observes each agent via **a
  single inotify watch on its append-only oplog** (+ on-demand body hydration). It
  does **not** enumerate per-file watches over `.context-pilot/`. N agents = N
  watches — well under `fs.inotify.max_user_watches` — so the control plane stays
  event-driven at fleet scale (resolves K8); the 2–3 s poll is a pure backstop.

---

## 6. The three abstraction seams

```text
interface AgentRegistry {              // §10 discovery
    list() -> [AgentHandle]
    watch() -> stream<RegistryEvent>   // appeared / disappeared / status / stale
}

interface AgentChannel {               // per-agent transport (one connection, internally managed)
    head() -> (rev, Heads)                  // current oplog head: rev + content-addressed heads (read)
    tail_oplog(since_rev) -> stream<OpEntry> // authoritative, append-only, gap-free deltas (tier ①)
    hydrate(hash) -> Body                   // on-demand body fetch, content-addressed, rev-pinned (I5)
    subscribe_stream() -> stream<StreamFrame> // LIVE token/hint frames (tier ③, best-effort)
    send(Command) -> Future<Ack>        // auth (v1: perms+bearer; remote: HMAC); journaled-to-oplog-then-ack (I11); ordered, idempotent
    health() -> Liveness                // UDS connected + polled heartbeat
}

interface AgentSupervisor {            // lifecycle / process control
    spawn(folder, opts) -> Future<AgentHandle>   // ALLOW-LIST gated (canonicalized); detached; resolves on registration
    stop(id, mode) ; restart(id) ; adopt(handle)
}
```

- **v1 impls:** `LocalRegistry` (watch the registry dir), `LocalChannel` (truth =
  oplog tail over a single inotify watch; bodies = content-addressed on-demand
  reads; live = `stream.sock` UDS; command = bearer-checked oplog append + UDS wake),
  `LocalSupervisor` (detached `cp --headless`, adopt via registry).
- **One transport-agnostic, versioned wire protocol** (`Command` / `OpEntry` /
  `StreamFrame` / `Heads` / `Body`). The medium is swappable (UDS → TCP/QUIC
  remote, or shared-memory ring for lower local latency) **without touching
  orchestration logic**.

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

**The agent tee.** The agent drains a single `StreamEvent` channel on its main
loop (`streaming.rs::process_stream_events`). The bridge adds a **lock-free SPSC
enqueue** at that point — **one atomic push, no serialization on the loop**. A
**dedicated publisher thread** drains the ring, serializes `StreamFrame`s, writes
the socket. The agent renders/persists identically; the tee can never steal CPU
from or backpressure the loop (I7).

**Frame schema:** `StreamFrame { agent_id, worker_id, thread_id, message_id, seq,
kind, payload }`, `kind ∈ { MessageStartHint, Token, ToolArgs, PhaseHint }`.
**`MessageStartHint` and `PhaseHint` are latency hints only** — their durable
truth is the oplog (I8/I10). The first hint per `message_id` is self-describing so
the frontend can paint before the oplog entry lands; if it drops, the oplog
"message created" entry self-heals it.

**Fan-out (StreamHub).** One UDS consumer per agent → N frontend WS subscribers.
The agent never scales connections. Fan-out is O(subscribers) direct writes.

**Backpressure (I7).** *Agent → backend:* non-blocking publisher; **O(1)
fail-fast drop** if the ring is full (drop must keep `MessageStartHint`/`Token`
coherent — a token whose start was dropped is replayable from the oplog). *Backend
→ frontend:* bounded per-WS buffer; on overflow, coalesce/drop **and set a
`degraded` flag** surfaced to the UI ("stream degraded — catching up"). Because a
*long* degraded stream has no final message yet, the backend falls back to
**periodic oplog phase/partial snapshots** as the reconcile target, not just the
final message (resolves R2-17).

**Publisher on a dead/slow UDS:** non-blocking `write` + bounded backoff; never
spins (no CPU burn), never blocks the ring beyond its bound (R2-14).

**Frontend rendering contract.** Mandatory: tokens accumulate into a per-message
buffer flushed **once per `requestAnimationFrame`** — **never** `setState` per
token. This is a first-class requirement of "fluid," not an implementation detail.

**Latency hygiene.** `TCP_NODELAY` on any TCP hop, flush per frame, never debounce
tokens, never route tokens through disk.

**Crash mid-stream.** Agent dies → `stream.sock` closes → backend reads the oplog
phase (→ `down`/`interrupted`) — never stuck. Partial live text is ephemeral.

---

## 8. Backend ↔ Agent (control = oplog)

### 8.1 Read — tail the oplog (truth) + hydrate bodies (lazy)
The backend keeps **one inotify watch on the append-only oplog** (I12) and tails
appended `OpEntry`s — **gap-free by construction** (append-only never coalesces,
unlike the tier-② debounced writer that *replaces* its pending batch and skips
intermediate revs — K5). Bodies referenced by an entry are hydrated on demand,
content-addressed and `rev`-pinned (I5). A 2–3 s poll of the oplog tail is a pure
backstop for a dropped inotify event.

### 8.2 Write — command, journal-then-ack
- **Authn (I9, v7):** v1 = **filesystem perms (`0700`/`0600`) + a presence-checked
  bearer `cap_token`** on every command. The same-machine assumption makes this the
  honest authn boundary; HMAC/nonce live at the remote seam (§17 G7).
- **Idempotency (I4):** the oplog `seen`-set keys on `dedup_token`, evicted by
  acknowledged-`rev` (not time) — replay-safe across any outage.
- **Journal-then-ack (I11):** the command is **appended to the oplog (`fsync`)
  first**, *then* `accepted` is returned. UDS is the low-latency wake; the oplog is
  the durable inbox. Survives deadman re-exec.
- **Injection bypasses the autonomy spine (K7):** the bridge applies a command's
  effect via the **existing user-message entry point** (the same path a human
  typing in the TUI uses — `actions/input.rs`, which clears `user_stopped`), **not**
  via `check_spine` / `apply_continuation`. The spine's anti-loop guards ("no two
  synthetic in a row," `2^n` error backoff, `user_stopped` hard-stop —
  `engine.rs`) exist to stop the agent looping on *itself*; a backend command is
  *external user input* and must not be throttled or swallowed by them.
- **Lifecycle states** (`queued → delivered → processing → done | failed |
  expired`) are **oplog appends** (never coalesced), so the UI reliably observes
  "processing" rather than a coalesced jump to "done" (resolves K5/#12). TTL bounds
  the wait; on expiry, reissue keeps the **same `dedup_token`**.
- **Two-phase semantics:** "send message" acks on **durable acceptance** (I11);
  the LLM work is observed later via the stream + oplog. Mutations ("archive
  thread") ack on completion.

---

## 9. Frontend ↔ Backend

- **REST** — initial load + point queries + non-streaming actions. Every response
  carries `rev`. Actions return a `command id` + echo the `dedup_token`.
- **WebSocket** — the single live channel, **authenticated** (R2-10 hardened): the
  backend mints a **short-lived, single-use upgrade ticket** delivered out-of-band;
  the WS handshake exchanges it for a session bound to that one connection (a
  leaked served ticket is useless after first use; sessions refresh). **CORS /
  `Origin` are NOT relied upon.** The channel carries:
  - *oplog deltas* — `rev`-numbered, **replayable, gap-free** (state, new messages,
    phase, MY_TURN, cost, lifecycle).
  - *stream hints* — ephemeral, **not** replayed (the oplog covers any gap).
- **Reconnect:** the backend replays oplog deltas by `rev` (the oplog is the ring);
  gap beyond the buffer ⇒ `resync` → REST refetch of heads + lazy hydrate.
- **Backend-down resilience (R2-1 resolved):** the frontend **queues actions
  client-side** and **replays on reconnect**; replay is safe because the oplog
  `seen`-set is evicted by acknowledged-`rev`, not time, so a replay after a long
  outage is still deduped.
- **Client monotonic rev:** ignore any frame/response with `rev ≤` applied rev.

---

## 10. Discovery, heartbeat & single-instance

- On boot: take the **folder flock** (I1, FD-inheritable — H5), bind `stream.sock`,
  **open/create the oplog**, mint `cap_token`, then register
  `~/.context-pilot/agents/<id>.json` (`0600`) = `{ id, folder, pid, boot_id,
  model, protocol_version, binary_version, socket_path, oplog_path, heartbeat_path,
  cap_token, started_at, status }` (atomic). Registry entry written **rarely**
  (boot + status change), **not** per heartbeat.
- **Liveness — decoupled (R2-18 hardened).** Two signals, neither churning the
  oplog/registry: (1) the **UDS being connected** (primary), and (2) a **polled
  heartbeat file** the agent updates by a **fixed-size, single-word, aligned
  in-place write** (torn-read-safe; no rename churn) on a dedicated thread, polled
  by the backend at a documented cadence. No mtime dependence.
- **Liveness verdict:** fresh heartbeat **AND** live pid **AND** matching
  `boot_id`/start-time (defeats pid reuse). Else stale → down.
- **Spawn = try-lock-or-adopt**, **allow-list gated with path canonicalization**
  (realpath before matching; reject symlink/`..` traversal out of an allow-listed
  root — R2-15).
- **GC:** registry `*.tmp` reaped by age; stale `stream.sock` unlinked before
  re-binding on boot; the oplog is **compacted** past the acknowledged-`rev`
  barrier (bounds its size; preserves the `seen`-set semantics).
- **Unmanaged agents:** live lock, no registry entry (bridge off / old binary) →
  listed read-only via tier-② files; no command/stream channel.

---

## 11. Agent-side delta (the entire footprint — see §24 for the code map)

### 11.1 Additive module — `cp-mod-bridge` (identical reasoning; added durability latency)
1. **Lock + register + heartbeat** (heartbeat = aligned in-place write on a
   dedicated thread).
2. **Oplog** — open the append-only WAL on a **dedicated oplog thread**; append
   command effects + rev + seen + phase + lifecycle as group-committed entries
   (`fdatasync` per group, I2/I8/I11). **Write immutable content-addressed bodies**
   into the oplog (inline small / `oplog/bodies/{hash}` spilled large, §3.1) — the
   bridge owns this store; `writer.rs` is untouched.
3. **Stream tee** — lock-free SPSC enqueue of each `StreamEvent`; dedicated
   publisher thread serializes + writes `stream.sock` (I7).
4. **Command intake** — verify bearer `cap_token` (I9; HMAC/nonce only at the
   remote seam); journal-then-ack (I11); apply
   the effect via the **existing user-message entry** (K7), never the spine.

**The module does not touch `writer.rs`** — tier-② persistence is unchanged, and
the bridge's body store is an *additional, disjoint* write path (§3.1). But the
inertness claim must be stated honestly: **the agent's *reasoning and decisions*
are identical with the bridge on or off; its *timing* is not.** With the bridge
**ON**, routine message/phase/lifecycle events incur per-event oplog durability
latency (off the hot path via the oplog thread + group commit, but non-zero) and
spilled-large bodies are written twice. Only with the bridge **OFF** does the
agent run **exactly** as today (no oplog, no added latency, no second write path).
V11 measures whether the ON-path latency violates anything the agent's own cadence
cares about.

### 11.2 What v5 does NOT require (vs v4)
v4 demanded a rewrite of the shared `PersistenceWriter` (fsync barrier, collapse
the dual channel, manifest-of-everything) — violating constraint #7 (K4) and
incurring O(S²) amplification (K3). **v5 requires none of that.** The only durable
machinery is the bridge's own oplog. The single agent-side interaction with
existing code is calling the **user-message entry point** to inject a command
effect (additive, K7) and reading the `StreamEvent` channel for the tee (additive,
I7). `flock`/deadman FD inheritance (H5) is the one watchdog touch.

---

## 12. Identity & multi-worker

- **Stable id:** FNV-1a of the canonical folder path (reuses search's scheme) →
  stable across restarts. Folder move/rename ⇒ new id + tombstone.
- **Multi-worker:** an agent may run N internal workers. **Today** the agent drains
  a single `StreamEvent` channel and `build_save_batch` snapshots synchronously on
  one thread → the tee is single-producer and the snapshot is consistent (no
  cross-worker race). **Under the future multi-worker model** (not yet merged),
  each worker has its own stream → **one SPSC ring + one publisher thread per
  worker** (not an MPSC ring; the thread budget is per-worker), and each worker's
  effects append to the shared oplog under the single main loop's `rev`
  assignment. `worker_id` rides every frame and oplog entry.

---

## 13. Failure modes & recovery (summary; full register §20, validation §25)

| Actor | Failure | Detection | Recovery |
|---|---|---|---|
| Agent | Hard crash | stale heartbeat + dead pid; socket closes | replay oplog → rebuild tier-② cache; phase from oplog; offer restart |
| Agent | Mid-append crash | partial last oplog entry | append is atomic-by-fsync; torn tail entry is discarded on replay |
| Agent | Double-launch | flock contention (I1) | 2nd passive |
| Agent | **Deadman re-exec mid-command** | — | command was oplog-journaled before ack (I11) → replayed, deduped by seen (I4) → no loss/dup (K2) |
| Agent | Re-run command post-crash | `seen`-set in oplog (I4), ack-rev evicted | duplicate skipped; exactly-once effect |
| Stream | Slow frontend/backend | bounded buffers | coalesce/drop + degraded flag (I7); agent unaffected |
| Stream | Dropped `MessageStartHint`/`PhaseHint` | — | self-heals from oplog (I8/I10); hint is latency-only |
| Backend | Crash / restart | n/a | rebuild registry + oplog heads (eager), bodies lazy & rev-pinned (I5); re-adopt detached agents; reconnect; clients replay queued actions |
| Backend | **Restart resets CostBreaker?** | — | **no** — cost aggregate is oplog-backed/durable (R2-8) |
| Transport | inotify event dropped | oplog poll backstop | converges within poll; oplog gap-free so no lost rev (K5) |
| Transport | inotify watch exhaustion | — | one watch per agent (I12) → not hit at fleet scale (K8) |
| Frontend | WS disconnect | reconnect + oplog `rev` replay / resync | gap replay or REST refetch; client action queue replays (R2-1 safe) |
| Security | Forged command (v1) | perms + bearer `cap_token` (I9) | rejected + logged; replay/forgery anti at remote seam (G7) |
| Protocol | Version skew | `protocol_version` + per-entry `schema_version` | N-1 major window; **backend upgrades first** (R2-16) |
| Fleet | Runaway spend | durable CostBreaker ceiling | stop issuing commands/spawns; surface |

Backend is **supervised**; spawned agents are **detached** (`setsid`) and
re-adopted.

---

## 14. Sequence diagrams

**Live streaming token (fluid path):**
```
provider →(SSE)→ agent: StreamEvent::Chunk("Hel")
agent: render to typewriter  AND  SPSC enqueue (hot loop, one atomic push)
publisher thread: serialize → stream.sock {MessageStartHint once, then Token seq, "Hel"}
backend: recv → fan-out to N WS (bounded; degraded flag on overflow)
frontend: route by (thread,message); rAF-batch append → paint
… repeat, sub-ms added latency …
agent: StreamDone → tier-② async save (best-effort) + oplog append {message created@rev} (fsync, authoritative)
backend: oplog delta {message@rev, phase→idle} → WS (truth; covers any dropped hint)
```

**Send-message (durable, idempotent, deadman-safe):**
```
UI →(REST/WS auth'd) POST message {text, dedup_token}
backend: bearer-check; append to oplog (fsync) → ack accepted {cmd-id}   (I11: durable BEFORE ack)
agent: bridge sees oplog/UDS wake → inject via USER-MESSAGE entry (NOT spine, K7) → streams (→ live path)
[if deadman re-execs here] → on resume, replay oplog → effect re-derived, deduped by seen (K2) → no loss/dup
agent: oplog appends {lifecycle: processing→done, message created@rev}
```

**Backend restart recovery (lazy, rev-pinned):**
```
scan registry → verify liveness → adopt live / tombstone dead
per live agent: open AgentChannel → tail oplog from HEAD (rev + heads); bodies lazy, rev-pinned
rebuild durable CostBreaker aggregate from oplog
accept frontend WS (auth'd; resync) → clients refetch heads + replay queued actions (dedup-safe)
```

---

## 15. Information to gather FROM agents

Identity/lifecycle; **phase/status** (durable in oplog + live hint); threads (id,
name, MY_TURN/ACTIVE/THEIR_TURN, unread, preview, full conversation on demand,
pending questions); messages + **live deltas** (stream); command lifecycle;
economics (tokens/cost per agent+thread, cache hit/miss/output, context budget —
cost aggregate is durable); every context panel (todos, memories, logs, entities,
spine, queue, scratchpad, tools, callbacks, tree, radar); fleet MY_TURN signals +
total spend + current `rev` + degraded-stream flags.

## 16. Actions to perform ON agents

Send-to-thread (primary); thread create/archive/restore/answer-question; lifecycle
spawn (allow-list, canonicalized)/stop/restart/pause/**interrupt-stream**;
manage rename/model/archive; toggles (auto-continuation/reverie/think);
thread-scoped coucou. All as **bearer-auth'd (v1; HMAC/nonce at the remote seam), dedup-idempotent,
oplog-journaled-before-ack** commands (§8.2), injected via the user-message entry.

---

## 17. Security & permissions

- **Command authn (I9, v7):** v1 = **filesystem perms + presence-checked bearer
  `cap_token`** (256-bit, `0600`, per boot). The same-machine channel is under
  `0700`/`0600`, so perms *are* the authn boundary; **HMAC + nonce + rotation are
  deferred to the remote-transport seam** (G7) because for v1 they guard a strictly
  weaker attacker than the same-user malware perms already can't stop. *(Earned
  simplification — the round-2 review was right that the crypto didn't pay for
  itself locally.)*
- **Frontend WS (I9b/R2-10):** **kept** — `ws://localhost` is reachable by any
  **website** in the user's browser (confused-deputy/DNS-rebind), which perms don't
  cover and `Origin` can't be trusted for. Single-use upgrade ticket → session
  bound to one connection, short-lived + refreshable. No CORS/`Origin` reliance.
- **Spawn (R2-15):** **allow-list with realpath canonicalization** (reject
  symlink/`..` escape). Spawned agents inherit user keys + run user tools (RCE
  blast radius intrinsic to running agents); allow-list + CostBreaker **bound** it;
  sandboxing is future.
- **Global cost circuit-breaker:** aggregate fleet spend, **durable (oplog-backed,
  R2-8)** so a restart/crash-loop cannot reset the ceiling; trips → stop
  commands/spawns, fail-closed on a missing counter.
- **Permissions:** `0700` on `.context-pilot/`, oplog dir, registry dir,
  `stream.sock`; `0600` on the registry entry.
- **Secrets:** registry holds no API keys; `cap_token` is a capability, not an
  external credential.
- **Residual honesty:** `cap_token` defends against *blind* injection, not against
  same-user malware that reads the `0700` registry (needs OS sandboxing, future);
  the spawn RCE blast radius is intrinsic and only *bounded*, not eliminated.

## 18. Versioning & compatibility (R2-16)

`schema_version` on every entry/frame; serde tolerant decode; registry advertises
`protocol_version`. **N-1 major compatibility window**, with the explicit ordering
invariant: **the backend upgrades before any agent** (so it never meets an N+1
agent it must reject). A backend may additionally tolerate N+1 oplog entries
read-only.

## 19. Observability & ops

Per-agent **stream latency p50/p99**, **dropped/coalesced frames + degraded
events**, command queue depth + lifecycle histogram, **`rev` lag** (view vs oplog
head), oplog append latency + `fsync` time, heartbeat freshness, WS subscriber
counts, reconnect/resync rates, **durable CostBreaker state**, rejected-command
(bad-bearer; remote seam: MAC/nonce) counts, **inotify watch count vs limit**. Structured logs keyed by
`agent_id` + `cmd_id` + `rev`.

---

## 20. Problem Register (track ALL problems)

Severity 🔴/🟠/🟡 · Status ✅ designed · 🔵 knob/future. *(v5 rows fold the

> **⚠️ Read this register through §25.** Every ✅ below means **"designed,"
> not "validated."** This is a hypothesis list, not a proof: v4's register was
> *also* fully ✅ and was wrong because it wasn't grounded in code. A problem is
> only **correctness-confirmed** when its §25 fault-injection row (V1–V11) passes.
> Treat ✅ as "designed, test-pending" everywhere in this table.
code-grounded round; see §24 for the code map and §25 for validation.)*

### A. Streaming / latency
| | Problem | Sev | Mitigation | St |
|---|---|---|---|---|
|A1|Disk-tailing too slow for tokens|🔴|UDS stream plane (§7)|✅|
|A2|Nagle/buffering latency|🟠|`TCP_NODELAY`, flush per frame|✅|
|A3|Fan-out O(N) per token|🟠|Per-agent subscriber list|✅|
|A4|Slow frontend backpressures chain|🔴|Bounded WS buffer + degraded (I7)|✅|
|A5|Slow backend stalls agent via tee|🔴|SPSC enqueue + publisher thread (I7)|✅|
|A6|Token reorder / pre-message|🔴|Durable "message created" in oplog + orphan buffer (I10)|✅|
|A7|Background-tab throttling|🟡|Frontend coalesces; refetch on focus|✅|
|A10|Per-token serialize steals CPU|🟠|Tee = one SPSC enqueue (I7)|✅|
|A11|Coalescing = silent UX regression|🟡|degraded flag + periodic oplog snapshot reconcile (R2-17)|✅|
|A12|Browser setState-per-token janks|🟠|rAF batching contract (§7)|✅|
|A13|Ring-full hot-loop behavior|🟡|O(1) fail-fast drop, coherent (R2-13)|✅|
|A14|Publisher on dead UDS spins/wedges|🟡|Non-blocking write + bounded backoff (R2-14)|✅|

### B. Consistency / durability
| | Problem | Sev | Mitigation | St |
|---|---|---|---|---|
|B1|Torn reads|🔴|Content-addressed immutable bodies + atomic oplog append (I2/I3)|✅|
|B0|**Body durable before the entry that names it**|🔴|body-before-reference barrier; orphan-safe via content-addressing (I13, V12)|✅|
|B2|Cross-file inconsistency|🔴|Oplog `rev` + bounded heads (I3)|✅|
|B3|Messages escape the rev (real writer)|🟠|Effects are oplog appends; tier-② is a cache (I8)|✅|
|B5|mtime/clock dependence|🟠|Oplog rev + content hashes, no clock (I3)|✅|
|B7|Power-loss durability|🟡|`fdatasync`/commit-group + `fsync(dir)`/segment, oplog only (I2)|✅|
|B9|**v4 fsync-per-write on shared writer**|🔴|Confine fsync to the tiny oplog; tier-② unchanged (I2, K1)|✅|
|B10|**Manifest O(total-files) → O(S²)**|🔴|Content-addressed bodies + bounded heads, not enumeration (I3, K3)|✅|
|B11|**Debounce coalescing skips revs**|🟠|Oplog is append-only/gap-free; lifecycle on oplog (I8, K5)|✅|
|B12|**rev announced before durable**|🟠|rev = fsync'd oplog offset; announce-after-durable (I8, K9)|✅|

### C. Command delivery
| | Problem | Sev | Mitigation | St |
|---|---|---|---|---|
|C1|Double-execution on crash|🔴|Oplog `seen`-set + atomic append (I4/I8)|✅|
|C2|Ordering|🟠|Sortable seq + append order|✅|
|C3|Lost command|🟠|Oplog is durable inbox; journal-then-ack (I11)|✅|
|C5|TTL-reissue semantic dup|🟠|Same `dedup_token`; ack-rev eviction (I4)|✅|
|C6|Ack hides delay|🟡|Lifecycle deltas on oplog (gap-free) (§8.2)|✅|
|C8|Forged command|🔴|v1: perms+bearer cap_token (I9); HMAC/nonce at remote seam (G7)|✅|
|C9|**Deadman re-exec drops accepted cmd**|🔴|Journal-then-ack; replay+dedup (I11, K2)|✅|
|C10|**Replayable bearer frame**|🟠|v1: perms gate same-user (I9); anti-replay HMAC/nonce deferred to remote seam (G7)|🔵|
|C11|**Dedup window vs long-outage replay**|🔴|`seen` evicted by ack-rev, not time (I4, R2-1)|✅|
|C12|**Spine guards stall backend commands**|🟠|Inject via user-message entry, not spine (§8.2, K7)|✅|

### D. Discovery / lifecycle / identity
| | Problem | Sev | Mitigation | St |
|---|---|---|---|---|
|D1|Ghost registry entries|🟠|Heartbeat+pid+boot_id|✅|
|D2|Double-launch|🔴|flock (I1)|✅|
|D4|Heartbeat on main loop|🔴|Dedicated thread|✅|
|D7|Backend restart kills children|🔴|Detached setsid; re-adopt|✅|
|D10|flock × deadman re-exec|🟠|Inherit lock FD (H5)|✅|
|D11|Heartbeat vs mtime ban|🟡|UDS + polled aligned-word heartbeat file (R2-18)|✅|
|D12|**inotify watch exhaustion**|🟡|One watch per agent on the oplog (I12, K8)|✅|

### E. Backend robustness
| | Problem | Sev | Mitigation | St |
|---|---|---|---|---|
|E1|Backend SPOF|🟠|Supervised + rebuild from oplog (I5)|✅|
|E2|Restart loses in-flight cmds|🟠|Oplog reconcile (dedup-safe)|✅|
|E5|Eager cold rebuild cost|🟠|Lazy bodies; eager heads only (I5)|✅|
|E6|Backend-down blackout|🟡|Client queue + replay (R2-1 safe)|✅|
|E7|**Lazy hydration reads newer rev**|🟠|Rev-pinned content-addressed reads (I5, R2-9)|✅|
|E8|**CostBreaker resets on restart**|🟠|Durable oplog-backed aggregate (R2-8)|✅|

### F. Frontend consistency
| | Problem | Sev | Mitigation | St |
|---|---|---|---|---|
|F1|Event before snapshot|🟠|Client monotonic rev|✅|
|F2|Reconnect after sleep|🟠|Oplog rev replay / resync|✅|
|F3|Optimistic UI hangs|🟡|Lifecycle on oplog + TTL|✅|
|F4|Duplicate/orphan tokens|🟡|Bounded orphan buffer + oplog header (I10, R2-3)|✅|

### G. Security
| | Problem | Sev | Mitigation | St |
|---|---|---|---|---|
|G1|Command injection|🔴|`0700`/`0600` perms + presence-checked bearer cap_token (I9)|✅|
|G2|WS unauth (CORS≠WS)|🔴|Single-use ticket → bound session (R2-10)|✅|
|G3|Spawn RCE amplifier|🟠|Allow-list + canonicalization (R2-15)|✅|
|G4|Runaway spend|🟠|Durable CostBreaker (R2-8)|✅|
|G6|cap_token at-rest exposure|🟡|Bearer cap (not a frame key); per-boot mint; only blind cross-talk defended — same-user malware needs sandboxing (I9)|🔵|
|G7|Network/multi-tenant future|🟡|mTLS/signed cmds at transport seam|🔵|

### H. Ops / versioning
| | Problem | Sev | Mitigation | St |
|---|---|---|---|---|
|H1|Upgrade w/ live agents|🟠|N-1 window + backend-upgrades-first (R2-16)|✅|
|H2|No observability|🟠|Metrics + correlated logs (§19)|✅|
|H5|**Phase latency from #6 fix**|🟠|Durable oplog phase + live hint, self-healing (I8/I10, K6)|✅|

### I. Edge / correctness / process
| | Problem | Sev | Mitigation | St |
|---|---|---|---|---|
|I1|Multi-worker→thread mapping|🟡|Flatten; worker_id metadata|✅|
|I4b|Giant blobs over stream|🟠|Reference by hash; REST fetch|✅|
|I7b|**I8/H3 rewrites shared writer (#7)**|🔴|Bridge-owned oplog; writer.rs untouched (I8, K4)|✅|
|I8b|UTF-8 split mid-token|🟡|Concatenate by message_id|✅|
|M1|**✅ "designed" ≠ "validated"**|🔵|§25 fault-injection acceptance matrix (R2-19)|🔵|

---

## 21. Open questions (genuine choices)

1. **Oplog format:** ~~framed vs JSON-lines?~~ **LOCKED (v6): length-prefixed
   framed records + per-record CRC** — the 🔴 torn-tail invariant (V1) depends on
   CRC to discard a partial tail, so this cannot stay a "lean."
2. **Oplog compaction trigger:** size threshold vs ack-rev barrier vs both?
3. **Stream-plane transport:** UDS v1; shared-memory ring future — confirm?
4. **Partial-message checkpoint cadence** on a long degraded stream (R2-17): every
   N tokens / M ms?
5. **Backend stack = Rust** (reuse serializable types) — confirm?
6. **Spawn allow-list source:** config file vs "register this folder" UI vs both?
7. **Inline-vs-spill body size threshold `T`** (I13): below `T` a body is inlined
   into the oplog entry (zero double-write); above `T` it spills to
   `oplog/bodies/{hash}` under the barrier. Pure tuning (oplog size/replay cost vs
   double-write frequency) — *not* a correctness question; inlining is always safe.

*(Resolved since v4: durability model = three tiers w/ a bridge-owned oplog;
exactly-once = oplog append; command auth = perms+bearer (v1); rev = oplog offset;
control observation = single watch per agent.)*

## 22. Decision log (v5 deltas + retractions)

- **2026-06-16 · v7 body-before-reference (I13)** · *Locked:* a content hash may
  appear in a durable oplog entry only after its body is durable; small inlined,
  large spilled-then-fsync'd-before-the-entry; crash-in-gap = orphan body (GC'd),
  never a dangling reference. *Rationale:* closes the round-2 gap — "accepted =
  durable" must cover *contents*, not just the envelope. Tier-② copy declared
  non-load-bearing for durability.
- **2026-06-16 · v7 oplog execution model** · *Locked:* the main loop **never
  fsyncs** — it lock-free-enqueues each record (incl. phase transitions); the
  dedicated oplog thread group-commits (one `fdatasync`/group); command-accept ack
  runs on the intake/oplog thread. *Rationale:* round-2 asked which thread appends
  run on; fsync-per-phase-on-loop would violate I7's latency budget.
- **2026-06-16 · v7 security subtraction** · *Locked:* v1 command authn = perms +
  presence-checked bearer `cap_token`; **HMAC/nonce/rotation moved to the remote
  seam** (G7). WS single-use ticket **kept**, re-justified as browser
  confused-deputy defense (not loopback). *Rationale:* round-2 was right — local
  crypto guarded a strictly-weaker attacker than same-user malware (which perms
  can't stop either); it didn't earn its keep. Earned simplification, not a
  capitulation: the seam survives, the cost doesn't.
- **2026-06-16 · v6 body-store ownership** · *Locked:* the **bridge** owns an
  immutable content-addressed body store (inline-small / `oplog/bodies/`-spilled);
  tier-② `messages/{uid}.yaml` stays the **mutable** agent cache (§3.1).
  *Rationale:* v5 called the same files both mutable and immutable — impossible.
  Cost named: a second (disjoint) write path; spilled-large bodies double-written.
  I1 intact (cross-process, not intra-thread).
- **2026-06-16 · v6 fsync model** · *Locked:* `fdatasync` per commit-group +
  `fsync(dir)` only on segment creation, on a dedicated oplog thread (I2).
  *Rationale:* per-append `fsync(file)+fsync(dir)` was unjustified syscall cost;
  durability belongs off the hot path.
- **2026-06-16 · v6 inertness honesty** · *Restated:* "exactly as today" holds
  only **bridge-OFF**; bridge-ON adds bounded, off-hot-path per-event durability
  latency (§11.1), measured by V11. *Rationale:* "behaviorally inert" conflated
  identical *logic* with identical *timing*.
- **2026-06-16 · v6 Open-Q1** · *Locked:* framed records + per-record CRC.
  *Rationale:* V1 (torn-tail) depends on it; a 🔴 invariant cannot rest on an open
  question.
- **2026-06-16 · v6 register discipline** · *Adopted:* §20 ✅ = "designed,
  test-pending"; §25 is the real status. *Rationale:* an all-✅ pre-code register
  is argument-completeness, not correctness (v4 proved it).
- **2026-06-16 · Durability model** · *Locked (v5):* three tiers — bridge-owned
  append-only `fsync`'d **oplog** (truth) + existing best-effort **state cache** +
  lossy **stream**. *Rationale:* the real `writer.rs` is async/debounced/coalescing/
  unsynced; only a tiny set of events needs exactly-once, so confine durability to
  the oplog and leave the shared writer untouched (K1/K4).
- **2026-06-16 · Manifest** · *Locked (v5):* content-addressed immutable bodies +
  bounded current-heads, **not** full enumeration. *Rationale:* v4's
  enumerate-everything manifest was O(S²) write-amplification (K3).
- **2026-06-16 · "Accepted" = durable** · *Locked (v5):* journal-to-oplog-then-ack
  (I11). *Rationale:* deadman re-exec would otherwise vaporize a UDS-accepted
  in-memory command (K2).
- **2026-06-16 · Command injection path** · *Locked (v5):* the existing
  user-message entry, **not** the autonomy spine. *Rationale:* spine anti-loop
  guards would stall/swallow external commands (K7).
- **2026-06-16 · Control observation** · *Locked (v5):* single inotify watch per
  agent on the append-only oplog. *Rationale:* per-file watches exhaust inotify at
  fleet scale (K8).
- **2026-06-16 · Phase** · *Locked (v5):* durable in oplog + live stream hint,
  self-healing. *Rationale:* the v4 phase→durable fix added 2–3 s phase latency
  (K6).
- **2026-06-16 · Command freshness** · *Locked (v5), **superseded by v7**:* HMAC +
  monotonic nonce (R2-6) → **deferred to the remote seam**; v1 freshness is moot on
  a `0700`/`0600` local channel (perms gate the same-user boundary, I9). · **Dedup
  eviction** · ack-rev, not time (R2-1). · **CostBreaker** ·
  durable/oplog-backed (R2-8). · **WS auth** · single-use ticket → bound session
  (R2-10). · **Lazy hydration** · rev-pinned (R2-9). · **Upgrade** · backend-first
  ordering (R2-16). · **Allow-list** · canonicalized (R2-15).
- **2026-06-16 · RETRACTIONS** (code proved them wrong): *cross-worker snapshot
  consistency* is a non-issue — `build_save_batch` snapshots synchronously on one
  thread. *SPSC-vs-multi-worker* is scoped to the future multi-worker model only;
  today the agent drains a single `StreamEvent` channel (single-producer).
- *Provisional (unchanged):* read transport, identity (FNV-1a), backend stack
  (Rust), discovery (registry + heartbeat + flock).

---

## 23. Adversarial review resolution (round 1 — retained)

The v4 §23 map of the 18 round-1 issues → resolutions is retained verbatim in git
history (commit `e2c97e9`); every row remains ✅ or is *strengthened* by the v5
oplog model (e.g. #1/#6/#9/#13 now resolve via the oplog rather than a shared-
writer rewrite). The round-2 issues (R2-1…R2-19) and the code-grounded round-3
issues (K1…K9) are folded into §20 above and mapped to code in §24.

## 24. Code-grounding pass — invariant → real code → impact

The critique that produced v5: *the design was written against an imagined
persistence layer.* This section maps each load-bearing claim to the **actual
file/function** it depends on, and states whether the agent-side change is
**additive** (constraint #7 safe) or **core** (forbidden).

| Invariant / mechanism | Real code it touches | Change type | Notes |
|---|---|---|---|
| I8 oplog | NEW `crates/cp-mod-bridge` only | **additive** | does **not** touch `writer.rs` |
| I2 fsync | oplog append in the bridge | **additive** | `writer.rs::write_file` (plain `fs::write`) **unchanged** |
| I3 heads/content-addr | bridge serializer; bridge **writes+fsyncs** authoritative immutable bodies (§3.1) | **additive** | `save.rs::build_save_batch` unchanged; the heads the bridge records reference its **own** fsync'd bodies (I13), not the tier-② cache |
| I13 body-before-reference | bridge oplog thread (body durable → then entry) | **additive** | content-addressed ⇒ a crash in the gap is an orphan body, never a dangling reference |
| Tier-② cache | `writer.rs` (async, 50 ms debounce, coalescing) | **unchanged** | explicitly left as-is |
| I11 journal-then-ack | bridge command intake | **additive** | UDS is wake-only |
| K7 command injection | `src/app/actions/input.rs` user-message entry (clears `user_stopped`) | **additive call** | bridge calls it; `engine.rs::check_spine` **untouched** |
| I7 tee | `streaming.rs::process_stream_events` (single channel, main loop) | **additive read** | one SPSC enqueue at the existing drain point |
| H5 flock × deadman | the deadman re-exec (`CommandExt::exec`, `--resume-stream`) | **core-adjacent** | clear `FD_CLOEXEC`, pass `CP_AGENT_LOCK_FD`; the one watchdog touch |
| I12 single watch | backend side only (`notify`/inotify) | **backend** | no agent impact |
| Phase hint (K6) | bridge tee + oplog | **additive** | live hint on ③, truth on ① |

**Verdict:** v5's agent-side footprint is **one additive module + one additive
call into the existing user-message entry + one additive read at the stream drain
+ the H5 watchdog FD tweak.** The shared `PersistenceWriter` and the autonomy
`spine` are **not modified** — constraint #7 is satisfied *for real*, not by
relabeling a rewrite as "hardening" (the v4 mistake, K4).

## 25. Fault-injection acceptance matrix (production-v1 gate)

"Designed" ≠ "validated." Each 🔴/🟠 durability/concurrency/security mechanism must
pass a fault-injection test before its §20 status is trusted (R2-19/M1).

| # | Mechanism | Fault injected | Pass criterion |
|---|---|---|---|
| V1 | I8 oplog append | `kill -9` between `write` and `fsync` | replay discards torn tail; no half-effect |
| V2 | I11 journal-then-ack | deadman re-exec after ack, before stream | effect replayed exactly once; no false-accept loss |
| V3 | I4 dedup | replay same `dedup_token` after a 2-h simulated outage | second apply is a no-op |
| V4 | I2 durability | power-cut (fsync fault) after dir fsync | committed `rev` fully readable; uncommitted absent |
| V5 | I10 ordering | drop + reorder stream frames; drop `MessageStartHint` | UI reconstructs from oplog; no orphan leak (bounded buffer) |
| V6 | I9 auth | v1: command with missing/invalid bearer `cap_token`; remote seam: replay a captured frame / tamper body | v1: bad-bearer rejected; remote seam: rejected (stale nonce / bad MAC) |
| V7 | I7 backpressure | stall the WS consumer; fill the ring | agent loop latency unaffected; degraded flag set |
| V8 | I12 / K8 | spawn 10k agents | inotify watch count ≈ agent count; no exhaustion |
| V9 | R2-8 CostBreaker | crash-loop the backend at the spend ceiling | breaker stays tripped (durable counter) |
| V10 | K5 gap-free | coalesce tier-② saves under load | oplog rev stream has no gaps; lifecycle "processing" observed |

Until a row passes, its §20 entries are **"designed, test-pending,"** not ✅.

### v6 additions

| # | Mechanism | Fault injected | Pass criterion |
|---|---|---|---|
| V11 | I2 / §11.1 bridge latency + loop-fsync | (a) bridge-ON vs bridge-OFF scripted session; (b) **burst of phase transitions** during streaming | (a) added p99 below the agent's turn-cadence budget, bridge-OFF byte-identical to today; (b) the per-transition op is a lock-free enqueue — **loop tick time statistically unchanged** under the burst (no fsync on the loop) |
| V12 | I13 body-before-reference barrier | `kill -9` **between** a spilled body's `fdatasync` and its referencing oplog entry's commit (and after the entry commits) | crash-in-gap ⇒ orphan body, **no durable entry references a missing body**; crash-after ⇒ body present; replay never yields a dangling head-hash |

**First prototype (do this before more doc):** implement the isolated oplog
append + crash-replay loop and pass **V1, V2, V10** against real `kill -9` and a
simulated deadman re-exec. ~few hundred lines; it falsifies (or confirms) the
load-bearing durability claim faster than any further revision.
