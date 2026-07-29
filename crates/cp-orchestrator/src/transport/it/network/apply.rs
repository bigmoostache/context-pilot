//! State → system configuration. The **only** writer of system network config.
//!
//! # The env gate (NFR-NET-04)
//!
//! Every tool this module drives is named by an environment variable, exactly as
//! Caddy is named by `CP_CADDYFILE`:
//!
//! | Variable | What it names |
//! |---|---|
//! | `CP_NMCLI_BIN` | `nmcli` — the `cp-wwan` and `cp-ap` profiles |
//! | `CP_IW_BIN` | `iw` — the regulatory domain |
//! | `CP_NETWORKCTL_BIN` | `networkctl` — reload/reconfigure after the drop-in |
//! | `CP_SYSTEMCTL_BIN` | `systemctl` — restart the failover supervisor |
//! | `CP_NETWORKD_DIR` | where the strict-`5g` drop-in is written |
//! | `CP_UPLINK_ENV` | `/etc/default/cp-uplink`, the supervisor's config |
//! | `CP_WAN_IFACE` / `CP_AP_IFACE` / `CP_WWAN_DEV` | hardware names, overridable |
//! | `CP_NETWORK_APPLIED` | where the applied-fingerprint marker lives |
//!
//! With `CP_NMCLI_BIN` unset the applier is a **no-op that reports `Ok(false)`**:
//! the backend persists the document and performs no system call. That is what
//! lets the whole feature be developed, unit-tested and reviewed off-hardware,
//! and it is why `cargo test` on a laptop never touches the laptop's network.
//! Each remaining gate degrades on its own, so a half-configured environment
//! skips one step rather than failing the call.
//!
//! # Ordering
//!
//! The steps are ordered by what breaks if they are not:
//!
//! 1. **Regulatory domain first.** An AP brought up under the world default `00`
//!    lands on a `no IR` channel and never beacons (landmine 1).
//! 2. **Profiles**, then **activation**, then **routes** — so a half-configured
//!    uplink is never the default route.
//! 3. **The supervisor's config last**, once the world it supervises is real.
//!
//! The Caddy site list is the one arrow this module does *not* own: it is driven
//! from [`commit`](super::commit) **before** the apply, so `10.42.0.1` is already
//! a served name by the time the first AP client can associate (landmine 11).

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use super::state::NetworkConfig;
use super::{profiles, routes};

/// The AP's own address — the gateway AP clients get from NetworkManager's
/// dnsmasq, and (landmine 11) a name Caddy must serve for the cockpit to be
/// reachable over HTTPS from the AP subnet.
pub(crate) const AP_ADDRESS: &str = "10.42.0.1";

/// The 5G bearer profile. Fixed, not derived from state: it is the handle the
/// supervisor, the applier and a human debugging with `nmcli` all share, and a
/// renamed profile would orphan whatever the previous one left behind.
pub(crate) const WWAN_PROFILE: &str = "cp-wwan";
/// The access-point profile.
pub(crate) const AP_PROFILE: &str = "cp-ap";

/// Route metric for the bearer when it is the chosen uplink — below `end0`'s 100.
pub(crate) const METRIC_PREFERRED: u32 = 50;
/// Route metric while the bearer merely stands by — above `end0`'s 100.
pub(crate) const METRIC_STANDBY: u32 = 700;

/// The ethernet uplink port. `end0` on the Photonicat 2; the drop-in path in
/// [`routes`] is derived from it, so an override must match reality.
pub(crate) fn wan_iface() -> String {
    std::env::var("CP_WAN_IFACE").unwrap_or_else(|_unset| "end0".to_owned())
}

/// The AP radio — `wlp1s0`, the ath11k (Wi-Fi 6, 6 GHz-capable, 23–30 dBm).
/// `wlan0` (aic8800) is deliberately left free for a future Wi-Fi client
/// uplink; do not consume it (landmine 7).
pub(crate) fn ap_device() -> String {
    std::env::var("CP_AP_IFACE").unwrap_or_else(|_unset| "wlp1s0".to_owned())
}

/// The modem's NetworkManager device: the QMI **control** port `cdc-wdm0`, not
/// the net port. NM drives the modem through ModemManager on the control port
/// and applies the resulting IP config to `wwu1u1i4` itself, which is not an NM
/// device at all (M0 correction to §5).
pub(crate) fn wwan_device() -> String {
    std::env::var("CP_WWAN_DEV").unwrap_or_else(|_unset| "cdc-wdm0".to_owned())
}

/// The resolved tool paths for this environment.
///
/// Built once per apply so a half-set environment degrades per-tool instead of
/// failing the whole call.
pub(crate) struct Tools {
    /// `nmcli`, and with it the whole applier: unset ⇒ the applier is inert.
    pub(crate) nmcli: OsString,
    /// `iw`, for the regulatory domain. Absent ⇒ the country is not pushed.
    pub(crate) iw: Option<OsString>,
    /// `networkctl`, to make the strict-`5g` drop-in take effect.
    pub(crate) networkctl: Option<OsString>,
    /// `systemctl`, to restart the failover supervisor after a config change.
    pub(crate) systemctl: Option<OsString>,
    /// Directory holding the `end0` `.network` drop-in for strict `5g`.
    pub(crate) networkd_dir: Option<PathBuf>,
    /// `/etc/default/cp-uplink` — the failover supervisor's configuration.
    pub(crate) uplink_env: Option<PathBuf>,
}

impl Tools {
    /// Resolve the gates, or `None` when this environment has no `nmcli` —
    /// i.e. local dev and every unit test.
    pub(crate) fn resolve() -> Option<Self> {
        Some(Self {
            nmcli: std::env::var_os("CP_NMCLI_BIN")?,
            iw: std::env::var_os("CP_IW_BIN"),
            networkctl: std::env::var_os("CP_NETWORKCTL_BIN"),
            systemctl: std::env::var_os("CP_SYSTEMCTL_BIN"),
            networkd_dir: std::env::var_os("CP_NETWORKD_DIR").map(PathBuf::from),
            uplink_env: std::env::var_os("CP_UPLINK_ENV").map(PathBuf::from),
        })
    }
}

/// Apply `config` to the system.
///
/// Returns `Ok(true)` when the system was touched, `Ok(false)` when this
/// environment has no `nmcli` gate set and the call was skipped cleanly.
///
/// # Errors
///
/// Returns a message describing the first step that failed. The caller
/// ([`commit`](super::commit)) then rolls the document **and** the system back,
/// so a bad setting can never wedge the box (NFR-NET-05).
pub(crate) fn apply(config: &NetworkConfig) -> Result<bool, String> {
    let Some(tools) = Tools::resolve() else {
        return Ok(false); // no gate in this environment — persistence only.
    };
    // Cheap and always safe, so it runs even when the profiles are up to date:
    // a radio that came up after the last call would otherwise keep the world
    // default and refuse to beacon.
    apply_regdom(&tools, config);

    if fingerprint_matches(config) {
        // Identical state ⇒ no `nmcli` mutation. Without this, an unrelated
        // `POST …/mode` would rewrite `cp-ap` and bounce every associated
        // client for nothing.
        write_uplink_env(&tools, config)?;
        return Ok(true);
    }

    profiles::reconcile_wwan(&tools.nmcli, config)?;
    profiles::reconcile_ap(&tools.nmcli, config)?;
    routes::apply_ap_activation(&tools, config)?;
    routes::apply_mode(&tools, config)?;
    write_uplink_env(&tools, config)?;
    record_fingerprint(config);
    Ok(true)
}

/// Push the regulatory country code before any radio comes up.
///
/// Best-effort and never fatal: without a country the AP simply cannot be
/// enabled (the state layer refuses it, FR-NET-14), so there is nothing here
/// worth rolling an apply back over. `iw reg set` is issued even when the global
/// domain already matches — it is a cheap netlink hint, and a self-managed phy
/// that came up since the last call would otherwise never receive it
/// (landmine 12).
fn apply_regdom(tools: &Tools, config: &NetworkConfig) {
    let (Some(iw_bin), false) = (tools.iw.as_ref(), config.ap.country.is_empty()) else {
        return;
    };
    if let Err(failure) = run(iw_bin, &["reg".to_owned(), "set".to_owned(), config.ap.country.clone()]) {
        eprintln!("network: iw reg set {} failed (non-fatal): {failure}", config.ap.country);
    }
}

/// Render `/etc/default/cp-uplink` and restart the supervisor when it changed.
///
/// Only on change (O5.3): the supervisor is the thing that restores
/// connectivity, and bouncing it on every unrelated save would drop its
/// hysteresis state and re-arm the cooldown for nothing.
///
/// # Errors
///
/// Returns a message when the file cannot be written.
fn write_uplink_env(tools: &Tools, config: &NetworkConfig) -> Result<(), String> {
    let Some(path) = tools.uplink_env.as_ref() else {
        return Ok(());
    };
    let body = render_uplink_env(config);
    if std::fs::read_to_string(path).is_ok_and(|current| current == body) {
        return Ok(());
    }
    std::fs::write(path, &body).map_err(|e| format!("write {}: {e}", path.display()))?;
    if let Some(systemctl) = tools.systemctl.as_ref() {
        let args = ["restart".to_owned(), "cp-uplink.service".to_owned()];
        if let Err(failure) = run(systemctl, &args) {
            eprintln!("network: could not restart cp-uplink (non-fatal): {failure}");
        }
    }
    Ok(())
}

/// The supervisor's environment file — a plain `KEY=value` list sourced by the
/// unit, in the same spirit as `/etc/default/pcat-ula`.
pub(crate) fn render_uplink_env(config: &NetworkConfig) -> String {
    let mut out = String::from("# Generated by the Context Pilot orchestrator — do not edit by hand.\n");
    out.push_str(&format!("CP_UPLINK_MODE={}\n", config.mode.as_str()));
    out.push_str(&format!("CP_UPLINK_WAN_IF={}\n", wan_iface()));
    out.push_str(&format!("CP_UPLINK_WWAN_PROFILE={WWAN_PROFILE}\n"));
    out.push_str(&format!("CP_UPLINK_WWAN_DEV={}\n", wwan_device()));
    out.push_str(&format!("CP_UPLINK_STANDBY={}\n", config.wwan.standby.as_str()));
    out.push_str(&format!("CP_UPLINK_TARGETS=\"{}\"\n", config.probe.targets.join(" ")));
    out.push_str(&format!("CP_UPLINK_FAIL_THRESHOLD={}\n", config.probe.fail_threshold));
    out.push_str(&format!("CP_UPLINK_OK_THRESHOLD={}\n", config.probe.ok_threshold));
    out.push_str(&format!("CP_UPLINK_INTERVAL_S={}\n", config.probe.interval_s));
    out.push_str(&format!("CP_UPLINK_METRIC_PREFERRED={METRIC_PREFERRED}\n"));
    out.push_str(&format!("CP_UPLINK_METRIC_STANDBY={METRIC_STANDBY}\n"));
    out
}

// ── Applied-state fingerprint ───────────────────────────────────────────────

/// Where the marker lives. `/run` by default, which is **cleared at every
/// boot** — so `apply_network_at_boot` always reconciles for real, and only
/// same-boot repeats are skipped. That is exactly the behaviour landmine 9
/// documents: a human's `nmcli` edit is reverted at the next apply or boot.
fn applied_marker() -> PathBuf {
    std::env::var_os("CP_NETWORK_APPLIED").map_or_else(|| PathBuf::from("/run/cp-network-applied"), PathBuf::from)
}

/// Hex SHA-256 of the config as serialised — secrets included, so a PSK change
/// with every other field identical still reconciles.
fn fingerprint(config: &NetworkConfig) -> String {
    let raw = serde_json::to_vec(config).unwrap_or_default();
    super::super::crypto::sha256(&raw).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Whether this exact config has already been applied during this boot.
fn fingerprint_matches(config: &NetworkConfig) -> bool {
    std::fs::read_to_string(applied_marker()).is_ok_and(|stored| stored.trim() == fingerprint(config))
}

/// Record the config as applied. Best-effort: a marker we fail to write only
/// costs a redundant reconcile next time.
fn record_fingerprint(config: &NetworkConfig) {
    let _written = std::fs::write(applied_marker(), fingerprint(config));
}

/// Run a tool and return its stdout, or the trimmed stderr as an error.
///
/// Secrets never reach a log line through this path: the error carries the
/// tool's own stderr, and the argv — which is where the PSK, the bearer password
/// and the SIM PIN travel — is deliberately **never** logged (O3.1).
///
/// # Errors
///
/// Returns a message when the tool cannot be spawned or exits non-zero.
pub(crate) fn run(bin: &OsStr, args: &[String]) -> Result<String, String> {
    let output = std::process::Command::new(bin)
        .args(args)
        .output()
        .map_err(|e| format!("spawn {}: {e}", bin.to_string_lossy()))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

/// The Caddy site addresses this configuration needs beyond the box's own.
///
/// Landmine 11: Caddy listens on the wildcard `*:443` but the generated
/// Caddyfile enumerates **explicit** site addresses, so `10.42.0.1` gets a TLS
/// `internal error` until it is one of them — measured in M0/O0.2, where the AP
/// subnet reached the cockpit over plain HTTP but not over HTTPS. FR-NET-09's
/// "the AP is a cul-de-sac whose only reachable service is the cockpit" is not
/// satisfied by the network applier alone.
pub(crate) fn caddy_subjects(config: &NetworkConfig) -> Vec<String> {
    if config.ap.enabled { vec![AP_ADDRESS.to_owned()] } else { Vec::new() }
}
