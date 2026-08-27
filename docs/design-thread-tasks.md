# Design — Thread-Owned Tasks (todos in threads)

> Status: **DRAFT / iterating**. Functional requirements only — **no NFR**
> (performance, scaling, persistence-format, multi-worker semantics are
> explicitly out of scope for this pass and will be added later).

## 1. Intent

Today's todos are one flat/nested **global list per worker**, rendered in a
dedicated Todo panel. This rework **binds every todo item to a thread**: a
thread owns its own checklist, the standalone Todo panel disappears, and items
are rendered inside the threads surface. Task management becomes part of "working
a thread" rather than a separate global backlog.

The existing `cp-mod-todo` crate is **adapted, not dropped**.

## 2. Functional requirements (the mandate)

- **FR1 — Adapt, don't drop.** `cp-mod-todo` survives and evolves. Its item
  type, nesting model, and status invariants are reused.
- **FR2 — Compulsory `thread_id`.** Every todo item gains a new **required**
  `thread_id` field. An item with no owning thread cannot exist.
- **FR3 — No Todo panel.** The dedicated Todo panel is removed. Items are
  **displayed inside the threads panel**, grouped under their owning thread.
- **FR4 — No backwards compatibility.** No migration. On upgrade, **all
  pre-existing todo items are wiped entirely.** (Deliberate owner decision.)
- **FR5 — `todo_move` is removed.** There is no manual reordering tool.
- **FR6 — Structural edits move into the `Think` tool.** `todo_create`,
  `todo_update`, and `todo_move` are merged into a single **`todo` subtool: a
  new parameter on the `Think` tool** (see §5).
- **FR7 — New `todo_mark` tool, tempo-preserving *and* panel-cheap.** Marking an
  item's status is a separate tool that **preserves tempo** AND **does NOT mark
  the threads panel deprecated** — a status flip is too frequent to pay a panel
  rebuild each time. The new status simply appears on the panel's next natural
  rebuild (§6).
- **FR8 — Structural `Think.todo` edits deprecate the panel, tempo-preserved.**
  When `Think`'s `todo` subtool actually *changes* todos, it **marks the threads
  panel deprecated** so the change surfaces — but still **preserves tempo**, so
  the re-emit is deferred to **tempo exhaustion**, not forced immediately (§5).
- **FR9 — Threads panel `max_freeze = 5`.** The threads panel's freeze budget is
  raised to **5** (from 3), bounding how long a tempo-preserved refresh (FR8) can
  be deferred before it is forced fresh.
- **FR10 — Three task states.** `planned` ○ · `in progress` ◐ · `done` ●.
  `planned` replaces the old `pending`. (No `cancelled`.)
- **FR11 — Work-hygiene nudges.** After a tool call made while a thread is
  focused, if that thread has **(a) no todo items** *or* **(b) actionable work
  (a `planned` item) but nothing `in progress` (`◐`)**, emit a **short,
  non-blocking** warning nudging the AI to `Think` and structure its work (§7).
  A thread whose items are all `done` does **not** nudge.
- **FR12 — No circular imports.** `cp-mod-threads` will depend on `cp-mod-todo`
  (its panel renders items). This must not create a dependency cycle (§8).

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

**`TodoStatus` — three states (FR10):**

| Variant      | wire value      | icon | bracket | meaning                          |
|--------------|-----------------|------|---------|----------------------------------|
| `Planned`    | `"planned"`     | ○    | `[ ]`   | not started (replaces `Pending`) |
| `InProgress` | `"in_progress"` | ◐    | `[~]`   | the current WIP item             |
| `Done`       | `"done"`        | ●    | `[x]`   | completed                        |


- `TodoState { todos: Vec<TodoItem>, next_todo_id }` **remains the single owner**
  of all items across all threads. Threads reference their items by the
  `thread_id` foreign key; the `Thread` struct is **not** modified.
- **Nesting is thread-local:** an item's `parent_id`, if set, must point at an
  item with the same `thread_id`. Cross-thread parenting is rejected.
- **Ordering:** with `todo_move` gone (FR5), order within a
  `(thread_id, parent_id)` group is **insertion order** (the `Vec` order as
  produced by upserts). No manual reorder.
- **Preserved invariants** (unchanged from today, now scoped per thread-subtree):
  - a parent cannot be `Done` while a child is not `Done` (`check_done_allowed`);
  - marking a child `InProgress` bubbles `Planned` ancestors to `InProgress`
    (`propagate_in_progress`);
  - deleting a parent that would orphan children is rejected;
  - reparent is validated (no self-parent, no unknown parent, **same thread**).

## 4. Rendering — items inside the threads panel (FR3)

The Todo panel is removed. `cp-mod-threads`' `build_panel_content` renders todo
items, filtered by `thread_id`:

```
threads:
  - id: T648
    name: Send
    status: THEIR_TURN
    todos: 3/5 done              # compact per-thread summary in the LIST
  - id: T649
    name: TODOs
    status: MY_TURN
    todos: 0/2 done
conversation:
  thread_id: T649
  todos:                          # full tree, focused thread only
    - [x] X11 Read the modules
    - [~] X13 Draft design
        - [ ] X14 sub-item
  messages: [ … ]
```

- **List view:** each thread row shows a one-line `N/M done` summary.
- **Focused view:** the focused thread's section shows the full nested tree
  (reusing the existing recursive renderer, filtered to that thread's items).

### 4.1 Panel refresh model (the load-bearing asymmetry)

The threads panel is **static** — `panel_content` is a stored string, rebuilt
only on demand. Todo statuses render *into* that string, so a status change is
only visible once the string is rebuilt. The cost of rebuilding is paid
**deliberately unevenly** by action:

| Action                    | Marks panel deprecated? | Tempo      | When the change becomes visible                         |
|---------------------------|-------------------------|------------|---------------------------------------------------------|
| `todo_mark` (status flip) | **No** (too costly)     | preserved  | on the next natural rebuild (`Read` / `Send` / `Think.todo`) |
| `Think.todo` (structural) | **Yes**                 | preserved  | at **tempo exhaustion** — forced fresh within `max_freeze = 5` freeze ticks |
| `Read`                    | Yes (rebuild now)       | breaks     | immediately                                             |
| `Send`                    | Yes (rebuild now)       | preserved  | next fresh tick (T648 mechanism)                        |

- **`todo_mark` never touches the panel** (FR7). It flips status in `TodoState`
  and returns; the flip surfaces whenever something else rebuilds the panel.
  This is the accepted trade — frequent marks must stay free.
- **`Think.todo` marks the panel deprecated but preserves tempo** (FR8): it does
  **not** break tempo, so the fresh content is deferred to tempo exhaustion
  rather than forced. The threads panel's `max_freeze` is **5** (FR9), so the
  deferral is bounded to at most 5 freeze ticks.
- Both deprecation paths reuse the existing `rebuild_threads_panel(state, tid,
  now)` helper (the mechanism shipped for `Send`); `todo_mark` deliberately does
  **not** call it.

## 5. Structural edits via `Think.todo` (FR6)

The `Think` tool gains a new optional **`todo`** parameter: a **recursive
upsert** of a task tree for the **currently focused thread**.

### Recursive shape

```
todo: [
  {
    id?:          string,       // present → UPDATE; absent → CREATE
    name:         string,
    description?: string,
    status?:      "planned" | "in_progress" | "done",
    children?:    [ { … same node shape … }, … ]
  },
  …
]
```

- **Scaffold without ids.** The AI passes a nested tree; the system creates the
  whole hierarchy in one call, allocating ids and wiring `parent_id` from the
  nesting. The AI **never needs to know child/parent ids first** (satisfies the
  "recursive data structure" requirement).
- **Upsert semantics per node:**
  - **id absent** → CREATE a new item (its `thread_id` = the focused thread; its
    `parent_id` = the enclosing node's id).
  - **id present** → UPDATE that item. **Only items of the currently focused
    thread are accepted**; an id belonging to another thread is rejected.
- **Thread scope = the focused thread.** No `thread_id` parameter on the subtool;
  the focused thread (`FocusState.focused_thread_id`) is resolved by the caller
  (§8). No focused thread → the `todo` parameter is rejected with a clear error.
- **Removed tools:** `todo_create`, `todo_update`, `todo_move` cease to exist as
  standalone tools; their create/update capability is fully covered here, and
  reorder (FR5) is dropped.
- **Panel refresh (FR8):** whenever this subtool actually *changes* todos, it
  **marks the threads panel deprecated** so the structural change surfaces, but
  **preserves tempo** — the fresh content re-emits at tempo exhaustion (bounded
  by `max_freeze = 5`), never forced immediately. See §4.1.

> Open question (§9): does `Think.todo` also handle **deletion** (e.g. a
> `delete?: true` flag on a node), or is delete out of scope for the subtool?

## 6. Status marking via `todo_mark` (FR7)

A dedicated tool marks item status, kept **out** of `Think` because it is
frequent and low-information:

```
todo_mark({ id: string, status: "planned" | "in_progress" | "done" })
```

- **Preserves tempo** — a status flip never triggers a full context refresh.
- **Does NOT mark the threads panel deprecated** (FR7) — this is the whole point
  of a separate tool: marking is frequent, and paying a panel rebuild per flip
  is too costly. The new status becomes visible on the panel's **next natural
  rebuild** (`Read`, `Send`, or a `Think.todo` structural edit). See §4.1.
- Enforces the same invariants (`check_done_allowed`, `propagate_in_progress`).
- Scope: acts on an item by id; the item's own `thread_id` locates it.

> Open question (§9): should `todo_mark` accept a **batch** of ids, or one at a
> time? Should it be restricted to the focused thread like updates are?

## 7. Work-hygiene nudges (FR8)

A **non-blocking**, **short** warning is surfaced to the AI when it acts while a
thread is focused but its task hygiene is lacking:

- **Trigger:** after a tool call completes **with a focused thread**, evaluate
  that thread's items:
  - **(a)** the focused thread has **no** todo items, **or**
  - **(b)** the focused thread has items but **none is `InProgress` (`~`)**.
- **Action:** emit a brief, non-lengthy nudge into the AI's context, prompting it
  to `Think` and structure its work (create a checklist / pick a WIP item).
- **Non-blocking:** it never blocks the tool call or the turn — it is guidance,
  benefiting both the AI (structure) and the user (visible plan).

> Open questions (§9): debounce/rate-limit so the nudge isn't repeated every
> tool call? Suppress during dangling / when no thread is focused (only fires
> *with* a focused thread by definition)? Exact wording.

## 8. Dependency direction & cycle analysis (FR9)

**New edge:** `cp-mod-threads` → `cp-mod-todo` (the threads panel reads
`TodoState`, filters by `thread_id`, and renders items).

**Verified current state:**
- `cp-mod-todo` depends on `cp-base`, `cp-render`, crossterm, serde — **no
  reference to `cp-mod-threads`**.
- `cp-mod-threads` depends on the same set — **no reference to `cp-mod-todo`**.
- Neither imports the other today → the new one-way edge is a **DAG addition,
  no cycle.**

**The only cycle risk** is the focus dependency: updates/marks are scoped to
`FocusState.focused_thread_id`, and `FocusState` lives in `cp-mod-threads`. If
`cp-mod-todo` read it directly, we'd get `threads → todo → threads`.

**Rule to prevent it (dependency injection):**
- `cp-mod-todo` **never** imports `cp-mod-threads`. Its operations are **pure
  functions that take `thread_id` as a parameter** (e.g. `upsert_tasks(state,
  thread_id, tree)`, `mark_task(state, id, status)`).
- The **`Think.todo` subtool and `todo_mark` are hosted in the main crate**
  (`src/`, which already depends on *all* modules). The caller there resolves
  the focused thread from `ThreadsState`/`FocusState` and passes `thread_id`
  **down** into the pure todo functions.
- The **nudge check** (§7) also lives at the tool-pipeline level (main crate),
  reading both `ThreadsState` and `TodoState`.

Result: `cp-mod-threads → cp-mod-todo` is the **only** new edge; focus flows
in through function arguments from the binary, not through a back-import.

## 9. Open questions (to iterate)

1. **`is_global` of `TodoState`.** Today it is per-worker; threads are shared.
   For rendering in the shared threads panel, does `TodoState` become shared
   too? (Deferred — touches multi-worker semantics, arguably NFR-adjacent.)
2. **Deletion path.** Delete via a `Think.todo` node flag, or a separate op, or
   not at all in this pass?
3. **`todo_mark` batching & focus restriction.** One id or many? Focused-thread
   only?
4. **Nudge debounce + wording.** How often can it fire; exact short text.
5. **What renders in the LIST vs the FOCUSED view.** Summary-only per row +
   full tree for focused (proposed) — confirm.
6. **Wiping semantics (FR4).** Wipe on first boot of the new binary only, or
   every boot until the todo store is empty?

## 10. Surface touched (informational, for later planning)

- `cp-mod-todo`: `TodoItem.thread_id`, pure `upsert_tasks`/`mark_task`, drop
  `todo_move`, drop standalone create/update tools, remove the Todo panel.
- `cp-mod-threads`: depend on `cp-mod-todo`; `build_panel_content` renders
  per-thread todos; reuse `rebuild_threads_panel`.
- `src/` (main crate): `Think` gains the recursive `todo` param; new `todo_mark`
  tool; nudge check in the tool pipeline.
- Frontend/orchestrator (M141, later): thread projection gains todos; web
  threads view renders per-thread checklist; remove the standalone todo surface.
