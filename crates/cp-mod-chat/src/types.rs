//! Chat state types: rooms, messages, search results, server status.
//!
//! All types here are serializable for persistence across reloads.

use std::collections::{HashMap, HashSet};

use cp_base::state::runtime::State;
use serde::{Deserialize, Serialize};

/// Maximum messages retained per room panel.
pub const ROOM_MESSAGE_LIMIT: usize = 30;

/// Top-level chat module state, stored in the runtime `TypeMap`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatState {
    /// Cached room list (refreshed by sync loop).
    pub rooms: Vec<RoomInfo>,

    /// Currently open room panels: `room_id` → panel metadata.
    pub open_rooms: HashMap<String, OpenRoom>,

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

    /// Room ID currently showing a typing indicator (set during streaming).
    ///
    /// Cleared when the tool call completes or the stream stops.
    #[serde(skip)]
    pub typing_room: Option<String>,

    /// Per-bridge runtime status (populated when bridges are started).
    #[serde(skip)]
    pub bridge_status: HashMap<String, BridgeStatus>,

    /// Rooms awaiting a response from the AI.
    ///
    /// Populated when an external message arrives; cleared when the AI
    /// sends a reply (unless `report_later_here` is set). A Spine
    /// notification fires at stream end if this set is non-empty.
    #[serde(default)]
    pub report_here: HashSet<String>,

    /// Short room refs: `"C1"` → full Matrix room ID.
    ///
    /// Assigned lazily when rooms first appear in the sync loop.
    /// Stable across the session — a room keeps its ref until restart.
    #[serde(default)]
    pub room_refs: HashMap<String, String>,

    /// Reverse map: full Matrix room ID → `"C1"`.
    #[serde(default)]
    pub room_id_to_ref: HashMap<String, String>,

    /// Next room ref counter (monotonically increasing).
    #[serde(default, rename = "next_room_ref")]
    pub next_room_ref: u32,

    /// Recently sent messages for echo suppression.
    ///
    /// Entries: `(room_id, body, timestamp_ms)`. When the sync loop sees
    /// a message matching room + body within a short window, it treats
    /// the message as our own (bridge puppet echo). Pruned lazily.
    #[serde(skip)]
    pub recent_sends: Vec<(String, String, u64)>,
}

impl Default for ChatState {
    fn default() -> Self {
        Self {
            rooms: Vec::new(),
            open_rooms: HashMap::new(),
            server_pid: None,
            bot_user_id: None,
            server_status: ServerStatus::Stopped,
            search_query: None,
            search_results: Vec::new(),
            typing_room: None,
            bridge_status: HashMap::new(),
            report_here: HashSet::new(),
            room_refs: HashMap::new(),
            room_id_to_ref: HashMap::new(),
            next_room_ref: 1,
            recent_sends: Vec::new(),
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

    /// Assign a short room ref (`C1`, `C2`, ...) to a room ID.
    ///
    /// Returns the existing ref if already assigned, or mints a new one.
    pub fn assign_room_ref(&mut self, room_id: &str) -> String {
        if let Some(existing) = self.room_id_to_ref.get(room_id) {
            return existing.clone();
        }
        let ref_str = format!("C{}", self.next_room_ref);
        self.next_room_ref = self.next_room_ref.saturating_add(1);
        {
            let _prev = self.room_refs.insert(ref_str.clone(), room_id.to_string());
        }
        {
            let _prev = self.room_id_to_ref.insert(room_id.to_string(), ref_str.clone());
        }
        ref_str
    }

    /// Resolve a short room ref (`"C3"`) to a full Matrix room ID.
    #[must_use]
    pub fn resolve_room_ref(&self, ref_str: &str) -> Option<&str> {
        self.room_refs.get(ref_str).map(String::as_str)
    }
}

/// Per-room state for an open room panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenRoom {
    /// Context panel ID (e.g. `P42`).
    pub panel_id: String,
    /// Matrix room ID (`!abc:localhost`).
    pub room_id: String,
    /// Buffered messages (newest last, capped at [`ROOM_MESSAGE_LIMIT`]).
    pub messages: Vec<MessageInfo>,
    /// Short event refs: `"E1"` → full event ID.
    pub event_refs: HashMap<String, String>,
    /// Reverse map: full event ID → `"E1"`.
    pub event_id_to_ref: HashMap<String, String>,
    /// Next event ref counter.
    pub next_ref: u32,
    /// Known room participants.
    pub participants: Vec<ParticipantInfo>,
    /// Active filter configuration.
    pub filter: RoomFilter,
}

impl OpenRoom {
    /// Create a new open-room state for the given panel.
    #[must_use]
    pub fn new(panel_id: String, room_id: String) -> Self {
        Self {
            panel_id,
            room_id,
            messages: Vec::new(),
            event_refs: HashMap::new(),
            event_id_to_ref: HashMap::new(),
            next_ref: 1,
            participants: Vec::new(),
            filter: RoomFilter::default(),
        }
    }

    /// Assign a short event ref to an event ID, returning the ref string.
    ///
    /// If the event already has a ref, returns the existing one.
    pub fn assign_ref(&mut self, event_id: &str) -> String {
        if let Some(existing) = self.event_id_to_ref.get(event_id) {
            return existing.clone();
        }
        let ref_str = format!("E{}", self.next_ref);
        self.next_ref = self.next_ref.saturating_add(1);
        let _prev_ref = self.event_refs.insert(ref_str.clone(), event_id.to_string());
        let _prev_id = self.event_id_to_ref.insert(event_id.to_string(), ref_str.clone());
        ref_str
    }

    /// Push a message, enforcing the sliding window limit.
    ///
    /// Oldest messages (and their event refs) are evicted when the
    /// buffer exceeds [`ROOM_MESSAGE_LIMIT`].
    pub fn push_message(&mut self, msg: MessageInfo) {
        self.messages.push(msg);
        while self.messages.len() > ROOM_MESSAGE_LIMIT {
            let evicted = self.messages.remove(0);
            if let Some(r) = self.event_id_to_ref.remove(&evicted.event_id) {
                let _removed = self.event_refs.remove(&r);
            }
        }
    }

    /// Resolve a short ref (`"E3"`) to a full event ID.
    #[must_use]
    pub fn resolve_ref(&self, short_ref: &str) -> Option<&str> {
        self.event_refs.get(short_ref).map(String::as_str)
    }
}

/// Metadata about a room participant, for YAML context output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantInfo {
    /// Matrix user ID (e.g. `@alice:localhost`).
    pub user_id: String,
    /// Display name.
    pub display_name: String,
    /// Detected bridge source (e.g. Discord, `WhatsApp`) if this is a puppet.
    pub platform: Option<BridgeSource>,
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
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
    /// Reaction added to a message.
    Reaction {
        /// Matrix room ID.
        room_id: String,
        /// Event ID of the message being reacted to.
        target_event_id: String,
        /// Emoji key (e.g. `👍`).
        emoji: String,
        /// Display name of the user who reacted.
        sender_display_name: String,
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

/// Runtime status of a bridge process.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BridgeStatus {
    /// Bridge is not running.
    Stopped,
    /// Bridge process spawned, waiting for health check.
    Starting,
    /// Bridge is running and healthy.
    Running {
        /// Process ID of the bridge.
        pid: u32,
    },
    /// Bridge encountered an error.
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
