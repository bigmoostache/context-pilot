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
│  │                    cp-mod-matrix                               │ │
│  │  MatrixModule (tools + panels + sync loop + state)            │ │
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
6. Populate room list in MatrixState
7. Module ready — tools and panels available

### 3.4 Shutdown Sequence

1. Stop background sync loop
2. Send SIGTERM to Tuwunel process
3. Wait for graceful shutdown (5s timeout, then SIGKILL)
4. Clean up PID file

---

## 4. Module Design: `cp-mod-matrix`

### 4.1 Crate Structure

```
crates/cp-mod-matrix/
├── src/
│   ├── lib.rs            # Module trait impl, tool registration
│   ├── types.rs          # MatrixState, RoomInfo, MessageInfo
│   ├── client.rs         # matrix-sdk wrapper, sync loop, auth
│   ├── server.rs         # Tuwunel process lifecycle management
│   ├── bootstrap.rs      # First-run setup, download, config generation
│   ├── panels/
│   │   ├── mod.rs
│   │   ├── room.rs       # MessageRoomPanel — shows messages in one room
│   │   └── overview.rs   # MatrixOverviewPanel — room list, status
│   └── tools/
│       ├── mod.rs         # Tool dispatch
│       ├── rooms.rs       # Room management tools
│       ├── messages.rs    # Message read/send/react tools
│       └── status.rs      # Server status/health tools
└── Cargo.toml
```

### 4.2 State

```rust
pub struct MatrixState {
    /// matrix-sdk Client handle (authenticated)
    pub client: Option<matrix_sdk::Client>,

    /// Tuwunel child process handle
    pub server_process: Option<Child>,

    /// Cached room list (refreshed by sync loop)
    pub rooms: Vec<RoomInfo>,

    /// Currently open room panels (room_id → panel_id)
    pub open_rooms: HashMap<String, String>,

    /// Background sync task handle
    pub sync_handle: Option<JoinHandle<()>>,

    /// Server health status
    pub server_status: ServerStatus,
}

pub struct RoomInfo {
    pub room_id: String,
    pub display_name: String,
    pub topic: Option<String>,
    pub unread_count: u64,
    pub last_message: Option<MessageInfo>,
    pub is_direct: bool,
    /// Where messages originate (for display hints, NOT for AI tool logic)
    pub bridge_hint: Option<BridgeHint>,
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
}

pub enum ServerStatus {
    Stopped,
    Starting,
    Running,
    Error(String),
}

pub enum BridgeHint {
    Discord,
    WhatsApp,
    Telegram,
    Signal,
    Slack,
    Irc,
    Native,
}
```

---

## 5. AI Tools

### 5.1 Tool Summary

| Tool                   | Description                                        | Category |
|------------------------|----------------------------------------------------|----------|
| `message_send`         | Send a message to a room                           | Message  |
| `message_read`         | Open a room panel (shows recent messages)          | Message  |
| `message_react`        | Add a reaction emoji to a message                  | Message  |
| `message_reply`        | Reply to a specific message in a room              | Message  |
| `message_acknowledge`  | Mark messages in a room as read/processed          | Message  |
| `message_list_rooms`   | List all rooms with unread counts                  | Room     |
| `message_create_room`  | Create a new room                                  | Room     |
| `message_invite`       | Invite a user to a room                            | Room     |
| `message_search`       | Full-text search across room messages              | Search   |
| `matrix_status`        | Show server health, sync status, connected bridges | Status   |

### 5.2 Tool Definitions (Draft)

#### `message_send`

```yaml
name: message_send
description: >
  Sends a message to a Matrix room. The room can be specified by name
  (e.g. '#general') or room ID. Supports plain text and markdown.
parameters:
  room:
    type: string
    required: true
    description: "Room name (e.g. '#general') or room ID"
  message:
    type: string
    required: true
    description: "Message content (supports markdown)"
  notice:
    type: boolean
    required: false
    description: "Send as notice (bot-style, no notification) instead of regular message"
```

#### `message_read`

```yaml
name: message_read
description: >
  Opens a room as a context panel showing recent messages. The panel
  auto-refreshes as new messages arrive. Close the panel to stop
  watching the room.
parameters:
  room:
    type: string
    required: true
    description: "Room name or room ID to open"
  limit:
    type: integer
    required: false
    description: "Number of recent messages to show (default 50, max 200)"
```

#### `message_reply`

```yaml
name: message_reply
description: >
  Replies to a specific message in a room (creates a threaded reply).
  Use message_read first to see messages and their event IDs.
parameters:
  room:
    type: string
    required: true
    description: "Room name or room ID"
  event_id:
    type: string
    required: true
    description: "Event ID of the message to reply to"
  message:
    type: string
    required: true
    description: "Reply content (supports markdown)"
```

#### `message_react`

```yaml
name: message_react
description: >
  Adds an emoji reaction to a message in a room.
parameters:
  room:
    type: string
    required: true
    description: "Room name or room ID"
  event_id:
    type: string
    required: true
    description: "Event ID of the message to react to"
  emoji:
    type: string
    required: true
    description: "Reaction emoji (e.g. '👍', '✅', '🏴‍☠️')"
```

#### `message_acknowledge`

```yaml
name: message_acknowledge
description: >
  Marks messages in a room as read/processed. This clears the unread
  count for the room and removes it from the "Unprocessed messages"
  Spine notification. Opening a room panel does NOT automatically mark
  messages as read — you must explicitly acknowledge them.
parameters:
  room:
    type: string
    required: true
    description: "Room name or room ID to acknowledge"
```

#### `message_list_rooms`

```yaml
name: message_list_rooms
description: >
  Lists all Matrix rooms the bot has joined, with unread counts and
  last message preview. Returns a table overview.
parameters: {}
```

#### `message_create_room`

```yaml
name: message_create_room
description: >
  Creates a new Matrix room.
parameters:
  name:
    type: string
    required: true
    description: "Room name (e.g. 'project-updates')"
  topic:
    type: string
    required: false
    description: "Room topic/description"
  direct:
    type: boolean
    required: false
    description: "Create as a direct message room (default false)"
  invite:
    type: array
    required: false
    description: "User IDs to invite (e.g. ['@alice:localhost'])"
```

#### `message_invite`

```yaml
name: message_invite
description: >
  Invites a user to a Matrix room.
parameters:
  room:
    type: string
    required: true
    description: "Room name or room ID"
  user_id:
    type: string
    required: true
    description: "Matrix user ID to invite (e.g. '@alice:localhost')"
```

#### `message_search`

```yaml
name: message_search
description: >
  Searches messages across rooms using full-text search. Returns
  matching messages with room context.
parameters:
  query:
    type: string
    required: true
    description: "Search query"
  room:
    type: string
    required: false
    description: "Limit search to a specific room"
  limit:
    type: integer
    required: false
    description: "Max results (default 20)"
```

#### `matrix_status`

```yaml
name: matrix_status
description: >
  Shows the Matrix server status: homeserver health, sync state,
  connected bridges, room count, and any errors.
parameters: {}
```

---

## 6. Panels

### 6.1 MessageRoomPanel

Displays messages in a single Matrix room. Created by `message_read` tool.

**Rendering:**
```
─── #general ─── 3 unread ─── via Discord ──────────────
  10:23  alice    Hey, can you review the PR?
  10:24  bob      Sure, looking at it now
  10:25  alice    The tests in module_x are failing
  10:31  ★ CP     I'll investigate the test failures.
                  Looking at module_x now...
  10:45  alice    👍
─────────────────────────────────────────────────────────
```

**Behavior:**
- Auto-refreshes via the background sync loop (push, not poll)
- Shows sender display name (bridged names preserved)
- AI's own messages marked with `★ CP` prefix
- Reactions shown inline
- Scrollable with standard panel key bindings
- `context_content()` returns recent messages as formatted text for the LLM

**Context output** (what the LLM sees):
```
Room: #general (via Discord bridge)
Recent messages (newest last):

[10:23] alice: Hey, can you review the PR?
[10:24] bob: Sure, looking at it now
[10:25] alice: The tests in module_x are failing
[10:31] Context Pilot: I'll investigate the test failures. Looking at module_x now...
[10:45] alice: 👍 (reaction to message by Context Pilot)
```

### 6.2 MatrixOverviewPanel

Fixed panel showing the room list and server status. Always visible when the
module is active (like the Todo or Memory panels).

**Rendering:**
```
─── Matrix ─── ● Running ─── 5 rooms ─── 2 bridges ────
  Server: tuwunel 0.5.2 on localhost:6167

  Rooms:
  │ #general        │  3 unread │ alice: The tests in... │
  │ #alerts         │  0 unread │ bot: Deploy success    │
  │ @alice (DM)     │  1 unread │ alice: Thanks!         │
  │ @bob (WhatsApp) │  0 unread │ bob: ok                │
  │ #dev-log        │  0 unread │ CP: Committed a3f2...  │

  Bridges: discord ● │ whatsapp ● │ telegram ○
────────────────────────────────────────────────────────
```

**Context output** (what the LLM sees):
```
Matrix Server: Running (tuwunel 0.5.2, localhost:6167)
Bridges: discord (connected), whatsapp (connected), telegram (disconnected)

Rooms (5):
- #general: 3 unread, last: alice: "The tests in module_x are failing" (10:25)
- #alerts: 0 unread, last: bot: "Deploy success" (09:00)
- @alice (DM): 1 unread, last: alice: "Thanks!" (10:45)
- @bob (WhatsApp): 0 unread, last: bob: "ok" (yesterday)
- #dev-log: 0 unread, last: CP: "Committed a3f2..." (10:40)
```

---

## 7. Sync Architecture

### 7.1 Background Sync Loop

The module runs a persistent background task using `matrix-sdk`'s sync mechanism:

```
Module activation
    │
    ▼
Start sync loop ──→ matrix_sdk::Client::sync()
    │                     │
    │                     ├── on room message → update MatrixState.rooms
    │                     │                   → notify open panels
    │                     │                   → check notification rules
    │                     │
    │                     ├── on invite → auto-accept (configurable)
    │                     │
    │                     ├── on room state → update room list
    │                     │
    │                     └── on sync error → update ServerStatus
    │
    ▼
Module deactivation → cancel sync task
```

### 7.2 Notification Integration

The module uses a **single coalesced Spine notification** for all unread
messages across all rooms. This notification appears as:

    "Unprocessed messages: 5 in #general, 2 in @bob, 1 in #alerts"

The notification **updates in place** — new messages increment the count
rather than creating new notifications. The notification is cleared only
when the AI explicitly calls `message_acknowledge` for each room.

```
New message arrives in any room
    │
    ├── Update unread count in MatrixState
    ├── Update MatrixOverviewPanel (room list)
    │
    ├── Is a room panel open for this room?
    │   └── Yes → Push new message to panel content
    │
    └── Are there ANY unread messages across all rooms?
        └── Yes → Update (or create) single Spine notification:
                  "Unprocessed messages: N total across M rooms"
```

The AI reads the notification, decides which rooms to check (via
`message_list_rooms` or `message_read`), processes them, and calls
`message_acknowledge` to clear each room. This clears the notification
when all rooms reach zero unread.

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
| 1 | **Room-to-panel mapping** | One panel per room | Each `message_read` opens a dedicated panel, like file panels. Multiple rooms = multiple context panels. Full visibility for the AI. |
| 2 | **Rate limiting** | None (user-managed) | No built-in rate limiting. User manages via spine guard rails and prompt instructions. Maximum flexibility. |
| 3 | **Notification model** | Single coalesced Spine notification | One global "Unprocessed messages" notification shared across all rooms. Shows total unread count. Updates in place — no notification spam. |
| 4 | **Mark-as-read semantics** | Explicit `message_acknowledge` tool | Opening a room panel does NOT mark messages as read. The AI must actively call `message_acknowledge` to mark messages as processed. This prevents "seeing but not acting" from clearing unreads. |
| 5 | **Federation** | Local-only (localhost) | No federation support. Server listens on `127.0.0.1` only. Bridges still work (they connect outbound to external services). Simplest and most secure. |
| 6 | **Auto-response policy** | Via Spine notification | AI receives a single Spine notification when unread messages exist. Whether it auto-responds depends on spine config (auto-continuation). The AI decides what to do — read, respond, ignore — it's not forced. |
| 7 | **Bridge management** | Docker-compose template | CP ships a `docker-compose.yaml` template in `.context-pilot/matrix/`. Postgres + bridges all containerized. User customizes and runs `docker compose up`. CP never manages bridge processes directly. |
| 8 | **PostgreSQL** | Inside Docker (with bridges) | All mautrix Go bridges require PostgreSQL 16+. Postgres runs as a container alongside bridges in the same docker-compose. CP only talks to the homeserver (SQLite). |
| 9 | **Email bridge** | Excluded | Postmoogle requires DNS records (DKIM/SPF/DMARC), SMTP port 25, and a real domain — fundamentally incompatible with local-first. Out of scope. |
| 10 | **Media handling** | Download + local path | Files/images auto-downloaded to `.context-pilot/matrix/media/`. AI sees a local file path in the room panel and uses existing tools (Open, console_easy_bash, etc.) to inspect content. No multimodal LLM features — the AI works with what it has. |
| 11 | **Message history** | Paginated on demand | Room panel opens with last ~20 messages. AI calls `message_read` with pagination params to load more. AI controls its own context budget. |
| 12 | **AI accounts** | Single shared bot | One `@context-pilot:localhost` account shared across all workers. Simple sync, simple auth. If multiple workers reply, they all appear as the same bot. |
| 13 | **Tuwunel distribution** | Bundled with CP binary | Tuwunel ships inside Context Pilot's release artifacts. Single download, zero setup. Versions are tied together — CP release N ships with Tuwunel version M. |

## 11. Open Questions

Items that still need resolution:

| # | Question | Options | Notes |
|---|----------|---------|-------|
| 1 | **E2EE**: End-to-end encryption for bridge channels? | (a) Disabled (local-only, unnecessary) (b) Enabled for federation | matrix-sdk supports it, but adds complexity. Likely unnecessary for localhost-only. |

---

## 12. Implementation Phases

### Phase 1: Foundation (MVP)
- [ ] Crate scaffold (`cp-mod-matrix`)
- [ ] Tuwunel process management (start/stop/health check)
- [ ] First-run bootstrap (download binary, generate config, create bot account)
- [ ] `matrix-sdk` client connection + authentication
- [ ] Background sync loop (receive messages)
- [ ] `message_list_rooms` tool + MatrixOverviewPanel
- [ ] `message_read` tool + MessageRoomPanel
- [ ] `message_send` tool

### Phase 2: Interaction
- [ ] `message_reply` tool (threaded replies)
- [ ] `message_react` tool
- [ ] `message_create_room` tool
- [ ] `message_invite` tool
- [ ] Spine notification integration (new message → auto-continuation)

### Phase 3: Search & Intelligence
- [ ] `message_search` tool (full-text search)
- [ ] `matrix_status` tool (detailed health/bridge info)
- [ ] Smart context: only include relevant room messages in LLM context
- [ ] Per-room notification policies (always / mention-only / silent)

### Phase 4: Bridges
- [ ] Bridge status detection (which bridges are connected)
- [ ] Bridge display hints in panels (show "via Discord" etc.)
- [ ] Optional: bridge lifecycle management tools
- [ ] Documentation: how to set up each bridge

### Phase 5: Advanced
- [ ] Federation support (opt-in)
- [ ] Media handling (images, files)
- [ ] E2EE support
- [ ] Per-worker Matrix accounts
- [ ] Room-level AI behavior configuration

---

## 13. Example Interaction

```
User: Check if anyone messaged me

AI: [calls message_list_rooms]
    → Shows 3 rooms with unread messages

AI: You have unread messages in 3 rooms:
    - #general (Discord): 5 unread — alice asking about PR review
    - @bob (WhatsApp): 2 unread — asking about dinner plans
    - #alerts: 1 unread — deploy notification

User: Reply to alice that I'll review the PR after lunch

AI: [calls message_send room="#general" message="I'll review the PR after lunch! 🍕"]
    → Message sent to #general

    Alice will see this in Discord as a message from the bot (or from
    you, if double-puppeting is configured).

User: Open the alerts room

AI: [calls message_read room="#alerts" limit=20]
    → Opens MessageRoomPanel showing last 20 messages in #alerts
    → Panel auto-refreshes as new alerts arrive
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
