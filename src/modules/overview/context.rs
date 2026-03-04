use cp_base::cast::SafeCast;

use crate::modules::all_modules;
use crate::state::{State, estimate_tokens, get_context_type_meta};

/// Estimate tokens for all enabled tool definitions as they'd appear in the API request.
pub(crate) fn estimate_tool_definitions_tokens(state: &State) -> usize {
    let mut total = 0;
    for tool in &state.tools {
        if !tool.enabled {
            continue;
        }
        // Each tool contributes: name, description, and parameter schema
        total += estimate_tokens(&tool.name);
        total += estimate_tokens(&tool.description);
        for param in &tool.params {
            total += estimate_tokens(&param.name);
            if let Some(desc) = &param.description {
                total += estimate_tokens(desc);
            }
            if let Some(vals) = &param.enum_values {
                for v in vals {
                    total += estimate_tokens(v);
                }
            }
            // JSON schema overhead per param (~10 tokens for type, required, etc.)
            total += 10;
        }
        // Per-tool JSON overhead (~15 tokens for wrapping object, input_schema, etc.)
        total += 15;
    }
    total
}

/// Generates the plain-text/markdown context content sent to the LLM.
/// This is separate from the TUI rendering (`overview_render.rs`).
pub(crate) fn generate_context_content(state: &State) -> String {
    // Estimate system prompt tokens
    let system_prompt = cp_mod_prompt::seed::get_active_agent_content(state);
    // The system prompt is sent twice: once in the system field, once as seed re-injection
    let system_prompt_tokens = estimate_tokens(&system_prompt) * 2;

    // Estimate tool definition tokens
    let tool_def_tokens = estimate_tool_definitions_tokens(state);

    let panel_tokens: usize = state.context.iter().map(|c| c.token_count).sum();
    let total_tokens = system_prompt_tokens + tool_def_tokens + panel_tokens;
    let budget = state.effective_context_budget();
    let threshold = state.cleaning_threshold_tokens();
    let usage_pct = (total_tokens.to_f64() / budget.to_f64() * 100.0).min(100.0);

    let mut output =
        format!("Context Usage: {total_tokens} / {threshold} threshold / {budget} budget ({usage_pct:.1}%)\n\n");

    let mut accumulated = 0usize;

    // --- Non-panel entries first: system prompt and tool definitions ---
    output.push_str("Context Elements:\n");

    accumulated += system_prompt_tokens;
    output.push_str(&format!("  -- system-prompt (×2): {system_prompt_tokens} tokens (acc: {accumulated})\n"));

    accumulated += tool_def_tokens;
    let enabled_count = state.tools.iter().filter(|t| t.enabled).count();
    output.push_str(&format!(
        "  -- tool-definitions ({enabled_count} enabled): {tool_def_tokens} tokens (acc: {accumulated})\n"
    ));

    // --- Panels sorted by last_refresh_ms, with Conversation forced to end ---
    let mut sorted_contexts: Vec<&crate::state::ContextElement> = state.context.iter().collect();
    sorted_contexts.sort_by_key(|ctx| ctx.last_refresh_ms);

    // Partition: conversation ("chat") always last
    let (mut panels, mut conversation): (Vec<_>, Vec<_>) =
        sorted_contexts.into_iter().partition(|ctx| ctx.id != "chat");
    panels.append(&mut conversation);

    let modules = all_modules();

    for ctx in &panels {
        let type_name =
            get_context_type_meta(ctx.context_type.as_str()).map_or(ctx.context_type.as_str(), |m| m.short_name);

        let details = modules.iter().find_map(|m| m.context_detail(ctx)).unwrap_or_default();

        let hit_miss = if ctx.panel_cache_hit { "\u{2713}" } else { "\u{2717}" };
        let cost = format!("${:.2}", ctx.panel_total_cost);

        accumulated += ctx.token_count;

        if details.is_empty() {
            output.push_str(&format!(
                "  {} {}: {} tokens {} {} (acc: {})\n",
                ctx.id, type_name, ctx.token_count, cost, hit_miss, accumulated
            ));
        } else {
            output.push_str(&format!(
                "  {} {} ({}): {} tokens {} {} (acc: {})\n",
                ctx.id, type_name, details, ctx.token_count, cost, hit_miss, accumulated
            ));
        }
    }

    // Statistics
    let user_msgs = state.messages.iter().filter(|m| m.role == "user").count();
    let assistant_msgs = state.messages.iter().filter(|m| m.role == "assistant").count();
    output.push_str(&format!(
        "\nMessages: {} ({} user, {} assistant)\n",
        state.messages.len(),
        user_msgs,
        assistant_msgs
    ));

    // Module-specific overview sections (todos, memories, git status, etc.)
    for module in &modules {
        if let Some(section) = module.overview_context_section(state) {
            output.push_str(&section);
        }
    }

    output
}
