//! First-run bootstrap for the global Tuwunel homeserver.
//!
//! Creates the directory layout under `~/.context-pilot/matrix/`,
//! generates a minimal `homeserver.toml` with UDS-only configuration,
//! and ensures the Tuwunel binary is present (downloading on first run).
//!
//! Per-project setup (user registration, room creation) lives in
//! [`crate::client::account`] and runs after the server is healthy.

use std::fmt::Write as _;

use cp_base::state::runtime::State;

use crate::client::account;
use crate::server;
use crate::types::ChatState;

/// Name used as the Matrix server name in local-only mode.
const SERVER_NAME: &str = "localhost";

/// Run the global bootstrap sequence.
///
/// Creates the directory tree under `~/.context-pilot/matrix/`, writes
/// `homeserver.toml` with UDS-only defaults, and ensures the binary
/// is present. Skips any step whose output already exists (idempotent).
///
/// # Errors
///
/// Returns a description of the first I/O failure encountered.
pub(crate) fn bootstrap() -> Result<(), String> {
    // 0. Ensure the Tuwunel binary is present (download if missing)
    crate::client::download::ensure_binary()?;

    // 1. Create global directory tree
    let matrix_dir = server::ensure_global_dirs()?;

    // 2. Write homeserver.toml (only if absent)
    let cfg = matrix_dir.join("homeserver.toml");
    if !cfg.exists() {
        write_config(&cfg, &matrix_dir)?;
    }

    // 3. Generate bridge registrations (runs mautrix -g for bridges that
    //    lack a registration.yaml; note: -g clobbers config.yaml with defaults)
    if let Err(e) = crate::bridges::lifecycle::generate_registrations() {
        log::warn!("Bridge registration generation failed: {e}");
    }

    // 4. Write our config templates ON TOP of the mautrix defaults.
    //    This restores homeserver address, database URI, bot token, etc.
    //    Always writes — not idempotent-skip — because -g may have clobbered.
    crate::bridges::generate_bridge_configs()?;

    // Note: appservice registration with Tuwunel happens in project_post_start()
    // after the Matrix client is connected — Tuwunel uses admin room commands,
    // not config-file-based registration.

    Ok(())
}

/// Post-start setup: register this project's bot account, store
/// credentials, and create its default room.
///
/// Runs once per project after the server becomes healthy. Reads
/// per-project `credentials.json` — if the access token is already
/// populated, skips registration. Idempotent.
///
/// # Errors
///
/// Returns a description of the first failure encountered.
pub(crate) fn project_post_start(state: &mut State) -> Result<(), String> {
    let creds_path = account::project_credentials_path();

    // Load existing credentials and validate the token is still accepted.
    // Tuwunel may have restarted, invalidating old access tokens —
    // if so, fall through to re-registration instead of using a stale token.
    if let Ok(existing) = account::load_credentials(&creds_path)
        && !existing.access_token.is_empty()
        && account::validate_token(&existing.access_token)
    {
        let cs = ChatState::get_mut(state);
        cs.bot_user_id = Some(existing.user_id);
        return Ok(());
    }

    // 1. Register this project's bot account on the homeserver
    let creds = match account::register_bot_account() {
        Ok(c) => c,
        Err(e) => {
            // Temporary debug trace — write to file so we can see what went wrong
            drop(std::fs::write(".context-pilot/matrix/registration_debug.log", format!("Registration failed: {e}\n")));
            return Err(e);
        }
    };

    // 2. Persist the credentials (per-project)
    account::save_credentials(&creds_path, &creds)?;

    // 3. Store user ID in ChatState for immediate use
    let cs = ChatState::get_mut(state);
    cs.bot_user_id = Some(creds.user_id.clone());

    // 4. Create this project's default room (best-effort)
    if let Err(e) = account::create_default_room(&creds.access_token) {
        log::warn!("Failed to create default room: {e}");
    }

    // 5. Set bot display name (best-effort)
    let display_name = account::project_display_name();
    if let Err(e) = account::set_bot_display_name(&creds.access_token, &display_name) {
        log::warn!("Failed to set display name: {e}");
    }

    // Note: appservice registration with Tuwunel happens in init_state()
    // after client::connect() — requires a live Matrix client to send
    // admin room commands.

    Ok(())
}

// -- Config scaffolding ------------------------------------------------------

/// Write a minimal `homeserver.toml` with UDS-only defaults.
///
/// Uses `RocksDB` (Tuwunel's default backend) with data stored in the
/// global `~/.context-pilot/matrix/data/` directory. Listens only on a
/// Unix domain socket — no TCP port is opened.
fn write_config(path: &std::path::Path, matrix_dir: &std::path::Path) -> Result<(), String> {
    let db_path = matrix_dir.join("data").join("db");
    let db_str = db_path.to_string_lossy();
    let reg_token = account::generate_registration_token();
    let sock_path = server::global_socket_path().ok_or("Cannot determine socket path")?;
    let sock_str = sock_path.to_string_lossy();

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
        let _r = writeln!(cfg, "database_path = \"{db_str}\"");
    }
    {
        let _r = writeln!(cfg, "unix_socket_path = \"{sock_str}\"");
    }
    {
        let _r = writeln!(cfg, "unix_socket_perms = 660");
    }
    {
        let _r = writeln!(cfg, "allow_registration = true");
    }
    {
        let _r = writeln!(cfg, "registration_token = \"{reg_token}\"");
    }
    {
        let _r = writeln!(cfg, "allow_federation = false");
    }
    {
        let _r = writeln!(cfg, "trusted_servers = []");
    }

    std::fs::write(path, cfg).map_err(|e| format!("Cannot write {}: {e}", path.display()))
}
