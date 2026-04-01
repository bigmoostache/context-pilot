//! Chat dashboard panel — always-on overview of rooms, server, bridges.
//!
//! Created automatically when the chat module activates. Shows room list
//! sorted by last activity, server status, bridge health indicators, and
//! an optional cross-room search section.

use std::fmt::Write as _;

use crossterm::event::KeyEvent;

use cp_base::panels::{CacheRequest, CacheUpdate, ContextItem, Panel, scroll_key_action};
use cp_base::state::actions::Action;
use cp_base::state::context::{Entry, Kind, estimate_tokens};
use cp_base::state::runtime::State;
use cp_base::ui::{self, TextCell};

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

    /// Append the room list table, sorted by last activity.
    fn write_room_list(out: &mut String, cs: &ChatState) {
        if cs.rooms.is_empty() {
            return;
        }
        let sorted = Self::sorted_rooms(cs);

        // Summary line
        let total_unread: u64 = cs.rooms.iter().map(|r| r.unread_count).sum();
        let bridged = cs.rooms.iter().filter(|r| r.bridge_source.is_some()).count();
        {
            let _r = writeln!(out, "rooms: {} total, {} bridged, {} unread", cs.rooms.len(), bridged, total_unread,);
        }

        // Build table rows
        let rows: Vec<Vec<TextCell>> = sorted.iter().map(|room| Self::room_text_row(room, cs)).collect();
        out.push_str(&ui::render_table_text(
            &["ID", "Room", "Platform", "Members", "Unread", "Last Message", "Time"],
            &rows,
        ));
    }

    /// Build a single text row for the LLM context table.
    fn room_text_row(room: &RoomInfo, cs: &ChatState) -> Vec<TextCell> {
        let ref_str = cs.room_id_to_ref.get(&room.room_id).map_or("-", String::as_str);
        let platform = room.bridge_source.map_or("Matrix", |b| b.label());
        let unread = if room.unread_count > 0 { room.unread_count.to_string() } else { "-".to_string() };
        let (last_msg, last_time) = room.last_message.as_ref().map_or_else(
            || ("-".to_string(), "-".to_string()),
            |msg| {
                let preview = format!("{}: {}", msg.sender_display_name, &msg.body);
                let time = format_timestamp_short(msg.timestamp);
                (preview, time)
            },
        );
        vec![
            TextCell::left(ref_str),
            TextCell::left(truncate_body(&room.display_name, 20)),
            TextCell::left(platform),
            TextCell::right(room.member_count.to_string()),
            TextCell::right(unread),
            TextCell::left(last_msg),
            TextCell::left(last_time),
        ]
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

    /// Sort rooms by last activity (newest first).
    fn sorted_rooms(cs: &ChatState) -> Vec<&RoomInfo> {
        let mut sorted: Vec<&RoomInfo> = cs.rooms.iter().collect();
        sorted.sort_by(|a, b| {
            let ts_a = a.last_message.as_ref().map_or(0, |m| m.timestamp);
            let ts_b = b.last_message.as_ref().map_or(0, |m| m.timestamp);
            ts_b.cmp(&ts_a)
        });
        sorted
    }
}

impl Panel for ChatDashboardPanel {
    fn blocks(&self, state: &State) -> Vec<cp_render::Block> {
        use cp_render::{Align, Block, Cell as IrCell, Semantic, Span as S};

        let cs = ChatState::get(state);
        let mut blocks = Vec::new();

        // Server status
        let (status_text, status_sem) = match &cs.server_status {
            ServerStatus::Stopped => ("● Stopped", Semantic::Error),
            ServerStatus::Starting => ("● Starting…", Semantic::Warning),
            ServerStatus::Running => ("● Running", Semantic::Success),
            ServerStatus::Error(_) => ("● Error", Semantic::Error),
        };

        let mut status_spans = vec![S::muted("  Server: ".into()), S::styled(status_text.into(), status_sem)];
        if matches!(cs.server_status, ServerStatus::Running) {
            let sock = crate::server::global_socket_path()
                .map_or_else(|| "unknown".to_string(), |p| p.to_string_lossy().to_string());
            status_spans.push(S::muted(format!(" (UDS: {sock})")));
            if let Some(ref uid) = cs.bot_user_id {
                status_spans.push(S::muted(format!("  as {uid}")));
            }
        }
        if let ServerStatus::Error(e) = &cs.server_status {
            status_spans.push(S::error(format!(": {e}")));
        }
        blocks.push(Block::Line(status_spans));
        blocks.push(Block::Empty);

        // Room list
        if cs.rooms.is_empty() {
            blocks
                .push(Block::Line(vec![S::muted("  No rooms yet — use Chat_create_room or bridge a platform".into())]));
        } else {
            let sorted = Self::sorted_rooms(cs);
            let total_unread: u64 = cs.rooms.iter().map(|r| r.unread_count).sum();
            let bridged = cs.rooms.iter().filter(|r| r.bridge_source.is_some()).count();

            blocks.push(Block::Line(vec![
                S::new("  Rooms ".into()),
                S::muted(format!("{} total, {} bridged, {} unread", cs.rooms.len(), bridged, total_unread)),
            ]));
            blocks.push(Block::Empty);

            let rows: Vec<Vec<IrCell>> = sorted
                .iter()
                .map(|room| {
                    let ref_str = cs.room_id_to_ref.get(&room.room_id).map_or_else(|| "-".to_string(), Clone::clone);
                    let platform = room.bridge_source.map_or("Matrix", |b| b.label());
                    let unread_str =
                        if room.unread_count > 0 { room.unread_count.to_string() } else { "-".to_string() };
                    let unread_sem = if room.unread_count > 0 { Semantic::Warning } else { Semantic::Muted };
                    let name_sem = if room.unread_count > 0 { Semantic::Warning } else { Semantic::Default };
                    let (last_msg, last_time) = room.last_message.as_ref().map_or_else(
                        || ("-".to_string(), "-".to_string()),
                        |msg| {
                            let preview = format!("{}: {}", msg.sender_display_name, &msg.body);
                            let time = format_timestamp_short(msg.timestamp);
                            (preview, time)
                        },
                    );
                    vec![
                        IrCell::styled(ref_str, Semantic::Info),
                        IrCell::styled(room.display_name.clone(), name_sem),
                        IrCell::styled(platform.into(), Semantic::Muted),
                        IrCell::right(S::styled(unread_str, unread_sem)),
                        IrCell::styled(last_msg, Semantic::Muted),
                        IrCell::styled(last_time, Semantic::Muted),
                    ]
                })
                .collect();

            blocks.push(Block::table(
                vec![
                    ("ID", Align::Left),
                    ("Room", Align::Left),
                    ("Platform", Align::Left),
                    ("Unread", Align::Right),
                    ("Last Message", Align::Left),
                    ("Time", Align::Left),
                ],
                rows,
            ));
        }

        // Search results
        if let Some(ref query) = cs.search_query {
            blocks.push(Block::Empty);
            blocks.push(Block::Line(vec![S::new("  Search: ".into()), S::warning(format!("\"{query}\""))]));

            if cs.search_results.is_empty() {
                blocks.push(Block::Line(vec![S::muted("  No results".into())]));
            } else {
                for sr in &cs.search_results {
                    blocks.push(Block::Line(vec![
                        S::info(format!("  [{}] ", sr.room_name)),
                        S::new(sr.sender.clone()),
                        S::muted(format!(": {}", truncate_body(&sr.body, 60))),
                    ]));
                }
            }
        }

        blocks
    }
    fn title(&self, _state: &State) -> String {
        "Chat".to_string()
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

    fn max_freezes(&self) -> u8 {
        0
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
