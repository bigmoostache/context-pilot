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

mod conversation;
mod file;
mod git;
mod glob;
mod grep;
mod memory;
mod overview;
mod system;
mod tmux;
mod todo;
mod tree;

use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crossterm::event::KeyEvent;

use crate::actions::Action;
use crate::state::{ContextType, State};
use crate::ui::{theme, helpers::count_wrapped_lines};

/// Get current time in milliseconds since UNIX epoch
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// Re-export panels
pub use conversation::ConversationPanel;
pub use file::FilePanel;
pub use git::GitPanel;
pub use glob::GlobPanel;
pub use grep::GrepPanel;
pub use memory::MemoryPanel;
pub use overview::OverviewPanel;
pub use system::SystemPanel;
pub use tmux::TmuxPanel;
pub use todo::TodoPanel;
pub use tree::TreePanel;

/// A single context item to be sent to the LLM
#[derive(Debug, Clone)]
pub struct ContextItem {
    /// Context element ID (e.g., "P7", "P8") for LLM reference
    pub id: String,
    /// Header/title for this context (e.g., "File: src/main.rs" or "Todo List")
    pub header: String,
    /// The actual content
    pub content: String,
}

impl ContextItem {
    pub fn new(id: impl Into<String>, header: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            header: header.into(),
            content: content.into(),
        }
    }

    /// Format this context item for the LLM
    pub fn format(&self) -> String {
        format!("=== [{}] {} ===\n{}\n=== End of [{}] {} ===",
            self.id, self.header, self.content, self.id, self.header)
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

    /// Refresh token counts and any cached data (called before generating context)
    fn refresh(&self, _state: &mut State) {
        // Default: no refresh needed
    }

    /// Generate context items to send to the LLM
    /// Returns empty vec if this panel doesn't contribute to LLM context
    fn context(&self, _state: &State) -> Vec<ContextItem> {
        Vec::new()
    }

    /// Render the panel to the frame (default implementation)
    fn render(&self, frame: &mut Frame, state: &mut State, area: Rect) {
        let base_style = Style::default().bg(theme::BG_SURFACE);
        let title = self.title(state);

        let inner_area = Rect::new(
            area.x + 1,
            area.y,
            area.width.saturating_sub(2),
            area.height
        );

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(ratatui::widgets::BorderType::Rounded)
            .border_style(Style::default().fg(theme::BORDER))
            .style(base_style)
            .title(Span::styled(format!(" {} ", title), Style::default().fg(theme::ACCENT).bold()));

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

/// Get the appropriate panel for a context type
pub fn get_panel(context_type: ContextType) -> Box<dyn Panel> {
    match context_type {
        ContextType::System => Box::new(SystemPanel),
        ContextType::Conversation => Box::new(ConversationPanel),
        ContextType::File => Box::new(FilePanel),
        ContextType::Tree => Box::new(TreePanel),
        ContextType::Glob => Box::new(GlobPanel),
        ContextType::Grep => Box::new(GrepPanel),
        ContextType::Tmux => Box::new(TmuxPanel),
        ContextType::Todo => Box::new(TodoPanel),
        ContextType::Memory => Box::new(MemoryPanel),
        ContextType::Overview => Box::new(OverviewPanel),
        ContextType::Git => Box::new(GitPanel),
    }
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

