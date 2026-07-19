use cp_base::cast::Safe as _;
use cp_base::state::data::model_helpers::ModelPricing as _;

use crate::modules::all_modules;
use crate::state::{State, estimate_tokens, get_context_type_meta};
use std::fmt::Write as _;

/// Estimate tokens for all enabled tool definitions as they'd appear in the API request.
pub(crate) fn estimate_tool_definitions_tokens(state: &State) -> usize {
    let mut total = 0usize;
    for tool in &state.tools {
        if !tool.enabled {
            continue;
        }
        // Each tool contributes: name, description, and parameter schema
        total = total.saturating_add(estimate_tokens(&tool.name));
        total = total.saturating_add(estimate_tokens(&tool.description));
        for param in &tool.params {
            total = total.saturating_add(estimate_tokens(&param.name));
            if let Some(desc) = &param.description {
                total = total.saturating_add(estimate_tokens(desc));
            }
            if let Some(vals) = &param.enum_values {
                for v in vals {
                    total = total.saturating_add(estimate_tokens(v));
                }
            }
            // JSON schema overhead per param (~10 tokens for type, required, etc.)
            total = total.saturating_add(10);
        }
        // Per-tool JSON overhead (~15 tokens for wrapping object, input_schema, etc.)
        total = total.saturating_add(15);
    }
    total
}

/// The agent's authoritative context-window occupancy as the
/// `(used, threshold, budget)` token triple.
///
/// This is the **single source of truth** for the figure rendered as the TUI
/// sidebar's `167K / 190K / 200K` line, the Statistics panel header, and the
/// `ContextUsage` oplog delta the bridge emits to the web HUD — so all three
/// show the *identical* number (T297: "the web meter must match ratatui
/// exactly"). `used` = the system prompt (counted twice, as it is sent in both
/// the `system` field and the seed re-injection) + every enabled tool
/// definition + the sum of all live context-panel token counts.
pub(crate) fn context_usage(state: &State) -> (usize, usize, usize) {
    let system_prompt = cp_mod_prompt::seed::get_active_agent_content(state);
    let system_prompt_tokens = estimate_tokens(&system_prompt).saturating_mul(2);
    let tool_def_tokens = estimate_tool_definitions_tokens(state);
    let panel_tokens: usize = state.context.iter().map(|c| c.token_count).sum();
    let used = system_prompt_tokens.saturating_add(tool_def_tokens).saturating_add(panel_tokens);
    (used, state.cleaning_threshold_tokens(), state.effective_context_budget())
}

/// The context-window occupancy split into its **cache `(hit, miss)`** halves —
/// the canonical companion to [`context_usage`], whose `used` equals
/// `hit + miss` exactly.
///
/// This is the **single source of truth** for the hit/miss decomposition the
/// TUI sidebar token bar renders as its green/amber segments (and lists in the
/// sidebar cache-stats table), and which the bridge emits to the web HUD so the
/// web meter can show the *identical* `Used (hit)` / `Used (miss)` breakdown
/// rather than re-deriving it (the same "must match ratatui exactly" discipline
/// as [`context_usage`], T297).
///
/// * **hit** = the stable, always-cached prefix (the system prompt counted
///   twice + every enabled tool definition) **plus** every live panel whose
///   `panel_cache_hit` flag is set — the bytes the provider served from cache.
/// * **miss** = every live panel whose cache flag is *not* set — the bytes
///   (re)sent uncached this turn.
///
/// The non-panel prefix is classed entirely as a hit because it is byte-stable
/// across turns and therefore always rides the cache breakpoint.
pub(crate) fn context_hit_miss(state: &State) -> (usize, usize) {
    let system_prompt = cp_mod_prompt::seed::get_active_agent_content(state);
    let system_prompt_tokens = estimate_tokens(&system_prompt).saturating_mul(2);
    let tool_def_tokens = estimate_tool_definitions_tokens(state);

    let mut hit = system_prompt_tokens.saturating_add(tool_def_tokens);
    let mut miss = 0usize;
    for ctx in &state.context {
        if ctx.panel_cache_hit {
            hit = hit.saturating_add(ctx.token_count);
        } else {
            miss = miss.saturating_add(ctx.token_count);
        }
    }
    (hit, miss)
}

/// The compact cache-status token for a panel's context line: hit tick, frozen
/// (`n/max`), or miss cross.
fn panel_hit_miss_token(ctx: &crate::state::Entry) -> String {
    if ctx.panel_cache_hit {
        "\u{2713}".to_owned()
    } else if ctx.freeze_count > 0 {
        let panel = crate::app::panels::get_panel(&ctx.context_type);
        format!("\u{2717} ({}/{})", ctx.freeze_count, panel.max_freezes())
    } else {
        "\u{2717}".to_owned()
    }
}

/// Append one panel's context line to `output` (id, type, optional details,
/// tokens, cost, cache status, freeze/miss counters, running accumulator).
fn write_panel_line(
    output: &mut String,
    ctx: &crate::state::Entry,
    accumulated: usize,
    modules: &[Box<dyn crate::modules::Module>],
) {
    let type_name =
        get_context_type_meta(ctx.context_type.as_str()).map_or(ctx.context_type.as_str(), |m| m.short_name);
    let details = modules.iter().find_map(|m| m.context_detail(ctx)).unwrap_or_default();
    let hit_miss = panel_hit_miss_token(ctx);
    let cost = format!("${:.2}", ctx.panel_total_cost);
    let freeze_info = if ctx.total_freezes > 0 { format!(" ❄{}", ctx.total_freezes) } else { String::new() };
    let miss_info =
        if ctx.total_cache_misses > 0 { format!(" miss:{}", ctx.total_cache_misses) } else { String::new() };

    if details.is_empty() {
        let _r = writeln!(
            output,
            "  {} {}: {} tokens {} {}{}{} (acc: {})",
            ctx.id, type_name, ctx.token_count, cost, hit_miss, freeze_info, miss_info, accumulated
        );
    } else {
        let _r = writeln!(
            output,
            "  {} {} ({}): {} tokens {} {}{}{} (acc: {})",
            ctx.id, type_name, details, ctx.token_count, cost, hit_miss, freeze_info, miss_info, accumulated
        );
    }
}

/// Generates the plain-text/markdown context content sent to the LLM.
/// This is separate from the TUI rendering (`overview_render.rs`).
pub(crate) fn generate_context_content(state: &State) -> String {
    // Estimate system prompt tokens
    let system_prompt = cp_mod_prompt::seed::get_active_agent_content(state);
    // The system prompt is sent twice: once in the system field, once as seed re-injection
    let system_prompt_tokens = estimate_tokens(&system_prompt).saturating_mul(2);

    // Estimate tool definition tokens
    let tool_def_tokens = estimate_tool_definitions_tokens(state);

    // Headline triple from the single canonical helper (shared with the sidebar
    // token bar and the web-HUD `ContextUsage` delta), so the figure never
    // drifts between surfaces (T297).
    let (total_tokens, threshold, budget) = context_usage(state);
    let usage_pct = (total_tokens.to_f64() / budget.to_f64() * 100.0).min(100.0);

    let mut output =
        format!("Context Usage: {total_tokens} / {threshold} threshold / {budget} budget ({usage_pct:.1}%)\n\n");

    let mut accumulated = 0usize;

    // --- Non-panel entries first: system prompt and tool definitions ---
    output.push_str("Context Elements:\n");

    accumulated = accumulated.saturating_add(system_prompt_tokens);
    let _r1 = writeln!(output, "  -- system-prompt (×2): {system_prompt_tokens} tokens (acc: {accumulated})");

    accumulated = accumulated.saturating_add(tool_def_tokens);
    let enabled_count = state.tools.iter().filter(|t| t.enabled).count();
    let _r2 = writeln!(
        output,
        "  -- tool-definitions ({enabled_count} enabled): {tool_def_tokens} tokens (acc: {accumulated})"
    );

    // --- Panels sorted by last_refresh_ms, with Conversation forced to end ---
    let mut sorted_contexts: Vec<&crate::state::Entry> = state.context.iter().collect();
    sorted_contexts.sort_by_key(|ctx| ctx.last_refresh_ms);

    // Partition: conversation ("chat") always last
    let (mut panels, mut conversation): (Vec<_>, Vec<_>) =
        sorted_contexts.into_iter().partition(|ctx| ctx.id != "chat");
    panels.append(&mut conversation);

    let modules = all_modules();

    for ctx in &panels {
        accumulated = accumulated.saturating_add(ctx.token_count);
        write_panel_line(&mut output, ctx, accumulated, &modules);
    }

    // Statistics
    let user_msgs = state.messages.iter().filter(|m| m.role == "user").count();
    let assistant_msgs = state.messages.iter().filter(|m| m.role == "assistant").count();
    let _r5 =
        write!(output, "\nMessages: {} ({} user, {} assistant)\n", state.messages.len(), user_msgs, assistant_msgs);

    // Module-specific overview sections (todos, memories, git status, etc.)
    for module in &modules {
        if let Some(section) = module.overview_context_section(state) {
            output.push_str(&section);
        }
    }

    output
}
