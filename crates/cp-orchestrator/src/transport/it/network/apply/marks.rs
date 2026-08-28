//! Per-step applied marks — the idempotence + correct-rollback machinery for
//! [`apply`](super::apply).
//!
//! Each apply step records a hash of **exactly the inputs it reads** the instant
//! it succeeds, so a later step's failure leaves the marks file an accurate
//! statement of what actually ran — which is what makes `commit`'s rollback
//! re-run precisely the steps that moved and skip the ones that never did.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::super::state::NetworkConfig;
use super::super::uplink;

/// Marker key for `reconcile_wwan`.
pub(in crate::transport::it::network) const STEP_WWAN: &str = "wwan";
/// Marker key for `reconcile_ap`.
pub(in crate::transport::it::network) const STEP_AP: &str = "ap";
/// Marker key for `apply_ap_activation`.
pub(in crate::transport::it::network) const STEP_AP_ACTIVATION: &str = "ap_activation";
/// Marker key for `apply_mode`.
pub(in crate::transport::it::network) const STEP_MODE: &str = "mode";
/// Marker key for `write_uplink_env`.
pub(in crate::transport::it::network) const STEP_UPLINK_ENV: &str = "uplink_env";

/// Where the marks live. `/run` by default, which is **cleared at every boot**
/// — so `apply_network_at_boot` always reconciles for real, and only same-boot
/// repeats are skipped. That is intended: only the backend writes system network
/// config, so a human's `nmcli` edit is reverted at the next apply or boot.
pub(super) fn applied_marker() -> PathBuf {
    std::env::var_os("CP_NETWORK_APPLIED").map_or_else(|| PathBuf::from("/run/cp-network-applied"), PathBuf::from)
}

/// Hex SHA-256 of anything serialisable — secrets included, so a PSK change with
/// every other field identical still reconciles.
fn hash_of<T>(inputs: &T) -> String
where
    T: Serialize,
{
    let raw = serde_json::to_vec(inputs).unwrap_or_default();
    super::super::super::crypto::sha256(&raw).iter().fold(String::with_capacity(64), |mut acc, byte| {
        let _w = write!(acc, "{byte:02x}");
        acc
    })
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
pub(in crate::transport::it::network) struct StepHashes {
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
    pub(in crate::transport::it::network) fn of(config: &NetworkConfig) -> Self {
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
pub(in crate::transport::it::network) struct Marks {
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
    pub(in crate::transport::it::network) fn load(path: &Path) -> Self {
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
    pub(in crate::transport::it::network) fn unchanged(&self, step: &str, hash: &str) -> bool {
        self.entries.get(step).is_some_and(|stored| stored == hash)
    }

    /// Record `step` as done, and flush **now**: the whole value of this file is
    /// that it is accurate at the instant a *later* step fails.
    fn record(&mut self, step: &str, hash: String) {
        drop(self.entries.insert(step.to_owned(), hash));
        let body = self.entries.iter().fold(String::new(), |mut acc, (name, digest)| {
            let _w = writeln!(acc, "{name}={digest}");
            acc
        });
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
pub(in crate::transport::it::network) fn step<F>(
    marks: &mut Marks,
    name: &str,
    hash: String,
    action: F,
) -> Result<(), String>
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
