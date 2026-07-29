//! Internet uplink (WAN / 5G) + Wi-Fi access point — state, API and applier.
//!
//! Design: `docs/design-network-uplink.md`. The appliance ships with a 5G modem
//! and two Wi-Fi radios and, before this module, used neither. Three uplink
//! modes (`wan`, `wan_5g`, `5g`) and an access point are configurable by the
//! vendor at provisioning time (Ansible seeds [`state`]) and by the client's IT
//! admin from the cockpit (`can_manage_it`), without the two fighting over the
//! same state — Ansible's seed is write-once, and this module is the only writer
//! after it.
//!
//! * [`state`] — the `.network.json` document, its validation and its
//!   secret-eliding read projection.
//! * [`apply`] — state → system configuration (`nmcli`, `networkctl`, `iw`,
//!   `sysctl`, and the Caddy site list). Env-gated exactly like `CP_CADDYFILE`:
//!   with the gates unset the backend persists and performs **no** system call,
//!   which is what makes the whole feature testable off-box (NFR-NET-04).
//! * [`status`] — the live read-back the cockpit polls.
//!
//! **The invariant that outranks every feature here (NFR-NET-01):** no mode ever
//! alters an address on `end0`/`end1`. Only default routes are touched. The
//! fleet ULA, the DHCP lease, the LAN reachability of the cockpit and the day-0
//! access path survive every mode, because they are the fleet's only recovery
//! path.

pub(crate) mod apply;
pub(crate) mod state;
pub(crate) mod status;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Deserializer};

use super::Backend;
use super::HttpReply;
use state::{ApConfig, Band, NetworkConfig, Standby, UplinkMode, WwanConfig};

/// serde helper distinguishing an **absent** field from an explicit `null`.
///
/// Secrets are write-only: a read never returns them, so the cockpit cannot send
/// back what it never received. Without this distinction every AP save from a UI
/// that simply omits the untouched passphrase field would wipe the PSK. With it,
/// absent means "keep", `null` means "clear", a string means "replace".
///
/// # Errors
///
/// Propagates the deserializer's own error for a malformed value.
fn double_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Deserialize::deserialize(deserializer).map(Some)
}

/// Resolve `.network.json`'s path from the backend, or the reply to send when
/// the backend lock is poisoned.
fn config_path(state: &Mutex<Backend>) -> Result<PathBuf, HttpReply> {
    match state.lock() {
        Ok(backend) => Ok(state::network_path(&backend.agents_dir)),
        Err(_poisoned) => Err(HttpReply::error(500, "backend lock poisoned")),
    }
}

/// Persist `next`, then apply it — rolling **both** back on failure.
///
/// NFR-NET-05: a bad setting can never wedge the box. This mirrors
/// [`caddy::regenerate`](super::caddy::regenerate) deliberately, down to the
/// shape of the error: the previous document is written back and re-applied, so
/// the box is left exactly as it was found, and the caller turns the `Err` into
/// a `502`.
///
/// # Errors
///
/// Returns a message when the document cannot be persisted, or when applying it
/// fails (in which case the rollback has already run).
fn commit(path: &Path, previous: &NetworkConfig, next: &NetworkConfig) -> Result<bool, String> {
    state::save(path, next).map_err(|e| format!("persist .network.json: {e}"))?;
    match apply::apply(next) {
        Ok(applied) => Ok(applied),
        Err(failure) => {
            let _restored = state::save(path, previous);
            let _reapplied = apply::apply(previous);
            Err(format!("network apply failed (rolled back): {failure}"))
        }
    }
}

/// Turn a `commit` outcome into the handler's reply, with `payload` merged in.
fn reply_for(outcome: Result<bool, String>, payload: serde_json::Value) -> HttpReply {
    match outcome {
        Ok(applied) => {
            let mut body = payload;
            if let Some(object) = body.as_object_mut() {
                drop(object.insert("applied".to_owned(), serde_json::Value::Bool(applied)));
            }
            HttpReply::ok(&body)
        }
        Err(failure) => {
            eprintln!("network: {failure}");
            HttpReply::error(502, "network settings rolled back — the box is unchanged")
        }
    }
}

/// `GET /api/it/network` — the current configuration (secrets elided) plus the
/// live status the cockpit polls.
///
/// Never fails on a missing state file or an absent tool: the config falls back
/// to defaults (fail-closed — `wan`, AP off) and every status field degrades to
/// `null`, so a dev machine with no gates set answers `200` with a fully-null
/// status (O3.5).
pub(crate) fn get_network(state: &Mutex<Backend>) -> HttpReply {
    let path = match config_path(state) {
        Ok(path) => path,
        Err(reply) => return reply,
    };
    let config = state::load(&path);
    HttpReply::ok(&serde_json::json!({
        "config": config.redacted(),
        "status": status::probe(&config),
    }))
}

/// `POST /api/it/network/mode` — select `wan`, `wan_5g` or `5g`.
pub(crate) fn set_mode(state: &Mutex<Backend>, body: &[u8]) -> HttpReply {
    /// Request body: the mode alone.
    #[derive(Deserialize)]
    struct Req {
        /// Target uplink mode.
        mode: UplinkMode,
    }
    let Ok(req) = serde_json::from_slice::<Req>(body) else {
        return HttpReply::error(400, "expected {\"mode\":\"wan\"|\"wan_5g\"|\"5g\"}");
    };
    let path = match config_path(state) {
        Ok(path) => path,
        Err(reply) => return reply,
    };
    let previous = state::load(&path);
    let mut next = previous.clone();
    next.mode = req.mode;
    reply_for(commit(&path, &previous, &next), serde_json::json!({ "mode": next.mode }))
}

/// `POST /api/it/network/ap` — the access-point settings.
///
/// `passphrase` is write-only (see [`double_option`]): omit it to keep the
/// current PSK, send `null` to clear it, send a string to replace it.
pub(crate) fn set_ap(state: &Mutex<Backend>, body: &[u8]) -> HttpReply {
    /// Request body: the full AP form, with a write-only passphrase.
    #[derive(Deserialize)]
    struct Req {
        /// Whether the AP should be running.
        enabled: bool,
        /// Broadcast network name.
        ssid: String,
        /// WPA2 PSK — absent keeps, `null` clears, a string replaces.
        #[serde(default, deserialize_with = "double_option")]
        passphrase: Option<Option<String>>,
        /// 2.4 (`bg`) or 5 GHz (`a`).
        band: Band,
        /// Channel number, `0` for automatic.
        channel: u16,
        /// ISO-3166 regulatory country code.
        country: String,
        /// Suppress the SSID from beacons.
        hidden: bool,
        /// NAT + DHCP + DNS the active uplink to AP clients.
        share_internet: bool,
    }
    let Ok(req) = serde_json::from_slice::<Req>(body) else {
        return HttpReply::error(400, "invalid access-point body");
    };
    let path = match config_path(state) {
        Ok(path) => path,
        Err(reply) => return reply,
    };
    let previous = state::load(&path);
    let access_point = ApConfig {
        enabled: req.enabled,
        ssid: req.ssid.trim().to_owned(),
        passphrase: req.passphrase.unwrap_or_else(|| previous.ap.passphrase.clone()),
        band: req.band,
        channel: req.channel,
        country: req.country.trim().to_uppercase(),
        hidden: req.hidden,
        share_internet: req.share_internet,
    };
    if let Err(reason) = state::validate_ap(&access_point) {
        return HttpReply::error(400, &reason);
    }
    let mut next = previous.clone();
    next.ap = access_point;
    reply_for(commit(&path, &previous, &next), serde_json::json!({ "ap": next.redacted_ap() }))
}

/// `POST /api/it/network/wwan` — the 5G bearer settings (FR-NET-15).
///
/// `password` and `pin` are write-only, with the same absent/`null`/string
/// semantics as the AP passphrase.
pub(crate) fn set_wwan(state: &Mutex<Backend>, body: &[u8]) -> HttpReply {
    /// Request body: the bearer form, with write-only credentials.
    #[derive(Deserialize)]
    struct Req {
        /// Carrier APN.
        apn: String,
        /// PAP/CHAP username — absent keeps, `null` clears.
        #[serde(default, deserialize_with = "double_option")]
        username: Option<Option<String>>,
        /// PAP/CHAP password — absent keeps, `null` clears.
        #[serde(default, deserialize_with = "double_option")]
        password: Option<Option<String>>,
        /// SIM PIN — absent keeps, `null` clears.
        #[serde(default, deserialize_with = "double_option")]
        pin: Option<Option<String>>,
        /// Allow attaching to a roaming network.
        roaming: bool,
        /// Standby policy while ethernet is the active uplink.
        standby: Standby,
    }
    let Ok(req) = serde_json::from_slice::<Req>(body) else {
        return HttpReply::error(400, "invalid wwan body");
    };
    let path = match config_path(state) {
        Ok(path) => path,
        Err(reply) => return reply,
    };
    let previous = state::load(&path);
    let wwan = WwanConfig {
        apn: req.apn.trim().to_owned(),
        username: req.username.unwrap_or_else(|| previous.wwan.username.clone()),
        password: req.password.unwrap_or_else(|| previous.wwan.password.clone()),
        pin: req.pin.unwrap_or_else(|| previous.wwan.pin.clone()),
        roaming: req.roaming,
        standby: req.standby,
    };
    if let Err(reason) = state::validate_wwan(&wwan) {
        return HttpReply::error(400, &reason);
    }
    let mut next = previous.clone();
    next.wwan = wwan;
    reply_for(commit(&path, &previous, &next), serde_json::json!({ "wwan": next.redacted_wwan() }))
}

/// Re-apply the persisted network configuration at boot, mirroring
/// [`apply_caddy_at_boot`](super::identity::apply_caddy_at_boot).
///
/// Write-and-apply, never fails startup: a box whose modem is missing, whose SIM
/// is absent or whose radio is rfkilled must still boot into a reachable
/// cockpit. A failure here is a journal line, not a dead appliance (NFR-NET-06).
pub(crate) fn apply_network_at_boot(state: &Mutex<Backend>) {
    let Ok(path) = config_path(state) else {
        return;
    };
    let config = state::load(&path);
    match apply::apply(&config) {
        Ok(true) => eprintln!("network: applied at boot (mode={})", config.mode.as_str()),
        Ok(false) => {} // no gates set in this environment — skipped cleanly.
        Err(failure) => eprintln!("WARN: network boot apply failed: {failure}"),
    }
}

#[cfg(test)]
mod tests;
