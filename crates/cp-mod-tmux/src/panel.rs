use std::process::Command;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::prelude::*;

use super::TMUX_DEPRECATION_MS;
use cp_base::actions::Action;
use cp_base::cache::{CacheRequest, CacheUpdate, hash_content};
use cp_base::constants::{chars, theme};
use cp_base::panels::{ContextItem, Panel, paginate_content, update_if_changed};
use cp_base::state::{ContextElement, ContextType, State, compute_total_pages, estimate_tokens};

pub struct TmuxPanel;

impl Panel for TmuxPanel {
    fn needs_cache(&self) -> bool {
        true
    }

    fn handle_key(&self, key: &KeyEvent, state: &State) -> Option<Action> {
        // Get current tmux pane ID
        let pane_id = state.context.get(state.selected_context).and_then(|c| c.tmux_pane_id.clone())?;

        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        // Convert key to tmux send-keys format
        // Note: Tab is reserved for panel switching, not sent to tmux
        let keys = match key.code {
            KeyCode::Char(c) if ctrl => format!("C-{}", c),
            KeyCode::Char(c) => c.to_string(),
            KeyCode::Enter => "Enter".to_string(),
            KeyCode::Backspace => "BSpace".to_string(),
            KeyCode::Esc => "Escape".to_string(),
            KeyCode::Up => "Up".to_string(),
            KeyCode::Down => "Down".to_string(),
            KeyCode::Left => "Left".to_string(),
            KeyCode::Right => "Right".to_string(),
            KeyCode::Home => "Home".to_string(),
            KeyCode::End => "End".to_string(),
            KeyCode::Delete => "DC".to_string(),
            _ => return None, // Let global handle (Tab, PageUp/Down, etc.)
        };

        Some(Action::TmuxSendKeys { pane_id, keys })
    }
    fn title(&self, state: &State) -> String {
        if let Some(ctx) = state.context.get(state.selected_context) {
            let pane_id = ctx.tmux_pane_id.as_deref().unwrap_or("?");
            format!("tmux {}", pane_id)
        } else {
            "Tmux".to_string()
        }
    }

    fn build_cache_request(&self, ctx: &ContextElement, _state: &State) -> Option<CacheRequest> {
        let pane_id = ctx.tmux_pane_id.as_ref()?;
        Some(CacheRequest::Tmux {
            context_id: ctx.id.clone(),
            pane_id: pane_id.clone(),
            lines: ctx.tmux_lines,
            current_source_hash: ctx.source_hash.clone(),
        })
    }

    fn apply_cache_update(&self, update: CacheUpdate, ctx: &mut ContextElement, _state: &mut State) -> bool {
        let CacheUpdate::Content { content, token_count, .. } = update else {
            return false;
        };
        ctx.source_hash = Some(hash_content(&content));
        ctx.cached_content = Some(content);
        ctx.token_count = token_count;
        ctx.total_pages = compute_total_pages(token_count);
        ctx.current_page = 0;
        ctx.cache_deprecated = false;
        let content_ref = ctx.cached_content.clone().unwrap_or_default();
        update_if_changed(ctx, &content_ref);
        true
    }

    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        Some(TMUX_DEPRECATION_MS)
    }

    fn refresh(&self, _state: &mut State) {
        // Tmux refresh is handled by background cache system via refresh_cache
    }

    fn refresh_cache(&self, request: CacheRequest) -> Option<CacheUpdate> {
        let CacheRequest::Tmux { context_id, pane_id, lines, current_source_hash } = request else {
            return None;
        };
        let start_line = format!("-{}", lines.unwrap_or(50));
        let output =
            Command::new("tmux").args(["capture-pane", "-p", "-S", &start_line, "-t", &pane_id]).output().ok()?;
        if !output.status.success() {
            return None;
        }
        let content = String::from_utf8_lossy(&output.stdout).to_string();
        let new_hash = hash_content(&content);
        if current_source_hash.as_ref() == Some(&new_hash) {
            return Some(CacheUpdate::Unchanged { context_id });
        }
        let token_count = estimate_tokens(&content);
        Some(CacheUpdate::Content { context_id, content, token_count })
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        state
            .context
            .iter()
            .filter(|c| c.context_type == ContextType::TMUX)
            .filter_map(|c| {
                let pane_id = c.tmux_pane_id.as_ref()?;
                // Use cached content only - no blocking operations
                let content = c.cached_content.as_ref()?;
                let output = paginate_content(content, c.current_page, c.total_pages);
                let desc = c.tmux_description.as_deref().unwrap_or("");
                let header = if desc.is_empty() {
                    format!("Tmux Pane {}", pane_id)
                } else {
                    format!("Tmux Pane {} ({})", pane_id, desc)
                };
                Some(ContextItem::new(&c.id, header, output, c.last_refresh_ms))
            })
            .collect()
    }

    fn content(&self, state: &State, base_style: Style) -> Vec<Line<'static>> {
        let (content, description, last_keys) = if let Some(ctx) = state.context.get(state.selected_context) {
            // Use cached content only - no blocking operations
            let content = ctx.cached_content.as_ref().cloned().unwrap_or_else(|| {
                if ctx.cache_deprecated { "Loading...".to_string() } else { "No content".to_string() }
            });
            let desc = ctx.tmux_description.clone().unwrap_or_default();
            let last = ctx.tmux_last_keys.clone();
            (content, desc, last)
        } else {
            (String::new(), String::new(), None)
        };

        let mut text: Vec<Line> = Vec::new();

        if !description.is_empty() {
            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled(description, Style::default().fg(theme::text_muted()).italic()),
            ]));
        }
        if let Some(ref keys) = last_keys {
            text.push(Line::from(vec![
                Span::styled(" last: ".to_string(), Style::default().fg(theme::text_muted())),
                Span::styled(keys.clone(), Style::default().fg(theme::accent_dim())),
            ]));
        }
        if !text.is_empty() {
            text.push(Line::from(vec![Span::styled(
                format!(" {}", chars::HORIZONTAL.repeat(40)),
                Style::default().fg(theme::border()),
            )]));
        }

        for line in content.lines() {
            text.push(Line::from(vec![
                Span::styled(" ".to_string(), base_style),
                Span::styled(line.to_string(), Style::default().fg(theme::text())),
            ]));
        }

        text
    }
}
