//! State → system configuration. The **only** writer of system network config.
//!
//! # The env gate
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
//! | `CP_NFT_BIN` | `nft` — drop NM's masquerade table for a cul-de-sac AP |
//! | `CP_REGDOM_BIN` | `/usr/local/sbin/cp-regdom` — the one implementation of "push the country" |
//! | `CP_NETWORKD_DIR` | where the strict-`5g` drop-in is written |
//! | `CP_UPLINK_ENV` | `/etc/default/cp-uplink` — what we tell the supervisor |
//! | `CP_UPLINK_STATE` | `/run/cp-uplink/state` — what the supervisor tells us |
//! | `CP_WAN_IFACE` / `CP_AP_IFACE` / `CP_WWAN_DEV` | hardware names, overridable |
//! | `CP_WWAN_PRESENT` | `0`/`1` — override the "does this box have a modem" probe |
//! | `CP_NETWORK_APPLIED` | where the per-step applied marker lives |
//!
//! The last two of those live in [`uplink`](super::uplink), which owns both ends
//! of the supervisor interface. Two more are read by [`status`](super::status)
//! on the same terms: `CP_MMCLI_BIN` (modem facts) and `CP_IP_BIN` (`end0`'s
//! address — `end0` is networkd's, so `nmcli` cannot answer for it).
//!
//! # What "inert" means, precisely
//!
//! [`Tools::resolve`] returns `None` — and [`apply`] then reports `Ok(false)`
//! having performed no system call — when `CP_NMCLI_BIN` is **unset or names a
//! path that does not exist**. Both halves matter: every provisioned box
//! gets the variable templated into its unit file whether or not `NetworkManager`
//! was ever installed, so gating on the variable alone made a box deployed with
//! `net_enabled=false` believe it was live and answer `502` to every network
//! POST. Gating on the binary is what makes "no `NetworkManager` here" and "not
//! configured for networking" the same, honest state.
//!
//! Off-box — a laptop, CI, every unit test — nothing sets the variable, which is
//! why `cargo test` never touches the machine's network. Each remaining gate
//! degrades on its own, so a half-configured environment skips one step rather
//! than failing the call.
//!
//! # Ordering
//!
//! The steps are ordered by what breaks if they are not:
//!
//! 1. **Regulatory domain first.** An AP brought up under the world default `00`
//!    lands on a `no IR` channel and never beacons.
//! 2. **Profiles**, then **activation**, then **routes** — so a half-configured
//!    uplink is never the default route.
//! 3. **The supervisor's config last**, once the world it supervises is real.
//!
//! The Caddy site list is the one arrow this module does *not* own: it is driven
//! from [`commit`](super::commit) **before** the apply, so `10.42.0.1` is already
//! a served name when the first AP client associates — or it meets a TLS error.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::state::{NetworkConfig, UplinkMode};
use super::{profiles, routes, uplink};

/// The AP's own address — the gateway AP clients get from `NetworkManager`'s
/// dnsmasq, and a name Caddy must serve for HTTPS to work from the AP subnet.
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
/// uplink; do not consume it.
pub(crate) fn ap_device() -> String {
    std::env::var("CP_AP_IFACE").unwrap_or_else(|_unset| "wlp1s0".to_owned())
}

/// The modem's `NetworkManager` device: the QMI **control** port `cdc-wdm0`, not
/// the net port. NM drives the modem through `ModemManager` on the control port
/// and applies the resulting IP config to `wwu1u1i4` itself, which is not an NM
/// device at all — MEASURED: `nmcli device status` lists `cdc-wdm0  gsm`.
pub(crate) fn wwan_device() -> String {
    std::env::var("CP_WWAN_DEV").unwrap_or_else(|_unset| "cdc-wdm0".to_owned())
}

/// Whether this box physically carries a 5G modem.
///
/// The Photonicat 2 ships in variants, and a box without the M.2 modem must not
/// be offered a 5G uplink at all: picking `5g` there suppresses the ethernet
/// default route with nothing to replace it. (Recoverable — no mode ever alters
/// an address on the ethernet ports, so the cockpit stays on the LAN address and
/// the fleet ULA — but it should never be reachable from the UI at all.)
///
/// Probed from **sysfs, not from `ModemManager`**. `mmcli` answering is a
/// statement about a daemon's current view; `/sys/class/usbmisc/cdc-wdm*` and
/// `/sys/class/net/ww*` are statements about hardware. Using the daemon would
/// make the whole 5G surface appear and disappear while `ModemManager` restarts.
///
/// * `CP_WWAN_PRESENT=0|1` overrides the probe outright — for a variant the
///   probe reads wrong, and for tests.
/// * With the applier inert — [`nmcli_bin`] answering `None`: local dev, every
///   unit test, a box with no `NetworkManager` — this reports `true`. Off-box
///   there is no hardware to protect, and the env gate already guarantees
///   nothing is applied.
pub(crate) fn modem_present() -> bool {
    if let Some(forced) = std::env::var_os("CP_WWAN_PRESENT") {
        return forced == "1";
    }
    if nmcli_bin().is_none() {
        return true;
    }
    // The QMI control port, then the QMI net port — either is enough.
    entry_starting_with("/sys/class/usbmisc", "cdc-wdm") || entry_starting_with("/sys/class/net", "ww")
}

/// `nmcli`'s path, **only if that path exists**.
///
/// The whole applier hangs off this one answer, so it is the one place the
/// distinction is made. Checking the file rather than the variable matters:
/// `context-pilot.service.j2` templates `CP_NMCLI_BIN` onto every box, including
/// one deployed with `net_enabled=false` where `NetworkManager` was never
/// installed. Believing the variable there meant every `nmcli` spawn failed,
/// every apply returned `Err`, and every network POST answered `502` forever.
fn nmcli_bin() -> Option<OsString> {
    let bin = std::env::var_os("CP_NMCLI_BIN")?;
    Path::new(&bin).exists().then_some(bin)
}

/// Whether `dir` holds an entry whose name starts with `prefix`.
fn entry_starting_with(dir: &str, prefix: &str) -> bool {
    std::fs::read_dir(dir)
        .is_ok_and(|entries| entries.flatten().any(|entry| entry.file_name().to_string_lossy().starts_with(prefix)))
}

/// The resolved tool paths for this environment.
///
/// Built once per apply so a half-set environment degrades per-tool instead of
/// failing the whole call.
pub(crate) struct Tools {
    /// `nmcli`, and with it the whole applier: unset ⇒ the applier is inert.
    pub nmcli: OsString,
    /// `iw`, for the regulatory domain. Absent ⇒ the country is not pushed.
    pub iw: Option<OsString>,
    /// `networkctl`, to make the strict-`5g` drop-in take effect.
    pub networkctl: Option<OsString>,
    /// `systemctl`, to restart the failover supervisor after a config change.
    pub systemctl: Option<OsString>,
    /// `nft`, to drop `NetworkManager`'s masquerade table when the AP is a
    /// cul-de-sac — internet sharing off, cockpit access only.
    pub nft: Option<OsString>,
    /// `cp-regdom`, the appliance's single implementation of "push the
    /// regulatory country". Preferred over [`Self::iw`] when installed — see
    /// [`apply_regdom`].
    pub regdom: Option<OsString>,
    /// Directory holding the `end0` `.network` drop-in for strict `5g`.
    pub networkd_dir: Option<PathBuf>,
    /// `/etc/default/cp-uplink` — the failover supervisor's configuration.
    pub uplink_env: Option<PathBuf>,
}

impl Tools {
    /// Resolve the gates, or `None` when this environment has no usable `nmcli`
    /// — i.e. local dev, every unit test, and a box provisioned with
    /// `net_enabled=false` (see the module doc on what "inert" means).
    pub(crate) fn resolve() -> Option<Self> {
        Some(Self {
            nmcli: nmcli_bin()?,
            iw: std::env::var_os("CP_IW_BIN"),
            networkctl: std::env::var_os("CP_NETWORKCTL_BIN"),
            systemctl: std::env::var_os("CP_SYSTEMCTL_BIN"),
            nft: std::env::var_os("CP_NFT_BIN"),
            regdom: std::env::var_os("CP_REGDOM_BIN"),
            networkd_dir: std::env::var_os("CP_NETWORKD_DIR").map(PathBuf::from),
            uplink_env: std::env::var_os("CP_UPLINK_ENV").map(PathBuf::from),
        })
    }
}

/// Apply `config` to the system.
///
/// Returns `Ok(true)` when the applier was live for this call, `Ok(false)` when
/// this environment has no usable `nmcli` and the call was skipped cleanly.
/// Individual steps are still skipped when [`Marks`] says their inputs have not
/// moved since they last succeeded.
///
/// The mode may be **coerced** before anything runs — see [`effective_config`].
///
/// # Errors
///
/// Returns a message describing the first step that failed. The caller
/// ([`commit`](super::commit)) then rolls the document **and** the system back,
/// so a bad setting can never wedge the box.
pub(crate) fn apply(requested: &NetworkConfig) -> Result<bool, String> {
    let Some(tools) = Tools::resolve() else {
        return Ok(false); // no gate in this environment — persistence only.
    };
    let effective = effective_config(requested);
    let config = effective.as_ref();
    // Cheap and always safe, so it runs even when the profiles are up to date:
    // a radio that came up after the last call would otherwise keep the world
    // default and refuse to beacon.
    apply_regdom(&tools, config);

    let marker = applied_marker();
    let mut marks = Marks::load(&marker);
    let hashes = StepHashes::of(config);
    // No modem ⇒ no `cp-wwan` profile. Creating one bound to a device that does
    // not exist would leave a permanently-inactive connection for a human to
    // puzzle over on a box that simply is not a 5G variant.
    if modem_present() {
        step(&mut marks, STEP_WWAN, hashes.wwan, || profiles::reconcile_wwan(&tools.nmcli, config))?;
    }
    step(&mut marks, STEP_AP, hashes.access_point, || profiles::reconcile_ap(&tools.nmcli, config))?;
    step(&mut marks, STEP_AP_ACTIVATION, hashes.ap_activation, || routes::apply_ap_activation(&tools, config))?;
    step(&mut marks, STEP_MODE, hashes.mode, || routes::apply_mode(&tools, config))?;
    step(&mut marks, STEP_UPLINK_ENV, hashes.uplink_env, || uplink::write_uplink_env(&tools, config))?;
    Ok(true)
}

/// The configuration the applier will actually enforce, which is not always the
/// one that was asked for.
///
/// **"A box with no modem is never offered a 5G mode" belongs here, not in the
/// HTTP handlers.** The handlers' `400 "this box has no 5G modem"` is a better
/// answer for a human and it stays — but it is not the only way a `5g` document
/// reaches the system. `apply_network_at_boot` reads the file straight off disk
/// and `network.json.j2` templates `cp_net_mode` verbatim, so `-e net_mode=5g`
/// on a non-5G variant seeds a document that suppresses `end0`'s default route
/// at **every boot**, reachable from provisioning with nothing to object.
///
/// Coercing to `wan` rather than refusing the whole apply is the deliberate
/// choice: a refusal would also take the access point down and leave the box
/// with neither a route nor a Wi-Fi network. "Route over ethernet, keep
/// everything else" is the honest posture. Doing it *before* any mutation is
/// also what makes [`routes::apply_mode`] structurally unable to write the
/// strict-`5g` drop-in on a modem-less box: by the time it runs, the mode it
/// sees is already `wan`.
///
/// The document on disk is left untouched — the admin's stated intent survives,
/// and the box starts honouring it the moment a modem is fitted.
fn effective_config(requested: &NetworkConfig) -> Cow<'_, NetworkConfig> {
    coerce_mode(requested, modem_present())
}

/// Pure half of [`effective_config`] — the hardware fact is a parameter so the
/// coercion is testable without a modem, and without mutating the environment
/// (which this workspace forbids `unsafe` for).
pub(super) fn coerce_mode(requested: &NetworkConfig, has_modem: bool) -> Cow<'_, NetworkConfig> {
    if matches!(requested.mode, UplinkMode::Wan) || has_modem {
        return Cow::Borrowed(requested);
    }
    crate::oerr!(
        "WARN: network: mode `{}` requires a 5G modem and this box has none — applying `wan` instead, \
         routing over {}. The document is unchanged.",
        requested.mode.as_str(),
        wan_iface()
    );
    let mut coerced = requested.clone();
    coerced.mode = UplinkMode::Wan;
    Cow::Owned(coerced)
}

/// Push the regulatory country code before any radio comes up.
///
/// Best-effort and never fatal in either path: without a country the AP simply
/// cannot be enabled (the state layer refuses it), so there is nothing here
/// worth rolling an apply back over. It is issued even when the global domain
/// already matches — a cheap netlink hint, and a self-managed phy that came up
/// since the last call would otherwise never receive it.
///
/// `iw reg get` flags both radios `(self-managed)`, which normally reads as "the
/// driver ignores your country code". Do not re-derive that from the flag alone:
/// MEASURED, `iw reg set FR` took `phy0` (ath11k) from the world default `00` to
/// `FR: DFS-ETSI` and its **`no IR` channel count from 89 to 0** — channels
/// 36–48 from unusable to 23 dBm, channel 100 to 30 dBm with radar detection.
/// Without this call there is no 5 `GHz` AP at all.
///
/// # `cp-regdom` first, `iw` only as a fallback
///
/// `deploy/photonicat/network/cp-regdom.sh` says in its own header that the
/// applier calls it on every AP apply. It did not: it shelled out to
/// `iw reg set` itself, which made the script a **second implementation** of
/// this function — two places to fix the same hazard, and a script whose boot
/// path and whose apply path could silently diverge. With `CP_REGDOM_BIN` set,
/// the country goes through the one implementation, as a single argument.
///
/// The `iw` path stays because the script is not always there: off-box, in a
/// container, and on any box where `network.yml` has not installed
/// `/usr/local/sbin/cp-regdom` yet.
fn apply_regdom(tools: &Tools, config: &NetworkConfig) {
    if config.ap.country.is_empty() {
        return;
    }
    if let Some(regdom) = tools.regdom.as_ref() {
        if let Err(failure) = run(regdom, std::slice::from_ref(&config.ap.country)) {
            crate::oerr!("network: cp-regdom {} failed (non-fatal): {failure}", config.ap.country);
        }
        return;
    }
    let Some(iw_bin) = tools.iw.as_ref() else {
        return;
    };
    if let Err(failure) = run(iw_bin, &["reg".to_owned(), "set".to_owned(), config.ap.country.clone()]) {
        crate::oerr!("network: iw reg set {} failed (non-fatal): {failure}", config.ap.country);
    }
}

// ── Per-step applied marks ──────────────────────────────────────────────────

/// Marker key for `reconcile_wwan`.
pub(super) const STEP_WWAN: &str = "wwan";
/// Marker key for `reconcile_ap`.
pub(super) const STEP_AP: &str = "ap";
/// Marker key for `apply_ap_activation`.
pub(super) const STEP_AP_ACTIVATION: &str = "ap_activation";
/// Marker key for `apply_mode`.
pub(super) const STEP_MODE: &str = "mode";
/// Marker key for `write_uplink_env`.
pub(super) const STEP_UPLINK_ENV: &str = "uplink_env";

/// Where the marks live. `/run` by default, which is **cleared at every boot**
/// — so `apply_network_at_boot` always reconciles for real, and only same-boot
/// repeats are skipped. That is intended: only the backend writes system network
/// config, so a human's `nmcli` edit is reverted at the next apply or boot.
fn applied_marker() -> PathBuf {
    std::env::var_os("CP_NETWORK_APPLIED").map_or_else(|| PathBuf::from("/run/cp-network-applied"), PathBuf::from)
}

/// Hex SHA-256 of anything serialisable — secrets included, so a PSK change with
/// every other field identical still reconciles.
fn hash_of<T>(inputs: &T) -> String where T: Serialize {
    let raw = serde_json::to_vec(inputs).unwrap_or_default();
    super::super::crypto::sha256(&raw).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// One hash per apply step, over **exactly the inputs that step reads** — the
/// single place the answer to "what does this step depend on?" is written down.
///
/// # Why this is not one whole-document fingerprint
///
/// It used to be, and that was two bugs at once.
///
/// * Any mode change moved the whole-document hash, so `reconcile_ap` +
///   `nmcli connection up cp-ap` re-ran and **every associated Wi-Fi client was
///   dropped** — while the comment sitting next to it claimed the marker existed
///   to prevent exactly that.
/// * The serious one: the marker was written only after a *complete*
///   successful apply, so a partial failure left it holding the hash of the
///   PREVIOUS document. `commit`'s rollback then called `apply(previous)`, the
///   fingerprint matched, and the rollback performed **no system work at all**:
///   no `nmcli`, no drop-in, no sysctl. The guarantee that a failed apply leaves
///   the box as it was found did not hold, and the `502` was lying.
///
/// Recording each hash **immediately after its step succeeds** ([`Marks::record`])
/// makes the rollback correct by construction rather than by care: a step that
/// ran with `next` holds a hash that cannot match `previous`, so the rollback
/// re-runs it; a step that never ran still holds `previous`' hash, so skipping it
/// is right.
pub(super) struct StepHashes {
    /// `reconcile_wwan` — the bearer config **and** the mode, which is what
    /// drives the route metric and the autoconnect flag.
    pub wwan: String,
    /// `reconcile_ap` — the access-point config alone.
    pub access_point: String,
    /// `apply_ap_activation` — the two fields it actually reads.
    pub ap_activation: String,
    /// `apply_mode` — the mode **and** the standby policy, which together decide
    /// the drop-in *and* whether the bearer is brought up or down. Standby is in
    /// here deliberately: keying on the mode alone would silently stop honouring
    /// a `hot` → `cold` switch, which is a regression the whole-document
    /// fingerprint did not have.
    pub mode: String,
    /// `write_uplink_env` — the rendered file body itself, which is the most
    /// precise statement of that step's inputs available.
    pub uplink_env: String,
}

impl StepHashes {
    /// Hash every step's inputs for `config`.
    pub(super) fn of(config: &NetworkConfig) -> Self {
        Self {
            wwan: hash_of(&(config.mode, &config.wwan)),
            access_point: hash_of(&config.ap),
            ap_activation: hash_of(&(config.ap.enabled, config.ap.share_internet)),
            mode: hash_of(&(config.mode, config.wwan.standby)),
            uplink_env: hash_of(&uplink::render_uplink_env(config)),
        }
    }
}

/// What each apply step last succeeded with: a `step=hash` line per step.
pub(super) struct Marks {
    /// Where the lines are read from and flushed to.
    path: PathBuf,
    /// Step name → hash. `BTreeMap` for a stable file order, so a human diffing
    /// `/run/cp-network-applied` between two applies sees only what moved.
    entries: BTreeMap<String, String>,
}

impl Marks {
    /// Read the marks, tolerating an absent, truncated or hand-edited file —
    /// anything unparseable simply reads as "this step has never run", which
    /// costs a redundant reconcile and never a wrong skip.
    pub(super) fn load(path: &Path) -> Self {
        let mut entries = BTreeMap::new();
        if let Ok(body) = std::fs::read_to_string(path) {
            for line in body.lines() {
                if let Some((step, hash)) = line.split_once('=') {
                    drop(entries.insert(step.trim().to_owned(), hash.trim().to_owned()));
                }
            }
        }
        Self { path: path.to_path_buf(), entries }
    }

    /// Whether `step`'s inputs are exactly what it last succeeded with.
    pub(super) fn unchanged(&self, step: &str, hash: &str) -> bool {
        self.entries.get(step).is_some_and(|stored| stored == hash)
    }

    /// Record `step` as done, and flush **now**: the whole value of this file is
    /// that it is accurate at the instant a *later* step fails.
    fn record(&mut self, step: &str, hash: String) {
        drop(self.entries.insert(step.to_owned(), hash));
        let body: String = self.entries.iter().map(|(step, hash)| format!("{step}={hash}\n")).collect();
        // Best-effort: a mark we fail to write only costs a redundant reconcile.
        let _written = std::fs::write(&self.path, body);
    }
}

/// Run one apply step unless its inputs are unchanged, then record it.
///
/// # Errors
///
/// Propagates `action`'s failure untouched, leaving the step's mark as it was —
/// which is what tells the rollback this step must run again.
pub(super) fn step<F>(marks: &mut Marks, name: &str, hash: String, action: F) -> Result<(), String>
where
    F: FnOnce() -> Result<(), String>,
{
    if marks.unchanged(name, &hash) {
        return Ok(());
    }
    action()?;
    marks.record(name, hash);
    Ok(())
}

/// Run a tool and return its stdout, or the trimmed stderr as an error.
///
/// Secrets never reach a log line through this path: the error carries the
/// tool's own stderr, and the argv — which is where the PSK, the bearer password
/// and the SIM PIN travel — is deliberately **never** logged.
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
/// Caddy listens on the wildcard `*:443` but the generated Caddyfile enumerates
/// **explicit** site addresses, so `10.42.0.1` gets a TLS `internal error` until
/// it is one of them. MEASURED: a client associated to the AP reached the
/// cockpit over plain HTTP (`200`) and failed TLS over HTTPS. "The AP is a
/// cul-de-sac whose only reachable service is the cockpit" is therefore not
/// satisfied by the network applier alone — enabling it moves the site list too.
pub(crate) fn caddy_subjects(config: &NetworkConfig) -> Vec<String> {
    if config.ap.enabled { vec![AP_ADDRESS.to_owned()] } else { Vec::new() }
}
