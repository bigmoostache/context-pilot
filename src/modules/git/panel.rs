use std::collections::HashMap;
use std::process::Command;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;

use super::GIT_STATUS_REFRESH_MS;
use crate::actions::Action;
use crate::cache::{CacheRequest, CacheUpdate, hash_content};
use crate::constants::{GIT_CMD_TIMEOUT_SECS, MAX_RESULT_CONTENT_BYTES};
use crate::constants::{SCROLL_ARROW_AMOUNT, SCROLL_PAGE_AMOUNT};
use crate::core::panels::{ContextItem, Panel, paginate_content, update_if_changed};
use crate::modules::{run_with_timeout, truncate_output};
use crate::state::{
    ContextElement, ContextType, GitChangeType, GitFileChange, State, compute_total_pages, estimate_tokens,
};
use crate::ui::{chars, theme};

pub(crate) struct GitResultPanel;
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
                    GitChangeType::Untracked => "U",
                    GitChangeType::Deleted => "D",
                    GitChangeType::Modified => "M",
                    GitChangeType::Renamed => "R",
                };
                output.push_str(&format!(
                    "| {} | {} | +{} | -{} | {} |\n",
                    file.path, type_str, file.additions, file.deletions, net_str
                ));
            }

            let total_net = total_add - total_del;
            let total_net_str = if total_net >= 0 { format!("+{}", total_net) } else { format!("{}", total_net) };
            output.push_str(&format!(
                "| **Total** | | **+{}** | **-{}** | **{}** |\n",
                total_add, total_del, total_net_str
            ));

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

/// Format git status for LLM context (from raw change tuples — used in cache refresh)
fn format_git_content_for_cache(
    branch: &Option<String>,
    changes: &[(String, i32, i32, GitChangeType, String)],
    show_diffs: bool,
) -> String {
    let mut output = String::new();
    if let Some(branch) = branch {
        output.push_str(&format!("Branch: {}\n", branch));
    }
    if changes.is_empty() {
        output.push_str("\nWorking tree clean\n");
    } else {
        output.push_str("\n| File | Type | + | - | Net |\n");
        output.push_str("|------|------|---|---|-----|\n");
        let mut total_add: i32 = 0;
        let mut total_del: i32 = 0;
        for (path, additions, deletions, change_type, _) in changes {
            total_add += additions;
            total_del += deletions;
            let net = additions - deletions;
            let net_str = if net >= 0 { format!("+{}", net) } else { format!("{}", net) };
            let type_str = match change_type {
                GitChangeType::Added => "A",
                GitChangeType::Untracked => "U",
                GitChangeType::Deleted => "D",
                GitChangeType::Modified => "M",
                GitChangeType::Renamed => "R",
            };
            output.push_str(&format!("| {} | {} | +{} | -{} | {} |\n", path, type_str, additions, deletions, net_str));
        }
        let total_net = total_add - total_del;
        let total_net_str = if total_net >= 0 { format!("+{}", total_net) } else { format!("{}", total_net) };
        output
            .push_str(&format!("| **Total** | | **+{}** | **-{}** | **{}** |\n", total_add, total_del, total_net_str));
        if show_diffs {
            output.push_str("\n## Diffs\n\n");
            for (_, _, _, _, diff_content) in changes {
                if !diff_content.is_empty() {
                    output.push_str("```diff\n");
                    output.push_str(diff_content);
                    output.push_str("```\n\n");
                }
            }
        }
    }
    output
}

/// Parse unified diff output and group by file
fn parse_diff_by_file(diff_output: &str, diff_contents: &mut HashMap<String, String>) {
    let mut current_file: Option<String> = None;
    let mut current_diff = String::new();
    for line in diff_output.lines() {
        if line.starts_with("diff --git") {
            if let Some(file) = current_file.take()
                && !current_diff.is_empty()
            {
                diff_contents.insert(file, current_diff.clone());
            }
            current_diff.clear();
            if let Some(b_part) = line.split(" b/").nth(1) {
                current_file = Some(b_part.to_string());
            }
            current_diff.push_str(line);
            current_diff.push('\n');
        } else if current_file.is_some() {
            current_diff.push_str(line);
            current_diff.push('\n');
        }
    }
    if let Some(file) = current_file
        && !current_diff.is_empty()
    {
        diff_contents.insert(file, current_diff);
    }
}

/// Parse git diff --numstat output and add to file_changes map
fn parse_numstat_to_map(output: &str, file_changes: &mut HashMap<String, (i32, i32, GitChangeType)>) {
    for line in output.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 3 {
            let add: i32 = parts[0].parse().unwrap_or(0);
            let del: i32 = parts[1].parse().unwrap_or(0);
            let path = parts[2].to_string();
            let path = if path.contains(" => ") {
                path.split(" => ").last().unwrap_or(&path).trim_end_matches('}').to_string()
            } else {
                path
            };
            let entry = file_changes.entry(path).or_insert((0, 0, GitChangeType::Modified));
            entry.0 += add;
            entry.1 += del;
        }
    }
}

impl Panel for GitPanel {
    fn needs_cache(&self) -> bool {
        true
    }

    fn build_cache_request(&self, ctx: &ContextElement, state: &State) -> Option<CacheRequest> {
        // Force full refresh if cache is explicitly deprecated (e.g., toggle_diffs)
        let current_source_hash = if ctx.cache_deprecated { None } else { ctx.source_hash.clone() };
        Some(CacheRequest::RefreshGitStatus {
            show_diffs: state.git_show_diffs,
            current_source_hash,
            diff_base: state.git_diff_base.clone(),
        })
    }

    fn apply_cache_update(&self, update: CacheUpdate, ctx: &mut ContextElement, state: &mut State) -> bool {
        match update {
            CacheUpdate::GitStatus {
                branch,
                is_repo,
                file_changes,
                branches,
                formatted_content,
                token_count,
                source_hash,
            } => {
                state.git_branch = branch;
                state.git_branches = branches;
                state.git_is_repo = is_repo;
                state.git_file_changes = file_changes
                    .into_iter()
                    .map(|(path, additions, deletions, change_type, diff_content)| GitFileChange {
                        path,
                        additions,
                        deletions,
                        change_type,
                        diff_content,
                    })
                    .collect();
                ctx.source_hash = Some(source_hash);
                ctx.cached_content = Some(formatted_content);
                ctx.full_token_count = token_count;
                ctx.total_pages = compute_total_pages(token_count);
                ctx.current_page = 0;
                // token_count reflects current page, not full content
                if ctx.total_pages > 1 {
                    let page_content = paginate_content(
                        ctx.cached_content.as_deref().unwrap_or(""),
                        ctx.current_page,
                        ctx.total_pages,
                    );
                    ctx.token_count = estimate_tokens(&page_content);
                } else {
                    ctx.token_count = token_count;
                }
                ctx.cache_deprecated = false;
                let content_ref = ctx.cached_content.clone().unwrap_or_default();
                update_if_changed(ctx, &content_ref);
                true
            }
            CacheUpdate::GitStatusUnchanged => {
                ctx.cache_deprecated = false;
                false // No actual content change
            }
            _ => false,
        }
    }

    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        Some(GIT_STATUS_REFRESH_MS)
    }

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
        let base_title =
            if let Some(branch) = &state.git_branch { format!("Git ({})", branch) } else { "Git".to_string() };
        if let Some(ref diff_base) = state.git_diff_base {
            format!("{} [vs {}]", base_title, diff_base)
        } else {
            base_title
        }
    }

    fn refresh(&self, state: &mut State) {
        // Token count is already set by cache system when GitStatus arrives
        // Only recalculate if no cached content exists (shouldn't happen normally)
        let needs_calc = state
            .context
            .iter()
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

    fn refresh_cache(&self, request: CacheRequest) -> Option<CacheUpdate> {
        let CacheRequest::RefreshGitStatus { show_diffs, current_source_hash, diff_base } = request else {
            return None;
        };
        let _guard = crate::profile!("cache::git_status");

        // Check if we're in a git repo (fast check)
        let is_repo =
            Command::new("git").args(["rev-parse", "--git-dir"]).output().map(|o| o.status.success()).unwrap_or(false);

        if !is_repo {
            return Some(CacheUpdate::GitStatus {
                branch: None,
                is_repo: false,
                file_changes: vec![],
                branches: vec![],
                formatted_content: "Not a git repository".to_string(),
                token_count: estimate_tokens("Not a git repository"),
                source_hash: hash_content("not_a_repo"),
            });
        }

        // Get status first for change detection
        let status_output = Command::new("git")
            .args(["status", "--porcelain"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();

        let branch_output = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();

        let new_hash = hash_content(&format!("{}\n{}", branch_output, status_output));

        // If hash unchanged, skip expensive operations
        if current_source_hash.as_ref() == Some(&new_hash) {
            return Some(CacheUpdate::GitStatusUnchanged);
        }

        // Get branch name
        let branch = if branch_output == "HEAD" {
            Command::new("git")
                .args(["rev-parse", "--short", "HEAD"])
                .output()
                .ok()
                .map(|o| format!("detached:{}", String::from_utf8_lossy(&o.stdout).trim()))
        } else if branch_output.is_empty() {
            None
        } else {
            Some(branch_output)
        };

        // Collect per-file changes from status output
        let mut file_changes: HashMap<String, (i32, i32, GitChangeType)> = HashMap::new();

        for line in status_output.lines() {
            if line.len() < 3 {
                continue;
            }
            let x = line.chars().next().unwrap_or(' ');
            let y = line.chars().nth(1).unwrap_or(' ');
            let path = line[3..].trim().to_string();
            let path =
                if path.contains(" -> ") { path.split(" -> ").last().unwrap_or(&path).to_string() } else { path };

            let change_type = match (x, y) {
                ('?', '?') => GitChangeType::Untracked,
                ('A', _) | (_, 'A') => GitChangeType::Added,
                ('D', _) | (_, 'D') => GitChangeType::Deleted,
                ('R', _) | (_, 'R') => GitChangeType::Renamed,
                _ => GitChangeType::Modified,
            };

            file_changes.entry(path).or_insert((0, 0, change_type));
        }

        // Only fetch numstat if we have changes
        if !file_changes.is_empty() {
            if let Some(ref base) = diff_base {
                // When diff_base is set, compare against that ref
                if let Ok(output) = Command::new("git").args(["diff", base, "--numstat"]).output()
                    && output.status.success()
                {
                    parse_numstat_to_map(&String::from_utf8_lossy(&output.stdout), &mut file_changes);
                }
            } else {
                if let Ok(output) = Command::new("git").args(["diff", "--cached", "--numstat"]).output()
                    && output.status.success()
                {
                    parse_numstat_to_map(&String::from_utf8_lossy(&output.stdout), &mut file_changes);
                }
                if let Ok(output) = Command::new("git").args(["diff", "--numstat"]).output()
                    && output.status.success()
                {
                    parse_numstat_to_map(&String::from_utf8_lossy(&output.stdout), &mut file_changes);
                }
            }

            // For untracked files, count lines
            let untracked_files: Vec<String> = file_changes
                .iter()
                .filter(|(_, (add, del, ct))| *ct == GitChangeType::Untracked && *add == 0 && *del == 0)
                .map(|(path, _)| path.clone())
                .collect();
            for path in untracked_files {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let lines = content.lines().count() as i32;
                    if let Some(entry) = file_changes.get_mut(&path) {
                        entry.0 = lines;
                    }
                }
            }

            // For deleted files, get line count from HEAD
            let deleted_files: Vec<String> = file_changes
                .iter()
                .filter(|(_, (add, del, ct))| *ct == GitChangeType::Deleted && *add == 0 && *del == 0)
                .map(|(path, _)| path.clone())
                .collect();
            for path in deleted_files {
                if let Ok(output) = Command::new("git").args(["show", &format!("HEAD:{}", path)]).output()
                    && output.status.success()
                {
                    let content = String::from_utf8_lossy(&output.stdout);
                    let lines = content.lines().count() as i32;
                    if let Some(entry) = file_changes.get_mut(&path) {
                        entry.1 = lines;
                    }
                }
            }
        }

        // Convert to vec and sort by path
        let mut changes: Vec<_> =
            file_changes.into_iter().map(|(path, (add, del, ct))| (path, add, del, ct, String::new())).collect();
        changes.sort_by(|a, b| a.0.cmp(&b.0));

        // Get all local branches
        let branches: Vec<(String, bool)> = Command::new("git")
            .args(["branch", "--format=%(refname:short)"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| {
                let current = branch.as_deref().unwrap_or("");
                String::from_utf8_lossy(&o.stdout).lines().map(|b| (b.to_string(), b == current)).collect()
            })
            .unwrap_or_default();

        // Only fetch diffs if show_diffs is enabled AND we have changes
        if show_diffs && !changes.is_empty() {
            let mut diff_contents: HashMap<String, String> = HashMap::new();

            let diff_ref = diff_base.as_deref().unwrap_or("HEAD");
            if let Ok(output) = Command::new("git").args(["diff", diff_ref]).output()
                && output.status.success()
            {
                let diff_output = String::from_utf8_lossy(&output.stdout);
                parse_diff_by_file(&diff_output, &mut diff_contents);
            }

            // For untracked files, create a pseudo-diff
            for (path, _, _, ct, _) in &changes {
                if *ct == GitChangeType::Untracked
                    && !diff_contents.contains_key(path)
                    && let Ok(content) = std::fs::read_to_string(path)
                {
                    let mut pseudo_diff = format!("diff --git a/{} b/{}\n", path, path);
                    pseudo_diff.push_str("new file\n");
                    pseudo_diff.push_str(&format!("--- /dev/null\n+++ b/{}\n", path));
                    pseudo_diff.push_str("@@ -0,0 +1 @@\n");
                    for line in content.lines() {
                        pseudo_diff.push_str(&format!("+{}\n", line));
                    }
                    diff_contents.insert(path.clone(), pseudo_diff);
                }
            }

            // For deleted files, create a pseudo-diff
            for (path, _, _, ct, _) in &changes {
                if *ct == GitChangeType::Deleted
                    && !diff_contents.contains_key(path)
                    && let Ok(output) = Command::new("git").args(["show", &format!("HEAD:{}", path)]).output()
                    && output.status.success()
                {
                    let content = String::from_utf8_lossy(&output.stdout);
                    let mut pseudo_diff = format!("diff --git a/{} b/{}\n", path, path);
                    pseudo_diff.push_str("deleted file\n");
                    pseudo_diff.push_str(&format!("--- a/{}\n+++ /dev/null\n", path));
                    pseudo_diff.push_str("@@ -1 +0,0 @@\n");
                    for line in content.lines() {
                        pseudo_diff.push_str(&format!("-{}\n", line));
                    }
                    diff_contents.insert(path.clone(), pseudo_diff);
                }
            }

            // Attach diff content to changes
            for (path, _, _, _, diff) in &mut changes {
                if let Some(d) = diff_contents.remove(path) {
                    *diff = d;
                }
            }
        }

        // Generate formatted content for LLM context
        let formatted_content = format_git_content_for_cache(&branch, &changes, show_diffs);
        let token_count = estimate_tokens(&formatted_content);

        Some(CacheUpdate::GitStatus {
            branch,
            is_repo: true,
            file_changes: changes,
            branches,
            formatted_content,
            token_count,
            source_hash: new_hash,
        })
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        if !state.git_is_repo {
            return vec![];
        }

        // Find the Git context element
        let git_ctx = state.context.iter().find(|c| c.context_type == ContextType::Git);

        // Use cached content if available
        let content = git_ctx
            .and_then(|ctx| ctx.cached_content.as_ref())
            .map(|c| {
                let is_deprecated = git_ctx.map(|ctx| ctx.cache_deprecated).unwrap_or(false);
                if is_deprecated { format!("[refreshing...]\n{}", c) } else { c.clone() }
            })
            .unwrap_or_else(|| Self::format_git_for_context(state));

        // Apply pagination
        let (id, last_refresh_ms, current_page, total_pages) = git_ctx
            .map(|c| (c.id.as_str(), c.last_refresh_ms, c.current_page, c.total_pages))
            .unwrap_or(("P6", 0, 0, 1));
        let output = paginate_content(&content, current_page, total_pages);
        vec![ContextItem::new(id, "Git Status", output, last_refresh_ms)]
    }

    fn content(&self, state: &State, base_style: Style) -> Vec<Line<'static>> {
        let _guard = crate::profile!("panel::git::content");
        let mut text: Vec<Line> = Vec::new();

        if !state.git_is_repo {
            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled("Not a git repository".to_string(), Style::default().fg(theme::text_muted()).italic()),
            ]));
            return text;
        }

        // Branch name
        if let Some(branch) = &state.git_branch {
            let branch_color = if branch.starts_with("detached:") { theme::warning() } else { theme::accent() };
            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled("Branch: ".to_string(), Style::default().fg(theme::text_secondary())),
                Span::styled(branch.clone(), Style::default().fg(branch_color).bold()),
            ]));
        }

        // All branches
        if !state.git_branches.is_empty() {
            text.push(Line::from(""));
            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled("Branches:".to_string(), Style::default().fg(theme::text_secondary()).bold()),
            ]));
            for (branch_name, is_current) in &state.git_branches {
                let (prefix, style) = if *is_current {
                    ("* ", Style::default().fg(theme::accent()).bold())
                } else {
                    ("  ", Style::default().fg(theme::text_muted()))
                };
                text.push(Line::from(vec![
                    Span::styled(" ".to_string(), base_style),
                    Span::styled(prefix.to_string(), style),
                    Span::styled(branch_name.clone(), style),
                ]));
            }
        }

        text.push(Line::from(""));

        if state.git_file_changes.is_empty() {
            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled("Working tree clean".to_string(), Style::default().fg(theme::success())),
            ]));
            return text;
        }

        // Calculate column widths
        let path_width = state.git_file_changes.iter().map(|f| f.path.len()).max().unwrap_or(4).clamp(4, 45); // Cap at 45 chars for the panel

        // Table header
        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled("T ".to_string(), Style::default().fg(theme::text_secondary()).bold()),
            Span::styled(
                format!("{:<width$}", "File", width = path_width),
                Style::default().fg(theme::text_secondary()).bold(),
            ),
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
            Span::styled(chars::HORIZONTAL.repeat(path_width + 30), Style::default().fg(theme::border())),
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
                GitChangeType::Added => ("A", theme::success()),
                GitChangeType::Untracked => ("U", theme::success()),
                GitChangeType::Deleted => ("D", theme::error()),
                GitChangeType::Modified => ("M", theme::warning()),
                GitChangeType::Renamed => ("R", theme::accent()),
            };

            // Truncate path if needed
            let display_path = if file.path.len() > path_width {
                format!("...{}", &file.path[file.path.len() - path_width + 3..])
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

            let net_str = if net > 0 { format!("+{}", net) } else { format!("{}", net) };

            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled(format!("{} ", type_char), Style::default().fg(type_color)),
                Span::styled(
                    format!("{:<width$}", display_path, width = path_width),
                    Style::default().fg(theme::text()),
                ),
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
            Span::styled(chars::HORIZONTAL.repeat(path_width + 30), Style::default().fg(theme::border())),
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
        let total_net_str = if total_net > 0 { format!("+{}", total_net) } else { format!("{}", total_net) };

        text.push(Line::from(vec![
            Span::styled(" ".to_string(), base_style),
            Span::styled("  ".to_string(), base_style),
            Span::styled(format!("{:<width$}", "Total", width = path_width), Style::default().fg(theme::text()).bold()),
            Span::styled("  ", base_style),
            Span::styled(format!("{:>6}", format!("+{}", total_add)), Style::default().fg(theme::success()).bold()),
            Span::styled("  ", base_style),
            Span::styled(format!("{:>6}", format!("-{}", total_del)), Style::default().fg(theme::error()).bold()),
            Span::styled("  ", base_style),
            Span::styled(format!("{:>6}", total_net_str), Style::default().fg(total_net_color).bold()),
        ]));

        // Summary stats
        text.push(Line::from(""));
        let added = state.git_file_changes.iter().filter(|f| f.change_type == GitChangeType::Added).count();
        let untracked = state.git_file_changes.iter().filter(|f| f.change_type == GitChangeType::Untracked).count();
        let modified = state.git_file_changes.iter().filter(|f| f.change_type == GitChangeType::Modified).count();
        let deleted = state.git_file_changes.iter().filter(|f| f.change_type == GitChangeType::Deleted).count();
        let renamed = state.git_file_changes.iter().filter(|f| f.change_type == GitChangeType::Renamed).count();

        let mut summary_parts = Vec::new();
        if added > 0 {
            summary_parts.push(format!("{} added", added));
        }
        if untracked > 0 {
            summary_parts.push(format!("{} untracked", untracked));
        }
        if modified > 0 {
            summary_parts.push(format!("{} modified", modified));
        }
        if deleted > 0 {
            summary_parts.push(format!("{} deleted", deleted));
        }
        if renamed > 0 {
            summary_parts.push(format!("{} renamed", renamed));
        }

        if !summary_parts.is_empty() {
            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled(summary_parts.join(", "), Style::default().fg(theme::text_muted())),
            ]));
        }

        // Git log (if enabled)
        if state.git_show_logs {
            text.push(Line::from(""));
            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled(chars::HORIZONTAL.repeat(60), Style::default().fg(theme::border())),
            ]));
            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled("Recent Commits:".to_string(), Style::default().fg(theme::text_secondary()).bold()),
            ]));

            if let Some(log_content) = &state.git_log_content {
                for line in log_content.lines() {
                    text.push(Line::from(vec![
                        Span::styled(" ".to_string(), base_style),
                        Span::styled(line.to_string(), Style::default().fg(theme::text_muted())),
                    ]));
                }
            }
        }

        // Display diff content for each file
        for file in &state.git_file_changes {
            if file.diff_content.is_empty() {
                continue;
            }

            text.push(Line::from(""));
            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled(chars::HORIZONTAL.repeat(60), Style::default().fg(theme::border())),
            ]));

            // Render diff with syntax highlighting
            for line in file.diff_content.lines() {
                let (style, display_line) = if line.starts_with("+++") || line.starts_with("---") {
                    // File header lines
                    (Style::default().fg(theme::text_secondary()).bold(), line.to_string())
                } else if line.starts_with("@@") {
                    // Hunk header
                    (Style::default().fg(theme::accent()), line.to_string())
                } else if line.starts_with('+') && !line.starts_with("+++") {
                    // Addition
                    (Style::default().fg(theme::success()), line.to_string())
                } else if line.starts_with('-') && !line.starts_with("---") {
                    // Deletion
                    (Style::default().fg(theme::error()), line.to_string())
                } else if line.starts_with("diff --git") {
                    // Diff header
                    (Style::default().fg(theme::accent()).bold(), line.to_string())
                } else if line.starts_with("new file") || line.starts_with("deleted file") || line.starts_with("index ")
                {
                    // Meta info
                    (Style::default().fg(theme::text_muted()), line.to_string())
                } else {
                    // Context line (already has leading space in unified diff format)
                    (Style::default().fg(theme::text_muted()), line.to_string())
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

// =============================================================================
// GitResultPanel — dynamic panel for read-only git command results
// =============================================================================

impl Panel for GitResultPanel {
    fn needs_cache(&self) -> bool {
        true
    }

    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        Some(GIT_STATUS_REFRESH_MS)
    }

    fn build_cache_request(&self, ctx: &ContextElement, _state: &State) -> Option<CacheRequest> {
        let command = ctx.result_command.as_ref()?;
        Some(CacheRequest::RefreshGitResult { context_id: ctx.id.clone(), command: command.clone() })
    }

    fn apply_cache_update(&self, update: CacheUpdate, ctx: &mut ContextElement, _state: &mut State) -> bool {
        match update {
            CacheUpdate::Content { content, token_count, .. } => {
                ctx.cached_content = Some(content);
                ctx.full_token_count = token_count;
                ctx.total_pages = compute_total_pages(token_count);
                ctx.current_page = 0;
                if ctx.total_pages > 1 {
                    let page_content = paginate_content(
                        ctx.cached_content.as_deref().unwrap_or(""),
                        ctx.current_page,
                        ctx.total_pages,
                    );
                    ctx.token_count = estimate_tokens(&page_content);
                } else {
                    ctx.token_count = token_count;
                }
                ctx.cache_deprecated = false;
                let content_ref = ctx.cached_content.clone().unwrap_or_default();
                update_if_changed(ctx, &content_ref);
                true
            }
            _ => false,
        }
    }

    fn refresh_cache(&self, request: CacheRequest) -> Option<CacheUpdate> {
        let CacheRequest::RefreshGitResult { context_id, command } = request else {
            return None;
        };

        // Parse and execute the command with timeout
        let args = crate::modules::git::classify::validate_git_command(&command).ok()?;

        let mut cmd = std::process::Command::new("git");
        cmd.args(&args).env("GIT_TERMINAL_PROMPT", "0");
        let output = run_with_timeout(cmd, GIT_CMD_TIMEOUT_SECS);

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                let content = if stderr.trim().is_empty() {
                    stdout.to_string()
                } else if stdout.trim().is_empty() {
                    stderr.to_string()
                } else {
                    format!("{}\n{}", stdout, stderr)
                };
                let content = truncate_output(&content, MAX_RESULT_CONTENT_BYTES);
                let token_count = estimate_tokens(&content);
                Some(CacheUpdate::Content { context_id, content, token_count })
            }
            Err(e) => {
                let content = format!("Error executing git: {}", e);
                let token_count = estimate_tokens(&content);
                Some(CacheUpdate::Content { context_id, content, token_count })
            }
        }
    }

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
        if let Some(ctx) = state.context.get(state.selected_context)
            && ctx.context_type == ContextType::GitResult
            && let Some(cmd) = &ctx.result_command
        {
            let short =
                if cmd.len() > 40 { format!("{}...", &cmd[..cmd.floor_char_boundary(37)]) } else { cmd.clone() };
            return short;
        }
        "Git Result".to_string()
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let mut items = Vec::new();
        for ctx in &state.context {
            if ctx.context_type != ContextType::GitResult {
                continue;
            }
            let content = ctx.cached_content.as_deref().unwrap_or("[loading...]");
            let header = ctx.result_command.as_deref().unwrap_or("Git Result");
            let output = paginate_content(content, ctx.current_page, ctx.total_pages);
            items.push(ContextItem::new(&ctx.id, header, output, ctx.last_refresh_ms));
        }
        items
    }

    fn content(&self, state: &State, base_style: Style) -> Vec<Line<'static>> {
        let mut text: Vec<Line> = Vec::new();

        // Find the selected GitResult panel
        let ctx = state.context.get(state.selected_context).filter(|c| c.context_type == ContextType::GitResult);

        let Some(ctx) = ctx else {
            text.push(Line::from(vec![Span::styled(" No git result panel", Style::default().fg(theme::text_muted()))]));
            return text;
        };

        if let Some(content) = &ctx.cached_content {
            // Render with diff-aware highlighting
            for line in content.lines() {
                let (style, display_line) = if line.starts_with('+') && !line.starts_with("+++") {
                    (Style::default().fg(theme::success()), line.to_string())
                } else if line.starts_with('-') && !line.starts_with("---") {
                    (Style::default().fg(theme::error()), line.to_string())
                } else if line.starts_with("@@") {
                    (Style::default().fg(theme::accent()), line.to_string())
                } else if line.starts_with("diff --git") || line.starts_with("+++") || line.starts_with("---") {
                    (Style::default().fg(theme::text_secondary()).bold(), line.to_string())
                } else if line.starts_with("commit ") {
                    (Style::default().fg(theme::accent()).bold(), line.to_string())
                } else {
                    (Style::default().fg(theme::text()), line.to_string())
                };
                text.push(Line::from(vec![
                    Span::styled(" ".to_string(), base_style),
                    Span::styled(display_line, style),
                ]));
            }
        } else {
            text.push(Line::from(vec![Span::styled(" Loading...", Style::default().fg(theme::text_muted()).italic())]));
        }

        text
    }
}
