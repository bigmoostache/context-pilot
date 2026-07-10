//! The on-box updater — fetch → verify → download → apply (update-policy §5.5).
//!
//! This module is the **security core** of the OTA feature. It turns a signed
//! channel manifest into an applied release, refusing at the first failed
//! check and never leaving the box unbootable:
//!
//! * [`verify`] — minisign signature over the exact manifest bytes (trust
//!   anchor: [`UPDATE_PUBKEY`](super::UPDATE_PUBKEY)), freshness
//!   (`expires_at`), anti-rollback (monotonic version + `min_from` floor).
//! * [`download`] — resolve the box's arch artifact, download into
//!   `releases/<tag>/`, verify the manifest-pinned `sha256` **before** any
//!   extraction.
//! * [`apply`] — back up `auth.db`, stage the new orchestrator over the
//!   install path (atomic rename + `.pending`/`.bak` markers), record the
//!   in-flight update, re-exec; the health-gated boot commit promotes
//!   (`active_tag`, agent binary) only after `/healthz` answers `200`, and a
//!   crash-looping binary rolls back with the database restored.
//! * [`state`] — durable `update-state.json` (last check / last result) the
//!   cockpit surfaces.

pub(crate) mod apply;
pub(crate) mod download;
pub(crate) mod state;
pub(crate) mod verify;

pub use apply::{boot_reconcile, promote_committed, restart_self, stage_apply};
pub use download::download_artifact;
pub use state::{UpdateResult, UpdateState};
pub use verify::{UpdateEvaluation, VerifyError, evaluate_manifest};

use std::path::Path;

use super::manifest::Manifest;

/// Raw-file base of the `channels` branch — where CI publishes the signed
/// per-channel manifests (update-policy §5.3).
fn channel_url(file: &str) -> String {
    format!("https://raw.githubusercontent.com/{}/channels/{file}", super::GITHUB_REPO)
}

/// Fetch the `stable` channel's manifest bytes + detached signature.
///
/// # Errors
///
/// Returns an error on network failure or a non-success HTTP status. No
/// verification happens here — callers hand the pair to [`evaluate_manifest`].
pub fn fetch_stable_manifest() -> Result<(Vec<u8>, String), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    let fetch = |file: &str| -> Result<Vec<u8>, String> {
        let url = channel_url(file);
        let resp = client
            .get(&url)
            .header("User-Agent", super::USER_AGENT)
            .send()
            .map_err(|e| format!("GET {url}: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("GET {url}: HTTP {}", resp.status()));
        }
        resp.bytes().map(|b| b.to_vec()).map_err(|e| format!("read {url}: {e}"))
    };
    let manifest = fetch("stable.json")?;
    let sig_bytes = fetch("stable.json.minisig")?;
    let sig = String::from_utf8(sig_bytes).map_err(|e| format!("signature is not UTF-8: {e}"))?;
    Ok((manifest, sig))
}

/// One full **check** (read-only): fetch the stable manifest, verify it, and
/// record the outcome in `update-state.json` under `releases_dir`.
///
/// A failed signature / freshness / anti-rollback check returns `Err` and the
/// last-known state is kept (`available` is cleared only on a *verified*
/// up-to-date answer, never on a fetch or verification failure).
///
/// # Errors
///
/// Returns an error string on fetch failure or any failed verification.
pub fn check_stable(releases_dir: &Path, current: &str) -> Result<UpdateEvaluation, String> {
    let mut st = UpdateState::load(releases_dir);
    st.last_check_ms = Some(state::now_ms());

    let outcome = fetch_stable_manifest().and_then(|(bytes, sig)| {
        evaluate_manifest(&bytes, &sig, current, state::now_epoch_secs()).map_err(|e| e.to_string())
    });
    match &outcome {
        Ok(UpdateEvaluation::Available(manifest)) => st.available = Some(manifest.version.clone()),
        Ok(UpdateEvaluation::UpToDate) => st.available = None,
        Err(_) => {} // keep last-known `available` — never regress on a bad fetch
    }
    st.save(releases_dir);
    outcome
}

/// Evaluate a fetched manifest and, **only** when it verifies as a newer
/// applicable version, run `download` on it. This is the single seam between
/// "checked" and "acting": a manifest that fails signature, freshness,
/// anti-rollback or schema checks returns `Err` here and the download hook is
/// provably never invoked (V3.1b).
///
/// Returns `Ok(Some(manifest))` when an update was verified + downloaded,
/// `Ok(None)` when the box is up to date.
///
/// # Errors
///
/// Any failed verification (as [`VerifyError`] text) or download failure.
pub fn check_and_prepare<D>(
    manifest_bytes: &[u8],
    signature: &str,
    current: &str,
    now_epoch_secs: u64,
    download: D,
) -> Result<Option<Manifest>, String>
where
    D: FnOnce(&Manifest) -> Result<(), String>,
{
    match evaluate_manifest(manifest_bytes, signature, current, now_epoch_secs) {
        Ok(UpdateEvaluation::Available(manifest)) => {
            download(&manifest)?;
            Ok(Some(manifest))
        }
        Ok(UpdateEvaluation::UpToDate) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Resolve the manifest artifact for `arch`, or explain what is on offer.
pub(crate) fn artifact_for<'m>(
    manifest: &'m Manifest,
    arch: &str,
) -> Result<&'m super::manifest::ManifestArtifact, String> {
    manifest.artifacts.get(arch).ok_or_else(|| {
        let offered: Vec<&str> = manifest.artifacts.keys().map(String::as_str).collect();
        format!("manifest {} has no artifact for arch {arch} (offers: {})", manifest.version, offered.join(", "))
    })
}

#[cfg(test)]
mod tests;
