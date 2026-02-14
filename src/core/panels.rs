//! Panel trait and implementations for different context types.
//!
//! Each panel type implements the Panel trait, providing a consistent
//! interface for rendering AND context generation for the LLM.
//!
//! ## Caching Architecture
//!
//! Panels use a two-level caching system:
//! - `cache_deprecated`: Source data changed, cache needs regeneration
//! - `cached_content`: The actual cached content string
//!
//! When `refresh()` is called:
//! 1. Check if cache is deprecated (or missing)
//! 2. If so, regenerate cache from source data
//! 3. Update token count from cached content
//!
//! `context()` returns the cached content without regenerating.

use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crossterm::event::KeyEvent;

use crate::actions::Action;
use crate::cache::{CacheRequest, CacheUpdate};
use crate::state::{ContextElement, ContextType, State};
use crate::ui::{theme, helpers::count_wrapped_lines};

/// Get current time in milliseconds since UNIX epoch
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Update last_refresh_ms only if content actually changed (hash differs).
/// Returns true if content changed.
pub fn update_if_changed(ctx: &mut crate::state::ContextElement, content: &str) -> bool {
    let new_hash = crate::cache::hash_content(content);
    if ctx.content_hash.as_deref() == Some(&new_hash) {
        return false;
    }
    ctx.content_hash = Some(new_hash);
    ctx.last_refresh_ms = now_ms();
    true
}

/// Mark all panels of a given context type as cache-deprecated (dirty).
/// Also sets `state.dirty = true` so the UI re-renders.
pub fn mark_panels_dirty(state: &mut crate::state::State, context_type: crate::state::ContextType) {
    for ctx in &mut state.context {
        if ctx.context_type == context_type {
            ctx.cache_deprecated = true;
        }
    }
    state.dirty = true;
}

/// Paginate content for LLM context output.
/// Returns the original content unchanged when total_pages <= 1.
/// Otherwise slices by approximate token offset, snaps to line boundaries,
/// and prepends a page header.
pub fn paginate_content(full_content: &str, current_page: usize, total_pages: usize) -> String {
    use crate::constants::{PANEL_PAGE_TOKENS, CHARS_PER_TOKEN};

    if total_pages <= 1 {
        return full_content.to_string();
    }

    let chars_per_page = PANEL_PAGE_TOKENS as f32 * CHARS_PER_TOKEN;
    let start_char = (current_page as f32 * chars_per_page) as usize;

    // Snap start to next line boundary
    let start = if start_char == 0 {
        0
    } else if start_char >= full_content.len() {
        full_content.len()
    } else {
        // Find next newline after start_char
        full_content[start_char..].find('\n')
            .map(|pos| start_char + pos + 1)
            .unwrap_or(full_content.len())
    };

    let end_char = start + chars_per_page as usize;
    let end = if end_char >= full_content.len() {
        full_content.len()
    } else {
        // Find next newline after end_char to snap to line boundary
        full_content[end_char..].find('\n')
            .map(|pos| end_char + pos + 1)
            .unwrap_or(full_content.len())
    };

    let page_content = &full_content[start..end];
    format!(
        "[Page {}/{} — use panel_goto_page to navigate]\n{}",
        current_page + 1,
        total_pages,
        page_content
    )
}

/// A single context item to be sent to the LLM
#[derive(Debug, Clone)]
pub struct ContextItem {
    /// Context element ID (e.g., "P7", "P8") for LLM reference
    pub id: String,
    /// Header/title for this context (e.g., "File: src/main.rs" or "Todo List")
    pub header: String,
    /// The actual content
    pub content: String,
    /// Last refresh timestamp in milliseconds since UNIX epoch (for sorting panels)
    pub last_refresh_ms: u64,
}

impl ContextItem {
    pub fn new(
        id: impl Into<String>,
        header: impl Into<String>,
        content: impl Into<String>,
        last_refresh_ms: u64,
    ) -> Self {
        Self {
            id: id.into(),
            header: header.into(),
            content: content.into(),
            last_refresh_ms,
        }
    }
}

/// Trait for all panel types
pub trait Panel {
    /// Generate the panel's title for display
    fn title(&self, state: &State) -> String;

    /// Generate the panel's content lines for rendering (uses 'static since we create owned data)
    fn content(&self, state: &State, base_style: Style) -> Vec<Line<'static>>;

    /// Handle keyboard input for this panel
    /// Returns None to use default handling, Some(action) to override
    fn handle_key(&self, _key: &KeyEvent, _state: &State) -> Option<Action> {
        None // Default: use global key handling
    }

    /// Whether this panel uses background caching (cached_content from background loading)
    fn needs_cache(&self) -> bool { false }

    /// Refresh token counts and any cached data (called before generating context)
    fn refresh(&self, _state: &mut State) {
        // Default: no refresh needed
    }

    /// Compute a cache update for this panel in the background.
    /// Called from a background thread — implementations should do blocking I/O here.
    /// Returns None if no update is needed (e.g., content unchanged).
    fn refresh_cache(&self, _request: CacheRequest) -> Option<CacheUpdate> {
        None
    }

    /// Build a cache request for the given context element.
    /// Returns None for panels without background caching.
    fn build_cache_request(&self, _ctx: &ContextElement, _state: &State) -> Option<CacheRequest> {
        None
    }

    /// Apply a cache update to the context element and state.
    /// Returns true if content changed (caller sets state.dirty).
    fn apply_cache_update(&self, _update: CacheUpdate, _ctx: &mut ContextElement, _state: &mut State) -> bool {
        false
    }

    /// Timer interval in ms for auto-refresh. None = no timer (uses watchers or no refresh).
    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        None
    }

    /// Generate context items to send to the LLM
    /// Returns empty vec if this panel doesn't contribute to LLM context
    fn context(&self, _state: &State) -> Vec<ContextItem> {
        Vec::new()
    }

    /// Render the panel to the frame (default implementation)
    fn render(&self, frame: &mut Frame, state: &mut State, area: Rect) {
        let base_style = Style::default().bg(theme::bg_surface());
        let title = self.title(state);

        let inner_area = Rect::new(
            area.x + 1,
            area.y,
            area.width.saturating_sub(2),
            area.height
        );

        // Build bottom title for dynamic panels: "refreshed Xs ago"
        let bottom_title = state.context.get(state.selected_context)
            .filter(|ctx| !ctx.context_type.is_fixed())
            .and_then(|ctx| {
                let ts = ctx.last_refresh_ms;
                if ts < 1577836800000 { return None; } // invalid timestamp
                let now = now_ms();
                if now <= ts { return None; }
                Some(format!(" {} ", crate::ui::helpers::format_time_ago(now - ts)))
            });

        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_type(ratatui::widgets::BorderType::Rounded)
            .border_style(Style::default().fg(theme::border()))
            .style(base_style)
            .title(Span::styled(format!(" {} ", title), Style::default().fg(theme::accent()).bold()));

        if let Some(ref bottom) = bottom_title {
            block = block.title_bottom(
                Span::styled(bottom, Style::default().fg(theme::text_muted()))
            );
        }

        let content_area = block.inner(inner_area);
        frame.render_widget(block, inner_area);

        let text = self.content(state, base_style);

        // Calculate and set max scroll (accounting for wrapped lines)
        let viewport_width = content_area.width as usize;
        let viewport_height = content_area.height as usize;
        let content_height: usize = {
            let _guard = crate::profile!("panel::scroll_calc");
            text.iter()
                .map(|line| count_wrapped_lines(line, viewport_width))
                .sum()
        };
        let max_scroll = content_height.saturating_sub(viewport_height) as f32;
        state.max_scroll = max_scroll;
        state.scroll_offset = state.scroll_offset.clamp(0.0, max_scroll);

        let paragraph = {
            let _guard = crate::profile!("panel::paragraph_new");
            Paragraph::new(text)
                .style(base_style)
                .wrap(Wrap { trim: false })
                .scroll((state.scroll_offset.round() as u16, 0))
        };

        {
            let _guard = crate::profile!("panel::frame_render");
            frame.render_widget(paragraph, content_area);
        }
    }
}

/// Get the appropriate panel for a context type (delegates to module system)
pub fn get_panel(context_type: ContextType) -> Box<dyn Panel> {
    crate::modules::create_panel(context_type)
        .unwrap_or_else(|| panic!("No module provides a panel for {:?}", context_type))
}

/// Refresh all panels (update token counts, etc.)
pub fn refresh_all_panels(state: &mut State) {
    // Get unique context types from state
    let context_types: Vec<ContextType> = state.context.iter()
        .map(|c| c.context_type)
        .collect();

    for context_type in context_types {
        let panel = get_panel(context_type);
        panel.refresh(state);
    }
}

/// Collect all context items from all panels
pub fn collect_all_context(state: &State) -> Vec<ContextItem> {
    let mut items = Vec::new();

    // Get UNIQUE context types from state (dedup to avoid multiplying items!)
    let mut seen = std::collections::HashSet::new();
    let context_types: Vec<ContextType> = state.context.iter()
        .map(|c| c.context_type)
        .filter(|ct| seen.insert(*ct))
        .collect();

    for context_type in context_types {
        let panel = get_panel(context_type);
        items.extend(panel.context(state));
    }

    items
}
