use crate::state::{ContextType, State, estimate_tokens};

/// Get formatted overview of context for API
pub fn get_overview_context(state: &State) -> String {
    let mut output = String::new();

    // Calculate totals
    let total_tokens: usize = state.context.iter().map(|c| c.token_count).sum();
    let max_tokens = 100_000;
    let usage_pct = (total_tokens as f64 / max_tokens as f64 * 100.0).min(100.0);

    output.push_str(&format!("Context Usage: {}/{} tokens ({:.1}%)\n",
        format_tokens(total_tokens),
        format_tokens(max_tokens),
        usage_pct
    ));
    output.push_str("\n");

    // List all context elements
    output.push_str("Context Elements:\n");
    for ctx in &state.context {
        let type_name = match ctx.context_type {
            ContextType::Conversation => "conversation",
            ContextType::File => "file",
            ContextType::Tree => "tree",
            ContextType::Glob => "glob",
            ContextType::Grep => "grep",
            ContextType::Tmux => "tmux",
            ContextType::Todo => "todo",
            ContextType::Memory => "memory",
            ContextType::Overview => "overview",
            ContextType::Tools => "tools",
        };

        let details = match ctx.context_type {
            ContextType::File => ctx.file_path.as_deref().unwrap_or("").to_string(),
            ContextType::Glob => ctx.glob_pattern.as_deref().unwrap_or("").to_string(),
            ContextType::Grep => ctx.grep_pattern.as_deref().unwrap_or("").to_string(),
            ContextType::Tmux => {
                let pane = ctx.tmux_pane_id.as_deref().unwrap_or("?");
                let desc = ctx.tmux_description.as_deref().unwrap_or("");
                if desc.is_empty() { pane.to_string() } else { format!("{} ({})", pane, desc) }
            }
            _ => String::new(),
        };

        let line = if details.is_empty() {
            format!("  {} [{}] {} - {} tokens\n", ctx.id, type_name, ctx.name, format_tokens(ctx.token_count))
        } else {
            format!("  {} [{}] {} ({}) - {} tokens\n", ctx.id, type_name, ctx.name, details, format_tokens(ctx.token_count))
        };
        output.push_str(&line);
    }

    // Message counts
    let user_msgs = state.messages.iter().filter(|m| m.role == "user").count();
    let assistant_msgs = state.messages.iter().filter(|m| m.role == "assistant").count();
    let total_msgs = state.messages.len();

    output.push_str("\n");
    output.push_str(&format!("Messages: {} total ({} user, {} assistant)\n", total_msgs, user_msgs, assistant_msgs));

    // Todo summary
    let total_todos = state.todos.len();
    let done_todos = state.todos.iter().filter(|t| t.status == crate::state::TodoStatus::Done).count();
    let in_progress = state.todos.iter().filter(|t| t.status == crate::state::TodoStatus::InProgress).count();

    if total_todos > 0 {
        output.push_str(&format!("Todos: {}/{} done, {} in progress\n", done_todos, total_todos, in_progress));
    }

    // Memory summary
    let total_memories = state.memories.len();
    if total_memories > 0 {
        let critical = state.memories.iter().filter(|m| m.importance == crate::state::MemoryImportance::Critical).count();
        let high = state.memories.iter().filter(|m| m.importance == crate::state::MemoryImportance::High).count();
        output.push_str(&format!("Memories: {} total ({} critical, {} high)\n", total_memories, critical, high));
    }

    output.trim_end().to_string()
}

/// Refresh token count for the Overview context element
pub fn refresh_overview_context(state: &mut State) {
    let overview_content = get_overview_context(state);
    let token_count = estimate_tokens(&overview_content);

    for ctx in &mut state.context {
        if ctx.context_type == ContextType::Overview {
            ctx.token_count = token_count;
            break;
        }
    }
}

fn format_tokens(n: usize) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
