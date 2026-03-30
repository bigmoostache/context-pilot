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

use crate::types::{ChatState, RoomInfo, ServerStatus};

/// Fixed panel showing the chat module overview.
#[derive(Debug)]
pub(crate) struct ChatDashboardPanel;

impl ChatDashboardPanel {
    /// Build YAML context string for the LLM.
    fn build_context(state: &State) -> String {
        let cs = ChatState::get(state);
        let mut out = String::with_capacity(1024);
        Self::write_server_status(&mut out, cs);
        Self::write_room_list(&mut out, cs);
        Self::write_search_results(&mut out, cs);
        out
    }

    /// Append the server status YAML block.
    fn write_server_status(out: &mut String, cs: &ChatState) {
        let status = match &cs.server_status {
            ServerStatus::Stopped => "stopped",
            ServerStatus::Starting => "starting",
            ServerStatus::Running => "running",
            ServerStatus::Error(e) => e.as_str(),
        };
        let addr = crate::server::server_addr();
        {
            let _r = writeln!(out, "server:\n  status: {status}\n  address: \"{addr}\"");
        }
        if let Some(ref uid) = cs.bot_user_id {
            let _r = writeln!(out, "  bot: \"{uid}\"");
        }
    }

    /// Append the room list YAML block, sorted by last activity.
    fn write_room_list(out: &mut String, cs: &ChatState) {
        if cs.rooms.is_empty() {
            return;
        }
        let mut sorted: Vec<&RoomInfo> = cs.rooms.iter().collect();
        sorted.sort_by(|a, b| {
            let ts_a = a.last_message.as_ref().map_or(0, |m| m.timestamp);
            let ts_b = b.last_message.as_ref().map_or(0, |m| m.timestamp);
            ts_b.cmp(&ts_a)
        });

        // Summary line
        let total_unread: u64 = cs.rooms.iter().map(|r| r.unread_count).sum();
        let bridged = cs.rooms.iter().filter(|r| r.bridge_source.is_some()).count();
        {
            let _r = writeln!(out, "rooms: {} total, {} bridged, {} unread", cs.rooms.len(), bridged, total_unread,);
        }

        // Table header
        out.push_str("  | Room | Platform | Members | Unread | Last Message | Time |\n");
        out.push_str("  |------|----------|---------|--------|--------------|------|\n");
        for room in &sorted {
            Self::write_room_row(out, room);
        }
    }

    /// Append a single room row to the table.
    fn write_room_row(out: &mut String, room: &RoomInfo) {
        let platform = room.bridge_source.map_or("Matrix", |b| b.label());
        let unread = if room.unread_count > 0 { room.unread_count.to_string() } else { "-".to_string() };
        let (last_msg, last_time) = room.last_message.as_ref().map_or_else(
            || ("-".to_string(), "-".to_string()),
            |msg| {
                let preview = format!("{}: {}", msg.sender_display_name, truncate_body(&msg.body, 50));
                let time = format_timestamp_short(msg.timestamp);
                (preview, time)
            },
        );
        let _r = writeln!(
            out,
            "  | {} | {} | {} | {} | {} | {} |",
            truncate_body(&room.display_name, 20),
            platform,
            room.member_count,
            unread,
            last_msg,
            last_time,
        );
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
            out.push_str("  results: []\n");
            return;
        }
        out.push_str("  results:\n");
        for sr in &cs.search_results {
            let _r = writeln!(
                out,
                "    - room: \"{}\"\n      sender: \"{}\"\n      body: \"{}\"",
                sr.room_name,
                sr.sender,
                truncate_body(&sr.body, 120),
            );
        }
    }

    /// Build the TUI render lines for the room list.
    fn render_room_lines(cs: &ChatState) -> Vec<Line<'static>> {
        let mut lines = Vec::new();

        if cs.rooms.is_empty() {
            lines.push(Line::from(Span::styled(
                "  No rooms yet — use Chat_create_room or bridge a platform",
                Style::default().fg(Color::DarkGray),
            )));
            return lines;
        }

        // Sort by last activity (newest first)
        let mut sorted: Vec<&RoomInfo> = cs.rooms.iter().collect();
        sorted.sort_by(|a, b| {
            let ts_a = a.last_message.as_ref().map_or(0, |m| m.timestamp);
            let ts_b = b.last_message.as_ref().map_or(0, |m| m.timestamp);
            ts_b.cmp(&ts_a)
        });

        lines.push(Line::from(Span::styled("  Rooms", Style::default().fg(Color::White))));
        lines.push(Line::from(""));

        for room in &sorted {
            let mut spans = Vec::new();

            // Unread indicator
            if room.unread_count > 0 {
                spans.push(Span::styled(format!("  ● {}", room.display_name), Style::default().fg(Color::Yellow)));
                spans.push(Span::styled(format!(" ({})", room.unread_count), Style::default().fg(Color::Yellow)));
            } else {
                spans.push(Span::styled(format!("  ○ {}", room.display_name), Style::default().fg(Color::Gray)));
            }

            // Bridge badge
            if let Some(ref bridge) = room.bridge_source {
                spans.push(Span::styled(format!(" [{}]", bridge.label()), Style::default().fg(Color::Cyan)));
            }

            // DM indicator
            if room.is_direct {
                spans.push(Span::styled(" DM", Style::default().fg(Color::DarkGray)));
            }

            // Encryption badge
            if room.encrypted {
                spans.push(Span::styled(" 🔒", Style::default()));
            }

            lines.push(Line::from(spans));

            // Last message preview (indented)
            if let Some(ref msg) = room.last_message {
                let preview = format!("    {} — {}", msg.sender_display_name, truncate_body(&msg.body, 60),);
                lines.push(Line::from(Span::styled(preview, Style::default().fg(Color::DarkGray))));
            }
        }

        lines
    }

    /// Build search result render lines.
    fn render_search_lines(cs: &ChatState) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let Some(ref query) = cs.search_query else {
            return lines;
        };

        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  Search: ", Style::default().fg(Color::White)),
            Span::styled(format!("\"{query}\""), Style::default().fg(Color::Yellow)),
        ]));

        if cs.search_results.is_empty() {
            lines.push(Line::from(Span::styled("  No results", Style::default().fg(Color::DarkGray))));
        } else {
            for sr in &cs.search_results {
                lines.push(Line::from(vec![
                    Span::styled(format!("  [{}] ", sr.room_name), Style::default().fg(Color::Cyan)),
                    Span::styled(sr.sender.clone(), Style::default().fg(Color::White)),
                    Span::styled(format!(": {}", truncate_body(&sr.body, 60)), Style::default().fg(Color::Gray)),
                ]));
            }
        }

        lines
    }
}

impl Panel for ChatDashboardPanel {
    fn title(&self, _state: &State) -> String {
        "Chat".to_string()
    }

    fn content(&self, state: &State, _base_style: Style) -> Vec<Line<'static>> {
        let cs = ChatState::get(state);

        let status_line = match &cs.server_status {
            ServerStatus::Stopped => {
                Line::from(vec![Span::raw("  Server: "), Span::styled("● Stopped", Style::default().fg(Color::Red))])
            }
            ServerStatus::Starting => Line::from(vec![
                Span::raw("  Server: "),
                Span::styled("● Starting…", Style::default().fg(Color::Yellow)),
            ]),
            ServerStatus::Running => {
                let addr = crate::server::server_addr();
                let mut spans = vec![
                    Span::raw("  Server: "),
                    Span::styled("● Running", Style::default().fg(Color::Green)),
                    Span::styled(format!(" ({addr})"), Style::default().fg(Color::DarkGray)),
                ];
                if let Some(ref uid) = cs.bot_user_id {
                    spans.push(Span::styled(format!("  as {uid}"), Style::default().fg(Color::DarkGray)));
                }
                Line::from(spans)
            }
            ServerStatus::Error(e) => Line::from(vec![
                Span::raw("  Server: "),
                Span::styled(format!("● Error: {e}"), Style::default().fg(Color::Red)),
            ]),
        };

        let mut lines = vec![status_line, Line::from("")];

        // Room list (sorted by activity)
        lines.extend(Self::render_room_lines(cs));

        // Search results (if active)
        lines.extend(Self::render_search_lines(cs));

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
        let _changed = crate::client::sync::drain_sync_events(state);

        // Refresh room list from the Matrix SDK
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

/// Format a Unix timestamp (ms) to short HH:MM display.
fn format_timestamp_short(timestamp_ms: u64) -> String {
    let secs = cp_base::panels::time_arith::ms_to_secs(timestamp_ms);
    let secs_i64 = i64::try_from(secs).unwrap_or(i64::MAX);
    chrono::DateTime::from_timestamp(secs_i64, 0)
        .map_or_else(|| "??:??".to_string(), |dt| dt.format("%H:%M").to_string())
}
