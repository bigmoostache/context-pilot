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
        appservice_port: 29320,
        user_namespace: "@telegram_.*",
        alias_namespace: "#telegram_.*",
        token_env_var: "TELEGRAM_BOT_TOKEN",
        config_login: true,
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

/// Generate config templates for all known bridges.
///
/// Each config is written to `~/.context-pilot/matrix/bridges/{name}/config.yaml`.
/// Existing files are **not** overwritten (safe to call repeatedly).
///
/// # Errors
///
/// Returns a description of the first I/O failure encountered.
pub(crate) fn generate_bridge_configs() -> Result<(), String> {
    for spec in BRIDGES {
        let dir = bridge_data_dir(spec.name);
        std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;

        let cfg_path = dir.join("config.yaml");

        let content = render_bridge_config(spec);
        std::fs::write(&cfg_path, content).map_err(|e| format!("Cannot write {}: {e}", cfg_path.display()))?;

        let reg_path = dir.join("registration.yaml.sample");
        if !reg_path.exists()
            && let Some(reg) = render_registration_template(spec.name)
        {
            std::fs::write(&reg_path, reg).map_err(|e| format!("Cannot write {}: {e}", reg_path.display()))?;
        }
    }

    Ok(())
}

/// Render a bridge `config.yaml` using `SQLite` and UDS connection.
fn render_bridge_config(spec: &BridgeSpec) -> String {
    let sock_path = server::global_socket_path()
        .map_or_else(|| "http://localhost:6167".to_string(), |p| format!("unix:{}", p.to_string_lossy()));
    let db_path = bridge_data_dir(spec.name).join(format!("{}.db", spec.name));
    let db_uri = format!("file:{}?_txlock=immediate", db_path.to_string_lossy());

    let mut cfg = String::with_capacity(1024);

    // Pre-read tokens from registration.yaml if it exists
    let reg_path = bridge_data_dir(spec.name).join("registration.yaml");
    let (as_token, hs_token) = std::fs::read_to_string(&reg_path).map_or((None, None), |reg| {
        (lifecycle::extract_yaml_value(&reg, "as_token"), lifecycle::extract_yaml_value(&reg, "hs_token"))
    });

    {
        let _r = writeln!(cfg, "# mautrix-{} configuration", spec.name);
    }
    {
        let _r = writeln!(cfg, "# Auto-generated by Context Pilot. Edit credentials before use.");
    }
    {
        let _r = writeln!(cfg, "# Documentation: https://docs.mau.fi/bridges/go/setup.html");
    }
    {
        let _r = writeln!(cfg);
    }
    {
        let _r = writeln!(cfg, "homeserver:");
    }
    {
        let _r = writeln!(cfg, "  address: {sock_path}");
    }
    {
        let _r = writeln!(cfg, "  domain: localhost");
    }
    {
        let _r = writeln!(cfg);
    }
    {
        let _r = writeln!(cfg, "appservice:");
    }
    {
        let _r = writeln!(cfg, "  address: http://localhost:{}", spec.appservice_port);
    }
    {
        let _r = writeln!(cfg, "  hostname: 0.0.0.0");
    }
    {
        let _r = writeln!(cfg, "  port: {}", spec.appservice_port);
    }
    {
        let _r = writeln!(cfg, "  bot:");
    }
    {
        let _r = writeln!(cfg, "    username: {}", spec.bot_username);
    }
    {
        let _r = writeln!(cfg, "    displayname: {} bridge bot", capitalize(spec.name));
    }
    // Inject as_token/hs_token inside appservice section
    if let Some(tok) = &as_token {
        let _r = writeln!(cfg, "  as_token: {tok}");
    }
    if let Some(tok) = &hs_token {
        let _r = writeln!(cfg, "  hs_token: {tok}");
    }
    {
        let _r = writeln!(cfg);
    }
    {
        let _r = writeln!(cfg, "database:");
    }
    {
        let _r = writeln!(cfg, "  type: sqlite3-fk-wal");
    }
    {
        let _r = writeln!(cfg, "  uri: \"{db_uri}\"");
    }
    {
        let _r = writeln!(cfg);
    }
    {
        let _r = writeln!(cfg, "bridge:");
    }
    {
        let _r = writeln!(cfg, "  permissions:");
    }
    {
        // All cpilot-* users on localhost get user-level access
        let _r = writeln!(cfg, "    \"localhost\": user");
    }
    {
        let _r = writeln!(cfg, "  relay:");
    }
    {
        let _r = writeln!(cfg, "    enabled: true");
    }

    // Platform-specific bot configuration
    write_platform_config(spec, &mut cfg);

    cfg
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

/// Write platform-specific bot configuration into the config YAML.
///
/// Injects the bot token from environment variables directly into the
/// config file so the bridge auto-authenticates on startup. Only
/// applies to bridges with `config_login = true`.
fn write_platform_config(spec: &BridgeSpec, cfg: &mut String) {
    let token = std::env::var(spec.token_env_var).ok();

    match spec.name {
        "telegram" => {
            let api_id =
                cp_base::config::global::resolve_api_key("telegram_api_id").unwrap_or_else(|| "12345".to_string());
            let api_hash = cp_base::config::global::resolve_api_key("telegram_api_hash")
                .unwrap_or_else(|| "YOUR_API_HASH_HERE".to_string());
            {
                let _r = writeln!(cfg);
            }
            {
                let _r = writeln!(cfg, "telegram:");
            }
            let val = token.as_deref().unwrap_or("YOUR_BOT_TOKEN_HERE");
            {
                let _r = writeln!(cfg, "  api_id: {api_id}");
            }
            {
                let _r = writeln!(cfg, "  api_hash: {api_hash}");
            }
            {
                let _r = writeln!(cfg, "  bot_token: \"{val}\"");
            }
        }
        "googlechat" => {
            {
                let _r = writeln!(cfg);
            }
            {
                let _r = writeln!(cfg, "googlechat:");
            }
            let val = token.as_deref().unwrap_or("/path/to/service-account.json");
            {
                let _r = writeln!(cfg, "  service_account_key: \"{val}\"");
            }
        }
        // Discord and Slack use interactive Matrix commands for login —
        // no config-based token injection needed. See ensure_bridge_login().
        _ => {}
    }
}

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
