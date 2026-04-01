//! Bridge configuration: specs, config templates, and registration management.
//!
//! Each mautrix bridge is a standalone Go binary. This module holds the
//! bridge specification table, generates config files, and manages
//! appservice registration in the homeserver config. Process lifecycle
//! (download, spawn, stop) lives in the [`lifecycle`] submodule.

/// Bridge process lifecycle: download, spawn, stop, health check.
pub(crate) mod lifecycle;
/// Bot token login via config or interactive Matrix commands.
pub(crate) mod login;

use std::fmt::Write as _;
use std::path::PathBuf;

use crate::server;

/// Bridge descriptor: everything needed to configure and run a bridge.
pub(crate) struct BridgeSpec {
    /// Short name used for directories and database names (e.g. `discord`).
    pub name: &'static str,
    /// Default appservice bot username (e.g. `discordbot`).
    pub bot_username: &'static str,
    /// Default appservice port the bridge listens on.
    pub appservice_port: u16,
    /// Puppet user namespace regex (e.g. `@discord_.*`).
    pub user_namespace: &'static str,
    /// Puppet room alias namespace regex (e.g. `#discord_.*`).
    pub alias_namespace: &'static str,
    /// Environment variable name for the bot token.
    pub token_env_var: &'static str,
    /// Whether this bridge supports config-based bot login (vs Matrix command).
    pub config_login: bool,
}

/// All supported mautrix bridges with their configuration defaults.
///
/// Only platforms with proper bot APIs are included — no human
/// impersonation, no phone-number-required flows (Iron Law #1).
///
/// Each bridge binary is downloaded from GitHub:
/// `https://github.com/mautrix/{name}/releases/latest/download/mautrix-{name}-{arch}`
pub(crate) const BRIDGES: &[BridgeSpec] = &[
    BridgeSpec {
        name: "telegram",
        bot_username: "telegrambot",
        appservice_port: 29317,
        user_namespace: "@telegram_.*",
        alias_namespace: "#telegram_.*",
        token_env_var: "TELEGRAM_BOT_TOKEN",
        config_login: false, // mautrix-telegram Go rewrite: login via Matrix DM command
    },
    BridgeSpec {
        name: "discord",
        bot_username: "discordbot",
        appservice_port: 29319,
        user_namespace: "@discord_.*",
        alias_namespace: "#discord_.*",
        token_env_var: "DISCORD_BOT_TOKEN",
        config_login: false,
    },
    BridgeSpec {
        name: "slack",
        bot_username: "slackbot",
        appservice_port: 29322,
        user_namespace: "@slack_.*",
        alias_namespace: "#slack_.*",
        token_env_var: "SLACK_BOT_TOKEN",
        config_login: false,
    },
    BridgeSpec {
        name: "googlechat",
        bot_username: "googlechatbot",
        appservice_port: 29325,
        user_namespace: "@googlechat_.*",
        alias_namespace: "#googlechat_.*",
        token_env_var: "GOOGLECHAT_SERVICE_ACCOUNT",
        config_login: true,
    },
];

// -- Config generation -------------------------------------------------------

/// Bridge data directory: `~/.context-pilot/matrix/bridges/{name}/`
#[must_use]
pub(crate) fn bridge_data_dir(name: &str) -> PathBuf {
    server::global_matrix_dir().unwrap_or_else(|| PathBuf::from(".context-pilot/matrix")).join("bridges").join(name)
}

/// Patch bridge config files with our required values.
///
/// Instead of replacing the config (which loses mautrix defaults for
/// hundreds of required fields), this reads the existing config and
/// patches only the lines we care about: homeserver address, database
/// URI, bot credentials, permissions, and platform-specific tokens.
///
/// Must run **after** `generate_registrations()` which creates the
/// initial config via `mautrix -g`.
///
/// # Errors
///
/// Returns a description of the first I/O failure encountered.
pub(crate) fn generate_bridge_configs() -> Result<(), String> {
    for spec in BRIDGES {
        let dir = bridge_data_dir(spec.name);
        std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;

        let cfg_path = dir.join("config.yaml");
        if !cfg_path.exists() {
            // Config missing (maybe deleted) but registration exists — regenerate
            // the mautrix default config by seeding {} and running -g again.
            let reg_path = dir.join("registration.yaml");
            if reg_path.exists() {
                std::fs::write(&cfg_path, "{}\n")
                    .map_err(|e| format!("Cannot seed config for mautrix-{}: {e}", spec.name))?;
                let bin = lifecycle::ensure_binary(spec.name)?;
                let _output = std::process::Command::new(&bin)
                    .arg("--generate-registration")
                    .arg("--config")
                    .arg(&cfg_path)
                    .arg("--registration")
                    .arg(&reg_path)
                    .current_dir(&dir)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .output()
                    .map_err(|e| format!("Failed to regenerate config for mautrix-{}: {e}", spec.name))?;
                log::info!("Regenerated default config for mautrix-{}", spec.name);
            } else {
                continue; // No config or registration — generate_registrations() will handle
            }
        }

        patch_bridge_config(spec, &cfg_path)?;

        let reg_path = dir.join("registration.yaml.sample");
        if !reg_path.exists()
            && let Some(reg) = render_registration_template(spec.name)
        {
            std::fs::write(&reg_path, reg).map_err(|e| format!("Cannot write {}: {e}", reg_path.display()))?;
        }
    }

    Ok(())
}

/// Patch specific fields in a mautrix-generated config file.
///
/// Reads the full config (typically ~600 lines from `mautrix -g`),
/// replaces only the values we need, and writes it back. This preserves
/// all mautrix defaults that the bridge requires to function.
///
/// Uses a top-level section tracker to avoid ambiguity — the mautrix
/// config reuses keywords like `uri:` and `address:` across different
/// sections, so naive string matching would patch the wrong fields.
fn patch_bridge_config(spec: &BridgeSpec, cfg_path: &std::path::Path) -> Result<(), String> {
    let content = std::fs::read_to_string(cfg_path).map_err(|e| format!("Cannot read {}: {e}", cfg_path.display()))?;

    let sock_path = server::global_socket_path()
        .map_or_else(|| "http://localhost:6167".to_string(), |p| format!("unix:{}", p.to_string_lossy()));
    let db_path = bridge_data_dir(spec.name).join(format!("{}.db", spec.name));
    let db_uri = format!("file:{}?_txlock=immediate", db_path.to_string_lossy());

    // Read tokens from registration.yaml
    let reg_path = bridge_data_dir(spec.name).join("registration.yaml");
    let (as_token, hs_token) = std::fs::read_to_string(&reg_path).map_or((None, None), |reg| {
        (lifecycle::extract_yaml_value(&reg, "as_token"), lifecycle::extract_yaml_value(&reg, "hs_token"))
    });

    let mut patched = String::with_capacity(content.len());
    // Current top-level section (indent 0, e.g. "homeserver", "database", "bridge")
    let mut section = String::new();
    // Current second-level key (indent 4, e.g. "bot", "relay", "permissions")
    let mut subsection = String::new();
    let mut in_permissions = false;

    for line in content.lines() {
        let trimmed = line.trim();
        let indent = line.len().saturating_sub(line.trim_start().len());

        // Track top-level sections (zero-indent keys like "homeserver:", "database:")
        if indent == 0
            && !trimmed.is_empty()
            && !trimmed.starts_with('#')
            && let Some(key) = trimmed.strip_suffix(':')
        {
            section = key.to_string();
            subsection.clear();
        }
        // Track second-level subsections (indent 4)
        if indent == 4 && !trimmed.is_empty() && !trimmed.starts_with('#') {
            if let Some(key) = trimmed.strip_suffix(':') {
                subsection = key.to_string();
            } else if let Some((key, _)) = trimmed.split_once(':') {
                subsection = key.to_string();
            }
        }

        // === homeserver section ===
        if section == "homeserver" {
            if trimmed.starts_with("address:") {
                let _r = writeln!(patched, "    address: {sock_path}");
                continue;
            }
            if trimmed.starts_with("domain:") {
                patched.push_str("    domain: localhost\n");
                continue;
            }
        }

        // === database section ===
        if section == "database" {
            if trimmed.starts_with("type:") {
                patched.push_str("    type: sqlite3-fk-wal\n");
                continue;
            }
            if trimmed.starts_with("uri:") {
                let _r = writeln!(patched, "    uri: {db_uri}");
                continue;
            }
        }

        // === appservice section ===
        if section == "appservice" {
            if subsection == "bot" && trimmed.starts_with("username:") {
                let _r = writeln!(patched, "        username: {}", spec.bot_username);
                continue;
            }
            if subsection == "bot" && trimmed.starts_with("displayname:") {
                let _r = writeln!(patched, "        displayname: {} bridge bot", capitalize(spec.name));
                continue;
            }
            if trimmed.starts_with("as_token:")
                && let Some(tok) = &as_token
            {
                let _r = writeln!(patched, "    as_token: {tok}");
                continue;
            }
            if trimmed.starts_with("hs_token:")
                && let Some(tok) = &hs_token
            {
                let _r = writeln!(patched, "    hs_token: {tok}");
                continue;
            }
        }

        // === bridge section ===
        if section == "bridge" {
            // Relay enabled — only within the relay subsection
            if subsection == "relay" && trimmed.starts_with("enabled:") {
                patched.push_str("        enabled: true\n");
                continue;
            }
            // Permissions — replace with blanket localhost access
            if trimmed == "permissions:" {
                in_permissions = true;
                patched.push_str(line);
                patched.push('\n');
                continue;
            }
            if in_permissions {
                if trimmed.starts_with('"') || trimmed.starts_with('\'') || trimmed.starts_with('*') {
                    continue;
                }
                patched.push_str("        \"localhost\": user\n");
                in_permissions = false;
            }
        }

        // === network section (platform-specific credentials) ===
        // mautrix-telegram puts api_id/api_hash/bot_token under "network:", not "telegram:".
        if section == "network" && spec.name == "telegram" {
            if trimmed.starts_with("api_id:") {
                let api_id =
                    cp_base::config::global::resolve_api_key("telegram_api_id").unwrap_or_else(|| "12345".to_string());
                let _r = writeln!(patched, "    api_id: {api_id}");
                continue;
            }
            if trimmed.starts_with("api_hash:") {
                let api_hash = cp_base::config::global::resolve_api_key("telegram_api_hash")
                    .unwrap_or_else(|| "YOUR_API_HASH_HERE".to_string());
                let _r = writeln!(patched, "    api_hash: {api_hash}");
                continue;
            }
            if trimmed.starts_with("bot_token:") {
                let token = std::env::var(spec.token_env_var).unwrap_or_else(|_| "YOUR_BOT_TOKEN_HERE".to_string());
                let _r = writeln!(patched, "    bot_token: {token}");
                continue;
            }
        }

        // Default: keep the line as-is
        patched.push_str(line);
        patched.push('\n');
    }

    // Flush any remaining permission injection
    if in_permissions {
        patched.push_str("        \"localhost\": user\n");
    }

    std::fs::write(cfg_path, &patched).map_err(|e| format!("Cannot write {}: {e}", cfg_path.display()))?;
    log::info!("Patched config for mautrix-{}", spec.name);
    Ok(())
}

/// Build `--execute` argument strings for registering all bridges with Tuwunel.
///
/// Each returned string is a Tuwunel admin command that registers one
/// appservice. Pass these as `--execute <arg>` to the Tuwunel binary
/// at startup. Bridges without a `registration.yaml` are skipped.
///
/// Format: `"appservices register\n<yaml_content>"`
#[must_use]
pub(crate) fn build_appservice_execute_args() -> Vec<String> {
    find_registration_files()
        .iter()
        .filter_map(|reg_path| {
            let yaml = std::fs::read_to_string(reg_path).ok()?;
            // Tuwunel's admin parser expects a code block even via --execute.
            // Format mirrors the admin room syntax: command + ```yaml\n...\n```
            Some(format!("appservices register\n```yaml\n{yaml}```"))
        })
        .collect()
}

// -- Registration file management --------------------------------------------

/// Scan for `registration.yaml` files across all global bridge directories.
#[must_use]
pub(crate) fn find_registration_files() -> Vec<PathBuf> {
    let mut found = Vec::new();
    for spec in BRIDGES {
        let reg = bridge_data_dir(spec.name).join("registration.yaml");
        if reg.exists() {
            found.push(reg);
        }
    }
    found
}

// -- Helpers -----------------------------------------------------------------

/// Resolve the bot token for a bridge from environment variables.
///
/// Returns `None` if the env var is unset or empty.
pub(crate) fn resolve_bot_token(bridge_name: &str) -> Option<String> {
    let spec = BRIDGES.iter().find(|b| b.name == bridge_name)?;
    let val = std::env::var(spec.token_env_var).ok()?;
    if val.is_empty() { None } else { Some(val) }
}

/// Render a sample `registration.yaml` for documentation purposes.
#[must_use]
pub(crate) fn render_registration_template(bridge_name: &str) -> Option<String> {
    let spec = BRIDGES.iter().find(|b| b.name == bridge_name)?;

    let mut out = String::with_capacity(512);
    {
        let _r = writeln!(out, "# Registration template for mautrix-{}", spec.name);
    }
    {
        let _r = writeln!(out, "# Replace as_token and hs_token with values from the bridge.");
    }
    {
        let _r = writeln!(out, "id: \"{}\"", spec.name);
    }
    {
        let _r = writeln!(out, "url: \"http://localhost:{}\"", spec.appservice_port);
    }
    {
        let _r = writeln!(out, "as_token: \"REPLACE_ME\"");
    }
    {
        let _r = writeln!(out, "hs_token: \"REPLACE_ME\"");
    }
    {
        let _r = writeln!(out, "sender_localpart: \"{}\"", spec.bot_username);
    }
    {
        let _r = writeln!(out, "namespaces:");
    }
    {
        let _r = writeln!(out, "  users:");
    }
    {
        let _r = writeln!(out, "    - exclusive: true");
    }
    {
        let _r = writeln!(out, "      regex: \"{}:localhost\"", spec.user_namespace);
    }
    {
        let _r = writeln!(out, "  aliases:");
    }
    {
        let _r = writeln!(out, "    - exclusive: true");
    }
    {
        let _r = writeln!(out, "      regex: \"{}:localhost\"", spec.alias_namespace);
    }

    Some(out)
}

/// Capitalize the first letter of a string.
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map_or_else(String::new, |c| {
        let mut result = String::with_capacity(s.len());
        for upper in c.to_uppercase() {
            result.push(upper);
        }
        result.extend(chars);
        result
    })
}
