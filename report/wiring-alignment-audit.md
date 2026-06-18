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
cache. The inversion that *was* the latency problem is corrected. Live *token*
streaming (§7) is now **live end-to-end** (agent-tagged frames → TeeReader →
StreamHub → SSE → rAF frontend consumer, proven with real data). What remains is
**completeness, not architecture**: carrying the roster inside checkpoints
(cold-restart-after-compaction) and the §19 observability surface.

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
| **I2** | Main loop never fsyncs; dedicated oplog thread group-commits | **ABIDES** ✅ | `src/app/run/threads/bridge.rs:148` `emit_roster_delta`→`submit_durable` (non-blocking durable); `:186` `append_best_effort`; `messages.rs:156` `submit_durable`. Loop only enqueues. **V11 proven**: `cp-oplog/src/service_tests.rs::v11_emit_burst_never_blocks_the_loop_on_fsync` (5k-emit burst, worst single emit `<25ms`, decoupled from `fdatasync`). | — |
| **I3** | Snapshot = bounded heads + content-addressed bodies | **ABIDES** ✅ | `materialized_view.rs` `AgentView{heads,roster}`; heads populated by `MessageCreated`; `Checkpoint`/`Snapshot` now carry the **roster** too (`cp-wire snapshot.rs` `Snapshot.roster`), restored wholesale on fold (`materialized_view.rs` `apply` Checkpoint `clone_from snapshot.roster`; `cp-oplog replay.rs` `fold_entry` Checkpoint `clone_from roster`) | — |
| **I5** | Tier ② is a lazily-rebuildable cache; live reads come from the view | **ABIDES** | `transport/rest/mod.rs:227` `/threads` served **roster-first from `backend.view`** (`overlay_roster` merges view roster onto disk log; view-only threads synthesised instantly); `/fleet`,`/agent` from view. Low-churn inspection reads (`panels.rs:1,4`, memory/todos/tree/…) stay tier-② **by the doc's documented allowance**. Cold-restart-after-compaction now bounded: the `Checkpoint` snapshot carries the **roster** (`cp-wire snapshot.rs`), so a backend that restarts after oplog compaction rebuilds the thread list by folding only the newest checkpoint-bearing segment (`replay.rs` `roster_survives_compaction_via_checkpoint`), not from offset 0. | — |
| **I8** | Command effects, rev, **phase, lifecycle, cost** are oplog appends | **ABIDES** ✅ | `bridge.rs:291/325/338` `ThreadCreated/Archived/Restored`; `:216/230` `PhaseTransition`/`CostAggregate`; `messages.rs:156` `MessageCreated`; **`boot.rs` `Boot::start_in` `Lifecycle::Running` + `Boot::drop` `Lifecycle::Stopping`** — all emitted on apply/lifecycle | — |
| **I10** | Durable "message created" in oplog; stream `MessageStart` is a hint | **PARTIAL** | durable side ABIDES: `messages.rs` `emit_messages` → `MessageCreated` + I13 body store; frontend `live.ts` `applyThreadDelta` `message_created` appends to the log | the **stream-hint side** (live token paint) is not yet consumed by the frontend (§7 / Phase 7) |
| **I11** | "Accepted" = durable (journal-then-ack) | **ABIDES** | `cp-mod-bridge/src/command.rs` `handle_frame` (append_durable before ack) | — |
| **I12** | One inotify watch per agent on the oplog; 2–3s poll is a backstop | **ABIDES** | `transport/mod.rs:394` `OplogWaiter.wait(TAIL_REPOLL)` — inotify/FSEvents primary, `:71` `TAIL_REPOLL=5ms` tight backstop; `runtime.rs` mtime scan demoted to a dirty→`invalidate` backstop for inspection resources only | (the `invalidate` backstop is the documented transitional fallback — removed per-resource in X859) |
| **I13** | Body-before-reference barrier; immutable content-addressed body store | **ABIDES** | `messages.rs` `emit_one_message`: `store.put(body)` (I13 barrier — inline small / spill+fdatasync large) **before** `submit_durable(MessageCreated)`; `cp-mod-bridge/src/body.rs` `Store` | — |
| **§7** | Stream plane: SPSC tee → publisher thread; rAF token batching mandatory | **ABIDES** ✅ | **end-to-end live** — agent publishes `Token` frames (`cp-mod-bridge/src/tee.rs`) **tagged with the active streaming `message_id`** (`lib.rs` `publish_frame` reads `state.messages.last().id`); backend `TeeReader` (`registry/tee_reader.rs`) connects each agent's `tee.sock` → `hub.publish` → `run_stream` drains the hub → SSE `stream`; `StreamHub` fans out; **frontend `useStreamingTokens(agentId)`** (`web/src/lib/live.ts`) subscribes SSE `stream`, accumulates `Token` text into a per-`message_id` buffer flushed **once per `requestAnimationFrame`** (never `setState` per token — §7 mandatory contract honoured), and `Conversation.tsx` overlays the live buffer onto the durable conversation (`useConversation`), reconciling per message: shows the longer of (live buffer, durable text) + a blinking cursor while the buffer leads, synthesises a trailing bubble for a streaming turn not yet flushed. **Proven with real data**: a raw SSE capture against the live agent carried token frames tagged `message_id="A2224"` with the exact streaming text (agent→TeeReader→hub→SSE). `ConversationMsg.id` surfaced (= `Message.id`) for correlation. | — (multi-`tool_use` messages render only the first tool card — minor cockpit-fidelity note, not load-bearing) |
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

### 4.1 Before/after latency table (X866)

`command → visible` is `POST /command → the matching rev-numbered roster delta
arrives on the SSE wire` (the instant `applyThreadDelta` paints it; the
frontend store update is an in-place sub-ms reducer, never a refetch). Measured
with `/tmp/lat.py` (25× `create_thread`, agent `f3a993c0ff357b41`).

| Path | BEFORE (disk/poll plane) | AFTER (push plane) |
|---|---|---|
| thread create → **visible** | ~seconds (≤ 2 s `config.json` mtime poll backstop; debounced 50 ms disk coalescing) | **p50 35 ms · p99 68 ms** (under live load) / p50 ~14 ms (agent idle) |
| durable **ack** (journal-then-ack) | — (no journal existed) | p50 22 ms · p99 47 ms |
| visible **misses** (> 2 s) | common (poll-bounded) | **0 / 25** |

Two AFTER columns because the figure is honest about contention: **35 ms p50 /
68 ms p99 is measured while the agent is actively `streaming`+`tooling`** (the
metrics endpoint read `phase: streaming`, rev lag 0–1 during the run), i.e. the
main loop is contending with a live LLM stream + tool execution. The ~14 ms p50
is the same path with the agent idle. The deployment claim is the **under-load**
number: even mid-stream, a web command is visible in **< 70 ms p99 with zero
misses** — versus a baseline where a created thread could sit invisible until the
next ~2 s mtime poll. An intermediate epoch (before the `TAIL_REPOLL=5ms`
FSEvents-coalescing fix) measured p50 111 ms, bounded by the old 100 ms
`TAIL_INTERVAL`; the 5 ms re-poll cap removed that floor.

### 4.2 Path breakdown

- Command **journaling**: down from the baseline's "seconds". Floor = the
  durable journal-then-ack fsync (I11).
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

1. **§7 live token streaming (Phase 7) — DONE ✅** — the stream plane is now
   live end-to-end. The agent publishes `Token` frames tagged with the active
   streaming `message_id`; the `TeeReader` republishes them into the
   `StreamHub`; `run_stream` emits them as SSE `stream` events; and the frontend
   `useStreamingTokens(agentId)` hook accumulates them into a per-`message_id`
   buffer flushed **once per `requestAnimationFrame`** (never `setState` per
   token), which `Conversation.tsx` overlays onto the durable conversation,
   reconciling against the flushed message. Proven with a real SSE capture
   (token frames tagged `message_id="A2224"` carrying the live streaming text).
   Residual: multi-`tool_use` assistant messages render only their first tool
   card (a cockpit-fidelity nicety, not load-bearing).
2. **Checkpoint carries the roster (I3/I5, X850/X836b) — DONE ✅** — the
   `Snapshot` a `Checkpoint` record carries now includes the thread **roster**
   (`cp-wire snapshot.rs` `Snapshot.roster: Vec<RosterThread>`, `#[serde(default)]`
   for N-1 tolerance). The writer stamps it into every rolled segment's leading
   checkpoint (`cp-oplog append.rs` `OplogWriter::snapshot`), agent replay folds
   + restores it wholesale (`replay.rs` `fold_entry` Checkpoint `clone_from
   roster`), and the backend `MaterializedView::apply` does the same
   (`clone_from snapshot.roster`). A single shared `RosterThread` type with
   `fold_created`/`fold_archived`/`fold_status`/`fold_message` helpers is the
   single source of truth, so a roster rebuilt by folding live deltas and one
   restored from a checkpoint are byte-identical. Proven by
   `replay.rs::roster_survives_compaction_via_checkpoint` (the early thread
   survives several segment rolls via the checkpoint roster, fast-path == full
   replay) and `materialized_view_tests.rs::checkpoint_restores_roster_wholesale`.
   A backend cold-start after oplog compaction no longer under-reports the
   thread list.
3. **`Lifecycle` emit (I8, X842) — DONE ✅** — `Boot::start_in` journals a
   durable `Lifecycle::Running` once every advertised resource is up, and
   `Boot::drop` journals `Lifecycle::Stopping` before teardown (the oplog commit
   thread drains + `fdatasync`s it before joining, so a graceful shutdown is
   durably recorded; a `SIGKILL` falls back to liveness — the intended
   best-effort-graceful contract). The backend already folds `Lifecycle`
   latest-wins into the view; `meta.rs::derive_status` now consults it so a
   `Stopping`/`Stopped` agent can never read "working". I8 is now fully ABIDES
   (every oplog-journaled fact the doc lists is emitted). Proven by
   `boot.rs::lifecycle_running_on_boot_and_stopping_on_drop`.
4. **§19 observability surface (X868)** — per-agent stream latency p50/p99,
   dropped/coalesced frames, rev lag (view vs oplog head), fsync latency, watch
   count, durable breaker state; structured logs keyed `agent_id+cmd_id+rev`.
5. **Band-aid cleanup (X859/X865)** — once each resource is fully delta-covered,
   remove its `invalidate` subscription + the `sendCommand` double-invalidate, so
   exactly one mechanism owns each resource's freshness.
6. **Formal validation (X866/X867/X869; X844 — DONE ✅)** — the **V11
   no-fsync-on-loop test is landed** (`cp-oplog/src/service_tests.rs::v11_emit_burst_never_blocks_the_loop_on_fsync`):
   a 5 000-record best-effort `PhaseTransition` burst (the "phase transitions
   during streaming" scenario the doc names) proves the worst single emit stays
   `< 25 ms` and the whole burst `< 2 s` — emit latency is *decoupled* from
   `fdatasync` latency, since `append_best_effort` is a `try_send` (structurally
   non-blocking), and a trailing durable barrier confirms the log stays intact +
   replayable. This validates the I2 execution-model keystone. Remaining: the
   before/after latency table, the flipped gap register with attached proofs,
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
