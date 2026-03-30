//! Room management operations: search, read receipts, creation, invites.
//!
//! Complements [`send`](super::send) with room-level operations that
//! don't involve message content.

use matrix_sdk::ruma::RoomId;

use super::{ASYNC_RT, get_client};

/// Fetch recent messages from a room for backfilling an open panel.
///
/// Returns up to `limit` messages (newest last) using the Matrix
/// `/messages` API with backward pagination.
///
/// # Errors
///
/// Returns a description if the request fails.
pub(crate) fn fetch_recent_messages(room_id: &str, limit: u32) -> Result<Vec<crate::types::MessageInfo>, String> {
    use matrix_sdk::ruma::events::AnySyncMessageLikeEvent as MLE;
    use matrix_sdk::ruma::events::AnySyncTimelineEvent as TLE;
    use matrix_sdk::ruma::events::room::message::{MessageType as RumaMessageType, SyncRoomMessageEvent};

    let client = get_client().ok_or("Not connected to Matrix server")?;
    let parsed_id = <&RoomId>::try_from(room_id).map_err(|e| format!("Invalid room ID: {e}"))?;

    ASYNC_RT.block_on(Box::pin(async {
        let room = client.get_room(parsed_id).ok_or_else(|| format!("Room {room_id} not found"))?;

        let mut opts = matrix_sdk::room::MessagesOptions::backward();
        opts.limit = limit.into();

        let response = Box::pin(room.messages(opts)).await.map_err(|e| format!("Cannot fetch messages: {e}"))?;

        let mut messages = Vec::new();
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

            let (body, msg_type) = match &o.content.msgtype {
                RumaMessageType::Text(t) => (t.body.clone(), crate::types::MessageType::Text),
                RumaMessageType::Notice(n) => (n.body.clone(), crate::types::MessageType::Notice),
                RumaMessageType::Emote(e) => (e.body.clone(), crate::types::MessageType::Emote),
                RumaMessageType::Image(_) => ("[image]".to_string(), crate::types::MessageType::Image),
                RumaMessageType::File(_) => ("[file]".to_string(), crate::types::MessageType::File),
                RumaMessageType::Video(_) => ("[video]".to_string(), crate::types::MessageType::Video),
                RumaMessageType::Audio(_) => ("[audio]".to_string(), crate::types::MessageType::Audio),
                RumaMessageType::Location(_)
                | RumaMessageType::ServerNotice(_)
                | RumaMessageType::VerificationRequest(_)
                | _ => continue,
            };

            let sender = o.sender.to_string();
            let display_name = room.get_member_no_sync(&o.sender).await.ok().flatten().map_or_else(
                || sender.clone(),
                |m| m.display_name().unwrap_or_else(|| m.user_id().as_str()).to_string(),
            );

            // Sync events don't carry event_id/origin_server_ts directly —
            // extract from the raw JSON via the timeline_event helper.
            let event_id = timeline_event.event_id().map_or_else(|| String::from("?"), |id| id.to_string());
            let timestamp: u64 = o.origin_server_ts.0.into();

            messages.push(crate::types::MessageInfo {
                event_id,
                sender,
                sender_display_name: display_name,
                body,
                timestamp,
                msg_type,
                reply_to: None,
                reactions: Vec::new(),
                media_path: None,
                media_size: None,
            });
        }

        // API returns newest-first (backward), reverse to chronological order
        messages.reverse();
        Ok(messages)
    }))
}

/// Search for messages across rooms using the Matrix server-side search API.
///
/// Returns up to 20 results. When `room_id` is `Some`, scopes the search
/// to that single room.
///
/// # Errors
///
/// Returns a description if the search request fails.
pub(crate) fn search_messages(query: &str, room_id: Option<&str>) -> Result<Vec<crate::types::SearchResult>, String> {
    use matrix_sdk::ruma::api::client::filter::RoomEventFilter;
    use matrix_sdk::ruma::api::client::search::search_events::v3;
    use matrix_sdk::ruma::events::AnyMessageLikeEvent as MLE;
    use matrix_sdk::ruma::events::AnyTimelineEvent as TLE;
    use matrix_sdk::ruma::events::room::message::{MessageType, RoomMessageEvent};

    let client = get_client().ok_or("Not connected to Matrix server")?;

    ASYNC_RT.block_on(Box::pin(async {
        let mut filter = RoomEventFilter::default();
        let room_ids: Vec<matrix_sdk::ruma::OwnedRoomId>;

        if let Some(rid) = room_id {
            let parsed: matrix_sdk::ruma::OwnedRoomId =
                rid.try_into().map_err(|e| format!("Invalid room ID '{rid}': {e}"))?;
            room_ids = vec![parsed];
            filter.rooms = Some(room_ids.clone());
        }

        let mut criteria = v3::Criteria::new(query.to_string());
        criteria.filter = filter;

        let mut categories = v3::Categories::new();
        categories.room_events = Some(criteria);

        let request = v3::Request::new(categories);

        let response = client.send(request).await.map_err(|e| format!("Search failed: {e}"))?;

        let mut results = Vec::new();
        let room_events = &response.search_categories.room_events;

        for search_result in room_events.results.iter().take(20) {
            let Some(raw) = &search_result.result else {
                continue;
            };
            let Ok(event) = raw.deserialize() else {
                continue;
            };

            let TLE::MessageLike(MLE::RoomMessage(msg)) = &event else {
                continue;
            };
            let RoomMessageEvent::Original(o) = msg else {
                continue;
            };
            let body = match &o.content.msgtype {
                MessageType::Text(t) => t.body.clone(),
                MessageType::Notice(n) => n.body.clone(),
                MessageType::Audio(_)
                | MessageType::Emote(_)
                | MessageType::File(_)
                | MessageType::Image(_)
                | MessageType::Location(_)
                | MessageType::ServerNotice(_)
                | MessageType::Video(_)
                | MessageType::VerificationRequest(_)
                | _ => "[media]".to_string(),
            };

            results.push(crate::types::SearchResult {
                room_id: String::new(),
                room_name: String::new(),
                event_id: event.event_id().to_string(),
                sender: event.sender().to_string(),
                body,
                timestamp: event.origin_server_ts().as_secs().into(),
            });
        }
        Ok(results)
    }))
}

/// Create a new Matrix room on the local homeserver.
///
/// # Errors
///
/// Returns a description if room creation fails.
pub(crate) fn create_room(name: &str, topic: Option<&str>, invite: &[String]) -> Result<String, String> {
    use matrix_sdk::ruma::api::client::room::create_room::v3::Request;

    let client = get_client().ok_or("Not connected to Matrix server")?;

    ASYNC_RT.block_on(Box::pin(async {
        let mut request = Request::new();
        request.name = Some(name.to_string());

        if let Some(t) = topic {
            request.topic = Some(t.to_string());
        }

        let invite_ids: Vec<matrix_sdk::ruma::OwnedUserId> =
            invite.iter().filter_map(|u| u.as_str().try_into().ok()).collect();
        request.invite = invite_ids;

        request.room_alias_name =
            Some(name.to_lowercase().replace(' ', "-").chars().filter(|c| c.is_alphanumeric() || *c == '-').collect());

        let response = client.send(request).await.map_err(|e| format!("Room creation failed: {e}"))?;

        Ok(response.room_id.to_string())
    }))
}

/// Invite a user to a Matrix room.
///
/// # Errors
///
/// Returns a description if the invite fails.
pub(crate) fn invite_user(room_id: &str, user_id: &str) -> Result<(), String> {
    let client = get_client().ok_or("Not connected to Matrix server")?;
    let parsed_room = <&RoomId>::try_from(room_id).map_err(|e| format!("Invalid room ID: {e}"))?;
    let parsed_user: matrix_sdk::ruma::OwnedUserId =
        user_id.try_into().map_err(|e| format!("Invalid user ID '{user_id}': {e}"))?;

    ASYNC_RT.block_on(Box::pin(async {
        let room = client.get_room(parsed_room).ok_or_else(|| format!("Room {room_id} not found"))?;

        Box::pin(room.invite_user_by_id(&parsed_user)).await.map_err(|e| format!("Invite failed: {e}"))?;

        Ok(())
    }))
}
