# Design Observations

## 1. Asymmetric Timer Eligibility

Dynamic panels (Glob, Grep, GitResult) only refresh on their timer when **selected** by the user in the TUI sidebar. This is defined in `app.rs::check_timer_based_deprecation()`:

```rust
let interval_eligible = ctx.context_type.is_fixed()
    || i == self.state.selected_context
    || ctx.context_type == ContextType::Tmux;
```

**Implications:**
- The LLM always sees stale data in unselected dynamic panels during multi-turn conversations
- The "refreshed Xs ago" timestamp in the panel footer only advances when the user is looking at the panel
- After a `file_edit`, grep panels with matching results go stale silently — the LLM may grep for something, edit the file, and the grep panel still shows pre-edit results
- This is a **deliberate CPU optimization** (documented in the source comment) to avoid wasting background threads on panels nobody is looking at
- The trade-off is acceptable for TUI display freshness, but problematic for LLM context accuracy since ALL panels are sent to the LLM regardless of selection

## 2. No Cross-Panel Invalidation for File Mutations

When `file_edit` or `file_write` changes a file, the ripple effects are incomplete:

| Mutation | File Panel | Tree Panel | Glob Panels | Grep Panels | GitResult Panels |
|----------|-----------|-----------|-------------|-------------|-----------------|
| `file_edit` | ❌ (watcher race) | ❌ (correct — content change, not structure) | ❌ | ❌ | ❌ |
| `file_write` (existing) | ✅ deprecated | ❌ | ❌ | ❌ | ❌ |
| `file_write` (new file) | ✅ created | ✅ deprecated | ❌ | ❌ | ❌ |

**Missing invalidations:**
- **Glob panels** should be invalidated when `file_write` creates or deletes a file (changes which files match the glob pattern)
- **Grep panels** should be invalidated when `file_edit` or `file_write` modifies a file within the grep's search scope (changes which lines match)
- **GitResult panels** showing `git diff` or `git status` should be invalidated when any tracked file changes

The file watcher handles File panels reactively, but no equivalent mechanism exists for Glob/Grep. The `.git/` watcher catches GitResult indirectly (via `.git/index` changes after staging), but only after `git add`, not after a raw file edit.

## 3. .git/ Watcher Blanket-Deprecates All GitResult Panels

In `app.rs::process_watcher_events()`, any `.git/` file change sets `cache_deprecated = true` on **every** `ContextType::GitResult` panel:

```rust
if is_git_event {
    for ctx in self.state.context.iter_mut() {
        if ctx.context_type == ContextType::GitResult || ctx.context_type == ContextType::Git {
            ctx.cache_deprecated = true;
            self.state.dirty = true;
        }
    }
}
```

For a workspace with 10 open GitResult panels (e.g., `git log`, `git diff`, `git blame` on various files), a single commit triggers 10 background re-executions. The targeted invalidation rules in `cache_invalidation.rs` are only used for mutating git **tool** calls, not for watcher events.

**Contrast:** The tool-based invalidation is surgical — `git add src/main.rs` only invalidates panels running `git diff`, `git status`, etc. But a `.git/index` watcher event (which fires after `git add`) invalidates everything.

**Possible improvement:** Apply the same `cache_invalidation.rs` rules to watcher events. Would require inferring what changed from the `.git/` path (e.g., `.git/refs/heads/main` changed → only invalidate `git log`/`git branch` panels). Complex but would reduce unnecessary re-executions.

## 4. GithubResult Has No Blanket Invalidation Fallback

Git invalidation has a safe fallback in `git/tools.rs`:

```rust
let invalidations = super::cache_invalidation::find_invalidations(command);
if invalidations.is_empty() {
    // Unknown mutating command → blanket invalidation (safe default)
    for ctx in &mut state.context {
        if ctx.context_type == ContextType::GitResult {
            ctx.cache_deprecated = true;
        }
    }
}
```

GitHub invalidation in `github/tools.rs` does **not** have this fallback:

```rust
let invalidations = super::cache_invalidation::find_invalidations(command);
for ctx in &mut state.context {
    if ctx.context_type == ContextType::GithubResult {
        if let Some(ref cmd) = ctx.result_command {
            if invalidations.iter().any(|re| re.is_match(cmd)) {
                ctx.cache_deprecated = true;
            }
        }
    }
}
// No fallback if invalidations is empty!
```

If a mutating `gh` command isn't covered by any rule (e.g., future GitHub CLI additions, `gh extension` commands, `gh project` mutations), affected panels silently stay stale. The current rules are comprehensive for common operations, but the asymmetry with git's conservative fallback is a gap.

## 5. Deferred Sleep Targets ALL Tmux Panes

`console_send_keys` to a specific pane (e.g., `%5`) triggers a deferred sleep that, when expired, marks **ALL** tmux panels as deprecated:

```rust
// app.rs::check_deferred_sleep()
if needs_tmux {
    for ctx in &mut self.state.context {
        if ctx.context_type == ContextType::Tmux {
            ctx.cache_deprecated = true;
        }
    }
}
```

In a multi-pane setup (e.g., build console + test console + server log), sending keys to one pane refreshes all three. The `tmux capture-pane` call is cheap (~0.1ms), so the overhead is negligible. But it does cause unnecessary `last_refresh_ms` bumps on unrelated panes, which could confuse the "refreshed Xs ago" display.

## 6. Two Completely Different Polling Architectures

The codebase has two independent background polling systems:

1. **Cache pipeline** (`cache.rs` + `app.rs`) — timer-driven, spawns one-shot background threads per request, communicates via `CacheUpdate` channel. Used by all panel types.

2. **GhWatcher** (`gh_watcher.rs`) — dedicated long-lived background thread with its own polling loop, maintains per-panel state (ETags, hashes, intervals), sends `CacheUpdate` directly. Only used by GithubResult.

GhWatcher exists because GitHub API polling benefits from HTTP ETags and `X-Poll-Interval` header respect — features that don't fit the one-shot cache pipeline model. The two systems coexist cleanly (both send through `cache_tx`), but it means GithubResult panels have two refresh paths that could theoretically conflict (GhWatcher sends update while cache pipeline also sends one). In practice the content hash in `apply_cache_update` deduplicates this.
