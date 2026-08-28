# Implementation Plan — todo-v2 (thread-owned tasks)

> **Companion to** `docs/design-thread-tasks.md` (the functional spec). This doc is
> **implementation-focused**: exact files, symbols, and an ordered, build-verifiable
> phase list. Branch: **`todo-v2`** (off `origin/master`).
>
> **Scope of this pass = Rust backend (TUI) only.** The frontend/orchestrator surface
> (M141) is deferred per design §10. No web/openapi work here.

## 0. Ground truth (verified in code)

- **`cp-mod-todo`** owns all task state. `TodoState { todos: Vec<TodoItem>, next_todo_id }`,
  `is_global() == false` today. `TodoItem { id, parent_id, name, description, status }`.
  `TodoStatus { Pending, InProgress, Done }`.
- **`cp-mod-threads`** owns `FocusState.focused_thread_id: Option<String>` (per-worker) and
  `ThreadsState` (`is_global == true`). Neither todo nor threads imports the other today.
- **Module persistence routes automatically on `is_global()`** (`boot.rs:55`, `save.rs:32`).
  Flipping `TodoState` to global lands its data in the shared store; old per-worker todo
  data is simply orphaned on load — which *is* the FR4 legacy wipe, for free.
- **`Think`** lives in the main crate (`src/modules/questions/`), which depends on every
  module → it can read `FocusState` **and** mutate `TodoState`. This is where `Think.todo`
  and `todo_mark` are hosted (design §8 dependency-injection rule).
- **Auto-continuation coupling** (§11 removal) spans 12 sites — enumerated in Phase 6.

**Circular-import guarantee (FR12):** `cp-mod-todo` stays free of `cp-mod-threads`. All
task ops are **pure functions taking `thread_id` as a parameter**; the main crate resolves
the focused thread and passes it down. No new module edge in either direction.

---

## Phase 1 — `cp-mod-todo` data model (types.rs)

**File:** `crates/cp-mod-todo/src/types.rs`

1. `TodoStatus`: rename `Pending → Planned`; add `Cancelled`. Four variants:
   `Planned` (`"planned"`, ○, `[ ]`), `InProgress` (`"in_progress"`, ◐, `[~]`),
   `Done` (`"done"`, ●, `[x]`), `Cancelled` (`"cancelled"`, ✕, `[/]`).
   - `#[default]` moves to `Planned`.
   - `icon()`: add cancelled arm (needs an icon accessor — reuse an existing glyph or a
     literal `✕`; check `cp_base::config::accessors::icons` for a `todo_cancelled`, add if
     missing, else use a literal in the panel to avoid a config-schema change this pass).
   - `FromStr`: accept `"planned"` (+ keep `" "` alias → `Planned`), `"cancelled"` (+ `"/"`).
2. `TodoItem`: add **compulsory** `thread_id: String` (serialized, no skip). Keep `parent_id`.
3. `TodoState`:
   - add `focus_filter: Option<String>` — the injected focused-thread id for panel scoping.
     **Transient**: NOT serialized (see Phase 4 `save_module_data`), init `None`.
   - **DELETE** `has_incomplete_todos()` and `incomplete_todos_summary()` (only the removed
     spine site calls them — Phase 6).

**Build gate:** `cargo build -p cp-mod-todo` will fail until Phases 2–4 catch up — expected;
build the crate at the end of Phase 4.

---

## Phase 2 — pure task ops (cp-mod-todo, threads-free)

**File:** `crates/cp-mod-todo/src/tools.rs` (repurpose; keep the reusable helpers).

Keep and re-scope the existing pure invariants (they already operate on a `Vec` + `parent_id`):
`collect_descendants`, `check_done_allowed`, `propagate_in_progress`, parent validation.
Cancelled items are excluded from `check_done_allowed` and from counts.

Add pure entry points (all take `thread_id`, none touch `FocusState`):

- `pub fn upsert_task_forest(state, thread_id, nodes: &[TodoNode]) -> UpsertOutcome`
  - `TodoNode { id: Option<String>, name: Option<String>, description: Option<String>,
    status: Option<TodoStatus>, parent_id: Option<String>, children: Vec<TodoNode> }`
    (define in `types.rs` or a new `upsert.rs`; serde-deserializable from the tool JSON).
  - **Pre-order DFS (outer→inner)**: resolve/allocate a node's id before its children.
    - `id` absent → CREATE: allocate `X{n}`, `thread_id` = passed thread, `parent_id` =
      enclosing node's resolved id (or `None` at root). `name` required.
    - `id` present → UPDATE (partial patch). Must exist **and** match `thread_id`.
      Nested under a `children` list ⇒ reparent to that enclosing id; at top level ⇒
      parent unchanged.
    - `parent_id` explicit ⇒ reparent to an already-existing id (pre-call or earlier in
      traversal); reject forward refs / self-parent / cross-thread / cycles.
  - Best-effort per node: a failed node skips its subtree; return `{ created, updated, errors }`.
- `pub fn mark_tasks(state, thread_id, marks: &[(String, TodoStatus)]) -> MarkOutcome`
  - Every id must belong to `thread_id`; enforce `check_done_allowed` + `propagate_in_progress`;
    `cancelled` is the soft-delete.
- `pub fn purge_threadless(state)` — drop every `TodoItem` whose `thread_id` is empty
  (FR4 legacy purge). Called from `load_module_data` (Phase 4).
- `pub fn set_focus_filter(state, thread_id: Option<String>) -> bool` — set
  `TodoState.focus_filter`, return whether it changed (drives the panel force-refresh).

**REMOVE** the old tool executors `execute_create`, `execute_update`, `execute_move`
(their capability is now covered by `upsert_task_forest` + `mark_tasks`; reorder is dropped, FR5).

---

## Phase 3 — focus-scoped Todo panel (panel.rs)

**File:** `crates/cp-mod-todo/src/panel.rs`

- `format_todos_for_context` + `blocks()`: filter to
  `TodoState.focus_filter` (`todos.iter().filter(|t| Some(&t.thread_id) == focus_filter.as_ref())`).
  Roots = `parent_id is None` **within that thread**.
- **Hide `Cancelled`** items (and their subtrees) from the panel.
- Status icons: `Planned` ○ Muted / `InProgress` ◐ Warning / `Done` ● Success
  (cancelled never rendered).
- Empty states: no `focus_filter` → `"No focused thread"`; focused but no items → `"No tasks"`.
- `max_freezes()`: **0 → 5** (FR9).

---

## Phase 4 — module wiring & persistence (lib.rs)

**File:** `crates/cp-mod-todo/src/lib.rs`

1. `is_global()`: **`false → true`** (FR14). Persistence auto-routes to the shared store.
2. `tool_definitions()`: **remove** `todo_create` / `todo_update` / `todo_move`. **Add**
   `todo_mark` (batch `marks: [{id, status}]`). *(Think.todo is defined in the questions
   module, Phase 5 — not here.)*
3. `pre_flight`: drop create/update/move arms; add a `todo_mark` arm (ids exist + belong to
   the focused thread — but focus lives in the main crate, so keep pre_flight light here and
   do the authoritative focused-thread check in the executor; see Phase 5 note).
4. `execute_tool`: drop create/update/move. **`todo_mark` is hosted in the main crate**
   (needs `FocusState`), so it is NOT dispatched here — remove todo's executor arms entirely.
   *(Decision: host both `todo_mark` and `Think.todo` in the main crate; `cp-mod-todo` exposes
   only pure fns + the panel. This keeps FR12 clean.)*

   > **Simplification:** since both tools move to the main crate, `cp-mod-todo` may end up
   > with **no** `tool_definitions()` of its own. That's fine — a module can own a panel +
   > pure ops with zero tools. Confirm the module system tolerates an empty tool list
   > (it does: several modules expose panels without tools). Register `todo_mark`'s def in
   > the main-crate host module instead (Phase 5).
5. `load_module_data`: after loading `{todos, next_todo_id}`, call `purge_threadless(state)`
   (FR4). `save_module_data`: serialize **only** `{todos, next_todo_id}` — never `focus_filter`.
6. `overview_context_section`: **keep**, reframe label `"Todos: N/M done"` → `"Tasks: N/M done"`
   (global rollup, cancelled excluded from both N and M). This is design §11 site 3 (kept).
7. Update the module doc comment (`//!` header) — remove the `continue_until_todos_done`
   mention.

**Build gate:** `cargo build -p cp-mod-todo`.

---

## Phase 5 — main-crate hosts: `Think.todo` + `todo_mark` + focus injection + nudge

All in the main crate (`src/`), which reaches both `FocusState` and `TodoState`.

### 5a. `Think.todo` — `src/modules/questions/`
- `mod.rs` `tool_definitions()`: add an optional `todo` array param to the `Think` def
  (recursive object shape per Phase 2 `TodoNode`). Update `yamls/tools/core.yaml` Think text.
- `think.rs` `execute()`: after the thought validation, if `todo` present:
  - resolve `cp_mod_threads::types::FocusState::get(state).focused_thread_id`; **reject with a
    clear error when `None`** (design §5 / §9-#7).
  - deserialize the forest → `cp_mod_todo::upsert_task_forest(state, thread_id, &nodes)`.
  - on any change: `state.touch_panel(Kind::TODO)` (deprecate) but **keep**
    `result.preserves_tempo = true` (FR8 — defer to tempo exhaustion, bounded by `max_freeze=5`).
  - fold the `{created, updated, errors}` summary into the tool result text.

### 5b. `todo_mark` tool — host module (questions, or a small new `tasks` module)
- Define `todo_mark` (`marks: [{id, status}]`) in the host module's `tool_definitions()`
  + `execute_tool`. Resolve focused thread → `cp_mod_todo::mark_tasks(state, thread_id, &marks)`.
  - **Preserves tempo AND does NOT deprecate the panel** (FR7): set
    `result.preserves_tempo = true`, do **not** `touch_panel`.
  - Add `yamls/tools/todo.yaml` text for `todo_mark` (replace the 3 old tool texts).

  > Host choice: **QuestionsModule** is the pragmatic host (already main-crate, already
  > touches `FocusState` indirectly). A dedicated `src/modules/tasks/` module is cleaner but
  > costs an `all_modules()` registration. **Plan default: QuestionsModule host** to minimize
  > surface; revisit only if it bloats the file past the 500-line cap.

### 5c. Focus-filter injection + panel force-refresh — `src/app/run/tools/pipeline.rs`
- After a tool batch completes (end of `handle_tool_execution` / `finalize_tool_cycle`),
  sync focus:
  - `let focused = FocusState::get(&state).focused_thread_id.clone();`
  - `if cp_mod_todo::set_focus_filter(&mut state, focused) { /* changed */ }`
  - on change: **force the Todo panel fresh immediately (breaks tempo)** — mirror threads'
    `force_refresh_threads_panel`: set the `Kind::TODO` ctx `cache_deprecated = true` +
    `freeze_count = u8::MAX`, and `state.tempo = false` (design §4.1 focus-change row).

### 5d. Fire-once hygiene nudge (FR11) — `src/app/run/tools/pipeline.rs`
- After the focus sync, if a thread is focused, evaluate its tasks:
  - (a) no non-cancelled todos for the focused thread, **or**
  - (b) has `Planned` work but no `InProgress` item.
- Fire **at most once per focused thread** until the condition clears or focus changes.
  Track with a transient `Option<String> nudged_thread` (store in `TodoState`, not persisted,
  OR a field on the pipeline/app). Reset when the condition clears or focus changes.
- Emit via the **existing inject-but-don't-accumulate pattern** (copy
  `QuestionsModule::on_tool_complete`'s Think-reminder: `SpineState::create_notification(Custom,…)`
  then immediately `mark_notification_processed`). Short one-line wording per design §7.

**Build gate:** `cargo build` (whole workspace).

---

## Phase 6 — remove todo auto-continuation entirely (§11)

Pure deletion of existing behaviour. Sites (grep-verified):

1. `crates/cp-mod-spine/src/types.rs:101` — delete `SpineConfig.continue_until_todos_done`.
2. `crates/cp-mod-spine/src/tools.rs:155-157` — delete the read/set block in `execute_configure`.
3. `crates/cp-mod-spine/src/lib.rs:222` — delete `.param("continue_until_todos_done", …)`.
4. `yamls/tools/spine.yaml:12` — delete the param doc line.
5. `crates/cp-mod-spine/src/panel.rs` — delete `:37` (`writeln! continue_until…`) and the
   `:243-244` KeyValue row in `push_config_blocks`.
6. `src/app/actions/mod.rs:349-353` — delete the `Action::ConfigToggleAutoContinue` arm.
7. `crates/cp-base/src/state/actions.rs:137` — delete the `ConfigToggleAutoContinue` enum
   variant (exhaustive `Action` enum; site 6 is its only handler, site 8 its only producer).
8. `src/app/events/mod.rs:316` — delete the `KeyCode::Char('s') => …AutoContinue` keybind.
9. `src/ui/help/config_overlay/builder.rs` — delete `auto_on` (`:170`) and the
   `"Auto-continue"` `ConfigToggle` entry in `build_toggles`.
10. `src/app/run/lifecycle.rs` — delete the `self.check_todo_continuation();` call
    (`:359`) **and** the `fn check_todo_continuation` (`:406-437`).
11. `crates/cp-mod-todo/src/types.rs` — `has_incomplete_todos` / `incomplete_todos_summary`
    already removed in Phase 1 (their only callers vanish here).
12. `crates/cp-mod-todo/src/lib.rs` header doc — mention removed in Phase 4.

**Kept:** `overview_context_section` (`Tasks: N/M done` rollup), all other `spine_configure`
guard-rail params, all non-todo auto-continuation triggers.

**Build gate:** `cargo build` — the exhaustive `Action` match in `apply_action` must compile
with the variant gone (no stale arm).

---

## Phase 7 — build, callbacks, reload, commit

1. `cargo build` (workspace) → 0 errors.
2. `cargo test -p cp-mod-todo -p cp-mod-spine -p cp-mod-threads` (+ workspace if cheap).
3. Rust callbacks green (CB1 api-contract, CB4 rust-lints, CB5 structure ≤500/≤8).
   - Watch file-length caps: if `think.rs` / `pipeline.rs` / todo `tools.rs` cross 500 lines,
     **factor into a new file** (M-tiny-6 user preference — extract, don't trim comments).
4. `system_reload` (TUI change → build + reload; M-mid-10). No orchestrator change this pass.
5. Commit on `todo-v2` with memories + tree-descriptions (M-mid-7). Local only unless a PR is
   requested (M151 / M-mid-20).

---

## Risk / watch-list

- **Exhaustive `Action` enum** (site 7): removing a variant forces the `apply_action` match to
  drop its arm in the same commit, else `non_exhaustive`/E0004 breakage. Do 6+7+8 together.
- **`is_global` flip data migration**: old per-worker todos vanish on first load — this is the
  intended FR4 wipe, but verify no crash when the shared store has no todo key (defaults empty).
- **Focus-change tempo break** (5c): the Todo panel force-refresh must NOT touch the threads
  panel — they are fully decoupled (design §9-#5, critical for cache cost).
- **`todo_mark` never deprecates the panel** (FR7) vs **`Think.todo` does** (FR8): easy to
  mix up — the asymmetry is load-bearing (design §4.1).
- **Empty tool list on `cp-mod-todo`** (Phase 4 note): confirm the module system is happy with
  a panel-only, tool-less module before relying on it; fall back to hosting `todo_mark` in
  `cp-mod-todo` only if a threads-free scoping path exists (it doesn't cleanly — main-crate host
  is correct).
