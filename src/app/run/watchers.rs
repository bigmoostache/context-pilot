use std::sync::mpsc::Receiver;

use crate::app::panels::now_ms;
use crate::infra::watcher::WatchEvent;
use crate::state::State;
use crate::state::cache::{CacheRequest, CacheUpdate, process_cache_request};

use crate::app::App;

/// Cache-refresh requests indexed by panel position, paired with the indices of
/// panels that asked to auto-close (`suicide`).
type TimerScan = (Vec<(usize, CacheRequest)>, Vec<usize>);

/// Set up file watchers from all modules' `watch_paths()`.
pub(super) fn setup_file_watchers(app: &mut App) {
    sync_file_watchers(app);
}

/// Schedule initial cache refreshes for fixed context elements only.
/// Dynamic panels (File, Glob, Grep, Tmux, `GitResult`, `GithubResult`) will be
/// populated gradually by `check_timer_based_deprecation` via its `needs_initial`
/// path, staggered by the `cache_in_flight` guard — preventing a massive burst
/// of concurrent background threads on startup when many panels are persisted.
pub(super) fn schedule_initial_cache_refreshes(app: &mut App) {
    // Collect requests first (immutable borrow), then mark in-flight (mutable borrow).
    let requests: Vec<(usize, CacheRequest)> = app
        .state
        .context
        .iter()
        .enumerate()
        .filter(|entry| entry.1.context_type.is_fixed())
        .filter_map(|(i, ctx)| {
            let panel = crate::app::panels::get_panel(&ctx.context_type);
            panel.build_cache_request(ctx, &app.state).map(|req| (i, req))
        })
        .collect();
    for (i, request) in requests {
        process_cache_request(request, app.cache_tx.clone());
        if let Some(ctx) = app.state.context.get_mut(i) {
            ctx.cache_in_flight = true;
        }
    }
}

/// Process incoming cache updates from background threads
pub(super) fn process_cache_updates(app: &mut App, cache_rx: &Receiver<CacheUpdate>) {
    process_cache_updates_static(&mut app.state, cache_rx);
}

/// Apply an `Unchanged` cache update: clear the in-flight + deprecated flags.
/// Returns `true` when the update was of this kind (caller should skip on).
fn apply_unchanged_update(state: &mut State, update: &CacheUpdate) -> bool {
    let Some(context_id) = update.unchanged_context_id() else { return false };
    if let Some(ctx) = state.context.iter_mut().find(|c| c.id == context_id) {
        ctx.cache_in_flight = false;
        ctx.cache_deprecated = false;
    }
    true
}

/// Apply a `ModuleSpecific` cache update (matched by context type). On a type
/// mismatch, hands the update back via `Err` for the `Content` path.
fn apply_module_specific_update(state: &mut State, update: CacheUpdate) -> Result<(), CacheUpdate> {
    let Some(context_type) = update.module_specific_type().cloned() else { return Err(update) };
    let Some(idx) = state.context.iter().position(|c| c.context_type == context_type) else { return Ok(()) };
    let mut ctx = state.context.remove(idx);
    let panel = crate::app::panels::get_panel(&ctx.context_type);
    let _changed = panel.apply_cache_update(update, &mut ctx, state);
    ctx.cache_in_flight = false;
    state.context.insert(idx, ctx);
    state.flags.ui.dirty = true;
    Ok(())
}

/// Apply a `Content` cache update (matched by context id).
fn apply_content_update(state: &mut State, update: CacheUpdate) {
    let Some(context_id) = update.content_context_id() else { return };
    let Some(idx) = state.context.iter().position(|c| c.id == context_id) else { return };
    let mut ctx = state.context.remove(idx);
    let panel = crate::app::panels::get_panel(&ctx.context_type);
    // apply_cache_update calls update_if_changed which sets last_refresh_ms on change
    let _changed = panel.apply_cache_update(update, &mut ctx, state);
    ctx.cache_in_flight = false;
    state.context.insert(idx, ctx);
    state.flags.ui.dirty = true;
}

/// Static version of `process_cache_updates` for use in wait module
fn process_cache_updates_static(state: &mut State, cache_rx: &Receiver<CacheUpdate>) {
    let _guard = crate::profile!("app::cache_updates");
    let _fg = cp_base::flame!("cache_updates");
    while let Ok(update) = cache_rx.try_recv() {
        if apply_unchanged_update(state, &update) {
            continue;
        }
        if let Err(leftover) = apply_module_specific_update(state, update) {
            apply_content_update(state, leftover);
        }
    }
}

/// Mark every panel a module claims for `path` as `cache_deprecated`, returning
/// the indices whose owning module wants an immediate refresh.
fn invalidate_matching_panels(app: &mut App, path: &str, is_dir_event: bool) -> Vec<usize> {
    let modules = crate::modules::all_modules();
    let mut refresh_indices = Vec::new();
    for (i, ctx) in app.state.context.iter_mut().enumerate() {
        for module in &modules {
            if module.should_invalidate_on_fs_change(ctx, path, is_dir_event) {
                ctx.cache_deprecated = true;
                if module.watcher_immediate_refresh() {
                    refresh_indices.push(i);
                }
                app.state.flags.ui.dirty = true;
                break; // Only one module owns each context type
            }
        }
    }
    refresh_indices
}

/// First pass: ask every module which panels to invalidate for the given file
/// events. Marks matching contexts `cache_deprecated`, returns the panel indices
/// needing an immediate refresh + the non-dir paths to re-watch.
fn collect_invalidations(app: &mut App, events: &[WatchEvent]) -> (Vec<usize>, Vec<String>) {
    let mut refresh_indices = Vec::new();
    let mut rewatch_paths: Vec<String> = Vec::new();
    for event in events {
        let (path, is_dir_event) = cp_base::deref_match!(event, {
            WatchEvent::FileChanged(ref p) => (p, false),
            WatchEvent::DirChanged(ref p) => (p, true),
        });
        refresh_indices.extend(invalidate_matching_panels(app, path, is_dir_event));
        if !is_dir_event {
            rewatch_paths.push(path.clone());
        }
    }
    (refresh_indices, rewatch_paths)
}

/// Second pass: build + send cache requests for the invalidated panels
/// (deduplicated, skipping any already in-flight).
fn dispatch_refresh_requests(app: &mut App, mut refresh_indices: Vec<usize>) {
    refresh_indices.sort_unstable();
    refresh_indices.dedup();
    for i in refresh_indices {
        let Some(ctx) = app.state.context.get(i) else { continue };
        if ctx.cache_in_flight {
            continue;
        }
        let panel = crate::app::panels::get_panel(&ctx.context_type);
        let built = panel.build_cache_request(ctx, &app.state);
        if let Some(request) = built {
            process_cache_request(request, app.cache_tx.clone());
            if let Some(ctx_mut) = app.state.context.get_mut(i) {
                ctx_mut.cache_in_flight = true;
            }
        }
    }
}

/// Third pass: re-watch files to pick up new inodes after atomic rename
/// (editors like vim/vscode save via rename, invalidating the inotify watch).
fn rewatch_changed_files(app: &mut App, rewatch_paths: Vec<String>) {
    if let Some(watcher) = app.file_watcher.as_mut() {
        for path in rewatch_paths {
            let _r = watcher.rewatch_file(&path);
        }
    }
}

/// Process file watcher events — delegates invalidation to modules via trait methods.
pub(super) fn process_watcher_events(app: &mut App) {
    let _guard = crate::profile!("app::watcher_events");
    let _fg = cp_base::flame!("watcher_events");
    // Collect events (immutable borrow on file_watcher released after this block)
    let events = {
        let Some(watcher) = app.file_watcher.as_ref() else { return };
        watcher.poll_events()
    };
    if events.is_empty() {
        return;
    }

    let (refresh_indices, rewatch_paths) = collect_invalidations(app, &events);
    dispatch_refresh_requests(app, refresh_indices);
    rewatch_changed_files(app, rewatch_paths);
}

/// What `classify_timer_panel` decided for one panel this tick.
enum TimerOutcome {
    /// Panel asked to auto-close.
    Suicide,
    /// Panel needs a cache refresh (initial load, dirty, or interval poll).
    Refresh(CacheRequest),
}

/// Decide one panel's timer fate: auto-close, refresh (initial / dirty /
/// interval), or nothing. Pure read of `app` — no mutation.
fn classify_timer_panel(app: &App, ctx: &crate::state::Entry, current_ms: u64) -> Option<TimerOutcome> {
    let panel = crate::app::panels::get_panel(&ctx.context_type);
    if panel.suicide(ctx, &app.state) {
        return Some(TimerOutcome::Suicide);
    }
    if ctx.cache_in_flight {
        return None;
    }
    // Case 1: Initial load — panel has no content yet.
    // Case 2: Explicitly dirty (watcher event, tool, self-invalidation).
    let needs_initial = ctx.cached_content.is_none() && ctx.context_type.needs_cache();
    if needs_initial || ctx.cache_deprecated {
        return panel.build_cache_request(ctx, &app.state).map(TimerOutcome::Refresh);
    }
    // Case 3: Timer-based polling (Tmux, Git, GitResult, GithubResult, Glob, Grep).
    let interval = panel.cache_refresh_interval_ms()?;
    let last = app.last_poll_ms.get(&ctx.id).copied().unwrap_or(0);
    if current_ms.saturating_sub(last) >= interval {
        return panel.build_cache_request(ctx, &app.state).map(TimerOutcome::Refresh);
    }
    None
}

/// Scan all panels: collect cache requests (initial load, dirty, or interval
/// poll) and the indices of panels that asked to auto-close (`suicide`).
fn collect_timer_requests(app: &App, current_ms: u64) -> TimerScan {
    let mut requests: Vec<(usize, CacheRequest)> = Vec::new();
    let mut suicide_indices: Vec<usize> = Vec::new();

    for (i, ctx) in app.state.context.iter().enumerate() {
        match classify_timer_panel(app, ctx, current_ms) {
            Some(TimerOutcome::Suicide) => suicide_indices.push(i),
            Some(TimerOutcome::Refresh(req)) => requests.push((i, req)),
            None => {}
        }
    }
    (requests, suicide_indices)
}

/// Remove panels that asked to auto-close (reverse order to preserve indices),
/// fixing `selected_context` and restoring scroll from the new selection.
fn remove_suicided_panels(app: &mut App, suicide_indices: &[usize]) {
    if suicide_indices.is_empty() {
        return;
    }
    // Save current scroll state before removals (entry might shift or disappear)
    if let Some(current) = app.state.context.get_mut(app.state.selected_context) {
        current.scroll_state.offset = app.state.scroll_offset;
        current.scroll_state.user_scrolled = app.state.flags.stream.user_scrolled;
    }
    for &i in suicide_indices.iter().rev() {
        // Fix selected_context if it pointed at or past the removed panel
        if app.state.selected_context >= app.state.context.len().saturating_sub(1) {
            app.state.selected_context = app.state.context.len().saturating_sub(2);
        } else if app.state.selected_context > i {
            app.state.selected_context = app.state.selected_context.saturating_sub(1);
        } else {
            // Selection is before the removed panel — index unaffected.
        }
        drop(app.state.context.remove(i));
    }
    // Restore scroll from the (possibly new) selected panel
    if let Some(incoming) = app.state.context.get(app.state.selected_context) {
        app.state.scroll_offset = incoming.scroll_state.offset;
        app.state.flags.stream.user_scrolled = incoming.scroll_state.user_scrolled;
    }
    app.state.flags.ui.dirty = true;
}

/// Check timer-based deprecation for glob, grep, tmux, git
/// Also handles initial population for newly created context elements.
///
/// Timer-based (interval) refreshes are restricted to **fixed panels and the
/// currently selected panel** to avoid wasting CPU on background refresh of
/// accumulated dynamic panels the user isn't looking at.  Dynamic panels still
/// get refreshed when:
///   - first created (`needs_initial`)
///   - explicitly deprecated by a file-watcher event
///   - the user selects them (becomes the selected panel)
pub(super) fn check_timer_based_deprecation(app: &mut App) {
    let current_ms = now_ms();

    // Only check every 100ms to avoid excessive work
    if current_ms.saturating_sub(app.last_timer_check_ms) < 100 {
        return;
    }
    let _guard = crate::profile!("app::timer_deprecation");
    app.last_timer_check_ms = current_ms;

    // Ensure all module-requested paths have active watchers
    sync_file_watchers(app);

    let (requests, suicide_indices) = collect_timer_requests(app, current_ms);

    // Mutable pass: send requests, mark in-flight, update poll timestamps
    for (i, request) in requests {
        process_cache_request(request, app.cache_tx.clone());
        if let Some(ctx) = app.state.context.get_mut(i) {
            ctx.cache_in_flight = true;
            let _r = app.last_poll_ms.insert(ctx.id.clone(), current_ms);
        }
    }

    remove_suicided_panels(app, &suicide_indices);
}

/// Gather every path all modules currently want watched, split into files and
/// dirs. `BTreeSet` (not `HashSet`) for deterministic iteration (dodges the
/// `iter_over_hash_type` lint).
fn collect_wanted_paths(app: &App) -> (std::collections::BTreeSet<String>, std::collections::BTreeSet<String>) {
    use cp_base::panels::WatchSpec;
    let mut wanted_files = std::collections::BTreeSet::new();
    let mut wanted_dirs = std::collections::BTreeSet::new();
    for module in &crate::modules::all_modules() {
        for spec in module.watch_paths(&app.state) {
            // `if let` (not exhaustive match) so WatchSpec stays #[non_exhaustive].
            if let WatchSpec::File(path) = spec {
                let _r = wanted_files.insert(path);
            } else if let WatchSpec::Dir(path) | WatchSpec::DirRecursive(path) = spec {
                let _r = wanted_dirs.insert(path);
            } else {
                // Future non_exhaustive variants: ignored by the watcher sync.
            }
        }
    }
    (wanted_files, wanted_dirs)
}

/// Unwatch any file/dir no longer wanted (frees kqueue FDs on macOS, where each
/// watched path costs one FD against the process limit).
fn remove_stale_watches(
    app: &mut App,
    wanted_files: &std::collections::BTreeSet<String>,
    wanted_dirs: &std::collections::BTreeSet<String>,
) {
    let Some(watcher) = app.file_watcher.as_mut() else { return };
    let stale_files: Vec<String> =
        app.watched_file_paths.iter().filter(|p| !wanted_files.contains(*p)).cloned().collect();
    for path in &stale_files {
        watcher.unwatch_file(path);
        let _r = app.watched_file_paths.remove(path);
    }
    let stale_dirs: Vec<String> = app.watched_dir_paths.iter().filter(|p| !wanted_dirs.contains(*p)).cloned().collect();
    for path in &stale_dirs {
        watcher.unwatch_dir(path);
        let _r = app.watched_dir_paths.remove(path);
    }
}

/// Add file watches for newly-wanted files not already watched.
fn add_file_watches(app: &mut App, wanted_files: std::collections::BTreeSet<String>) {
    let Some(watcher) = app.file_watcher.as_mut() else { return };
    for path in wanted_files {
        if !app.watched_file_paths.contains(&path) && watcher.watch_file(&path).is_ok() {
            let _r = app.watched_file_paths.insert(path);
        }
    }
}

/// Add dir watches for newly-wanted dirs, honoring each spec's recursive flag
/// (re-scans module specs since recursion isn't captured in the wanted set).
fn add_dir_watches(app: &mut App) {
    use cp_base::panels::WatchSpec;
    let Some(watcher) = app.file_watcher.as_mut() else { return };
    for module in &crate::modules::all_modules() {
        for spec in module.watch_paths(&app.state) {
            // File specs are handled by add_file_watches; here we only add dir
            // watches. `if let` (not exhaustive match) so WatchSpec stays #[non_exhaustive].
            if let WatchSpec::Dir(path) = spec {
                if !app.watched_dir_paths.contains(&path) && watcher.watch_dir(&path).is_ok() {
                    let _r = app.watched_dir_paths.insert(path);
                }
            } else if let WatchSpec::DirRecursive(path) = spec
                && !app.watched_dir_paths.contains(&path)
                && watcher.watch_dir_recursive(&path).is_ok()
            {
                let _r = app.watched_dir_paths.insert(path);
            } else {
                // File specs (handled by add_file_watches) + future non_exhaustive variants.
            }
        }
    }
}

/// Sync file watchers from all modules' `watch_paths()`.
/// Adds new watches and **removes stale ones** to prevent FD exhaustion.
/// On macOS, kqueue uses 1 FD per watched path — without cleanup, opening
/// hundreds of files over a session will hit the default 256 FD limit.
fn sync_file_watchers(app: &mut App) {
    if app.file_watcher.is_none() {
        return;
    }
    let (wanted_files, wanted_dirs) = collect_wanted_paths(app);
    remove_stale_watches(app, &wanted_files, &wanted_dirs);
    add_file_watches(app, wanted_files);
    add_dir_watches(app);
}
