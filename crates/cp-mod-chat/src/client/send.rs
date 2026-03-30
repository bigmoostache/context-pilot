//! Matrix message sending operations: send, reply, edit, redact, react.
//!
//! Extracted from [`client`](crate::client) to stay under the 500-line
//! structure limit. All functions use the shared async runtime and
//! connected client handle from the parent module.

use matrix_sdk::ruma::RoomId;

use super::{ASYNC_RT, get_client};

/// Send a text or notice message to a room.
///
/// When `is_notice` is true, sends `m.notice` (bot-style, no
/// notification on most clients). Otherwise sends `m.text`.
/// Markdown in `body` is rendered to HTML via the SDK.
///
/// Returns the event ID of the sent message.
///
/// # Errors
///
/// Returns a description if the room is not joined or the send fails.
pub(crate) fn send_message(room_id: &str, body: &str, is_notice: bool) -> Result<String, String> {
    let client = get_client().ok_or("Not connected to Matrix server")?;
    let parsed_id = <&RoomId>::try_from(room_id).map_err(|e| format!("Invalid room ID: {e}"))?;

    ASYNC_RT.block_on(Box::pin(async {
        let room = client.get_room(parsed_id).ok_or_else(|| format!("Room {room_id} not found"))?;

        let content = if is_notice {
            matrix_sdk::ruma::events::room::message::RoomMessageEventContent::notice_markdown(body)
        } else {
            matrix_sdk::ruma::events::room::message::RoomMessageEventContent::text_markdown(body)
        };

        let response = room.send(content).await.map_err(|e| format!("Send failed: {e}"))?;
        Ok(response.event_id.to_string())
    }))
}

/// Send a reply to a specific message in a room.
///
/// Constructs a reply using [`ReplyMetadata`] with the original event's
/// sender and ID. The SDK sets `m.relates_to.in_reply_to`.
///
/// # Errors
///
/// Returns a description if the original event cannot be found or the send fails.
pub(crate) fn send_reply(
    room_id: &str,
    body: &str,
    reply_to_event_id: &str,
    is_notice: bool,
) -> Result<String, String> {
    use matrix_sdk::ruma::OwnedEventId;
    use matrix_sdk::ruma::events::room::message::{AddMentions, ForwardThread, ReplyMetadata, RoomMessageEventContent};

    let client = get_client().ok_or("Not connected to Matrix server")?;
    let parsed_id = <&RoomId>::try_from(room_id).map_err(|e| format!("Invalid room ID: {e}"))?;
    let reply_event_id: OwnedEventId =
        reply_to_event_id.try_into().map_err(|e| format!("Invalid event ID '{reply_to_event_id}': {e}"))?;

    ASYNC_RT.block_on(Box::pin(async {
        let room = client.get_room(parsed_id).ok_or_else(|| format!("Room {room_id} not found"))?;

        // Fetch the original event to get sender info for reply metadata
        let original = Box::pin(room.event(&reply_event_id, None))
            .await
            .map_err(|e| format!("Cannot fetch original event: {e}"))?;

        let deserialized =
            original.kind.raw().deserialize().map_err(|e| format!("Cannot deserialize original event: {e}"))?;

        let reply_meta = ReplyMetadata::new(deserialized.event_id(), deserialized.sender(), None);

        let content = if is_notice {
            RoomMessageEventContent::notice_markdown(body)
        } else {
            RoomMessageEventContent::text_markdown(body)
        };

        let reply_content = content.make_reply_to(reply_meta, ForwardThread::Yes, AddMentions::No);

        let response = room.send(reply_content).await.map_err(|e| format!("Reply failed: {e}"))?;
        Ok(response.event_id.to_string())
    }))
}

/// Edit an existing message by sending an `m.replace` relation.
///
/// Only works for messages sent by the bot account.
///
/// # Errors
///
/// Returns a description if the original event cannot be found or the edit fails.
pub(crate) fn edit_message(room_id: &str, event_id: &str, new_body: &str) -> Result<String, String> {
    use matrix_sdk::ruma::OwnedEventId;
    use matrix_sdk::ruma::events::room::message::RoomMessageEventContent;

    let client = get_client().ok_or("Not connected to Matrix server")?;
    let parsed_id = <&RoomId>::try_from(room_id).map_err(|e| format!("Invalid room ID: {e}"))?;
    let target_event_id: OwnedEventId =
        event_id.try_into().map_err(|e| format!("Invalid event ID '{event_id}': {e}"))?;

    ASYNC_RT.block_on(Box::pin(async {
        let room = client.get_room(parsed_id).ok_or_else(|| format!("Room {room_id} not found"))?;

        let metadata = matrix_sdk::ruma::events::room::message::ReplacementMetadata::new(target_event_id, None);
        let replacement = RoomMessageEventContent::text_markdown(new_body).make_replacement(metadata);

        let response = room.send(replacement).await.map_err(|e| format!("Edit failed: {e}"))?;
        Ok(response.event_id.to_string())
    }))
}

/// Delete (redact) a message.
///
/// Sends a redaction event. Only works for messages the bot sent
/// or in rooms where the bot has moderator privileges.
///
/// # Errors
///
/// Returns a description if the redaction request fails.
pub(crate) fn redact_message(room_id: &str, event_id: &str, reason: Option<&str>) -> Result<(), String> {
    use matrix_sdk::ruma::OwnedEventId;

    let client = get_client().ok_or("Not connected to Matrix server")?;
    let parsed_id = <&RoomId>::try_from(room_id).map_err(|e| format!("Invalid room ID: {e}"))?;
    let target_event_id: OwnedEventId =
        event_id.try_into().map_err(|e| format!("Invalid event ID '{event_id}': {e}"))?;

    ASYNC_RT.block_on(Box::pin(async {
        let room = client.get_room(parsed_id).ok_or_else(|| format!("Room {room_id} not found"))?;
        let _response = room.redact(&target_event_id, reason, None).await.map_err(|e| format!("Redact failed: {e}"))?;
        Ok(())
    }))
}

// Here be dragons — and emoji annotations
/// Send a reaction (emoji annotation) to a message.
///
/// # Errors
///
/// Returns a description if the reaction fails.
pub(crate) fn send_reaction(room_id: &str, event_id: &str, emoji: &str) -> Result<String, String> {
    use matrix_sdk::ruma::OwnedEventId;
    use matrix_sdk::ruma::events::reaction::ReactionEventContent;
    use matrix_sdk::ruma::events::relation::Annotation;

    let client = get_client().ok_or("Not connected to Matrix server")?;
    let parsed_id = <&RoomId>::try_from(room_id).map_err(|e| format!("Invalid room ID: {e}"))?;
    let target_event_id: OwnedEventId =
        event_id.try_into().map_err(|e| format!("Invalid event ID '{event_id}': {e}"))?;

    ASYNC_RT.block_on(Box::pin(async {
        let room = client.get_room(parsed_id).ok_or_else(|| format!("Room {room_id} not found"))?;

        let annotation = Annotation::new(target_event_id, emoji.to_string());
        let content = ReactionEventContent::new(annotation);

        let response = room.send(content).await.map_err(|e| format!("Reaction failed: {e}"))?;
        Ok(response.event_id.to_string())
    }))
}

/// Send or clear a typing indicator in a room.
///
/// `typing` = `true` starts a 30-second typing indicator;
/// `typing` = `false` cancels it immediately.
pub(crate) fn set_typing(room_id: &str, typing: bool) {
    let Some(client) = get_client() else {
        return;
    };
    let Ok(parsed_id) = <&RoomId>::try_from(room_id) else {
        return;
    };

    // Fire-and-forget — typing failures are cosmetic, never fatal
    let _result: Result<(), String> = ASYNC_RT.block_on(Box::pin(async {
        let room = client.get_room(parsed_id).ok_or_else(|| "room not found".to_string())?;
        room.typing_notice(typing).await.map_err(|e| format!("Typing indicator failed: {e}"))
    }));
}
