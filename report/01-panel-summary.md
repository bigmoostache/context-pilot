# Panel-by-Panel Cache Invalidation Summary

| Panel | Type | Refresh Mechanism | Interval | Watcher | Cross-Panel Invalidation | cache_in_flight Bug? |
|-------|------|-------------------|----------|---------|--------------------------|---------------------|
| **File** | Dynamic | Watcher + explicit | None (watcher-only) | ✅ inotify on file | file_write ✓, file_edit ✗ | **YES** |
| **Tree** | Fixed | Watcher + explicit | None (watcher-only) | ✅ inotify on open dirs | file_write (new files) ✓ | No |
| **Glob** | Dynamic | Timer (selected only) | 30s | ❌ | ❌ None | No |
| **Grep** | Dynamic | Timer (selected only) | 30s | ❌ | ❌ None | No |
| **Tmux** | Dynamic | Timer (always) | 1s | ❌ | send_keys → deferred sleep | **YES** |
| **Git (P6)** | Fixed | Timer + .git/ watcher | GIT_STATUS_REFRESH_MS | ✅ .git/ files+refs | gh_execute → always invalidate | No |
| **GitResult** | Dynamic | Timer (selected only) + .git/ watcher | GIT_STATUS_REFRESH_MS | ✅ .git/ files+refs | git_execute → targeted regex rules | No |
| **GithubResult** | Dynamic | GhWatcher (60s, all) + Timer (120s, selected) | 60s/120s | ❌ | gh_execute → targeted regex+backrefs | No |

---

## Per-Panel Details

### File Panel (ContextType::File) — Dynamic

**Invalidation triggers:**

1. **File Watcher (inotify)** — `app.rs::process_watcher_events()` handles `WatchEvent::FileChanged(path)`. Matches context elements where `context_type == File && file_path == path`. Sets `cache_deprecated = true`, immediately builds cache request and spawns background thread. After processing, calls `watcher.rewatch_file()` for atomic rename support (editors like vim save via rename). NOT triggered for `.git/` paths.

2. **file_open tool** — Creates new `ContextElement` with `cache_deprecated: true`, `cached_content: None`. Background timer picks it up via `needs_initial` path. Wait-for-panels also triggers it if mid-tool-execution.

3. **file_edit tool** — Does NOT set `cache_deprecated`. Writes file directly to disk, updates `token_count` on the context element. Relies entirely on the file watcher to detect the disk write and trigger refresh. **Gap: race condition — LLM gets stale cached_content until watcher fires.**

4. **file_write tool** — If file already open: sets `cache_deprecated = true`. If file NOT open: creates new `ContextElement` with `cached_content: Some(contents)` AND `cache_deprecated: true`. Also invalidates Tree cache when creating new files.

5. **Timer-based** — File panels have NO `cache_refresh_interval_ms()`. Timer check only fires for `needs_initial` or `explicitly_deprecated`. No periodic polling — relies entirely on watchers + explicit deprecation.

6. **Wait-for-panels** — `has_dirty_file_panels()` checks `ContextType::File && cache_deprecated`. `trigger_dirty_panel_refresh()` spawns background refresh. Called after tool execution, blocks streaming until files load.

**Cache hash check:** `FilePanel::refresh_cache()` compares `current_hash` with `hash_content(&content)`. Returns `None` if unchanged — **this causes the cache_in_flight bug** (see Bugs section).

---

### Tree Panel (ContextType::Tree) — Fixed

**Invalidation triggers:**

1. **Directory Watcher (inotify)** — `WatchEvent::DirChanged(path)` matches ALL Tree context elements. Sets `cache_deprecated = true`, immediately builds cache request. Only watches `tree_open_folders`. NOT triggered for `.git/` paths.

2. **tree_toggle tool** — Opens/closes folders. Calls `invalidate_tree_cache()` → `cache_deprecated = true`.

3. **tree_describe tool** — Adds/updates/removes descriptions. Calls `invalidate_tree_cache()`.

4. **tree_filter tool** — Changes gitignore filter. Calls `invalidate_tree_cache()`.

5. **file_write tool (cross-panel)** — When creating a NEW file, invalidates Tree cache. Correct: new file changes directory listing.

**Cache hash check:** `apply_cache_update()` hashes content, returns false if unchanged. `refresh_cache()` always returns `Some(TreeContent)` — no cache_in_flight bug.

**Notes:** Watcher only covers open folders — changes to closed folders aren't detected until reopened. `git checkout` relies on inotify `DirChanged` events which may or may not fire depending on what changed.

---

### Glob Panel (ContextType::Glob) — Dynamic

**Invalidation triggers:**

1. **Timer-based** — `cache_refresh_interval_ms()` returns `GLOB_DEPRECATION_MS` (30s). Only fires when panel is **selected** (not fixed, not tmux). Unselected glob panels go stale indefinitely.

2. **Initial creation** — `file_glob` tool creates element with `cache_deprecated: true`. `needs_initial` path picks it up immediately.

3. **NO file watcher integration** — File additions/deletions within the glob pattern are only detected by timer (30s, selected only). No explicit invalidation from file_write, file_edit, or any other tool.

**Cache hash check:** `apply_cache_update()` hashes content. `refresh_cache()` always returns `Some(GlobContent)`. No cache_in_flight bug.

---

### Grep Panel (ContextType::Grep) — Dynamic

**Invalidation triggers:** Structurally identical to Glob.

1. **Timer-based** — `GREP_DEPRECATION_MS` (30s), selected only.
2. **Initial creation** — `file_grep` tool, `needs_initial` path.
3. **NO file watcher integration** — File edits that change matching lines won't be detected until timer refresh (and only if selected).

**Cache hash check:** Same as Glob — always returns `Some(GrepContent)`. No bug.

**Note:** More impactful than glob staleness — grep results are used for code navigation. Stale grep during active editing could mislead the LLM.

---

### Tmux Panel (ContextType::Tmux) — Dynamic

**Invalidation triggers:**

1. **Timer-based** — `TMUX_DEPRECATION_MS` (1s). Tmux is special-cased as always `interval_eligible` in `check_timer_based_deprecation()` — ALL tmux panels refresh every 1s, even when not selected.

2. **Initial creation** — `console_create` tool, `needs_initial` path.

3. **console_send_keys tool** — Does NOT set `cache_deprecated` directly. Sets `state.tool_sleep_until_ms` (deferred sleep) and `state.tool_sleep_needs_tmux_refresh = true`. After sleep expires, `check_deferred_sleep()` marks ALL tmux panels `cache_deprecated = true`, triggers refresh, enters wait-for-panels flow.

4. **Wait-for-panels** — `has_dirty_panels()` includes `ContextType::Tmux`. Tmux panels block streaming when dirty.

**Cache hash check:** `refresh_cache()` returns `None` if hash matches — **same cache_in_flight bug as File panel**. An idle tmux pane could get stuck. In practice rare (panes usually have changing content).

---

### Git Panel (ContextType::Git) — Fixed (P6)

**Invalidation triggers:**

1. **File Watcher (.git/ paths)** — Watches `.git/HEAD`, `.git/index`, `.git/MERGE_HEAD`, `.git/REBASE_HEAD`, `.git/CHERRY_PICK_HEAD` + `.git/refs/heads`, `.git/refs/tags`, `.git/refs/remotes` (recursive). Any change sets `cache_deprecated = true` on Git AND GitResult panels. Deliberately does NOT spawn immediate refresh — prevents feedback loop (git status → .git/index → watcher → repeat).

2. **Timer-based** — `GIT_STATUS_REFRESH_MS` interval. As a fixed panel, always eligible.

3. **git_configure_p6 tool** — Explicitly sets `cache_deprecated = true` after config change.

4. **gh_execute mutating commands** — Always invalidates Git status after ANY mutating gh command.

5. **git_execute does NOT directly invalidate P6** — Relies on `.git/` watcher.

**Cache hash check:** Returns `Some(GitStatusUnchanged)` when unchanged — NO cache_in_flight bug. When `cache_deprecated` is true, `build_cache_request` passes `current_hash: None` to force full refresh.

---

### GitResult Panel (ContextType::GitResult) — Dynamic

**Invalidation triggers:**

1. **Timer-based** — `GIT_STATUS_REFRESH_MS` interval. Dynamic → selected only.

2. **Initial creation / reuse** — `git_execute` ReadOnly creates or reuses panel with `cache_deprecated: true`.

3. **File Watcher (.git/ paths)** — Any `.git/` change deprecates ALL GitResult panels. Timer only fires when selected → unselected panels stay deprecated until viewed.

4. **git_execute Mutating** — Targeted invalidation via `cache_invalidation::find_invalidations(command)`. 11 regex rules. Unknown commands → blanket invalidation of ALL GitResult panels (safe fallback).

5. **gh_execute does NOT invalidate GitResult** — Correct: gh commands don't affect git refs.

**Cache hash check:** Always returns `Some(GitResultContent)`. Uses `update_if_changed()` with content hash. No bug.

---

### GithubResult Panel (ContextType::GithubResult) — Dynamic

**Invalidation triggers:**

1. **Timer-based** — 120s interval. Dynamic → selected only. Fallback mechanism.

2. **GhWatcher (background polling thread)** — Dedicated thread, polls every 5s wake cycle. Per-panel interval: 60s default or from `X-Poll-Interval` header. Two strategies: ETag-based (for `gh api`) and SHA-256 hash comparison (for other `gh` commands). Sends `CacheUpdate::GithubResultContent` DIRECTLY through `cache_tx` — bypasses `cache_deprecated`/`build_cache_request`. Watches ALL panels (not just selected).

3. **Initial creation / reuse** — `gh_execute` ReadOnly creates or reuses panel with `cache_deprecated: true`.

4. **gh_execute Mutating** — Targeted invalidation via `github/cache_invalidation::find_invalidations(command)`. Backreference-aware regex substitution. NO blanket fallback (unlike git).

5. **git_execute does NOT invalidate GithubResult** — Correct.

**Cache hash check:** Always returns `Some(GithubResultContent)`. No bug. GhWatcher sends directly to `cache_tx`, processed via `process_cache_updates`.
