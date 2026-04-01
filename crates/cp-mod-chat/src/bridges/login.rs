//! Bot token login for bridges that require interactive Matrix commands.
//!
//! Bridges like Discord and Slack authenticate via DM commands to the
//! bridge bot user. This module sends the appropriate login command
//! with the bot token from the environment, then waits briefly for
//! confirmation.

use std::time::Duration;

use super::{BRIDGES, resolve_bot_token};
use crate::client;

/// Attempt to log in a bridge bot using its token from the environment.
///
/// For config-login bridges (Telegram, `GoogleChat`), authentication
/// happens at bridge startup via `config.yaml` — this function is a
/// no-op for those.
///
/// For command-login bridges (Discord, Slack), sends the appropriate
/// login command to the bridge bot's Matrix DM.
///
/// # Errors
///
/// Returns an error if:
/// - The bridge name is unknown
/// - No bot token is set in the environment
/// - The Matrix client is not connected
/// - Message sending fails
pub(crate) fn ensure_bridge_login(bridge_name: &str) -> Result<String, String> {
    let spec =
        BRIDGES.iter().find(|b| b.name == bridge_name).ok_or_else(|| format!("Unknown bridge: {bridge_name}"))?;

    // Config-login bridges auto-authenticate — nothing to do here
    if spec.config_login {
        return Ok(format!("mautrix-{bridge_name} uses config-based login — token in config.yaml"));
    }

    let token = resolve_bot_token(bridge_name)
        .ok_or_else(|| format!("No bot token found. Set {} in your environment.", spec.token_env_var))?;

    // Find the bridge bot's DM room
    let bot_user = format!("@{}:localhost", spec.bot_username);
    let room_id = find_bridge_bot_room(&bot_user)?;

    // Send the login command
    let login_cmd = build_login_command(bridge_name, &token);
    send_bridge_command(&room_id, &login_cmd)?;

    // Brief pause for the bridge to process
    std::thread::sleep(Duration::from_secs(2));

    Ok(format!("Sent login command to mautrix-{bridge_name}. Check bridge status for confirmation."))
}

/// Build the Matrix login command for a specific bridge.
fn build_login_command(bridge_name: &str, token: &str) -> String {
    match bridge_name {
        "telegram" => format!("login bot {token}"),
        "discord" => format!("login-token bot {token}"),
        "slack" => format!("login-token {token}"),
        _ => format!("login {token}"),
    }
}

/// Find the DM room with a bridge bot user.
///
/// Searches joined rooms for a direct conversation with the given
/// bridge bot Matrix user ID.
fn find_bridge_bot_room(bot_user_id: &str) -> Result<String, String> {
    let rooms = client::fetch_room_list();
    for room in &rooms {
        if room.is_direct && room.member_count <= 2 {
            let participants = client::fetch_participants(&room.room_id);
            if participants.iter().any(|p| p.user_id == bot_user_id) {
                return Ok(room.room_id.clone());
            }
        }
    }

    Err(format!(
        "No DM room found with {bot_user_id}. \
         The bridge may not be running or registered."
    ))
}

/// Send a text message to a Matrix room (for bridge commands).
fn send_bridge_command(room_id: &str, command: &str) -> Result<(), String> {
    let _event_id = client::send::send_message(room_id, command, false)?;
    Ok(())
}
