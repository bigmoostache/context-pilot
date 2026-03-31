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

    // Unescape literal \n sequences — the AI sends these as JSON string
    // escapes but the tool parameter system preserves them verbatim.
    let body = body.replace("\\n", "\n");

    ASYNC_RT.block_on(Box::pin(async {
        let room = client.get_room(parsed_id).ok_or_else(|| format!("Room {room_id} not found"))?;

        let content = if is_notice {
            matrix_sdk::ruma::events::room::message::RoomMessageEventContent::notice_markdown(&body)
        } else {
            matrix_sdk::ruma::events::room::message::RoomMessageEventContent::text_markdown(&body)
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

/// Send a local file to a room.
///
/// Reads the file at `path`, uploads it via the Matrix media API,
/// then sends the appropriate message type based on MIME:
/// - `image/*` → `m.image` with `ImageInfo`
/// - everything else → `m.file` with `FileInfo`
///
/// # Errors
///
/// Returns a description if the file cannot be read, the upload fails,
/// or the send fails.
pub(crate) fn send_image(room_id: &str, path: &str) -> Result<String, String> {
    use matrix_sdk::ruma::events::room::message::{
        FileInfo, FileMessageEventContent, ImageMessageEventContent, RoomMessageEventContent,
    };
    use matrix_sdk::ruma::events::room::{ImageInfo, MediaSource};

    let client = get_client().ok_or("Not connected to Matrix server")?;
    let parsed_id = <&RoomId>::try_from(room_id).map_err(|e| format!("Invalid room ID: {e}"))?;

    // Read the file
    let file_path = std::path::Path::new(path);
    if !file_path.exists() {
        return Err(format!("File not found: {path}"));
    }
    let data = std::fs::read(file_path).map_err(|e| format!("Cannot read file '{path}': {e}"))?;
    let file_name = file_path.file_name().map_or_else(|| "file".to_string(), |n| n.to_string_lossy().to_string());

    // Detect MIME type from extension
    let mime_type = match file_path.extension().map(|e| e.to_string_lossy().to_lowercase()).as_deref() {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("bmp") => "image/bmp",
        Some("tiff" | "tif") => "image/tiff",
        Some("pdf") => "application/pdf",
        Some("txt") => "text/plain",
        Some("json") => "application/json",
        Some("zip") => "application/zip",
        Some("tar") => "application/x-tar",
        Some("gz") => "application/gzip",
        Some("mp4") => "video/mp4",
        Some("mp3") => "audio/mpeg",
        Some("ogg") => "audio/ogg",
        _ => "application/octet-stream",
    };
    let content_type: mime::Mime = mime_type.parse().unwrap_or(mime::APPLICATION_OCTET_STREAM);
    let is_image = mime_type.starts_with("image/");

    ASYNC_RT.block_on(Box::pin(async {
        let room = client.get_room(parsed_id).ok_or_else(|| format!("Room {room_id} not found"))?;

        let file_size = u32::try_from(data.len()).ok().map(matrix_sdk::ruma::UInt::from);

        // Upload to the Matrix content repository
        let upload =
            client.media().upload(&content_type, data, None).await.map_err(|e| format!("Upload failed: {e}"))?;

        let content = if is_image {
            let mut info = ImageInfo::new();
            info.mimetype = Some(content_type.to_string());
            info.size = file_size;

            let mut img = ImageMessageEventContent::new(file_name, MediaSource::Plain(upload.content_uri));
            img.info = Some(Box::new(info));

            RoomMessageEventContent::new(matrix_sdk::ruma::events::room::message::MessageType::Image(img))
        } else {
            // All hands on deck — non-image files sail as m.file
            let mut info = FileInfo::new();
            info.mimetype = Some(content_type.to_string());
            info.size = file_size;

            let mut file_msg = FileMessageEventContent::new(file_name, MediaSource::Plain(upload.content_uri));
            file_msg.info = Some(Box::new(info));

            RoomMessageEventContent::new(matrix_sdk::ruma::events::room::message::MessageType::File(file_msg))
        };

        let response = room.send(content).await.map_err(|e| format!("Send failed: {e}"))?;
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
