# Multi-Worker ("Subworkers") Design

Branch: `subworkers` (off `master`).

## Goal

Run **multiple workers** inside a single Context Pilot process / Rust main loop.
Each worker is an independent agent session (its own conversation, scratch, work
panels, cost) but they **share** the project's knowledge substrate (tree
descriptions, memories, Meilisearch index, logs, entities) and the global UI
config (LLM model/provider, theme).

There is **no master worker** — all workers are equal peers. The user focuses
one at a time (the "live projection"); the others keep running in the
background.

## Locked decisions

| # | Topic | Decision |
|---|-------|----------|
| 1 | Concurrency | Background (non-focused) workers **keep running** — stream **and** execute tools. True concurrency in one loop. Focused vs. background differ **only** in rendering + input routing; switching does **not** pause the previous worker. |
| 2 | Model / theme | **Both shared** across all workers. |
| 3 | Identity | `worker_id` = **short random id** (8 hex), used as the state filename. `display_name` = free-text label shown in UI. Ctrl+S "create" **prompts for a display name**. |
| 4 | Cost | **Per-worker** token/cost counters, keyed by `worker_id`. Net-new persistence in `WorkerState`. |
| 5 | Background autonomy | **Full** — unfocused workers execute **all** tools freely, including shared-state mutations and file edits. The user owns collisions. |
| 6 | Background visibility | **Status-bar aggregate by state, no push notifications.** Counts bucketed: **Working** = {streaming, tooling, waiting}; **Needs attention** = {idle, asked-question, blocked, errored}. Badge counts background workers. |
| 7 | Guard rails | **Per-worker only.** No global ceiling — N workers can collectively overspend (accepted). |
| 8 | Reverie | **All** workers may auto-reverie, bounded by a **global concurrent cap** (`REVERIE_GLOBAL_CAP`). |
| 9 | Max workers | **Soft cap of 8.** Create blocked beyond it, with a message. |
| 10 | Delete | **Confirm in overlay** (nukes conversation/panels/scratch). Delete-last → **fresh empty worker**. Deleting mid-stream **aborts** that worker's stream. |
| 11 | File collisions | **Non-issue.** Tools serialize on the single main loop → concurrent edits are effectively sequential (normal `Edit`/`Write` last-write-wins). No warning, no lock. |

## Worker state model & status bar

Each worker carries a runtime **state** that drives the status-bar aggregate
(decision 6). Two buckets:

- **Working** (actively progressing — no user action needed):
  - `Streaming` — LLM is generating.
  - `Tooling` — executing tool calls.
  - `Waiting` — blocked on an async result (`console_wait`, coucou timer, panel
    readiness) but still progressing on its own.
- **Needs attention** (user action will unblock / direct it):
  - `Idle` — quiescent; finished its turn, nothing queued.
  - `AwaitingInput` — an `ask_user_question` form is open for it.
  - `Blocked` — a guard rail tripped.
  - `Errored` — stream errored after exhausting retries.

The status bar shows, for **background** workers only, e.g.
`▶ 3 working · ⚠ 2 need attention`, plus a worker-count badge
`⊞ active/total`. The focused worker's own activity continues to show via the
existing streaming/tooling status badge. A background worker that opens a
question form blocks **only its own** pipeline; other workers keep running.

## Shared vs. per-worker

**Shared (single instance, identical across all workers):**
- Tree descriptions (`tree` module)
- Memories (`memory` module)
- Meilisearch indexing (`search` module — global server + per-project index)
- Logs (`logs` module — *small details to refine with the user*)
- Entities (`entities` module — SQLite DB)
- LLM provider / model, theme

These modules already return `is_global() == true`, so their data already lives
in the shared `config.json` `modules` map (or in their own shared stores:
SQLite, Meilisearch, chunked log files). They are recreated as fixed panels for
each worker but back onto the same underlying store.

**Per-worker (keyed by `worker_id`):**
- Scratch (`scratchpad` module)
- Conversation messages + the Conversation panel
- Dynamic work panels (file / git / console / search-result / etc.)
- Input draft + cursor
- Local id counters (`next_tool_id`, `next_result_id`, message ids)
- Token / cost counters
- Cache breakpoint engine (tracks the per-worker prompt)
- Reverie streams
- Streaming runtime (stream rx channel, typewriter, pending tools/done)

## Existing scaffolding to reuse

The single-worker codebase was already structured with a worker seam:

- `WorkerState` (`crates/cp-base/src/state/data/config.rs`) carries `worker_id`,
  `important_panel_uids`, `panel_uid_to_local_id`, id counters, and a per-worker
  `modules` map.
- `worker::load_worker(worker_id)` (`src/state/persistence/worker.rs`) loads one
  worker's state file. Today only `DEFAULT_WORKER_ID` is ever loaded.
- `Module::is_global()` drives the global-vs-worker split in
  `build_save_batch` (`save.rs`).
- Persistence layout: `config.json` (Shared + global module data) +
  `states/{worker_id}.json` (per-worker) + `panels/{uid}.json` +
  `messages/{uid}.yaml`.

So panels and messages are **already worker-scoped** by uid mapping; the main
gaps are (1) a worker **registry** + active pointer, (2) lifecycle
(create/switch/delete), (3) the **Ctrl+S overlay** + status-bar count, and
(4) **concurrent ticking** of background workers.

## Architecture

### Worker identity
`WorkerState` gains:
- `worker_id: String` — **short random id** (8 hex), the stable key and state
  file name. Generated on create; never changes.
- `display_name: String` — user-provided label shown in the overlay/status bar.
- per-worker cost counters (tokens + USD legs) so cost survives reloads keyed
  to the `worker_id`.

### Registry (shared config)
`Shared` (`config.json`) gains:
- `worker_ids: Vec<String>` — ordered list of worker ids.
- `active_worker_id: String` — the focused worker.
- `global_next_uid: usize` — promoted to a **first-class shared field**
  (replacing the current overview-module storage), so panel/message uids stay
  globally unique across workers in the flat `panels/` and `messages/` dirs.

Migration adopts the existing single `states/main_worker.json` as worker #1
(its `worker_id` is kept as-is; `display_name` defaults to `"main"`).

### Concurrency model (phased)
True concurrency requires every worker's per-worker State slice to be live
simultaneously, while shared module state is reached by whichever worker runs a
shared-mutating tool (`memory_create`, `entity_sql`, `log_create`,
`tree_describe`, `search`).

Because the tool-execution signature is universal
(`Module::execute_tool(&self, tool, &mut State)`), there are two routes:

- **Route A (full split):** extract shared module data into an App-owned
  `SharedModules`, pass `(&mut WorkerState, &mut SharedModules)` to tools. Clean
  but ripples through every module.
- **Route B (routed TypeMap):** keep one access API; mark shared `TypeId`s and
  route `get_ext::<T>()` to a shared `Arc<Mutex<…>>` for shared modules,
  per-worker map otherwise. Tools run on the single main thread, so the mutex is
  uncontended; only streams live on background threads.

**Route B** is favored for a first concurrent cut (smaller blast radius), with
Route A as a possible later cleanup.

## Phasing

**Phase 1 — Data model + registry + persistence (no concurrency yet).**
- `WorkerState`: add `worker_id` (8-hex), `display_name`, per-worker cost
  counters.
- `Shared`: add `worker_ids`, `active_worker_id`, `global_next_uid`.
- Migration: adopt existing default worker as worker #1.
- `App` holds a worker registry; only the active worker is live (background
  **suspended**). Switching = save active slice → load target slice **without
  re-initializing shared modules**.
- Lifecycle: create (fresh id + name prompt), delete (remove files +
  registry entry; delete-last → fresh worker), switch.

**Phase 2 — Ctrl+S overlay + status bar.**
- New overlay (modeled on the Ctrl+H config / Ctrl+I index overlays): list
  workers (name, uuid short, cost, message count), highlight active, keys to
  switch / create (name input) / delete.
- Status bar: worker count badge.

**Phase 3 — Concurrent ticking (the "keep running" requirement).**
- Make `background_tick` iterate every worker: drain each worker's stream rx,
  advance its typewriter, run its tool pipeline against shared modules
  (Route B), tick its reveries.
- Only the active worker renders; background workers run headless-style.
- Guard rails / spine evaluated per worker.

**Phase 4 — Logs detail pass.**
- Decide with the user the exact shared-vs-scoped semantics for logs (e.g. one
  shared log stream tagged by worker, vs. per-worker views over a shared store).

## Implementation defaults (locked)

The following defaults were proposed and accepted — they are now locked.

1. **Route B for Phase 3 shared state.** Keep one `get_ext::<T>()` API; mark
   shared `TypeId`s and route them to `Arc<Mutex<…>>`. Only shared-route
   modules with **real authoritative in-memory state**: memory, logs, tree,
   callback definitions, prompt library. External-backed shared modules
   (entities → SQLite, search → Meilisearch) keep cheap per-worker caches
   pointing at the same backing store — no `Arc<Mutex>` needed.
2. **Watchers = union** across all workers (refcounted path set). Forced by
   full bg autonomy: a background worker's file/console/git panels must stay
   live so its tools see fresh content and the user sees current state on
   switch.
3. **`global_next_uid`** promoted to a first-class `Shared` field (dropped
   from the overview-module hack). Seeded from `max(existing UIDs on disk)`
   on migration. Single shared counter → no collision in flat `panels/` +
   `messages/` dirs. For Phase 3 (concurrent ticks): plain `usize` mutated by
   whichever tick is active (tools serialize on the main loop, so no true
   parallel increment — no atomic needed).
4. **Phase 3 tool-execution fairness = round-robin.** Each worker advances its
   pipeline one step per tick, iterated in registry order. Focused worker
   renders; all workers tick.
5. **Ctrl+S overlay keymap:** `↑`/`↓` navigate, `Enter` switch, `n` create
   (inline name prompt), `d` delete (confirm), `Esc` close.
6. **`SCHEMA_VERSION` 1 → 2** + migration adopts `main_worker` as worker #1
   with `display_name = "main"`.
7. **`REVERIE_GLOBAL_CAP` = 2** concurrent reverie sessions across all workers.
   Per-agent cap (one per agent type per worker) stays.
8. **Per-worker cost persistence:** add `cost_hit_usd`, `cost_miss_usd`,
   `cost_output_usd`, `cache_hit_tokens`, `cache_miss_tokens`,
   `total_output_tokens`, `uncached_input_tokens` to `WorkerState`. These
   fields are currently NOT persisted at all (reset on reload) — this makes
   them durable per worker.
9. **Phasing scope:** Phase 1+2 definitive, Phase 3 well-specified but
   flagged, Phase 4 (logs detail) deferred with user.

## Derisking — code-exploration findings (anterior work)

Concrete findings from auditing the single-worker code before writing any. The
goal: surface downstream surprises now.

### Authoritative shared/per-worker split (grepped all modules)
- **Shared** (`is_global() == true`, data in `config.json` `modules`): files,
  firecrawl, tree, memory, brave, logs, entities, callback, prompt, ocr,
  conversation, conversation_history, overview, questions (14).
- **Per-worker** (`is_global() == false`, data in `states/{worker}.json`
  `modules`): queue, git, spine, scratchpad, todo, console, github, search (8).
- Note: `conversation`/`conversation_history` are `is_global=true` but that only
  governs their tiny module-config blob — the **messages themselves are
  per-worker** (referenced via `WorkerState.important_panel_uids` →
  `panels/{uid}` → `messages/{uid}.yaml`). Confirmed.
- `search` is per-worker (radar / task-signals) but the Meili index + server are
  global/external — relevant to the Phase 4 logs detail pass.

### Persistence facts
- `DEFAULT_WORKER_ID = "main_worker"` (defined twice: cp-base + src/infra).
- `states/`, `panels/`, `messages/` are **flat shared dirs** (no per-worker
  subfolder). Panels/messages are scoped to a worker only via that worker's
  `WorkerState` uid maps.
- `global_next_uid` (source of `UID_n_{P,U,A}` ids) **is persisted** — through
  the `overview` module (`is_global=true`) into `config.json`. So it is already
  a **project-wide shared monotonic counter**. Good for Phase 1 (one live
  worker); for Phase 3 (N live States) it must become a single atomic shared
  counter to avoid id collisions in the flat dirs.
- **Token/cost counters are NOT persisted anywhere** (`save.rs` never writes the
  `State.cost_*` / `*_tokens` fields). Per-UUID cost is therefore net-new
  persistence to add to `WorkerState`.

### Streaming channel
- A **single** `(tx, rx)` pair is created in `main.rs` and passed by reference
  into the loop via `EventChannels`; every `start_streaming(params, tx.clone())`
  shares it. `process_stream_events(app, rx)` already takes `rx` as a parameter.
- Phase 3 concurrency needs **per-worker channel pairs** (events from concurrent
  workers would otherwise interleave on one channel with no worker tag).

### UI templates (clean, copyable)
- Overlay pattern = a bool flag (`flags.config.config_view` for Ctrl+H,
  `flags.overlays.index_status` for Ctrl+I) gated in `events.rs` + a render path.
  Ctrl+S → add `flags.overlays.workers_overlay` + `Action::ToggleWorkersOverlay`
  + a render fn.
- Name prompt on create → reuse `PendingForm` (`cp-base/ui/question_form.rs`) or
  a simpler inline text field in the overlay state. Leaning inline text field.
- Status bar = `StatusBar` (`cp-render/frame.rs`) with `auto_continue`/`queue`
  cards built in `sidebar.rs`. Add a worker-count badge there.

### Risk register

| ID | Sev | Phase | Risk | Mitigation |
|----|-----|-------|------|------------|
| S1 | MED | 1 | Per-worker `module_data` must swap on switch for the 8 per-worker modules **without** reinitializing the 14 shared modules (else memory/logs/tree in-memory state resets). | A `switch_worker` path that calls `save_worker_data`/`load_module_data` for per-worker modules **only**. |
| S2 | MED | 1 | `State.context` mixes shared-fixed panels (Memories/Tree/Entities/Logs/Search/Overview/Library) with per-worker (Conversation/Scratch/File/Git/Console). | On switch, keep shared-fixed panels, swap per-worker ones. Classifier = `is_fixed` + owning module `is_global`. |
| S3 | LOW | 1 | Cost counters are currently not persisted. `global_next_uid` is already shared via overview module (fine for one live worker). | Persist per-worker cost in `WorkerState`. Promote `global_next_uid` to `Shared`. |
| S4 | HIGH | 3 | Single stream channel → concurrent workers' events interleave undemuxable. | Per-worker `(tx, rx)` in `WorkerRuntime`. `process_stream_events` is already parameterized on `rx`. |
| S5 | MED | 3 | 59 `self.state` uses in `lifecycle.rs` + App-level streaming runtime (typewriter/pending/reverie/rx) assume one worker. | Move per-worker runtime into a `WorkerRuntime` struct; extract a per-worker tick fn iterated over all workers. |
| S6 | MED | 3 | Shared module in-memory state (memory/logs/tree `Vec`) must be single-instance while N States are live. | Route B (mark shared `TypeId`s, route `get_ext` to `Arc<Mutex>`) — or extract `SharedModules`. |

### Recommended Phase 1 ordering
1. `WorkerState`: add `worker_id` (8-hex), `display_name`, per-worker token/cost
   counters (+persist).
2. `Shared`: add `worker_ids: Vec<String>`, `active_worker_id: String`,
   `global_next_uid: usize`.
3. Migration: adopt existing `main_worker.json` as worker #1 (`worker_id` stays
   the file name; `display_name = "main"`).
4. `switch_worker()`: save the active per-worker slice (per-worker module data +
   panels + messages + counters) → load the target slice, **keep shared modules
   in place**, rebuild `context` (shared-fixed kept + target per-worker panels).
5. create / delete lifecycle (delete-last → fresh worker). Background suspended
   for Phase 1.
