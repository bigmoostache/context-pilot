# Wiring Alignment Audit — implementation ⇄ `design-orchestration-backend.md`

> **Purpose.** Make the **running wiring** embody the design doc's architecture
> *philosophy* — not merely contain its primitives. This is the deployment gate.
>
> **Scorecard semantics.** Each load-bearing invariant / keystone / clause is
> rated **ABIDES** (implemented faithfully + validated), **PARTIAL** (primitive
> exists but not wired, or wired incompletely), **VIOLATES** (contradicts the
> doc), or **N-A**. Phase 9 flips every VIOLATES/PARTIAL → ABIDES with a
> validating test or measurement.
>
> Generated: 2026-06-18 (Phase 0.1). Branch: `demaquetting`.

---

## 0. The one-sentence verdict

The demaquetting work built the **inspection plane** (read tier-② disk files →
reshape → REST) as the frontend's *primary live-data path*, and left the
**orchestration plane** (oplog → MaterializedView → rev-numbered SSE deltas) as a
faithfully-built, well-tested **backend skeleton whose muscles are not connected
to real agent state**. The doc's whole point is the inverse: disk is the
disposable cache; the oplog→view→SSE push is the live truth. **That inversion is
the entire latency problem.**

---

## 1. The two planes (the philosophy being measured)

| Plane | Doc intent | Should carry | Speed |
|---|---|---|---|
| **A — Orchestration (PUSH)** | `oplog → backend Tailer (1 inotify/agent) → in-memory MaterializedView → rev-numbered SSE deltas → frontend APPLIES deltas` | the live read path: threads, messages, phase, MY_TURN, cost, lifecycle | sub-50ms |
| **B — State cache (tier ②)** | a **lazily-rebuildable, disposable** cache (I5) | cold-start hydration + unmanaged read-only listing only | debounced disk, irrelevant to live UX |

**The sin:** plane B is the live path; plane A is inert.

---

## 2. Invariant-by-invariant gap register

| # | Invariant (doc) | Verdict | Evidence (file:line) | Gap / correction |
|---|---|---|---|---|
| **I1** | Single writer *process* per folder (flock) | **ABIDES** | `cp-mod-bridge/src/boot.rs` `acquire_lock` (+ bounded retry, commit 2a76cfe) | — |
| **I2** | Main loop never fsyncs; dedicated oplog thread group-commits | **PARTIAL** | `cp-oplog/src/service.rs` `OplogService` (group-commit exists, tested) | primitive is faithful, but the agent only routes `CommandEffect` through it — state mutations bypass the oplog entirely |
| **I3** | Snapshot = bounded heads + content-addressed bodies | **PARTIAL** | `materialized_view.rs` `AgentView{heads}`; `cp-wire` `Heads`/`ContentHash` | view *can* hold heads, but the agent emits nothing to populate them; no thread-roster representation at all |
| **I5** | Tier ② is a lazily-rebuildable cache; reads come from the view | **VIOLATES** 🔴 | `transport/rest.rs:218,232` (`/threads` → `StateReader` → `config.json`, 295 KB re-parse) vs `:91,107` (`/fleet`,`/agent` use `backend.view`) | high-churn reads (`/threads`,`/panels`,`/memory`) must serve from the view; disk = cold-start hydrate only |
| **I8** | Command effects, rev, **phase, lifecycle, cost** are oplog appends | **VIOLATES** 🔴 | `cp-mod-bridge/src/command.rs:191` — the **only** `OpEntryKind` ever appended is `CommandEffect`; `src/app/run/threads.rs` `apply_create_thread`/`apply_archive_thread`/`apply_send_message` just set `state.flags.ui.dirty = true` | emit `ThreadCreated/Archived/Restored`, `MessageCreated`, `PhaseTransition`, `CostAggregate`, `Lifecycle`, `Checkpoint` (Phase 2) |
| **I10** | Durable "message created" in oplog; stream `MessageStart` is a hint | **VIOLATES** 🔴 | no `MessageCreated` emitted anywhere (grep `OpEntryKind::MessageCreated` → only tests + view fold) | message finalize must append `MessageCreated` + body store (I13) |
| **I11** | "Accepted" = durable (journal-then-ack) | **ABIDES** | `cp-mod-bridge/src/command.rs` `handle_frame` (append_durable before ack) | — |
| **I12** | One inotify watch per agent on the oplog; 2–3s poll is a backstop | **VIOLATES** (inverted) 🟠 | `cp-orchestrator/src/runtime.rs` driver = ~2s `config.json` **mtime poll is the mechanism**; oplog Tailer exists but folds nothing because no state deltas are emitted | make the oplog Tailer the primary signal; demote mtime poll to pure backstop |
| **I13** | Body-before-reference barrier; immutable content-addressed body store | **PARTIAL** | `cp-mod-bridge/src/body.rs` `Store` (built + tested, e2e hydrate works) | not wired into a live `MessageCreated` path (none exists yet) |
| **§7** | Stream plane: SPSC tee → publisher thread; rAF token batching mandatory | **PARTIAL / UNVERIFIED** | `cp-mod-bridge/src/tee.rs` + `lib.rs` (publishes Token frames); frontend rAF batching unaudited | verify tokens reach the frontend live + rAF batch in the conversation renderer |
| **§9** | SSE carries rev-numbered, replayable, gap-free deltas; client applies | **VIOLATES** 🔴 | `transport/sse.rs` + `transport/mod.rs` emit a bare `invalidate`; `web/src/lib/live.ts` `useLiveQuery` does `invalidate → refetch-from-disk` | SSE emits real `OpEntry` deltas with payload; frontend applies to a local store (Phase 5/6) |
| **§18** | schema_version + N-1 compat + Unknown tolerance | **ABIDES** | `cp-wire` all types `schema_version`'d, `#[serde(other)] Unknown` | adding roster variants must keep this (Phase 1.2) |
| **§19** | Observability: latency p50/p99, dropped frames, rev lag, fsync latency, watch count | **VIOLATES** 🟠 | no metrics surface exists | stand up the §19 surface (Phase 9.3) |

---

## 3. The protocol/view representational gap (root of Phase 1)

`cp-wire/src/types/oplog.rs` `OpEntryKind` models: `CommandEffect`, `SeenMark`,
`PhaseTransition`, `MessageCreated`, `Lifecycle`, `CostAggregate`,
`Checkpoint`, `Unknown`. **There is no thread-roster representation**
(`ThreadCreated`/`ThreadStatusChanged`/`ThreadArchived`/`ThreadRestored`).

`cp-orchestrator/src/services/materialized_view.rs` `AgentView` carries
`rev + heads + phase + lifecycle + cost`. **It has no thread roster** (id, name,
status, archived, last_activity, msg_count) — the exact shape `/threads` needs.

So even if the agent emitted deltas today, the view couldn't represent the thread
list. **Phase 1 closes this representational gap** (Option A: add roster
`OpEntryKind` variants + roster field on `AgentView`, riding the protocol's
existing `Unknown` forward-compat). Faithful to §16 (thread create/archive/restore
are listed as oplog-journaled actions).

---

## 4. Measured proof (the "why it's slow")

- Command **journaling**: ~54 ms (fast — bridge intake journal-then-ack).
- Command **becoming visible**: **seconds** — waits on
  `agent main-loop applies → dirty → 50 ms PersistenceWriter debounce → 295 KB
  serialize → disk → backend re-parse`. Worse when the agent is busy
  streaming/tooling (the loop starves `poll_bridge_commands`,
  `src/app/run/threads.rs:227`, called `lifecycle.rs:136`).
- Backend disk read itself: <10 ms (the 295 KB parse is cheap; **disk is not the
  bottleneck — the debounced write + poll-based change detection is**).

The "3-layer invalidation" shipped earlier (`live.ts` invalidation bus + SSE
`invalidate` + `runtime.rs` mtime detection) is a **band-aid over the missing push
plane**: it made re-fetching fast, but the data isn't on disk yet when we re-fetch.

---

## 5. Honest credit

The backend orchestration **primitives are built faithfully and well-tested**
(95 green cp-orchestrator tests): `MaterializedView`, `Tailer`, `StreamHub`,
`CostBreaker`, `TicketStore`, SSE transport, the oplog WAL + group-commit, the
content-addressed body `Store`. The skeleton matches the doc. **The gap is
wiring, not architecture** — connecting (a) agent delta emission, (b) backend
read-from-view, (c) SSE-carries-deltas, (d) frontend-applies-deltas. This is
*abidance work*, not a redesign.

---

## 6. BEFORE baseline (Phase 0.3 — to be filled)

| Action | p50 (before) | p99 (before) | Target p99 |
|---|---|---|---|
| Thread create → visible | _TBD_ | _TBD_ (seconds) | < 50 ms |
| Thread archive → visible | _TBD_ | _TBD_ | < 50 ms |
| Send message → visible | _TBD_ | _TBD_ | < 50 ms |
| Phase change → visible | _TBD_ | _TBD_ | < 50 ms |
| Cost update → visible | _TBD_ | _TBD_ | < 50 ms |

_Filled by Phase 0.2/0.3 instrumentation once the trace hooks land._
