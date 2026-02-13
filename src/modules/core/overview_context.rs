use crate::state::{ContextType, State, TodoStatus};

/// Generates the plain-text/markdown context content sent to the LLM.
/// This is separate from the TUI rendering (overview_render.rs).
pub fn generate_context_content(state: &State) -> String {
    let total_tokens: usize = state.context.iter().map(|c| c.token_count).sum();
    let budget = state.effective_context_budget();
    let threshold = state.cleaning_threshold_tokens();
    let usage_pct = (total_tokens as f64 / budget as f64 * 100.0).min(100.0);

    let mut output = format!("Context Usage: {} / {} threshold / {} budget ({:.1}%)\n\n",
        total_tokens, threshold, budget, usage_pct);

    output.push_str("Context Elements:\n");

    // Sort by last_refresh_ms ascending (oldest first = same order LLM sees them)
    let mut sorted_contexts: Vec<&crate::state::ContextElement> = state.context.iter().collect();
    sorted_contexts.sort_by_key(|ctx| ctx.last_refresh_ms);

    for ctx in &sorted_contexts {
        let type_name = match ctx.context_type {
            ContextType::System => "seed",
            ContextType::Conversation => "chat",
            ContextType::File => "file",
            ContextType::Tree => "tree",
            ContextType::Glob => "glob",
            ContextType::Grep => "grep",
            ContextType::Tmux => "tmux",
            ContextType::Todo => "wip",
            ContextType::Memory => "memories",
            ContextType::Overview => "world",
            ContextType::Git => "changes",
            ContextType::GitResult => "git-cmd",
            ContextType::GithubResult => "gh-cmd",
            ContextType::Scratchpad => "scratch",
            ContextType::Library => "library",
            ContextType::Skill => "skill",
            ContextType::ConversationHistory => "history",
            ContextType::Spine => "spine",
            ContextType::Logs => "logs",
        };

        let details = match ctx.context_type {
            ContextType::File => ctx.file_path.as_deref().unwrap_or("").to_string(),
            ContextType::Glob => ctx.glob_pattern.as_deref().unwrap_or("").to_string(),
            ContextType::Grep => ctx.grep_pattern.as_deref().unwrap_or("").to_string(),
            ContextType::Tmux => ctx.tmux_pane_id.as_deref().unwrap_or("").to_string(),
            ContextType::GitResult | ContextType::GithubResult => {
                ctx.result_command.as_deref().unwrap_or("").to_string()
            }
            _ => String::new(),
        };

        if details.is_empty() {
            output.push_str(&format!("  {} {}: {} tokens\n", ctx.id, type_name, ctx.token_count));
        } else {
            output.push_str(&format!("  {} {} ({}): {} tokens\n", ctx.id, type_name, details, ctx.token_count));
        }
    }

    // Statistics
    let user_msgs = state.messages.iter().filter(|m| m.role == "user").count();
    let assistant_msgs = state.messages.iter().filter(|m| m.role == "assistant").count();
    output.push_str(&format!("\nMessages: {} ({} user, {} assistant)\n",
        state.messages.len(), user_msgs, assistant_msgs));

    if !state.todos.is_empty() {
        let done = state.todos.iter().filter(|t| t.status == TodoStatus::Done).count();
        output.push_str(&format!("Todos: {}/{} done\n", done, state.todos.len()));
    }

    if !state.memories.is_empty() {
        output.push_str(&format!("Memories: {}\n", state.memories.len()));
    }

    // Presets table for LLM
    let presets = crate::modules::preset::tools::list_presets_with_info();
    if !presets.is_empty() {
        output.push_str("\nPresets:\n\n");
        output.push_str("| Name | Type | Description |\n");
        output.push_str("|------|------|-------------|\n");
        for p in &presets {
            let ptype = if p.built_in { "built-in" } else { "custom" };
            output.push_str(&format!("| {} | {} | {} |\n", p.name, ptype, p.description));
        }
    }

    // Git status for LLM (as markdown table)
    if state.git_is_repo {
        if let Some(branch) = &state.git_branch {
            output.push_str(&format!("\nGit Branch: {}\n", branch));
        }

        if state.git_file_changes.is_empty() {
            output.push_str("Git Status: Working tree clean\n");
        } else {
            output.push_str("\nGit Changes:\n\n");
            output.push_str("| File | + | - | Net |\n");
            output.push_str("|------|---|---|-----|\n");

            let mut total_add: i32 = 0;
            let mut total_del: i32 = 0;

            for file in &state.git_file_changes {
                total_add += file.additions;
                total_del += file.deletions;
                let net = file.additions - file.deletions;
                let net_str = if net >= 0 { format!("+{}", net) } else { format!("{}", net) };
                output.push_str(&format!("| {} | +{} | -{} | {} |\n",
                    file.path, file.additions, file.deletions, net_str));
            }

            let total_net = total_add - total_del;
            let total_net_str = if total_net >= 0 { format!("+{}", total_net) } else { format!("{}", total_net) };
            output.push_str(&format!("| **Total** | **+{}** | **-{}** | **{}** |\n",
                total_add, total_del, total_net_str));
        }
    }

    // Tools table (markdown format for LLM)
    let enabled_count = state.tools.iter().filter(|t| t.enabled).count();
    let disabled_count = state.tools.iter().filter(|t| !t.enabled).count();
    output.push_str(&format!("\nTools ({} enabled, {} disabled):\n\n", enabled_count, disabled_count));
    output.push_str("| Category | Tool | Status | Description |\n");
    output.push_str("|----------|------|--------|-------------|\n");
    for tool in &state.tools {
        let status = if tool.enabled { "✓" } else { "✗" };
        output.push_str(&format!("| {} | {} | {} | {} |\n", tool.category.short_name(), tool.id, status, tool.short_desc));
    }

    output
}
