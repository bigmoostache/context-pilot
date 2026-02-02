use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;

use super::{ContextItem, Panel};
use crate::actions::Action;
use crate::constants::{SCROLL_ARROW_AMOUNT, SCROLL_PAGE_AMOUNT};
use crate::state::{estimate_tokens, ContextType, GitChangeType, State};
use crate::ui::{theme, chars};

pub struct GitPanel;

impl GitPanel {
    /// Format git status for LLM context (as markdown table + diffs)
    fn format_git_for_context(state: &State) -> String {
        if !state.git_is_repo {
            return "Not a git repository".to_string();
        }

        let mut output = String::new();

        // Branch
        if let Some(branch) = &state.git_branch {
            output.push_str(&format!("Branch: {}\n", branch));
        }

        if state.git_file_changes.is_empty() {
            output.push_str("\nWorking tree clean\n");
        } else {
            output.push_str("\n| File | Type | + | - | Net |\n");
            output.push_str("|------|------|---|---|-----|\n");

            let mut total_add: i32 = 0;
            let mut total_del: i32 = 0;

            for file in &state.git_file_changes {
                total_add += file.additions;
                total_del += file.deletions;
                let net = file.additions - file.deletions;
                let net_str = if net >= 0 { format!("+{}", net) } else { format!("{}", net) };
                let type_str = match file.change_type {
                    GitChangeType::Added => "A",
                    GitChangeType::Deleted => "D",
                    GitChangeType::Modified => "M",
                    GitChangeType::Renamed => "R",
                };
                output.push_str(&format!("| {} | {} | +{} | -{} | {} |\n",
                    file.path, type_str, file.additions, file.deletions, net_str));
            }

            let total_net = total_add - total_del;
            let total_net_str = if total_net >= 0 { format!("+{}", total_net) } else { format!("{}", total_net) };
            output.push_str(&format!("| **Total** | | **+{}** | **-{}** | **{}** |\n",
                total_add, total_del, total_net_str));

            // Add diff content only if git_show_diffs is enabled
            if state.git_show_diffs {
                output.push_str("\n## Diffs\n\n");
                for file in &state.git_file_changes {
                    if !file.diff_content.is_empty() {
                        output.push_str("```diff\n");
                        output.push_str(&file.diff_content);
                        output.push_str("```\n\n");
                    }
                }
            }
        }

        output
    }
}

impl Panel for GitPanel {
    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        match key.code {
            KeyCode::Up => Some(Action::ScrollUp(SCROLL_ARROW_AMOUNT)),
            KeyCode::Down => Some(Action::ScrollDown(SCROLL_ARROW_AMOUNT)),
            KeyCode::PageUp => Some(Action::ScrollUp(SCROLL_PAGE_AMOUNT)),
            KeyCode::PageDown => Some(Action::ScrollDown(SCROLL_PAGE_AMOUNT)),
            _ => None,
        }
    }

    fn title(&self, state: &State) -> String {
        if let Some(branch) = &state.git_branch {
            format!("Git ({})", branch)
        } else {
            "Git".to_string()
        }
    }

    fn refresh(&self, state: &mut State) {
        // Token count is already set by cache system when GitStatus arrives
        // Only recalculate if no cached content exists (shouldn't happen normally)
        let needs_calc = state.context.iter()
            .find(|c| c.context_type == ContextType::Git)
            .map(|ctx| ctx.cached_content.is_none())
            .unwrap_or(false);

        if needs_calc {
            let git_content = Self::format_git_for_context(state);
            let token_count = estimate_tokens(&git_content);
            for ctx in &mut state.context {
                if ctx.context_type == ContextType::Git {
                    ctx.token_count = token_count;
                    break;
                }
            }
        }
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        if !state.git_is_repo {
            return vec![];
        }
        
        // Use cached content if available
        let content = state.context.iter()
            .find(|c| c.context_type == ContextType::Git)
            .and_then(|ctx| ctx.cached_content.as_ref())
            .map(|c| {
                if state.context.iter()
                    .find(|ctx| ctx.context_type == ContextType::Git)
                    .map(|ctx| ctx.cache_deprecated)
                    .unwrap_or(false)
                {
                    format!("[refreshing...]\n{}", c)
                } else {
                    c.clone()
                }
            })
            .unwrap_or_else(|| Self::format_git_for_context(state));
        
        vec![ContextItem::new("Git Status", content)]
    }

    fn content(&self, state: &State, base_style: Style) -> Vec<Line<'static>> {
        let mut text: Vec<Line> = Vec::new();

        if !state.git_is_repo {
            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled("Not a git repository".to_string(), Style::default().fg(theme::TEXT_MUTED).italic()),
            ]));
            return text;
        }

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
            text.push(Line::from(""));
        }

        if state.git_file_changes.is_empty() {
            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled("Working tree clean".to_string(), Style::default().fg(theme::SUCCESS)),
            ]));
            return text;
        }

        // Calculate column widths
        let path_width = state.git_file_changes.iter()
            .map(|f| f.path.len())
            .max()
            .unwrap_or(4)
            .max(4)
            .min(45); // Cap at 45 chars for the panel

        // Table header
        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled("T ".to_string(), Style::default().fg(theme::TEXT_SECONDARY).bold()),
            Span::styled(format!("{:<width$}", "File", width = path_width), Style::default().fg(theme::TEXT_SECONDARY).bold()),
            Span::styled("  ", base_style),
            Span::styled(format!("{:>6}", "+"), Style::default().fg(theme::SUCCESS).bold()),
            Span::styled("  ", base_style),
            Span::styled(format!("{:>6}", "-"), Style::default().fg(theme::ERROR).bold()),
            Span::styled("  ", base_style),
            Span::styled(format!("{:>6}", "Net"), Style::default().fg(theme::TEXT_SECONDARY).bold()),
        ]));

        // Separator
        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled(chars::HORIZONTAL.repeat(path_width + 30), Style::default().fg(theme::BORDER)),
        ]));

        // File rows
        let mut total_add: i32 = 0;
        let mut total_del: i32 = 0;

        for file in &state.git_file_changes {
            total_add += file.additions;
            total_del += file.deletions;
            let net = file.additions - file.deletions;

            // Type indicator
            let (type_char, type_color) = match file.change_type {
                GitChangeType::Added => ("A", theme::SUCCESS),
                GitChangeType::Deleted => ("D", theme::ERROR),
                GitChangeType::Modified => ("M", theme::WARNING),
                GitChangeType::Renamed => ("R", theme::ACCENT),
            };

            // Truncate path if needed
            let display_path = if file.path.len() > path_width {
                format!("...{}", &file.path[file.path.len() - path_width + 3..])
            } else {
                file.path.clone()
            };

            let net_color = if net > 0 {
                theme::SUCCESS
            } else if net < 0 {
                theme::ERROR
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
                Span::styled(format!("{:<width$}", display_path, width = path_width), Style::default().fg(theme::TEXT)),
                Span::styled("  ", base_style),
                Span::styled(format!("{:>6}", format!("+{}", file.additions)), Style::default().fg(theme::SUCCESS)),
                Span::styled("  ", base_style),
                Span::styled(format!("{:>6}", format!("-{}", file.deletions)), Style::default().fg(theme::ERROR)),
                Span::styled("  ", base_style),
                Span::styled(format!("{:>6}", net_str), Style::default().fg(net_color)),
            ]));
        }

        // Total row separator
        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled(chars::HORIZONTAL.repeat(path_width + 30), Style::default().fg(theme::BORDER)),
        ]));

        // Total row
        let total_net = total_add - total_del;
        let total_net_color = if total_net > 0 {
            theme::SUCCESS
        } else if total_net < 0 {
            theme::ERROR
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
            Span::styled("  ".to_string(), base_style),
            Span::styled(format!("{:<width$}", "Total", width = path_width), Style::default().fg(theme::TEXT).bold()),
            Span::styled("  ", base_style),
            Span::styled(format!("{:>6}", format!("+{}", total_add)), Style::default().fg(theme::SUCCESS).bold()),
            Span::styled("  ", base_style),
            Span::styled(format!("{:>6}", format!("-{}", total_del)), Style::default().fg(theme::ERROR).bold()),
            Span::styled("  ", base_style),
            Span::styled(format!("{:>6}", total_net_str), Style::default().fg(total_net_color).bold()),
        ]));

        // Summary stats
        text.push(Line::from(""));
        let added = state.git_file_changes.iter().filter(|f| f.change_type == GitChangeType::Added).count();
        let modified = state.git_file_changes.iter().filter(|f| f.change_type == GitChangeType::Modified).count();
        let deleted = state.git_file_changes.iter().filter(|f| f.change_type == GitChangeType::Deleted).count();
        let renamed = state.git_file_changes.iter().filter(|f| f.change_type == GitChangeType::Renamed).count();

        let mut summary_parts = Vec::new();
        if added > 0 { summary_parts.push(format!("{} added", added)); }
        if modified > 0 { summary_parts.push(format!("{} modified", modified)); }
        if deleted > 0 { summary_parts.push(format!("{} deleted", deleted)); }
        if renamed > 0 { summary_parts.push(format!("{} renamed", renamed)); }

        if !summary_parts.is_empty() {
            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled(summary_parts.join(", "), Style::default().fg(theme::TEXT_MUTED)),
            ]));
        }

        // Display diff content for each file
        for file in &state.git_file_changes {
            if file.diff_content.is_empty() {
                continue;
            }

            text.push(Line::from(""));
            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled(chars::HORIZONTAL.repeat(60), Style::default().fg(theme::BORDER)),
            ]));

            // Render diff with syntax highlighting
            for line in file.diff_content.lines() {
                let (style, display_line) = if line.starts_with("+++") || line.starts_with("---") {
                    // File header lines
                    (Style::default().fg(theme::TEXT_SECONDARY).bold(), line.to_string())
                } else if line.starts_with("@@") {
                    // Hunk header
                    (Style::default().fg(theme::ACCENT), line.to_string())
                } else if line.starts_with('+') && !line.starts_with("+++") {
                    // Addition
                    (Style::default().fg(theme::SUCCESS), line.to_string())
                } else if line.starts_with('-') && !line.starts_with("---") {
                    // Deletion
                    (Style::default().fg(theme::ERROR), line.to_string())
                } else if line.starts_with("diff --git") {
                    // Diff header
                    (Style::default().fg(theme::ACCENT).bold(), line.to_string())
                } else if line.starts_with("new file") || line.starts_with("deleted file") || line.starts_with("index ") {
                    // Meta info
                    (Style::default().fg(theme::TEXT_MUTED), line.to_string())
                } else {
                    // Context line
                    (Style::default().fg(theme::TEXT_MUTED), format!(" {}", line))
                };

                text.push(Line::from(vec![
                    Span::styled(" ".to_string(), base_style),
                    Span::styled(display_line, style),
                ]));
            }
        }

        text
    }
}
