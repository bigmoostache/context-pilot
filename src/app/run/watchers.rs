use std::sync::mpsc::Receiver;

use crate::app::panels::now_ms;
use crate::infra::watcher::WatchEvent;
use crate::state::cache::{CacheRequest, CacheUpdate, process_cache_request};
use crate::state::{Kind, State};

use crate::app::App;

/// Set up file watchers from all modules' `watch_paths()`.
pub(super) fn setup_file_watchers(app: &mut App) {
    sync_file_watchers(app);
}

/// Sync `GhWatcher` with current `GithubResult` panels
pub(super) fn sync_gh_watches(app: &App) {
    let token = match &cp_mod_github::types::GithubState::get(&app.state).github_token {
        Some(t) => t.clone(),
        None => return,
    };
    let panels: Vec<(String, String, String)> = app
        .state
        .context
        .iter()
        .filter(|c| c.context_type.as_str() == Kind::GITHUB_RESULT)
        .filter_map(|c| c.get_meta_str("result_command").map(|cmd| (c.id.clone(), cmd.to_string(), token.clone())))
        .collect();
    app.gh_watcher.sync_watches(&panels);

    // Sync branch PR watch — poll for PRs on the current git branch
    let branch = cp_mod_git::types::GitState::get(&app.state).branch.as_deref();
    app.gh_watcher.sync_branch_pr(branch, Some(&token));
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
        .filter(|(_, ctx)| ctx.context_type.is_fixed())
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

/// Static version of `process_cache_updates` for use in wait module
fn process_cache_updates_static(state: &mut State, cache_rx: &Receiver<CacheUpdate>) {
    let _guard = crate::profile!("app::cache_updates");
    while let Ok(update) = cache_rx.try_recv() {
        // Handle Unchanged early — just clear in_flight, no panel dispatch needed
        if let CacheUpdate::Unchanged { ref context_id } = update {
            if let Some(ctx) = state.context.iter_mut().find(|c| c.id == *context_id) {
                ctx.cache_in_flight = false;
                ctx.cache_deprecated = false;
            }
            continue;
        }

        // ModuleSpecific: match by context_type
        if let CacheUpdate::ModuleSpecific { ref context_type, ref data, .. } = update {
            // Special case: BranchPrUpdate targets GithubState, not a panel
            if context_type.as_str() == Kind::GITHUB_RESULT && data.is::<cp_mod_github::watcher::BranchPrUpdate>() {
                if let CacheUpdate::ModuleSpecific { data: owned_data, .. } = update
                    && let Ok(pr_update) = owned_data.downcast::<cp_mod_github::watcher::BranchPrUpdate>()
                {
                    cp_mod_github::types::GithubState::get_mut(state).branch_pr = pr_update.pr_info;
                    state.flags.ui.dirty = true;
                }
                continue;
            }

            let idx = state.context.iter().position(|c| c.context_type == *context_type);
            let Some(idx) = idx else { continue };
            let mut ctx = state.context.remove(idx);
            let panel = crate::app::panels::get_panel(&ctx.context_type);
            let _changed = panel.apply_cache_update(update, &mut ctx, state);
            ctx.cache_in_flight = false;
            state.context.insert(idx, ctx);
            state.flags.ui.dirty = true;
            continue;
        }

        // Content: match by context_id
        let CacheUpdate::Content { ref context_id, .. } = update else { continue };
        let idx = state.context.iter().position(|c| c.id == *context_id);
        let Some(idx) = idx else { continue };
        let mut ctx = state.context.remove(idx);
        let panel = crate::app::panels::get_panel(&ctx.context_type);
        // apply_cache_update calls update_if_changed which sets last_refresh_ms on change
        let _changed = panel.apply_cache_update(update, &mut ctx, state);
        ctx.cache_in_flight = false;
        state.context.insert(idx, ctx);
        state.flags.ui.dirty = true;
    }
}

/// Process file watcher events — delegates invalidation to modules via trait methods.
pub(super) fn process_watcher_events(app: &mut App) {
    let _guard = crate::profile!("app::watcher_events");
    // Collect events (immutable borrow on file_watcher released after this block)
    let events = {
        let Some(watcher) = &app.file_watcher else { return };
        watcher.poll_events()
    };
    if events.is_empty() {
        return;
    }

    let modules = crate::modules::all_modules();

    // First pass: ask modules which panels to invalidate
    let mut refresh_indices = Vec::new();
    let mut rewatch_paths: Vec<String> = Vec::new();
    for event in &events {
        let (path, is_dir_event) = match event {
            WatchEvent::FileChanged(p) => (p, false),
            WatchEvent::DirChanged(p) => (p, true),
        };

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

        if !is_dir_event {
            rewatch_paths.push(path.clone());
        }
    }

    // Second pass: build and send requests (deduplicated, skip in-flight)
    refresh_indices.sort_unstable();
    refresh_indices.dedup();
    for i in refresh_indices {
        let Some(ctx) = app.state.context.get(i) else { continue };
        if ctx.cache_in_flight {
            continue;
        }
        let panel = crate::app::panels::get_panel(&ctx.context_type);
        let request = panel.build_cache_request(ctx, &app.state);
        if let Some(request) = request {
            process_cache_request(request, app.cache_tx.clone());
            if let Some(ctx_mut) = app.state.context.get_mut(i) {
                ctx_mut.cache_in_flight = true;
            }
        }
    }

    // Third pass: re-watch files to pick up new inodes after atomic rename
    // (editors like vim/vscode save via rename, which invalidates the inotify watch)
    if let Some(watcher) = &mut app.file_watcher {
        for path in rewatch_paths {
            let _r = watcher.rewatch_file(&path);
        }
    }
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

    let mut requests: Vec<(usize, CacheRequest)> = Vec::new();
    let mut suicide_indices: Vec<usize> = Vec::new();

    for (i, ctx) in app.state.context.iter().enumerate() {
        // Suicide check: ask panel if it wants to auto-close
        {
            let panel = crate::app::panels::get_panel(&ctx.context_type);
            if panel.suicide(ctx, &app.state) {
                suicide_indices.push(i);
                continue;
            }
        }

        if ctx.cache_in_flight {
            continue;
        }

        let panel = crate::app::panels::get_panel(&ctx.context_type);

        // Case 1: Initial load — panel has no content yet
        if ctx.cached_content.is_none() && ctx.context_type.needs_cache() {
            if let Some(req) = panel.build_cache_request(ctx, &app.state) {
                requests.push((i, req));
            }
            continue;
        }

        // Case 2: Explicitly dirty (watcher event, tool, self-invalidation)
        // ALL dirty panels refresh regardless of selection — no UI-gating.
        if ctx.cache_deprecated {
            if let Some(req) = panel.build_cache_request(ctx, &app.state) {
                requests.push((i, req));
            }
            continue;
        }

        // Case 3: Timer-based polling (Tmux, Git, GitResult, GithubResult, Glob, Grep)
        if let Some(interval) = panel.cache_refresh_interval_ms() {
            let last = app.last_poll_ms.get(&ctx.id).copied().unwrap_or(0);
            if current_ms.saturating_sub(last) >= interval
                && let Some(req) = panel.build_cache_request(ctx, &app.state)
            {
                requests.push((i, req));
            }
        }
    }

    // Mutable pass: send requests, mark in-flight, update poll timestamps
    for (i, request) in requests {
        process_cache_request(request, app.cache_tx.clone());
        if let Some(ctx) = app.state.context.get_mut(i) {
            ctx.cache_in_flight = true;
            let _r = app.last_poll_ms.insert(ctx.id.clone(), current_ms);
        }
    }

    // Mutable pass: remove suicided panels (reverse order to preserve indices)
    if !suicide_indices.is_empty() {
        for &i in suicide_indices.iter().rev() {
            // Fix selected_context if it pointed at or past the removed panel
            if app.state.selected_context >= app.state.context.len().saturating_sub(1) {
                app.state.selected_context = app.state.context.len().saturating_sub(2);
            } else if app.state.selected_context > i {
                app.state.selected_context = app.state.selected_context.saturating_sub(1);
            }
            drop(app.state.context.remove(i));
        }
        app.state.flags.ui.dirty = true;
    }
}

/// Sync file watchers from all modules' `watch_paths()`.
/// Called periodically to catch panels created during tool execution.
fn sync_file_watchers(app: &mut App) {
    use cp_base::panels::WatchSpec;
    let Some(watcher) = &mut app.file_watcher else { return };

    let modules = crate::modules::all_modules();
    for module in &modules {
        for spec in module.watch_paths(&app.state) {
            match spec {
                WatchSpec::File(path) => {
                    if !app.watched_file_paths.contains(&path) && watcher.watch_file(&path).is_ok() {
                        let _ = app.watched_file_paths.insert(path);
                    }
                }
                WatchSpec::Dir(path) => {
                    if !app.watched_dir_paths.contains(&path) && watcher.watch_dir(&path).is_ok() {
                        let _ = app.watched_dir_paths.insert(path);
                    }
                }
                WatchSpec::DirRecursive(path) => {
                    if !app.watched_dir_paths.contains(&path) && watcher.watch_dir_recursive(&path).is_ok() {
                        let _ = app.watched_dir_paths.insert(path);
                    }
                }
            }
        }
    }
}
