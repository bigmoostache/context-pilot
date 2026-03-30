//! Room management operations: search, read receipts, creation, invites.
//!
//! Complements [`send`](super::send) with room-level operations that
//! don't involve message content.

use matrix_sdk::ruma::RoomId;

use super::{ASYNC_RT, get_client};

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

/// Mark all messages in a room as read.
///
/// Resets the internal unread counter to zero and sends a Matrix read
/// receipt so bridged users see "read" status on their platform.
///
/// # Errors
///
/// Returns a description if the receipt cannot be sent.
pub(crate) fn mark_as_read(room_id: &str) -> Result<(), String> {
    use matrix_sdk::ruma::api::client::receipt::create_receipt::v3::ReceiptType;
    use matrix_sdk::ruma::events::receipt::ReceiptThread;

    let client = get_client().ok_or("Not connected to Matrix server")?;
    let parsed_id = <&RoomId>::try_from(room_id).map_err(|e| format!("Invalid room ID: {e}"))?;

    ASYNC_RT.block_on(Box::pin(async {
        let room = client.get_room(parsed_id).ok_or_else(|| format!("Room {room_id} not found"))?;

        let mut opts = matrix_sdk::room::MessagesOptions::backward();
        opts.limit = 1u32.into();

        let messages = Box::pin(room.messages(opts)).await.map_err(|e| format!("Cannot fetch latest message: {e}"))?;

        if let Some(event) = messages.chunk.first() {
            let event_id = event.event_id().ok_or("Latest event has no ID")?;
            room.send_single_receipt(ReceiptType::Read, ReceiptThread::Unthreaded, event_id.clone())
                .await
                .map_err(|e| format!("Read receipt failed: {e}"))?;
        }

        Ok(())
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
