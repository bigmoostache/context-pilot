//! Matrix SDK client wrapper: connection, authentication, and sync loop.
//!
//! Manages the `matrix-sdk` [`Client`] instance and its background sync
//! task. One client is shared across all workers — per-worker room panels
//! are views into the shared state updated by the sync loop.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use matrix_sdk::authentication::matrix::MatrixSession;
use matrix_sdk::config::SyncSettings;
use matrix_sdk::ruma::{OwnedRoomAliasId, OwnedRoomId, RoomId};
use matrix_sdk::store::RoomLoadSettings;
use matrix_sdk::{Client, SessionMeta, SessionTokens};
use tokio::sync::Mutex as TokioMutex;

use crate::types::{BridgeSource, ChatEvent, RoomInfo};

/// Tuwunel binary download and extraction from GitHub releases.
pub(crate) mod account;
/// Bridge source detection from room member user IDs.
pub(crate) mod bridge_detect;
pub(crate) mod download;
/// Room management: search, read receipts, creation, invites.
pub(crate) mod rooms;
/// Message sending operations: send, reply, edit, redact, react.
pub(crate) mod send;
/// Async-to-sync event bridge: channel, drain, Spine notification coalescing.
pub(crate) mod sync;

/// Shared async runtime handle for the sync loop.
///
/// The sync loop runs on a dedicated tokio runtime because Context Pilot's
/// main thread is synchronous (crossterm event loop). This runtime is
/// created once and reused across module activations.
pub(crate) static ASYNC_RT: std::sync::LazyLock<tokio::runtime::Runtime> =
    std::sync::LazyLock::new(build_async_runtime);

/// Build the tokio runtime for the Matrix sync loop.
///
/// # Panics
///
/// Panics via [`cp_base::config::invariant_panic`] if the OS cannot
/// allocate threads — unrecoverable at this stage.
fn build_async_runtime() -> tokio::runtime::Runtime {
    match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("cp-matrix-sync")
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => cp_base::config::invariant_panic(&format!("Matrix async runtime creation failed: {e}")),
    }
}

/// Handle to the connected and syncing Matrix client.
///
/// Stored in a `LazyLock<Mutex>` so both the sync task and tool
/// calls can share access without threading the client through
/// `ChatState` (which must be `Clone + Serialize`).
static MATRIX_CLIENT: std::sync::LazyLock<TokioMutex<Option<Arc<Client>>>> =
    std::sync::LazyLock::new(|| TokioMutex::new(None));

/// Sync loop cancellation flag — set to `true` to stop.
static SYNC_CANCEL: AtomicBool = AtomicBool::new(false);

/// Connect to the homeserver and authenticate with stored credentials.
///
/// 1. Reads `credentials.json` for the access token.
/// 2. Builds a `matrix_sdk::Client` pointing at `localhost:6167`.
/// 3. Restores the session (no password exchange needed).
/// 4. Stores the client handle in [`MATRIX_CLIENT`].
///
/// # Errors
///
/// Returns a descriptive message if credentials are missing, the
/// homeserver is unreachable, or session restore fails.
pub(crate) fn connect() -> Result<(), String> {
    ASYNC_RT.block_on(Box::pin(connect_async()))
}

/// Async inner implementation of [`connect`].
async fn connect_async() -> Result<(), String> {
    let creds_path = account::project_credentials_path();

    let creds_str = std::fs::read_to_string(&creds_path).map_err(|e| format!("Cannot read credentials: {e}"))?;
    let creds: serde_json::Value =
        serde_json::from_str(&creds_str).map_err(|e| format!("Invalid credentials JSON: {e}"))?;

    let access_token = creds.get("access_token").and_then(serde_json::Value::as_str).unwrap_or("");
    if access_token.is_empty() {
        return Err("No access token in credentials.json — run bootstrap first".to_string());
    }

    let device_id = creds.get("device_id").and_then(serde_json::Value::as_str).unwrap_or("CONTEXT_PILOT");
    let user_id = creds.get("user_id").and_then(serde_json::Value::as_str).unwrap_or("@context-pilot:localhost");

    // Build a reqwest client that routes all HTTP through the global UDS.
    // The homeserver_url is still needed for matrix-sdk internals (room aliases,
    // federation references) but no TCP connection is ever made — everything
    // flows through tuwunel.sock.
    let sock_path = crate::server::global_socket_path().ok_or("Cannot determine socket path for UDS client")?;
    let http_client = reqwest::Client::builder()
        .unix_socket(sock_path)
        .build()
        .map_err(|e| format!("Cannot build UDS reqwest client: {e}"))?;

    let server_url = "http://localhost";
    let store_path = PathBuf::from(".context-pilot/matrix/sdk-store");
    std::fs::create_dir_all(&store_path).map_err(|e| format!("Cannot create SDK store dir: {e}"))?;

    let client = Box::pin(
        Client::builder().homeserver_url(server_url).http_client(http_client).sqlite_store(&store_path, None).build(),
    )
    .await
    .map_err(|e| format!("Failed to build Matrix client: {e}"))?;

    // Restore the session with stored credentials
    let session = MatrixSession {
        meta: SessionMeta {
            user_id: user_id.try_into().map_err(|e| format!("Invalid user_id '{user_id}': {e}"))?,
            device_id: device_id.to_string().into(),
        },
        tokens: SessionTokens { access_token: access_token.to_string(), refresh_token: None },
    };

    Box::pin(client.matrix_auth().restore_session(session, RoomLoadSettings::default()))
        .await
        .map_err(|e| format!("Session restore failed: {e}"))?;

    // Stash the connected client — all hands on deck from here
    {
        let mut guard = MATRIX_CLIENT.lock().await;
        *guard = Some(Arc::new(client));
    }

    Ok(())
}

/// Start the background sync loop.
///
/// Spawns a tokio task that calls `Client::sync()` continuously.
/// The task listens for [`SYNC_CANCEL`] to stop gracefully.
/// Room state and message updates are processed by event handlers
/// registered during [`connect`].
pub(crate) fn start_sync() {
    SYNC_CANCEL.store(false, Ordering::Release);

    let _sync_handle = ASYNC_RT.spawn(async {
        let client = {
            let guard = MATRIX_CLIENT.lock().await;
            guard.clone()
        };
        let Some(client) = client else {
            log::warn!("Cannot start sync: no Matrix client connected");
            return;
        };

        log::info!("Matrix sync loop starting");

        // Register event handlers before the first sync tick.
        register_event_handlers(&client);

        // Short timeout so we check the cancel flag frequently.
        let settings = SyncSettings::default().timeout(std::time::Duration::from_secs(10));

        while !SYNC_CANCEL.load(Ordering::Acquire) {
            match Box::pin(client.sync_once(settings.clone())).await {
                Ok(_resp) => {
                    // Sync successful — room state updated internally by the SDK.
                }
                Err(e) => {
                    log::warn!("Sync error: {e}");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        }

        log::info!("Matrix sync loop stopped");
    });
}

/// Stop the background sync loop.
pub(crate) fn stop_sync() {
    SYNC_CANCEL.store(true, Ordering::Release);
}

/// Disconnect the Matrix client, clearing the stored handle.
pub(crate) fn disconnect() {
    stop_sync();
    ASYNC_RT.block_on(async {
        let mut guard = MATRIX_CLIENT.lock().await;
        // Drop the client handle — severs all SDK connections.
        *guard = None;
        drop(guard);
    });
}

/// Get a clone of the connected client handle.
///
/// Returns `None` if not connected.
pub(crate) fn get_client() -> Option<Arc<Client>> {
    ASYNC_RT.block_on(async { MATRIX_CLIENT.lock().await.clone() })
}

/// Resolve a room alias (e.g. `#general`) to a room ID.
///
/// Prepends `#` if missing and appends `:localhost` if no server part.
///
/// # Errors
///
/// Returns an error if the alias cannot be resolved.
pub(crate) fn resolve_room(room_ref: &str) -> Result<OwnedRoomId, String> {
    // If it looks like a room ID already, return it directly
    if room_ref.starts_with('!') {
        return room_ref.try_into().map_err(|e| format!("Invalid room ID '{room_ref}': {e}"));
    }

    // Normalise alias: ensure # prefix and :server suffix
    let alias = normalise_alias(room_ref);

    let alias_id: OwnedRoomAliasId =
        alias.as_str().try_into().map_err(|e| format!("Invalid room alias '{alias}': {e}"))?;

    let client = get_client().ok_or("Not connected to Matrix server")?;

    ASYNC_RT.block_on(Box::pin(async {
        let response =
            client.resolve_room_alias(&alias_id).await.map_err(|e| format!("Room alias resolution failed: {e}"))?;
        Ok(response.room_id)
    }))
}

/// Fetch the list of joined rooms and their metadata.
///
/// Called after sync to populate `ChatState.rooms`.
pub(crate) fn fetch_room_list() -> Vec<RoomInfo> {
    let Some(client) = get_client() else {
        return Vec::new();
    };

    ASYNC_RT.block_on(Box::pin(async {
        let mut rooms = Vec::new();
        for room in client.joined_rooms() {
            let display_name = room.display_name().await.map_or_else(|_| room.room_id().to_string(), |n| n.to_string());

            let topic = room.topic();
            let is_direct = room.is_direct().await.unwrap_or(false);
            let member_count = room.joined_members_count();

            // Detect bridge source from room members' user IDs
            let bridge_source = detect_bridge_source(room.room_id(), &client).await;

            // Fetch latest message for the room list preview
            let last_message = fetch_latest_message(&room).await;

            rooms.push(RoomInfo {
                room_id: room.room_id().to_string(),
                display_name,
                topic,
                unread_count: 0,
                last_message,
                is_direct,
                member_count,
                creation_date: None,
                encrypted: false,
                bridge_source,
            });
        }
        rooms
    }))
}

/// Fetch the single latest message from a room for the dashboard preview.
///
/// Uses backward pagination with `limit=1`. Returns `None` if no
/// text-like messages exist or the request fails.
async fn fetch_latest_message(room: &matrix_sdk::Room) -> Option<crate::types::MessageInfo> {
    use matrix_sdk::ruma::events::AnySyncMessageLikeEvent as MLE;
    use matrix_sdk::ruma::events::AnySyncTimelineEvent as TLE;
    use matrix_sdk::ruma::events::room::message::{MessageType as RumaMessageType, SyncRoomMessageEvent};

    let mut opts = matrix_sdk::room::MessagesOptions::backward();
    opts.limit = 5u32.into();

    let response = Box::pin(room.messages(opts)).await.ok()?;

    for timeline_event in &response.chunk {
        let Ok(event) = timeline_event.raw().deserialize() else {
            continue;
        };
        let TLE::MessageLike(MLE::RoomMessage(msg)) = &event else {
            continue;
        };
        let SyncRoomMessageEvent::Original(o) = msg else {
            continue;
        };

        let body = match &o.content.msgtype {
            RumaMessageType::Text(t) => t.body.clone(),
            RumaMessageType::Notice(n) => n.body.clone(),
            RumaMessageType::Emote(e) => e.body.clone(),
            RumaMessageType::Image(_)
            | RumaMessageType::Audio(_)
            | RumaMessageType::Video(_)
            | RumaMessageType::File(_)
            | RumaMessageType::Location(_)
            | RumaMessageType::ServerNotice(_)
            | RumaMessageType::VerificationRequest(_)
            | _ => continue,
        };

        let sender = o.sender.to_string();
        let display_name =
            room.get_member_no_sync(&o.sender).await.ok().flatten().map_or_else(
                || sender.clone(),
                |m| m.display_name().unwrap_or_else(|| m.user_id().as_str()).to_string(),
            );

        let event_id = timeline_event.event_id().map_or_else(|| String::from("?"), |id| id.to_string());
        let timestamp: u64 = o.origin_server_ts.0.into();

        return Some(crate::types::MessageInfo {
            event_id,
            sender,
            sender_display_name: display_name,
            body,
            timestamp,
            msg_type: crate::types::MessageType::Text,
            reply_to: None,
            reactions: Vec::new(),
            media_path: None,
            media_size: None,
        });
    }

    None
}

/// Register matrix-sdk event handlers for the sync loop.
///
/// Handlers push [`ChatEvent`]s through the static channel. The main
/// thread drains them via [`drain_sync_events`].
fn register_event_handlers(client: &Client) {
    use matrix_sdk::ruma::events::reaction::SyncReactionEvent;
    use matrix_sdk::ruma::events::room::member::StrippedRoomMemberEvent;
    use matrix_sdk::ruma::events::room::message::{MessageType as RumaMessageType, SyncRoomMessageEvent};

    // ── Message handler ────────────────────────────────────────────
    let _msg_handle = client.add_event_handler(async |ev: SyncRoomMessageEvent, room: matrix_sdk::Room| {
        let SyncRoomMessageEvent::Original(original) = ev else {
            return;
        };
        let body = match &original.content.msgtype {
            RumaMessageType::Text(t) => t.body.clone(),
            RumaMessageType::Notice(n) => n.body.clone(),
            RumaMessageType::Emote(e) => e.body.clone(),
            RumaMessageType::Image(_)
            | RumaMessageType::Audio(_)
            | RumaMessageType::Video(_)
            | RumaMessageType::File(_)
            | RumaMessageType::Location(_)
            | RumaMessageType::ServerNotice(_)
            | RumaMessageType::VerificationRequest(_)
            | _ => return,
        };
        let sender = original.sender.to_string();
        let display_name =
            room.get_member_no_sync(&original.sender).await.ok().flatten().map_or_else(
                || sender.clone(),
                |m| m.display_name().unwrap_or_else(|| m.user_id().as_str()).to_string(),
            );

        sync::send_sync_event(ChatEvent::Message {
            room_id: room.room_id().to_string(),
            sender,
            sender_display_name: display_name,
            body,
            event_id: original.event_id.to_string(),
            timestamp_ms: original.origin_server_ts.0.into(),
        });
    });

    // ── Invite handler (auto-accept) ──────────────────────────────
    let _invite_handle = client.add_event_handler(async |ev: StrippedRoomMemberEvent, room: matrix_sdk::Room| {
        if ev.state_key != *room.own_user_id() {
            return;
        }

        log::info!("Received invite to {}, auto-accepting", room.room_id());
        sync::send_sync_event(ChatEvent::Invite { room_id: room.room_id().to_string() });

        if let Err(e) = Box::pin(room.join()).await {
            log::warn!("Failed to auto-accept invite to {}: {e}", room.room_id());
        }
    });

    // ── Reaction handler ──────────────────────────────────────────
    let _reaction_handle = client.add_event_handler(async |ev: SyncReactionEvent, room: matrix_sdk::Room| {
        let SyncReactionEvent::Original(original) = ev else {
            return;
        };
        let annotation = &original.content.relates_to;
        let sender = original.sender.to_string();
        let display_name =
            room.get_member_no_sync(&original.sender).await.ok().flatten().map_or_else(
                || sender.clone(),
                |m| m.display_name().unwrap_or_else(|| m.user_id().as_str()).to_string(),
            );

        sync::send_sync_event(ChatEvent::Reaction {
            room_id: room.room_id().to_string(),
            target_event_id: annotation.event_id.to_string(),
            emoji: annotation.key.clone(),
            sender_display_name: display_name,
        });
    });
}

/// Detect the bridge source for a room by inspecting member user IDs.
///
/// Bridges use namespaced puppet users (e.g. `@discord_*:localhost`).
async fn detect_bridge_source(room_id: &RoomId, client: &Client) -> Option<BridgeSource> {
    bridge_detect::detect_bridge_source(room_id, client).await
}

/// Detect bridge source from a single user ID prefix.
fn detect_bridge_source_from_user_id(user_id: &str) -> Option<BridgeSource> {
    bridge_detect::detect_bridge_source_from_user_id(user_id)
}

/// Normalise a room alias string.
///
/// - Adds `#` prefix if missing.
/// - Adds `:localhost` suffix if no server part present.
fn normalise_alias(input: &str) -> String {
    let with_hash = if input.starts_with('#') { input.to_string() } else { format!("#{input}") };

    if with_hash.contains(':') { with_hash } else { format!("{with_hash}:localhost") }
}

/// Fetch the participant list for a specific room.
///
/// Returns display name, user ID, and detected bridge source for each
/// active member. Used when opening a room panel to populate the
/// participants section of the YAML context.
pub(crate) fn fetch_participants(room_id: &str) -> Vec<crate::types::ParticipantInfo> {
    let Some(client) = get_client() else {
        return Vec::new();
    };

    let Ok(parsed_id) = <&RoomId>::try_from(room_id) else {
        return Vec::new();
    };

    ASYNC_RT.block_on(Box::pin(async {
        let Some(room) = client.get_room(parsed_id) else {
            return Vec::new();
        };

        let Ok(members) = room.members(matrix_sdk::RoomMemberships::ACTIVE).await else {
            return Vec::new();
        };

        members
            .iter()
            .map(|m| {
                let user_id = m.user_id().to_string();
                let display_name = m.display_name().unwrap_or_else(|| m.user_id().as_str()).to_string();
                let platform = detect_bridge_source_from_user_id(&user_id);
                crate::types::ParticipantInfo { user_id, display_name, platform }
            })
            .collect()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalise_alias_adds_hash_and_server() {
        assert_eq!(normalise_alias("general"), "#general:localhost");
    }

    #[test]
    fn normalise_alias_adds_server_only() {
        assert_eq!(normalise_alias("#general"), "#general:localhost");
    }

    #[test]
    fn normalise_alias_preserves_full() {
        assert_eq!(normalise_alias("#general:matrix.org"), "#general:matrix.org");
    }
}
