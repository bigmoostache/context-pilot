//! Bridge configuration: specs, config templates, and registration management.
//!
//! Each mautrix bridge is a standalone Go binary. This module holds the
//! bridge specification table, generates config files, and manages
//! appservice registration in the homeserver config. Process lifecycle
//! (download, spawn, stop) lives in the [`lifecycle`] submodule.

/// Bridge process lifecycle: download, spawn, stop, health check.
pub(crate) mod lifecycle;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

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
}

/// All supported mautrix bridges with their configuration defaults.
///
/// Each bridge binary is downloaded from GitHub:
/// `https://github.com/mautrix/{name}/releases/latest/download/mautrix-{name}-{arch}`
pub(crate) const BRIDGES: &[BridgeSpec] = &[
    BridgeSpec {
        name: "whatsapp",
        bot_username: "whatsappbot",
        appservice_port: 29318,
        user_namespace: "@whatsapp_.*",
        alias_namespace: "#whatsapp_.*",
    },
    BridgeSpec {
        name: "discord",
        bot_username: "discordbot",
        appservice_port: 29319,
        user_namespace: "@discord_.*",
        alias_namespace: "#discord_.*",
    },
    BridgeSpec {
        name: "telegram",
        bot_username: "telegrambot",
        appservice_port: 29320,
        user_namespace: "@telegram_.*",
        alias_namespace: "#telegram_.*",
    },
    BridgeSpec {
        name: "signal",
        bot_username: "signalbot",
        appservice_port: 29321,
        user_namespace: "@signal_.*",
        alias_namespace: "#signal_.*",
    },
    BridgeSpec {
        name: "slack",
        bot_username: "slackbot",
        appservice_port: 29322,
        user_namespace: "@slack_.*",
        alias_namespace: "#slack_.*",
    },
    BridgeSpec {
        name: "meta",
        bot_username: "metabot",
        appservice_port: 29323,
        user_namespace: "@meta_.*",
        alias_namespace: "#meta_.*",
    },
    BridgeSpec {
        name: "twitter",
        bot_username: "twitterbot",
        appservice_port: 29324,
        user_namespace: "@twitter_.*",
        alias_namespace: "#twitter_.*",
    },
    BridgeSpec {
        name: "googlechat",
        bot_username: "googlechatbot",
        appservice_port: 29325,
        user_namespace: "@googlechat_.*",
        alias_namespace: "#googlechat_.*",
    },
    BridgeSpec {
        name: "gmessages",
        bot_username: "gmessagesbot",
        appservice_port: 29326,
        user_namespace: "@gmessages_.*",
        alias_namespace: "#gmessages_.*",
    },
    BridgeSpec {
        name: "bluesky",
        bot_username: "blueskybot",
        appservice_port: 29327,
        user_namespace: "@bluesky_.*",
        alias_namespace: "#bluesky_.*",
    },
];

// -- Config generation -------------------------------------------------------

/// Bridge data directory: `.context-pilot/matrix/bridges/{name}/`
#[must_use]
pub(crate) fn bridge_data_dir(project_root: &Path, name: &str) -> PathBuf {
    server::data_dir(project_root).join("bridges").join(name)
}

/// Generate config templates for all known bridges.
///
/// Each config is written to `.context-pilot/matrix/bridges/{name}/config.yaml`.
/// Existing files are **not** overwritten (safe to call repeatedly).
///
/// # Errors
///
/// Returns a description of the first I/O failure encountered.
pub(crate) fn generate_bridge_configs(project_root: &Path) -> Result<(), String> {
    for spec in BRIDGES {
        let dir = bridge_data_dir(project_root, spec.name);
        std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;

        let cfg_path = dir.join("config.yaml");
        if cfg_path.exists() {
            continue;
        }

        let content = render_bridge_config(spec, project_root);
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

/// Render a bridge `config.yaml` using `SQLite` and localhost addresses.
fn render_bridge_config(spec: &BridgeSpec, project_root: &Path) -> String {
    let hs_addr = format!("http://{}", server::server_addr());
    let db_path = bridge_data_dir(project_root, spec.name).join(format!("{}.db", spec.name));
    let db_uri = format!("file:{}?_txlock=immediate", db_path.to_string_lossy());

    let mut cfg = String::with_capacity(1024);

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
        let _r = writeln!(cfg, "  address: {hs_addr}");
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
        let _r = writeln!(cfg, "  database:");
    }
    {
        let _r = writeln!(cfg, "    type: sqlite3-fk-wal");
    }
    {
        let _r = writeln!(cfg, "    uri: {db_uri}");
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
        let _r = writeln!(cfg, "    \"localhost\": user");
    }
    {
        let _r = writeln!(cfg, "    \"@context-pilot:localhost\": admin");
    }

    cfg
}

// -- Registration file management --------------------------------------------

/// Scan for `registration.yaml` files across all bridge directories.
#[must_use]
pub(crate) fn find_registration_files(project_root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for spec in BRIDGES {
        let reg = bridge_data_dir(project_root, spec.name).join("registration.yaml");
        if reg.exists() {
            found.push(reg);
        }
    }
    found
}

/// Update the homeserver config to include all detected registration files.
///
/// Reads `homeserver.toml`, appends any registration paths not already
/// listed to `app_service_config_files`, and writes back.
///
/// # Errors
///
/// Returns a description if the config cannot be read or written.
pub(crate) fn update_appservice_registrations(project_root: &Path) -> Result<bool, String> {
    let registrations = find_registration_files(project_root);
    if registrations.is_empty() {
        return Ok(false);
    }

    let cfg_path = server::config_path(project_root);
    let content = std::fs::read_to_string(&cfg_path).map_err(|e| format!("Cannot read {}: {e}", cfg_path.display()))?;

    let cfg_dir = cfg_path.parent().unwrap_or_else(|| Path::new("."));
    let reg_strs: Vec<String> = registrations
        .iter()
        .filter_map(|p| p.strip_prefix(cfg_dir).ok())
        .map(|p| p.to_string_lossy().to_string())
        .collect();

    let has_new = reg_strs.iter().any(|r| !content.contains(r.as_str()));
    if !has_new {
        return Ok(false);
    }

    let mut list = String::from("app_service_config_files = [");
    for (i, reg) in reg_strs.iter().enumerate() {
        if i > 0 {
            list.push_str(", ");
        }
        {
            let _r = write!(list, "\"{reg}\"");
        }
    }
    list.push(']');

    let updated = if content.contains("app_service_config_files") {
        let mut result = String::with_capacity(content.len());
        for line in content.lines() {
            if line.trim_start().starts_with("app_service_config_files") {
                result.push_str(&list);
            } else {
                result.push_str(line);
            }
            result.push('\n');
        }
        result
    } else {
        let mut result = String::with_capacity(content.len().saturating_add(list.len()).saturating_add(2));
        let mut inserted = false;
        for line in content.lines() {
            result.push_str(line);
            result.push('\n');
            if !inserted && line.trim() == "[global]" {
                result.push_str(&list);
                result.push('\n');
                inserted = true;
            }
        }
        if !inserted {
            result.push_str(&list);
            result.push('\n');
        }
        result
    };

    std::fs::write(&cfg_path, updated).map_err(|e| format!("Cannot write {}: {e}", cfg_path.display()))?;
    Ok(true)
}

// -- Helpers -----------------------------------------------------------------

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
