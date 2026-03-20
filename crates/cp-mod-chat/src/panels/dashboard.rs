//! Chat dashboard panel — always-on overview of rooms, server, bridges.
//!
//! Created automatically when the chat module activates. Shows room list
//! sorted by last activity, server status, bridge health indicators, and
//! an optional cross-room search section.

use std::fmt::Write as _;

use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::prelude::{Color, Line, Rect, Span, Style};

use cp_base::panels::{CacheRequest, CacheUpdate, ContextItem, Panel, scroll_key_action};
use cp_base::state::actions::Action;
use cp_base::state::context::{Entry, Kind, estimate_tokens};
use cp_base::state::runtime::State;

use crate::types::ChatState;

/// Fixed panel showing the chat module overview.
#[derive(Debug)]
pub(crate) struct ChatDashboardPanel;

impl ChatDashboardPanel {
    /// Build YAML context string for the LLM.
    fn build_context(state: &State) -> String {
        let cs = ChatState::get(state);
        let mut out = String::new();
        Self::write_server_status(&mut out, cs);
        Self::write_room_list(&mut out, cs);
        Self::write_search_results(&mut out, cs);
        out
    }

    /// Append the server status YAML block.
    fn write_server_status(out: &mut String, cs: &ChatState) {
        let status = match &cs.server_status {
            crate::types::ServerStatus::Stopped => "stopped",
            crate::types::ServerStatus::Starting => "starting",
            crate::types::ServerStatus::Running => "running",
            crate::types::ServerStatus::Error(e) => e.as_str(),
        };
        {
            let _r = writeln!(out, "server:\n  status: {status}\n  address: \"localhost:6167\"");
        }
    }

    /// Append the room list YAML block.
    fn write_room_list(out: &mut String, cs: &ChatState) {
        if cs.rooms.is_empty() {
            return;
        }
        out.push_str("rooms:\n");
        for room in &cs.rooms {
            Self::write_room_entry(out, room);
        }
    }

    /// Append a single room entry to the YAML output.
    fn write_room_entry(out: &mut String, room: &crate::types::RoomInfo) {
        {
            let _r = writeln!(out, "  - name: \"{}\"", room.display_name);
        }
        if let Some(ref bridge) = room.bridge_source {
            let _r = writeln!(out, "    bridge: {}", bridge.label());
        }
        {
            let _r = writeln!(out, "    unread: {}", room.unread_count);
        }
        if let Some(ref msg) = room.last_message {
            let _r =
                writeln!(out, "    last_message: \"{}: {}\"", msg.sender_display_name, truncate_body(&msg.body, 80),);
        }
    }

    /// Append the search results YAML block (if a search is active).
    fn write_search_results(out: &mut String, cs: &ChatState) {
        let Some(ref query) = cs.search_query else {
            return;
        };
        {
            let _r = writeln!(out, "search:\n  query: \"{query}\"");
        }
        if cs.search_results.is_empty() {
            return;
        }
        out.push_str("  results:\n");
        for sr in &cs.search_results {
            let _r = writeln!(
                out,
                "    - room: \"{}\"\n      sender: {}\n      body: \"{}\"",
                sr.room_name,
                sr.sender,
                truncate_body(&sr.body, 120),
            );
        }
    }
}

impl Panel for ChatDashboardPanel {
    fn title(&self, _state: &State) -> String {
        "Chat".to_string()
    }

    fn content(&self, state: &State, _base_style: Style) -> Vec<Line<'static>> {
        let cs = ChatState::get(state);

        let status_label = match &cs.server_status {
            crate::types::ServerStatus::Stopped => Span::styled("● Stopped", Style::default().fg(Color::Red)),
            crate::types::ServerStatus::Starting => Span::styled("● Starting", Style::default().fg(Color::Yellow)),
            crate::types::ServerStatus::Running => Span::styled("● Running", Style::default().fg(Color::Green)),
            crate::types::ServerStatus::Error(e) => {
                Span::styled(format!("● Error: {e}"), Style::default().fg(Color::Red))
            }
        };

        let mut lines = vec![Line::from(vec![Span::raw("  Server: "), status_label]), Line::from("")];

        if cs.rooms.is_empty() {
            lines.push(Line::from(Span::styled("  No rooms yet", Style::default().fg(Color::DarkGray))));
        } else {
            for room in &cs.rooms {
                let unread =
                    if room.unread_count > 0 { format!("  {} unread", room.unread_count) } else { String::new() };
                let bridge = room.bridge_source.as_ref().map_or(String::new(), |b| format!(" ({})", b.label()));
                lines.push(Line::from(format!("  {}{}{unread}", room.display_name, bridge)));
            }
        }

        lines
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

    fn refresh(&self, state: &mut State) {
        // Drain sync events from the async loop into ChatState + fire Spine notifications
        let _changed = crate::sync::drain_sync_events(state);

        // Sync room list from the Matrix SDK into ChatState
        let rooms = crate::client::fetch_room_list();
        if !rooms.is_empty() {
            let cs = ChatState::get_mut(state);
            cs.rooms = rooms;
        }

        let content = Self::build_context(state);
        let token_count = estimate_tokens(&content);

        for ctx in &mut state.context {
            if ctx.context_type.as_str() == Kind::CHAT_DASHBOARD {
                ctx.token_count = token_count;
                let _ = cp_base::panels::update_if_changed(ctx, &content);
                break;
            }
        }
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let content = Self::build_context(state);
        let (id, last_refresh_ms) = state
            .context
            .iter()
            .find(|c| c.context_type.as_str() == Kind::CHAT_DASHBOARD)
            .map_or(("P0", 0), |c| (c.id.as_str(), c.last_refresh_ms));
        vec![ContextItem::new(id, "Chat", content, last_refresh_ms)]
    }
}

/// Truncate a message body to `max_len` characters, appending `…` if cut.
#[must_use]
fn truncate_body(body: &str, max_len: usize) -> String {
    if body.len() <= max_len {
        body.to_string()
    } else {
        let boundary = body.floor_char_boundary(max_len.saturating_sub(1));
        format!("{}…", body.get(..boundary).unwrap_or(""))
    }
}
