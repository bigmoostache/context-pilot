# Design — Thread-Owned Tasks (todos in threads)

> Status: **DRAFT / iterating**. Functional requirements only — **no NFR**(performance, scaling, persistence-format, multi-worker semantics are explicitly out of scope for this pass and will be added later).

## 1. Intent

Today's todos are one flat/nested **global list per worker**, rendered in a dedicated Todo panel. This rework **binds every todo item to a thread**: a thread owns its own checklist, and the WIP/Todo panel is **retargeted to show only the currently focused thread's items** (embedding tasks in the threads panel proved too costly). Task management becomes part of "working a thread" rather than a separate global backlog.

The existing `cp-mod-todo` crate is **adapted, not dropped**.

## 2. Functional requirements (the mandate)

- **FR1 — Adapt, don't drop.** `cp-mod-todo` survives and evolves. Its item type, nesting model, and status invariants are reused.
- **FR2 — Compulsory** `thread_id`**.** Every todo item gains a new **required**`thread_id` field. An item with no owning thread cannot exist.
- **FR3 — Todo panel kept, but focus-scoped.** The dedicated WIP/Todo panel **survives** (rendering it inside the threads panel proved too costly). It now displays **only the items of the currently focused thread** — its data source changes from "the global list" to "items where `thread_id` = the focused thread". Tasks are **not** rendered into the threads panel.
- **FR4 — No backwards compatibility (legacy thread-less items purged forever).** Todo state **is** serialized and persisted normally — thread-owned items survive across runs like any other state. There is **no migration of the old schema**: because `thread_id` is now compulsory (FR2), any pre-existing todo item that has **no** `thread_id` is **dropped from memory and removed from disk, permanently**, on load — a one-time-forever purge of the legacy thread-less backlog. New (thread-owned) items persist as usual. (Deliberate owner decision.)
- **FR5 —** `todo_move` **is removed.** There is no manual reordering tool.
- **FR6 — Structural edits move into the** `Think` **tool.** `todo_create`, `todo_update`, and `todo_move` are merged into a single `todo` **subtool: a new parameter on the** `Think` **tool** (see §5).
- **FR7 — New** `todo_mark` **tool, tempo-preserving *and* panel-cheap.** Marking item status is a separate tool that **preserves tempo** AND **does NOT mark the Todo panel deprecated** — a status flip is too frequent to pay a panel rebuild each time. It accepts a **batch** of `{id, status}` marks in one call, restricted to items of the **currently focused thread**. The new status simply appears on the panel's next natural rebuild (§6).
- **FR8 — Structural** `Think.todo` **edits deprecate the panel, tempo-preserved**.When `Think`'s `todo` subtool actually *changes* todos, it **marks the Todo panel deprecated** so the change surfaces — but still **preserves tempo**, so the re-emit is deferred to **tempo exhaustion**, not forced immediately (§5).
- **FR9 — Todo panel** `max_freeze = 5`**.** The Todo panel's freeze budget is raised to **5** (from 3), bounding how long a tempo-preserved refresh (FR8) can be deferred before it is forced fresh. *(Re-targeted from the threads panel to the Todo panel now that tasks render there again — FR3.)*
- **FR10 — Four task states.** `planned` ○ · `in progress` ◐ · `done` ● · `cancelled` ✕. `planned` replaces the old `pending`. `cancelled` **is the soft-delete** (FR13) — an item is removed *by being marked cancelled*, there is no hard-delete op.
- **FR11 — Work-hygiene nudges.** After a tool call made while a thread is focused, if that thread has **(a) no todo items** *or* **(b) actionable work (a** `planned` **item) but nothing** `in progress` **(**`◐`**)**, emit a **short, non-blocking** warning nudging the AI to `Think` and structure its work (§7). A thread whose items are all `done`/`cancelled` does **not** nudge. The nudge **fires at most once per focused thread** until the condition clears (a todo is created / a WIP item is picked) or focus moves to another thread — so it never repeats on every tool call (§7).
- **FR12 — No circular imports.** With tasks rendered in the (focus-scoped) Todo panel rather than the threads panel, `cp-mod-threads` **no longer needs to depend on** `cp-mod-todo`, and `cp-mod-todo` stays free of `cp-mod-threads`(the focused-thread filter is injected as data, not imported). No new module edge in either direction (§8).
- **FR13 — Deletion path = hard-delete cascade + cancel.** Todo items are deleted in exactly two ways: **(a)** when a thread is **hard-deleted** (`apply_delete_thread` / `ThreadDeleted`), all of its tasks are hard-removed (cascade). **Archiving** a thread does **not** touch its tasks — they stay and return when the thread is resurrected (archive is a soft-delete). **(b)** Marking an item `cancelled` is the user-facing per-item delete. Cancelled items are **hidden from the panel** and **excluded from every count** and from `check_done_allowed` (a parent may be `done` once its remaining non-cancelled children are done). There is no other delete operation.
- **FR14 —** `TodoState` **is global.** `TodoState` becomes a **shared** module (`is_global() == true`), matching `cp-mod-threads`: a thread is shared, so its tasks are shared. (Was per-worker.)

## 3. Data model

`TodoItem` (in `cp-mod-todo`) gains one compulsory field:

```
TodoItem {
  id:          String,          // "X{n}" — unchanged allocation
  thread_id:   String,          // NEW, compulsory — owning thread (FR2)
  parent_id:   Option<String>,  // nesting; parent MUST share the same thread_id
  name:        String,
  description: String,
  status:      TodoStatus,      // Planned ○ | InProgress ◐ | Done ●
}
```

`TodoStatus` **— four states (FR10):**

| Variant | wire value | icon | bracket | meaning |
| --- | --- | --- | --- | --- |
| `Planned` | `"planned"` | ○ | `[ ]` | not started (replaces `Pending`) |
| `InProgress` | `"in_progress"` | ◐ | `[~]` | the current WIP item |
| `Done` | `"done"` | ● | `[x]` | completed |
| `Cancelled` | `"cancelled"` | ✕ | `[/]` | soft-deleted (FR13) — hidden from panel, excluded from all counts |

- `TodoState { todos: Vec<TodoItem>, next_todo_id }` is **shared** (`is_global() == true`, FR14) and **remains the single owner** of all items across all threads. Threads reference their items by the `thread_id` foreign key; the `Thread` struct is **not** modified.
- **Nesting is thread-local:** an item's `parent_id`, if set, must point at an item with the same `thread_id`. Cross-thread parenting is rejected.
- **Ordering:** with `todo_move` gone (FR5), order within a `(thread_id, parent_id)` group is **insertion order** (the `Vec` order as produced by upserts). No manual reorder.
- **Preserved invariants** (unchanged from today, now scoped per thread-subtree):
  - a parent cannot be `Done` while a child is not `Done` (`check_done_allowed`);
  - marking a child `InProgress` bubbles `Planned` ancestors to `InProgress`(`propagate_in_progress`);
  - deleting a parent that would orphan children is rejected;
  - reparent is validated (no self-parent, no unknown parent, **same thread**).

## 4. Rendering — the focus-scoped Todo panel (FR3)

The WIP/Todo panel is **kept** (embedding tasks in the threads panel proved too costly). It is retargeted: instead of the whole global list, it renders **only the currently focused thread's items**, filtered by `thread_id`. The threads panel carries **no** tasks.

```
# Todo panel — focused thread only (thread_id == FocusState.focused_thread_id)
todos:
  - [x] X11 Read the modules
  - [~] X13 Draft design
      - [ ] X14 sub-item
```

- **Data source:** `TodoState.todos.filter(t.thread_id == focused_thread_id)`, rendered with the **existing recursive tree renderer** (unchanged).
- **No focused thread:** the panel shows an empty / "no focused thread" state.
- **Focus change is a refresh trigger:** when the focused thread changes, the Todo panel's content is now stale (it points at the previous thread's items), so a focus change **deprecates the Todo panel and forces it fresh immediately** (**breaks tempo** — the panel must show the newly-focused thread's items at once).
- **Threads panel:** stays **strictly task-free** — **no** per-thread task summary, **no** `N/M done` counter, **no** embedded tree, **nothing** todo-related whatsoever. This is a hard requirement, not a stylistic preference: any todo datum in the threads panel would couple todo edits to the (expensive) threads-panel rebuild, so **editing a todo must never touch or deprecate the threads panel** (otherwise every todo edit breaks the threads-panel cache — prohibitively expensive). The threads panel and todo state are fully decoupled.

### 4.1 Panel refresh model (the load-bearing asymmetry)

The Todo panel is **static** — its content is a stored string, rebuilt only on demand. Todo statuses render *into* that string, so a status change is only visible once the string is rebuilt. The cost of rebuilding is paid **deliberately unevenly** by action:

| Action | Marks Todo panel deprecated? | Tempo | When the change becomes visible |
| --- | --- | --- | --- |
| `todo_mark` (status flip) | **No** (too costly) | preserved | on the next natural rebuild (focus change / `Think.todo`) |
| `Think.todo` (structural) | **Yes** | preserved | at **tempo exhaustion** — forced fresh within `max_freeze = 5` freeze ticks |
| focus change | **Yes** (panel now stale) | **breaks** | **immediately** — the panel now points at the previous thread's items, so it is forced fresh at once |

- `todo_mark` **never touches the panel** (FR7). It flips status in `TodoState`and returns; the flip surfaces whenever something else rebuilds the panel. This is the accepted trade — frequent marks must stay free.
- `Think.todo` **marks the panel deprecated but preserves tempo** (FR8): it does **not** break tempo, so the fresh content is deferred to tempo exhaustion rather than forced. The Todo panel's `max_freeze` is **5** (FR9), so the deferral is bounded to at most 5 freeze ticks.
- The threads panel is **no longer a task surface**, so `Read` / `Send` on it are irrelevant to task rendering.

## 5. Structural edits via `Think.todo` (FR6)

The `Think` tool gains a new optional `todo` parameter: a **recursive upsert** of a task tree for the **currently focused thread**.

### Node schema

```
todo: [ TodoNode, … ]          // an ordered forest for the FOCUSED thread

TodoNode {
  id?:          string,        // ABSENT → create · PRESENT → update
  name?:        string,        // REQUIRED on create · optional on update (partial)
  description?: string,        // default "" on create
  status?:      "planned" | "in_progress" | "done" | "cancelled",   // default "planned" on create
  parent_id?:   string,        // reparent an EXISTING item to an already-existing id (rare)
  children?:    [ TodoNode, … ]// nodes that live UNDER this node
}
```

`Think` keeps its `thought_body`; `todo` is an additional optional param — one call can carry both a thought and a task-tree mutation.

### Create vs. update (per node)

- `id` **absent → CREATE.** `name` is required. A fresh `X{n}` id is allocated; `thread_id` = the focused thread; `parent_id` = the enclosing node (or root if top-level). The AI **never needs to know ids in advance** — a whole hierarchy is scaffolded in one call.
- `id` **present → UPDATE.** A **partial patch**: only the provided fields change. The id must exist **and belong to the focused thread**, else that node is rejected.
- **Upsert is a PATCH, not a replace.** Items you don't mention are left untouched — **omission is never a delete** (deletion is only cascade-on-thread-delete or marking `cancelled`, FR13).

### Parentage (the linkage rule)

- `children` **always means "descendants of me."** A node placed in another node's `children` list has that node as its parent.
- **CREATE** takes its parent from where it sits: nested → that enclosing node; top-level → a **root** of the thread.
- **UPDATE at top-level → parent unchanged.** Listing an existing item at the top level (just to edit its fields) does **not** yank it to root.
- **UPDATE nested inside a** `children` **list → reparented** to that enclosing node (you deliberately nested it, so you mean it).
- **Reparent to a brand-new parent** created in the same call: nest the existing item inside that new parent's `children` — no id needed. `parent_id` is reserved for reparenting to an **already-existing** id.

### Processing order: outer-to-inner (pre-order)

The tree is applied **parent-before-children (pre-order DFS), always outer-to-inner.** A node is created/updated and its id resolved **before** its `children` are processed, so:

- a newly-created parent already exists (has an id) by the time its children attach to it — you can create a parent and reparent an existing item under it **in the same call**;
- an explicit `parent_id` must reference an item that **already exists** — either pre-call, or one created **earlier** in this call's outer-to-inner traversal. The reliable way to attach under a *new* parent is **nesting**, never a forward `parent_id` reference.

This is why you always structure the call from the outside in: you can never move something under an item that hasn't been created yet.

### Validation (reuses today's invariants)

- `name` non-empty on create; `status` ∈ the four values (aliases /`~`/`x` still accepted for ergonomics).
- Reparent checks: same thread (always true here), **no self-parent**, **no cycle** (a node can't become its own ancestor), parent must exist per the ordering rule above.
- `check_done_allowed`: can't set `done` while a **non-cancelled** child isn't done; `propagate_in_progress`: setting a child `in_progress` bubbles `planned` ancestors to `in_progress`.
- **No focused thread → the whole** `todo` **param is rejected** with a clear error.

### Failure mode & result

- **Best-effort, per node** (mirrors today's `todo_update` tally): valid nodes apply; a failed node is reported and **its subtree is skipped** (a child can't attach to a parent that failed to create).
- **Result is concise:** `created: [X31, X32]`, `updated: [X12]`, plus any `errors: […]`. The focus-scoped Todo panel shows the resulting tree.

### Worked examples

```
// Scaffold a new subtree (all creates)
todo: [{ name: "Auth epic", children: [
          { name: "Design tokens" },
          { name: "Session store", status: "in_progress" } ] }]

// Rename one item, no structural change (parent untouched)
todo: [{ id: "X12", name: "Session store (Redis)" }]

// Attach a new child under an existing item
todo: [{ id: "X12", children: [{ name: "Token rotation" }] }]

// Move existing X20 under the newly-created "Auth epic" (outer-to-inner: parent first)
todo: [{ name: "Auth epic", children: [{ id: "X20" }] }]
```

- **Removed tools:** `todo_create`, `todo_update`, `todo_move` cease to exist as standalone tools; their create/update capability is fully covered here, and reorder (FR5) is dropped.
- **Panel refresh (FR8):** whenever this subtool actually *changes* todos, it **marks the Todo panel deprecated** so the structural change surfaces, but **preserves tempo** — the fresh content re-emits at tempo exhaustion (bounded by `max_freeze = 5`), never forced immediately. See §4.1.

> **Deletion (FR13):** there is **no** `delete` flag. To remove an item, set its `status` to `"cancelled"` (here or via `todo_mark`) — the soft-delete. Hard removal happens only when the owning **thread** is deleted (cascade).

## 6. Status marking via `todo_mark` (FR7)

A dedicated tool marks item status, kept **out** of `Think` because it is frequent and low-information:

```
todo_mark({ marks: [ { id: string, status: "planned" | "in_progress" | "done" | "cancelled" }, … ] })
```

- **Preserves tempo** — a status flip never triggers a full context refresh.
- **Does NOT mark the Todo panel deprecated** (FR7) — this is the whole point of a separate tool: marking is frequent, and paying a panel rebuild per flip is too costly. The new status becomes visible on the panel's **next natural rebuild** (focus change, or a `Think.todo` structural edit). See §4.1.
- Enforces the same invariants (`check_done_allowed`, `propagate_in_progress`); `cancelled` items are excluded from `check_done_allowed` (a parent may go `done` once its non-cancelled children are done).
- **Batch:** accepts **many** `{id, status}` marks in one call (FR7) — flip several items, to possibly-different statuses, at once.
- **Focused-thread only:** every id must belong to the currently focused thread; an id from another thread is rejected (mirrors the `Think.todo` update restriction).
- `cancelled` **is the per-item delete** (FR13): marking `cancelled` soft-deletes — the item leaves the panel and all counts.

## 7. Work-hygiene nudges (FR11)

A **non-blocking**, **short** warning is surfaced to the AI when it acts while a thread is focused but its task hygiene is lacking:

- **Trigger:** after a tool call completes **with a focused thread**, evaluate that thread's items:

  - **(a)** the focused thread has **no** todo items, **or**
  - **(b)** the focused thread has items but **none is** `InProgress` **(**`~`**)**.

- **Action:** emit a brief, non-lengthy nudge into the AI's context, prompting it to `Think` and structure its work (create a checklist / pick a WIP item).

- **Non-blocking:** it never blocks the tool call or the turn — it is guidance, benefiting both the AI (structure) and the user (visible plan).

- **Fire-once, not every tool call.** The nudge must **not** re-fire on every qualifying tool call (that would be spam). Rule: once the focused thread has been nudged, it is **not nudged again for that thread** until either the condition clears (a todo is created / a WIP `◐` item is picked) **or** focus moves to another thread. Tracked with a small per-thread "already nudged" flag that resets when the condition clears or on focus change.

- **Wording (short, plain):** e.g. *"This thread has no tasks yet — use* `Think` *to sketch a checklist before you dig in."* (case a) / *"You have planned tasks but none in progress — mark one* `~` *so your plan is visible."* (case b). Non-technical, one line.

## 8. Dependency direction & cycle analysis (FR12)

Because tasks now render in the **Todo panel** (owned by `cp-mod-todo`) rather than in the threads panel, the edge the original design needed — `cp-mod-threads → cp-mod-todo` — is **no longer required**. The concern reduces to: the Todo panel must filter by the *focused* thread, and `focused_thread_id` lives in `cp-mod-threads::FocusState`.

**Verified current state:**

- `cp-mod-todo` depends on `cp-base`, `cp-render`, crossterm, serde — **no reference to** `cp-mod-threads`.
- `cp-mod-threads` depends on the same set — **no reference to** `cp-mod-todo`.
- Neither imports the other today.

**Rule to keep it that way (dependency injection):**

- `cp-mod-todo` **never** imports `cp-mod-threads`. The focused-thread filter is **pushed in as data**: the main crate stamps the current focused thread id onto `TodoState` (e.g. a `focus_filter: Option<String>` field, or a `set_focus_filter(state, thread_id)` call) whenever focus changes. The Todo panel renders using its own stored filter — it does **not** read `FocusState`.
- All task operations remain **pure functions that take** `thread_id` **as a parameter** (`upsert_tasks(state, thread_id, tree)`, `mark_task(state, id, status)`).
- The `Think.todo` **subtool,** `todo_mark`**, and the nudge check** are hosted in the main crate (`src/`, which already depends on *all* modules). The caller there resolves the focused thread from `ThreadsState`/`FocusState` and passes `thread_id` **down** into the pure todo functions (and updates the panel's focus filter).

Result: **no new module edge in either direction.** The dependency graph is strictly simpler than the original plan — focus flows in through function arguments / injected state from the binary, not through a module import.

## 9. Resolved decisions (were open questions)

All six earlier open questions are now decided by the owner:

1. `is_global` **of** `TodoState` **→ global.** `TodoState` becomes **shared** (`is_global() == true`), matching `cp-mod-threads` (FR14 / §3). A shared thread's tasks are shared.
2. **Deletion path → cascade + cancel (FR13).** Hard removal only on **thread deletion** (cascade); per-item removal is **marking the item** `cancelled` (soft-delete). No separate delete op / flag.
3. `todo_mark` **batching & scope → batch, focused-thread only (FR7 / §6).** Many `{id, status}` marks per call; every id must belong to the focused thread.
4. **Nudge repetition → fire-once per thread (FR11 / §7).** Not "debounce" in any timer sense — it simply fires **at most once per focused thread** until the condition clears (todo created / WIP picked) or focus changes. Plain wording specified in §7.
5. **Focus-change refresh → breaks tempo (forced, immediate) (§4 / §4.1).** And, critically: **no todo data of any kind lives in the threads panel** — todo edits must never touch/deprecate the threads panel (else its cache breaks on every edit, prohibitively expensive). No `N/M` per-row counter.
6. **Backwards-compat → forever purge of legacy items (FR4).** Normal persistence is kept — thread-owned todos survive across runs. The *only* removal is a **permanent purge of legacy items with no** `thread_id` (the old-schema backlog), dropped from memory **and disk** on load. This is **not** a per-boot wipe of the store.
7. **Agent-level / un-threaded tasks → none.** All task-tracking **must happen inside a thread**. There is no unfocused/agent-level task list: when no thread is focused, `Think.todo` is rejected (§5). The discipline is "plan inside a thread."
8. **Cross-thread task move → disallowed.** A task is **born and dies in one thread** — reparent is same-thread only (§5), and there is no move-to-thread operation.
9. **Archive vs. hard-delete → archive keeps tasks (FR13).** Archiving a thread (soft-delete) leaves its tasks intact; they return when the thread is resurrected. Only a **hard-delete** of the thread cascades a removal of its tasks.
10. **Todo auto-continuation → removed entirely (§11).** Rather than rescope `continue_until_todos_done` to threads, the whole todo-driven spine auto-continuation is **deleted from the codebase** (a clean wipe, no migration). It may be redesigned from scratch later if wanted.

## 10. Surface touched (informational, for later planning)

- `cp-mod-todo`: `TodoItem.thread_id`, `is_global() == true` (shared), **four states inc.** `Cancelled` (soft-delete), pure `upsert_tasks`/`mark_task` (batch), **cascade-delete on thread deletion**, drop `todo_move`, drop standalone create/update tools, **keep the Todo panel but filter it to the focused thread** (injected `focus_filter`, cancelled items hidden), `max_freeze = 5`, thread-scoped `has_incomplete_todos`/`incomplete_todos_summary`.
- `cp-mod-threads`: **unchanged** — no new dependency, threads panel stays strictly task-free.
- `cp-mod-spine` + `src/` (main crate): `Think` gains the recursive `todo` param; new batch `todo_mark`tool; fire-once hygiene-nudge check in the tool pipeline; stamps the focused thread id onto `TodoState` on focus change (forces panel refresh); **DELETE the todo auto-continuation entirely** — `continue_until_todos_done` (config flag, tool param, keybind action, panel/overlay display) + `check_todo_continuation` + the now-dead `has_incomplete_todos`/`incomplete_todos_summary` helpers (§11); `overview_context_section` stays a cheap global rollup (`Tasks: N/M done`); **on hard-delete** of a thread, cascade-remove that thread's tasks (archive leaves them); on load, **purge (forever) any todo item lacking a** `thread_id` from memory and disk, persisting normally otherwise.
- Frontend/orchestrator (M141, later): thread projection gains todos; web threads view renders per-thread checklist; remove the standalone todo surface.

## 11. Decision: remove todo auto-continuation entirely

The earlier plan was to *rework* the todo-fired spine coupling to be thread-scoped. **Decision (owner): don't rework it — delete it.** For now the whole todo-driven spine auto-continuation is wiped from the codebase; if a thread-aware version is wanted later it will be **redesigned from scratch**, which is cleaner than migrating today's global-list logic into the new thread model.

### 11.1 What is removed (deletion checklist)

The todo↔spine coupling lives in three sites; **sites 1 and 2 are deleted outright**, site 3 (a plain display line, not auto-continuation) is kept as a rollup:

1. **`continue_until_todos_done` config flag** — delete everywhere:
   - the field on `SpineConfig` (`cp-mod-spine/src/types.rs`);
   - the `spine_configure` tool param + handler (`cp-mod-spine/src/lib.rs`, `tools.rs`);
   - the toggle **keybind action** (`src/app/actions/mod.rs`);
   - the config-overlay + spine-panel **display** of the flag (`src/ui/help/config_overlay/builder.rs`, `cp-mod-spine/src/panel.rs`).
2. **`check_todo_continuation`** (`src/app/run/lifecycle.rs`) — delete the function **and its call site** in `check_spine`. This removes the `todo_continuation` spine notification path entirely.
3. **`overview_context_section`** (`cp-mod-todo/src/lib.rs`) — **kept** (it is a display line, not auto-continuation). Reframed as a cheap global rollup: `Tasks: N/M done` — a plain count, no per-thread breakdown, no coupling to any panel.

Because sites 1–2 are their only callers, **`has_incomplete_todos()` and `incomplete_todos_summary()`** (`cp-mod-todo/src/types.rs`) become dead and are **deleted too**.

### 11.2 Consequences

- The spine no longer auto-continues based on todo state at all. Auto-continuation still works for its other triggers (unchanged); it simply no longer chases unfinished todos.
- No behaviour needs to be thread-scoped here — there is nothing left to scope.
- This is a self-contained, backward-behaviour-only change: it can ship as an isolated commit/PR independently of the rest of the thread-tasks rework.
