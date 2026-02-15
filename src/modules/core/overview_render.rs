use ratatui::prelude::*;

use crate::state::{ContextType, MemoryImportance, State, TodoStatus};
use crate::ui::{
    chars,
    helpers::{Cell, format_number, render_table},
    theme,
};

/// Horizontal separator line.
pub fn separator() -> Vec<Line<'static>> {
    vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            format!(" {}", chars::HORIZONTAL.repeat(60)),
            Style::default().fg(theme::border()),
        )]),
        Line::from(""),
    ]
}

/// Render the TOKEN USAGE section with progress bar.
pub fn render_token_usage(state: &State, base_style: Style) -> Vec<Line<'static>> {
    let mut text: Vec<Line> = Vec::new();

    let total_tokens: usize = state.context.iter().map(|c| c.token_count).sum();
    let budget = state.effective_context_budget();
    let threshold = state.cleaning_threshold_tokens();
    let usage_pct = (total_tokens as f64 / budget as f64 * 100.0).min(100.0);

    text.push(Line::from(vec![
        Span::styled(" ".to_string(), base_style),
        Span::styled("TOKEN USAGE".to_string(), Style::default().fg(theme::text_muted()).bold()),
    ]));
    text.push(Line::from(""));

    let current = format_number(total_tokens);
    let threshold_str = format_number(threshold);
    let budget_str = format_number(budget);
    let pct = format!("{:.1}%", usage_pct);

    text.push(Line::from(vec![
        Span::styled(" ".to_string(), base_style),
        Span::styled(current, Style::default().fg(theme::text()).bold()),
        Span::styled(" / ".to_string(), Style::default().fg(theme::text_muted())),
        Span::styled(threshold_str, Style::default().fg(theme::warning())),
        Span::styled(" / ".to_string(), Style::default().fg(theme::text_muted())),
        Span::styled(budget_str, Style::default().fg(theme::accent()).bold()),
        Span::styled(format!(" ({})", pct), Style::default().fg(theme::text_muted())),
    ]));

    // Progress bar with threshold marker
    let bar_width = 60usize;
    let threshold_pct = state.cleaning_threshold;
    let filled = ((usage_pct / 100.0) * bar_width as f64) as usize;
    let threshold_pos = (threshold_pct as f64 * bar_width as f64) as usize;

    let bar_color = if total_tokens >= threshold {
        theme::error()
    } else if total_tokens as f64 >= threshold as f64 * 0.9 {
        theme::warning()
    } else {
        theme::accent()
    };

    let mut bar_spans = vec![Span::styled(" ".to_string(), base_style)];
    for i in 0..bar_width {
        let char = if i == threshold_pos && threshold_pos < bar_width {
            "|" // Threshold marker
        } else if i < filled {
            chars::BLOCK_FULL
        } else {
            chars::BLOCK_LIGHT
        };

        let color = if i == threshold_pos {
            theme::warning()
        } else if i < filled {
            bar_color
        } else {
            theme::bg_elevated()
        };

        bar_spans.push(Span::styled(char, Style::default().fg(color)));
    }
    text.push(Line::from(bar_spans));

    text
}

/// Render the GIT STATUS section (branch, file changes table).
pub fn render_git_status(state: &State, base_style: Style) -> Vec<Line<'static>> {
    let mut text: Vec<Line> = Vec::new();

    if !state.git_is_repo {
        return text;
    }

    text.push(Line::from(vec![
        Span::styled(" ".to_string(), base_style),
        Span::styled("GIT STATUS".to_string(), Style::default().fg(theme::text_muted()).bold()),
    ]));
    text.push(Line::from(""));

    // Branch name
    if let Some(branch) = &state.git_branch {
        let branch_color = if branch.starts_with("detached:") { theme::warning() } else { theme::accent() };
        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled("Branch: ".to_string(), Style::default().fg(theme::text_secondary())),
            Span::styled(branch.clone(), Style::default().fg(branch_color).bold()),
        ]));
    }

    if state.git_file_changes.is_empty() {
        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled("Working tree clean".to_string(), Style::default().fg(theme::success())),
        ]));
    } else {
        text.push(Line::from(""));

        use crate::state::GitChangeType;

        let mut total_add: i32 = 0;
        let mut total_del: i32 = 0;

        let header = [
            Cell::new("File", Style::default()),
            Cell::right("+", Style::default()),
            Cell::right("-", Style::default()),
            Cell::right("Net", Style::default()),
        ];

        let rows: Vec<Vec<Cell>> = state
            .git_file_changes
            .iter()
            .map(|file| {
                total_add += file.additions;
                total_del += file.deletions;
                let net = file.additions - file.deletions;

                let (type_char, _type_color) = match file.change_type {
                    GitChangeType::Added => ("A", theme::success()),
                    GitChangeType::Untracked => ("U", theme::success()),
                    GitChangeType::Deleted => ("D", theme::error()),
                    GitChangeType::Modified => ("M", theme::warning()),
                    GitChangeType::Renamed => ("R", theme::accent()),
                };

                let display_path = if file.path.len() > 38 {
                    format!("{}...{}", type_char, &file.path[file.path.len() - 35..])
                } else {
                    format!("{} {}", type_char, file.path)
                };

                let net_color = if net > 0 {
                    theme::success()
                } else if net < 0 {
                    theme::error()
                } else {
                    theme::text_muted()
                };
                let net_str = if net > 0 { format!("+{}", net) } else { format!("{}", net) };

                vec![
                    Cell::new(display_path, Style::default().fg(theme::text())),
                    Cell::right(format!("+{}", file.additions), Style::default().fg(theme::success())),
                    Cell::right(format!("-{}", file.deletions), Style::default().fg(theme::error())),
                    Cell::right(net_str, Style::default().fg(net_color)),
                ]
            })
            .collect();

        let total_net = total_add - total_del;
        let total_net_color = if total_net > 0 {
            theme::success()
        } else if total_net < 0 {
            theme::error()
        } else {
            theme::text_muted()
        };
        let total_net_str = if total_net > 0 { format!("+{}", total_net) } else { format!("{}", total_net) };

        let footer = [
            Cell::new("Total", Style::default().fg(theme::text())),
            Cell::right(format!("+{}", total_add), Style::default().fg(theme::success())),
            Cell::right(format!("-{}", total_del), Style::default().fg(theme::error())),
            Cell::right(total_net_str, Style::default().fg(total_net_color)),
        ];

        text.extend(render_table(&header, &rows, Some(&footer), 1));
    }

    text
}

/// Render the CONTEXT ELEMENTS section.
pub fn render_context_elements(state: &State, base_style: Style) -> Vec<Line<'static>> {
    let mut text: Vec<Line> = Vec::new();

    text.push(Line::from(vec![
        Span::styled(" ".to_string(), base_style),
        Span::styled("CONTEXT ELEMENTS".to_string(), Style::default().fg(theme::text_muted()).bold()),
    ]));
    text.push(Line::from(""));

    let header = [
        Cell::new("ID", Style::default()),
        Cell::new("Type", Style::default()),
        Cell::right("Tokens", Style::default()),
        Cell::right("Cost", Style::default()),
        Cell::new("Hit", Style::default()),
        Cell::new("Refreshed", Style::default()),
        Cell::new("Details", Style::default()),
    ];

    // Sort by last_refresh_ms ascending (oldest first = same order LLM sees them)
    let mut sorted_contexts: Vec<&crate::state::ContextElement> = state.context.iter().collect();
    sorted_contexts.sort_by_key(|ctx| ctx.last_refresh_ms);

    let now_ms = crate::core::panels::now_ms();

    let rows: Vec<Vec<Cell>> = sorted_contexts
        .iter()
        .map(|ctx| {
            let type_name = match ctx.context_type {
                ContextType::System => "system",
                ContextType::Conversation => "conversation",
                ContextType::File => "file",
                ContextType::Tree => "tree",
                ContextType::Glob => "glob",
                ContextType::Grep => "grep",
                ContextType::Tmux => "tmux",
                ContextType::Todo => "todo",
                ContextType::Memory => "memory",
                ContextType::Overview => "overview",
                ContextType::Git => "git",
                ContextType::GitResult => "git-result",
                ContextType::GithubResult => "github-result",
                ContextType::Scratchpad => "scratchpad",
                ContextType::Library => "library",
                ContextType::Skill => "skill",
                ContextType::ConversationHistory => "chat-history",
                ContextType::Spine => "spine",
                ContextType::Logs => "logs",
            };

            let details = match ctx.context_type {
                ContextType::File => ctx.file_path.as_deref().unwrap_or("").to_string(),
                ContextType::Glob => ctx.glob_pattern.as_deref().unwrap_or("").to_string(),
                ContextType::Grep => ctx.grep_pattern.as_deref().unwrap_or("").to_string(),
                ContextType::Tmux => {
                    let pane = ctx.tmux_pane_id.as_deref().unwrap_or("?");
                    let desc = ctx.tmux_description.as_deref().unwrap_or("");
                    if desc.is_empty() { pane.to_string() } else { format!("{}: {}", pane, desc) }
                }
                ContextType::GitResult | ContextType::GithubResult => {
                    ctx.result_command.as_deref().unwrap_or("").to_string()
                }
                _ => String::new(),
            };

            let truncated_details = if details.len() > 30 {
                format!("{}...", &details[..details.floor_char_boundary(27)])
            } else {
                details
            };

            // Format refresh time as relative
            let refreshed = if ctx.last_refresh_ms < 1577836800000 {
                "—".to_string()
            } else if now_ms > ctx.last_refresh_ms {
                crate::ui::helpers::format_time_ago(now_ms - ctx.last_refresh_ms)
            } else {
                "now".to_string()
            };

            let icon = ctx.context_type.icon();
            let id_with_icon = format!("{}{}", icon, ctx.id);

            let cost_str = format!("${:.2}", ctx.panel_total_cost);
            let (hit_str, hit_color) =
                if ctx.panel_cache_hit { ("\u{2713}", theme::success()) } else { ("\u{2717}", theme::error()) };

            vec![
                Cell::new(id_with_icon, Style::default().fg(theme::accent_dim())),
                Cell::new(type_name, Style::default().fg(theme::text_secondary())),
                Cell::right(format_number(ctx.token_count), Style::default().fg(theme::accent())),
                Cell::right(cost_str, Style::default().fg(theme::text_muted())),
                Cell::new(hit_str, Style::default().fg(hit_color)),
                Cell::new(refreshed, Style::default().fg(theme::text_muted())),
                Cell::new(truncated_details, Style::default().fg(theme::text_muted())),
            ]
        })
        .collect();

    text.extend(render_table(&header, &rows, None, 1));

    text
}

/// Render the STATISTICS section.
pub fn render_statistics(state: &State, base_style: Style) -> Vec<Line<'static>> {
    let mut text: Vec<Line> = Vec::new();

    text.push(Line::from(vec![
        Span::styled(" ".to_string(), base_style),
        Span::styled("STATISTICS".to_string(), Style::default().fg(theme::text_muted()).bold()),
    ]));
    text.push(Line::from(""));

    let user_msgs = state.messages.iter().filter(|m| m.role == "user").count();
    let assistant_msgs = state.messages.iter().filter(|m| m.role == "assistant").count();
    let total_msgs = state.messages.len();

    text.push(Line::from(vec![
        Span::styled(" ".to_string(), base_style),
        Span::styled("Messages: ".to_string(), Style::default().fg(theme::text_secondary())),
        Span::styled(format!("{}", total_msgs), Style::default().fg(theme::text()).bold()),
        Span::styled(
            format!(" ({} user, {} assistant)", user_msgs, assistant_msgs),
            Style::default().fg(theme::text_muted()),
        ),
    ]));

    let total_todos = state.todos.len();
    if total_todos > 0 {
        let done_todos = state.todos.iter().filter(|t| t.status == TodoStatus::Done).count();
        let in_progress = state.todos.iter().filter(|t| t.status == TodoStatus::InProgress).count();
        let pending = total_todos - done_todos - in_progress;

        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled("Todos: ".to_string(), Style::default().fg(theme::text_secondary())),
            Span::styled(format!("{}/{}", done_todos, total_todos), Style::default().fg(theme::success()).bold()),
            Span::styled(" done".to_string(), Style::default().fg(theme::text_muted())),
            Span::styled(
                format!(", {} in progress, {} pending", in_progress, pending),
                Style::default().fg(theme::text_muted()),
            ),
        ]));
    }

    let total_memories = state.memories.len();
    if total_memories > 0 {
        let critical = state.memories.iter().filter(|m| m.importance == MemoryImportance::Critical).count();
        let high = state.memories.iter().filter(|m| m.importance == MemoryImportance::High).count();
        let medium = state.memories.iter().filter(|m| m.importance == MemoryImportance::Medium).count();
        let low = state.memories.iter().filter(|m| m.importance == MemoryImportance::Low).count();

        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled("Memories: ".to_string(), Style::default().fg(theme::text_secondary())),
            Span::styled(format!("{}", total_memories), Style::default().fg(theme::text()).bold()),
            Span::styled(
                format!(" ({} critical, {} high, {} medium, {} low)", critical, high, medium, low),
                Style::default().fg(theme::text_muted()),
            ),
        ]));
    }

    text
}

/// Render the AGENTS section (system prompts table).
pub fn render_seeds(state: &State, base_style: Style) -> Vec<Line<'static>> {
    let mut text: Vec<Line> = Vec::new();

    text.push(Line::from(vec![
        Span::styled(" ".to_string(), base_style),
        Span::styled("AGENTS".to_string(), Style::default().fg(theme::text_muted()).bold()),
        Span::styled(format!("  ({} available)", state.agents.len()), Style::default().fg(theme::text_muted())),
    ]));
    text.push(Line::from(""));

    let header = [
        Cell::new("ID", Style::default()),
        Cell::new("Name", Style::default()),
        Cell::new("Active", Style::default()),
        Cell::new("Description", Style::default()),
    ];

    let rows: Vec<Vec<Cell>> = state
        .agents
        .iter()
        .map(|agent| {
            let is_active = state.active_agent_id.as_deref() == Some(&agent.id);
            let (active_str, active_color) =
                if is_active { ("\u{2713}", theme::success()) } else { ("", theme::text_muted()) };

            let display_name = if agent.name.len() > 20 {
                format!("{}...", &agent.name[..agent.name.floor_char_boundary(17)])
            } else {
                agent.name.clone()
            };

            let display_desc = if agent.description.len() > 35 {
                format!("{}...", &agent.description[..agent.description.floor_char_boundary(32)])
            } else {
                agent.description.clone()
            };

            vec![
                Cell::new(&agent.id, Style::default().fg(theme::accent_dim())),
                Cell::new(display_name, Style::default().fg(theme::text())),
                Cell::new(active_str, Style::default().fg(active_color)),
                Cell::new(display_desc, Style::default().fg(theme::text_muted())),
            ]
        })
        .collect();

    text.extend(render_table(&header, &rows, None, 1));

    // Skills section
    if !state.skills.is_empty() {
        text.extend(separator());
        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled("SKILLS".to_string(), Style::default().fg(theme::text_muted()).bold()),
            Span::styled(
                format!("  ({} available, {} loaded)", state.skills.len(), state.loaded_skill_ids.len()),
                Style::default().fg(theme::text_muted()),
            ),
        ]));
        text.push(Line::from(""));

        let skill_header = [
            Cell::new("ID", Style::default()),
            Cell::new("Name", Style::default()),
            Cell::new("Loaded", Style::default()),
            Cell::new("Description", Style::default()),
        ];

        let skill_rows: Vec<Vec<Cell>> = state
            .skills
            .iter()
            .map(|skill| {
                let is_loaded = state.loaded_skill_ids.contains(&skill.id);
                let (loaded_str, loaded_color) =
                    if is_loaded { ("\u{2713}", theme::success()) } else { ("", theme::text_muted()) };
                vec![
                    Cell::new(&skill.id, Style::default().fg(theme::accent_dim())),
                    Cell::new(skill.name.clone(), Style::default().fg(theme::text())),
                    Cell::new(loaded_str, Style::default().fg(loaded_color)),
                    Cell::new(skill.description.clone(), Style::default().fg(theme::text_muted())),
                ]
            })
            .collect();

        text.extend(render_table(&skill_header, &skill_rows, None, 1));
    }

    // Commands section
    if !state.commands.is_empty() {
        text.extend(separator());
        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled("COMMANDS".to_string(), Style::default().fg(theme::text_muted()).bold()),
            Span::styled(format!("  ({} available)", state.commands.len()), Style::default().fg(theme::text_muted())),
        ]));
        text.push(Line::from(""));

        let cmd_header = [
            Cell::new("ID", Style::default()),
            Cell::new("Name", Style::default()),
            Cell::new("Description", Style::default()),
        ];

        let cmd_rows: Vec<Vec<Cell>> = state
            .commands
            .iter()
            .map(|cmd| {
                vec![
                    Cell::new(format!("/{}", cmd.id), Style::default().fg(theme::accent())),
                    Cell::new(cmd.name.clone(), Style::default().fg(theme::text())),
                    Cell::new(cmd.description.clone(), Style::default().fg(theme::text_muted())),
                ]
            })
            .collect();

        text.extend(render_table(&cmd_header, &cmd_rows, None, 1));
    }

    text
}

/// Render the PRESETS section.
pub fn render_presets(base_style: Style) -> Vec<Line<'static>> {
    let mut text: Vec<Line> = Vec::new();

    let presets = crate::modules::preset::tools::list_presets_with_info();
    if presets.is_empty() {
        return text;
    }

    text.push(Line::from(vec![
        Span::styled(" ".to_string(), base_style),
        Span::styled("PRESETS".to_string(), Style::default().fg(theme::text_muted()).bold()),
        Span::styled(format!("  ({} available)", presets.len()), Style::default().fg(theme::text_muted())),
    ]));
    text.push(Line::from(""));

    let header = [
        Cell::new("Name", Style::default()),
        Cell::new("Type", Style::default()),
        Cell::new("Description", Style::default()),
    ];

    let rows: Vec<Vec<Cell>> = presets
        .iter()
        .map(|p| {
            let (type_label, type_color) =
                if p.built_in { ("built-in", theme::accent_dim()) } else { ("custom", theme::success()) };

            let display_name = if p.name.len() > 25 {
                format!("{}...", &p.name[..p.name.floor_char_boundary(22)])
            } else {
                p.name.clone()
            };

            let display_desc = if p.description.len() > 35 {
                format!("{}...", &p.description[..p.description.floor_char_boundary(32)])
            } else {
                p.description.clone()
            };

            vec![
                Cell::new(display_name, Style::default().fg(theme::text())),
                Cell::new(type_label, Style::default().fg(type_color)),
                Cell::new(display_desc, Style::default().fg(theme::text_muted())),
            ]
        })
        .collect();

    text.extend(render_table(&header, &rows, None, 1));

    text
}

/// Render the TOOLS section (grouped by category).
pub fn render_tools(state: &State, base_style: Style) -> Vec<Line<'static>> {
    let mut text: Vec<Line> = Vec::new();

    let enabled_count = state.tools.iter().filter(|t| t.enabled).count();
    let disabled_count = state.tools.iter().filter(|t| !t.enabled).count();

    text.push(Line::from(vec![
        Span::styled(" ".to_string(), base_style),
        Span::styled("TOOLS".to_string(), Style::default().fg(theme::text_muted()).bold()),
        Span::styled(
            format!("  ({} enabled, {} disabled)", enabled_count, disabled_count),
            Style::default().fg(theme::text_muted()),
        ),
    ]));
    text.push(Line::from(""));

    use crate::constants::tool_categories;
    use crate::tool_defs::ToolCategory;

    for category in ToolCategory::all() {
        let category_tools: Vec<_> = state.tools.iter().filter(|t| &t.category == category).collect();

        if category_tools.is_empty() {
            continue;
        }

        let (cat_name, cat_desc) = match category {
            ToolCategory::File => ("FILE", tool_categories::file_desc()),
            ToolCategory::Tree => ("TREE", tool_categories::tree_desc()),
            ToolCategory::Console => ("CONSOLE", tool_categories::console_desc()),
            ToolCategory::Context => ("CONTEXT", tool_categories::context_desc()),
            ToolCategory::Skill => ("SKILL", "Manage knowledge skills"),
            ToolCategory::Agent => ("AGENT", "Manage system prompt agents"),
            ToolCategory::Command => ("COMMAND", "Manage input commands"),
            ToolCategory::System => ("SYSTEM", "System configuration and control"),
            ToolCategory::Todo => ("TODO", tool_categories::todo_desc()),
            ToolCategory::Memory => ("MEMORY", tool_categories::memory_desc()),
            ToolCategory::Git => ("GIT", tool_categories::git_desc()),
            ToolCategory::Github => ("GITHUB", "GitHub API operations via gh CLI"),
            ToolCategory::Scratchpad => ("SCRATCHPAD", tool_categories::scratchpad_desc()),
            ToolCategory::Spine => ("SPINE", "Auto-continuation and stream control"),
        };

        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled(cat_name.to_string(), Style::default().fg(theme::accent()).bold()),
            Span::styled(format!("  {}", cat_desc), Style::default().fg(theme::text_muted())),
        ]));

        let header = [
            Cell::new("Tool", Style::default()),
            Cell::new("On", Style::default()),
            Cell::new("Description", Style::default()),
        ];

        let rows: Vec<Vec<Cell>> = category_tools
            .iter()
            .map(|tool| {
                let (status_icon, status_color) =
                    if tool.enabled { ("\u{2713}", theme::success()) } else { ("\u{2717}", theme::error()) };

                vec![
                    Cell::new(&tool.id, Style::default().fg(theme::text())),
                    Cell::new(status_icon, Style::default().fg(status_color)),
                    Cell::new(&tool.short_desc, Style::default().fg(theme::text_muted())),
                ]
            })
            .collect();

        text.extend(render_table(&header, &rows, None, 2));
        text.push(Line::from(""));
    }

    text
}
