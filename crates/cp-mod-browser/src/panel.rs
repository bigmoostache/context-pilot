//! Browser panel: compact digest + paginated e-ref snapshot table.
//!
//! Heavy state (the snapshot element table) lives here, never inline in the
//! conversation — the token-economy core of the module design.

use std::fmt::Write as _;

use cp_base::panels::{CacheRequest, CacheUpdate, ContextItem, Panel, paginate_content, update_if_changed};
use cp_base::state::context::{Entry, Kind, compute_total_pages, estimate_tokens, make_default_entry};
use cp_base::state::runtime::State;

use crate::BROWSER_KIND;
use crate::types::BrowserState;

/// Create the Browser panel if it doesn't exist yet; mark it dirty otherwise.
pub(crate) fn ensure_panel(state: &mut State) {
    if state.context.iter().any(|c| c.context_type.as_str() == BROWSER_KIND) {
        mark_dirty(state);
        return;
    }
    let panel_id = state.next_available_context_id();
    let uid = format!("UID_{}_P", state.global_next_uid);
    state.global_next_uid = state.global_next_uid.saturating_add(1);
    let mut ctx = make_default_entry(&panel_id, Kind::new(BROWSER_KIND), "Browser", true);
    ctx.uid = Some(uid);
    state.context.push(ctx);
}

/// Remove the Browser panel (used by `browser_close` — Chrome already killed).
pub(crate) fn remove_panel(state: &mut State) {
    state.context.retain(|c| c.context_type.as_str() != BROWSER_KIND);
    state.flags.ui.dirty = true;
}

/// Mark the Browser panel as needing a content refresh.
pub(crate) fn mark_dirty(state: &mut State) {
    cp_base::panels::mark_panels_dirty(state, BROWSER_KIND);
}

/// Build the full panel content from the current `BrowserState`.
fn build_content(state: &State) -> String {
    let bs = BrowserState::get(state);
    let Some(meta) = bs.meta.as_ref() else {
        return "No browser running. Call browser_open.".to_string();
    };
    let status = bs.handle.as_ref().map_or_else(|| "?".to_string(), |h| h.get_status().label());
    let mode = if meta.headless { "headless" } else { "headed" };
    let mut out = format!("Chrome [{mode}] — status: {status} (pid {})\n", meta.pid);
    // Runtime data is worker-written behind the shared lock.
    let Ok(shared) = bs.shared.lock() else {
        out.push_str("\n(browser state momentarily locked)\n");
        return out;
    };
    if !shared.last_action.is_empty() {
        let _w = writeln!(out, "Last action: {}", shared.last_action);
    }
    if shared.snapshot_text.is_empty() {
        out.push_str("\nNo snapshot yet — call browser_snapshot to enumerate interactive elements.\n");
    } else {
        let _w = write!(out, "\nInteractive elements ({} e-refs):\n{}", shared.erefs.len(), shared.snapshot_text);
    }
    out
}

/// Panel implementation for the browser module.
pub(crate) struct BrowserPanel;

impl Panel for BrowserPanel {
    fn title(&self, state: &State) -> String {
        let bs = BrowserState::get(state);
        bs.meta.as_ref().map_or_else(
            || "Browser".to_string(),
            |m| format!("browser ({})", if m.headless { "headless" } else { "headed" }),
        )
    }

    fn blocks(&self, state: &State) -> Vec<cp_render::Block> {
        use cp_render::{Block, Semantic, Span};
        let content = state
            .context
            .get(state.selected_context)
            .and_then(|c| c.cached_content.clone())
            .unwrap_or_else(|| build_content(state));
        content
            .lines()
            .map(|line| {
                if line.starts_with("Chrome ") {
                    Block::Line(vec![Span::styled(line.to_string(), Semantic::Accent)])
                } else if line.starts_with("Last action:") {
                    Block::Line(vec![Span::styled(line.to_string(), Semantic::Info)])
                } else {
                    Block::text(line.to_string())
                }
            })
            .collect()
    }

    fn handle_key(&self, _key: &crossterm::event::KeyEvent, _state: &State) -> Option<cp_base::state::actions::Action> {
        None
    }

    fn needs_cache(&self) -> bool {
        false
    }

    fn refresh(&self, state: &mut State) {
        let content = build_content(state);
        let token_count = estimate_tokens(&content);
        let total_pages = compute_total_pages(token_count);
        for ctx in &mut state.context {
            if ctx.context_type.as_str() == BROWSER_KIND {
                ctx.cached_content = Some(content.clone());
                ctx.token_count = token_count;
                ctx.total_pages = total_pages;
                ctx.cache_deprecated = false;
                let _changed = update_if_changed(ctx, &content);
            }
        }
    }

    fn refresh_cache(&self, _request: CacheRequest) -> Option<CacheUpdate> {
        None
    }

    fn build_cache_request(&self, _ctx: &Entry, _state: &State) -> Option<CacheRequest> {
        None
    }

    fn apply_cache_update(&self, _update: CacheUpdate, _ctx: &mut Entry, _state: &mut State) -> bool {
        false
    }

    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        None
    }

    fn max_freezes(&self) -> u8 {
        0
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        state
            .context
            .iter()
            .filter(|c| c.context_type.as_str() == BROWSER_KIND)
            .filter_map(|c| {
                let content = c.cached_content.as_ref()?;
                let output = paginate_content(content, c.current_page, c.total_pages);
                Some(ContextItem::new(&c.id, "Browser", output, c.last_refresh_ms))
            })
            .collect()
    }

    fn suicide(&self, _ctx: &Entry, _state: &State) -> bool {
        false
    }
}
