# Wiring Alignment Audit — implementation ⇄ `design-orchestration-backend.md`

> **Purpose.** Make the **running wiring** embody the design doc's architecture
> *philosophy* — not merely contain its primitives. This is the deployment gate.
>
> **Scorecard semantics.** Each load-bearing invariant / keystone / clause is
> rated **ABIDES** (implemented faithfully + wired live), **PARTIAL** (wired for
> the common path but a sub-case or validation remains), **VIOLATES**
> (contradicts the doc), or **N-A**.
>
> **Re-audit: 2026-06-18 (Phase 5).** Branch: `demaquetting`. This supersedes the
> Phase 0.1 baseline (which was written *before* the push plane was connected).
> The intervening work — agent delta emission (`b3a6544` roster, `e16411b`
> phase+cost, `05349b1` MessageCreated), `/threads` served from the view
> (`f54a5f4`), SSE rev-numbered deltas + `live.ts` `applyThreadDelta`/
> `applyAgentDelta`, and the `TAIL_REPOLL=5ms` tailer-primary latency fix —
> **connected the orchestration plane end-to-end**. Most rows that read VIOLATES
> in the baseline now read ABIDES, with evidence below.

---

## 0. The one-sentence verdict (re-audit)

The **orchestration plane is now the live path**: every user-visible mutation
(thread create/archive/restore, message created, phase, cost) is appended to the
agent's oplog the instant it applies, folded by the backend `MaterializedView`,
pushed as a rev-numbered SSE delta, and applied in-place by the frontend store —
**command→visible p50 ≈ 14 ms** (measured this session, down from "seconds"). The
tier-② disk files are back in their designed role as a disposable cold-start
cache. The inversion that *was* the latency problem is corrected. What remains is
**completeness, not architecture**: live *token* streaming (§7), carrying the
roster inside checkpoints (cold-restart-after-compaction), a `Lifecycle` emit,
and the §19 observability surface.

---

## 1. The two planes (the philosophy being measured)

| Plane | Doc intent | Carries | Speed | State |
|---|---|---|---|---|
| **A — Orchestration (PUSH)** | `oplog → backend Tailer (1 inotify/agent) → in-memory MaterializedView → rev-numbered SSE deltas → frontend APPLIES deltas` | threads, messages, phase, MY_TURN, cost | sub-50ms | **CONNECTED** ✅ |
| **B — State cache (tier ②)** | a **lazily-rebuildable, disposable** cache (I5) | cold-start hydration + low-churn inspection reads (memory/todos/tree/callbacks/entities) | debounced disk | **demoted to its designed role** ✅ |

The baseline's sin ("plane B is the live path; plane A is inert") is **fixed**.

---

## 2. Invariant-by-invariant gap register (re-audit)

| # | Invariant (doc) | Verdict | Evidence (file:line) | Remaining gap |
|---|---|---|---|---|
| **I1** | Single writer *process* per folder (flock) | **ABIDES** | `cp-mod-bridge/src/boot.rs` `acquire_lock` (+ bounded retry) | — |
| **I2** | Main loop never fsyncs; dedicated oplog thread group-commits | **ABIDES** | `src/app/run/threads/bridge.rs:148` `emit_roster_delta`→`submit_durable` (non-blocking durable); `:186` `append_best_effort`; `messages.rs:156` `submit_durable`. Loop only enqueues. | V11 explicit "burst leaves tick time unchanged" test (X844) |
| **I3** | Snapshot = bounded heads + content-addressed bodies | **PARTIAL** | `materialized_view.rs` `AgentView{heads,roster}`; heads populated by `MessageCreated` | `Checkpoint`/`Snapshot` restores **heads only, not the roster** (`materialized_view.rs:108` note) — see I5 cold-restart gap |
| **I5** | Tier ② is a lazily-rebuildable cache; live reads come from the view | **ABIDES** | `transport/rest/mod.rs:227` `/threads` served **roster-first from `backend.view`** (`overlay_roster` merges view roster onto disk log; view-only threads synthesised instantly); `/fleet`,`/agent` from view. Low-churn inspection reads (`panels.rs:1,4`, memory/todos/tree/…) stay tier-② **by the doc's documented allowance**. | cold-start view **roster** hydration (X850) so a backend restart after oplog compaction doesn't briefly under-report the roster |
| **I8** | Command effects, rev, **phase, lifecycle, cost** are oplog appends | **PARTIAL** | `bridge.rs:291/325/338` `ThreadCreated/Archived/Restored`; `:216/230` `PhaseTransition`/`CostAggregate`; `messages.rs:156` `MessageCreated` — all emitted on apply | only **`Lifecycle`** (boot/shutdown state) is not yet emitted (X842) — everything else ABIDES |
| **I10** | Durable "message created" in oplog; stream `MessageStart` is a hint | **PARTIAL** | durable side ABIDES: `messages.rs` `emit_messages` → `MessageCreated` + I13 body store; frontend `live.ts` `applyThreadDelta` `message_created` appends to the log | the **stream-hint side** (live token paint) is not yet consumed by the frontend (§7 / Phase 7) |
| **I11** | "Accepted" = durable (journal-then-ack) | **ABIDES** | `cp-mod-bridge/src/command.rs` `handle_frame` (append_durable before ack) | — |
| **I12** | One inotify watch per agent on the oplog; 2–3s poll is a backstop | **ABIDES** | `transport/mod.rs:394` `OplogWaiter.wait(TAIL_REPOLL)` — inotify/FSEvents primary, `:71` `TAIL_REPOLL=5ms` tight backstop; `runtime.rs` mtime scan demoted to a dirty→`invalidate` backstop for inspection resources only | (the `invalidate` backstop is the documented transitional fallback — removed per-resource in X859) |
| **I13** | Body-before-reference barrier; immutable content-addressed body store | **ABIDES** | `messages.rs` `emit_one_message`: `store.put(body)` (I13 barrier — inline small / spill+fdatasync large) **before** `submit_durable(MessageCreated)`; `cp-mod-bridge/src/body.rs` `Store` | — |
| **§7** | Stream plane: SPSC tee → publisher thread; rAF token batching mandatory | **PARTIAL** 🟠 | agent side built: `cp-mod-bridge/src/tee.rs` publishes `Token` frames; backend `StreamHub` fans out | **frontend does not yet consume the `stream` SSE channel** into the conversation view, and there is no rAF token buffer (Phase 7: X853/X857/X861). Live *phase* is shown; live *typing* is not. |
| **§9** | SSE carries rev-numbered, replayable, gap-free deltas; client applies | **ABIDES** | `transport/mod.rs:346` `SseMessage::delta(entry.rev, data)` per `OpEntry`; `web/src/lib/live.ts` `applyThreadDelta`/`applyAgentDelta` apply in-place with a monotonic-rev high-water guard | `invalidate` fallback still present for inspection resources (cleanup X859) |
| **§18** | schema_version + N-1 compat + Unknown tolerance | **ABIDES** | `cp-wire` all types `schema_version`'d, `#[serde(other)] Unknown`; roster variants added with tolerant-decode tests | — |
| **§19** | Observability: latency p50/p99, dropped frames, rev lag, fsync latency, watch count | **VIOLATES** 🟠 | no metrics surface exists | stand up the §19 surface (X868) |

---

## 3. Frontend feature wiring (T120 enumerated contract) — VERIFIED

Every feature the user enumerated is wired **and** proven by a live Playwright
e2e test (14 tests, all green against web `:5175` + orchestrator `:7878` + this
agent's bridge — no mocks, no screenshots):

| Feature | Wiring | e2e proof (`web/e2e/`) |
|---|---|---|
| Threads create / archive / restore | `ThreadsView` → `sendCommand` → bridge → oplog → view → SSE delta → `applyThreadDelta` | `threads.spec.ts` (3) — UI action + roster ground-truth |
| Send message in a thread | composer → `send_message` (K7) → `emit_messages` → `MessageCreated` delta → log append | `messages.spec.ts` (1) — user bubble live + roster log grows |
| Token / dollar cost in the footer | `CostAggregate` delta → `applyAgentDelta` → `costUsd`; footer + fleet card | `cost.spec.ts` (2) — both surfaces within drift of `/meta` |
| Finder — every button/option | `useFs` live realm listing; nav, breadcrumbs, child counts, 4 view modes, pins, download | `finder.spec.ts` (4) — listing == backend `/fs`, nav, view modes |
| (harness) live pipe | — | `smoke.spec.ts` (4) |

A real product bug was found and fixed during finder testing: live (realm-root-
relative) navigation paths left the cwd relative, collapsing the breadcrumb and
breaking go-up; normalised to absolute paths (`Finder.tsx` `toAbs`, `26d6f3c`).

---

## 4. Measured proof (the "why it's now fast")

- Command **journaling**: ~14 ms p50 / ~33 ms p99 *to visible in the browser*
  (measured this session via the latency probe, agent `f3a993c0ff357b41`), down
  from the baseline's "seconds". Floor = the durable journal-then-ack fsync (I11).
- Path: `agent applies → submit_durable enqueue (no loop fsync) → oplog group-commit
  off-loop → backend Tailer.poll (≤TAIL_REPOLL 5ms) → MaterializedView fold → SSE
  delta → frontend applyDelta (zero refetch)`.
- The former "3-layer invalidation band-aid" is now a **fallback**, not the
  mechanism: the data is on the oplog (and in the delta) the instant it changes.

---

## 5. Honest remaining ledger (post-feature-completion)

The four enumerated frontend features are **done and e2e-verified**. The
following are **alignment-completeness** items — the deeper X818 invariants that
make abidance 99.9% rather than "the user's features work":

1. **§7 live token streaming (Phase 7, biggest)** — phase is live, but assistant
   *tokens* don't yet paint in real time. Needs: SSE `stream` channel consumed in
   `live.ts`, a per-message rAF token buffer, conversation-view wiring, and the
   backend StreamHub→SSE `stream` fan-out. The user explicitly called streaming
   slow; this closes it.
2. **Checkpoint carries the roster (I3/I5, X850/X836b)** — a backend restart
   *after oplog compaction* rebuilds heads from the checkpoint but not the roster
   (briefly under-reports threads until the next disk flush / replay). Carry the
   roster inside `Snapshot`.
3. **`Lifecycle` emit (I8, X842)** — emit `Lifecycle{Running}` on boot and
   `Stopping/Stopped` on graceful shutdown so the fleet view shows accurate
   lifecycle without liveness guessing.
4. **§19 observability surface (X868)** — per-agent stream latency p50/p99,
   dropped/coalesced frames, rev lag (view vs oplog head), fsync latency, watch
   count, durable breaker state; structured logs keyed `agent_id+cmd_id+rev`.
5. **Band-aid cleanup (X859/X865)** — once each resource is fully delta-covered,
   remove its `invalidate` subscription + the `sendCommand` double-invalidate, so
   exactly one mechanism owns each resource's freshness.
6. **Formal validation (X844/X866/X867/X869)** — the V11 no-fsync-on-loop test,
   the before/after latency table, the flipped gap register with attached proofs,
   and the multi-agent soak / V1–V12 fault matrix in CI.

---

## 6. Credit

The orchestration **primitives were built faithfully and well-tested** (95 green
`cp-orchestrator` tests): `MaterializedView`, `Tailer`, `StreamHub`,
`CostBreaker`, `TicketStore`, SSE transport, the oplog WAL + group-commit, the
content-addressed body `Store`. This session **connected the muscles to the
skeleton** — agent emission, read-from-view, SSE deltas, frontend apply — and
proved the result live. The gap was wiring; the wiring is now done for the live
path, with a short, named ledger of completeness items remaining.
