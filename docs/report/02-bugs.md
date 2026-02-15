# Bugs Found

## Bug 1: `cache_in_flight` stuck forever when `refresh_cache()` returns `None`

**Affected panels:** File, Tmux

**Severity:** Medium — panels permanently stop refreshing until app restart.

### Root Cause

`process_cache_request()` in `cache.rs` spawns a background thread that calls `panel.refresh_cache(request)`. If the result is `Some(update)`, it sends the update through the `cache_tx` channel. If `None`, nothing is sent.

Back in the main loop, `process_cache_updates()` in `app.rs` receives updates from `cache_rx` and clears `cache_in_flight` on the matching context element:

```rust
// app.rs — process_cache_updates
ctx.cache_in_flight = false;
ctx.last_polled_ms = now_ms();
```

**The problem:** `cache_in_flight = false` is ONLY set when a `CacheUpdate` is received. If `refresh_cache()` returns `None` (content unchanged), no update is sent, and `cache_in_flight` stays `true` forever.

In `check_timer_based_deprecation()`, panels with `cache_in_flight == true` are skipped:

```rust
if ctx.cache_in_flight { continue; }
```

**Result:** The panel never gets another refresh attempt. It's stuck.

### Why Only File and Tmux

- **File:** `FilePanel::refresh_cache()` returns `None` when `current_hash == new_hash` (file unchanged since last check)
- **Tmux:** `TmuxPanel::refresh_cache()` returns `None` when `current_content_hash == new_hash` (pane content unchanged)
- **Git:** Returns `Some(GitStatusUnchanged)` — always sends a response
- **Tree, Glob, Grep:** `refresh_cache()` always returns `Some(...)` — they don't have an early-exit path
- **GitResult, GithubResult:** Always return `Some(...)` from `refresh_cache()`

### Reproduction

1. Open a file with `file_open`
2. Don't modify the file
3. Wait for the file watcher to trigger a refresh (or manually deprecate)
4. `refresh_cache()` sees unchanged hash → returns `None`
5. `cache_in_flight` stays `true`
6. No future refresh attempts — file panel is frozen

For Tmux: leave a pane idle with no output. After one refresh cycle detects no change, the pane is frozen until external output appears (which it can't detect because polling stopped).

### Recommended Fix

Add a `CacheUpdate::Unchanged` variant (or per-type variants like `FileUnchanged`, `TmuxUnchanged`):

```rust
// cache.rs
pub enum CacheUpdate {
    // ... existing variants ...
    /// Content unchanged — clear in_flight without updating content
    Unchanged { context_id: String },
}
```

In `refresh_cache()`, return `Some(CacheUpdate::Unchanged { ... })` instead of `None`.

In `process_cache_updates()`, handle `Unchanged` by clearing `cache_in_flight` and bumping `last_polled_ms` without touching content, `last_refresh_ms`, or setting `dirty`.

---

## Bug 2: `file_edit` does not invalidate the file panel

**Affected panels:** File

**Severity:** Low-Medium — stale content for one LLM turn, self-heals via watcher.

### Root Cause

In `edit_file.rs::execute_edit()`, after writing the file to disk:

```rust
// Update the context element's token count
if let Some(ctx) = state.context.iter_mut().find(|c| {
    c.context_type == ContextType::File && c.file_path.as_deref() == Some(path_str)
}) {
    ctx.token_count = estimate_tokens(&content);
    // ← Missing: ctx.cache_deprecated = true;
}
```

The token count is updated, but `cache_deprecated` is NOT set. The `cached_content` field (which is what the LLM actually sees) retains the **pre-edit** content.

### Why It Usually Self-Heals

The file watcher (`watcher.rs`) detects the disk write via inotify and fires `WatchEvent::FileChanged(path)`, which sets `cache_deprecated = true` and triggers a background refresh. But there's a timing gap:

1. `file_edit` writes to disk and returns
2. Tool execution continues (more tools may execute in the same batch)
3. `handle_tool_execution()` creates the tool result message
4. `wait_for_panels` checks for dirty panels — but the file isn't marked dirty yet
5. Streaming continues — LLM sees stale `cached_content`
6. Eventually, the watcher event fires and the panel refreshes
7. Next LLM turn sees correct content

### Impact

During the tool execution batch, the LLM may read the "current file content" from the panel and see pre-edit text. If it's editing the same file multiple times in one turn, subsequent edits may be based on stale content (though `file_edit` reads from disk, not `cached_content`, so the edit itself is correct — only the LLM's context view is wrong).

### Recommended Fix

One line in `edit_file.rs`:

```rust
if let Some(ctx) = state.context.iter_mut().find(|c| {
    c.context_type == ContextType::File && c.file_path.as_deref() == Some(path_str)
}) {
    ctx.token_count = estimate_tokens(&content);
    ctx.cache_deprecated = true; // ← ADD THIS
}
```

Compare with `file_write`, which correctly does this for existing files:

```rust
if let Some(ctx) = already_open {
    ctx.token_count = token_count;
    ctx.cache_deprecated = true; // ← file_write does it right
}
```
