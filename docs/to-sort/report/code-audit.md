# Code Audit Report — context-pilot TUI

**Date:** 2026-02-14
**Scope:** Full codebase audit for production readiness
**Codebase:** ~28,400 LOC across ~100 Rust source files

---

## Executive Summary

The POC is functionally impressive — a multi-provider LLM TUI with panel-based context management, background caching, git/github integration, a prompt library system, and conversation history management. The architecture shows good instincts (module trait system, background I/O, content-hash-based caching), but the implementation has the hallmarks of rapid prototyping: a monolithic state struct, ~~no error type hierarchy~~, ~~zero test coverage in production code~~, ~~unbounded thread spawning~~, and ~~several paths that panic on recoverable errors~~.

**Risk Rating:** MEDIUM-LOW for production. No data-loss bugs found. ~~The app will crash on recoverable conditions (network errors, lock poisoning) and lacks the guardrails needed for a multi-developer team.~~ Lock poisoning, thread pool bounds, LLM error typing, TOCTOU ownership race, and all clippy warnings have been fixed. Test coverage includes 147 tests across 16 files covering core business logic. CI enforces clippy + fmt + build + test. Remaining structural items (State decomposition, ContextElement enum) are design improvements, not crash risks.

---

## 1. CRITICAL — Fix Before Anything Else

### 1.1 Zero Test Coverage in Production Code ✅ DONE

~~- **Files with `#[test]`:** 7 files total~~
~~- **Nature of tests:** All 7 are in classify.rs (git/github command validation) and claude_code.rs (live API integration tests that require OAuth tokens)~~
~~- **Unit tests for business logic:** ZERO~~
~~- **Integration tests directory:** Does not exist~~
~~- **CI pipeline** (`.github/workflows/rust.yml`): Runs `cargo test` — but there's nothing meaningful to test~~

**Fixed (Batch 2):** Added test infrastructure with 46 new unit tests across 8 files, bringing the total to **147 tests across 16 files**. Coverage now includes:

- **Test helper:** `MessageBuilder` in `src/state/message.rs` — fluent builder for `Message` structs with auto-incrementing IDs, chainable `.status()` and `.tl_dr()`
- **Pure function tests:** `estimate_tokens`, `compute_total_pages` (state/context.rs), `hash_content` (cache.rs), `format_messages_to_chunk` (state/message.rs)
- **Logic tests:** `is_turn_boundary` (core/context.rs), `check_can_deactivate` (modules/mod.rs), `validate_name` (preset/tools.rs), `validate_tldr` (memory/tools.rs)
- **LlmError Display:** All 5 variants (llms/error.rs)

**Still TODO for deeper coverage:**
- State serialization round-trip (load → save → load)
- Panel cache update application (`core/app.rs` cache processing logic)
- Conversation detachment end-to-end (`detach_conversation_chunks` — `is_turn_boundary` is now tested, but the full loop needs a State fixture)
- Tool dispatch and execution (`tools/mod.rs`)

### 1.2 Monolithic State Struct — 200 Fields, All `pub`

**File:** `src/state/runtime.rs` — 575 lines, ~80 pub fields + Default impl

The `State` struct is a god object. Every piece of runtime data lives here with `pub` visibility — UI state, git status, token counters, module-specific data, render caches, API check state, all mixed together. It's passed as `&mut State` to virtually every function in the codebase.

**Concrete problems this causes:**
- **Impossible to test in isolation.** You can't test the conversation detachment logic without constructing a full State with git fields, theme data, spine config, etc.
- **Invisible coupling.** A function that takes `&mut State` could read/write any of 80 fields. You can't know its actual dependencies without reading the full implementation.
- **Merge conflicts guaranteed.** As soon as 2+ developers touch State (adding fields, changing defaults), every PR conflicts.
- **No encapsulation.** Any module can modify any other module's state — e.g., git tools could accidentally mutate todo state.

**What to do:** Break into domain sub-structs:
- `ConversationState` (messages, input, cursor, paste_buffers, streaming flags)
- `PanelState` (context vec, selected_context)
- `GitState` (branch, branches, is_repo, file_changes, diff_base, etc.)
- `UiState` (dirty, scroll, spinner, theme, config_view, viewport)
- `TokenState` (all the hit/miss/output token counters)
- `ModuleData` (todos, memories, agents, skills, commands, scratchpad, logs, notifications, spine_config)

State becomes a thin wrapper: `pub struct State { pub panels: PanelState, pub conversation: ConversationState, ... }`. This is a large refactor but essential before scaling the team.

### 1.3 ContextElement Is a Flat Union of All Panel Types

**File:** `src/state/context.rs:70-162` — 30+ fields, most `Option<String>`

`ContextElement` uses the "bag of optionals" anti-pattern. A File panel only uses `file_path` and `file_hash`. A Grep panel only uses `grep_pattern`, `grep_path`, `grep_file_pattern`. But every panel carries all 15+ optional fields for all panel types, plus 12 cache/runtime fields.

**Concrete problems:**
- **Constructor hell.** Creating a `ContextElement` requires setting 30+ fields (see `core/context.rs:314-347` — a 34-line struct literal with 25 explicit `None` fields).
- **Type safety void.** Nothing prevents setting `grep_pattern` on a File panel. The compiler can't help you.
- **Serialization bloat.** Panel files on disk contain empty fields for every panel type.
- **Duplicated format logic.** `persistence/mod.rs:51-87` (`panel_to_context`) manually maps 15+ fields between `PanelData` and `ContextElement` — same fields, same optionals, same pattern.

**What to do:** Replace with a type-safe enum:
```rust
pub enum PanelConfig {
    File { path: String, hash: Option<String> },
    Glob { pattern: String, base_path: Option<String> },
    Grep { pattern: String, path: Option<String>, file_pattern: Option<String> },
    Tmux { pane_id: String, lines: Option<usize>, last_keys: Option<String>, description: Option<String> },
    GitResult { command: String, command_hash: String },
    GithubResult { command: String, command_hash: String },
    Skill { prompt_id: String },
    Fixed, // System, Overview, Tree, Git, Library, etc.
}

pub struct ContextElement {
    pub id: String,
    pub uid: Option<String>,
    pub context_type: ContextType,
    pub name: String,
    pub config: PanelConfig,
    // ... shared cache/runtime fields only
}
```

---

## 2. HIGH — Address in First Sprint

### 2.1 No Error Type — Everything Is `Result<T, String>` ✅ PARTIALLY DONE (LLM layer)

**Found across:** `src/llms/`, `src/modules/*/tools.rs`, `src/tools/mod.rs`, `src/cache.rs`

~~The entire codebase uses `String` as the error type.~~

**Fixed (Batch 2 — LLM layer):** Introduced `LlmError` enum in `src/llms/error.rs` with 5 typed variants:
- `Auth(String)` — missing/invalid API key or OAuth token
- `Network(String)` — DNS, connection, timeout (with `From<reqwest::Error>`)
- `Api { status: u16, body: String }` — non-success HTTP status
- `StreamRead(String)` — SSE stream read failure
- `Parse(String)` — JSON parse failure

Hand-rolled `Display`, `Error`, and `From<reqwest::Error>` (no `thiserror` dependency). Converted:
- `LlmClient::stream()` trait signature: `Result<(), String>` → `Result<(), LlmError>`
- All 5 client files: `anthropic.rs`, `claude_code.rs`, `grok.rs`, `groq.rs`, `deepseek.rs`
- `background.rs::summarize_content()`: `Result<String, String>` → `Result<String, LlmError>`
- `StreamEvent::Error` bridge: `LlmError` → `String` via `.to_string()` (display-only)

**NOT converted (intentionally):** Validation functions (`validate_git_command`, `check_shell_operators`, `validate_name`, `validate_tldr`) keep `Result<_, String>` — they produce user-facing `ToolResult` messages, not programmatic errors.

**Still TODO:** A broader `AppError` type for non-LLM code paths (persistence, config loading) is not yet needed — those paths use `Option<T>` or `eprintln!` logging, which is appropriate for their fire-and-forget semantics.

### 2.2 LLM Client Panics on Network Errors ~~INVALID — FALSE POSITIVE~~

> **Audit correction:** Upon inspection, ALL `.expect()` calls in `claude_code.rs` (lines ~860, ~902, ~905, ~916) are inside `#[cfg(test)] mod tests` — test-only code, not production. The production `stream()` method (line 288) properly uses `?` with `.map_err()` and `.ok_or_else()` throughout. No production panics exist. This finding was a false positive.

### 2.3 Unbounded Thread Spawning in Cache System ✅ DONE

**File:** `src/cache.rs`

~~Every cache refresh spawns a new OS thread with no limit.~~

**Fixed:** Replaced unbounded `thread::spawn` with a `CachePool` — a fixed-size thread pool of 6 named worker threads (`cache-worker-0` through `cache-worker-5`) using `mpsc::channel` + `Arc<Mutex<Receiver>>`. Global `CACHE_POOL: LazyLock<CachePool>` singleton. `process_cache_request()` now submits to pool instead of spawning.

### 2.4 Lock Poisoning = App Crash ✅ DONE

**Files:** `src/perf.rs` (11 locations), `src/gh_watcher.rs` (4), `src/highlight.rs` (2), `src/persistence/writer.rs` (4)

~~All use `.expect("...lock poisoned")` or `.unwrap()` on Mutex/RwLock.~~

**Fixed:** All 21 sites converted to `.unwrap_or_else(|e| e.into_inner())` recovery pattern. App now recovers from poisoned locks instead of crashing.

### 2.5 Content Hashing Uses Non-Cryptographic DefaultHasher ✅ DONE

**File:** `src/cache.rs`

~~SipHash — 64-bit, NOT collision resistant~~

**Fixed:** Replaced `DefaultHasher` (SipHash, 64-bit) with `sha2::Sha256` (256-bit, cryptographically collision resistant). `hash_content()` now produces 64-char SHA-256 hex strings. `sha2` crate was already a dependency.

---

## 3. MEDIUM — Plan for Next Iteration

### 3.1 The `App` Struct Mirrors State's Problems ✅ PARTIALLY DONE (Timers)

**File:** `src/core/app.rs:27-69` — 20 fields

The `App` struct accumulates timing fields, watcher handles, deferred state, and channel handles. `app.run()` is a 1161-line function containing the entire event loop. This should be decomposed:
- ~~Extract a `Timers` struct for the 6 `last_*_ms` fields~~
- Extract watcher management into its own struct
- Extract stream/retry logic into a `StreamController`

**Fixed (Batch 3 — Timers):** Extracted 5 `last_*_ms` timing fields into a `Timers` struct in `src/core/app.rs`. Fields: `timer_check_ms`, `ownership_check_ms`, `render_ms`, `spinner_ms`, `gh_sync_ms`. All references updated from `self.last_X_ms` to `self.timers.X_ms`. Pure structural cleanup, no behavior change.

### 3.2 Code Duplication: Chunk Content Formatting ✅ DONE

~~The logic for formatting conversation messages into text appears in TWO places with subtle differences:~~
~~- `src/core/context.rs:160-195` — `format_chunk_content()` (used for detachment)~~
~~- `src/persistence/mod.rs:128-177` — inline in `load_state_new()` (used for reload)~~

**Fixed:** Extracted shared `format_messages_to_chunk()` function in `src/state/message.rs`. Both `core/context.rs::format_chunk_content()` and `persistence/mod.rs::load_state_new()` now delegate to this single source of truth. Re-exported from `src/state/mod.rs`.

### 3.3 `CacheUpdate` / `CacheRequest` Enum Bloat

**File:** `src/cache.rs:15-141`

These enums have 10+ variants each, with many carrying the same fields (`context_id`, `content`, `token_count`). Both have `context_type()` and `context_id()` methods that pattern-match every variant. Adding a new cacheable panel type requires touching 4+ match arms across 2 enums.

Consider a struct-based approach: `CacheUpdate { context_id: String, update_type: CacheUpdateType, content: String, token_count: usize }`.

### 3.4 Persistence Layer Lacks Versioning ✅ DONE

~~No `version` field in `SharedConfig` or `WorkerState`~~

**Fixed:** Added `SCHEMA_VERSION: u32 = 1` constant and `schema_version` field to both `SharedConfig` and `WorkerState` in `src/state/config.rs`. Uses `#[serde(default = "default_schema_version")]` for backward compatibility with existing state files. `build_save_batch()` in `persistence/mod.rs` sets the version on save.

### 3.5 Persistence Writes Silently Swallow Errors ✅ DONE

~~`let _ = fs::write(path, content);` — Silently drops write errors~~

**Fixed:** `persistence/writer.rs::write_file()` now logs errors via `eprintln!` for both `create_dir_all` and `fs::write` failures. `persistence/mod.rs::save_state()` also logs I/O errors instead of silently discarding them. Errors in `execute_batch()` are logged (excluding `NotFound` on deletes, which is expected).

### 3.6 TOCTOU Race in Ownership Check ✅ MITIGATED

**File:** `src/persistence/mod.rs:470-478`

```rust
pub fn check_ownership() -> bool {
    if let Some(cfg) = config::load_config() {
        if let Some(owner) = cfg.owner_pid {
            return owner == current_pid();
        }
    }
    true
}
```

~~Between reading the config and acting on the result, another process could write a new owner PID. This is a classic Time-Of-Check-Time-Of-Use issue. Use file locking (e.g., `fs2` crate's `lock_exclusive()`) for proper mutex behavior.~~

**Fixed (Batch 3):** Mitigated the two concrete damage scenarios without adding a file-locking dependency:
1. **On ownership loss:** `PersistenceWriter::cancel()` discards all pending writes and shuts down immediately (new `Cancel` variant in `WriterMsg`), preventing the exiting process from overwriting the new owner's state.
2. **On graceful exit:** `save_state_and_release()` clears `owner_pid` from config.json before the final save, preventing stale PIDs from blocking future instances.
3. **Drop safety:** `PersistenceWriter::Drop` now checks `self.handle.is_some()` to avoid double-flush after `cancel()`.

The fundamental TOCTOU window still exists (no file locking), but the damage from the race is now limited — the exiting process can no longer corrupt the new owner's state.

### 3.7 `is_fixed()` Allocates on Every Call ✅ DONE

~~`all_modules()` allocates a `Vec<Box<dyn Module>>` (15 heap allocations) on every call.~~

**Fixed:** Added `FIXED_TYPES` and `CACHE_TYPES` as `LazyLock<HashSet<ContextType>>` in `src/state/context.rs`. Both `is_fixed()` and `needs_cache()` now do simple `HashSet::contains()` lookups — zero allocations per call. Eliminates dozens of heap allocations per second.

---

## 4. LOW — Track for Future

### 4.1 CI Pipeline Is Minimal ✅ DONE

~~No `clippy` lint check, no `rustfmt` check~~

**Fixed (Batch 1):** Enhanced `.github/workflows/rust.yml` with:
- `cargo fmt -- --check` step (formatting enforcement)
- `cargo clippy -- -D warnings` step (lint enforcement)
- Steps ordered: fmt → clippy → build → test

**Fixed (Batch 3):** Resolved all 193 pre-existing clippy warnings across ~60 files. Major categories: collapsible_if (145), empty_line_after_doc_comments (7), while_let_on_iterator (7), unnecessary_map_or (7), useless_format (5), needless_borrow (4), manual_div_ceil (3), type_complexity (2), plus ~15 single-instance lints. Codebase now passes `cargo clippy -- -D warnings` cleanly.

### 4.2 No `rustfmt.toml` or `clippy.toml` ✅ DONE (rustfmt)

~~No code style enforcement.~~

**Fixed:** Created `rustfmt.toml` with `max_width = 120`, `use_small_heuristics = "Max"`, `edition = "2021"`. Enforced in CI via `cargo fmt -- --check`. (`clippy.toml` not yet created — clippy defaults are sufficient for now.)

### 4.3 Hardcoded Model Pricing

**File:** `src/llms/mod.rs:112-142`

Model prices are hardcoded in match arms with no documentation of units or source. When Anthropic changes pricing (which they do regularly), someone needs to find these buried constants and update them.

**What to do:** Move to a YAML config file (the codebase already has a pattern for embedded YAML configs via `LazyLock`).

### 4.4 Magic Constants Scattered ✅ DONE

**File:** `src/constants.rs` — ~~63 lines of constants, but many more scattered in modules~~

~~- `DEBOUNCE_MS = 50` in `persistence/writer.rs:60`~~
~~- Various inline magic numbers in `app.rs`~~
- `GH_RESULT_REFRESH_MS` in github module (correctly module-local)
- `GLOB_DEPRECATION_MS` in glob module (correctly module-local)

**Fixed (Batch 3):** Centralized 5 inline magic constants into `src/constants.rs`:
- `IDLE_POLL_MS = 50` (was inline `50` in app.rs)
- `SPINNER_THROTTLE_MS = 100` (was inline `100` in app.rs)
- `TIMER_CHECK_MS = 100` (was inline `100` in app.rs)
- `OWNERSHIP_CHECK_MS = 1_000` (was inline `1000` in app.rs)
- `PERSISTENCE_DEBOUNCE_MS = 50` (was `DEBOUNCE_MS` in writer.rs)

Module-owned constants (e.g., `GLOB_DEPRECATION_MS`, `GH_RESULT_REFRESH_MS`) remain in their modules — they are correctly scoped.

### 4.5 Clone-Heavy Data Flow

**Observation:** 589 `.clone()` calls across the codebase (per grep count). The cache system clones full panel content strings across thread boundaries. `StreamContext` clones the entire message vec and tool definitions on every LLM tick.

Not a bug — Rust requires explicit cloning — but as content grows, this could become a performance bottleneck. Consider `Arc<String>` for shared immutable content, or `Cow<'_, str>` where ownership isn't always needed.

### 4.6 `swallowed.rs` LogOk Adoption Incomplete ✅ PARTIALLY DONE

~~`persistence/writer.rs` uses raw `let _ = fs::write(...)` extensively~~

**Fixed:** `persistence/writer.rs` now logs all I/O errors via `eprintln!` instead of silently swallowing them. Full `LogOk` trait adoption across the codebase is not yet done but the most critical location (persistence writes) is now covered.

### 4.7 GitHub Token Stored as Plain String

**File:** `src/state/runtime.rs:177`

```rust
pub github_token: Option<String>,
```

The Anthropic API key uses `secrecy::SecretBox` (good), but the GitHub token is a plain `String` that could appear in debug output, logs, or core dumps.

---

## 5. Performance Analysis

### 5.1 Render Pipeline — WELL OPTIMIZED

The render system is the strongest engineering in the codebase.

**Three-level render cache** (`src/modules/core/conversation_panel.rs:84-240`, `src/state/render_cache.rs`):
1. **Full content cache** — hashes viewport width, dev_mode, streaming state, all message contents. If hash matches, returns cached `Rc<Vec<Line>>` immediately.
2. **Per-message cache** — `HashMap<String, MessageRenderCache>` stores rendered lines per message ID. Only re-renders messages whose content hash changed.
3. **Input cache** — cached by input string + cursor position + viewport width.

All caches use `Rc<Vec<Line>>` to avoid deep cloning rendered output. This is well-designed.

**One inefficiency:** `compute_full_content_hash()` (`conversation_panel.rs:51-82`) iterates ALL messages hashing their content on EVERY render call — even when the full cache will hit. This could be guarded by a simple "conversation_changed" dirty flag.

**Adaptive polling** (`constants.rs:92-95`):
- `EVENT_POLL_MS = 8ms` during streaming/dirty (responsive)
- Falls back to ~50ms idle (saves CPU)
- `RENDER_THROTTLE_MS = 36ms` caps at ~28fps (appropriate for TUI)
- Spinner throttled to 100ms (10fps) — good separation of animation rate from render rate

### 5.2 Event Loop — 14 Steps Per Iteration, Well-Ordered

**File:** `src/core/app.rs:133-240`

Each loop iteration processes in sequence:
1. Input polling (non-blocking `event::poll(Duration::ZERO)`)
2. Immediate render after input (instant feedback)
3. Stream events (`try_recv` — non-blocking)
4. Retry handling (flag check)
5. Typewriter buffer drain
6. TL;DR results (`try_recv`)
7. Cache updates (`try_recv`)
8. File watcher events
9. Wait-for-panels check
10. Deferred sleep check
11. GH watcher sync (every 5s, mutex lock)
12. Timer-based cache deprecation (every 100ms)
13. Tool execution state machine
14. API check results (`try_recv`)

Only 4 channel receives per iteration, all non-blocking. Timer checks are properly gated. This is efficient.

**Save state frequency:** Uses `PersistenceWriter::send_batch()` (debounced, background thread). Sync `save_state()` only on quit. Excellent.

### 5.3 Memory Allocation Hot Paths

**Conversation rendering** (`src/modules/core/conversation_render.rs`):
- `format!()` calls per tool param, per message prefix, per line
- `wrap_text()` called per content line per message
- ALL mitigated by per-message render cache — only re-rendered on content change
- **Net impact: LOW** due to caching

**Markdown parsing** (`src/ui/markdown.rs`):
- Character-by-character `String::push()` for code/bold/italic segments
- Table rendering allocates `Vec<Vec<String>>` for cells
- Called only on cache miss (per-message cache)
- **Net impact: LOW**

**Overview render** (`src/modules/core/overview_render.rs`):
- Creates `Vec<Vec<Cell>>` rows per render (no cache for this panel)
- Multiple `format!()` calls per context element for token counts, costs, paths
- Scales with number of panels (~20) and git file changes
- **Net impact: LOW-MEDIUM** — panel is optional and low-frequency

### 5.4 Hot Allocation: `is_fixed()` and `needs_cache()` ✅ DONE

~~`all_modules()` heap-allocates 15 `Box<dyn Module>` each call.~~

**Fixed:** Pre-computed into `LazyLock<HashSet<ContextType>>` — see section 3.7 above.

### 5.5 Large Data Handling

**Size limits in place** (`src/constants.rs`):
- `PANEL_MAX_LOAD_BYTES = 5 MB` — hard cap per panel
- `MAX_RESULT_CONTENT_BYTES = 1 MB` — git/gh command output cap
- `PANEL_PAGE_TOKENS = 25,000` — pagination threshold
- `GIT_CMD_TIMEOUT_SECS = 30` / `GH_CMD_TIMEOUT_SECS = 60` — command timeouts

These are sensible. No unbounded content loading found.

**Conversation growth concern:** `State.messages: Vec<Message>` grows unbounded. However, the `detach_conversation_chunks()` function (`core/context.rs:205-359`) detaches old messages into frozen ConversationHistory panels when thresholds are exceeded (`DETACH_CHUNK_MIN_MESSAGES = 25`, `DETACH_CHUNK_MIN_TOKENS = 5000`). This is a good automatic pruning mechanism.

### 5.6 Profiling Infrastructure — EXCELLENT

**Files:** `src/profiler.rs` (58 lines), `src/perf.rs` (~300 lines)

- `profile!("name")` macro — drop-based guard records operation timing
- Logs operations >5ms to `.context-pilot/perf.log`
- In-memory `PerfMetrics` with RingBuffer (64 samples per operation)
- Frame timing tracked in separate RingBuffer (40 entries)
- CPU/RAM monitoring via `/proc/self/stat` (every 500ms)
- F12 toggles live overlay with top 10 operations, sparkline, budget bars
- **Zero overhead when disabled** — guard is created but metrics collection is the only cost (microseconds)

**What's missing:**
- No allocation profiling (can't see how much memory render allocations consume)
- No per-panel render time breakdown
- No cache hit rate metrics in the overlay
- Consider adding `jemalloc` with stats or `dhat` for heap profiling during development

### 5.7 Performance Summary

| Area | Rating | Notes |
|------|--------|-------|
| Render caching | Excellent | Three-level cache with Rc, hash-based invalidation |
| Event loop | Excellent | Adaptive polling, non-blocking channels, proper throttling |
| Memory allocation | Good → ✅ Excellent | ~~`is_fixed()`/`needs_cache()` hot allocs~~ Fixed via LazyLock |
| Large data | Good | Size limits, pagination, conversation detachment |
| Profiling | Excellent | Low-overhead, production-safe, useful overlay |
| Thread efficiency | ~~Poor~~ → ✅ Good | ~~Unbounded spawning~~ Fixed: 6-thread CachePool |

**Bottom line:** Performance is not a concern for current usage. The render cache is genuinely well-engineered. ~~The two actionable items are: (1) pre-compute `is_fixed()`/`needs_cache()` sets, and (2) bound the cache thread pool.~~ Both items are now fixed.

---

## 6. Positive Observations — What's Done Right

These should be preserved and built upon:

- **Shell injection prevention** (`git/classify.rs`, `github/classify.rs`): Thorough validation with proper tests. Blocks `|`, `;`, `` ` ``, `$()`, `&&`, newlines. This is production-quality security code.
- **Background persistence writer** (`persistence/writer.rs`): Clean channel-based design with debouncing, proper Drop impl, flush synchronization. Well-engineered.
- **Module trait system** (`modules/mod.rs`): Clean plugin architecture with dependency validation, fixed panel ordering, tool registration. Good foundation for extensibility.
- **Adaptive polling** (`core/app.rs`): 50ms idle / 8ms active is a sensible approach that balances CPU usage and responsiveness.
- **Content-hash-based cache invalidation**: Prevents unnecessary updates and timestamp bumps. ~~The pattern is correct even if the hash function should be stronger.~~ Now using SHA-256 — collision-resistant.
- **Theme system** (`config.rs`): `AtomicPtr` for zero-cost theme access is the right call. The single `unsafe` block is well-justified and documented.
- **Git file watchers**: Smart use of `notify` crate to reactively refresh panels instead of polling.

---

## Recommended Remediation Order

| Priority | Item | Effort | Impact | Status |
|----------|------|--------|--------|--------|
| 1 | Add test infrastructure + core tests | 1 week | Enables safe refactoring | ✅ Done (Batch 2) — 147 tests |
| 2 | Add clippy/fmt to CI | 1 hour | Catches bugs automatically | ✅ Done (Batch 1) |
| 3 | Fix claude_code.rs panics → Result | 2 hours | Prevents user-facing crashes | ~~N/A~~ False positive |
| 4 | Fix lock poisoning patterns | 2 hours | Prevents cascading crashes | ✅ Done (Batch 1) |
| 5 | Add thread pool for cache | 4 hours | Prevents resource exhaustion | ✅ Done (Batch 1) |
| 6 | Introduce LlmError type | 1 day | Typed error handling for LLM layer | ✅ Done (Batch 2) |
| 7 | ContextElement → enum-based PanelConfig | 2-3 days | Type safety, less boilerplate | |
| 8 | Break State into sub-structs | 3-5 days | Testability, encapsulation | |
| 9 | Add persistence versioning | 1 day | Safe schema evolution | ✅ Done (Batch 1) |
| 10 | Extract duplicated chunk formatting | 2 hours | DRY | ✅ Done (Batch 1) |

**Additional fixes applied:**
| Item | Source | Status |
|------|--------|--------|
| SHA-256 content hashing | 2.5 | ✅ Done (Batch 1) |
| `is_fixed()`/`needs_cache()` LazyLock | 3.7 / 5.4 | ✅ Done (Batch 1) |
| Persistence write error logging | 3.5 | ✅ Done (Batch 1) |
| `rustfmt.toml` creation | 4.2 | ✅ Done (Batch 1) |
| Persistence error logging (LogOk) | 4.6 | ✅ Partially done (Batch 1) |
| `LlmError` enum + 5 client conversions | 2.1 | ✅ Done (Batch 2) |
| `MessageBuilder` test helper | 1.1 | ✅ Done (Batch 2) |
| Centralize inline magic constants | 4.4 | ✅ Done (Batch 3) |
| TOCTOU ownership race mitigation | 3.6 | ✅ Done (Batch 3) |
| Extract `Timers` struct from App | 3.1 | ✅ Done (Batch 3) |
| Resolve all 193 clippy warnings | 4.1 | ✅ Done (Batch 3) |

**Next steps (Batch 4 candidates):**
| Item | Source | Effort | Notes |
|------|--------|--------|-------|
| ContextElement → enum-based PanelConfig | 1.3 | 2-3 days | Biggest type-safety win. Tests from Batch 2 cover `is_turn_boundary`, `check_can_deactivate`, etc. so refactoring is now safer. |
| Break State into sub-structs | 1.2 | 3-5 days | Depends on ContextElement rework. Would benefit from more test coverage on State-dependent code paths first. |
| Deeper test coverage | 1.1 | 1-2 days | State serialization round-trip, `detach_conversation_chunks` end-to-end, tool dispatch. Needs a `State` test fixture (minimal `State::default()` that doesn't touch disk). |

---

## Files Referenced in This Audit

| File | Lines | Key Concern |
|------|-------|-------------|
| `src/state/runtime.rs` | 575 | Monolithic State struct |
| `src/state/context.rs` | 174 | ~~ContextElement union type~~ + ✅ LazyLock is_fixed/needs_cache + ✅ tests |
| `src/core/app.rs` | 1161 | God function event loop + ✅ Timers struct + named constants (Batch 3) |
| `src/core/context.rs` | 360 | Stream context + detachment + ✅ `is_turn_boundary` tests |
| `src/cache.rs` | 212 | ✅ Bounded thread pool + SHA-256 hash + ✅ tests |
| `src/llms/error.rs` | — | ✅ New (Batch 2) — `LlmError` enum (Auth, Network, Api, StreamRead, Parse) |
| `src/llms/mod.rs` | 563 | ✅ `LlmClient::stream()` → `Result<(), LlmError>` |
| `src/llms/anthropic.rs` | 534 | ✅ Converted to `LlmError` |
| `src/llms/claude_code.rs` | 1140 | ~~Panics on network errors~~ False positive. ✅ Converted to `LlmError` |
| `src/llms/grok.rs` | 258 | ✅ Converted to `LlmError` |
| `src/llms/groq.rs` | 292 | ✅ Converted to `LlmError` |
| `src/llms/deepseek.rs` | 308 | ✅ Converted to `LlmError` |
| `src/background.rs` | 149 | ✅ `summarize_content()` → `Result<String, LlmError>` |
| `src/persistence/mod.rs` | 514 | ✅ Error logging + shared format fn + schema version |
| `src/persistence/writer.rs` | 229 | ✅ Lock poisoning recovery + error logging + Cancel variant (Batch 3) |
| `src/modules/mod.rs` | 458 | Module system (good) + ✅ `check_can_deactivate` tests |
| `src/modules/preset/tools.rs` | — | ✅ `validate_name` tests |
| `src/modules/memory/tools.rs` | — | ✅ `validate_tldr` tests |
| `src/state/message.rs` | — | ✅ `MessageBuilder` test helper + `format_messages_to_chunk` tests |
| `src/constants.rs` | 100+ | ✅ Centralized event loop + persistence constants (Batch 3) |
| `src/perf.rs` | ~300 | ✅ Lock poisoning recovery (11 sites) |
| `src/gh_watcher.rs` | 493 | ✅ Lock poisoning recovery (4 sites) |
| `src/highlight.rs` | — | ✅ Lock poisoning recovery (2 sites) |
| `.github/workflows/rust.yml` | 23 | ✅ clippy + fmt enforcement |
| `rustfmt.toml` | — | ✅ New — code style config |
| `src/state/config.rs` | — | ✅ `SCHEMA_VERSION` + versioned configs |
