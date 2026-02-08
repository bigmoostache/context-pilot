use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;

use crate::core::panels::{ContextItem, Panel};
use crate::actions::Action;
use crate::constants::{SCROLL_ARROW_AMOUNT, SCROLL_PAGE_AMOUNT};
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
        // Use cached content if available (set by refresh)
        if let Some(ctx) = state.context.iter().find(|c| c.context_type == ContextType::Overview) {
            if let Some(content) = &ctx.cached_content {
                return vec![ContextItem::new(&ctx.id, "Context Overview", content.clone(), ctx.last_refresh_ms)];
            }
        }

        // Fallback: generate fresh
        let output = self.generate_context_content(state);
        let (id, last_refresh_ms) = state.context.iter()
            .find(|c| c.context_type == ContextType::Overview)
            .map(|c| (c.id.as_str(), c.last_refresh_ms))
            .unwrap_or(("P5", 0));
        vec![ContextItem::new(id, "Context Overview", output, last_refresh_ms)]
    }

    fn refresh(&self, state: &mut State) {
        let content = self.generate_context_content(state);
        let token_count = crate::state::estimate_tokens(&content);

        if let Some(ctx) = state.context.iter_mut().find(|c| c.context_type == ContextType::Overview) {
            ctx.token_count = token_count;
            ctx.cached_content = Some(content);
        }
    }

    fn content(&self, state: &State, base_style: Style) -> Vec<Line<'static>> {
        let _guard = crate::profile!("panel::overview::content");
        let mut text: Vec<Line> = Vec::new();

        // Token usage header
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

        text.push(Line::from(""));
        text.push(Line::from(vec![
            Span::styled(format!(" {}", chars::HORIZONTAL.repeat(60)), Style::default().fg(theme::border())),
        ]));
        text.push(Line::from(""));

        // Git status section (only if in a git repo)
        if state.git_is_repo {
            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled("GIT STATUS".to_string(), Style::default().fg(theme::text_muted()).bold()),
            ]));
            text.push(Line::from(""));

            // Branch name
            if let Some(branch) = &state.git_branch {
                let branch_color = if branch.starts_with("detached:") {
                    theme::warning()
                } else {
                    theme::accent()
                };
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
                    Span::styled(format!("{:<width$}", "File", width = path_width), Style::default().fg(theme::text_secondary()).bold()),
                    Span::styled("  ", base_style),
                    Span::styled(format!("{:>6}", "+"), Style::default().fg(theme::success()).bold()),
                    Span::styled("  ", base_style),
                    Span::styled(format!("{:>6}", "-"), Style::default().fg(theme::error()).bold()),
                    Span::styled("  ", base_style),
                    Span::styled(format!("{:>6}", "Net"), Style::default().fg(theme::text_secondary()).bold()),
                ]));

                // Separator
                text.push(Line::from(vec![
                    Span::styled(" ".to_string(), base_style),
                    Span::styled(chars::HORIZONTAL.repeat(path_width + 26), Style::default().fg(theme::border())),
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
                        GitChangeType::Added => ("A", theme::success()),
                        GitChangeType::Untracked => ("U", theme::success()),
                        GitChangeType::Deleted => ("D", theme::error()),
                        GitChangeType::Modified => ("M", theme::warning()),
                        GitChangeType::Renamed => ("R", theme::accent()),
                    };

                    // Truncate path if needed (account for type indicator)
                    let effective_path_width = path_width.saturating_sub(2);
                    let display_path = if file.path.len() > effective_path_width {
                        format!("...{}", &file.path[file.path.len() - effective_path_width + 3..])
                    } else {
                        file.path.clone()
                    };

                    let net_color = if net > 0 {
                        theme::success()
                    } else if net < 0 {
                        theme::error()
                    } else {
                        theme::text_muted()
                    };

                    let net_str = if net > 0 {
                        format!("+{}", net)
                    } else {
                        format!("{}", net)
                    };

                    text.push(Line::from(vec![
                        Span::styled(" ".to_string(), base_style),
                        Span::styled(format!("{} ", type_char), Style::default().fg(type_color)),
                        Span::styled(format!("{:<width$}", display_path, width = effective_path_width), Style::default().fg(theme::text())),
                        Span::styled("  ", base_style),
                        Span::styled(format!("{:>6}", format!("+{}", file.additions)), Style::default().fg(theme::success())),
                        Span::styled("  ", base_style),
                        Span::styled(format!("{:>6}", format!("-{}", file.deletions)), Style::default().fg(theme::error())),
                        Span::styled("  ", base_style),
                        Span::styled(format!("{:>6}", net_str), Style::default().fg(net_color)),
                    ]));
                }

                // Total row separator
                text.push(Line::from(vec![
                    Span::styled(" ".to_string(), base_style),
                    Span::styled(chars::HORIZONTAL.repeat(path_width + 26), Style::default().fg(theme::border())),
                ]));

                // Total row
                let total_net = total_add - total_del;
                let total_net_color = if total_net > 0 {
                    theme::success()
                } else if total_net < 0 {
                    theme::error()
                } else {
                    theme::text_muted()
                };
                let total_net_str = if total_net > 0 {
                    format!("+{}", total_net)
                } else {
                    format!("{}", total_net)
                };

                text.push(Line::from(vec![
                    Span::styled(" ".to_string(), base_style),
                    Span::styled(format!("{:<width$}", "Total", width = path_width), Style::default().fg(theme::text()).bold()),
                    Span::styled("  ", base_style),
                    Span::styled(format!("{:>6}", format!("+{}", total_add)), Style::default().fg(theme::success()).bold()),
                    Span::styled("  ", base_style),
                    Span::styled(format!("{:>6}", format!("-{}", total_del)), Style::default().fg(theme::error()).bold()),
                    Span::styled("  ", base_style),
                    Span::styled(format!("{:>6}", total_net_str), Style::default().fg(total_net_color).bold()),
                ]));
            }

            text.push(Line::from(""));
            text.push(Line::from(vec![
                Span::styled(format!(" {}", chars::HORIZONTAL.repeat(60)), Style::default().fg(theme::border())),
            ]));
            text.push(Line::from(""));
        }

        // Context elements header
        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled("CONTEXT ELEMENTS".to_string(), Style::default().fg(theme::text_muted()).bold()),
        ]));
        text.push(Line::from(""));

        let id_width = state.context.iter().map(|c| c.id.len()).max().unwrap_or(2);

        for ctx in &state.context {
            let icon = ctx.context_type.icon();
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

            let tokens = format_number(ctx.token_count);
            let shortcut = format!("{:>width$}", &ctx.id, width = id_width);

            let mut spans = vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled(icon, Style::default().fg(theme::text_muted())),
                Span::styled(format!("{} ", shortcut), Style::default().fg(theme::accent_dim())),
                Span::styled(format!("{:<12}", type_name), Style::default().fg(theme::text_secondary())),
                Span::styled(format!("{:>8}", tokens), Style::default().fg(theme::accent())),
            ];

            if !details.is_empty() {
                let max_detail_len = 40usize;
                let truncated_details = if details.len() > max_detail_len {
                    format!("{}...", &details[..max_detail_len.saturating_sub(3)])
                } else {
                    details
                };
                spans.push(Span::styled(format!("  {}", truncated_details), Style::default().fg(theme::text_muted())));
            }

            text.push(Line::from(spans));
        }

        text.push(Line::from(""));
        text.push(Line::from(vec![
            Span::styled(format!(" {}", chars::HORIZONTAL.repeat(60)), Style::default().fg(theme::border())),
        ]));
        text.push(Line::from(""));

        // Statistics section
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
            Span::styled(format!(" ({} user, {} assistant)", user_msgs, assistant_msgs), Style::default().fg(theme::text_muted())),
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
                Span::styled(format!(", {} in progress, {} pending", in_progress, pending), Style::default().fg(theme::text_muted())),
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
                Span::styled(format!(" ({} critical, {} high, {} medium, {} low)", critical, high, medium, low), Style::default().fg(theme::text_muted())),
            ]));
        }

        text.push(Line::from(""));
        text.push(Line::from(vec![
            Span::styled(format!(" {}", chars::HORIZONTAL.repeat(60)), Style::default().fg(theme::border())),
        ]));
        text.push(Line::from(""));

        // Seeds section
        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled("SEEDS".to_string(), Style::default().fg(theme::text_muted()).bold()),
            Span::styled(format!("  ({} available)", state.systems.len()), Style::default().fg(theme::text_muted())),
        ]));
        text.push(Line::from(""));

        {
            // Calculate column widths
            let id_width = state.systems.iter().map(|s| s.id.len()).max().unwrap_or(4).max(4);
            let name_width = state.systems.iter().map(|s| s.name.len()).max().unwrap_or(10).max(10).min(20);

            // Table header
            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled(format!("{:<width$}", "ID", width = id_width), Style::default().fg(theme::text_secondary()).bold()),
                Span::styled("  ", base_style),
                Span::styled(format!("{:<width$}", "Name", width = name_width), Style::default().fg(theme::text_secondary()).bold()),
                Span::styled("  ", base_style),
                Span::styled("Active", Style::default().fg(theme::text_secondary()).bold()),
                Span::styled("  ", base_style),
                Span::styled("Description", Style::default().fg(theme::text_secondary()).bold()),
            ]));

            // Table separator
            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled(format!("{}", chars::HORIZONTAL.repeat(id_width + name_width + 50)), Style::default().fg(theme::border())),
            ]));

            // Table rows
            for sys in &state.systems {
                let is_active = state.active_system_id.as_deref() == Some(&sys.id);
                let (active_icon, active_color) = if is_active {
                    (crate::config::normalize_icon("✓"), theme::success())
                } else {
                    ("  ".to_string(), theme::text_muted())  // 2 spaces for alignment
                };

                // Truncate name if needed
                let display_name = if sys.name.len() > name_width {
                    format!("{}...", &sys.name[..name_width.saturating_sub(3)])
                } else {
                    sys.name.clone()
                };

                // Truncate description
                let desc_max = 35;
                let display_desc = if sys.description.len() > desc_max {
                    format!("{}...", &sys.description[..desc_max.saturating_sub(3)])
                } else {
                    sys.description.clone()
                };

                text.push(Line::from(vec![
                    Span::styled(" ".to_string(), base_style),
                    Span::styled(format!("{:<width$}", sys.id, width = id_width), Style::default().fg(theme::accent_dim())),
                    Span::styled("  ", base_style),
                    Span::styled(format!("{:<width$}", display_name, width = name_width), Style::default().fg(theme::text())),
                    Span::styled("  ", base_style),
                    Span::styled(format!("  {} ", active_icon), Style::default().fg(active_color)),
                    Span::styled("  ", base_style),
                    Span::styled(display_desc, Style::default().fg(theme::text_muted())),
                ]));
            }
        }

        text.push(Line::from(""));
        text.push(Line::from(vec![
            Span::styled(format!(" {}", chars::HORIZONTAL.repeat(60)), Style::default().fg(theme::border())),
        ]));
        text.push(Line::from(""));

        // Presets section
        let presets = crate::modules::preset::tools::list_presets_with_info();
        if !presets.is_empty() {
            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled("PRESETS".to_string(), Style::default().fg(theme::text_muted()).bold()),
                Span::styled(format!("  ({} available)", presets.len()), Style::default().fg(theme::text_muted())),
            ]));
            text.push(Line::from(""));

            // Calculate column widths
            let name_width = presets.iter().map(|p| p.name.len()).max().unwrap_or(10).max(10).min(25);
            let type_width = 8; // "built-in" length

            // Table header
            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled(format!("{:<width$}", "Name", width = name_width), Style::default().fg(theme::text_secondary()).bold()),
                Span::styled("  ", base_style),
                Span::styled(format!("{:<width$}", "Type", width = type_width), Style::default().fg(theme::text_secondary()).bold()),
                Span::styled("  ", base_style),
                Span::styled("Description", Style::default().fg(theme::text_secondary()).bold()),
            ]));

            // Table separator
            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled(format!("{}", chars::HORIZONTAL.repeat(name_width + type_width + 40)), Style::default().fg(theme::border())),
            ]));

            // Table rows
            for p in &presets {
                let (type_label, type_color) = if p.built_in {
                    ("built-in", theme::accent_dim())
                } else {
                    ("custom", theme::success())
                };

                // Truncate name if needed
                let display_name = if p.name.len() > name_width {
                    format!("{}...", &p.name[..name_width.saturating_sub(3)])
                } else {
                    p.name.clone()
                };

                // Truncate description
                let desc_max = 35;
                let display_desc = if p.description.len() > desc_max {
                    format!("{}...", &p.description[..desc_max.saturating_sub(3)])
                } else {
                    p.description.clone()
                };

                text.push(Line::from(vec![
                    Span::styled(" ".to_string(), base_style),
                    Span::styled(format!("{:<width$}", display_name, width = name_width), Style::default().fg(theme::text())),
                    Span::styled("  ", base_style),
                    Span::styled(format!("{:<width$}", type_label, width = type_width), Style::default().fg(type_color)),
                    Span::styled("  ", base_style),
                    Span::styled(display_desc, Style::default().fg(theme::text_muted())),
                ]));
            }
        }

        text.push(Line::from(""));
        text.push(Line::from(vec![
            Span::styled(format!(" {}", chars::HORIZONTAL.repeat(60)), Style::default().fg(theme::border())),
        ]));
        text.push(Line::from(""));

        // Tools section - grouped by category
        let enabled_count = state.tools.iter().filter(|t| t.enabled).count();
        let disabled_count = state.tools.iter().filter(|t| !t.enabled).count();

        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled("TOOLS".to_string(), Style::default().fg(theme::text_muted()).bold()),
            Span::styled(format!("  ({} enabled, {} disabled)", enabled_count, disabled_count), Style::default().fg(theme::text_muted())),
        ]));
        text.push(Line::from(""));

        use crate::constants::tool_categories;
        use crate::tool_defs::ToolCategory;

        // Iterate through each category
        for category in ToolCategory::all() {
            let category_tools: Vec<_> = state.tools.iter()
                .filter(|t| &t.category == category)
                .collect();
            
            if category_tools.is_empty() {
                continue;
            }

            // Category header with description
            let (cat_name, cat_desc) = match category {
                ToolCategory::File => ("FILE", tool_categories::file_desc()),
                ToolCategory::Tree => ("TREE", tool_categories::tree_desc()),
                ToolCategory::Console => ("CONSOLE", tool_categories::console_desc()),
                ToolCategory::Context => ("CONTEXT", tool_categories::context_desc()),
                ToolCategory::Todo => ("TODO", tool_categories::todo_desc()),
                ToolCategory::Memory => ("MEMORY", tool_categories::memory_desc()),
                ToolCategory::Git => ("GIT", tool_categories::git_desc()),
                ToolCategory::Github => ("GITHUB", "GitHub API operations via gh CLI"),
                ToolCategory::Scratchpad => ("SCRATCHPAD", tool_categories::scratchpad_desc()),
            };

            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled(cat_name.to_string(), Style::default().fg(theme::accent()).bold()),
                Span::styled(format!("  {}", cat_desc), Style::default().fg(theme::text_muted())),
            ]));

            // Table header for this category
            let name_width = category_tools.iter().map(|t| t.id.len()).max().unwrap_or(10).max(10);
            text.push(Line::from(vec![
                Span::styled("   ".to_string(), base_style),
                Span::styled(format!("{:<width$}", "Tool", width = name_width), Style::default().fg(theme::text_secondary())),
                Span::styled("  ", base_style),
                Span::styled("  ", base_style),
                Span::styled("Description", Style::default().fg(theme::text_secondary())),
            ]));

            // Tool rows for this category
            for tool in &category_tools {
                let (status_icon, status_color) = if tool.enabled {
                    ("✓", theme::success())
                } else {
                    ("✗", theme::error())
                };

                text.push(Line::from(vec![
                    Span::styled("   ".to_string(), base_style),
                    Span::styled(format!("{:<width$}", tool.id, width = name_width), Style::default().fg(theme::text())),
                    Span::styled("  ", base_style),
                    Span::styled(status_icon, Style::default().fg(status_color)),
                    Span::styled("  ", base_style),
                    Span::styled(tool.short_desc.clone(), Style::default().fg(theme::text_muted())),
                ]));
            }

            text.push(Line::from(""));
        }

        text
    }
}

impl OverviewPanel {
    fn generate_context_content(&self, state: &State) -> String {
        let total_tokens: usize = state.context.iter().map(|c| c.token_count).sum();
        let budget = state.effective_context_budget();
        let threshold = state.cleaning_threshold_tokens();
        let usage_pct = (total_tokens as f64 / budget as f64 * 100.0).min(100.0);

        let mut output = format!("Context Usage: {} / {} threshold / {} budget ({:.1}%)\n\n",
            total_tokens, threshold, budget, usage_pct);

        output.push_str("Context Elements:\n");
        for ctx in &state.context {
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

        // Available seeds (system prompts) for LLM
        output.push_str("\nSeeds (System Prompts):\n\n");
        output.push_str("| ID | Name | Active | Description |\n");
        output.push_str("|-----|------|--------|-------------|\n");
        for sys in &state.systems {
            let active = if state.active_system_id.as_deref() == Some(&sys.id) { "✓" } else { " " };
            output.push_str(&format!("| {} | {} | {} | {} |\n", sys.id, sys.name, active, sys.description));
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
}
