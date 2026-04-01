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
        let sock = crate::server::global_socket_path()
            .map_or_else(|| "unknown".to_string(), |p| p.to_string_lossy().to_string());
        {
            let _r = writeln!(out, "server:\n  status: {status}\n  socket: \"{sock}\"");
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
        out.push_str("  | ID | Room | Platform | Members | Unread | Last Message | Time |\n");
        out.push_str("  |----|------|----------|---------|--------|--------------|------|\n");
        for room in &sorted {
            let ref_str = cs.room_id_to_ref.get(&room.room_id).map_or("-", String::as_str);
            Self::write_room_row(out, room, ref_str);
        }
    }

    /// Append a single room row to the table.
    fn write_room_row(out: &mut String, room: &RoomInfo, ref_str: &str) {
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
            "  | {} | {} | {} | {} | {} | {} | {} |",
            ref_str,
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

    /// Build the TUI render lines for the room list as a table.
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

        // Summary header
        let total_unread: u64 = cs.rooms.iter().map(|r| r.unread_count).sum();
        let bridged = cs.rooms.iter().filter(|r| r.bridge_source.is_some()).count();
        lines.push(Line::from(vec![
            Span::styled("  Rooms ", Style::default().fg(Color::White)),
            Span::styled(
                format!("{} total, {} bridged, {} unread", cs.rooms.len(), bridged, total_unread),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
        lines.push(Line::from(""));

        // Table header
        lines.push(Line::from(vec![
            Span::styled("  ID  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Room                 ", Style::default().fg(Color::DarkGray)),
            Span::styled("Platform  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Unread  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Last Message", Style::default().fg(Color::DarkGray)),
        ]));

        for room in &sorted {
            let ref_str = cs.room_id_to_ref.get(&room.room_id).map_or("-", String::as_str);
            let platform = room.bridge_source.map_or("Matrix", |b| b.label());

            // ID column color: cyan for easy identification
            let id_span = Span::styled(format!("  {ref_str:<4}"), Style::default().fg(Color::Cyan));

            // Room name: yellow if unread, gray otherwise
            let name_display = truncate_body(&room.display_name, 20);
            let name_span = if room.unread_count > 0 {
                Span::styled(format!("{name_display:<21}"), Style::default().fg(Color::Yellow))
            } else {
                Span::styled(format!("{name_display:<21}"), Style::default().fg(Color::White))
            };

            // Platform badge
            let platform_span = Span::styled(format!("{platform:<10}"), Style::default().fg(Color::DarkGray));

            // Unread count
            let unread_span = if room.unread_count > 0 {
                Span::styled(format!("{:<8}", room.unread_count), Style::default().fg(Color::Yellow))
            } else {
                Span::styled(format!("{:<8}", "-"), Style::default().fg(Color::DarkGray))
            };

            // Last message preview
            let msg_span = room.last_message.as_ref().map_or_else(
                || Span::styled("-", Style::default().fg(Color::DarkGray)),
                |msg| {
                    let preview = format!("{}: {}", msg.sender_display_name, truncate_body(&msg.body, 40));
                    Span::styled(preview, Style::default().fg(Color::DarkGray))
                },
            );

            lines.push(Line::from(vec![id_span, name_span, platform_span, unread_span, msg_span]));
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
                let sock = crate::server::global_socket_path()
                    .map_or_else(|| "unknown".to_string(), |p| p.to_string_lossy().to_string());
                let mut spans = vec![
                    Span::raw("  Server: "),
                    Span::styled("● Running", Style::default().fg(Color::Green)),
                    Span::styled(format!(" (UDS: {sock})"), Style::default().fg(Color::DarkGray)),
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
            // Assign stable short refs (C1, C2, ...) to any new rooms
            for room in &rooms {
                let _ref = cs.assign_room_ref(&room.room_id);
            }
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
