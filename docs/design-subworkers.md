# Sub-workers: single → N cooperative workers

**Status:** design (T710). Ultra-synthetic. No code yet.

## 1. Goal

Run **N equal workers** in one TUI process — each with its own context window
(panels, conversation, messages, thread/todo focus), all advancing in the
**same single-threaded loop**. A worker is a first-class agent, not a background
helper.

**Non-goals.** No OS threads for state, no locks, no shared-memory parallelism.
True parallelism stays confined to where it already lives: the per-call LLM
stream thread. All *state mutation* remains serialized on the one loop — the
property that makes the codebase lock-free and race-free today.

## 2. Today

One `App { state: State, … }`, mutated only on the loop thread. Each tick:

```
handle_input_phase        # 1 terminal event → action → render
run_background_phase       # flat ordered pipeline (below)
reload/spinner/render/poll # adaptive 8/2/50ms poll
```

`run_background_phase` (ordered, no branching):

```
bridge → process_stream_events → typewriter → cache → watchers
       → deferred/waiting → handle_tool_execution → finalize_stream
       → check_spine → check_my_turn_threads → REVERIE BLOCK
```

Real threads: the **LLM stream** (spawned per call, pushes `StreamEvent`s over
an mpsc channel), a background **cache-hasher**, the **console** sidecar.
Everything is drained non-blockingly each tick.

There are already **two kinds of logical agent** on this one loop: the **main
worker** and the **reveries**.

## 3. The primitive already exists (reverie)

A reverie is a second logical agent *multiplexed onto the same loop* — and it is
already multi-instance (`cleaner`, `cartographer`). It is the exact shape N
equal workers need, minus scope:

| Piece | Reverie today | N workers |
| --- | --- | --- |
| stream channels | `App.reverie_streams: Map<id, ReverieStream{rx, pending_tools, report_called}>` | `App.workers: Map<WorkerId, Worker{rx, pending_tools, …}>` |
| session state | `State.reveries: Map<id, Session{messages, tool_call_count, is_streaming, …}>` | `State.workers: Map<WorkerId, WorkerState>` |
| per-tick steps | 4: `maybe_start` · `process_events` · `handle_tools` · `check_end_turn` | the **full** pipeline (§2) run per worker |
| tools | shared `dispatch_tool`; disallowed rejected at dispatch | same, no restriction |
| prompt | shares main prefix via `prepare_stream_context(…, ReverieContext)`; only the conversation section diverges (main convo → read-only panel) | each worker owns its full context window |

**Consequence:** the migration is **evolutionary, not a rewrite.** The hardest
bug class — threading/locking — is already designed out. Reverie proves
per-agent channels + `HashMap` sessions + per-tick cooperative interleave work.

## 4. Target model

```
State {
  shared:  SharedState        # one instance
  workers: Map<WorkerId, WorkerState>
  active:  WorkerId           # the one the UI renders
}
App {
  workers: Map<WorkerId, WorkerRuntime{ rx, pending_tools, is_streaming, … }>
}
```

Every worker runs the **same full pipeline** the single main worker runs today —
the reduced 4-step reverie loop and the full pipeline **unify** into one
`for (id, w) in workers` body.

## 5. The State split (the crux — ~80% of the work)

`State` is monolithic and singular today; reveries piggyback on the main
worker's panels/context. Equal workers can't. Each field is classified once:

**Shared (one instance):** module registries, tool definitions, config/theme,
LLM provider clients, the bridge, console sidecar, search/entities/meili
handles, fleet-wide caches, the id counters.

**Per-worker (one per `WorkerId`):** the context window (open panels + their
state), conversation + message log, streaming buffers, todo/thread **focus**,
spine/auto-continuation counters, cost/token telemetry, cache-breakpoint
snapshots, per-tick derived caches.

Rule of thumb: *"is this the agent's view of the world?"* → per-worker.
*"is this the machine it runs on?"* → shared. (M-long-9 sizing: ~14 shared
modules, ~8 per-worker.)

Access sites migrate from `self.state.foo` → `self.state.worker(id).foo` for the
per-worker set; shared stays `self.state.shared.foo`. This is the bulk churn.

## 6. Pipeline generalization

The single-worker chokepoints become a loop over workers:

- `process_stream_events`, `handle_tool_execution`, `finalize_stream`,
  `check_spine`, `check_my_turn_threads` → each takes a `WorkerId` (or iterates).
- The **reverie block is deleted as a special case** and re-expressed as
  "workers whose policy is background" — a reverie is just a worker with a
  reduced tool policy + Report sentinel + tool cap. One code path, two policies.

Fairness: round-robin drain each tick (every worker's `rx` is non-blocking, so
one slow LLM call never starves the others). Exactly one **active** worker is
rendered; the rest advance headless.

## 7. Persistence

`save_state` / `save_message` assume one log today. Make them per-worker:
`…/workers/<WorkerId>/{state.json, messages/}`. Shared state saves once. Load
rehydrates the worker map. (Mirrors how the bridge already keys oplog/registry
per agent.)

## 8. UI

A worker switcher (the reverie cards already render one panel per active
reverie — generalize that to "one entry per worker", with the active worker's
context window shown full). ⌘/Ctrl+number or a palette to switch `active`.

## 9. Invariants preserved

- **Single-thread mutation.** No field is touched off-loop; the worker map is
  mutated only in the loop body. No `Arc<Mutex>` needed anywhere in `State`.
- **Prompt-cache.** Shared prefix (system + tools + shared panels) stays stable
  across workers → cache hits; only the per-worker conversation/panels diverge,
  exactly as reverie does today.
- **Focus ownership.** Thread/todo focus is per-worker, so two workers can hold
  different focused threads without contention.

## 10. Risks

- **Access-site churn** (§5) is large and mechanical — mitigate with a
  `worker(id)` accessor introduced *before* the split, so call sites migrate
  incrementally while still compiling.
- **Cost/telemetry** must attribute per worker or the HUD lies.
- **Render cost** with many headless workers — cap rendered detail to `active`.

## 11. Migration phases (each shippable)

1. **Accessor seam.** Introduce `state.worker(id)` / `state.shared` over the
   *existing* single worker (id = `main`). No behaviour change; compiles green.
2. **Field classification.** Move fields behind the seam, shared vs per-worker,
   in batches. Still one worker.
3. **Worker map.** Turn the single worker into `Map<WorkerId, _>` of size 1.
   Pipeline fns take `WorkerId`.
4. **Unify reverie.** Re-express the reverie block as a background-policy
   worker; delete the special-case pipeline.
5. **Spawn/switch UI + per-worker persistence.** Allow N > 1.

## 12. Effort

**Medium–high, low-risk.** Phases 1–3 are large but mechanical and continuously
green; phase 4 removes code; phase 5 is additive. No new concurrency primitives,
so the danger is churn, not correctness.

## 13. Spine integration (the arbiter)

Today the spine is **singular**: one `auto_continuation_count`, one
`autonomous_start_ms`, one guard-rail set, one notification queue, one
"do I continue?" decision, run once per tick (`check_spine`). It is the thing
that decides **who runs and whether to keep running** — so it is the load-bearing
piece for N workers. Its job splits into two: **routing** (which worker wakes)
and **arbitration** (which ready workers may fire, and for how long).

### 13.1 State split (spine ⊂ §5)

| Spine state | Where | Why |
| --- | --- | --- |
| `auto_continuation_count`, `autonomous_start_ms`, per-turn token/message tallies | **per-worker** | "how long has *this* agent run un-prompted?" is each worker's own clock — A's runaway must not spend B's budget |
| notification inbox | **per-worker** + one **broadcast** lane | a wakeup is addressed to a worker; a few events (reload-resume) hit all |
| guard-rail limits (`max_auto_retries`, `max_duration_secs`, `max_messages`, `max_output_tokens`) | **shared policy** | one policy, *counted* per worker |
| **fleet** aggregate counters + `max_concurrent_autonomous` | **shared (new)** | N workers each under their own rail can still collectively blow a global cost/rate ceiling — needs a fleet cap |

### 13.2 Routing: notification → worker

Every wakeup carries (or resolves to) a target:

- **user message on thread T** → the worker that **owns/focuses T** (focus is
  per-worker, §9). Unowned T → assign to an idle worker, else the active one.
- **auto-read** (a thread flips `MY_TURN`, T697) → its owning worker; assignment
  policy same as above.
- **`coucou`** → the worker owning its `thread_id`; a new optional `worker_id`
  scope for worker-local timers; no scope → broadcast.
- **broadcast** (reload-resume, fleet notices) → every worker.

Starvation is free: the loop already drains every worker's channel
non-blockingly each tick (§2/§6), so B's user message is served the same tick it
is seen even while A is mid-stream.

### 13.3 Arbitration: which ready workers fire

Each tick the spine picks the subset of *ready* workers allowed to start a
continuation, gated by:

1. **per-worker rails** — a worker past its own retry/duration/msg/token rail
   **pauses itself** (stops auto-continuing; still answers direct human input).
2. **fleet rails** — the aggregate duration/token/message/cost ceiling or
   `max_concurrent_autonomous`: when tripped, **all** autonomous continuation
   pauses fleet-wide; direct human turns are still served.

LLM concurrency is fine (each stream is its own thread; state mutation stays
serialized), so "fire" can mean several workers streaming at once up to
`max_concurrent_autonomous` — or 1-at-a-time round-robin as a conservative
default.

### 13.4 Autonomy stays OFF by default (hard constraint)

Multi-worker is **not** autopilot. Auto-continuation remains **disabled by
default** (standing user mandate). With it off, the spine is a **pure router**:
each routed wakeup grants the target worker **exactly one** turn, then hands
back — identical to today's manual sailing, just fanned across N workers. The
arbiter machinery (§13.3) only engages if a human explicitly opts a worker into
autonomy, and the fleet rails are the backstop when they do.

### 13.5 UI

Per-worker notification lanes + one fleet lane. A **non-active** worker with a
pending wakeup shows a switch badge (mirrors the thread `MY_TURN` badge), so the
human sees "worker 2 needs you" without leaving worker 1.

### 13.6 Migration fit

- Per-worker spine counters ride the **field classification** (phase 2).
- Routing + the arbiter land with the **worker map** (phases 3–4); the reverie
  unification (phase 4) already forces "one continuation policy per worker".
- Fleet rails + notification lanes + switch badges land with **phase 5**.

No new primitive — the spine becomes a per-worker counter set plus a small
routing/arbitration pass over the same worker map the pipeline already iterates.
