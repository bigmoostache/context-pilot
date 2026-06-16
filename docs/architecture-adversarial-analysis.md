# Orchestration Backend — Adversarial Analysis

> **Purpose.** A deliberately hostile review of
> [`design-orchestration-backend.md`](./design-orchestration-backend.md) (v3 —
> two-plane architecture). The goal is to surface **every** way the design could
> be *less robust*, *add latency*, *introduce vulnerabilities*, or hide a *grey
> area*, before any of it is built. Constructive-critical: each issue ships with
> a concrete fix.
>
> **Verdict up front.** The architecture is right. The four issues a v1 should
> block on are: the **commit transaction boundary** (#1), **command + WebSocket
> authentication** (#2/#3), and **cross-plane ordering + sticky-state-on-the-
> lossy-plane** (#4/#6). Everything else is designable-against now.
>
> Severity: 🔴 critical (correctness/security — block v1) · 🟠 high · 🟡 medium.
> Section refs (§N, I-N, A-N…) point into the design doc.

---

## 0. What is sound (calibration)

So the teardown is calibrated, not nihilistic — these decisions are correct and
should be kept:

- **The two-plane split** (durable control plane + ephemeral stream plane, §3) is
  the right keystone. It is what lets streaming be fast *and* state be safe.
- **Manifest-over-mtime** (I3) is the correct consistency primitive — no clock
  dependence, doubles as an incremental-read index.
- **I7** (the live plane must never backpressure the agent) is the right instinct.

The skeleton holds. The seams below are where it leaks.

---

## 1. 🔴 Critical — correctness & security holes

### 1.1 — I6's "atomic effect + seen mark" is contradicted by the actual writer
The design (B3) admits messages persist via a **separate, non-debounced**
`WriterMsg::Message` path that bypasses the rev batch, while the rev/manifest is
written on the debounced (50 ms) `Batch` channel. There is **no ordering
guarantee** between the two channels. Consequences:

- the manifest can reference a message file that is **not yet in** the committed
  rev, or
- a message can land on disk with **no corresponding rev bump**.

I6 claims an atomicity the current architecture cannot deliver, and the
mitigation in B3 ("route into batch") is a one-liner where it needs to be a
*designed transaction*.

**Fix.** Introduce a real single-writer **commit primitive**: a command's effect
(new message + spine notification), its `seen`-ledger mark, and the `rev`/manifest
bump all flow through **one** writer transaction — not the existing dual
`Batch`/`Message` channels. Until that primitive exists, exactly-once-effect is
aspirational.

### 1.2 — Command authentication is filesystem permissions only
"Commands carry `agent_id`, rejected if mismatched" (G1) is **not** authentication
— `agent_id` is a self-asserted field in the frame/file. `0700` perms only stop
*other Unix users*. Any process running **as the same user** — a malicious
`npm postinstall`, a compromised transitive dependency in *any* of the user's
projects — can write to `inbox/` or connect to `stream.sock` and **puppet the
entire fleet**: send messages, archive threads, and burn API spend.

For a tool shipped to clients this is a local privilege-escalation and
financial-DoS surface, and G1 understates it.

**Fix.** Per-agent **capability token**: minted at boot, stored inside the `0700`
registry entry, **required on every command** (UDS frame *and* `inbox/` file).
Treat the backend as the only legitimate command author.

### 1.3 — The frontend WebSocket is effectively unauthenticated
"Localhost-bind + CORS" (G3) is a known-weak posture for WebSockets: **CORS does
not protect WS.** The `Origin` header is advisory and trivially spoofed by any
non-browser local client. A malicious localhost process opens `ws://localhost:port`
and drives the fleet.

**Fix.** A real **bearer token in the WS handshake** (subprotocol or first-frame
auth), minted by the backend and delivered to the frontend out-of-band. Reject
connections without it. Do not rely on `Origin`/CORS.

### 1.4 — Cross-plane ordering is undefined; tokens can precede the message
`StreamFrame.seq` is **per-plane**. The "message created" notice rides the durable
(slow, disk) control plane; the first token rides the fast (UDS) stream plane.
The browser can therefore receive **tokens for a `message_id` it has never heard
of**. The design never states the resolution rule.

**Fix.** Pick one and write it down:
- (a) the **first** stream frame for a message is self-describing (carries enough
  to lazily create the buffer), or
- (b) the frontend **buffers orphan tokens** by `message_id` until the control
  delta arrives.

---

## 2. 🟠 Robustness gaps

### 2.1 — The "tee" is non-blocking but not free
I7 protects against *backpressure*, not *CPU steal*. Serializing a `StreamFrame`
per token, on the agent's main loop, across many workers, is measurable jitter on
the render/persist cadence. "Behaviorally inert" ≠ zero-cost.

**Fix.** The tee does **one lock-free push** to an SPSC ring; a **dedicated
publisher thread** does the serialize + socket write. The hot loop stays at one
atomic enqueue.

### 2.2 — Sticky state routed on the lossy plane
`StreamFrame.kind: Phase` puts phase transitions (idle / streaming / tooling) on
the **droppable** stream plane. Drop the "stream ended" phase frame → the UI is
**stuck "streaming" forever.** Latched/sticky state on a best-effort channel is a
bug class.

**Fix.** Phase belongs on the **durable control plane** (with `rev`); the stream
plane carries only ephemeral deltas.

### 2.3 — `flock` × the deadman re-exec is unanalyzed
The agent already performs a re-exec under its deadman watchdog
(`CommandExt::exec`). `flock` is tied to the open file description; whether the
lock survives re-exec depends on `CLOEXEC` and FD inheritance. Get it wrong and
you either **deadlock the agent against its own lock** or open a **double-writer
window** during re-exec. The design proposes `flock` (I1 / H3) without reckoning
with the existing re-exec path.

**Fix.** Explicitly **inherit the lock FD** across re-exec (no `CLOEXEC`, pass the
FD number), or re-acquire with a documented contention window + adopt logic.

### 2.4 — Power-loss durability is under-specified
I2 says `tmp → fsync → rename`, but the manifest commit needs more: fsync
**every** batch data file *before* the manifest rename, then **fsync the
directory** to durably record the renames. Without the directory fsync, a
renamed-and-durable manifest can point at content lost from the page cache on
power loss.

**Fix.** State the full barrier: data fsyncs → manifest rename → **dir fsync**.

### 2.5 — Backend cold-rebuild cost is unbounded
I5 ("rebuildable cache") with **eager** rebuild means restart latency scales with
*total fleet disk* (one agent's tree alone can be tens of KB of tokens; message
logs can be MBs). Lazy materialization is filed as a 🔵 *future* knob — so v1 has
a real availability gap on every deploy/crash.

**Fix.** Make **lazy materialization v1**, not future: eagerly rebuild only the
registry + each agent's `rev`/manifest head; hydrate bodies on demand.

---

## 3. ⏱ Latency adders

### 3.1 — The latency floor is the browser, and it's declared out of scope
End-to-end there are three serialization hops (agent → UDS → backend → WS →
browser) **plus React reconciliation**. `setState` per token at high token rate
will jank or drop frames. The "fluid as fuck" target lives or dies on the
**client render strategy** (batch tokens into `requestAnimationFrame`, never
`setState`-per-token) — which the backend doc punts on (A7). You cannot claim the
UX target while excluding the component that determines it.

**Fix.** Make the **rAF-batching rule a first-class requirement**, even though it
is frontend.

### 3.2 — Coalescing is a latency/UX regression dressed as robustness
When the bounded WS buffer overflows (and on a busy fleet the single backend
fan-out **will** be the chokepoint), coalescing produces a visible
**stall-then-jump**. Given "any added millisecond is thousands of users," the
design should *admit* this trade-off and define **when degradation becomes a
user-visible "stream degraded" indicator** rather than silent jank.

### 3.3 — "Accepted" ack hides an unbounded effect delay
The two-phase ack (§8.2) returns fast, but the **effect** waits for a stream/tool
safe-point — the optimistic UI shows "accepted" while nothing happens for seconds.

**Fix.** Surface the **real command lifecycle** (`queued → delivered →
processing`) in the UI, not just the accept-ack, so the user sees "queued behind
a running turn."

---

## 4. 🌫 Grey areas

### 4.1 — Semantic idempotency is missing
TTL-expiry → reissue with a **new id** means a merely-*slow* (not lost) command
gets a second id; the `seen`-ledger keys on transport id, so **both execute →
double message.** The "`seen`-window > TTL" rule (C5) only prevents resurrecting
the *old* id.

**Fix.** A **client-supplied dedup token** (semantic key), deduped independently
of the transport id.

### 4.2 — Backend-down = silent action blackout
Detached agents survive a backend restart (good), but frontend actions **during**
backend downtime have nowhere to go — the browser speaks REST/WS to the *backend*,
not `inbox/`. The "just restart, near-stateless" framing understates that
**in-flight user intent vanishes** during the gap.

**Fix.** State it explicitly; optionally let the frontend degrade to writing
`inbox/` directly, or queue + replay actions on reconnect.

### 4.3 — `rev` assignment under multi-worker is unstated
Many modules are per-worker, yet the manifest/`rev` is per-agent. What serializes
the counter across N concurrent workers? (It is *probably* fine — single main
loop, single `PersistenceWriter` — but the design never says the `rev` is assigned
by the single writer and that the manifest captures a **consistent cross-worker
snapshot**.)

**Fix.** Write the invariant down: the single persistence actor assigns `rev` and
snapshots all workers atomically per batch.

### 4.4 — Heartbeat mechanism contradicts I3
Frequent atomic-rename of the registry entry per heartbeat = inode churn + a
**watcher storm** on the backend. But the obvious alternative (mtime touch) is
**banned by I3** (no clock/mtime dependence). The mechanism is unspecified and
the two requirements are in tension.

**Fix.** Heartbeat on a **separate channel** (the UDS, or a dedicated heartbeat
file the backend **polls**, not watches), decoupled from the manifest/registry
rename path.

### 4.5 — The upgrade window contradicts itself
§18 says **both** "tolerant decode" *and* "reject unknown major." During the
rolling binary upgrade the design explicitly supports (long-lived agents,
upgrade-in-place), a new backend will **hard-reject** old agents (or vice versa),
orphaning live agents mid-task.

**Fix.** Support **N-1 major** compatibility, not hard reject.

### 4.6 — Spawn blast radius + no global cost breaker
The supervisor execs `cp --headless` inheriting the user's API keys; a
compromised or buggy backend becomes an **RCE amplifier** (spawned agents run
file/console tools as the user) with **no global cost ceiling** — per-worker guard
rails do not bound a backend issuing commands in a loop.

**Fix.** A backend-level **cost circuit-breaker** + a **spawn allow-list** (which
folders may host agents).

---

## 5. Priority summary

| # | Issue | Bucket | Sev | One-line fix |
|---|---|---|---|---|
| 1 | Effect+seen+rev not one transaction | robustness | 🔴 | single-writer commit primitive |
| 2 | Command auth = perms only | vulnerability | 🔴 | per-agent capability token on every command |
| 3 | WS auth = localhost+CORS | vulnerability | 🔴 | bearer token in WS handshake |
| 4 | Cross-plane token/message ordering | robustness | 🔴 | self-describing first frame OR buffer orphans |
| 5 | Tee CPU steal on hot loop | latency | 🟠 | SPSC ring → dedicated publisher thread |
| 6 | Phase (sticky) on lossy plane | robustness | 🟠 | move phase to durable control plane |
| 7 | flock × deadman re-exec | robustness | 🟠 | inherit lock FD across re-exec |
| 8 | Power-loss: no dir fsync | robustness | 🟡 | data fsyncs → manifest rename → dir fsync |
| 9 | Eager cold rebuild cost | robustness | 🟡 | lazy materialization in v1 |
| 10 | Browser render floor out of scope | latency | 🟠 | mandate rAF token batching |
| 11 | Coalescing = silent UX regression | latency | 🟡 | user-visible "degraded" signal |
| 12 | "Accepted" hides effect delay | latency | 🟡 | surface full command lifecycle |
| 13 | No semantic idempotency | grey | 🟠 | client dedup token |
| 14 | Backend-down action blackout | grey | 🟡 | state it; queue+replay on reconnect |
| 15 | Multi-worker rev serialization | grey | 🟡 | document single-writer rev invariant |
| 16 | Heartbeat vs I3 (mtime ban) | grey | 🟡 | poll a dedicated heartbeat file / UDS |
| 17 | Upgrade hard-reject vs tolerant | grey | 🟠 | N-1 major compat window |
| 18 | Spawn RCE + no cost breaker | vulnerability | 🟠 | cost circuit-breaker + spawn allow-list |

**Bottom line.** Block v1 on #1–#4 (and #6). Fold the rest into a v4 of the design
(new invariants for the commit transaction + auth; a cross-plane ordering rule;
phase → control plane).
