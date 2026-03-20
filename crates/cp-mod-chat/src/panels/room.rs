//! Chat room panel — per-room message display with YAML context output.
//!
//! Created by `Chat_open` for each room the AI opens. Shows messages
//! with timestamps, sender names, reply markers, reactions, and media.
//! Auto-refreshes as new messages arrive via the sync loop.

use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::prelude::{Color, Line, Rect, Span, Style};

use cp_base::panels::{CacheRequest, CacheUpdate, ContextItem, Panel, scroll_key_action};
use cp_base::state::actions::Action;
use cp_base::state::context::Entry;
use cp_base::state::runtime::State;

/// Per-room message panel, created dynamically by `Chat_open`.
#[derive(Debug)]
pub(crate) struct ChatRoomPanel;

impl Panel for ChatRoomPanel {
    fn title(&self, _state: &State) -> String {
        "Room".to_string()
    }

    fn content(&self, _state: &State, _base_style: Style) -> Vec<Line<'static>> {
        // Scaffold: empty room panel with placeholder text (§5).
        vec![Line::from(Span::styled(
            "  Room panel — messages will appear here (§5)",
            Style::default().fg(Color::DarkGray),
        ))]
    }

    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        scroll_key_action(key)
    }

    fn needs_cache(&self) -> bool {
        false
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

    fn suicide(&self, _ctx: &Entry, _state: &State) -> bool {
        false
    }

    fn render(&self, _frame: &mut Frame<'_>, _state: &mut State, _area: Rect) {}

    fn refresh(&self, _state: &mut State) {}

    fn context(&self, _state: &State) -> Vec<ContextItem> {
        // Scaffold: minimal YAML stub until §5 populates real messages.
        vec![ContextItem::new("P0", "Room", "room: \"(not connected)\"\nmessages: []\n", 0)]
    }
}
