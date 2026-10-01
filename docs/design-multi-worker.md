# Design: Multi-Thread Core Loop

> Status: **DRAFT v8 — thread-centric rewrite.** Iterating with Guillaume. Nothing here is implemented yet.
> ****Disclaimer — the one place the old vocabulary survives.** Earlier drafts (v1–v7) were *worker-centric*: a fixed pool of **worker** entities roamed over threads, acquiring an exclusive **focus lease** on one thread at a time. That framing is **superseded**. The unit of execution is now the **thread itself** — there is no separate worker entity. A reader of the old draft can map across with this table; the word "worker" does **not** appear anywhere below this disclaimer.
>
> | v1–v7 (worker-centric) | v8 (thread-centric) |
> | --- | --- |
> | a *worker* (execution entity) | the **thread** is its own executor |
> | *worker context* (conversation + panels + budget) | the **thread's** context |
> | *fixed pool of 2 workers* | **concurrency cap** `K = 2`: at most K threads executing at once |
> | *focus lease* (thread ↔ worker), contention, acquire/release | **deleted** — a thread never competes for itself |
> | *dispatch* = assign an idle worker to an unowned thread | **promotion** = give a waiting Runnable thread a free active slot |
> | the *Dispatcher* | the **promoter** (the scheduler gate that fills free slots) |
> | `worker[0]` (the ex-main worker) | no special thread; the on-screen thread is just one of many |
> | existing code identifiers `WorkerState`, `worker_id`, `states/<worker>.json` | kept as-is in code for now; conceptually they back a **thread's** execution state (rename is an implementation detail) |

## 1. Purpose

Today the core loop drives exactly **one** thread: a single LLM-driving conversation with a single context (panel stack + message history + cache state).

Goal: let the **same core loop** drive **multiple threads simultaneously**, where:

- Threads share one set of backing **state** stores (todos, memories, entities, tree data, callbacks, logs, search index, behaviours) — these are **never duplicated**.
- Each thread has its **own context**: its own conversation/message history, its own open panel set + view state, its own context/cache budget.
- All threads run the **same LLM model** and the **same agent behaviour** (both fleet-wide, not per-thread).
- Concurrency is **bounded**: at most `K` threads may be actively executing at any instant (`K = 2` initially). This cap is the *only* thing left of the old fixed pool — demoted from an entity to a number.

## 2. Definitions

| Term | Meaning |
| --- | --- |
| **Thread** | The unit of execution: a conversation with its own message history, context window, open-panel set + view state, context budget, and execution state. Same model and same agent behaviour as every other thread. |
| **Context** | Everything sent to the LLM for a thread: its panel stack + conversation. **Per-thread.** |
| **State** | The backing data stores behind the panels. **Shared, single instance.** |
| **Execution state** | A thread's position in the loop: `Idle / Runnable / Streaming / AwaitingLLM / AwaitingTool / Errored` (§5.2). |
| **Active set** | The threads currently being advanced (streaming / stepping). \*\*\` |
| **Waiting** | A thread that is `Runnable` but has no active slot yet — queued for promotion (§7.3). |
| **Step** | The smallest unit of thread progress the loop advances in one tick (§5.2). |
| **Fleet** | The full set of threads + reveries (subordinates) — one table, driven by the one loop. |

## 3. The Shared / Per-Thread Partition `RATIFIED`

The codebase already has a **shared / per-instance split**: the `Module` trait's `is_global()` decides whether a module's data persists to shared `config.json` or to a per-instance state file. That flag is the ground truth we start from — but it reflects the *single-thread* world, so a few modules are flagged `global` **only because there is one thread today** and must become per-thread under this design (marked **FLIP** below).

### 3.1 Exhaustive module classification

Every module (24 + core default), its current `is_global`, and its multi-thread target bucket:

| Module | `is_global` today | Multi-thread bucket | Note |
| --- | --- | --- | --- |
| memory | true | **Shared state** | M-slots, single instance |
| entities | true | **Shared state** | SQLite, single-writer |
| logs | true | **Shared state** | fleet-wide log |
| todo | true | **Shared state** | thread-owned → already keyed by thread |
| scratchpad | true | **Shared state** | thread-owned → already keyed by thread |
| threads | true | **Shared state** | the thread list + messages (the registry every executing thread lives in) |
| callback | true | **Shared state** | file-edit hooks are fleet-wide definitions |
| prompt | true | **Shared state** | system prompt / behaviour = fleet-wide (§1) |
| bridge | true | **Shared state** | orchestrator link / tee socket = fleet infra |
| tree | true | **Split** | data + descriptions shared; **folder-expansion per-thread** |
| files | true | **Split** | file contents on disk shared; **open file panels per-thread** |
| search | false | **Split** | Meili **index shared**; results panel per-thread |
| git | false | **Split** | the **repo is shared**; git status panel per-thread view |
| github | false | **Split** | remote is external; `gh` result panels per-thread |
| console | false | **Per-thread** | processes spawned by a thread; panel + watcher owned by it |
| firecrawl | true | **Per-thread job** | stateless tool; in-flight crawl = watcher owned by caller |
| brave | true | **Per-thread job** | stateless tool; results panel per-thread |
| ocr | true | **Per-thread job** | stateless tool; in-flight OCR = watcher owned by caller |
| queue | false | **Per-thread** | each thread batches its own edits |
| overview | true | **Per-thread view** | each thread renders its own HUD (budget/context) |
| questions | true | **Per-thread** | think-reminder cadence per-thread; forms are thread-scoped |
| **conversation** | true | **Per-thread — FLIP** | the live conversation IS the per-thread context (§1 premise) |
| **conversation_history** | true | **Per-thread — FLIP** | each thread has its own message history |
| spine | false | **Split** | per-thread inbox + guards; **promotion/routing = fleet** (§7) |
| *(core default)* | false | per-thread | any module not overriding `is_global` |

### 3.2 The three buckets, consolidated

**(A) Shared state — single instance, all threads read/write**: memories · entities · logs · todos (thread-keyed) · scratchpad (thread-keyed) · the thread registry (list + messages) · callbacks · active LLM model · active agent behaviour / prompt (fleet-wide) · behaviours library · bridge infra · tree **data + descriptions** · search **index** (Meili) · the working tree / git repo on disk.

**(B) Per-thread context — own instance each**: conversation + **conversation history** (the FLIP) · open panel set + panel view state · **tree folder-expansion** state · overview/HUD view · context budget + cache/freeze state (K-anchor, emission ranks) · execution state + active-slot membership · queue (edit batch) · per-thread spine inbox + anti-loop guards · the per-thread *views/sessions* over shared substrates (git/search/github/console/files panels) · in-flight jobs + watchers **owned by the registering thread** (console, web scrape/search, OCR — §7.2).

**(C) Fleet-level — genuinely new, neither pure shared-data nor per-thread**: the thread **execution registry** + the **active set / concurrency cap** (`cp-fleet`, §12) · the notification router / promotion policy (spine, §7) · the loop that advances runnable threads (§7.4).

**Governing pattern:** a panel that renders shared state is a **per-thread view over the same underlying data**. The **tree is the canonical example** — descriptions shared, folder open/closed per-thread. `files`/`search`/`git`/`github`/`console` follow the same shape: shared (or external) substrate, per-thread panel/session.

**Load-bearing consequence:** the two **FLIP** modules (`conversation`, `conversation_history`) are the mechanical heart of the change — they move from the shared store to the per-thread store. Everything else is already on the correct side or is a view-state refinement.

## 4. No Lease — the Thread Is the Executor `DECIDED`

The old exclusive focus lease is **deleted**. It existed only to arbitrate *which roaming executor owns which thread* — a question that no longer exists once the thread **is** the executor.

- A thread **never competes for itself**: there is no acquire, no contention, no refusal, no release, no dual source of truth. A thread with work is simply `Runnable`.
- **Human view-focus is a pure UI concern.** "Which thread is on screen" (§9) selects what the human looks at; it does **not** gate execution. A thread advances because it is `Runnable` and holds an active slot — never because a human is looking at it.
- The only remaining scarcity is the **concurrency cap** `K` (§7.3): more Runnable threads than slots means some wait. That is a scheduling decision, not an ownership one.

This single change dissolves a whole class of former problems: a stuck thread strands only *itself* and can never block another; there is nothing to "reassign"; dispatch has nothing to arbitrate (§13).

## 5. Concurrency Model `DECIDED: cooperative, yield-based, lock-free`

**Decision (accepted): cooperative, single-threaded, yield-based — NOT async/multi-thread.** The rest of this section is the justification the choice depends on, because the naive reading of "cooperative" hides a trap.

### 5.1 What "a step" actually is today

The premise behind cooperative is *"a loop step never takes time — JAMAIS."* That is true for **one meaning** of "step" and false for another:

- **(a) Event-loop tick** — render a frame, handle one input, apply one delta. Sub-millisecond. **Never blocks. Premise holds.**
- **(b) Thread turn-advancement** — get the next LLM chunk, run a tool, apply its result. As currently written this **can block for a long time**:
  - LLM API round-trip / streaming: seconds → minutes (the I/O wait we want to overlap across threads).
  - Blocking tools: a `cargo`/clippy **blocking callback up to 180s**, `console_wait` up to 60s, `firecrawl_crawl` minutes, synchronous git/file IO.

If "cooperative" meant *"fully advance one thread's turn, then the next,"* a thread stuck in a 3-minute build would **freeze the whole fleet** — killing the only reason to run threads concurrently. So pure blocking-cooperative is a non-starter.

### 5.2 The invariant + the definition of one step `ACCEPTED`

> **INVARIANT (load-bearing): no loop step may block. Every LLM call and every tool is either instantaneous or represented as a *pending job* the thread parks on, yielding control back to the loop.**

**One** `step` for a thread is exactly one of, and never more:

- consume **one** streamed LLM chunk, or
- apply **one** tool result that arrived from a background job, or
- execute **one** non-blocking (instantaneous) tool call, or
- **park** on a blocking/async tool (register its background job + watcher) and yield.

Every step is bounded and non-blocking by construction. This precise definition is what makes the loop-advancement rule (§7.4) trivially fair.

The loop cooperatively advances the thread state machines, each in one of:

- `Runnable` — has queued work (LLM chunk consumed, tool result to apply, fresh notification). Eligible to be advanced one step this tick **if it holds an active slot** (§7.3).
- `Streaming` / `AwaitingLLM` — a request/stream is in flight on a background socket; parked until the next chunk arrives. **Holds an active slot** (it is consuming the rate-limited LLM resource).
- `AwaitingTool` — a blocking tool (console/callback/web/OCR) runs on a background thread; parked until its watcher fires. **Releases its active slot** (the LLM is idle); re-enters the Runnable queue when the result lands.
- `Idle` — no work (its thread is THEIR_TURN or finished). Not in the active set.
- `Errored` — last turn failed; surfaced in the TUI (§8); re-engaged like the single-thread error path today.

Interleaving happens at **await points** (chunk boundaries, tool-park boundaries), not whole-turn boundaries — this is what stops a slow thread from starving the others.

**Payoff:** single-threaded ⇒ **zero locks on shared state**. Memories (fixed-slot), SQLite (single-writer), tree, callbacks, logs — all mutated by one thread-of-control, serialized by construction. No data races, no lock-ordering, no torn multi-store updates. Given how shared-mutable this codebase is, that is the biggest single win available and the real reason to prefer cooperative to async.

### 5.3 The reverie already proves this works

The "park on a background job, resume when a watcher fires" shape — and the "multiple agents cooperatively interleaved on one thread-of-control, sharing state, lock-free" shape — **already exist and ship today as the reverie** (§11). The reverie is the existence proof that the §5.2 invariant is satisfiable on this loop. We are generalizing a proven pattern, not inventing one.

Also already async with completion watchers: console processes, callbacks, web search/scrape, OCR, and the LLM stream (watchdog-timed). So the conversion is mostly: (1) make the **LLM turn** a resumable state machine that yields at chunk boundaries, and (2) keep **synchronous IO off the hot path**.

### 5.4 The options, with downsides (for the record)

- **(A) Pure cooperative, single thread, yield-based** — CHOSEN.

  - PRO: zero locks; simple consistent reload/snapshot; matches "same exact loop"; reverie already demonstrates it.
  - CON: must audit **every** synchronous op and make it yield-based; the LLM streaming loop must become resumable.
  - RISK: one careless synchronous tool (sync `reqwest`, big blocking `std::fs`) silently reintroduces head-of-line blocking. **Mitigation:** a discipline guard/lint — no blocking IO on the loop thread.
  - RISK: a genuinely CPU-bound step (huge prompt build, freeze/cache pass) blocks all threads for its duration. **Mitigation:** these are bounded and cheap (cache DP is O(k·|gap|²)); keep them bounded.

- **(B) Async tasks (tokio), lock the shared state** — REJECTED.

  - CON: `Mutex`/`RwLock` everywhere; every single-threaded invariant re-audited under interleaving; hard consistent reload across live tasks.
  - RISK: deadlocks, lock-ordering, torn updates — large subtle bug surface on a shared-mutable codebase. "Same exact loop" lost.

- **(C) Actor framing of (A)** — the concrete shape we adopt: the loop thread-of-control owns all shared state (lock-free); blocking work runs on background threads and posts results back over channels; the loop applies each result on the owning thread-of-control.

## 6. Shared-State Races `RESOLVED by §5`

Single-thread-of-control ⇒ **no concurrent writers**, so the race questions dissolve. Todos/scratchpad are thread-keyed and touched only by the thread they belong to; memories/entities/tree/callbacks/logs are mutated by one thread-of-control; the search index is already concurrency-safe.

## 7. The Spine: Thread-Addressed Notifications `DECIDED`

The spine's job is **unchanged in kind**: relaunch a thread that has stopped advancing but still has work to do, by **sending it a notification**. The spine does **not** "advance ticks" — that is the loop's job (§7.4). What changes for N threads is only that **every notification now names the thread it concerns** — which is natural, since the notification's content belongs to that thread's own conversation.

### 7.1 Today (single thread)

- **Notifications**: injected into the single GLOBAL message stream; auto-continuation relaunches the one agent when work remains.
- **WatcherRegistry**: `poll_all` fires watchers → each fire creates a notification into that same global stream.
- Neither a `WatcherResult` nor a `Notification` carries a thread id — one recipient, so none was needed.

### 7.2 Rework for N threads `DECIDED`

1. **Notifications become thread-addressed.** Every notification resolves to a target thread; its message is injected into *that thread's* conversation, not a global stream.
2. **Watchers carry their owner.** A thread registering a watcher (`console_wait`, `coucou`, channel, OCR, callback) tags the **owning thread id**. On fire → notify **that thread**. The result returns to the **owning thread's conversation** — the one holding the pending `tool_use`. (Conversations are per-thread, §3.)
3. **A user message in a thread** is delivered to **that thread** (it already is the recipient — the conversation is the thread's own).
4. **The "relaunch idle agent" hack disappears.** A thread that is MY_TURN with no in-flight step is just `Runnable` and gets advanced once it holds an active slot; a thread gone THEIR_TURN parks `Idle` until a notification targets it.

### 7.3 Promotion — filling active slots `DECIDED`

With more Runnable threads than the cap `K` allows, the **promoter** (a `cp-fleet` component, §12) decides which Waiting threads enter the **active set** each tick:

- A MY_TURN thread with work is `Runnable`. If the active set has a free slot (`|active| < K`), the promoter moves a Waiting thread into it, which lets the loop advance it (§7.4).
- When an active thread finishes its turn (goes `Idle`/THEIR_TURN) or parks on a tool (`AwaitingTool`, §5.2), it **frees its slot**; the promoter fills it next tick.
- If no slot is free, Waiting threads simply wait — that is the intended, bounded behaviour of the cap. There is **nothing to arbitrate**: a thread is its own executor, so promotion never has to resolve ownership, only pick from the Waiting queue.
- **Selection policy** = oldest-waiting thread first (stable, starvation-free).

This generalizes today's single-thread `check_my_turn_threads` / `inject_direct_auto_read`: the same "a MY_TURN thread needs attention" trigger, now choosing *which* Waiting thread gets the next free slot. It also fulfils the **unowned-thread visibility** requirement: a freshly-arrived MY_TURN thread is picked up by the promoter as soon as a slot frees, and surfaced in the thread list meanwhile (§9).

### 7.4 Loop advancement — NOT a spine duty, NOT a "scheduler entity" `DECIDED`

Called out explicitly to draw the line: **advancing threads is the loop, not the spine.** Each loop tick advances **every active** `Runnable` **thread by exactly one step** (§5.2). Because a step is bounded and non-blocking, advancing all active runnable threads each tick is naturally fair and starvation-free — no priority queue, no round-robin cursor. This is **exactly how the loop already interleaves the reverie streams** today; we keep that behaviour and let every thread join the same rotation. The only gate on top is the `K` cap (§7.3), which bounds how many threads are in the active set at once.

### 7.5 Notification routing is TOTAL `DECIDED — non-negotiable`

**Every notification targets exactly one thread.** There is no "notify nobody" outcome — the whole reason cp-spine exists is to relaunch a stopped thread, so a notification that resolves to no thread is meaningless. Routing is a **total function →** `thread_id`, resolved in this order:

1. **Explicitly addressed** → that thread.
2. **Watcher fire** → the watcher's owning thread (§7.2.2).
3. **User message in a thread** → that thread (the conversation is the thread's own).

**Corollary:** an event that targets no thread is **not a notification** — it is a plain state update the loop reads. Example: a genuinely global event ("meili reindexed", "reload complete"). A MY_TURN thread that merely needs a free slot is **not** a notification either — it is `Runnable` thread *state* the promoter (§7.3) reads each tick. If the spine emits a notification, it names a thread.

## 8. Lifecycle & Failure `DECIDED`

- **Concurrency**: **cap** `K = 2` active threads for now. The fleet crate (§12) supports raising K; it is static = 2 initially.
- **Failure isolation**: a thread's turn erroring (guardrail, API error) does **not** stall the others (cooperative → the loop moves on, and the errored thread frees its slot). The failing thread goes `Errored` and is **surfaced loudly in the TUI** (§9) — a stuck thread strands only itself, and the human is told which one.
- **Re-engagement = same as single-thread today.** An `Errored` thread is re-engaged exactly like the single-thread error path today (human re-engages, or it is re-woken). There is nothing to transfer — deliberately kept simple for now.
- **Reload/restart**: single-thread-of-control snapshot of the thread execution registry + each thread's context. Parked background jobs (console/watchers) reattach to the right thread on resume (they carry their owning thread id, §7.2). In-memory channel-backed jobs (async tool channels, the LLM stream) do not survive a reload and are restarted, not reattached.

## 9. UI Surface (TUI) `DECIDED — TUI only; web frontend catches up later`

**Scope:** the TUI is the only surface for now; the web frontend is deferred to a later pass. Everything below is the TUI navigation model.

### 9.1 Navigation model

- **Drop** `Ctrl+V` as the view switch (current behaviour removed).
- **Left / Right arrows** move *between* views (out / in).
- **Up / Down arrows** move the selection *within* a list.

Two levels: a **general view** and the **detail views** it drills into.

```
        ┌──────────────── General view ────────────────┐
        │  Threads                                      │
        │  ─────────                                    │
        │  ▸ T765  Streaming        ●                   │
        │    T766  Waiting                              │
        │    T767  Idle (THEIR_TURN)                    │
        │    T773  Errored  ⚠ stuck                     │
        │                                               │
        │  [create thread]                              │
        └───────────────────────────────────────────────┘
             │ Right on a selected thread → Thread view
             ◄ Left from a detail view → back to General
```

- **General view**: the **thread list**, each row showing the thread's execution state (`Streaming`/`AwaitingLLM`/`AwaitingTool`/`Runnable`/`Waiting`/`Idle`/`Errored`) and an **attention marker** when it needs the human (`Errored` / `⚠ stuck`). `Up`/`Down` move the selection; `Right` drills into the selected thread's **Thread view**; `Left` returns here.
- **Thread view**: that thread's own context — its open panels + conversation — the closest analogue to today's main screen, one per thread.
- Navigating the UI is **inspection**, not execution control: opening a thread view does **not** promote it into the active set. Promotion (§7.3) is governed by the `K` cap and the Runnable/Waiting queue, never by human navigation.

### 9.2 Actions in the general view

- **Create thread** — opens a textarea for the initial message, then creates the thread. A new MY_TURN thread is `Runnable` and competes for a slot like any other.
- A thread is retired by the existing **delete / archive thread** flow; archiving removes it from the active/Waiting set.

(Exact keybindings are a minor detail to settle at implementation — e.g. `n` new thread.)

Reveries already render one background card each; that surface is unchanged and coexists with the thread list.

## 10. Non-Functional Requirements `DECIDED`

- **Context budget**: **independent per thread** (each its own budget).
- Concurrency: **cap** `K = 2` now; the memory hint "2–4 simultaneous projects" is the eventual target once K is raised.
- Fairness: advance-every-active-runnable-per-tick (§7.4) + oldest-waiting-first promotion (§7.3) — no further latency policy needed.

## 11. Unifying with the Reverie `RATIFIED`

The reverie is already the prototype of everything in §5/§7, so multi-thread **generalizes the reverie machinery** rather than building a parallel one.

### 11.1 What the reverie already is

- `HashMap<agent_id, ReverieState>` — a **table of concurrent cooperatively-scheduled agents**.
- Per-agent stream channels (`HashMap<agent_id, ReverieStream>`), **cooperatively interleaved on the single loop**.
- Shares all state, uses the **same** `dispatch_tool` and the same prompt builder (`prepare_stream_context`).
- One UI card per active reverie.
- Auto-activates the Queue on start; empties it on destroy/timeout.

That is *exactly* "N agents cooperatively multiplexed on one loop, sharing state, one stream each" — the multi-thread engine, already shipping.

### 11.2 The merge: one execution primitive, two roles `RATIFIED`

There is **one** underlying primitive — a *cooperatively-scheduled execution instance* with its own stream, conversation, and tool dispatch. A **thread** and a **reverie** are two **roles** of it:

| Aspect | **Thread** (peer) | **Reverie** (subordinate) |
| --- | --- | --- |
| Context | own full context (panels + conversation + budget) | derived/scoped lens on a parent |
| Behaviour | shared fleet-wide agent behaviour | fixed built-in (cleaner, cartographer) |
| Counts against `K` | yes | no (background, parent-scoped) |
| Lifetime | persistent | transient (timeout-bounded) |
| Tool cap | none | 50 |
| Driven by | the loop; woken by the spine + human | a parent's directive; auto-terminates |
| Visibility | human-facing, in the thread list | background card |

**Ratified:** (a) one execution primitive / two roles; (b) there is no special "main" instance — every thread is an equal entry in the registry; (c) reveries are subordinate entries in that same table. The old "main-tool phase then reverie phase" collapses into "the loop advances the active runnable set" (§7.4). This is a *generalization* of proven code, not a rewrite — the strongest de-risking argument in the whole design.

**Open point (per review):** a reverie is *not* a thread (it has no user-facing conversation and holds no slot against `K`). It stays a distinct role rather than folding into "thread." Confirmed: the primitive is the *execution instance*; "thread" and "reverie" are its two roles.

## 12. The `cp-fleet` Crate `DECIDED`

A **new crate,** `cp-fleet`, owns the *mechanism* of the fleet:

- The **thread execution registry**: `thread_id → { role, execution state, runnable flag, active-slot membership, budget }` — generalizing `HashMap<agent_id, ReverieState>`. (Agent behaviour is fleet-wide, not stored per entry; reveries carry their fixed built-in role.)
- The **active set + concurrency cap** `K`: the bound on how many threads execute at once, and the membership test the loop uses.
- The **promoter**: the pickup *policy* (§7.3) — reads the registry + thread state and moves Waiting threads into free active slots (oldest-waiting first). It decides *which* threads get slots; there is no ownership to resolve.
- **Lifecycle utilities**: register / retire / list / lookup / count; enforce the cap `K`.
- Absorbs / wraps the reverie session machinery (§11).

**Mechanism vs policy (ratified):**

- `cp-fleet` = **mechanism/storage** (execution registry, active set, lifecycle) **+ the promoter** (slot-fill policy, §7.3).
- The **spine** = the **notification engine** — delivers/injects thread-addressed notifications and drives auto-continuation (unchanged from single-thread, now per-thread).
- The **loop** = **advancing active runnable threads** (§7.4). There is no separate "scheduler" entity.

The spine reads/mutates the registry through `cp-fleet`'s API but holds no storage itself. **There is no lease map** — it was deleted with the lease (§4).

## 13. Adversarial Review — Resolutions `CLOSED`

Full post-ratification stress test, triaged with Guillaume. Every item is either **resolved** (folded into the design), **dropped** (with the reason), or **dissolved** (the thread-centric model of §4 removes the problem by construction). Nothing here blocks the architecture.

### Dissolved by the thread-centric model (§4)

- **H3 — Single source of truth for the lease.** Moot: there is no lease. A thread *is* its executor; its execution state lives in one place (the registry), with nothing to mirror.
- **H6 — A held thread is stranded by a stuck executor.** Reframed: a stuck thread strands only *itself* (nothing else depends on it). Accepted for now with human re-engagement, and the stuck thread is **surfaced loudly in the TUI** (§8/§9).
- **H7 — Dispatch livelock.** Moot: there is no assignment loop to thrash — promotion just fills free slots from the Waiting queue.
- **H11 — Same-tick double-dispatch.** Moot: a thread is never "assigned" to two executors, because there are no executors separate from the thread.

### Resolved

- **H1 — Notification injection timing.** Keep the existing single-thread notification logic **unchanged**: content is injected at a turn boundary, and `create_notification` already refuses to inject between a `tool_use` and its `tool_result`. No new drain mechanism — each thread drives its own inbox through this same path.
- **H2 — Callbacks under concurrency.** Callbacks are **per-thread instances of shared definitions**: a firing is owned by the thread whose edits triggered it (its console session + watcher live in that thread's containers), and its result returns to that thread. Dedup (`active_sessions`) is keyed **per-thread** (thread + callback id) so one thread's edit can never kill another thread's in-flight run of the same callback. **No locking, no serialization** of the subprocesses. The only cross-thread addition is **informational**, gated by a new `concurrency_friendly` flag on the callback definition: on **timeout**, if another thread is running the same callback, append a note to the (detached-panel) result ("another run of this callback is in flight"); on **failure**, if another thread runs it concurrently or ran it recently, append "this failure may be caused by that concurrent run."
- **H4 — LLM stream collection.** Each thread has an **async stream collector outside the main loop** whose sole job is to gather provider deltas during a stream. The loop merely polls "still gathering? / new chunks?". This replaces any notion of a "resumable stream state machine."
- **H5 — Shared token + rate limit.** Industry-standard: a **fleet-level single-flight** OAuth refresh (one refresh in flight, result shared; others wait and reuse it) + standard `429`/rate-limit backoff & retry. Never refreshed mid-stream — only between calls. The cap `K` also bounds concurrent LLM pressure directly.
- **H8 — Background-job ownership across reload.** Every background job records the **id of the thread that launched it**; on reload each job reattaches to its owner. (Realised structurally by H15.)
- **H10 — The promoter.** A `cp-fleet`-owned component (§7.3, §12) that fills free active slots from the Waiting queue (oldest-waiting first) and fulfils unowned-thread visibility. It is **policy only** and has no ownership to resolve.
- **H12 — No file-level lease.** Assumption: concurrent threads operate on disjoint files. No per-file locking in this phase.
- **H13 — Reverie edit queue** becomes **per-instance** (consistent with the queue partition, §3).
- **H14 — Reverie↔thread interleaving.** The merge drops the old "main-tools-then-reverie-phase" ordering; individual tool calls stay serialized, and the nondeterministic interleaving is acceptable.
- **H15 — Structural resource teardown.** Ownership is **by container, not best-effort bookkeeping**:
  - **Per-thread watcher registry** — retiring a thread drops its registry, so every watcher (console, callback, coucou, OCR, web) vanishes *by construction*.
  - **Thread-prefixed console session keys** + a new **prefix-kill** command on `cp-console-server`, so the authority deterministically reaps *all* of a thread's processes — even ones the TUI forgot.
  - Teardown sequence (deterministic): stream collector → watcher registry → all console/callback processes (prefix-kill) → context (conversation + history, panels and their `panels/<uid>.json`, queue, spine inbox) → the thread's execution-state file.
- **H16 — Guardrails.** Guardrails are **per-thread** (already so in the spine config), **no fleet ceiling**. The planned **removal** of `MaxOutputTokens` / `MaxDuration` / `MaxMessages` (keeping `MaxAutoRetries`) is **deferred to the implementation phase** — see the backlog below. Anti-flood: the existing mechanisms stay (documented below); any generalization is deferred.
- **H17 — Panels & attention.** Panel UIDs are already per-instance-safe (per-instance maps + a global uid counter). Cross-thread attention is surfaced by an **indicator in the thread list** (`Errored` / `⚠ stuck` / awaiting-input highlighted). **Hard invariant: a thread's detail view is pixel-identical to today's panel-view — nothing changes.**

### Dropped

- **H9 — Per-tick cache recompute.** Rejected: the cache/context build is a fragile, precise mechanism and **must not be modified**. It is **duplicated per thread verbatim**; no "recompute-only-on-change" optimisation.

### Existing notification anti-flood (documented, for reference)

Kept as-is; becomes per-thread under the inbox split:

1. **5 s cooldown** in the idle-MY_TURN detector (`cp-mod-threads/src/watcher.rs`: `COOLDOWN_MS` + `last_fired_ms`) — the fix for the 1300-notification flood; a fuller guard was deliberately deferred.
2. **Dedup** in `create_notification` — no two unprocessed notifications with the same `kind`+`source`.
3. **GC cap** at 100 stored notifications.
4. `last_synthetic_unanswered` — no two synthetic auto-continuations before the assistant answers.
5. **Exponential backoff** (`2^errors`, capped 60 s) after continuation errors; `user_stopped` hard stop.

### Implementation-phase backlog (deferred, not design-blocking)

- Remove the three guardrails (`MaxOutputTokens` / `MaxDuration` / `MaxMessages`): contained to `cp-mod-spine` (`guard_rail.rs`, `types.rs`, `engine.rs`, `tools.rs`) + `yamls/tools/spine.yaml`; `autonomous_start_ms` becomes dead with `MaxDuration`. Keep `MaxAutoRetries`. Serde-safe (old persisted keys ignored on load).
- Decide whether to generalise the anti-flood cooldown beyond the MY_TURN detector (a per-source cooldown).
- Confirm the exact rule for whether an `AwaitingTool` thread yields its `K` slot (§5.2) vs holds it; current decision = yields.

---

## Decisions Log

- **Thread-centric pivot (v8):** the unit of execution is the **thread**; there is no separate executor entity. The old fixed pool becomes a **concurrency cap** `K = 2` (at most K threads active at once). The **focus lease is deleted** (§4). "Dispatch" becomes **promotion** — filling free active slots from the Waiting queue (§7.3). The word for the old entity does not appear in this doc except the top disclaimer.
- Thread = own conversation/context + own view state + own execution state; **same model AND same agent behaviour** fleet-wide.
- Partition (§3) grounded in the real `Module::is_global` inventory (24 modules + core); **three buckets** (shared state / per-thread context / fleet-level). Key finding: `conversation` + `conversation_history` **FLIP** global→per-thread (the mechanical heart of the change); several modules follow "shared substrate / per-thread view" (tree/files/search/git/github/console).
- Concurrency = **cooperative, single-thread-of-control, yield-based, lock-free**; invariant "no loop step blocks" accepted; **step** precisely defined (§5.2).
- No lease — a thread never competes for itself; human view-focus is a pure UI concern and does not gate execution (§4).
- Shared-state races = **dissolved** by the single-thread-of-control model (no locks).
- Promotion = **fill free active slots** (`|active| < K`) from the Waiting queue, oldest-waiting first; nothing to arbitrate.
- Loop advancement = **advance every active Runnable thread one step per tick**; this is the loop, **not the spine**, and there is **no "scheduler" entity** — only the `K` cap on top.
- Notifications = **thread-addressed** (ratified): a notification exists only to resume a specific thread whose conversation has pending work; a `tool_result` returns to its **owning thread's conversation**. Watchers **carry owner**; routing is a **TOTAL function → thread** (never "nobody"). "A MY_TURN thread needs a slot" and global no-target events are **state read by the promoter/loop, not notifications**.
- Tree = **descriptions shared, folder open/closed per-thread** (canonical "shared data / per-thread view").
- Failure = go `Errored`, free the slot, **surface loudly in the TUI**; re-engagement = same as single-thread today (nothing to transfer).
- UI = **TUI only** for now (web later). Drop `Ctrl+V`; **Left/Right** switch views, **Up/Down** select; **general view** = the thread list (states + attention markers) drilling into thread views; action: create thread (textarea). Navigation is inspection, not execution control.
- Lifecycle = **cap** `K = 2` now; `cp-fleet` crate owns register/retire/list + the active set.
- Budget = **independent per thread**.
- Reverie = **same execution primitive**, subordinate role; it does **not** count against `K` and is not itself a thread — **RATIFIED**.
- Split of concern: `cp-fleet` **= mechanism + the promoter (slot-fill policy)**, **spine = notification engine**, **loop = advancement** — RATIFIED. **No lease map.**
- Hardening (§13): the lease/dispatch items (**H3, H6, H7, H11**) are **dissolved** by the thread-centric model; the rest keep their resolutions (streaming = per-thread async collector; cache build duplicated verbatim, never modified; callbacks per-thread with `concurrency_friendly` informational notes; structural teardown by container; guardrails per-thread, no fleet ceiling; removal of three guardrails deferred to implementation; per-thread detail view **pixel-identical** to today's panel-view).

## Open Questions Log

- *(none architectural — the thread-centric model is ratified. Minor implementation-phase items are in the §13 backlog, including the* `AwaitingTool`*-holds-slot rule.)*