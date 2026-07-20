//! Sidebar IR builders — assemble [`Sidebar`] from application state.
//!
//! Extracts the sidebar data logic into pure functions returning IR types.
//! No ratatui, no Frame.

use cp_render::frame::{HelpHint, PrCard, Sidebar, SidebarEntry, SidebarMode, TokenBar, TokenRow, TokenStats};
use cp_render::{ProgressSegment, Semantic};

use crate::state::{Kind, State};
use crate::ui::helpers::spinner;
use cp_base::cast::Safe as _;
use cp_base::cast::float_math;

/// Returns a count badge for fixed panels, replacing the panel ID (P1, P2, etc.)
/// with a meaningful number that reflects the panel's content.
fn fixed_panel_badge(ctx_type: &str, state: &State) -> Option<String> {
    let count = match ctx_type {
        "todo" => {
            let ts = cp_mod_todo::types::TodoState::get(state);
            ts.todos.iter().filter(|t| !matches!(t.status, cp_mod_todo::types::TodoStatus::Done)).count()
        }
        "library" => cp_mod_prompt::types::PromptState::get(state).loaded_skill_ids.len(),
        "tree" => cp_mod_tree::types::TreeState::get(state).open_folders.len(),
        "memory" => cp_mod_memory::types::MemoryState::get(state).memories.len(),
        "spine" => cp_mod_spine::types::SpineState::unprocessed_notifications(state).len(),
        "logs" => {
            let ls = cp_mod_logs::types::LogsState::get(state);
            ls.logs.len()
        }
        "callback" => cp_mod_callback::types::CallbackState::get(state).definitions.len(),
        "scratchpad" => cp_mod_scratchpad::types::ScratchpadState::get(state).scratchpad_cells.len(),
        "queue" => cp_mod_queue::types::QueueState::get(state).queued_calls.len(),
        "overview" => state.context.len().saturating_add(2),
        "tools" => state.tools.iter().filter(|t| t.enabled).count(),
        _ => return None,
    };
    Some(count.to_string())
}

/// Build the sidebar region from application state.
#[must_use]
pub(crate) fn build_sidebar(state: &State) -> Sidebar {
    let mode = if state.view_mode == cp_base::state::data::config::ViewMode::Threads {
        SidebarMode::Hidden
    } else {
        SidebarMode::Normal
    };

    if matches!(mode, SidebarMode::Hidden) {
        return Sidebar {
            mode,
            entries: Vec::new(),
            token_bar: None,
            token_stats: None,
            pr_card: None,
            help_hints: Vec::new(),
        };
    }

    let entries = build_entries(state);
    let token_bar = Some(build_token_bar(state));
    let token_stats = build_token_stats(state);
    let pr_card = build_pr_card(state);
    let help_hints = build_help_hints(state);

    Sidebar { mode, entries, token_bar, token_stats, pr_card, help_hints }
}

// ── Entries ──────────────────────────────────────────────────────────

/// Build the context element entries list for the sidebar.
fn build_entries(state: &State) -> Vec<SidebarEntry> {
    // Sort by panel ID numerically
    let mut sorted_indices: Vec<usize> = (0..state.context.len()).collect();
    sorted_indices.sort_by(|&a, &b| {
        let id_a = state
            .context
            .get(a)
            .and_then(|c| c.id.strip_prefix('P'))
            .and_then(|n| n.parse::<usize>().ok())
            .unwrap_or(usize::MAX);
        let id_b = state
            .context
            .get(b)
            .and_then(|c| c.id.strip_prefix('P'))
            .and_then(|n| n.parse::<usize>().ok())
            .unwrap_or(usize::MAX);
        id_a.cmp(&id_b)
    });

    let mut entries = Vec::new();

    // Conversation entry first
    if let Some(conv_idx) = state.context.iter().position(|c| c.context_type == Kind::new(Kind::CONVERSATION))
        && let Some(ctx) = state.context.get(conv_idx)
    {
        entries.push(SidebarEntry {
            id: String::new(),
            icon: ctx.context_type.icon(),
            shortcut: String::new(),
            label: "Conversation".to_owned(),
            tokens: ctx.token_count.to_u32(),
            active: conv_idx == state.selected_context,
            frozen: false,
            badge: None,
            fixed: true,
        });
    }

    // Fixed + dynamic entries
    for &i in &sorted_indices {
        let Some(ctx) = state.context.get(i) else { continue };
        if ctx.context_type == Kind::new(Kind::CONVERSATION) {
            continue;
        }
        entries.push(context_to_entry(ctx, state, i == state.selected_context));
    }

    entries
}

/// Build one sidebar entry from a context element (non-conversation).
/// Resolves fixed-panel badges/shortcuts, running-console spinner, and the
/// loading-spinner label suffix.
fn context_to_entry(ctx: &crate::state::Entry, state: &State, active: bool) -> SidebarEntry {
    let is_loading = ctx.cached_content.is_none() && ctx.context_type.needs_cache();
    let is_fixed = ctx.context_type.is_fixed();
    let is_console = ctx.context_type.as_str() == "console";
    let is_running_console = is_console && ctx.get_meta_str("console_status").is_some_and(|s| s.starts_with("running"));

    let badge = if is_fixed {
        fixed_panel_badge(ctx.context_type.as_str(), state)
    } else if ctx.total_pages > 1 {
        Some(format!("{}/{}", ctx.current_page.saturating_add(1), ctx.total_pages))
    } else {
        None
    };

    let shortcut = if is_fixed {
        // Don't show "0" — empty string hides the badge
        fixed_panel_badge(ctx.context_type.as_str(), state).filter(|s| s != "0").unwrap_or_default()
    } else if is_running_console {
        spinner().to_owned()
    } else {
        ctx.id.clone()
    };

    let label = {
        let name = crate::ui::helpers::truncate_string(&ctx.name, 18);
        if is_loading { format!("{name} {spin}", spin = spinner()) } else { name }
    };

    SidebarEntry {
        id: ctx.id.clone(),
        icon: ctx.context_type.icon(),
        shortcut,
        label,
        tokens: ctx.token_count.to_u32(),
        active,
        frozen: ctx.freeze_count > 0 && ctx.freeze_count < u8::MAX,
        badge,
        fixed: is_fixed,
    }
}

// ── Token bar ────────────────────────────────────────────────────────

/// Build the token usage progress bar.
fn build_token_bar(state: &State) -> TokenBar {
    // The headline `used / threshold / budget` triple comes from the single
    // canonical helper (shared with the Statistics panel and the `ContextUsage`
    // delta the web HUD reads), so the three surfaces never drift (T297).
    let (total, threshold, budget) = crate::modules::overview::context::context_usage(state);

    // Cache hit / miss breakdown from the SAME canonical helper the web HUD
    // emit reads (T297: the web `Used (hit)` / `Used (miss)` split must be
    // byte-identical to this bar's green/amber segments — one definition, no
    // re-derivation that could drift).
    let (hit, miss) = crate::modules::overview::context::context_hit_miss(state);

    let hit_pct = if budget > 0 { float_math::percent(hit.to_f64(), budget.to_f64()).to_u8() } else { 0 };
    let miss_pct = if budget > 0 { float_math::percent(miss.to_f64(), budget.to_f64()).to_u8() } else { 0 };

    TokenBar {
        segments: vec![
            ProgressSegment { percent: hit_pct, semantic: Semantic::Success, label: None },
            ProgressSegment { percent: miss_pct, semantic: Semantic::Warning, label: None },
        ],
        used: total.to_u32(),
        budget: budget.to_u32(),
        threshold: threshold.to_u32(),
        streaming: state.flags.stream.phase.is_streaming(),
    }
}

// ── Token stats ──────────────────────────────────────────────────────

/// Build the token statistics breakdown (cache hit / miss / output + costs).
fn build_token_stats(state: &State) -> Option<TokenStats> {
    /// Returns `Some(cost)` when ≥ $0.001, else `None`.
    fn cost_opt(c: f64) -> Option<f64> {
        (c >= 0.001).then_some(c)
    }

    if state.cache_hit_tokens == 0 && state.cache_miss_tokens == 0 && state.total_output_tokens == 0 {
        return None;
    }

    let mut rows = Vec::new();

    // tot row — costs are frozen at consumption-time pricing (not recomputed here).
    rows.push(TokenRow {
        label: "tot".into(),
        hit: state.cache_hit_tokens.to_u32(),
        miss: state.cache_miss_tokens.to_u32(),
        output: state.total_output_tokens.to_u32(),
        hit_cost: cost_opt(state.cost_hit_usd),
        miss_cost: cost_opt(state.cost_miss_usd),
        output_cost: cost_opt(state.cost_output_usd),
    });

    // strm row
    if state.stream_output_tokens > 0 || state.stream_cache_hit_tokens > 0 || state.stream_cache_miss_tokens > 0 {
        rows.push(TokenRow {
            label: "strm".into(),
            hit: state.stream_cache_hit_tokens.to_u32(),
            miss: state.stream_cache_miss_tokens.to_u32(),
            output: state.stream_output_tokens.to_u32(),
            hit_cost: cost_opt(state.stream_cost_hit_usd),
            miss_cost: cost_opt(state.stream_cost_miss_usd),
            output_cost: cost_opt(state.stream_cost_output_usd),
        });
    }

    // tick row
    if state.tick_output_tokens > 0 || state.tick_cache_hit_tokens > 0 || state.tick_cache_miss_tokens > 0 {
        rows.push(TokenRow {
            label: "tick".into(),
            hit: state.tick_cache_hit_tokens.to_u32(),
            miss: state.tick_cache_miss_tokens.to_u32(),
            output: state.tick_output_tokens.to_u32(),
            hit_cost: cost_opt(state.tick_cost_hit_usd),
            miss_cost: cost_opt(state.tick_cost_miss_usd),
            output_cost: cost_opt(state.tick_cost_output_usd),
        });
    }

    // Total cost (sum of frozen legs)
    let total_cost = float_math::sum3(state.cost_hit_usd, state.cost_miss_usd, state.cost_output_usd);
    let total_cost_opt = (total_cost >= 0.001f64).then_some(total_cost);

    Some(TokenStats {
        rows,
        uncached_input: state.tick_uncached_input_tokens.to_u32(),
        alive_breakpoints: state.tick_alive_breakpoints.to_u32(),
        alive_bp_positions: state.tick_alive_bp_positions.clone(),
        total_cost: total_cost_opt,
    })
}

// ── PR card ──────────────────────────────────────────────────────────

/// Build the PR summary card from git state, if a branch PR exists.
fn build_pr_card(state: &State) -> Option<PrCard> {
    let pr = cp_mod_github::types::GithubState::get(state).branch_pr.as_ref()?;

    Some(PrCard {
        number: pr.number.to_u32(),
        title: pr.title.clone(),
        additions: pr.additions.unwrap_or(0).to_u32(),
        deletions: pr.deletions.unwrap_or(0).to_u32(),
        review_status: pr.review_decision.clone(),
        checks_status: pr.checks_status.clone(),
    })
}

// ── Help hints ───────────────────────────────────────────────────────

/// Build keyboard shortcut help hints for the sidebar.
fn build_help_hints(state: &State) -> Vec<HelpHint> {
    let copy_flash = {
        let ms = state.flags.overlays.copied_flash_ms;
        ms > 0 && cp_base::panels::now_ms().saturating_sub(ms) < 2_000
    };

    [
        ("Tab", "next panel"),
        ("\u{2191}\u{2193}", "scroll"),
        ("Ctrl+U/D", "history"),
        ("Ctrl+C", if copy_flash { "copied \u{2713}" } else { "copy panel" }),
        ("Ctrl+I", "search index"),
        ("Ctrl+P", "commands"),
        ("Ctrl+H", "config"),
        ("Ctrl+V", "view"),
        ("Ctrl+Q", "quit"),
    ]
    .into_iter()
    .map(|(key, desc)| HelpHint { key: key.into(), description: desc.into() })
    .collect()
}
