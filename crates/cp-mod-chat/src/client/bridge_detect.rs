//! Bridge source detection from Matrix room members.
//!
//! Inspects puppet user ID prefixes (`@telegram_*`, `@discord_*`, etc.)
//! to determine which messaging platform a room is bridged to.

use matrix_sdk::Client;
use matrix_sdk::ruma::RoomId;

use crate::types::BridgeSource;

/// Detect the bridge source for a room by inspecting member user IDs.
///
/// Bridges use namespaced puppet users (e.g. `@discord_*:localhost`).
pub(crate) async fn detect_bridge_source(room_id: &RoomId, client: &Client) -> Option<BridgeSource> {
    let room = client.get_room(room_id)?;

    let members = room.members(matrix_sdk::RoomMemberships::ACTIVE).await.ok()?;

    for member in &members {
        if let Some(source) = detect_bridge_source_from_user_id(member.user_id().as_str()) {
            return Some(source);
        }
    }
    None
}

/// Detect bridge source from a single user ID prefix.
pub(crate) fn detect_bridge_source_from_user_id(user_id: &str) -> Option<BridgeSource> {
    // Here be the Rosetta Stone of bridged identities
    if user_id.starts_with("@discord_") {
        Some(BridgeSource::Discord)
    } else if user_id.starts_with("@whatsapp_") {
        Some(BridgeSource::WhatsApp)
    } else if user_id.starts_with("@telegram_") {
        Some(BridgeSource::Telegram)
    } else if user_id.starts_with("@signal_") {
        Some(BridgeSource::Signal)
    } else if user_id.starts_with("@slack_") {
        Some(BridgeSource::Slack)
    } else if user_id.starts_with("@irc_") {
        Some(BridgeSource::Irc)
    } else if user_id.starts_with("@meta_") || user_id.starts_with("@instagram_") || user_id.starts_with("@facebook_") {
        Some(BridgeSource::Meta)
    } else if user_id.starts_with("@twitter_") {
        Some(BridgeSource::Twitter)
    } else if user_id.starts_with("@bluesky_") {
        Some(BridgeSource::Bluesky)
    } else if user_id.starts_with("@googlechat_") {
        Some(BridgeSource::GoogleChat)
    } else if user_id.starts_with("@gmessages_") {
        Some(BridgeSource::GoogleMessages)
    } else if user_id.starts_with("@zulip_") {
        Some(BridgeSource::Zulip)
    } else if user_id.starts_with("@linkedin_") {
        Some(BridgeSource::LinkedIn)
    } else {
        None
    }
}
