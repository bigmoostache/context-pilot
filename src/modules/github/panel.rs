use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;

use crate::cache::{CacheRequest, CacheUpdate};
use crate::constants::{GH_CMD_TIMEOUT_SECS, MAX_RESULT_CONTENT_BYTES};
use crate::core::panels::{now_ms, paginate_content, ContextItem, Panel};
use crate::modules::{run_with_timeout, truncate_output};
use crate::actions::Action;
use crate::constants::{SCROLL_ARROW_AMOUNT, SCROLL_PAGE_AMOUNT};
use crate::state::{compute_total_pages, estimate_tokens, ContextElement, ContextType, State};
use crate::ui::theme;

pub(crate) struct GithubResultPanel;

impl Panel for GithubResultPanel {
    fn needs_cache(&self) -> bool { true }

    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        Some(120_000) // Fallback timer; GhWatcher also polls via ETag/hash every 60s
    }

    fn build_cache_request(&self, ctx: &ContextElement, state: &State) -> Option<CacheRequest> {
        let command = ctx.result_command.as_ref()?;
        let token = state.github_token.as_ref()?;
        Some(CacheRequest::RefreshGithubResult {
            context_id: ctx.id.clone(),
            command: command.clone(),
            github_token: token.clone(),
        })
    }

    fn apply_cache_update(&self, update: CacheUpdate, ctx: &mut ContextElement, _state: &mut State) -> bool {
        match update {
            CacheUpdate::GithubResultContent { content, token_count, is_error, .. } => {
                ctx.cached_content = Some(content);
                ctx.full_token_count = token_count;
                ctx.total_pages = compute_total_pages(token_count);
                ctx.current_page = 0;
                if ctx.total_pages > 1 {
                    let page_content = paginate_content(ctx.cached_content.as_deref().unwrap_or(""), ctx.current_page, ctx.total_pages);
                    ctx.token_count = estimate_tokens(&page_content);
                } else {
                    ctx.token_count = token_count;
                }
                ctx.cache_deprecated = false;
                ctx.last_refresh_ms = now_ms();
                let _ = is_error;
                true
            }
            _ => false,
        }
    }

    fn refresh_cache(&self, request: CacheRequest) -> Option<CacheUpdate> {
        let CacheRequest::RefreshGithubResult { context_id, command, github_token } = request else {
            return None;
        };

        // Parse and execute the command with timeout
        let args = crate::modules::github::classify::validate_gh_command(&command).ok()?;

        let mut cmd = std::process::Command::new("gh");
        cmd.args(&args)
            .env("GITHUB_TOKEN", &github_token)
            .env("GH_TOKEN", &github_token)
            .env("GH_PROMPT_DISABLED", "1")
            .env("NO_COLOR", "1");
        let output = run_with_timeout(cmd, GH_CMD_TIMEOUT_SECS);

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
                let is_error = !out.status.success();
                // Redact token if accidentally in output
                let content = if github_token.len() >= 8 && content.contains(&github_token) {
                    content.replace(&github_token, "[REDACTED]")
                } else {
                    content
                };
                let content = truncate_output(&content, MAX_RESULT_CONTENT_BYTES);
                let token_count = estimate_tokens(&content);
                Some(CacheUpdate::GithubResultContent { context_id, content, token_count, is_error })
            }
            Err(e) => {
                let content = format!("Error executing gh: {}", e);
                let token_count = estimate_tokens(&content);
                Some(CacheUpdate::GithubResultContent { context_id, content, token_count, is_error: true })
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
        if let Some(ctx) = state.context.get(state.selected_context) {
            if ctx.context_type == ContextType::GithubResult {
                if let Some(cmd) = &ctx.result_command {
                    let short = if cmd.len() > 40 { format!("{}...", &cmd[..37]) } else { cmd.clone() };
                    return short;
                }
            }
        }
        "GitHub Result".to_string()
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let mut items = Vec::new();
        for ctx in &state.context {
            if ctx.context_type != ContextType::GithubResult {
                continue;
            }
            let content = ctx.cached_content.as_deref().unwrap_or("[loading...]");
            let header = ctx.result_command.as_deref().unwrap_or("GitHub Result");
            let output = paginate_content(content, ctx.current_page, ctx.total_pages);
            items.push(ContextItem::new(&ctx.id, header, output, ctx.last_refresh_ms));
        }
        items
    }

    fn content(&self, state: &State, base_style: Style) -> Vec<Line<'static>> {
        let mut text: Vec<Line> = Vec::new();

        let ctx = state.context.get(state.selected_context)
            .filter(|c| c.context_type == ContextType::GithubResult);

        let Some(ctx) = ctx else {
            text.push(Line::from(vec![
                Span::styled(" No GitHub result panel", Style::default().fg(theme::text_muted())),
            ]));
            return text;
        };

        if let Some(content) = &ctx.cached_content {
            for line in content.lines() {
                // Replace tabs with aligned spacing for readable output
                if line.contains('\t') {
                    let parts: Vec<&str> = line.split('\t').collect();
                    let mut spans = vec![Span::styled(" ".to_string(), base_style)];
                    for (i, part) in parts.iter().enumerate() {
                        let style = match i {
                            0 => Style::default().fg(theme::accent()), // ID / number
                            1 => {
                                // State field (OPEN/CLOSED/MERGED)
                                let color = match part.trim() {
                                    "OPEN" => theme::success(),
                                    "CLOSED" => theme::error(),
                                    "MERGED" => theme::accent(),
                                    _ => theme::text_secondary(),
                                };
                                Style::default().fg(color)
                            },
                            2 => Style::default().fg(theme::text()), // Title
                            _ => Style::default().fg(theme::text_muted()), // Labels, dates, etc.
                        };
                        if i > 0 {
                            spans.push(Span::styled("  ", base_style)); // double-space separator
                        }
                        spans.push(Span::styled(part.to_string(), style));
                    }
                    text.push(Line::from(spans));
                } else {
                    text.push(Line::from(vec![
                        Span::styled(" ".to_string(), base_style),
                        Span::styled(line.to_string(), Style::default().fg(theme::text())),
                    ]));
                }
            }
        } else {
            text.push(Line::from(vec![
                Span::styled(" Loading...", Style::default().fg(theme::text_muted()).italic()),
            ]));
        }

        text
    }
}
