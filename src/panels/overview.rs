use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;

use super::{ContextItem, Panel};
use crate::actions::Action;
use crate::constants::{MAX_CONTEXT_TOKENS, SCROLL_ARROW_AMOUNT, SCROLL_PAGE_AMOUNT};
use crate::state::{ContextType, State, TodoStatus, MemoryImportance};
use crate::ui::{theme, chars, helpers::format_number};

pub struct OverviewPanel;

impl Panel for OverviewPanel {
    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        match key.code {
            KeyCode::Up => Some(Action::ScrollUp(SCROLL_ARROW_AMOUNT)),
            KeyCode::Down => Some(Action::ScrollDown(SCROLL_ARROW_AMOUNT)),
            KeyCode::PageUp => Some(Action::ScrollUp(SCROLL_PAGE_AMOUNT)),
            KeyCode::PageDown => Some(Action::ScrollDown(SCROLL_PAGE_AMOUNT)),
            _ => None,
        }
    }

    fn title(&self, _state: &State) -> String {
        "Context Overview".to_string()
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        // LLM should see the same overview the user sees
        let total_tokens: usize = state.context.iter().map(|c| c.token_count).sum();
        let max_tokens = MAX_CONTEXT_TOKENS;
        let usage_pct = (total_tokens as f64 / max_tokens as f64 * 100.0).min(100.0);

        let mut output = format!("Context Usage: {} / {} tokens ({:.1}%)\n\n",
            total_tokens, max_tokens, usage_pct);

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
            };

            let details = match ctx.context_type {
                ContextType::File => ctx.file_path.as_deref().unwrap_or("").to_string(),
                ContextType::Glob => ctx.glob_pattern.as_deref().unwrap_or("").to_string(),
                ContextType::Grep => ctx.grep_pattern.as_deref().unwrap_or("").to_string(),
                ContextType::Tmux => ctx.tmux_pane_id.as_deref().unwrap_or("").to_string(),
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
        output.push_str("| Tool | Status | Description |\n");
        output.push_str("|------|--------|-------------|\n");
        for tool in &state.tools {
            let status = if tool.enabled { "✓" } else { "✗" };
            output.push_str(&format!("| {} | {} | {} |\n", tool.id, status, tool.short_desc));
        }

        vec![ContextItem::new("Context Overview", output)]
    }

    fn content(&self, state: &State, base_style: Style) -> Vec<Line<'static>> {
        let mut text: Vec<Line> = Vec::new();

        // Token usage header
        let total_tokens: usize = state.context.iter().map(|c| c.token_count).sum();
        let max_tokens = MAX_CONTEXT_TOKENS;
        let usage_pct = (total_tokens as f64 / max_tokens as f64 * 100.0).min(100.0);

        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled("TOKEN USAGE".to_string(), Style::default().fg(theme::TEXT_MUTED).bold()),
        ]));
        text.push(Line::from(""));

        let current = format_number(total_tokens);
        let max = format_number(max_tokens);
        let pct = format!("{:.1}%", usage_pct);

        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled(current, Style::default().fg(theme::TEXT).bold()),
            Span::styled(" / ".to_string(), Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(max, Style::default().fg(theme::ACCENT).bold()),
            Span::styled(format!(" ({})", pct), Style::default().fg(theme::TEXT_MUTED)),
        ]));

        // Progress bar
        let bar_width = 60usize;
        let filled = ((usage_pct / 100.0) * bar_width as f64) as usize;
        let empty = bar_width.saturating_sub(filled);

        let bar_color = if usage_pct > 80.0 {
            theme::WARNING
        } else {
            theme::ACCENT
        };

        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled(chars::BLOCK_FULL.repeat(filled), Style::default().fg(bar_color)),
            Span::styled(chars::BLOCK_LIGHT.repeat(empty), Style::default().fg(theme::BG_ELEVATED)),
        ]));

        text.push(Line::from(""));
        text.push(Line::from(vec![
            Span::styled(format!(" {}", chars::HORIZONTAL.repeat(60)), Style::default().fg(theme::BORDER)),
        ]));
        text.push(Line::from(""));

        // Git status section (only if in a git repo)
        if state.git_is_repo {
            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled("GIT STATUS".to_string(), Style::default().fg(theme::TEXT_MUTED).bold()),
            ]));
            text.push(Line::from(""));

            // Branch name
            if let Some(branch) = &state.git_branch {
                let branch_color = if branch.starts_with("detached:") {
                    theme::WARNING
                } else {
                    theme::ACCENT
                };
                text.push(Line::from(vec![
                    Span::styled(" ".to_string(), base_style),
                    Span::styled("Branch: ".to_string(), Style::default().fg(theme::TEXT_SECONDARY)),
                    Span::styled(branch.clone(), Style::default().fg(branch_color).bold()),
                ]));
            }

            if state.git_file_changes.is_empty() {
                text.push(Line::from(vec![
                    Span::styled(" ".to_string(), base_style),
                    Span::styled("Working tree clean".to_string(), Style::default().fg(theme::SUCCESS)),
                ]));
            } else {
                text.push(Line::from(""));

                // Calculate column widths
                let path_width = state.git_file_changes.iter()
                    .map(|f| f.path.len())
                    .max()
                    .unwrap_or(4)
                    .max(4)
                    .min(40); // Cap at 40 chars

                // Table header
                text.push(Line::from(vec![
                    Span::styled(" ".to_string(), base_style),
                    Span::styled(format!("{:<width$}", "File", width = path_width), Style::default().fg(theme::TEXT_SECONDARY).bold()),
                    Span::styled("  ", base_style),
                    Span::styled(format!("{:>6}", "+"), Style::default().fg(theme::SUCCESS).bold()),
                    Span::styled("  ", base_style),
                    Span::styled(format!("{:>6}", "-"), Style::default().fg(Color::Rgb(200, 80, 80)).bold()),
                    Span::styled("  ", base_style),
                    Span::styled(format!("{:>6}", "Net"), Style::default().fg(theme::TEXT_SECONDARY).bold()),
                ]));

                // Separator
                text.push(Line::from(vec![
                    Span::styled(" ".to_string(), base_style),
                    Span::styled(chars::HORIZONTAL.repeat(path_width + 26), Style::default().fg(theme::BORDER)),
                ]));

                // File rows
                let mut total_add: i32 = 0;
                let mut total_del: i32 = 0;

                for file in &state.git_file_changes {
                    use crate::state::GitChangeType;

                    total_add += file.additions;
                    total_del += file.deletions;
                    let net = file.additions - file.deletions;

                    // Type indicator
                    let (type_char, type_color) = match file.change_type {
                        GitChangeType::Added => ("A", theme::SUCCESS),
                        GitChangeType::Deleted => ("D", Color::Rgb(200, 80, 80)),
                        GitChangeType::Modified => ("M", theme::WARNING),
                        GitChangeType::Renamed => ("R", theme::ACCENT),
                    };

                    // Truncate path if needed (account for type indicator)
                    let effective_path_width = path_width.saturating_sub(2);
                    let display_path = if file.path.len() > effective_path_width {
                        format!("...{}", &file.path[file.path.len() - effective_path_width + 3..])
                    } else {
                        file.path.clone()
                    };

                    let net_color = if net > 0 {
                        theme::SUCCESS
                    } else if net < 0 {
                        Color::Rgb(200, 80, 80)
                    } else {
                        theme::TEXT_MUTED
                    };

                    let net_str = if net > 0 {
                        format!("+{}", net)
                    } else {
                        format!("{}", net)
                    };

                    text.push(Line::from(vec![
                        Span::styled(" ".to_string(), base_style),
                        Span::styled(format!("{} ", type_char), Style::default().fg(type_color)),
                        Span::styled(format!("{:<width$}", display_path, width = effective_path_width), Style::default().fg(theme::TEXT)),
                        Span::styled("  ", base_style),
                        Span::styled(format!("{:>6}", format!("+{}", file.additions)), Style::default().fg(theme::SUCCESS)),
                        Span::styled("  ", base_style),
                        Span::styled(format!("{:>6}", format!("-{}", file.deletions)), Style::default().fg(Color::Rgb(200, 80, 80))),
                        Span::styled("  ", base_style),
                        Span::styled(format!("{:>6}", net_str), Style::default().fg(net_color)),
                    ]));
                }

                // Total row separator
                text.push(Line::from(vec![
                    Span::styled(" ".to_string(), base_style),
                    Span::styled(chars::HORIZONTAL.repeat(path_width + 26), Style::default().fg(theme::BORDER)),
                ]));

                // Total row
                let total_net = total_add - total_del;
                let total_net_color = if total_net > 0 {
                    theme::SUCCESS
                } else if total_net < 0 {
                    Color::Rgb(200, 80, 80)
                } else {
                    theme::TEXT_MUTED
                };
                let total_net_str = if total_net > 0 {
                    format!("+{}", total_net)
                } else {
                    format!("{}", total_net)
                };

                text.push(Line::from(vec![
                    Span::styled(" ".to_string(), base_style),
                    Span::styled(format!("{:<width$}", "Total", width = path_width), Style::default().fg(theme::TEXT).bold()),
                    Span::styled("  ", base_style),
                    Span::styled(format!("{:>6}", format!("+{}", total_add)), Style::default().fg(theme::SUCCESS).bold()),
                    Span::styled("  ", base_style),
                    Span::styled(format!("{:>6}", format!("-{}", total_del)), Style::default().fg(Color::Rgb(200, 80, 80)).bold()),
                    Span::styled("  ", base_style),
                    Span::styled(format!("{:>6}", total_net_str), Style::default().fg(total_net_color).bold()),
                ]));
            }

            text.push(Line::from(""));
            text.push(Line::from(vec![
                Span::styled(format!(" {}", chars::HORIZONTAL.repeat(60)), Style::default().fg(theme::BORDER)),
            ]));
            text.push(Line::from(""));
        }

        // Context elements header
        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled("CONTEXT ELEMENTS".to_string(), Style::default().fg(theme::TEXT_MUTED).bold()),
        ]));
        text.push(Line::from(""));

        let id_width = state.context.iter().map(|c| c.id.len()).max().unwrap_or(2);

        for ctx in &state.context {
            let icon = ctx.context_type.icon();
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
                _ => String::new(),
            };

            let tokens = format_number(ctx.token_count);
            let shortcut = format!("{:>width$}", &ctx.id, width = id_width);

            let mut spans = vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled(format!("{} ", icon), Style::default().fg(theme::TEXT_MUTED)),
                Span::styled(format!("{} ", shortcut), Style::default().fg(theme::ACCENT_DIM)),
                Span::styled(format!("{:<12}", type_name), Style::default().fg(theme::TEXT_SECONDARY)),
                Span::styled(format!("{:>8}", tokens), Style::default().fg(theme::ACCENT)),
            ];

            if !details.is_empty() {
                let max_detail_len = 40usize;
                let truncated_details = if details.len() > max_detail_len {
                    format!("{}...", &details[..max_detail_len.saturating_sub(3)])
                } else {
                    details
                };
                spans.push(Span::styled(format!("  {}", truncated_details), Style::default().fg(theme::TEXT_MUTED)));
            }

            text.push(Line::from(spans));
        }

        text.push(Line::from(""));
        text.push(Line::from(vec![
            Span::styled(format!(" {}", chars::HORIZONTAL.repeat(60)), Style::default().fg(theme::BORDER)),
        ]));
        text.push(Line::from(""));

        // Statistics section
        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled("STATISTICS".to_string(), Style::default().fg(theme::TEXT_MUTED).bold()),
        ]));
        text.push(Line::from(""));

        let user_msgs = state.messages.iter().filter(|m| m.role == "user").count();
        let assistant_msgs = state.messages.iter().filter(|m| m.role == "assistant").count();
        let total_msgs = state.messages.len();

        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled("Messages: ".to_string(), Style::default().fg(theme::TEXT_SECONDARY)),
            Span::styled(format!("{}", total_msgs), Style::default().fg(theme::TEXT).bold()),
            Span::styled(format!(" ({} user, {} assistant)", user_msgs, assistant_msgs), Style::default().fg(theme::TEXT_MUTED)),
        ]));

        let total_todos = state.todos.len();
        if total_todos > 0 {
            let done_todos = state.todos.iter().filter(|t| t.status == TodoStatus::Done).count();
            let in_progress = state.todos.iter().filter(|t| t.status == TodoStatus::InProgress).count();
            let pending = total_todos - done_todos - in_progress;

            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled("Todos: ".to_string(), Style::default().fg(theme::TEXT_SECONDARY)),
                Span::styled(format!("{}/{}", done_todos, total_todos), Style::default().fg(theme::SUCCESS).bold()),
                Span::styled(" done".to_string(), Style::default().fg(theme::TEXT_MUTED)),
                Span::styled(format!(", {} in progress, {} pending", in_progress, pending), Style::default().fg(theme::TEXT_MUTED)),
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
                Span::styled("Memories: ".to_string(), Style::default().fg(theme::TEXT_SECONDARY)),
                Span::styled(format!("{}", total_memories), Style::default().fg(theme::TEXT).bold()),
                Span::styled(format!(" ({} critical, {} high, {} medium, {} low)", critical, high, medium, low), Style::default().fg(theme::TEXT_MUTED)),
            ]));
        }

        text.push(Line::from(""));
        text.push(Line::from(vec![
            Span::styled(format!(" {}", chars::HORIZONTAL.repeat(60)), Style::default().fg(theme::BORDER)),
        ]));
        text.push(Line::from(""));

        // Tools section
        let enabled_count = state.tools.iter().filter(|t| t.enabled).count();
        let disabled_count = state.tools.iter().filter(|t| !t.enabled).count();

        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled("TOOLS".to_string(), Style::default().fg(theme::TEXT_MUTED).bold()),
            Span::styled(format!("  ({} enabled, {} disabled)", enabled_count, disabled_count), Style::default().fg(theme::TEXT_MUTED)),
        ]));
        text.push(Line::from(""));

        // Table header
        let name_width = state.tools.iter().map(|t| t.id.len()).max().unwrap_or(10).max(10);
        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled(format!("{:<width$}", "Tool", width = name_width), Style::default().fg(theme::TEXT_SECONDARY).bold()),
            Span::styled("  ", base_style),
            Span::styled("  ", base_style),
            Span::styled("Description", Style::default().fg(theme::TEXT_SECONDARY).bold()),
        ]));

        // Table separator
        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled(format!("{}", chars::HORIZONTAL.repeat(name_width + 40)), Style::default().fg(theme::BORDER)),
        ]));

        // Table rows
        for tool in &state.tools {
            let (status_icon, status_color) = if tool.enabled {
                ("✓", theme::SUCCESS)
            } else {
                ("✗", Color::Rgb(200, 80, 80)) // Red for disabled
            };

            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled(format!("{:<width$}", tool.id, width = name_width), Style::default().fg(theme::TEXT)),
                Span::styled("  ", base_style),
                Span::styled(status_icon, Style::default().fg(status_color)),
                Span::styled("  ", base_style),
                Span::styled(tool.short_desc.clone(), Style::default().fg(theme::TEXT_MUTED)),
            ]));
        }

        text
    }
}
