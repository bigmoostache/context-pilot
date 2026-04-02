//! Chat room panel — per-room message display with YAML context output.
//!
//! Created by `Chat_open` for each room the AI opens. Shows messages
//! with timestamps, sender names, reply markers, reactions, and media.
//! Auto-refreshes as new messages arrive via the sync loop.

use std::fmt::Write as _;

use crossterm::event::KeyEvent;

use cp_base::panels::{CacheRequest, CacheUpdate, ContextItem, Panel, scroll_key_action, time_arith};
use cp_base::state::actions::Action;
use cp_base::state::context::{Entry, estimate_tokens};
use cp_base::state::runtime::State;

use crate::types::{ChatState, MessageType, OpenRoom};

/// Per-room message panel, created dynamically by `Chat_open`.
#[derive(Debug)]
pub(crate) struct ChatRoomPanel;

// -- YAML context builders ---------------------------------------------------

impl ChatRoomPanel {
    /// Build the full YAML context string for a room panel.
    fn build_context(open: &OpenRoom, room_name: &str) -> String {
        let mut out = String::with_capacity(2048);
        Self::write_room_header(&mut out, open, room_name);
        Self::write_participants(&mut out, open);
        Self::write_messages(&mut out, open);
        out
    }

    /// Room metadata header: name, id, filter settings.
    fn write_room_header(out: &mut String, open: &OpenRoom, room_name: &str) {
        {
            let _r = writeln!(out, "room:");
        }
        {
            let _r = writeln!(out, "  name: \"{room_name}\"");
        }
        {
            let _r = writeln!(out, "  id: \"{}\"", open.room_id);
        }
        {
            let _r = writeln!(out, "  messages_shown: {}", open.messages.len());
        }
        if let Some(ref q) = open.filter.query {
            let _r = writeln!(out, "  filter_query: \"{q}\"");
        }
        if let Some(n) = open.filter.n_messages {
            let _r = writeln!(out, "  filter_n_messages: {n}");
        }
        if let Some(ref age) = open.filter.max_age {
            let _r = writeln!(out, "  filter_max_age: \"{age}\"");
        }
    }

    /// Participant list: display name, platform hint, Matrix user ID.
    fn write_participants(out: &mut String, open: &OpenRoom) {
        if open.participants.is_empty() {
            return;
        }
        out.push_str("participants:\n");
        for p in &open.participants {
            {
                let _r = writeln!(out, "  - name: \"{}\"", p.display_name);
            }
            {
                let _r = writeln!(out, "    user_id: \"{}\"", p.user_id);
            }
            if let Some(ref platform) = p.platform {
                let _r = writeln!(out, "    platform: \"{}\"", platform.label());
            }
        }
    }

    /// Message list with event refs, sender, body, reactions, replies, media.
    fn write_messages(out: &mut String, open: &OpenRoom) {
        if open.messages.is_empty() {
            out.push_str("messages: []\n");
            return;
        }
        out.push_str("messages:\n");
        for msg in &open.messages {
            let event_ref = open.event_id_to_ref.get(&msg.event_id).map_or_else(|| msg.event_id.clone(), Clone::clone);

            {
                let _r = writeln!(out, "  - ref: {event_ref}");
            }
            {
                let _r = writeln!(out, "    sender: \"{}\"", msg.sender_display_name);
            }
            {
                let _r = writeln!(out, "    time: \"{}\"", format_timestamp_iso(msg.timestamp));
            }

            // Message type hint for non-text messages
            match msg.msg_type {
                MessageType::Text => {}
                MessageType::Notice => {
                    let _r = writeln!(out, "    type: notice");
                }
                MessageType::Image => {
                    let _r = writeln!(out, "    type: image");
                }
                MessageType::File => {
                    let _r = writeln!(out, "    type: file");
                }
                MessageType::Video => {
                    let _r = writeln!(out, "    type: video");
                }
                MessageType::Audio => {
                    let _r = writeln!(out, "    type: audio");
                }
                MessageType::Emote => {
                    let _r = writeln!(out, "    type: emote");
                }
            }

            {
                let _r = writeln!(out, "    body: \"{}\"", escape_yaml_str(&msg.body));
            }

            // Reply marker
            if let Some(ref reply_to) = msg.reply_to {
                let reply_ref = open.event_id_to_ref.get(reply_to).map_or_else(|| reply_to.clone(), Clone::clone);
                let _r = writeln!(out, "    reply_to: {reply_ref}");
            }

            // Reactions
            if !msg.reactions.is_empty() {
                let reactions_str: String = msg
                    .reactions
                    .iter()
                    .map(|r| format!("{} {}", r.emoji, r.sender_name))
                    .collect::<Vec<_>>()
                    .join(", ");
                let _r = writeln!(out, "    reactions: [{reactions_str}]");
            }

            // Media path
            if let Some(ref path) = msg.media_path {
                let _r = writeln!(out, "    media_path: \"{path}\"");
                if let Some(size) = msg.media_size {
                    let _s = writeln!(out, "    media_size: {size}");
                }
            }
        }
    }

    /// Render messages as IR blocks for platform-agnostic display.
    fn render_messages_blocks(open: &OpenRoom) -> Vec<cp_render::Block> {
        use cp_render::{Block, Semantic, Span as S};

        if open.messages.is_empty() {
            return vec![Block::styled_text("  No messages yet".into(), Semantic::Muted)];
        }

        let mut blocks = Vec::new();

        for msg in &open.messages {
            let event_ref = open.event_id_to_ref.get(&msg.event_id).map_or_else(|| msg.event_id.clone(), Clone::clone);
            let timestamp = format_timestamp_short(msg.timestamp);

            // Truncate long messages for display
            let body = if msg.body.len() > 120 {
                let boundary = msg.body.floor_char_boundary(119);
                format!("{}…", msg.body.get(..boundary).unwrap_or(""))
            } else {
                msg.body.clone()
            };

            blocks.push(Block::Line(vec![
                S::muted(format!("  {timestamp} ")),
                S::styled(format!("[{event_ref}] "), Semantic::Info),
                S::warning(format!("{}: ", msg.sender_display_name)),
                S::new(body),
            ]));

            // Reply indicator
            if let Some(ref reply_to) = msg.reply_to {
                let reply_ref = open.event_id_to_ref.get(reply_to).map_or_else(|| "?".to_string(), Clone::clone);
                blocks.push(Block::Line(vec![S::muted(format!("    ↳ reply to {reply_ref}"))]));
            }

            // Reactions
            if !msg.reactions.is_empty() {
                let reactions_str: String = msg
                    .reactions
                    .iter()
                    .map(|r| format!("{} {}", r.emoji, r.sender_name))
                    .collect::<Vec<_>>()
                    .join("  ");
                blocks.push(Block::Line(vec![S::muted(format!("    {reactions_str}"))]));
            }

            // Media indicator
            if let Some(ref path) = msg.media_path {
                blocks.push(Block::Line(vec![S::styled(format!("    📎 {path}"), Semantic::Info)]));
            }
        }

        blocks
    }
}

// -- Panel trait implementation -----------------------------------------------

impl Panel for ChatRoomPanel {
    fn blocks(&self, state: &State) -> Vec<cp_render::Block> {
        use cp_render::{Block, Semantic};

        let cs = ChatState::get(state);

        // Find the OpenRoom that corresponds to this panel
        for ctx in &state.context {
            if ctx.context_type.as_str().starts_with("chat:")
                && ctx.context_type.as_str() != "chat-dashboard"
                && let Some(room_id) = ctx.get_meta_str("room_id")
                && let Some(open) = cs.open_rooms.get(room_id)
            {
                return Self::render_messages_blocks(open);
            }
        }

        vec![Block::styled_text("  Room not connected".into(), Semantic::Muted)]
    }
    fn title(&self, state: &State) -> String {
        let cs = ChatState::get(state);
        // Find the room_id from the context entry, then look up the display name
        for ctx in &state.context {
            if ctx.context_type.as_str().starts_with("chat:")
                && ctx.context_type.as_str() != "chat-dashboard"
                && let Some(room_id) = ctx.get_meta_str("room_id")
                && let Some(room) = cs.rooms.iter().find(|r| r.room_id == room_id)
            {
                return room.display_name.clone();
            }
        }
        "Room".to_string()
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
        // Each room panel refreshes its YAML context from ChatState.
        // The sync loop pushes messages via drain_sync_events() in dashboard;
        // we just need to rebuild our context string from the latest state.
        let mut updates: Vec<(String, String, usize)> = Vec::new();

        {
            let cs = ChatState::get(state);
            for ctx in &state.context {
                if ctx.context_type.as_str().starts_with("chat:")
                    && ctx.context_type.as_str() != "chat-dashboard"
                    && let Some(room_id) = ctx.get_meta_str("room_id")
                    && let Some(open) = cs.open_rooms.get(room_id)
                {
                    let room_name = cs
                        .rooms
                        .iter()
                        .find(|r| r.room_id == open.room_id)
                        .map_or("Unknown", |r| r.display_name.as_str());
                    let content = Self::build_context(open, room_name);
                    let tokens = estimate_tokens(&content);
                    updates.push((ctx.id.clone(), content, tokens));
                }
            }
        }

        for (panel_id, content, tokens) in updates {
            if let Some(ctx) = state.context.iter_mut().find(|c| c.id == panel_id) {
                ctx.token_count = tokens;
                let _ = cp_base::panels::update_if_changed(ctx, &content);
            }
        }
    }

    fn max_freezes(&self) -> u8 {
        0
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let cs = ChatState::get(state);

        for ctx in &state.context {
            if ctx.context_type.as_str().starts_with("chat:")
                && ctx.context_type.as_str() != "chat-dashboard"
                && let Some(room_id) = ctx.get_meta_str("room_id")
                && let Some(open) = cs.open_rooms.get(room_id)
            {
                let room_name =
                    cs.rooms.iter().find(|r| r.room_id == open.room_id).map_or("Unknown", |r| r.display_name.as_str());
                let content = Self::build_context(open, room_name);
                return vec![ContextItem::new(&ctx.id, room_name, content, ctx.last_refresh_ms)];
            }
        }

        vec![ContextItem::new("P0", "Room", "room: \"(not connected)\"\nmessages: []\n", 0)]
    }
}

// -- Helpers -----------------------------------------------------------------

/// Format a Unix timestamp (ms) to ISO 8601 short format.
fn format_timestamp_iso(timestamp_ms: u64) -> String {
    let secs = time_arith::ms_to_secs(timestamp_ms);
    let secs_i64 = i64::try_from(secs).unwrap_or(i64::MAX);
    chrono::DateTime::from_timestamp(secs_i64, 0)
        .map_or_else(|| "unknown".to_string(), |dt| dt.format("%Y-%m-%dT%H:%M:%S").to_string())
}

/// Format a Unix timestamp (ms) to short HH:MM display.
fn format_timestamp_short(timestamp_ms: u64) -> String {
    let secs = time_arith::ms_to_secs(timestamp_ms);
    let secs_i64 = i64::try_from(secs).unwrap_or(i64::MAX);
    chrono::DateTime::from_timestamp(secs_i64, 0)
        .map_or_else(|| "??:??".to_string(), |dt| dt.format("%H:%M").to_string())
}

/// Escape a string for YAML double-quoted context.
fn escape_yaml_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}
