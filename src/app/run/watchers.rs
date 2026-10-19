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
/// of concurrent background agents on startup when many panels are persisted.
pub(super) fn schedule_initial_cache_refreshes(app: &mut App) {
    // Collect requests first (immutable borrow), then mark in-flight (mutable borrow).
    let requests: Vec<(usize, CacheRequest)> = app
        .state
        .agent()
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
        if let Some(ctx) = app.state.agent_mut().context.get_mut(i) {
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
    if let Some(ctx) = state.agent_mut().context.iter_mut().find(|c| c.id == context_id) {
        ctx.cache_in_flight = false;
        ctx.cache_deprecated = false;
    }
    true
}

/// Apply a `ModuleSpecific` cache update (matched by context type). On a type
/// mismatch, hands the update back via `Err` for the `Content` path.
fn apply_module_specific_update(state: &mut State, update: CacheUpdate) -> Result<(), CacheUpdate> {
    let Some(context_type) = update.module_specific_type().cloned() else { return Err(update) };
    let Some(idx) = state.agent().context.iter().position(|c| c.context_type == context_type) else { return Ok(()) };
    let mut ctx = state.agent_mut().context.remove(idx);
    let panel = crate::app::panels::get_panel(&ctx.context_type);
    let _changed = panel.apply_cache_update(update, &mut ctx, state);
    ctx.cache_in_flight = false;
    state.agent_mut().context.insert(idx, ctx);
    state.flags.ui.dirty = true;
    Ok(())
}

/// Apply a `Content` cache update (matched by context id).
fn apply_content_update(state: &mut State, update: CacheUpdate) {
    let Some(context_id) = update.content_context_id() else { return };
    let Some(idx) = state.agent().context.iter().position(|c| c.id == context_id) else { return };
    let mut ctx = state.agent_mut().context.remove(idx);
    let panel = crate::app::panels::get_panel(&ctx.context_type);
    // apply_cache_update calls update_if_changed which sets last_refresh_ms on change
    let _changed = panel.apply_cache_update(update, &mut ctx, state);
    ctx.cache_in_flight = false;
    state.agent_mut().context.insert(idx, ctx);
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
        // One perf row per panel kind (`<parent>.apply_<kind>`): attributes the
        // main-thread cost of each `apply_cache_update`. Interning only when monitoring.
        let _kind_guard = apply_kind_guard(state, &update);
        if let Err(leftover) = apply_module_specific_update(state, update) {
            apply_content_update(state, leftover);
        }
    }
}

/// Perf guard named `apply_<kind>` for the panel an update targets; `None` when
/// the F12 profiler is off or the target panel is gone.
fn apply_kind_guard(state: &State, update: &CacheUpdate) -> Option<crate::infra::profiler::ProfileGuard> {
    if !crate::ui::perf::PERF.enabled.load(std::sync::atomic::Ordering::Relaxed) {
        return None;
    }
    let kind = update.module_specific_type().cloned().or_else(|| {
        let id = update.content_context_id()?;
        state.agent().context.iter().find(|c| c.id == id).map(|c| c.context_type.clone())
    })?;
    Some(crate::profile!(crate::infra::profiler::intern(format!("apply_{}", kind.as_str()))))
}

/// Mark every panel a module claims for `path` as `cache_deprecated`, returning
/// the indices whose owning module wants an immediate refresh.
fn invalidate_matching_panels(app: &mut App, path: &str, is_dir_event: bool) -> Vec<usize> {
    let modules = crate::modules::all_modules();
    let mut refresh_indices = Vec::new();
    for (i, ctx) in app.state.agent_store.current_mut().context.iter_mut().enumerate() {
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
        let (path, is_dir_event) = cp_macros::deref_match!(event, {
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
        let Some(ctx) = app.state.agent().context.get(i) else { continue };
        if ctx.cache_in_flight {
            continue;
        }
        let panel = crate::app::panels::get_panel(&ctx.context_type);
        let built = panel.build_cache_request(ctx, &app.state);
        if let Some(request) = built {
            process_cache_request(request, app.cache_tx.clone());
            if let Some(ctx_mut) = app.state.agent_mut().context.get_mut(i) {
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
        let _g = crate::profile!("poll_events");
        let Some(watcher) = app.file_watcher.as_ref() else { return };
        watcher.poll_events()
    };
    if events.is_empty() {
        return;
    }

    let (refresh_indices, rewatch_paths) = {
        let _g = crate::profile!("invalidate");
        collect_invalidations(app, &events)
    };
    {
        let _g = crate::profile!("dispatch_refresh");
        dispatch_refresh_requests(app, refresh_indices);
    }
    let _g = crate::profile!("rewatch");
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
    let suicide = {
        let _g = crate::infra::profiler::dyn_guard("tr_suicide_", ctx.context_type.as_str());
        panel.suicide(ctx, &app.state)
    };
    if suicide {
        return Some(TimerOutcome::Suicide);
    }
    if ctx.cache_in_flight {
        return None;
    }
    let _g = crate::infra::profiler::dyn_guard("tr_req_", ctx.context_type.as_str());
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

    for (i, ctx) in app.state.agent().context.iter().enumerate() {
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
    let rt = app.state.agent_store.current_mut();
    if let Some(current) = rt.context.get_mut(rt.selected_context) {
        current.scroll_state.offset = rt.scroll_offset;
        current.scroll_state.user_scrolled = rt.stream.user_scrolled;
    }
    for &i in suicide_indices.iter().rev() {
        // Fix selected_context if it pointed at or past the removed panel
        if app.state.agent().selected_context >= app.state.agent().context.len().saturating_sub(1) {
            app.state.agent_mut().selected_context = app.state.agent_mut().context.len().saturating_sub(2);
        } else if app.state.agent().selected_context > i {
            app.state.agent_mut().selected_context = app.state.agent_mut().selected_context.saturating_sub(1);
        } else {
            // Selection is before the removed panel — index unaffected.
        }
        drop(app.state.agent_mut().context.remove(i));
    }
    // Restore scroll from the (possibly new) selected panel
    let after = app.state.agent_store.current_mut();
    if let Some(incoming) = after.context.get(after.selected_context) {
        after.scroll_offset = incoming.scroll_state.offset;
        after.stream.user_scrolled = incoming.scroll_state.user_scrolled;
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
    {
        let _g = crate::profile!("sync_file_watchers");
        sync_file_watchers(app);
    }

    let (requests, suicide_indices) = {
        let _g = crate::profile!("collect_timer_requests");
        collect_timer_requests(app, current_ms)
    };

    // Mutable pass: send requests, mark in-flight, update poll timestamps
    {
        let _g = crate::profile!("dispatch_requests");
        for (i, request) in requests {
            {
                let _s = crate::profile!("cache_submit");
                process_cache_request(request, app.cache_tx.clone());
            }
            if let Some(ctx) = app.state.agent_mut().context.get_mut(i) {
                ctx.cache_in_flight = true;
                let _r = app.last_poll_ms.insert(ctx.id.clone(), current_ms);
            }
        }
    }

    let _g = crate::profile!("remove_suicided");
    remove_suicided_panels(app, &suicide_indices);
}

/// One watch request in comparable form: `(kind, path)` with kind 0 = file,
/// 1 = dir, 2 = recursive dir (the recursion flag must survive the union).
type SpecKey = (u8, String);

/// Every watch the executing agent's modules want right now.
fn executing_agent_specs(app: &App) -> std::collections::BTreeSet<SpecKey> {
    use cp_base::panels::WatchSpec;
    let mut specs = std::collections::BTreeSet::new();
    for module in &crate::modules::all_modules() {
        for spec in module.watch_paths(&app.state) {
            // `if let` (not exhaustive match) so WatchSpec stays #[non_exhaustive].
            if let WatchSpec::File(path) = spec {
                let _r = specs.insert((0, path));
            } else if let WatchSpec::Dir(path) = spec {
                let _r = specs.insert((1, path));
            } else if let WatchSpec::DirRecursive(path) = spec {
                let _r = specs.insert((2, path));
            } else {
                // Future non_exhaustive variants: ignored by the watcher sync.
            }
        }
    }
    specs
}

/// Unwatch every file/dir no agent wants anymore.
fn remove_stale_watches(app: &mut App, wanted: &std::collections::BTreeSet<SpecKey>) {
    let Some(watcher) = app.file_watcher.as_mut() else { return };
    let stale_files: Vec<String> =
        app.watched_file_paths.iter().filter(|p| !wanted.contains(&(0, (*p).clone()))).cloned().collect();
    for path in &stale_files {
        watcher.unwatch_file(path);
        let _r = app.watched_file_paths.remove(path);
    }
    let stale_dirs: Vec<String> = app
        .watched_dir_paths
        .iter()
        .filter(|p| !wanted.contains(&(1, (*p).clone())) && !wanted.contains(&(2, (*p).clone())))
        .cloned()
        .collect();
    for path in &stale_dirs {
        watcher.unwatch_dir(path);
        let _r = app.watched_dir_paths.remove(path);
    }
}

/// Watch every wanted file/dir not already watched, honoring the recursion flag.
fn add_new_watches(app: &mut App, wanted: std::collections::BTreeSet<SpecKey>) {
    let Some(watcher) = app.file_watcher.as_mut() else { return };
    for (kind, path) in wanted {
        let (tracked, result) = if kind == 0 {
            if app.watched_file_paths.contains(&path) {
                continue;
            }
            (&mut app.watched_file_paths, watcher.watch_file(&path))
        } else {
            if app.watched_dir_paths.contains(&path) {
                continue;
            }
            let result = if kind == 2 { watcher.watch_dir_recursive(&path) } else { watcher.watch_dir(&path) };
            (&mut app.watched_dir_paths, result)
        };
        if result.is_ok() {
            let _r = tracked.insert(path);
        }
    }
}

/// Sync file watchers to the union of every agent's `watch_paths()`.
///
/// `watch_paths` only sees the executing agent, so each agent's last specs are
/// cached in `watch_specs_by_agent` and refreshed while it executes. Watching
/// the union (not just the focused agent) makes a focus switch free: on macOS
/// each `FSEvents` `watch`/`unwatch` stops, joins and respawns the runloop
/// thread, so swapping one agent's watch set for another's cost up to ~300ms.
/// Stale watches are still dropped (paths no agent wants) to bound FD use.
fn sync_file_watchers(app: &mut App) {
    if app.file_watcher.is_none() {
        return;
    }
    let specs = executing_agent_specs(app);
    let owner = app.state.executing_agent_id().map(str::to_owned);
    if app.watch_specs_by_agent.get(&owner) == Some(&specs) {
        return;
    }
    let _miss = crate::profile!("watch_sync_miss");
    let _r = app.watch_specs_by_agent.insert(owner, specs);
    let store = &app.state.agent_store;
    app.watch_specs_by_agent.retain(|id, _| id.as_deref().is_none_or(|i| store.contains(i)));
    let wanted: std::collections::BTreeSet<SpecKey> = app.watch_specs_by_agent.values().flatten().cloned().collect();
    remove_stale_watches(app, &wanted);
    add_new_watches(app, wanted);
}
