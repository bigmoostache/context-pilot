//! Sha256-verified artifact download (§5.5 apply step 1 — the reversible part).
//!
//! The tarball's `sha256` is checked against the manifest's pin **before any
//! extraction**: a mismatching download aborts cleanly with nothing moved and
//! no `releases/<tag>/` directory left behind. The signature over the manifest
//! (checked upstream in [`verify`](super::verify)) covers the pins, so trust
//! extends from the minisign key to the tarball bits on disk.

use sha2::{Digest as _, Sha256};

use super::super::{Manifest, ReleaseStore};

/// Download the artifact for `arch` from a **verified** manifest into
/// `releases/<version>/`, checking the pinned `sha256` before extraction.
///
/// Idempotent: an already-downloaded release (its `cpilot` binary present) is
/// a no-op success.
///
/// # Errors
///
/// Returns an error on a missing arch artifact, network failure, `sha256`
/// mismatch (nothing extracted), or extraction failure (directory cleaned).
pub fn download_artifact(store: &ReleaseStore, manifest: &Manifest, arch: &str) -> Result<(), String> {
    let artifact = super::artifact_for(manifest, arch)?;
    let tag = &manifest.version;
    if store.binary_path(tag).exists() {
        return Ok(()); // already downloaded (sha was verified when it landed)
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    let resp = client
        .get(&artifact.url)
        .header("User-Agent", super::super::USER_AGENT)
        .send()
        .map_err(|e| format!("download {}: {e}", artifact.url))?;
    if !resp.status().is_success() {
        return Err(format!("download {}: HTTP {}", artifact.url, resp.status()));
    }
    let bytes = resp.bytes().map_err(|e| format!("download read: {e}"))?;

    verify_and_extract(store, tag, &bytes, &artifact.sha256)
}

/// Verify `bytes` against the pinned `sha256`, then extract into
/// `releases/<tag>/` (the same layout [`ReleaseStore::download`] produces).
///
/// # Errors
///
/// On a hash mismatch nothing is written at all; on an extraction failure the
/// partial `releases/<tag>/` directory is removed.
pub(crate) fn verify_and_extract(
    store: &ReleaseStore,
    tag: &str,
    bytes: &[u8],
    expected_sha256: &str,
) -> Result<(), String> {
    let actual = hex_sha256(bytes);
    if !actual.eq_ignore_ascii_case(expected_sha256) {
        return Err(format!("sha256 mismatch for {tag}: manifest pins {expected_sha256}, tarball hashes to {actual}"));
    }

    let releases_dir = store.dir();
    std::fs::create_dir_all(releases_dir).map_err(|e| format!("mkdir releases: {e}"))?;

    // Write the tarball to a temp file for the system tar (macOS + Linux).
    let tmp_tarball = releases_dir.join(format!("{tag}.tar.gz.tmp"));
    std::fs::write(&tmp_tarball, bytes).map_err(|e| format!("write tarball: {e}"))?;

    let dest = releases_dir.join(tag);
    let extracted = std::fs::create_dir_all(&dest).map_err(|e| format!("mkdir {tag}: {e}")).and_then(|()| {
        let status = std::process::Command::new("tar")
            .args(["xzf", &tmp_tarball.to_string_lossy(), "-C", &dest.to_string_lossy()])
            .status()
            .map_err(|e| format!("tar command failed: {e}"))?;
        if status.success() { Ok(()) } else { Err("tar extraction failed".to_owned()) }
    });
    let _rm = std::fs::remove_file(&tmp_tarball);

    if let Err(e) = extracted {
        let _cleanup = std::fs::remove_dir_all(&dest);
        return Err(e);
    }

    // Executable bits on the shipped binaries (tar usually preserves them —
    // be explicit, same as the legacy download path).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        for name in ["cpilot", "cp-console-server", "cp-orchestrator"] {
            let binary = dest.join(name);
            if binary.exists() {
                let _r = std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755));
            }
        }
    }
    Ok(())
}

/// Lower-hex SHA-256 of `bytes`.
fn hex_sha256(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        let _w = write!(out, "{byte:02x}");
    }
    out
}

/// The would-be extraction directory for a tag — used by the tests to assert
/// the abort path left the store untouched.
#[cfg(test)]
pub(crate) fn tag_dir(store: &ReleaseStore, tag: &str) -> std::path::PathBuf {
    store.dir().join(tag)
}
