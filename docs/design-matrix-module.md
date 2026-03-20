# Matrix Module — Design Document

**Branch**: `matrix.org`
**Status**: Draft v1 — open for refinement
**Date**: 2026-03-20

---

## 1. Vision

The Matrix module gives Context Pilot a **universal messaging layer**. The AI sees
rooms and messages through a single, uniform interface — it never knows whether a
message originated from Discord, WhatsApp, Telegram, Signal, Slack, IRC, or native
Matrix. Bridges handle the translation invisibly.

Any human can reach the AI from the chat platform they already use. The AI replies
in the same room, in the same thread, with the same tools. This turns Context Pilot
from a terminal-only assistant into a **multi-channel agent**.

### Core Principles

1. **Uniform abstraction**: The AI interacts with Matrix rooms and messages. Period.
   Bridge details never surface in tool definitions or panel content.
2. **Local-first**: A self-hosted Matrix homeserver runs alongside CP. No external
   accounts, no cloud dependencies, no data leaving the machine (unless the user
   enables federation).
3. **Progressive complexity**: Basic read/reply works with zero bridge config.
   Bridges, federation, and advanced features are opt-in layers.

---

## 2. Architecture Overview

```
┌────────────────────────────────────────────────────────────────────┐
│                        Context Pilot TUI                           │
│                                                                    │
│  ┌─────────────┐  ┌──────────────┐  ┌──────────────────────────┐  │
│  │ Message Room │  │ Message Room │  │    Matrix Overview        │  │
│  │ Panel #work  │  │ Panel #alert │  │    Panel (room list,      │  │
│  │              │  │              │  │    status, unread counts)  │  │
│  └──────┬───────┘  └──────┬───────┘  └────────────┬─────────────┘  │
│         │                 │                        │                │
│  ┌──────┴─────────────────┴────────────────────────┴─────────────┐ │
│  │                    cp-mod-chat                                │ │
│  │  ChatModule (tools + panels + sync loop + state)              │ │
│  │  Uses: matrix-sdk (Rust crate)                                │ │
│  └──────────────────────────┬────────────────────────────────────┘ │
└─────────────────────────────┼──────────────────────────────────────┘
                              │  HTTP (Client-Server API)
                              │  localhost:6167
                              │
┌─────────────────────────────┼──────────────────────────────────────┐
│              Local Matrix Homeserver (Tuwunel)                      │
│                              │                                      │
│  ┌──────────┐ ┌──────────┐ ┌┴──────────┐ ┌──────────┐             │
│  │ mautrix  │ │ mautrix  │ │ mautrix   │ │ mautrix  │             │
│  │ discord  │ │ whatsapp │ │ telegram  │ │ signal   │  ...        │
│  └──────────┘ └──────────┘ └───────────┘ └──────────┘             │
│                                                                    │
│  Storage: .context-pilot/matrix/                                   │
│  Config:  .context-pilot/matrix/homeserver.toml                    │
└────────────────────────────────────────────────────────────────────┘
```

---

## 3. Server Lifecycle

### 3.1 Homeserver: Tuwunel (Managed Child Process)

Tuwunel (the Conduwuit successor) runs as a **managed child process** — similar to
how `cp-console-server` works today. CP starts it on module activation, stops it on
deactivation, and monitors its health.

| Aspect           | Decision                                              |
|------------------|-------------------------------------------------------|
| Binary location  | Bundled in CP release artifacts (extracted to `~/.context-pilot/bin/tuwunel`) |
| Data directory   | `.context-pilot/matrix/data/`                         |
| Config           | `.context-pilot/matrix/homeserver.toml` (auto-generated) |
| Listening        | `127.0.0.1:6167` (localhost only, no federation by default) |
| Database         | SQLite (Tuwunel default) or RocksDB                   |
| Logs             | `.context-pilot/matrix/server.log`                    |
| Process mgmt     | Spawned by CP, PID tracked, health-checked via `/_matrix/client/versions` |

### 3.2 First-Run Bootstrap

On first activation of the Matrix module:

1. **Extract Tuwunel** binary from bundled CP assets to `~/.context-pilot/bin/tuwunel` if not already present
2. **Generate config** (`homeserver.toml`) with secure defaults:
   - Server name: `localhost` (or user-configured)
   - Registration: disabled (CP creates the bot account directly)
   - Listening: `127.0.0.1:6167`
3. **Create bot account**: `@context-pilot:localhost` with admin privileges
4. **Store access token** in `.context-pilot/matrix/credentials.json`
5. **Create default room**: `#general:localhost`

### 3.3 Startup Sequence (Every Module Activation)

1. Check if Tuwunel binary exists at `~/.context-pilot/bin/tuwunel` → extract from bundled assets if missing
2. Start Tuwunel process
3. Wait for `/_matrix/client/versions` to respond (with timeout)
4. Authenticate with stored access token
5. Start background sync loop (`matrix-sdk` sliding sync)
6. Populate room list in ChatState
7. Module ready — tools and panels available

### 3.4 Shutdown Sequence

1. Stop background sync loop
2. Send SIGTERM to Tuwunel process
3. Wait for graceful shutdown (5s timeout, then SIGKILL)
4. Clean up PID file

---

## 4. Module Design: `cp-mod-chat`

### 4.1 Crate Structure

```
crates/cp-mod-chat/
├── src/
│   ├── lib.rs            # Module trait impl, tool registration
│   ├── types.rs          # ChatState, RoomInfo, MessageInfo
│   ├── client.rs         # matrix-sdk wrapper, sync loop, auth
│   ├── server.rs         # Tuwunel process lifecycle management
│   ├── bootstrap.rs      # First-run setup, download, config generation
│   ├── panels/
│   │   ├── mod.rs
│   │   ├── room.rs       # ChatRoomPanel — shows messages in one room
│   │   └── dashboard.rs  # ChatDashboardPanel — room list, status, search
│   └── tools/
│       ├── mod.rs         # Tool dispatch
│       ├── rooms.rs       # Room management tools
│       ├── messages.rs    # Message read/send/react tools
│       └── status.rs      # Server status/health tools
└── Cargo.toml
```

### 4.2 State

```rust
pub struct ChatState {
    /// matrix-sdk Client handle (authenticated, shared across workers)
    pub client: Option<matrix_sdk::Client>,

    /// Tuwunel child process handle
    pub server_process: Option<Child>,

    /// Cached room list (refreshed by sync loop)
    pub rooms: Vec<RoomInfo>,

    /// Currently open room panels per worker (room_id → panel_id)
    pub open_rooms: HashMap<String, String>,

    /// Event ref mapping per open room (short ref "E1" → full event ID)
    pub event_refs: HashMap<String, HashMap<String, String>>,

    /// Background sync task handle (shared, one sync loop)
    pub sync_handle: Option<JoinHandle<()>>,

    /// Server health status
    pub server_status: ServerStatus,

    /// Active dashboard search query (None = no search)
    pub search_query: Option<String>,

    /// Dashboard search results (populated by Chat_search)
    pub search_results: Vec<SearchResult>,
}

pub struct RoomInfo {
    pub room_id: String,
    pub display_name: String,
    pub topic: Option<String>,
    pub unread_count: u64,
    pub last_message: Option<MessageInfo>,
    pub is_direct: bool,
    pub member_count: u64,
    pub creation_date: Option<String>,
    pub encrypted: bool,
    /// Detected from appservice puppet user namespace
    pub bridge_source: Option<BridgeSource>,
}

pub struct MessageInfo {
    pub event_id: String,
    pub sender: String,
    pub sender_display_name: String,
    pub body: String,
    pub timestamp: u64,
    pub msg_type: MessageType,
    pub reply_to: Option<String>,
    pub reactions: Vec<ReactionInfo>,
    /// Local path for downloaded media (if applicable)
    pub media_path: Option<String>,
    pub media_size: Option<u64>,
}

pub struct SearchResult {
    pub room_id: String,
    pub room_name: String,
    pub event_id: String,
    pub sender: String,
    pub body: String,
    pub timestamp: u64,
}

pub struct RoomFilter {
    pub n_messages: Option<u64>,
    pub max_age: Option<String>,
    pub query: Option<String>,
}

pub enum ServerStatus {
    Stopped,
    Starting,
    Running,
    Error(String),
}

pub enum BridgeSource {
    Discord,
    WhatsApp,
    Telegram,
    Signal,
    Slack,
    Irc,
    Meta,
    Twitter,
    Bluesky,
    GoogleChat,
    GoogleMessages,
    Zulip,
    LinkedIn,
    Native,
}
```

---

## 5. AI Tools

### 5.1 Tool Summary

| Tool                   | Description                                        | Category |
|------------------------|----------------------------------------------------|----------|
| `Chat_open`            | Open a room as a context panel (shows messages)    | View     |
| `Chat_send`            | Send, reply, edit, or delete a message             | Message  |
| `Chat_react`           | Add a reaction emoji to a message                  | Message  |
| `Chat_configure`       | Set/clear filters on an open room panel            | View     |
| `Chat_search`          | Cross-room search in the dashboard panel           | Search   |
| `Chat_mark_as_read`    | Acknowledge all messages in a room                 | Status   |
| `Chat_create_room`     | Create a new Matrix room                           | Room     |
| `Chat_invite`          | Invite a user to a room                            | Room     |

### 5.2 Tool Definitions

#### `Chat_open`

```yaml
name: Chat_open
description: >
  Opens a Matrix room as a context panel showing recent messages.
  The panel auto-refreshes as new messages arrive via the sync loop.
  If the room is already open, returns success without creating a
  duplicate. Close the panel with Close_panel to stop watching.
  Room is resolved by alias (e.g. '#general') via Matrix API.
  Default: shows last 30 messages. Use Chat_configure to filter.
parameters:
  room:
    type: string
    required: true
    description: "Room alias (e.g. '#general') or room ID"
```

#### `Chat_send`

```yaml
name: Chat_send
description: >
  Sends a message to a Matrix room. Supports plain text and markdown
  (sent as both plain text body + HTML formatted_body per Matrix spec).
  Default mode is notice (bot-style, no notification to bridged users).
  Optional params for reply, edit, or delete operations on own messages.
  Only one of edit/delete can be used per call. Event refs (E1, E2...)
  from open room panels can be used for reply_to/edit/delete.
parameters:
  room:
    type: string
    required: true
    description: "Room alias (e.g. '#general') or room ID"
  message:
    type: string
    required: false
    description: "Message content (markdown). Required unless deleting."
  reply_to:
    type: string
    required: false
    description: "Event ref (e.g. 'E3') to reply to (creates threaded reply)"
  edit:
    type: string
    required: false
    description: "Event ref (e.g. 'E5') of own message to edit. message param is the new content."
  delete:
    type: string
    required: false
    description: "Event ref (e.g. 'E3') of own message to delete. No message param needed."
  notice:
    type: boolean
    required: false
    description: "Send as notice (default true). Set false for regular message (triggers notifications)."
```

#### `Chat_react`

```yaml
name: Chat_react
description: >
  Adds an emoji reaction to a message in a room.
parameters:
  room:
    type: string
    required: true
    description: "Room alias or room ID"
  event_id:
    type: string
    required: true
    description: "Event ref (e.g. 'E3') of the message to react to"
  emoji:
    type: string
    required: true
    description: "Reaction emoji (e.g. '👍', '✅', '🏴‍☠️')"
```

#### `Chat_configure`

```yaml
name: Chat_configure
description: >
  Configures the view on an open room panel. Sets or clears filters
  for message display. All params are optional — omitted params keep
  current values. Call with no filter params to reset to default view
  (latest 30 messages, no filters). The room panel must already be
  open (via Chat_open).
parameters:
  room:
    type: string
    required: true
    description: "Room alias or room ID (must be an open panel)"
  n_messages:
    type: integer
    required: false
    description: "Max messages to show (default 30)"
  max_age:
    type: string
    required: false
    description: "Only show messages newer than this (e.g. '24h', '7d')"
  query:
    type: string
    required: false
    description: "Filter messages containing this text"
```

#### `Chat_search`

```yaml
name: Chat_search
description: >
  Activates a cross-room search section in the Chat dashboard panel.
  Only one search active at a time — calling again replaces the
  previous search. Results appear in a split view below the room list.
  Max 20 results. Call with empty query to clear the search section.
  Results contribute to dashboard context_content().
parameters:
  query:
    type: string
    required: true
    description: "Search query (empty string clears active search)"
  room:
    type: string
    required: false
    description: "Limit search to a specific room"
```

#### `Chat_mark_as_read`

```yaml
name: Chat_mark_as_read
description: >
  Marks ALL messages in a room as read/processed. Resets the room's
  unread count to zero, removes it from the 'Unprocessed messages'
  Spine notification, and sends a Matrix read receipt (visible to
  bridged users as 'read' status). Opening a room panel does NOT
  mark messages as read — this explicit call is required.
parameters:
  room:
    type: string
    required: true
    description: "Room alias or room ID to acknowledge"
```

#### `Chat_create_room`

```yaml
name: Chat_create_room
description: >
  Creates a new Matrix room on the local homeserver.
parameters:
  name:
    type: string
    required: true
    description: "Room name (e.g. 'deploy-alerts')"
  topic:
    type: string
    required: false
    description: "Room topic/description"
  invite:
    type: array
    required: false
    description: "User IDs to invite (e.g. ['@alice:localhost'])"
```

#### `Chat_invite`

```yaml
name: Chat_invite
description: >
  Invites a user to a Matrix room.
parameters:
  room:
    type: string
    required: true
    description: "Room alias or room ID"
  user_id:
    type: string
    required: true
    description: "Matrix user ID (e.g. '@alice:localhost')"
```

---

## 6. Panels

### 6.1 ChatRoomPanel

Displays messages in a single Matrix room. Created by `Chat_open`.
Context type: `chat:<room_id>` (unique per room).
Panel title: `#general (Discord)` — room name + platform hint.

**Rendering:**
```
─── #general (Discord) ─── 3 unread ──────────────────
  10:23  alice    Hey, can you review the PR?
  10:24  bob      Sure, looking at it now
  10:25  alice    └─ reply to bob: Thanks
  10:31  ★ CP     I'll investigate the test failures.
  10:45  alice    👍 (to CP's message)
  10:50  bob      📎 image: screenshot.png (245 KB)
───────────────────────────────────────────────────────
```

**Context output** (YAML — what the LLM sees):
```yaml
room: "#general"
bridge: discord
topic: "Development discussion"
members: 5
unread: 3
encrypted: false
filter: null  # or {n_messages: 50, max_age: "24h", query: "deploy"}
participants:
  - name: alice
    platform: discord
    user_id: "@discord_alice:localhost"
  - name: bob
    platform: discord
    user_id: "@discord_bob:localhost"
  - name: Context Pilot
    platform: native
    user_id: "@context-pilot:localhost"
messages:
  - id: E1
    sender: alice
    time: "2026-03-20T10:23:00Z"
    body: "Hey, can you review the PR?"
  - id: E2
    sender: bob
    time: "2026-03-20T10:24:00Z"
    body: "Sure, looking at it now"
  - id: E3
    sender: alice
    time: "2026-03-20T10:25:00Z"
    reply_to: E2
    body: "Thanks"
  - id: E4
    sender: Context Pilot
    time: "2026-03-20T10:31:00Z"
    body: "I'll investigate the test failures."
    reactions: [👍 alice]
  - id: E5
    sender: bob
    time: "2026-03-20T10:50:00Z"
    body: "📎 image: .context-pilot/matrix/media/screenshot.png (245 KB)"
```

**Behavior:**
- Auto-refreshes via the background sync loop (push, not poll)
- Event refs (E1, E2...) are short sequential IDs mapped to full Matrix event IDs internally
- Event refs reset when panel is closed and re-opened
- Bot's own messages included in context (sender: "Context Pilot")
- Media files downloaded to `.context-pilot/matrix/media/` with inline path + size
- Reactions shown inline after the message body
- Replies shown with `reply_to: E<n>` reference
- Default: last 30 messages (sliding window). Configurable via Chat_configure
- Duplicate Chat_open on same room is a no-op (returns success)

### 6.2 ChatDashboardPanel

Fixed panel (always present when module is active). Shows room list,
server status, bridge info, and optional search results.
Context type: `chat-dashboard`.

**Rendering:**
```
─── Chat ─── ● Running ─── 5 rooms ─── 2 bridges ─────
  Server: tuwunel 0.5.2 on localhost:6167

  Rooms:
  │ #general        │  3 unread │ alice: The tests in... │
  │ #alerts         │  0 unread │ bot: Deploy success    │
  │ @alice (DM)     │  1 unread │ alice: Thanks!         │
  │ @bob (WhatsApp) │  0 unread │ bob: ok                │
  │ #dev-log        │  0 unread │ CP: Committed a3f2...  │

  Bridges: discord ● │ whatsapp ● │ telegram ○

  Search: "deploy error" (3 results)
  │ #general  │ alice │ 10:25 │ deploy error on staging  │
  │ #alerts   │ bot   │ 09:00 │ deploy error: timeout    │
  │ #dev-log  │ CP    │ 10:40 │ fixed deploy error in... │
────────────────────────────────────────────────────────
```

**Context output** (YAML):
```yaml
server:
  status: running
  version: "tuwunel 0.5.2"
  address: "localhost:6167"
bridges:
  - name: discord
    status: connected
  - name: whatsapp
    status: connected
  - name: telegram
    status: disconnected
rooms:
  - name: "#general"
    bridge: discord
    unread: 3
    last_message: "alice: The tests in module_x are failing"
    last_activity: "2026-03-20T10:25:00Z"
  - name: "#alerts"
    bridge: null
    unread: 0
    last_message: "bot: Deploy success"
    last_activity: "2026-03-20T09:00:00Z"
  - name: "@alice"
    bridge: null
    unread: 1
    last_message: "alice: Thanks!"
    last_activity: "2026-03-20T10:45:00Z"
  - name: "@bob"
    bridge: whatsapp
    unread: 0
    last_message: "bob: ok"
    last_activity: "2026-03-19T18:30:00Z"
  - name: "#dev-log"
    bridge: null
    unread: 0
    last_message: "Context Pilot: Committed a3f2..."
    last_activity: "2026-03-20T10:40:00Z"
search:
  query: "deploy error"
  results:
    - room: "#general"
      sender: alice
      time: "2026-03-20T10:25:00Z"
      body: "deploy error on staging server"
    - room: "#alerts"
      sender: bot
      time: "2026-03-20T09:00:00Z"
      body: "deploy error: timeout after 30s"
    - room: "#dev-log"
      sender: Context Pilot
      time: "2026-03-20T10:40:00Z"
      body: "fixed deploy error in CI pipeline"
```

**Behavior:**
- Always-on fixed panel when chat module is active
- Room list sorted by last activity (most recent first)
- No room count limit — all joined rooms shown
- Bridge status detected via appservice registration health checks
- Search section appears in split view (rooms always visible above)
- Max 20 search results
- Chat_search with empty query clears the search section
- Status info (server + bridges) integrated — no separate status tool

---

## 7. Sync Architecture

### 7.1 Background Sync Loop

The module runs a persistent background task using `matrix-sdk`'s
**Sliding Sync** (MSC3575) mechanism — only fetching rooms and events
the client cares about, with faster initial sync and lower bandwidth
than traditional `/sync`:

```
Module activation
    │
    ▼
Start sync loop ──→ matrix_sdk::Client::sliding_sync()
    │                     │
    │                     ├── on room message → update ChatState.rooms
    │                     │                   → notify open panels
    │                     │                   → update Spine notification
    │                     │                   → set typing indicator (if Chat_send streaming)
    │                     │
    │                     ├── on invite → auto-accept (all invites)
    │                     │
    │                     ├── on room state → update room list
    │                     │
    │                     └── on sync error → update ServerStatus
    │
    ▼
Module deactivation → cancel sync task
```

The sync loop is **shared across workers**: one `matrix-sdk::Client` instance,
one sync task. Per-worker room panels are views into the shared state.

### 7.2 Notification Integration

The module uses a **single coalesced Spine notification** for all unread
messages across all rooms. The notification **updates in place immediately**
on every new message — replacing the existing notification, never creating
duplicates.

Format: `"Unprocessed messages: 5 in #general, 2 in @bob, 1 in #alerts"`

```
New message arrives in any room
    │
    ├── Update unread count in ChatState
    ├── Update ChatDashboardPanel (room list)
    │
    ├── Is a room panel open for this room?
    │   └── Yes → Push new message to panel content
    │
    └── Are there ANY unread messages across all rooms?
        └── Yes → Replace (or create) single Spine notification:
                  "Unprocessed messages: N total across M rooms"
```

The AI reads the notification, decides which rooms to check (via
the dashboard or `Chat_open`), processes them, and calls
`Chat_mark_as_read` to clear each room. When all rooms reach zero
unread, the notification is removed.

`Chat_mark_as_read` acknowledges the entire room (all messages),
resets internal unread count, AND sends a Matrix read receipt
(bridged users see "read" status on their platform).

### 7.3 Typing Indicators

The bot sends Matrix typing indicators using the existing
`StreamingTool` infrastructure:

```
LLM streams Chat_send tool call
    │
    ├── StreamEvent::ToolProgress with tool_name="Chat_send"
    │   └── Room param parsed from partial JSON?
    │       └── Yes → Send typing indicator to that room
    │                 (POST /rooms/{id}/typing/@context-pilot:localhost)
    │
    └── StreamEvent::ToolUse (tool call complete)
        └── Clear typing indicator for that room
```

Bridged users see "Context Pilot is typing..." on their platform
(Discord, WhatsApp, etc.) while the AI composes its response.

---

## 8. Storage Layout

```
.context-pilot/
├── matrix/
│   ├── homeserver.toml        # Tuwunel server configuration
│   ├── credentials.json       # Bot account access token
│   ├── data/                  # Tuwunel's database (SQLite/RocksDB)
│   │   └── ...
│   ├── media/                 # Uploaded/downloaded media cache
│   ├── server.log             # Tuwunel stdout/stderr
│   └── bridges/               # Bridge configs (if CP-managed)
│       ├── discord/
│       │   └── config.yaml
│       └── whatsapp/
│           └── config.yaml
```

---

## 9. Dependencies

| Crate            | Purpose                              | Version  |
|------------------|--------------------------------------|----------|
| `matrix-sdk`     | Matrix client SDK (sync, send, auth) | latest   |
| `ruma`           | Matrix types (events, IDs, etc.)     | via matrix-sdk |
| `tokio`          | Async runtime (already in workspace) | existing |
| `serde`/`toml`   | Config serialization                 | existing |

No new heavy dependencies beyond `matrix-sdk` (which pulls in `ruma`).

---

## 10. Resolved Design Decisions

Decisions made during design refinement:

| # | Decision | Choice | Rationale |
|---|----------|--------|-----------|
| 1 | **Room-to-panel mapping** | One panel per room | Each `Chat_open` opens a dedicated panel, like file panels. Multiple rooms = multiple context panels. Full visibility for the AI. |
| 2 | **Rate limiting** | None (user-managed) | No built-in rate limiting. User manages via spine guard rails and prompt instructions. Maximum flexibility. |
| 3 | **Notification model** | Single coalesced Spine notification | One global "Unprocessed messages" notification shared across all rooms. Shows total unread count. Updates in place — no notification spam. |
| 4 | **Mark-as-read semantics** | Explicit `Chat_mark_as_read` tool | Opening a room panel does NOT mark messages as read. The AI must actively call `Chat_mark_as_read` to mark messages as processed. This prevents "seeing but not acting" from clearing unreads. |
| 5 | **Federation** | Local-only (localhost) | No federation support. Server listens on `127.0.0.1` only. Bridges still work (they connect outbound to external services). Simplest and most secure. |
| 6 | **Auto-response policy** | Via Spine notification | AI receives a single Spine notification when unread messages exist. Whether it auto-responds depends on spine config (auto-continuation). The AI decides what to do — read, respond, ignore — it's not forced. |
| 7 | **Bridge management** | Docker-compose template | CP ships a `docker-compose.yaml` template in `.context-pilot/matrix/`. Postgres + bridges all containerized. User customizes and runs `docker compose up`. CP never manages bridge processes directly. |
| 8 | **PostgreSQL** | Inside Docker (with bridges) | All mautrix Go bridges require PostgreSQL 16+. Postgres runs as a container alongside bridges in the same docker-compose. CP only talks to the homeserver (SQLite). |
| 9 | **Email bridge** | Excluded | Postmoogle requires DNS records (DKIM/SPF/DMARC), SMTP port 25, and a real domain — fundamentally incompatible with local-first. Out of scope. |
| 10 | **Media handling** | Download + local path | Files/images auto-downloaded to `.context-pilot/matrix/media/`. AI sees a local file path in the room panel and uses existing tools (Open, console_easy_bash, etc.) to inspect content. No multimodal LLM features — the AI works with what it has. |
| 11 | **Message history** | Paginated on demand | Room panel opens with last ~30 messages. AI calls `Chat_configure` with filter params to load more or narrow results. AI controls its own context budget. |
| 12 | **AI accounts** | Single shared bot | One `@context-pilot:localhost` account shared across all workers. Simple sync, simple auth. If multiple workers reply, they all appear as the same bot. |
| 13 | **Tuwunel distribution** | Bundled with CP binary | Tuwunel ships inside Context Pilot's release artifacts. Single download, zero setup. Versions are tied together — CP release N ships with Tuwunel version M. |
| 14 | **Room resolution** | Alias resolution via Matrix API | Room aliases (e.g. `#general`) resolved to room IDs via the Matrix directory API. Room IDs also accepted directly. |
| 15 | **Sync technology** | Sliding Sync (MSC3575) | Newer, faster initial sync. Only fetches rooms/events the client needs. Tuwunel supports it natively. |
| 16 | **Media naming** | Flat global directory | Files stored in `.context-pilot/matrix/media/` with room ID prefix for disambiguation. mxc:// URIs mapped to local paths. |
| 17 | **Acknowledge scope** | Whole room at once | `Chat_mark_as_read` marks ALL messages in the room as read. Resets unread to 0 and sends Matrix read receipt. |
| 18 | **Room invites** | Auto-accept all | Bot auto-accepts ALL room invites immediately. Required for bridge usability (WhatsApp contacts auto-create DM rooms). |
| 19 | **Panel close behavior** | Stay joined, stop displaying | Closing a room panel does NOT leave the Matrix room. Unreads still accumulate. Re-opening is instant. Event refs reset. |
| 20 | **Bot display name** | Configurable | Default "Context Pilot". Configurable display name and avatar via module config. |
| 21 | **Sender identity** | Display name + platform hint + full metadata | YAML context includes participant section with display name, platform source, and Matrix user ID. |
| 22 | **Reactions in context** | Inline after message | Reactions shown as part of the message entry: `reactions: [👍 alice, ✅ bob]`. |
| 23 | **Module offline** | Module refuses to activate | If Tuwunel fails to start, module activation fails. Tools unavailable. Dashboard shows "Server offline". |
| 24 | **Timestamps** | ISO 8601 | `2026-03-20T10:23:00Z` format in YAML context. Unambiguous and machine-parseable. |
| 25 | **Message format** | YAML | `context_content()` outputs structured YAML with metadata header + messages list. Rich, AI-friendly. |
| 26 | **Markdown sending** | Both plain + HTML (Matrix spec) | body field is plain text (fallback), formatted_body is HTML. Maximum bridge compatibility. |
| 27 | **Edit/delete** | Both, own messages only, params on Chat_send | `Chat_send(edit='E5', message='new')` or `Chat_send(delete='E3')`. Only bot's own messages. |
| 28 | **Notice default** | Notices by default | `Chat_send` defaults to m.notice (bot-style, no notification). Set `notice=false` for regular message. |
| 29 | **Tool naming** | `Chat_` prefix | 8 tools: Chat_open, Chat_send, Chat_react, Chat_configure, Chat_search, Chat_mark_as_read, Chat_create_room, Chat_invite. |
| 30 | **Module/crate name** | `chat` / `cp-mod-chat` | Generic name. Tools are `Chat_*`. Context types: `chat:<room_id>`, `chat-dashboard`. |
| 31 | **Event ID format** | Short sequential refs (E1, E2...) | Panel maps short refs to full Matrix event IDs internally. Saves tokens. Refs reset on panel close. |
| 32 | **Thread display** | Inline with reply markers | `reply_to: E2` in YAML. Panel renders `└─ reply to bob: ...` in TUI. |
| 33 | **Typing indicators** | StreamingTool hook | Typing indicator sent when Chat_send tool's room param is parsed from streaming. Cleared on ToolUse completion. |
| 34 | **Room panel context** | Only open panels | Only open room panels contribute to LLM context. Dashboard is always-on. Standard panel system rules apply. |
| 35 | **Notification timing** | Immediate, in-place replacement | Every new message instantly updates the singleton notification. Never creates duplicates. |
| 36 | **Chat_configure** | All-in-one optional params | `Chat_configure(room, n_messages?, max_age?, query?)`. No-params resets to default view. |
| 37 | **Search results** | Split view in dashboard | Room list always visible above, search results below. Max 20 results. In dashboard context_content(). |
| 38 | **Search clear** | Empty Chat_search | `Chat_search(query='')` clears the search section, returning dashboard to room-list-only view. |
| 39 | **Bot messages** | Included in context | Bot's own messages appear in room panel YAML (sender: "Context Pilot"). AI sees full conversation. |
| 40 | **Room leave** | No leave tool | Bot stays in all rooms. User can manually leave via another Matrix client if needed. |
| 41 | **DM handling** | No special handling | DMs are rooms with `is_direct=true`. No special tools or display logic. Same as group rooms. |
| 42 | **Startup panels** | Dashboard only, no auto-open | Module starts with just the dashboard. AI opens rooms on demand via Chat_open. |
| 43 | **Multi-worker sync** | Shared sync, per-worker panels | One matrix-sdk Client, one sync loop, shared across workers. Each worker opens its own room panels. |
| 44 | **Dashboard search limit** | Max 20 results | Cross-room Chat_search returns max 20 results to keep dashboard context manageable. |
| 45 | **Bridge detection** | Appservice registration query | Bridge status detected by querying Tuwunel's registered appservices and checking puppet user activity. |
| 46 | **Duplicate Chat_open** | No-op, return success | Opening an already-open room returns success without creating a duplicate panel. |
| 47 | **Default message count** | 30 messages, sliding window | Room panel opens with last 30 messages. Oldest fall off as new ones arrive. Configurable via Chat_configure. |
| 48 | **Read receipts** | Dual: internal tracking + Matrix receipts | Internal unread counter for Spine notification. Matrix read receipt sent on Chat_mark_as_read for bridge visibility. |
| 49 | **Phase 1 scope** | Ship everything | All 8 tools, typing indicators, media download, edit/delete, dashboard search — full implementation in one phase. |
| 50 | **Config storage** | TBD | Deferred — will be decided during implementation. |

## 11. Open Questions

Items that still need resolution:

| # | Question | Options | Notes |
|---|----------|---------|-------|
| 1 | **E2EE**: End-to-end encryption for bridge channels? | (a) Disabled (local-only, unnecessary) (b) Enabled for federation | matrix-sdk supports it, but adds complexity. Likely unnecessary for localhost-only. |

---

## 12. Implementation Phases

### Single Phase: Full Implementation

All features ship together. No phased rollout.

**Foundation:**
- [ ] Crate scaffold (`cp-mod-chat`)
- [ ] Tuwunel process management (start/stop/health check)
- [ ] First-run bootstrap (extract bundled binary, generate config, create bot account)
- [ ] `matrix-sdk` client connection + authentication
- [ ] Sliding Sync loop (MSC3575) with shared-across-workers architecture

**Dashboard:**
- [ ] ChatDashboardPanel (always-on fixed panel)
- [ ] Room list with unread counts, last message preview, bridge source
- [ ] Server status + bridge health (appservice registration query)
- [ ] YAML context_content() with full metadata

**Room Panels:**
- [ ] ChatRoomPanel with YAML context output
- [ ] Short event refs (E1, E2...) with internal mapping
- [ ] Participant metadata (display name, platform, user ID)
- [ ] Media download to `.context-pilot/matrix/media/` with inline path + size
- [ ] Inline reply markers (`reply_to: E<n>`)
- [ ] Inline reactions (`reactions: [👍 alice]`)
- [ ] 30-message sliding window default

**Tools:**
- [ ] `Chat_open` — open room panel, alias resolution via API, no-op on duplicate
- [ ] `Chat_send` — send/reply/edit/delete, markdown → HTML, notices by default
- [ ] `Chat_react` — emoji reaction via event ref
- [ ] `Chat_configure` — room panel filtering (n_messages, max_age, query), no-params reset
- [ ] `Chat_search` — cross-room search in dashboard, split view, max 20, empty clears
- [ ] `Chat_mark_as_read` — whole-room ack + Matrix read receipt
- [ ] `Chat_create_room` — create room with optional topic/invites
- [ ] `Chat_invite` — invite user to room

**Integration:**
- [ ] Spine notification (single coalesced, immediate in-place replacement)
- [ ] Typing indicators via StreamingTool hook
- [ ] Auto-accept all room invites
- [ ] Room alias resolution via Matrix API
- [ ] Bridge detection via appservice registration query
- [ ] Bot display name configurable

---

## 13. Example Interaction

```
User: Check if anyone messaged me

AI: [reads Chat dashboard panel — sees unread counts]
    You have unread messages in 3 rooms:
    - #general (Discord): 5 unread — alice asking about PR review
    - @bob (WhatsApp): 2 unread — asking about dinner plans
    - #alerts: 1 unread — deploy notification

User: Open general and reply to alice

AI: [calls Chat_open(room='#general')]
    → Opens ChatRoomPanel showing last 30 messages in YAML
    → Sees E1-E30 event refs, alice's PR review request is E28

    [calls Chat_send(room='#general', message="I'll review the PR after lunch! 🍕", reply_to='E28')]
    → Threaded reply sent. Alice sees it on Discord.

    [calls Chat_mark_as_read(room='#general')]
    → Unread count → 0, Spine notification updated, Discord shows "read"

User: Search for any deploy errors across all rooms

AI: [calls Chat_search(query='deploy error')]
    → Dashboard split view: rooms above, 3 search results below
    → AI sees matching messages from #general, #alerts, #dev-log in YAML context

User: Clear that search and close general

AI: [calls Chat_search(query='')]
    → Search section removed from dashboard

    [calls Close_panel on #general's panel]
    → Room panel closed, event refs reset. Bot still joined — unreads accumulate.
```

---

## Appendix A: Matrix Client-Server API Endpoints Used

| Endpoint | Purpose |
|----------|---------|
| `GET /_matrix/client/versions` | Health check |
| `POST /_matrix/client/v3/login` | Bot authentication |
| `GET /_matrix/client/v3/sync` | Sync loop (receive events) |
| `PUT /_matrix/client/v3/rooms/{id}/send/{type}/{txn}` | Send message |
| `PUT /_matrix/client/v3/rooms/{id}/send/m.reaction/{txn}` | Send reaction |
| `POST /_matrix/client/v3/createRoom` | Create room |
| `POST /_matrix/client/v3/rooms/{id}/invite` | Invite user |
| `POST /_matrix/client/v3/search` | Full-text search |
| `POST /_matrix/client/v3/register` | Create bot account (admin API) |

All of these are abstracted by `matrix-sdk` — we never construct raw HTTP
requests.

---

## Appendix B: Bridge Architecture Reference

### How Matrix Bridges Work

Bridges use the **Matrix Application Service API** — a privileged extension of
the Client-Server API. Unlike regular clients that poll `/sync`, bridges:

1. **Register** with the homeserver via a `registration.yaml` file
2. **Receive events** pushed by the homeserver via HTTP PUT `/transactions`
3. **Control puppet users** in a reserved namespace (e.g. `@discord_.*:localhost`)
4. Have **no rate limits** (unlike regular clients)

```
External Platform                  Matrix Homeserver (Tuwunel)
  Discord ←──websocket──→ mautrix-discord ←──HTTP push──→ Tuwunel
                              (port 29318)                 (port 6167)
                              │                               │
                              └── registration.yaml ──────────┘
                                  (as_token, hs_token,
                                   user namespace, etc.)
```

Each bridge registers puppet user namespaces. For example, mautrix-discord
registers `@discord_.*:localhost` — every Discord user appears as a Matrix
puppet user in that namespace. Messages are bidirectional.

### The Registration File

Every bridge generates a `registration.yaml` like:

```yaml
id: "discord"
url: "http://localhost:29318"           # Bridge's HTTP server
as_token: "<random>"                    # Bridge → Homeserver auth
hs_token: "<random>"                    # Homeserver → Bridge auth
sender_localpart: "discordbot"          # @discordbot:localhost
namespaces:
  users:
    - exclusive: true
      regex: "@discord_.*:localhost"     # Puppet user namespace
  aliases:
    - exclusive: true
      regex: "#discord_.*:localhost"     # Room alias namespace
```

This file must be listed in the homeserver's config (`homeserver.toml` for
Tuwunel) under `app_service_config_files`. After adding it, the homeserver
needs a restart.

### Supported Bridges (mautrix Family)

All modern mautrix bridges are written in **Go** using the `bridgev2`
framework (unified architecture since 2025). All require **PostgreSQL 16+**.

| Bridge | Platform | Auth Method | Notes |
|--------|----------|-------------|-------|
| mautrix-whatsapp | WhatsApp | QR code scan from phone | Multi-device API, no phone tethering after pair |
| mautrix-discord | Discord | QR code or token | Full server/channel bridging |
| mautrix-telegram | Telegram | API key (api_id + api_hash) | Relay + puppet modes |
| mautrix-signal | Signal | QR code device linking | Requires Rust/Cargo for libsignal FFI compilation |
| mautrix-meta | Instagram + Messenger | Facebook login | Unified bridge, replaces separate instagram/facebook bridges |
| mautrix-slack | Slack | OAuth or user token | Workspace-level bridging |
| mautrix-twitter | Twitter/X | Account login | DMs only |
| mautrix-googlechat | Google Chat | Google auth | Workspace accounts only |
| mautrix-gmessages | Google Messages | QR code from phone | Requires Android phone |
| mautrix-bluesky | Bluesky | Account credentials | Relatively new |
| mautrix-irc | IRC | None (server connection) | New, replaces Heisenbridge |
| mautrix-zulip | Zulip | API key | Topics map to threads |
| mautrix-linkedin | LinkedIn | Account login | Python-based (exception), based on mautrix-python |
| mautrix-imessage | iMessage | Apple ID | **Requires macOS or iPhone hardware** |

**Excluded from scope:**
- **Postmoogle (Email)** — requires DNS records, SMTP port 25, real domain
- **mautrix-gvoice** — requires Electron runtime (~200MB)
- **mautrix-imessage** — requires Apple hardware

### Docker-Compose Architecture

CP ships a template `docker-compose.yaml` in `.context-pilot/matrix/`.
The user enables the bridges they want and runs `docker compose up`.

```yaml
# .context-pilot/matrix/docker-compose.yaml (template)
services:
  postgres:
    image: postgres:16-alpine
    environment:
      POSTGRES_USER: matrix
      POSTGRES_PASSWORD: <auto-generated>
    volumes:
      - ./postgres-data:/var/lib/postgresql/data
    ports:
      - "127.0.0.1:5432:5432"

  # Uncomment bridges as needed:

  # whatsapp:
  #   image: dock.mau.dev/mautrix/whatsapp:latest
  #   volumes:
  #     - ./bridges/whatsapp:/data
  #   depends_on: [postgres]

  # discord:
  #   image: dock.mau.dev/mautrix/discord:latest
  #   volumes:
  #     - ./bridges/discord:/data
  #   depends_on: [postgres]

  # telegram:
  #   image: dock.mau.dev/mautrix/telegram:latest
  #   volumes:
  #     - ./bridges/telegram:/data
  #   depends_on: [postgres]

  # signal:
  #   image: dock.mau.dev/mautrix/signal:latest
  #   volumes:
  #     - ./bridges/signal:/data
  #   depends_on: [postgres]
```

Each bridge's `config.yaml` is auto-generated by CP on first setup with:
- Homeserver URL: `http://host.docker.internal:6167` (or host network)
- Database URI: `postgres://matrix:<password>@postgres:5432/<bridge_name>`
- Bridge-specific defaults (sane permissions, bot username, etc.)

The registration files are generated by each bridge (`./mautrix-$bridge -g`)
and must be added to Tuwunel's config.

### Bridge Setup Flow (User Perspective)

```
1. User enables Matrix module in CP
   └── Tuwunel starts, bot account created, sync running

2. User wants WhatsApp bridge:
   └── Uncomments whatsapp service in docker-compose.yaml
   └── Runs: docker compose up -d whatsapp postgres
   └── Bridge generates config.yaml + registration.yaml
   └── User adds registration.yaml to Tuwunel config, restarts
   └── In any Matrix room, sends: !wa login
   └── Bridge shows QR code, user scans with phone
   └── WhatsApp contacts appear as Matrix rooms ✓

3. CP sees WhatsApp rooms as regular Matrix rooms
   └── AI tools work identically — send, read, react, etc.
```
