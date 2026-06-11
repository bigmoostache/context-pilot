//! General settings backend: system info, WiFi (NetworkManager), API key
//! management (`.env`), service restart, and new-project defaults.
//!
//! Everything here is Pi-level — it belongs to the installation, not to a
//! project. The agent already has full control of the machine by design
//! (see the framing doc §7), so these endpoints expose nothing the web
//! token does not already imply; they remain Bearer-guarded like the rest.

use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// API keys surfaced in the settings page (order = display order).
pub const KNOWN_KEYS: &[(&str, &str)] = &[
    ("ANTHROPIC_API_KEY", "Anthropic"),
    ("XAI_API_KEY", "Grok (xAI)"),
    ("GROQ_API_KEY", "Groq"),
    ("DEEPSEEK_API_KEY", "DeepSeek"),
    ("MINIMAX_API_KEY", "MiniMax"),
    ("BRAVE_API_KEY", "Brave Search"),
    ("FIRECRAWL_API_KEY", "Firecrawl"),
    ("GITHUB_TOKEN", "GitHub"),
    ("VOYAGE_API_KEY", "Voyage (embeddings)"),
    ("DATALAB_API_KEY", "Datalab (OCR)"),
];

/// Run a command and capture stdout (empty string on failure).
fn run(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd)
        .args(args)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).to_string())
        .unwrap_or_default()
}

/// Read a small file, trimmed (empty string on failure).
fn read_trimmed(path: &str) -> String {
    std::fs::read_to_string(path).map(|s| s.trim().to_string()).unwrap_or_default()
}

// ─── Infos système ──────────────────────────────────────────────────────────

/// Assemble the system info payload (hostname, uptime, RAM, disk, CPU temp).
#[must_use]
pub fn info(projects_root: Option<&Path>, version: &str) -> Value {
    let hostname = read_trimmed("/etc/hostname");
    let uptime_s = read_trimmed("/proc/uptime").split('.').next().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    let (mem_total_kb, mem_available_kb) = meminfo();
    let temp_milli = read_trimmed("/sys/class/thermal/thermal_zone0/temp").parse::<u64>().unwrap_or(0);
    let ip = run("hostname", &["-I"]).split_whitespace().next().unwrap_or_default().to_string();
    let (disk_total, disk_avail) = disk_root();
    json!({
        "hostname": hostname,
        "ip": ip,
        "version": version,
        "uptime_s": uptime_s,
        "mem_total_kb": mem_total_kb,
        "mem_available_kb": mem_available_kb,
        "cpu_temp_milli_c": temp_milli,
        "disk_total_bytes": disk_total,
        "disk_avail_bytes": disk_avail,
        "projects_root": projects_root.map(|p| p.display().to_string()),
    })
}

/// Parse MemTotal/MemAvailable from `/proc/meminfo` (kB).
fn meminfo() -> (u64, u64) {
    let raw = read_trimmed("/proc/meminfo");
    let field = |name: &str| {
        raw.lines()
            .find(|line| line.starts_with(name))
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0)
    };
    (field("MemTotal:"), field("MemAvailable:"))
}

/// Total/available bytes on `/` via `df` (portable enough for the POC).
fn disk_root() -> (u64, u64) {
    let out = run("df", &["-B1", "--output=size,avail", "/"]);
    let mut nums = out.lines().nth(1).unwrap_or("").split_whitespace();
    let total = nums.next().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    let avail = nums.next().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    (total, avail)
}

// ─── WiFi (NetworkManager) ──────────────────────────────────────────────────

/// Current connection + scan results, via `nmcli` terse output.
#[must_use]
pub fn wifi_status() -> Value {
    let ip = run("hostname", &["-I"]).split_whitespace().next().unwrap_or_default().to_string();
    // `--rescan yes` force NetworkManager à relancer un scan plutôt que de
    // renvoyer le cache du précédent — sinon le bouton « Scanner » ne change rien.
    let list =
        run("nmcli", &["-t", "-f", "ACTIVE,SSID,SIGNAL,SECURITY", "dev", "wifi", "list", "--rescan", "yes"]);
    let mut networks: Vec<Value> = Vec::new();
    let mut current: Option<String> = None;
    let mut seen: Vec<String> = Vec::new();
    for line in list.lines() {
        let mut parts = line.splitn(4, ':');
        let active = parts.next().unwrap_or("") == "yes";
        let ssid = parts.next().unwrap_or("").to_string();
        let signal = parts.next().and_then(|v| v.parse::<u8>().ok()).unwrap_or(0);
        let security = parts.next().unwrap_or("").to_string();
        if ssid.is_empty() || seen.contains(&ssid) {
            continue; // SSID caché ou doublon multi-bandes
        }
        if active {
            current = Some(ssid.clone());
        }
        seen.push(ssid.clone());
        networks.push(json!({ "ssid": ssid, "signal": signal, "security": security, "active": active }));
    }
    json!({ "ip": ip, "current": current, "networks": networks })
}

/// Connect to a `WiFi` network (blocking — run on a blocking thread).
///
/// # Errors
///
/// Returns the nmcli error output on failure.
pub fn wifi_connect(ssid: &str, password: Option<&str>) -> Result<(), String> {
    let mut args: Vec<&str> = vec!["dev", "wifi", "connect", ssid];
    if let Some(pw) = password.filter(|p| !p.is_empty()) {
        args.push("password");
        args.push(pw);
    }
    let output = Command::new("nmcli").args(&args).output().map_err(|e| format!("nmcli introuvable : {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        Err(format!("{}{}", stdout.trim(), stderr.trim()))
    }
}

// ─── Clés API (.env) ────────────────────────────────────────────────────────

/// Validate an env key name: `[A-Z][A-Z0-9_]*`.
#[must_use]
pub fn valid_env_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 64
        && key.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && key.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// Parse a `.env` file into ordered `(key, value)` pairs.
fn parse_env(path: &Path) -> Vec<(String, String)> {
    let raw = std::fs::read_to_string(path).unwrap_or_default();
    raw.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (key, value) = line.split_once('=')?;
            let value = value.trim().trim_matches('"').trim_matches('\'').to_string();
            Some((key.trim().to_string(), value))
        })
        .collect()
}

/// Mask a secret: `••••` + last 4 chars (fully masked when short).
fn mask(value: &str) -> String {
    if value.len() > 8 {
        let tail: String = value.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
        format!("••••{tail}")
    } else {
        "••••".to_string()
    }
}

/// The settings payload: known keys (present or not) + extra keys found in
/// the file, all masked. Never returns plaintext values.
#[must_use]
pub fn env_list(path: &Path, home: Option<&Path>) -> Value {
    let entries = parse_env(path);
    let mut keys: Vec<Value> = KNOWN_KEYS
        .iter()
        .map(|(key, label)| {
            let value = entries.iter().find(|(k, _v)| k == key).map(|(_k, v)| mask(v));
            json!({ "key": key, "label": label, "set": value.is_some(), "masked": value })
        })
        .collect();
    for (key, value) in &entries {
        if !KNOWN_KEYS.iter().any(|(k, _l)| k == key) && key != "CP_WEB_PASSWORD" {
            keys.push(json!({ "key": key, "label": key, "set": true, "masked": mask(value) }));
        }
    }
    let claude_oauth = home.is_some_and(|h| h.join(".claude").join(".credentials.json").exists());
    json!({ "keys": keys, "claude_oauth": claude_oauth })
}

/// Upsert or remove (`value = None`) a key in the `.env` file, atomically.
/// Comments and unrelated lines are preserved.
///
/// # Errors
///
/// Returns a message on invalid key or I/O failure.
pub fn env_set(path: &Path, key: &str, value: Option<&str>) -> Result<(), String> {
    if !valid_env_key(key) {
        return Err("nom de clé invalide".to_string());
    }
    if let Some(v) = value
        && (v.contains('\n') || v.is_empty())
    {
        return Err("valeur invalide".to_string());
    }
    let raw = std::fs::read_to_string(path).unwrap_or_default();
    let mut lines: Vec<String> = Vec::new();
    let mut replaced = false;
    for line in raw.lines() {
        let is_target = line.trim().split_once('=').is_some_and(|(k, _v)| k.trim() == key);
        if is_target {
            if let Some(v) = value
                && !replaced
            {
                lines.push(format!("{key}={v}"));
            }
            replaced = true; // suppression : on saute la ligne
        } else {
            lines.push(line.to_string());
        }
    }
    if !replaced && let Some(v) = value {
        lines.push(format!("{key}={v}"));
    }
    let mut content = lines.join("\n");
    content.push('\n');
    let staging = path.with_extension("env.new");
    std::fs::write(&staging, content).map_err(|e| e.to_string())?;
    // 600 : le fichier contient des secrets.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _r = std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&staging, path).map_err(|e| e.to_string())
}

// ─── Défauts des nouveaux projets ───────────────────────────────────────────

/// Name of the defaults file under the projects root.
pub const DEFAULTS_FILE: &str = ".defaults.json";

/// New-project defaults (applied by the core on a project's first boot).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectDefaults {
    /// Serde ID of the default provider (e.g. `"claudecode"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Serde ID of the default model for that provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// Read the defaults file (empty defaults when absent/invalid).
#[must_use]
pub fn defaults_read(projects_root: &Path) -> ProjectDefaults {
    std::fs::read_to_string(projects_root.join(DEFAULTS_FILE))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Write the defaults file atomically.
///
/// # Errors
///
/// Returns a message on serialization or I/O failure.
pub fn defaults_write(projects_root: &Path, defaults: &ProjectDefaults) -> Result<(), String> {
    let json = serde_json::to_string_pretty(defaults).map_err(|e| e.to_string())?;
    let staging = projects_root.join(".defaults.json.new");
    std::fs::write(&staging, json).map_err(|e| e.to_string())?;
    std::fs::rename(&staging, projects_root.join(DEFAULTS_FILE)).map_err(|e| e.to_string())
}

// ─── Actions système ────────────────────────────────────────────────────────

/// Restart the systemd service (detached: the reply leaves before the kill).
pub fn restart_service() {
    let _r = Command::new("sh").args(["-c", "sleep 1; sudo -n systemctl restart nestor"]).spawn();
}

/// Reboot the Pi (detached).
pub fn reboot() {
    let _r = Command::new("sh").args(["-c", "sleep 2; sudo -n reboot"]).spawn();
}
