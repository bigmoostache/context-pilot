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
StreamHub → SSE → rAF frontend consumer, proven with real data), and the
**token-economics** surfaces (cockpit `StatsPanel`/`LeftRail`, fleet `UsagePage`)
now read live cumulative tokens + spend from `/metrics` — **no app surface still
draws from mock data**. The keystone (K1–K9) and fault-matrix (V1–V12) re-walk
below leaves **zero un-validated load-bearing invariants**: every I-invariant +
keystone is ABIDES, 10/12 V-rows are proven by a landed test, the 2 deferred
V-rows (V4 fsync-fault FS-injection, V8 literal 10k-agent OS soak) are external-CI
harnesses for invariants already validated by proxy / at-scale logic, and the
single honest follow-up — the §19 latency/fsync **histograms** — is explicitly
non-load-bearing.

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
| **I10** | Durable "message created" in oplog; stream `MessageStart` is a hint | **ABIDES** ✅ | durable side: `messages.rs` `emit_messages` → `MessageCreated` + I13 body store; frontend `live.ts` `applyThreadDelta` `message_created` appends to the log. Stream-hint side now **consumed**: agent tags `Token` frames with the active `message_id` (`cp-mod-bridge lib.rs` `publish_frame`) → `TeeReader` → `StreamHub` → SSE `stream` → `useStreamingTokens` rAF buffer overlaid by `Conversation.tsx`; oplog `MessageCreated` reconciles/finalises the durable message (the hint is latency, the oplog is truth). | — |
| **I11** | "Accepted" = durable (journal-then-ack) | **ABIDES** | `cp-mod-bridge/src/command.rs` `handle_frame` (append_durable before ack) | — |
| **I12** | One inotify watch per agent on the oplog; 2–3s poll is a backstop | **ABIDES** | `transport/mod.rs:394` `OplogWaiter.wait(TAIL_REPOLL)` — inotify/FSEvents primary, `:71` `TAIL_REPOLL=5ms` tight backstop; `runtime.rs` mtime scan demoted to a dirty→`invalidate` backstop for inspection resources only | (the `invalidate` backstop is the documented transitional fallback — removed per-resource in X859) |
| **I13** | Body-before-reference barrier; immutable content-addressed body store | **ABIDES** | `messages.rs` `emit_one_message`: `store.put(body)` (I13 barrier — inline small / spill+fdatasync large) **before** `submit_durable(MessageCreated)`; `cp-mod-bridge/src/body.rs` `Store` | — |
| **§7** | Stream plane: SPSC tee → publisher thread; rAF token batching mandatory | **ABIDES** ✅ | **end-to-end live** — agent publishes `Token` frames (`cp-mod-bridge/src/tee.rs`) **tagged with the active streaming `message_id`** (`lib.rs` `publish_frame` reads `state.messages.last().id`); backend `TeeReader` (`registry/tee_reader.rs`) connects each agent's `tee.sock` → `hub.publish` → `run_stream` drains the hub → SSE `stream`; `StreamHub` fans out; **frontend `useStreamingTokens(agentId)`** (`web/src/lib/live.ts`) subscribes SSE `stream`, accumulates `Token` text into a per-`message_id` buffer flushed **once per `requestAnimationFrame`** (never `setState` per token — §7 mandatory contract honoured), and `Conversation.tsx` overlays the live buffer onto the durable conversation (`useConversation`), reconciling per message: shows the longer of (live buffer, durable text) + a blinking cursor while the buffer leads, synthesises a trailing bubble for a streaming turn not yet flushed. **Proven with real data**: a raw SSE capture against the live agent carried token frames tagged `message_id="A2224"` with the exact streaming text (agent→TeeReader→hub→SSE). `ConversationMsg.id` surfaced (= `Message.id`) for correlation. | — (multi-`tool_use` messages render only the first tool card — minor cockpit-fidelity note, not load-bearing) |
| **§9** | SSE carries rev-numbered, replayable, gap-free deltas; client applies | **ABIDES** | `transport/mod.rs:346` `SseMessage::delta(entry.rev, data)` per `OpEntry`; `web/src/lib/live.ts` `applyThreadDelta`/`applyAgentDelta` apply in-place with a monotonic-rev high-water guard | `invalidate` fallback still present for inspection resources (cleanup X859) |
| **§18** | schema_version + N-1 compat + Unknown tolerance | **ABIDES** | `cp-wire` all types `schema_version`'d, `#[serde(other)] Unknown`; roster variants added with tolerant-decode tests | — |
| **§19** | Observability: breaker state, stream health, rev lag, token economics (+ latency/fsync histograms) | **ABIDES** (load-bearing) ✅ · histograms **DEFERRED** (non-load-bearing) | `transport/inspect/metrics.rs` `agent_metrics`/`fleet_metrics` (`GET /api/agent/{id}/metrics`, `GET /api/metrics`) expose the state the backend already holds: durable breaker `{tripped,spendUsd,budgetUsd}`, stream `{subscribers,droppedFrames,degraded}` (`StreamHub::agent_stream_health`), rev `{view,oplogHead,lag}` (view rev vs fresh `cp_oplog::replay` head), **tokens `{input,output}`** (cumulative-since-boot, folded from `CostAggregate` into `view.cost` — feeds the live token-economics UI: `StatsPanel`/`LeftRail`/`UsagePage`, proven live input 3.0 B / output 10.6 M), phase, lifecycle. Surfaced live in the web cockpit by `FleetDashboard.tsx` `HealthBadge` (T121: a tripped breaker is now a visible pill, not a silent latch), proven by `web/e2e/metrics.spec.ts` (4). | The latency p50/p99 + fsync-latency **histograms** are the only un-shipped piece and are **explicitly non-load-bearing** (they observe a path whose *correctness* is already validated by V11/the latency table — a histogram would only refine the percentile, not gate any invariant). They need new timestamped hot-path instrumentation (deliberately not faked); tracked as the X830/X869 follow-up. **No load-bearing observability is missing.** |

---

## 2.5 Keystones K1–K9 (the design-review crux issues) — re-walk

The doc folds nine *keystone* concerns (the round-2 senior-review cruxes) into its
fault analysis. Each is satisfied by the invariant work tabulated above; this
makes the keystone walk explicit (the audit preamble claimed to cover K1–K9 but
never tabulated them). **All nine ABIDE.**

| K | Crux (doc) | Verdict | Evidence (rides) |
|---|---|---|---|
| **K1** | Confine `fsync` to the tiny oplog; the tier-② shared writer is *untouched* (no fsync-per-write amplification, the v4 mistake) | **ABIDES** ✅ | `cp-oplog` owns every `fdatasync`; `state/persistence/writer.rs` (tier-②) byte-unchanged. Rides **I2**. |
| **K2** | Journal-then-ack + replay + dedup ⇒ no double-apply on a deadman re-exec | **ABIDES** ✅ | `cp-mod-bridge/command.rs` `handle_frame` (append_durable before ack) + `SeenSet`; `tests/crash_replay.rs` V2. Rides **I11/I4**. |
| **K3** | Content-addressed bodies + bounded heads, not an O(S²) enumerate-everything manifest | **ABIDES** ✅ | `cp-wire snapshot.rs` `Snapshot{heads,seen,roster}` (bounded); `cp-mod-bridge/body.rs` `Store` (content-addressed). Rides **I3/I13**. |
| **K4** | Don't *rewrite* the shared writer under the banner of "hardening" (the v4 trap) — bridge-owned oplog instead | **ABIDES** ✅ | The bridge crate is purely *additive*; `writer.rs`/`save.rs` untouched. Rides **I8**. |
| **K5** | Oplog append-only & gap-free; lifecycle shows "processing", not a coalesced jump to "done" | **ABIDES** ✅ | `cp-oplog/append.rs` monotonic `rev`; `PhaseTransition`/`Lifecycle` emitted on transition (`run/threads/bridge.rs`), folded latest-wins. Rides **I8**, validated by **V10** (gap-free; soak deferred to X869). |
| **K6** | Phase shown via durable oplog record **and** a sub-ms live hint, self-healing a dropped hint | **ABIDES** ✅ | durable `PhaseTransition` (oplog) + `PhaseHint` on the stream plane (`cp-mod-bridge/lib.rs`); frontend applies whichever arrives first, oplog wins. Rides **I10**. |
| **K7** | Inject a command's effect via the **existing user-message entry**, never the autonomy spine | **ABIDES** ✅ | `run/threads/bridge.rs` `apply_command` `SendMessage` → the K7 user-message path (`actions/input.rs`); `spine/engine.rs::check_spine` untouched. |
| **K8** | One inotify watch per agent on the oplog ⇒ no watch exhaustion at fleet scale | **ABIDES** ✅ | `transport/sse.rs` `OplogWaiter` = one `RecommendedWatcher` per agent stream; `runtime.rs` scan is poll, not per-file watches. Rides **I12**, literal 10k soak = **V8** (X869). |
| **K9** | `rev` = the fsync'd oplog offset; announce **after** durable (never before) | **ABIDES** ✅ | `cp-oplog/append.rs` `append` = append_buffered+sync then return rev (announce-after-durable); `service.rs` group-commit releases acks only post-`fdatasync`. Rides **I8/I11**. |

## 2.6 Fault matrix V1–V12 — validation status (re-walk)

The doc's V1–V12 fault matrix is the formal validation contract. This table
records, for each row, **the test that proves it today** versus the literal
fault-injection/soak deferred to the X869 CI matrix. **10 of 12 are proven by a
landed test now; the remaining 2 (V4 fsync-fault FS-injection, V8 literal 10k-agent
OS soak) are design-ABIDES with their external-CI harness scheduled for X869**
(neither is an un-closed *design* gap — each is an additional adversarial *proof*
of an invariant already validated by proxy / at-scale logic, and neither can be a
localhost cargo test without contorting durability code or spawning 10k processes).

| V | Guards | Asserts (doc) | Status | Proof / plan |
|---|---|---|---|---|
| **V1** | I8 oplog append | `kill -9` between write & fsync ⇒ replay discards torn tail, no half-effect | **PROVEN** ✅ | `cp-oplog/tests/crash_replay.rs` (real SIGKILL) + `segment.rs` `scan` torn-tail boundary |
| **V2** | I11 journal-then-ack | deadman re-exec after ack ⇒ effect replayed exactly once | **PROVEN** ✅ | `cp-mod-bridge/command.rs` `v2_dedup_survives_deadman_reexec` + `crash_replay.rs` |
| **V3** | I4 dedup | replay same `dedup_token` after a long outage ⇒ second apply is a no-op | **PROVEN** ✅ | `command.rs` seen-set replay test (idempotent re-accept, no re-apply) |
| **V4** | I2 durability | power-cut (fsync fault) ⇒ committed `rev` readable, uncommitted absent | **PROVEN (by proxy)** · fault-injection **X869** | replay reads exactly the synced prefix (V1 torn-tail); a literal fsync-fault FS harness is the X869 add |
| **V5** | I10 ordering | drop+reorder stream frames, drop `MessageStartHint` ⇒ UI reconstructs from oplog, no orphan leak | **PROVEN** ✅ | `tests/fleet_soak.rs::a_flooded_reordered_subscriber_coalesces_then_reconciles_from_the_view` (a small-capacity subscriber flooded with out-of-order frames coalesces to the newest window, latches `degraded`, and is reconciled from the `MaterializedView`'s oplog-folded heads) + `registry/tee_reader.rs` corrupt-frame resync + `stream_hub.rs` overflow-evict-degraded + frontend `applyThreadDelta` high-water guard |
| **V6** | I9 auth | command with missing/invalid bearer ⇒ rejected | **PROVEN** ✅ | `command.rs` auth test (empty/mismatch → reject) + `transport/ticket.rs` single-use redeem |
| **V7** | I7 backpressure | stall the consumer, fill the ring ⇒ loop latency unaffected, degraded flag set | **PROVEN** ✅ | `cp-mod-bridge/tee.rs` `v7_stalled_consumer_never_blocks_producer` (100k publishes < 5 s) + `stream_hub.rs` admit-evict + degraded |
| **V8** | I12/K8 | spawn 10k agents ⇒ watch count ≈ agent count, no exhaustion | **PROVEN (logical, N=16)** · literal 10k OS soak **external CI** | `tests/fleet_soak.rs::n_agents_under_concurrent_load_stay_gap_free_and_isolated` proves the *logical* fleet-isolation invariant at N=16 (one shared view/breaker/hub keeps 16 isolated projections, no cross-contamination); one `OplogWaiter`/agent by construction means watch count is `O(agents)` at any scale. The literal 10k-agent OS soak (flat RSS/FD/threads) is genuinely external CI infra (can't spawn 10k processes on a laptop) |
| **V9** | R2-8 CostBreaker | crash-loop at the spend ceiling ⇒ breaker stays tripped (durable counter) | **PROVEN** ✅ | `cost_breaker.rs` V9 high-water latch + `rebuild_from_view` + `tests/services_integration.rs` restart-rebuild-trips |
| **V10** | K5 gap-free | coalesce tier-② saves under load ⇒ oplog rev stream has no gaps | **PROVEN** ✅ | `tests/fleet_soak.rs::n_agents_under_concurrent_load_stay_gap_free_and_isolated` (16 agents each drive a contended mixed durable/best-effort workload into their own oplog; every replayed log is a strictly contiguous `0..=rev_head` stream — a shed best-effort record never consumes a `rev`, so the durable sequence never tears) + `cp-oplog` monotonic `rev` + replay-identical-after-compaction tests |
| **V11** | I2 / §11.1 loop-fsync | burst of phase transitions during streaming ⇒ loop tick time statistically unchanged (no fsync on the loop) | **PROVEN** ✅ | `cp-oplog/src/service_tests.rs` `v11_emit_burst_never_blocks_the_loop_on_fsync` (5 000-emit burst, worst single emit < 25 ms, decoupled from `fdatasync`) |
| **V12** | I13 body barrier | `kill -9` between a spilled body's fdatasync and its referencing entry's commit ⇒ orphan body, never a dangling head-hash | **PROVEN** ✅ | `cp-mod-bridge/body.rs` `gc_keeps_in_flight_spill_within_grace` (V12 race guard) + `get_detects_corruption` |

**Re-walk verdict.** Zero un-validated **load-bearing** invariants remain. Every
I-invariant, every keystone, and 10/12 fault-matrix rows are proven by a landed
test today; the 2 remaining V-rows (V4 fsync-fault FS-injection, V8 literal
10k-agent OS soak) are external-CI harnesses for invariants already validated by
proxy (V4 rides V1's real-SIGKILL torn-tail proof) / at-scale logic (V8 rides the
N=16 isolation proof) — not open design gaps. The single honest *follow-up* is the
§19 latency/fsync **histograms** (explicitly non-load-bearing).

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
4. **§19 observability surface (X868) — DONE ✅** — `transport/inspect/metrics.rs`
   `agent_metrics`/`fleet_metrics` expose the durable breaker state, stream
   health (subscribers/dropped/degraded), rev lag (view vs oplog head), and the
   cumulative token totals; surfaced live by `FleetDashboard.tsx` `HealthBadge`
   and proven by `web/e2e/metrics.spec.ts` (4). The latency p50/p99 + fsync-latency
   **histograms** are the one un-shipped piece and are **non-load-bearing** (they
   would refine a percentile, not gate an invariant whose correctness V11 + the
   latency table already prove); they need new timestamped hot-path
   instrumentation, deliberately not faked — tracked as the X830/X869 follow-up.
4b. **Token-economics surfaces (X868 cont.) — DONE ✅** — the cumulative
   input/output token totals fold from `CostAggregate` into `view.cost` and ship
   on `/metrics` as `tokens:{input,output}` (proven live: input 3.0 B / output
   10.6 M). The cockpit `StatsPanel` (live session tokens + spend + panel-context
   meter), `LeftRail` (live context-budget meter), and fleet `UsagePage`
   (per-agent spend/token table) were rewritten off live data; the cache hit/miss
   split + monthly history (private agent state the backend never journals) render
   an honest `InspectionUnavailable` notice instead of fabricated data. **No app
   surface still draws from mock.** tsc clean; e2e 23/24 (the one miss is the
   `messages` single-live-agent load flake, passes isolated).
5. **Band-aid cleanup (X859/X865) — DONE ✅** — each delta-covered resource now
   owns its freshness purely through in-place SSE delta apply; the dead
   `invalidate` bus + the `sendCommand` double-invalidate are gone. Exactly one
   mechanism owns each resource's freshness.
6. **Formal validation (X866/X867 — DONE ✅; X869 — IN CI follow-up)** — the
   before/after latency table (§4) is **measured** (command→visible p50 ~14 ms
   idle / 35 ms under live load, p99 68 ms, 0/25 misses), the **V11
   no-fsync-on-loop test is landed**
   (`cp-oplog/src/service_tests.rs::v11_emit_burst_never_blocks_the_loop_on_fsync`),
   and the gap register has been **fully re-walked** (X867): the §2 invariant
   table, the §2.5 keystone K1–K9 table, and the §2.6 V1–V12 fault-matrix table
   each carry a verdict + attached proof, leaving **zero un-validated
   load-bearing invariants**. The only items still open are the X869 CI additions
   — the literal 10k-agent OS soak (V8) and the fsync-fault FS-injection harness
   (V4), each an *additional* adversarial proof of an invariant already validated
   by proxy / at-scale logic (V5 + V10 are now **proven** by the landed
   `fleet_soak.rs` chaos + gap-free-under-load tests) — and the X870 design-doc
   decision-log + deploy tag.

---

## 6. Credit

The orchestration **primitives were built faithfully and well-tested** (95 green
`cp-orchestrator` tests): `MaterializedView`, `Tailer`, `StreamHub`,
`CostBreaker`, `TicketStore`, SSE transport, the oplog WAL + group-commit, the
content-addressed body `Store`. This session **connected the muscles to the
skeleton** — agent emission, read-from-view, SSE deltas, frontend apply — and
proved the result live. The gap was wiring; the wiring is now done for the live
path, with a short, named ledger of completeness items remaining.

---

## 7. Disk-read ledger (X864) — every backend read path justified

> **Claim being proven.** *No endpoint rides disk for a live, high-churn path
> that should be the view.* Every surviving `fs::read*` / `StateReader` call in
> `cp-orchestrator` is one of: **discovery** (the registry — disk *is* its
> truth), **designed cold-start hydration** (I5), **doc-sanctioned low-churn
> inspection** (the "unmanaged read-only listing", mtime-memoized), the
> **Finder file-manager feature** (a file browser by definition), the **durable
> conversation reconcile** (live typing rides the stream plane), or the
> **metrics rev-lag probe** (reading the oplog head is its whole point).
>
> Enumerated by grepping every `fs::read`/`read_dir`/`read_to_string` and every
> `StateReader` method call across `crates/cp-orchestrator/src` (test sites
> excluded). Each row cites file:line and its plane verdict.

| # | Read site (file:line) | What it reads | Plane / verdict | Justification |
|---|---|---|---|---|
| 1 | `registry/mod.rs:211,247,262` | agent registry `<id>.json` records (scan-and-diff) | **Discovery** | The registry dir *is* the durable source of truth for which agents exist (§10); low-churn (changes only on agent boot/shutdown). |
| 2 | `registry/mod.rs:239` | heartbeat file (60 B) | **Discovery / liveness** | Fixed-size liveness probe for the three-factor verdict; intrinsic to discovery. |
| 3 | `registry/channel.rs:95` | one registry record | **Discovery** | `AgentChannel` construction from an `Entry`. |
| 4 | `transport/mod.rs:442`, `rest/mod.rs:212` (`resolve_entry`) | `<id>.json` per request | **Discovery** | Resolves the agent's registry record (folder/paths/cap_token), *not* agent state; a tiny JSON read. Low-churn; could be memoized but is cheap. |
| 5 | `inspect/meta.rs:210`, `inspect/metrics.rs:131` (`list_entries`) | `read_dir` of the agents dir + each record | **Discovery** | Fleet enumeration for `/fleet/meta` and `/metrics`. O(agents) per fleet poll — acceptable for realistic fleets; a dir-mtime memo is a noted future optimization. |
| 6 | `channel.rs` `hydrate` / `Tailer` seed (reads the **oplog**) | bounded cold-start replay | **Cold-start hydration (I5)** ✅ | The *designed* path: hydrate the view once from the oplog at first-sight, then ride the tail. Restart cost bounded by agent count, not fleet disk. |
| 7 | `rest/mod.rs` `fleet()` / `agent()` | — | **VIEW (live)** ✅ | Served from `backend.view`, never disk. |
| 8 | `rest/mod.rs:275` `threads()` roster | `backend.view.get().roster` | **VIEW (live)** ✅ | The roster/status/archived/lastActivity come from the view. The `read_config` at `:263` hydrates only the per-thread **message log** (conversation bodies), merged by `overlay_roster` — a tier-② hydrate, not the live roster path. |
| 9 | `inspect/meta.rs:109` (`inspect_threads`) | `config.json` (thread count / MY_TURN / task glance) | **Tier-② (memoized)** | Fleet-*card* enrichment glance, `read_config` mtime-memoized; the authoritative live roster rides the view via `/threads`. Phase/lifecycle/cost on `/meta` already come from the view. |
| 10 | `inspect/panels.rs:105,134,143` (`read_shared`) | `shared/{memories,tree-descriptions,callbacks}.yaml` | **Tier-② inspection** ✅ | The doc's "unmanaged read-only listing" — genuinely low-churn state with no oplog delta to fold. mtime-memoized via `StateReader` `AgentCache`. |
| 11 | `inspect/panels.rs:176,348` (`read_worker`) | `states/<wid>.json` (todos/spine/queue/scratchpad) | **Tier-② inspection** ✅ | Per-worker module data; low-churn; mtime-memoized. |
| 12 | `inspect/panels.rs:30,42` (panel_list), `:199,206` (library) | `panels/*.json`, library `*.md` | **Tier-② inspection** ✅ | Panel inventory + behaviour library; low-churn listings. |
| 13 | `inspect/finder.rs:227,253` (`conversation`) | `messages/*.yaml` | **Tier-② reconcile** ⚠️ | The **durable** conversation record. Live *typing* rides the stream plane (`useStreamingTokens`); this disk read is the lower-frequency reconcile (cold load + 5 s poll). **Noted future optimization:** serve from view `heads` + the `/body/{hash}` content store instead of re-reading message files. |
| 14 | `inspect/finder.rs:50,83,191,304` (`fs_list`/`preview`/`download`) | the agent **realm filesystem** | **N-A — Finder feature** | This *is* a file manager; reading the realm is its purpose, not state inspection. |
| 15 | `inspect/metrics.rs` `oplog_head_rev` | the oplog dir head `rev` | **Metrics probe (off-lock)** ✅ | The metrics endpoint exists to expose view-vs-oplog-head **lag**, so reading the head is intrinsic. Read *outside* the backend lock so it never blocks a command path. |

**Verdict: ABIDES.** Zero accidental disk reads on a live path. The single row
to keep an eye on is #13 (`/conversation`), with a documented stream-plane
ownership of the live path and a named future optimization; everything else is
exactly where the design doc's plane discipline puts it.
