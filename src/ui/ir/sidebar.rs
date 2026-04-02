//! Sidebar IR builders — assemble [`Sidebar`] from application state.
//!
//! Extracts the data logic from `ui::sidebar::full` and `ui::sidebar::collapsed`
//! into pure functions returning IR types. No ratatui, no Frame.

use cp_render::frame::{HelpHint, PrCard, Sidebar, SidebarEntry, SidebarMode, TokenBar, TokenRow, TokenStats};
use cp_render::{ProgressSegment, Semantic};

use crate::state::{Kind, State};
use crate::ui::helpers::{format_number, spinner};
use cp_base::cast::Safe as _;

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
            ls.logs.iter().filter(|l| l.is_top_level()).count()
        }
        "callback" => cp_mod_callback::types::CallbackState::get(state).active_set.len(),
        "scratchpad" => cp_mod_scratchpad::types::ScratchpadState::get(state).scratchpad_cells.len(),
        "queue" => cp_mod_queue::types::QueueState::get(state).queued_calls.len(),
        "overview" => state.context.len().saturating_add(2),
        "tools" => state.tools.iter().filter(|t| t.enabled).count(),
        "chat-dashboard" => cp_mod_chat::types::ChatState::get(state).rooms.len(),
        _ => return None,
    };
    Some(count.to_string())
}

/// Build the sidebar region from application state.
#[must_use]
pub(crate) fn build_sidebar(state: &State) -> Sidebar {
    let mode = match state.sidebar_mode {
        cp_base::state::data::config::SidebarMode::Normal => SidebarMode::Normal,
        cp_base::state::data::config::SidebarMode::Collapsed => SidebarMode::Collapsed,
        cp_base::state::data::config::SidebarMode::Hidden => SidebarMode::Hidden,
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

    let entries = build_entries(state, matches!(mode, SidebarMode::Collapsed));
    let token_bar = Some(build_token_bar(state));
    let token_stats = build_token_stats(state);
    let pr_card = build_pr_card(state);
    let help_hints = build_help_hints();

    Sidebar { mode, entries, token_bar, token_stats, pr_card, help_hints }
}

// ── Entries ──────────────────────────────────────────────────────────

/// Build the context element entries list for the sidebar.
fn build_entries(state: &State, collapsed: bool) -> Vec<SidebarEntry> {
    let id_width = state.context.iter().map(|c| c.id.len()).max().unwrap_or(2);
    let spin = spinner(state.spinner_frame);

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
            label: "Conversation".into(),
            tokens: ctx.token_count.to_u32(),
            active: conv_idx == state.selected_context,
            frozen: false,
            badge: None,
            fixed: true,
        });
    }

    let lctx = EntryLabelCtx { state, id_width, spin };

    // Fixed + dynamic entries
    for &i in &sorted_indices {
        let Some(ctx) = state.context.get(i) else { continue };
        if ctx.context_type == Kind::new(Kind::CONVERSATION) {
            continue;
        }

        let is_loading = ctx.cached_content.is_none() && ctx.context_type.needs_cache();

        let is_fixed = ctx.context_type.is_fixed();

        let badge = if is_fixed {
            fixed_panel_badge(ctx.context_type.as_str(), state)
        } else if ctx.total_pages > 1 {
            Some(format!("{}/{}", ctx.current_page.saturating_add(1), ctx.total_pages))
        } else {
            None
        };

        let label = if collapsed { String::new() } else { build_entry_label(ctx, &lctx, is_loading) };

        entries.push(SidebarEntry {
            id: ctx.id.clone(),
            icon: ctx.context_type.icon(),
            label,
            tokens: ctx.token_count.to_u32(),
            active: i == state.selected_context,
            frozen: ctx.freeze_count > 0 && ctx.freeze_count < u8::MAX,
            badge,
            fixed: is_fixed,
        });
    }

    entries
}

/// Context for building sidebar entry labels.
struct EntryLabelCtx<'ctx> {
    /// Application state.
    state: &'ctx State,
    /// Alignment width for panel IDs.
    id_width: usize,
    /// Current spinner character.
    spin: &'ctx str,
}

/// Build the display label for a sidebar entry (full mode only).
fn build_entry_label(ctx: &cp_base::state::context::Entry, lctx: &EntryLabelCtx<'_>, is_loading: bool) -> String {
    let name = crate::ui::helpers::truncate_string(&ctx.name, 18);
    let shortcut = if ctx.context_type.is_fixed() {
        let badge = fixed_panel_badge(ctx.context_type.as_str(), lctx.state).unwrap_or_default();
        format!("{badge:>id_width$}", id_width = lctx.id_width)
    } else {
        format!("{:>width$}", &ctx.id, width = lctx.id_width)
    };

    let tokens_or_spinner = if is_loading {
        format!("{spin:>6}", spin = lctx.spin)
    } else if ctx.total_pages > 1 {
        format!("{}/{}", ctx.current_page.saturating_add(1), ctx.total_pages)
    } else {
        format_number(ctx.token_count)
    };

    format!("{shortcut} {name:<18}{tokens_or_spinner:>6}")
}

// ── Token bar ────────────────────────────────────────────────────────

/// Build the token usage progress bar.
fn build_token_bar(state: &State) -> TokenBar {
    let system_prompt_tokens = {
        let sp = cp_mod_prompt::seed::get_active_agent_content(state);
        crate::state::estimate_tokens(&sp).saturating_mul(2)
    };
    let tool_def_tokens = crate::modules::overview::context::estimate_tool_definitions_tokens(state);
    let panel_tokens: usize = state.context.iter().map(|c| c.token_count).sum();
    let total = system_prompt_tokens.saturating_add(tool_def_tokens).saturating_add(panel_tokens);
    let budget = state.effective_context_budget();

    // Cache hit / miss breakdown
    let mut hit = system_prompt_tokens.saturating_add(tool_def_tokens);
    let mut miss = 0usize;
    for ctx in &state.context {
        if ctx.panel_cache_hit {
            hit = hit.saturating_add(ctx.token_count);
        } else {
            miss = miss.saturating_add(ctx.token_count);
        }
    }

    let hit_pct = if budget > 0 { (hit.to_f64() / budget.to_f64() * 100.0).to_u8() } else { 0 };
    let miss_pct = if budget > 0 { (miss.to_f64() / budget.to_f64() * 100.0).to_u8() } else { 0 };

    let threshold = state.cleaning_threshold_tokens();

    TokenBar {
        segments: vec![
            ProgressSegment { percent: hit_pct, semantic: Semantic::Success, label: None },
            ProgressSegment { percent: miss_pct, semantic: Semantic::Warning, label: None },
        ],
        used: total.to_u32(),
        budget: budget.to_u32(),
        threshold: threshold.to_u32(),
    }
}

// ── Token stats ──────────────────────────────────────────────────────

/// Build the token statistics breakdown (cache hit / miss / output + costs).
fn build_token_stats(state: &State) -> Option<TokenStats> {
    /// Returns `Some(cost)` when ≥ $0.001, else `None`.
    fn token_cost_opt(tokens: usize, price: f32) -> Option<f64> {
        let c = State::token_cost(tokens, price);
        (c >= 0.001).then_some(c)
    }

    if state.cache_hit_tokens == 0 && state.cache_miss_tokens == 0 && state.total_output_tokens == 0 {
        return None;
    }

    let hit_price = state.cache_hit_price_per_mtok();
    let miss_price = state.cache_miss_price_per_mtok();
    let out_price = state.output_price_per_mtok();

    let mut rows = Vec::new();

    // tot row
    rows.push(TokenRow {
        label: "tot".into(),
        hit: state.cache_hit_tokens.to_u32(),
        miss: state.cache_miss_tokens.to_u32(),
        output: state.total_output_tokens.to_u32(),
        hit_cost: token_cost_opt(state.cache_hit_tokens, hit_price),
        miss_cost: token_cost_opt(state.cache_miss_tokens, miss_price),
        output_cost: token_cost_opt(state.total_output_tokens, out_price),
    });

    // strm row
    if state.stream_output_tokens > 0 || state.stream_cache_hit_tokens > 0 || state.stream_cache_miss_tokens > 0 {
        rows.push(TokenRow {
            label: "strm".into(),
            hit: state.stream_cache_hit_tokens.to_u32(),
            miss: state.stream_cache_miss_tokens.to_u32(),
            output: state.stream_output_tokens.to_u32(),
            hit_cost: token_cost_opt(state.stream_cache_hit_tokens, hit_price),
            miss_cost: token_cost_opt(state.stream_cache_miss_tokens, miss_price),
            output_cost: token_cost_opt(state.stream_output_tokens, out_price),
        });
    }

    // tick row
    if state.tick_output_tokens > 0 || state.tick_cache_hit_tokens > 0 || state.tick_cache_miss_tokens > 0 {
        rows.push(TokenRow {
            label: "tick".into(),
            hit: state.tick_cache_hit_tokens.to_u32(),
            miss: state.tick_cache_miss_tokens.to_u32(),
            output: state.tick_output_tokens.to_u32(),
            hit_cost: token_cost_opt(state.tick_cache_hit_tokens, hit_price),
            miss_cost: token_cost_opt(state.tick_cache_miss_tokens, miss_price),
            output_cost: token_cost_opt(state.tick_output_tokens, out_price),
        });
    }

    // Total cost
    let total_cost = State::token_cost(state.cache_hit_tokens, hit_price)
        + State::token_cost(state.cache_miss_tokens, miss_price)
        + State::token_cost(state.total_output_tokens, out_price);
    let total_cost_opt = (total_cost >= 0.001).then_some(total_cost);

    Some(TokenStats { rows, total_cost: total_cost_opt })
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
fn build_help_hints() -> Vec<HelpHint> {
    [
        ("Tab", "next panel"),
        ("↑↓", "scroll"),
        ("Ctrl+P", "commands"),
        ("Ctrl+H", "config"),
        ("Ctrl+V", "view"),
        ("Ctrl+Q", "quit"),
    ]
    .into_iter()
    .map(|(key, desc)| HelpHint { key: key.into(), description: desc.into() })
    .collect()
}
