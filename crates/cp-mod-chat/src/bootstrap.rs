//! First-run bootstrap for the Tuwunel homeserver.
//!
//! Creates the data directory layout, generates a minimal
//! `homeserver.toml`, writes a credentials placeholder, and
//! scaffolds the docker-compose template for bridge management.

use std::fmt::Write as _;
use std::path::Path;

use cp_base::state::runtime::State;

use crate::server;
use crate::types::ChatState;

/// Name used as the Matrix server name in local-only mode.
const SERVER_NAME: &str = "localhost";

/// Default bot account localpart.
const BOT_LOCALPART: &str = "context-pilot";

/// Default display name shown in room member lists and message senders.
const BOT_DISPLAY_NAME: &str = "Context Pilot";

/// Run the first-time bootstrap sequence.
///
/// Creates the directory tree under `.context-pilot/matrix/`, writes
/// `homeserver.toml` with secure defaults, and scaffolds support files.
/// Skips any step whose output already exists (safe to call repeatedly).
///
/// # Errors
///
/// Returns a description of the first I/O failure encountered.
pub(crate) fn bootstrap(project_root: &Path) -> Result<(), String> {
    let data = server::data_dir(project_root);
    let cfg = server::config_path(project_root);

    // 1. Create directory tree
    create_dirs(&data)?;

    // 2. Write homeserver.toml (only if absent)
    if !cfg.exists() {
        write_config(&cfg)?;
    }

    // 3. Write credentials placeholder (only if absent)
    let creds = data.join("credentials.json");
    if !creds.exists() {
        write_credentials_placeholder(&creds)?;
    }

    // 4. Scaffold docker-compose template (only if absent)
    let compose = data.join("docker-compose.yaml");
    if !compose.exists() {
        write_docker_compose(&compose)?;
    }

    Ok(())
}

/// Post-start setup: register the bot account, store credentials,
/// and create the default `#general` room.
///
/// Runs once after the server becomes healthy. Skips steps that are
/// already complete (idempotent). Reads `credentials.json` — if the
/// access token is already populated, nothing happens.
///
/// # Errors
///
/// Returns a description of the first failure encountered.
pub(crate) fn post_start_setup(state: &mut State) -> Result<(), String> {
    let root = Path::new(".");
    let data = server::data_dir(root);
    let creds_path = data.join("credentials.json");

    // Load existing credentials to check if registration already done
    let existing = load_credentials(&creds_path)?;
    if !existing.access_token.is_empty() {
        // Already registered — just propagate to ChatState
        let cs = ChatState::get_mut(state);
        cs.bot_user_id = Some(existing.user_id);
        return Ok(());
    }

    // 1. Register the bot account on the homeserver
    let creds = register_bot_account()?;

    // 2. Persist the credentials
    save_credentials(&creds_path, &creds)?;

    // 3. Store user ID in ChatState for immediate use
    let cs = ChatState::get_mut(state);
    cs.bot_user_id = Some(creds.user_id.clone());

    // 4. Create default #general room (best-effort — not fatal)
    if let Err(e) = create_default_room(&creds.access_token) {
        log::warn!("Failed to create default room: {e}");
    }

    // 5. Set bot display name (best-effort — not fatal)
    if let Err(e) = set_bot_display_name(&creds.access_token, BOT_DISPLAY_NAME) {
        log::warn!("Failed to set display name: {e}");
    }

    Ok(())
}

// -- Credential types and I/O -----------------------------------------------

/// Credentials stored in `credentials.json`.
#[derive(serde::Serialize, serde::Deserialize)]
struct Credentials {
    /// Full Matrix user ID (e.g. `@context-pilot:localhost`).
    user_id: String,
    /// Access token for API calls.
    access_token: String,
    /// Device ID assigned during registration.
    device_id: String,
}

/// Load credentials from disk.
fn load_credentials(path: &Path) -> Result<Credentials, String> {
    let contents = std::fs::read_to_string(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    serde_json::from_str(&contents).map_err(|e| format!("Invalid credentials JSON: {e}"))
}

/// Save credentials to disk.
fn save_credentials(path: &Path, creds: &Credentials) -> Result<(), String> {
    let json = serde_json::to_string_pretty(creds).map_err(|e| format!("Cannot serialize credentials: {e}"))?;
    std::fs::write(path, json).map_err(|e| format!("Cannot write {}: {e}", path.display()))
}

// -- Account registration ---------------------------------------------------

/// Register the bot account via the Matrix client registration endpoint.
///
/// Uses the dummy auth flow (available when Tuwunel has registration
/// disabled but the admin creates accounts directly). Falls back to
/// login if the account already exists (`M_USER_IN_USE`).
fn register_bot_account() -> Result<Credentials, String> {
    let url = format!("http://{}/_matrix/client/v3/register", server::SERVER_ADDR);

    let body = serde_json::json!({
        "username": BOT_LOCALPART,
        "password": generate_password(),
        "auth": { "type": "m.login.dummy" },
        "device_id": "CONTEXT_PILOT",
        "initial_device_display_name": "Context Pilot",
        "inhibit_login": false,
    });

    let client = reqwest::blocking::Client::new();
    let resp = client.post(&url).json(&body).send().map_err(|e| format!("Registration request failed: {e}"))?;

    let status = resp.status();
    let resp_body: serde_json::Value = resp.json().map_err(|e| format!("Cannot parse registration response: {e}"))?;

    if status.is_success() {
        return Ok(credentials_from_response(&resp_body));
    }

    // Account already exists — try logging in instead
    if status.as_u16() == 400 {
        let errcode = resp_body.get("errcode").and_then(serde_json::Value::as_str).unwrap_or("");
        if errcode == "M_USER_IN_USE" {
            return login_bot_account();
        }
    }

    Err(format!(
        "Registration failed (HTTP {status}): {}",
        resp_body.get("error").and_then(serde_json::Value::as_str).unwrap_or("unknown")
    ))
}

/// Log in to an existing bot account (fallback when already registered).
fn login_bot_account() -> Result<Credentials, String> {
    let url = format!("http://{}/_matrix/client/v3/login", server::SERVER_ADDR);

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

// -- Default room creation ---------------------------------------------------

/// Create the default `#general:localhost` room.
///
/// Idempotent: returns `Ok(())` if the room alias is already taken.
fn create_default_room(access_token: &str) -> Result<(), String> {
    let url = format!("http://{}/_matrix/client/v3/createRoom", server::SERVER_ADDR);

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
///
/// Idempotent: overwrites the current display name.
fn set_bot_display_name(access_token: &str, display_name: &str) -> Result<(), String> {
    let user_id = format!("@{BOT_LOCALPART}:{SERVER_NAME}");
    let encoded_user = encode_matrix_user_id(&user_id);
    let url = format!("http://{}/_matrix/client/v3/profile/{encoded_user}/displayname", server::SERVER_ADDR);

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

// -- Directory and config scaffolding ----------------------------------------

/// Create the matrix data directory tree.
fn create_dirs(data: &Path) -> Result<(), String> {
    for sub in &["data", "media", "bridges"] {
        let p = data.join(sub);
        std::fs::create_dir_all(&p).map_err(|e| format!("Cannot create {}: {e}", p.display()))?;
    }
    Ok(())
}

/// Write a minimal `homeserver.toml` with localhost-only defaults.
fn write_config(path: &Path) -> Result<(), String> {
    let mut cfg = String::with_capacity(512);

    {
        let _r = writeln!(cfg, "# Tuwunel homeserver configuration");
    }
    {
        let _r = writeln!(cfg, "# Auto-generated by Context Pilot. Edit with care.");
    }
    {
        let _r = writeln!(cfg);
    }
    {
        let _r = writeln!(cfg, "[global]");
    }
    {
        let _r = writeln!(cfg, "server_name = \"{SERVER_NAME}\"");
    }
    {
        let _r = writeln!(cfg, "port = [6167]");
    }
    {
        let _r = writeln!(cfg, "address = \"127.0.0.1\"");
    }
    {
        let _r = writeln!(cfg, "database_backend = \"sqlite\"");
    }
    {
        let _r = writeln!(cfg, "allow_registration = false");
    }
    {
        let _r = writeln!(cfg, "allow_federation = false");
    }
    {
        let _r = writeln!(cfg, "trusted_servers = []");
    }

    std::fs::write(path, cfg).map_err(|e| format!("Cannot write {}: {e}", path.display()))
}

/// Write a JSON placeholder for bot credentials.
///
/// The actual access token is populated after the bot account is
/// registered with the running homeserver (§2 task X439).
fn write_credentials_placeholder(path: &Path) -> Result<(), String> {
    let json = format!(
        "{{\n  \"user_id\": \"@{BOT_LOCALPART}:{SERVER_NAME}\",\n  \"access_token\": \"\",\n  \"device_id\": \"\"\n}}\n"
    );
    std::fs::write(path, json).map_err(|e| format!("Cannot write {}: {e}", path.display()))
}

/// Write the docker-compose template for bridge containers.
fn write_docker_compose(path: &Path) -> Result<(), String> {
    let template = r#"# Matrix bridge containers — managed by docker compose
# Auto-generated by Context Pilot. Uncomment bridges as needed.
#
# Usage:
#   docker compose up -d postgres whatsapp
#
# After starting a bridge:
#   1. The bridge generates config.yaml + registration.yaml in its volume
#   2. Add the registration.yaml path to homeserver.toml [global] app_service_config_files
#   3. Restart Tuwunel (deactivate + reactivate the Chat module)
#   4. Use the bridge bot's login command in any Matrix room (e.g. !wa login)

services:
  postgres:
    image: postgres:16-alpine
    environment:
      POSTGRES_USER: matrix
      POSTGRES_PASSWORD: changeme
    volumes:
      - ./postgres-data:/var/lib/postgresql/data
    ports:
      - "127.0.0.1:5432:5432"
    restart: unless-stopped

  # ── WhatsApp ──────────────────────────────────────
  # whatsapp:
  #   image: dock.mau.dev/mautrix/whatsapp:latest
  #   volumes:
  #     - ./bridges/whatsapp:/data
  #   depends_on: [postgres]
  #   restart: unless-stopped

  # ── Discord ───────────────────────────────────────
  # discord:
  #   image: dock.mau.dev/mautrix/discord:latest
  #   volumes:
  #     - ./bridges/discord:/data
  #   depends_on: [postgres]
  #   restart: unless-stopped

  # ── Telegram ──────────────────────────────────────
  # telegram:
  #   image: dock.mau.dev/mautrix/telegram:latest
  #   volumes:
  #     - ./bridges/telegram:/data
  #   depends_on: [postgres]
  #   restart: unless-stopped

  # ── Signal ────────────────────────────────────────
  # signal:
  #   image: dock.mau.dev/mautrix/signal:latest
  #   volumes:
  #     - ./bridges/signal:/data
  #   depends_on: [postgres]
  #   restart: unless-stopped

  # ── Slack ─────────────────────────────────────────
  # slack:
  #   image: dock.mau.dev/mautrix/slack:latest
  #   volumes:
  #     - ./bridges/slack:/data
  #   depends_on: [postgres]
  #   restart: unless-stopped
"#;
    std::fs::write(path, template).map_err(|e| format!("Cannot write {}: {e}", path.display()))
}
