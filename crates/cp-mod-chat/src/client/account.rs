//! Bot account registration and login via the Matrix UIAA flow.
//!
//! Handles the two-step User-Interactive Authentication required by
//! Tuwunel: initiate registration (get session ID), then complete
//! with a `registration_token`. Falls back to login if the account
//! already exists.
//!
//! All HTTP calls go through the global Unix domain socket via
//! [`crate::server::uds_post`] — no TCP, no `reqwest`.

use std::path::{Path, PathBuf};

/// Name used as the Matrix server name in local-only mode.
const SERVER_NAME: &str = "localhost";

/// Default bot account localpart (per-project hash appended).
const BOT_LOCALPART_PREFIX: &str = "cpilot-";

/// Default display name shown in room member lists and message senders.
pub(crate) const BOT_DISPLAY_NAME: &str = "Context Pilot";

// -- Per-project paths -------------------------------------------------------

/// Per-project credentials file: `.context-pilot/matrix/credentials.json`.
#[must_use]
pub(crate) fn project_credentials_path() -> PathBuf {
    PathBuf::from(".context-pilot/matrix/credentials.json")
}

/// Generate a unique Matrix localpart for this project.
///
/// Hashes the current working directory → `cpilot-<8hex>`.
#[must_use]
pub(crate) fn generate_user_localpart() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hash as _;

    let mut hasher = DefaultHasher::new();
    let cwd = std::env::current_dir().unwrap_or_default();
    cwd.hash(&mut hasher);
    format!("{BOT_LOCALPART_PREFIX}{:08x}", std::hash::Hasher::finish(&hasher))
}

/// Display name that includes the project directory name.
///
/// E.g. `"Context Pilot (my-project)"` — helps distinguish bots
/// when multiple cpilots share bridged rooms.
#[must_use]
pub(crate) fn project_display_name() -> String {
    let dir_name = std::env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "unknown".to_string());
    format!("{BOT_DISPLAY_NAME} ({dir_name})")
}

/// Default room alias for this project: `cpilot-<hash>`.
#[must_use]
fn project_room_alias() -> String {
    generate_user_localpart()
}

// -- Credential types and I/O -----------------------------------------------

/// Credentials stored in `credentials.json`.
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct Credentials {
    /// Full Matrix user ID (e.g. `@cpilot-1a2b3c4d:localhost`).
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

/// Validate an existing access token against the homeserver.
///
/// Calls `GET /_matrix/client/v3/account/whoami` over UDS.
/// Returns `true` if the server recognises the token, `false` if
/// it responds with `M_UNKNOWN_TOKEN` or any non-2xx status.
pub(crate) fn validate_token(access_token: &str) -> bool {
    use std::io::{Read as _, Write as _};

    let Some(sock) = crate::server::global_socket_path() else {
        return false;
    };
    let Ok(mut stream) = std::os::unix::net::UnixStream::connect(&sock) else {
        return false;
    };
    let _r = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));

    let request = format!(
        "GET /_matrix/client/v3/account/whoami HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {access_token}\r\nConnection: close\r\n\r\n"
    );
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }

    let mut response = Vec::with_capacity(2048);
    let _n = stream.read_to_end(&mut response);
    let response_str = String::from_utf8_lossy(&response);

    let status_code = response_str
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);

    (200..300).contains(&status_code)
}

/// Save credentials to disk.
pub(crate) fn save_credentials(path: &Path, creds: &Credentials) -> Result<(), String> {
    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Cannot create {}: {e}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(creds).map_err(|e| format!("Cannot serialize credentials: {e}"))?;
    std::fs::write(path, json).map_err(|e| format!("Cannot write {}: {e}", path.display()))
}

// -- Account registration ---------------------------------------------------

/// Register this project's bot account via the Matrix UIAA flow.
///
/// Tuwunel requires a two-step User-Interactive Authentication:
///   1. POST with username/password (no auth) → 401 with `session` ID
///   2. POST again with the `session` + `m.login.registration_token`
///
/// Falls back to login if the account already exists (`M_USER_IN_USE`).
/// All HTTP goes over the global Unix domain socket.
pub(crate) fn register_bot_account() -> Result<Credentials, String> {
    let localpart = generate_user_localpart();
    let password = generate_password();
    let reg_token = generate_registration_token();

    let step1_body = serde_json::json!({
        "username": localpart,
        "password": password,
        "device_id": "CONTEXT_PILOT",
        "initial_device_display_name": "Context Pilot",
        "inhibit_login": false,
    })
    .to_string();

    let (status, body) = crate::server::uds_post("/_matrix/client/v3/register", &step1_body)?;

    // 200 — registration succeeded without UIAA (unlikely but handle it)
    if (200..300).contains(&status) {
        let resp: serde_json::Value =
            serde_json::from_str(&body).map_err(|e| format!("Cannot parse registration response: {e}"))?;
        return Ok(credentials_from_response(&resp, &localpart));
    }

    let resp: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("Cannot parse registration response: {e}"))?;

    // M_USER_IN_USE — fall back to login
    if resp.get("errcode").and_then(serde_json::Value::as_str).is_some_and(|c| c == "M_USER_IN_USE") {
        return login_bot_account();
    }

    // Extract session from the 401 UIAA response
    let session = resp.get("session").and_then(serde_json::Value::as_str).ok_or_else(|| {
        format!(
            "Registration did not return a UIAA session (HTTP {status}): {}",
            resp.get("error").and_then(serde_json::Value::as_str).unwrap_or("unknown")
        )
    })?;

    // Step 2: complete with registration token
    let step2_body = serde_json::json!({
        "username": localpart,
        "password": password,
        "auth": {
            "type": "m.login.registration_token",
            "token": reg_token,
            "session": session,
        },
        "device_id": "CONTEXT_PILOT",
        "initial_device_display_name": "Context Pilot",
        "inhibit_login": false,
    })
    .to_string();

    let (status2, body2) = crate::server::uds_post("/_matrix/client/v3/register", &step2_body)?;

    if (200..300).contains(&status2) {
        let resp2: serde_json::Value =
            serde_json::from_str(&body2).map_err(|e| format!("Cannot parse registration response: {e}"))?;
        return Ok(credentials_from_response(&resp2, &localpart));
    }

    let resp2: serde_json::Value =
        serde_json::from_str(&body2).map_err(|e| format!("Cannot parse registration response: {e}"))?;

    // Account may have been created between step 1 and step 2
    if resp2.get("errcode").and_then(serde_json::Value::as_str).is_some_and(|c| c == "M_USER_IN_USE") {
        return login_bot_account();
    }

    Err(format!(
        "Registration failed (HTTP {status2}): {}",
        resp2.get("error").and_then(serde_json::Value::as_str).unwrap_or("unknown")
    ))
}

/// Log in to an existing bot account (fallback when already registered).
fn login_bot_account() -> Result<Credentials, String> {
    let localpart = generate_user_localpart();

    let body = serde_json::json!({
        "type": "m.login.password",
        "identifier": { "type": "m.id.user", "user": localpart },
        "password": generate_password(),
        "device_id": "CONTEXT_PILOT",
        "initial_device_display_name": "Context Pilot",
    })
    .to_string();

    let (status, resp_body) = crate::server::uds_post("/_matrix/client/v3/login", &body)?;

    if (200..300).contains(&status) {
        let resp: serde_json::Value =
            serde_json::from_str(&resp_body).map_err(|e| format!("Cannot parse login response: {e}"))?;
        return Ok(credentials_from_response(&resp, &localpart));
    }

    let resp: serde_json::Value =
        serde_json::from_str(&resp_body).map_err(|e| format!("Cannot parse login response: {e}"))?;

    Err(format!(
        "Login failed (HTTP {status}): {}",
        resp.get("error").and_then(serde_json::Value::as_str).unwrap_or("unknown")
    ))
}

/// Extract `Credentials` from a registration or login JSON response.
fn credentials_from_response(resp: &serde_json::Value, localpart: &str) -> Credentials {
    Credentials {
        user_id: resp
            .get("user_id")
            .and_then(serde_json::Value::as_str)
            .map_or_else(|| format!("@{localpart}:{SERVER_NAME}"), String::from),
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

/// Create this project's default room.
///
/// Alias: `#cpilot-<hash>:localhost`. Each project gets its own
/// private room. Idempotent: returns `Ok(())` if already exists.
pub(crate) fn create_default_room(access_token: &str) -> Result<(), String> {
    use std::io::{Read as _, Write as _};

    let alias = project_room_alias();

    let body = serde_json::json!({
        "room_alias_name": alias,
        "name": "General",
        "topic": "Default room for Context Pilot chat",
        "visibility": "private",
        "preset": "private_chat",
    })
    .to_string();

    let path = "/_matrix/client/v3/createRoom";
    // Need auth header — use uds_put with POST semantics via raw call
    let sock = crate::server::global_socket_path().ok_or("Cannot determine socket path")?;
    let mut stream = std::os::unix::net::UnixStream::connect(&sock).map_err(|e| format!("UDS connect failed: {e}"))?;
    let _r = stream.set_read_timeout(Some(std::time::Duration::from_secs(10)));

    let content_len = body.len();
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {content_len}\r\nAuthorization: Bearer {access_token}\r\nConnection: close\r\n\r\n{body}"
    );
    stream.write_all(request.as_bytes()).map_err(|e| format!("UDS write failed: {e}"))?;

    let mut response = Vec::with_capacity(4096);
    let _n = stream.read_to_end(&mut response).map_err(|e| format!("UDS read failed: {e}"))?;
    let response_str = String::from_utf8_lossy(&response);

    let status_line = response_str.lines().next().unwrap_or("");
    let status_code = status_line.split_whitespace().nth(1).and_then(|s| s.parse::<u16>().ok()).unwrap_or(0);

    if (200..300).contains(&status_code) {
        return Ok(());
    }

    let resp_body = crate::server::extract_body(&response_str);
    let resp: serde_json::Value = serde_json::from_str(&resp_body).unwrap_or_default();
    let errcode = resp.get("errcode").and_then(serde_json::Value::as_str).unwrap_or("");

    if errcode == "M_ROOM_IN_USE" {
        return Ok(());
    }

    Err(format!(
        "Create room failed (HTTP {status_code}): {}",
        resp.get("error").and_then(serde_json::Value::as_str).unwrap_or("unknown")
    ))
}

/// Set the bot's display name via the Matrix profile API.
///
/// Uses `PUT /profile/{userId}/displayname` over UDS.
pub(crate) fn set_bot_display_name(access_token: &str, display_name: &str) -> Result<(), String> {
    let localpart = generate_user_localpart();
    let user_id = format!("@{localpart}:{SERVER_NAME}");
    let encoded_user = encode_matrix_user_id(&user_id);
    let path = format!("/_matrix/client/v3/profile/{encoded_user}/displayname");

    let body = serde_json::json!({ "displayname": display_name }).to_string();
    let (status, _resp) = crate::server::uds_put(&path, &body, access_token)?;

    if (200..300).contains(&status) { Ok(()) } else { Err(format!("Set display name failed (HTTP {status})")) }
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
