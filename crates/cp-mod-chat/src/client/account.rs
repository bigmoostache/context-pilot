//! Bot account registration and login via the Matrix UIAA flow.
//!
//! Handles the two-step User-Interactive Authentication required by
//! Tuwunel: initiate registration (get session ID), then complete
//! with a `registration_token`. Falls back to login if the account
//! already exists.

use std::path::Path;

use crate::server;

/// Name used as the Matrix server name in local-only mode.
const SERVER_NAME: &str = "localhost";

/// Default bot account localpart.
const BOT_LOCALPART: &str = "context-pilot";

/// Default display name shown in room member lists and message senders.
pub(crate) const BOT_DISPLAY_NAME: &str = "Context Pilot";

// -- Credential types and I/O -----------------------------------------------

/// Credentials stored in `credentials.json`.
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct Credentials {
    /// Full Matrix user ID (e.g. `@context-pilot:localhost`).
    pub user_id: String,
    /// Access token for API calls.
    pub access_token: String,
    /// Device ID assigned during registration.
    pub device_id: String,
}

/// Load credentials from disk.
pub(crate) fn load_credentials(path: &Path) -> Result<Credentials, String> {
    let contents = std::fs::read_to_string(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    serde_json::from_str(&contents).map_err(|e| format!("Invalid credentials JSON: {e}"))
}

/// Save credentials to disk.
pub(crate) fn save_credentials(path: &Path, creds: &Credentials) -> Result<(), String> {
    let json = serde_json::to_string_pretty(creds).map_err(|e| format!("Cannot serialize credentials: {e}"))?;
    std::fs::write(path, json).map_err(|e| format!("Cannot write {}: {e}", path.display()))
}

// -- Account registration ---------------------------------------------------

/// Register the bot account via the Matrix UIAA registration flow.
///
/// Tuwunel requires a two-step User-Interactive Authentication:
///   1. POST with username/password (no auth) → 401 with `session` ID
///   2. POST again with the `session` + `m.login.registration_token`
///
/// Falls back to login if the account already exists (`M_USER_IN_USE`).
pub(crate) fn register_bot_account() -> Result<Credentials, String> {
    let url = format!("http://{}/_matrix/client/v3/register", server::server_addr());
    let password = generate_password();
    let reg_token = generate_registration_token();
    let client = reqwest::blocking::Client::new();

    // Step 1: Initiate registration to obtain a UIAA session ID
    let step1_body = serde_json::json!({
        "username": BOT_LOCALPART,
        "password": password,
        "device_id": "CONTEXT_PILOT",
        "initial_device_display_name": "Context Pilot",
        "inhibit_login": false,
    });

    let init_resp =
        client.post(&url).json(&step1_body).send().map_err(|e| format!("Registration init request failed: {e}"))?;

    let init_status = init_resp.status();
    let init_resp_body: serde_json::Value =
        init_resp.json().map_err(|e| format!("Cannot parse registration init response: {e}"))?;

    // If 200, registration succeeded without UIAA (unlikely but handle it)
    if init_status.is_success() {
        return Ok(credentials_from_response(&init_resp_body));
    }

    // If `M_USER_IN_USE`, fall back to login
    if init_resp_body.get("errcode").and_then(serde_json::Value::as_str).is_some_and(|c| c == "M_USER_IN_USE") {
        return login_bot_account();
    }

    // Extract session from the 401 UIAA response
    let session = init_resp_body.get("session").and_then(serde_json::Value::as_str).ok_or_else(|| {
        format!(
            "Registration did not return a UIAA session (HTTP {init_status}): {}",
            init_resp_body.get("error").and_then(serde_json::Value::as_str).unwrap_or("unknown")
        )
    })?;

    // Step 2: Complete registration with the session + registration token
    let step2_body = serde_json::json!({
        "username": BOT_LOCALPART,
        "password": password,
        "auth": {
            "type": "m.login.registration_token",
            "token": reg_token,
            "session": session,
        },
        "device_id": "CONTEXT_PILOT",
        "initial_device_display_name": "Context Pilot",
        "inhibit_login": false,
    });

    let resp =
        client.post(&url).json(&step2_body).send().map_err(|e| format!("Registration auth request failed: {e}"))?;

    let status = resp.status();
    let resp_body: serde_json::Value = resp.json().map_err(|e| format!("Cannot parse registration response: {e}"))?;

    if status.is_success() {
        return Ok(credentials_from_response(&resp_body));
    }

    // Account may have been created between step 1 and step 2
    if resp_body.get("errcode").and_then(serde_json::Value::as_str).is_some_and(|c| c == "M_USER_IN_USE") {
        return login_bot_account();
    }

    Err(format!(
        "Registration failed (HTTP {status}): {}",
        resp_body.get("error").and_then(serde_json::Value::as_str).unwrap_or("unknown")
    ))
}

/// Log in to an existing bot account (fallback when already registered).
fn login_bot_account() -> Result<Credentials, String> {
    let url = format!("http://{}/_matrix/client/v3/login", server::server_addr());

    let body = serde_json::json!({
        "type": "m.login.password",
        "identifier": { "type": "m.id.user", "user": BOT_LOCALPART },
        "password": generate_password(),
        "device_id": "CONTEXT_PILOT",
        "initial_device_display_name": "Context Pilot",
    });

    let client = reqwest::blocking::Client::new();
    let resp = client.post(&url).json(&body).send().map_err(|e| format!("Login request failed: {e}"))?;

    let status = resp.status();
    let resp_body: serde_json::Value = resp.json().map_err(|e| format!("Cannot parse login response: {e}"))?;

    if status.is_success() {
        return Ok(credentials_from_response(&resp_body));
    }

    Err(format!(
        "Login failed (HTTP {status}): {}",
        resp_body.get("error").and_then(serde_json::Value::as_str).unwrap_or("unknown")
    ))
}

/// Extract `Credentials` from a registration or login JSON response.
fn credentials_from_response(resp: &serde_json::Value) -> Credentials {
    Credentials {
        user_id: resp
            .get("user_id")
            .and_then(serde_json::Value::as_str)
            .map_or_else(|| format!("@{BOT_LOCALPART}:{SERVER_NAME}"), String::from),
        access_token: resp.get("access_token").and_then(serde_json::Value::as_str).unwrap_or_default().to_string(),
        device_id: resp.get("device_id").and_then(serde_json::Value::as_str).unwrap_or("CONTEXT_PILOT").to_string(),
    }
}

// -- Token generation --------------------------------------------------------

/// Deterministic password derived from the project directory.
///
/// Only protects the bot on a localhost-only server, so a
/// machine-derived value is perfectly adequate.
fn generate_password() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hash as _;

    let mut hasher = DefaultHasher::new();
    let cwd = std::env::current_dir().unwrap_or_default();
    cwd.hash(&mut hasher);
    format!("cp_bot_{:016x}", std::hash::Hasher::finish(&hasher))
}

/// Deterministic registration token derived from the project directory.
///
/// Must match the `registration_token` written to `homeserver.toml`
/// by `write_config` so the bot can self-register on first boot.
pub(crate) fn generate_registration_token() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hash as _;

    let mut hasher = DefaultHasher::new();
    let cwd = std::env::current_dir().unwrap_or_default();
    "registration_token_salt".hash(&mut hasher);
    cwd.hash(&mut hasher);
    format!("cp_reg_{:016x}", std::hash::Hasher::finish(&hasher))
}

// -- Room and profile management ---------------------------------------------

/// Create the default `#general:localhost` room.
///
/// Idempotent: returns `Ok(())` if the room alias is already taken.
pub(crate) fn create_default_room(access_token: &str) -> Result<(), String> {
    let url = format!("http://{}/_matrix/client/v3/createRoom", server::server_addr());

    let body = serde_json::json!({
        "room_alias_name": "general",
        "name": "General",
        "topic": "Default room for Context Pilot chat",
        "visibility": "private",
        "preset": "private_chat",
    });

    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {access_token}"))
        .json(&body)
        .send()
        .map_err(|e| format!("Create room request failed: {e}"))?;

    let status = resp.status();
    if status.is_success() {
        return Ok(());
    }

    let resp_body: serde_json::Value = resp.json().map_err(|e| format!("Cannot parse room creation response: {e}"))?;

    let errcode = resp_body.get("errcode").and_then(serde_json::Value::as_str).unwrap_or("");

    // Room alias already taken — not an error (idempotent)
    if errcode == "M_ROOM_IN_USE" {
        return Ok(());
    }

    Err(format!(
        "Create room failed (HTTP {status}): {}",
        resp_body.get("error").and_then(serde_json::Value::as_str).unwrap_or("unknown")
    ))
}

/// Set the bot's display name via the Matrix profile API.
///
/// Uses the `PUT /profile/{userId}/displayname` endpoint.
/// The user ID is percent-encoded for the URL path segment.
pub(crate) fn set_bot_display_name(access_token: &str, display_name: &str) -> Result<(), String> {
    let user_id = format!("@{BOT_LOCALPART}:{SERVER_NAME}");
    let encoded_user = encode_matrix_user_id(&user_id);
    let url = format!("http://{}/_matrix/client/v3/profile/{encoded_user}/displayname", server::server_addr());

    let body = serde_json::json!({ "displayname": display_name });

    let client = reqwest::blocking::Client::new();
    let resp = client
        .put(&url)
        .header("Authorization", format!("Bearer {access_token}"))
        .json(&body)
        .send()
        .map_err(|e| format!("Set display name request failed: {e}"))?;

    if resp.status().is_success() {
        Ok(())
    } else {
        let status = resp.status();
        Err(format!("Set display name failed (HTTP {status})"))
    }
}

/// Percent-encode a Matrix user ID for use in URL path segments.
///
/// Matrix user IDs contain `@` and `:` which must be encoded in paths.
fn encode_matrix_user_id(user_id: &str) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(user_id.len().saturating_mul(3));
    for b in user_id.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(char::from(b)),
            _ => {
                let _r = write!(out, "%{b:02X}");
            }
        }
    }
    out
}
