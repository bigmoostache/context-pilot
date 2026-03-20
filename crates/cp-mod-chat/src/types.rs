//! Chat state types: rooms, messages, search results, server status.
//!
//! All types here are serializable for persistence across reloads.

use std::collections::HashMap;

use cp_base::state::runtime::State;
use serde::{Deserialize, Serialize};

/// Top-level chat module state, stored in the runtime `TypeMap`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatState {
    /// Cached room list (refreshed by sync loop).
    pub rooms: Vec<RoomInfo>,

    /// Currently open room panels per worker (`room_id` → `panel_id`).
    pub open_rooms: HashMap<String, String>,

    /// Event ref mapping per open room (short ref `E1` → full event ID).
    pub event_refs: HashMap<String, HashMap<String, String>>,

    /// PID of the running Tuwunel server process (`None` when stopped).
    pub server_pid: Option<u32>,

    /// Bot Matrix user ID (e.g. `@context-pilot:localhost`), set after registration.
    pub bot_user_id: Option<String>,

    /// Server health status.
    pub server_status: ServerStatus,

    /// Active dashboard search query (`None` = no search).
    pub search_query: Option<String>,

    /// Dashboard search results (populated by `Chat_search`).
    pub search_results: Vec<SearchResult>,
}

impl Default for ChatState {
    fn default() -> Self {
        Self {
            rooms: Vec::new(),
            open_rooms: HashMap::new(),
            event_refs: HashMap::new(),
            server_pid: None,
            bot_user_id: None,
            server_status: ServerStatus::Stopped,
            search_query: None,
            search_results: Vec::new(),
        }
    }
}

impl ChatState {
    /// Borrow the `ChatState` from the runtime `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if the chat module was not initialised.
    #[must_use]
    pub fn get(state: &State) -> &Self {
        state.ext()
    }

    /// Mutably borrow the `ChatState` from the runtime `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if the chat module was not initialised.
    pub fn get_mut(state: &mut State) -> &mut Self {
        state.ext_mut()
    }
}

/// Metadata for a single Matrix room (group or DM).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomInfo {
    /// Matrix room ID (e.g. `!abc123:localhost`).
    pub room_id: String,
    /// Human-readable room name.
    pub display_name: String,
    /// Optional room topic.
    pub topic: Option<String>,
    /// Number of unread messages (internal counter).
    pub unread_count: u64,
    /// Most recent message in the room.
    pub last_message: Option<MessageInfo>,
    /// Whether this is a direct-message room.
    pub is_direct: bool,
    /// Total members in the room.
    pub member_count: u64,
    /// ISO 8601 creation date.
    pub creation_date: Option<String>,
    /// Whether the room uses E2EE.
    pub encrypted: bool,
    /// Detected bridge source (if room is bridged).
    pub bridge_source: Option<BridgeSource>,
}

/// A single message in a Matrix room.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageInfo {
    /// Full Matrix event ID.
    pub event_id: String,
    /// Matrix user ID of the sender.
    pub sender: String,
    /// Human-readable display name of the sender.
    pub sender_display_name: String,
    /// Message body (plain text).
    pub body: String,
    /// Unix timestamp (milliseconds).
    pub timestamp: u64,
    /// Message content type.
    pub msg_type: MessageType,
    /// Event ID this message replies to (if threaded reply).
    pub reply_to: Option<String>,
    /// Reactions aggregated on this message.
    pub reactions: Vec<ReactionInfo>,
    /// Local file path for downloaded media (if applicable).
    pub media_path: Option<String>,
    /// Media file size in bytes.
    pub media_size: Option<u64>,
}

/// Matrix message content type.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MessageType {
    /// Regular text message (`m.text`).
    Text,
    /// Bot notice (`m.notice`).
    Notice,
    /// Image attachment (`m.image`).
    Image,
    /// File attachment (`m.file`).
    File,
    /// Video attachment (`m.video`).
    Video,
    /// Audio attachment (`m.audio`).
    Audio,
    /// Emote (`m.emote`).
    Emote,
}

/// A reaction on a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReactionInfo {
    /// Emoji key (e.g. `👍`).
    pub emoji: String,
    /// Display name of the user who reacted.
    pub sender_name: String,
}

/// A cross-room search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// Room ID containing the match.
    pub room_id: String,
    /// Room display name.
    pub room_name: String,
    /// Event ID of the matching message.
    pub event_id: String,
    /// Sender display name.
    pub sender: String,
    /// Message body excerpt.
    pub body: String,
    /// Unix timestamp (milliseconds).
    pub timestamp: u64,
}

/// Filter configuration for a room panel view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomFilter {
    /// Maximum messages to display.
    pub n_messages: Option<u64>,
    /// Only show messages newer than this duration (e.g. `"24h"`, `"7d"`).
    pub max_age: Option<String>,
    /// Text search filter within the room.
    pub query: Option<String>,
}

/// Event pushed from the async sync loop to the main thread.
///
/// The sync loop has no access to [`State`], so it sends these through
/// a [`std::sync::mpsc`] channel. The dashboard panel drains them on
/// each `refresh()` tick and applies them to [`ChatState`].
#[derive(Debug, Clone)]
pub enum ChatEvent {
    /// New message arrived in a room.
    Message {
        /// Matrix room ID.
        room_id: String,
        /// Sender Matrix user ID.
        sender: String,
        /// Sender display name.
        sender_display_name: String,
        /// Message body (plain text).
        body: String,
        /// Full Matrix event ID.
        event_id: String,
        /// Unix timestamp in milliseconds.
        timestamp_ms: u64,
    },
    /// Room invite received — auto-accepted by the handler.
    Invite {
        /// Matrix room ID of the invitation.
        room_id: String,
    },
    /// Room metadata changed (name, topic, member count).
    RoomMeta {
        /// Matrix room ID.
        room_id: String,
        /// Updated display name.
        display_name: String,
        /// Updated topic.
        topic: Option<String>,
        /// Updated member count.
        member_count: u64,
    },
}

/// Tuwunel homeserver health status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ServerStatus {
    /// Server process not running.
    Stopped,
    /// Server is starting up (health check pending).
    Starting,
    /// Server is running and healthy.
    Running,
    /// Server encountered an error.
    Error(String),
}

/// Detected bridge platform source for a room.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum BridgeSource {
    /// Discord bridge (`mautrix-discord`).
    Discord,
    /// `WhatsApp` bridge (`mautrix-whatsapp`).
    WhatsApp,
    /// Telegram bridge (`mautrix-telegram`).
    Telegram,
    /// Signal bridge (`mautrix-signal`).
    Signal,
    /// Slack bridge (`mautrix-slack`).
    Slack,
    /// IRC bridge (`mautrix-irc`).
    Irc,
    /// Meta (Instagram + Messenger) bridge.
    Meta,
    /// Twitter/X bridge.
    Twitter,
    /// Bluesky bridge.
    Bluesky,
    /// Google Chat bridge.
    GoogleChat,
    /// Google Messages bridge.
    GoogleMessages,
    /// Zulip bridge.
    Zulip,
    /// `LinkedIn` bridge.
    LinkedIn,
    /// Native Matrix (no bridge).
    Native,
}

impl BridgeSource {
    /// Short display label for the bridge source.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Discord => "Discord",
            Self::WhatsApp => "WhatsApp",
            Self::Telegram => "Telegram",
            Self::Signal => "Signal",
            Self::Slack => "Slack",
            Self::Irc => "IRC",
            Self::Meta => "Meta",
            Self::Twitter => "Twitter",
            Self::Bluesky => "Bluesky",
            Self::GoogleChat => "Google Chat",
            Self::GoogleMessages => "Google Messages",
            Self::Zulip => "Zulip",
            Self::LinkedIn => "LinkedIn",
            Self::Native => "Matrix",
        }
    }
}
