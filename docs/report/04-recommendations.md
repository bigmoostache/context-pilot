# Recommended Fixes

## Priority 1: Fix `cache_in_flight` stuck forever (Bug 1)

**Impact:** Medium — panels permanently stop refreshing until app restart.

**Change:** Add a `CacheUpdate::Unchanged { context_id: String }` variant. Panels that detect no change return this instead of `None`. In `process_cache_updates`, handle it by clearing `cache_in_flight` and bumping `last_polled_ms` without touching content or `last_refresh_ms`.

**Files to modify:**
- `src/cache.rs` — Add `Unchanged` variant to `CacheUpdate` enum + `context_type()` + `context_id()` impls
- `src/modules/files/panel.rs` — `refresh_cache()`: return `Unchanged` instead of `None` on hash match
- `src/modules/tmux/panel.rs` — `refresh_cache()`: return `Unchanged` instead of `None` on hash match
- `src/core/app.rs` — `process_cache_updates()`: handle `Unchanged` variant (clear in_flight, bump polled_ms, don't set dirty)

**Estimated effort:** ~30 minutes.

---

## Priority 2: Fix `file_edit` not invalidating file panel (Bug 2)

**Impact:** Low-Medium — stale content visible to LLM for one turn, self-heals via watcher.

**Change:** After writing the file in `execute_edit()`, set `cache_deprecated = true` on the file's context element.

**Files to modify:**
- `src/modules/files/tools/edit_file.rs` — Add `ctx.cache_deprecated = true;` after `ctx.token_count = estimate_tokens(&content);`

**Estimated effort:** ~5 minutes.

---

## Priority 3: Cross-invalidate Grep/Glob on file mutations

**Impact:** Medium — LLM sees stale grep/glob results after editing files, potentially making wrong decisions.

**Change:** When `file_edit` or `file_write` modifies a file, deprecate grep panels whose search scope includes the file, and glob panels whose base path contains the file.

**Approach:**
- In `execute_edit()` and `execute()` (write), after the file write, iterate `state.context` looking for:
  - `ContextType::Grep` panels where the modified file's path is within `grep_path` (or `.` if None)
  - `ContextType::Glob` panels where the modified file's path is within `glob_path` (or `.` if None) — only for `file_write` creating new files
- Set `cache_deprecated = true` on matching panels

**Files to modify:**
- `src/modules/files/tools/edit_file.rs` — Add grep invalidation after write
- `src/modules/files/tools/write.rs` — Add grep + glob invalidation after write

**Estimated effort:** ~30 minutes.

---

## Priority 4: Add blanket fallback to GitHub invalidation

**Impact:** Low — only affects edge cases with uncovered `gh` commands.

**Change:** In `execute_gh_command()`, add the same blanket fallback that `execute_git_command()` has:

```rust
let invalidations = super::cache_invalidation::find_invalidations(command);
if invalidations.is_empty() {
    // Unknown mutating command → blanket invalidation
    for ctx in &mut state.context {
        if ctx.context_type == ContextType::GithubResult {
            ctx.cache_deprecated = true;
        }
    }
} else {
    // Targeted invalidation (existing code)
    ...
}
```

**Files to modify:**
- `src/modules/github/tools.rs` — Add `if invalidations.is_empty()` fallback

**Estimated effort:** ~10 minutes.

---

## Lower Priority / Future Considerations

### A. Apply targeted invalidation to .git/ watcher events
Currently `.git/` watcher events blanket-deprecate ALL GitResult panels. Could apply `cache_invalidation.rs` rules based on which `.git/` path changed. Complex but would reduce unnecessary re-executions in multi-panel setups.

### B. Target deferred sleep to specific tmux pane
`console_send_keys` to pane %5 currently refreshes ALL tmux panels. Could track which pane was targeted and only deprecate that one. Minor efficiency gain.

### C. Refresh all panels before streaming (not just File + Tmux)
`wait.rs::has_dirty_panels()` only checks File and Tmux panels. Grep/Glob panels deprecated during tool execution won't block streaming — the LLM may see stale search results. Extending wait-for-panels to include recently-deprecated Grep/Glob panels would improve context accuracy at the cost of slightly longer tool execution pauses.

### D. Periodic background refresh for LLM-visible panels
Currently, unselected dynamic panels only refresh when explicitly deprecated. Since ALL panels are sent to the LLM (not just the selected one), stale unselected panels degrade LLM context quality. A low-frequency background refresh (e.g., 60s) for all panels with `cached_content` that are included in LLM context would improve accuracy without major CPU impact.
