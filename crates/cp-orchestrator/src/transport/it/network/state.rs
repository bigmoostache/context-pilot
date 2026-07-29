//! The persisted network document — `.network.json` (design-network-uplink §6).
//!
//! One owner, one applier. Ansible **seeds** this file write-once at
//! provisioning time; from then on the cockpit's `can_manage_it` routes are the
//! only thing that edits it, and the backend applier is the only thing that
//! turns it into system configuration. That is what makes "Ansible configures it
//! *and* the UI configures it" safe: a re-run of `site.yml` finds the file
//! present and leaves it alone, so the client's choices survive (FR-NET-12).
//!
//! It lives beside `.identity.json` and `.provisioned` in the agents dir, mode
//! `0600` — it holds the Wi-Fi PSK and the SIM PIN. Those two, plus the WWAN
//! password, are **never** returned by a read path: [`NetworkConfig::redacted`]
//! replaces each with a `*_set` boolean (FR-NET-13).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::super::state::write_atomic;

/// Which uplink the box routes through (design §7).
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub(crate) enum UplinkMode {
    /// Ethernet only; the modem is not connected.
    #[default]
    #[serde(rename = "wan")]
    Wan,
    /// Ethernet preferred, 5G takes over when the ethernet path stops carrying
    /// traffic and hands back when it recovers.
    #[serde(rename = "wan_5g")]
    WanThen5g,
    /// 5G only — the ethernet default route is suppressed even with a cable in.
    #[serde(rename = "5g")]
    FiveG,
}

impl UplinkMode {
    /// The wire spelling, for log lines and for `/etc/default/cp-uplink`.
    /// Written by hand rather than derived through `Debug` so the journal and
    /// the supervisor's env file agree with the JSON, character for character.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Wan => "wan",
            Self::WanThen5g => "wan_5g",
            Self::FiveG => "5g",
        }
    }
}

/// How much of the 5G stack stays warm while ethernet is the active uplink.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub(crate) enum Standby {
    /// Bearer connected at a standby metric — failover is a metric flip
    /// (sub-second), at the cost of keeping the SIM attached (landmine 8).
    #[default]
    #[serde(rename = "hot")]
    Hot,
    /// Registered but no bearer — failover pays the bearer setup. For metered SIMs.
    #[serde(rename = "cold")]
    Cold,
}

/// Wi-Fi band for the access point.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub(crate) enum Band {
    /// 2.4 GHz (`nmcli` spells it `bg`).
    #[serde(rename = "bg")]
    Bg,
    /// 5 GHz (`nmcli` spells it `a`).
    #[default]
    #[serde(rename = "a")]
    A,
}

/// The 5G bearer settings.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub(crate) struct WwanConfig {
    /// Access point name for the carrier (empty = let ModemManager pick).
    pub(crate) apn: String,
    /// PAP/CHAP username, when the carrier needs one.
    pub(crate) username: Option<String>,
    /// PAP/CHAP password — secret, never read back.
    pub(crate) password: Option<String>,
    /// SIM PIN — secret, never read back. `None` for a SIM with no PIN lock,
    /// which is the case on the test box (M0 closed landmine 4 on this point).
    pub(crate) pin: Option<String>,
    /// Whether the bearer may attach to a roaming network.
    pub(crate) roaming: bool,
    /// Standby policy while ethernet is the active uplink.
    pub(crate) standby: Standby,
}

/// The Wi-Fi access point settings.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ApConfig {
    /// Whether the AP should be running.
    pub(crate) enabled: bool,
    /// Broadcast network name, 1–32 bytes.
    pub(crate) ssid: String,
    /// WPA2 pre-shared key, 8–63 characters — secret, never read back.
    pub(crate) passphrase: Option<String>,
    /// 2.4 or 5 GHz.
    pub(crate) band: Band,
    /// Channel number, or `0` for automatic selection.
    pub(crate) channel: u16,
    /// ISO-3166 regulatory country code. Without one, every 5 GHz channel is
    /// `no IR` and the AP cannot start at all (FR-NET-14, landmine 1).
    pub(crate) country: String,
    /// Suppress the SSID from beacons.
    pub(crate) hidden: bool,
    /// NAT + DHCP + DNS the active uplink to AP clients. Off leaves the AP a
    /// cul-de-sac whose only reachable service is the cockpit (FR-NET-09).
    pub(crate) share_internet: bool,
}

impl Default for ApConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            ssid: "ContextPilot".to_owned(),
            passphrase: None,
            band: Band::A,
            channel: 0,
            country: String::new(),
            hidden: false,
            share_internet: true,
        }
    }
}

/// Failover probe tuning (design §8).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ProbeConfig {
    /// Addresses probed through the WAN interface; any reply counts as up.
    pub(crate) targets: Vec<String>,
    /// Consecutive failures before the WAN is demoted.
    pub(crate) fail_threshold: u32,
    /// Consecutive successes before the WAN is restored.
    pub(crate) ok_threshold: u32,
    /// Seconds between probe rounds.
    pub(crate) interval_s: u32,
}

impl Default for ProbeConfig {
    fn default() -> Self {
        Self {
            targets: vec!["1.1.1.1".to_owned(), "9.9.9.9".to_owned()],
            fail_threshold: 3,
            ok_threshold: 2,
            interval_s: 10,
        }
    }
}

/// The whole `.network.json` document.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub(crate) struct NetworkConfig {
    /// Which uplink the box routes through.
    pub(crate) mode: UplinkMode,
    /// The 5G bearer.
    pub(crate) wwan: WwanConfig,
    /// The Wi-Fi access point.
    pub(crate) ap: ApConfig,
    /// Failover probe tuning.
    pub(crate) probe: ProbeConfig,
}

/// On-disk location of the network document, beside `.identity.json`.
pub(crate) fn network_path(agents_dir: &Path) -> PathBuf {
    agents_dir.join(".network.json")
}

/// Load the persisted config, falling back to defaults when the file is absent,
/// unreadable, malformed **or invalid**.
///
/// Fail-closed by construction, mirroring
/// [`load_identity`](super::super::identity::load_identity): this document is fed
/// straight into `nmcli` argument vectors, so a tampered file with a junk SSID or
/// a bogus country must never reach the applier. Falling back to defaults means
/// falling back to `mode: wan` with the AP off — the safest possible posture, and
/// one that keeps the cockpit reachable on the fleet ULA.
pub(crate) fn load(path: &Path) -> NetworkConfig {
    let Ok(raw) = std::fs::read(path) else {
        return NetworkConfig::default();
    };
    let Ok(config) = serde_json::from_slice::<NetworkConfig>(&raw) else {
        return NetworkConfig::default();
    };
    if validate(&config).is_err() { NetworkConfig::default() } else { config }
}

/// Persist the config atomically + durably, then tighten it to `0600` — it holds
/// the Wi-Fi PSK and the SIM PIN.
///
/// # Errors
///
/// Returns the underlying I/O error if the file cannot be written or chmod'd.
pub(crate) fn save(path: &Path, config: &NetworkConfig) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(config).unwrap_or_default();
    write_atomic(path, &json)?;
    restrict(path)
}

/// `chmod 0600`. Split out so the permission intent is one named thing rather
/// than a magic constant buried in [`save`].
#[cfg(unix)]
fn restrict(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

/// No-op on non-Unix (local dev on a platform without POSIX modes).
#[cfg(not(unix))]
fn restrict(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

// ── Validation ──────────────────────────────────────────────────────────────

/// Validate the whole document, returning a caller-facing message on the first
/// problem. The message is what the `400` body says, so it names the field.
///
/// # Errors
///
/// Returns a human-readable reason when any field is out of range.
pub(crate) fn validate(config: &NetworkConfig) -> Result<(), String> {
    validate_ap(&config.ap)?;
    validate_wwan(&config.wwan)?;
    validate_probe(&config.probe)
}

/// Validate the AP half. The country check is deliberately conditional on
/// `enabled`: a box that has never been given a country must still be able to
/// persist an SSID, but must not be able to turn the radio on (FR-NET-14).
///
/// # Errors
///
/// Returns a human-readable reason when a field is out of range.
pub(crate) fn validate_ap(access_point: &ApConfig) -> Result<(), String> {
    if !(1..=32).contains(&access_point.ssid.as_bytes().len()) {
        return Err("ssid must be 1–32 bytes".to_owned());
    }
    if access_point.passphrase.as_ref().is_some_and(|psk| !(8..=63).contains(&psk.chars().count())) {
        return Err("passphrase must be 8–63 characters".to_owned());
    }
    if !access_point.country.is_empty() && !valid_country(&access_point.country) {
        return Err("country must be a two-letter ISO-3166 code".to_owned());
    }
    if !valid_channel(access_point.band, access_point.channel) {
        return Err("channel is not valid for the selected band (0 = automatic)".to_owned());
    }
    if access_point.enabled {
        enabled_ap_prerequisites(access_point)?;
    }
    Ok(())
}

/// The two things an AP needs before it can legally beacon: a regulatory country
/// (FR-NET-14 — without it every 5 GHz channel is `no IR`) and a passphrase (we
/// do not ship open networks).
fn enabled_ap_prerequisites(access_point: &ApConfig) -> Result<(), String> {
    if access_point.country.is_empty() {
        return Err("a regulatory country code is required before the access point can be enabled".to_owned());
    }
    if access_point.passphrase.is_none() {
        return Err("a passphrase is required before the access point can be enabled".to_owned());
    }
    Ok(())
}

/// Validate the WWAN half.
///
/// # Errors
///
/// Returns a human-readable reason when a field is out of range.
pub(crate) fn validate_wwan(wwan: &WwanConfig) -> Result<(), String> {
    if wwan.apn.chars().count() > 100 {
        return Err("apn is too long".to_owned());
    }
    if !wwan.apn.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_') {
        return Err("apn may only contain letters, digits, '.', '-' and '_'".to_owned());
    }
    if wwan.pin.as_ref().is_some_and(|pin| !valid_pin(pin)) {
        return Err("pin must be 4–8 digits".to_owned());
    }
    Ok(())
}

/// A SIM PIN is 4–8 decimal digits. Anything else would be rejected by the modem
/// and, worse, could burn one of the three unlock retries.
fn valid_pin(pin: &str) -> bool {
    (4..=8).contains(&pin.chars().count()) && pin.chars().all(|ch| ch.is_ascii_digit())
}

/// Validate the probe tuning. Zero thresholds would make the supervisor either
/// never fail over or fail over on the first dropped packet; a zero interval
/// would spin.
///
/// # Errors
///
/// Returns a human-readable reason when a field is out of range.
fn validate_probe(probe: &ProbeConfig) -> Result<(), String> {
    if probe.targets.is_empty() {
        return Err("at least one probe target is required".to_owned());
    }
    if probe.targets.iter().any(|target| target.parse::<std::net::IpAddr>().is_err()) {
        return Err("probe targets must be IP addresses".to_owned());
    }
    if probe.fail_threshold == 0 || probe.ok_threshold == 0 {
        return Err("probe thresholds must be at least 1".to_owned());
    }
    if !(1..=3600).contains(&probe.interval_s) {
        return Err("probe interval must be between 1 and 3600 seconds".to_owned());
    }
    Ok(())
}

/// A two-letter alphabetic country code (`FR`, `DE`, …).
fn valid_country(country: &str) -> bool {
    country.chars().count() == 2 && country.chars().all(|ch| ch.is_ascii_alphabetic())
}

/// Whether `channel` is legal on `band`. `0` always means "let the driver pick".
///
/// The 5 GHz list is the union of the UNII bands `nmcli` will accept; the
/// regulatory domain decides which of them are actually usable at run time, and
/// that is `cp-regdom`'s job, not this function's.
fn valid_channel(band: Band, channel: u16) -> bool {
    const FIVE_GHZ: &[u16] = &[
        36, 40, 44, 48, 52, 56, 60, 64, 100, 104, 108, 112, 116, 120, 124, 128, 132, 136, 140, 144, 149, 153, 157, 161,
        165,
    ];
    if channel == 0 {
        return true;
    }
    match band {
        Band::Bg => (1..=14).contains(&channel),
        Band::A => FIVE_GHZ.contains(&channel),
    }
}

// ── Secret elision (FR-NET-13) ──────────────────────────────────────────────

impl NetworkConfig {
    /// The read-path projection: the same document with every secret replaced by
    /// a `*_set` boolean.
    ///
    /// Built as a fresh `Value` rather than by mutating a serialisation of
    /// `self`, so a field added to the struct later is **absent** from the read
    /// path until someone deliberately adds it here. The failure mode of the
    /// other direction — forgetting to strip a newly-added secret — is a leak.
    pub(crate) fn redacted(&self) -> serde_json::Value {
        serde_json::json!({
            "mode": self.mode,
            "wwan": self.redacted_wwan(),
            "ap": self.redacted_ap(),
            "probe": {
                "targets": self.probe.targets,
                "fail_threshold": self.probe.fail_threshold,
                "ok_threshold": self.probe.ok_threshold,
                "interval_s": self.probe.interval_s,
            },
        })
    }

    /// The bearer half of [`redacted`](Self::redacted) — `password` and `pin`
    /// collapse to booleans.
    pub(crate) fn redacted_wwan(&self) -> serde_json::Value {
        serde_json::json!({
            "apn": self.wwan.apn,
            "username": self.wwan.username,
            "password_set": self.wwan.password.is_some(),
            "pin_set": self.wwan.pin.is_some(),
            "roaming": self.wwan.roaming,
            "standby": self.wwan.standby,
        })
    }

    /// The access-point half of [`redacted`](Self::redacted) — `passphrase`
    /// collapses to a boolean.
    pub(crate) fn redacted_ap(&self) -> serde_json::Value {
        serde_json::json!({
            "enabled": self.ap.enabled,
            "ssid": self.ap.ssid,
            "passphrase_set": self.ap.passphrase.is_some(),
            "band": self.ap.band,
            "channel": self.ap.channel,
            "country": self.ap.country,
            "hidden": self.ap.hidden,
            "share_internet": self.ap.share_internet,
        })
    }
}
