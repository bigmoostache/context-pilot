//! Meilisearch binary download and platform detection.
//!
//! Downloads the latest Meilisearch release from GitHub for the current
//! platform. The binary is stored at `~/.context-pilot/meilisearch/bin/`.

use std::time::Duration;

use super::server::{binary_path, ensure_global_dirs};

/// Detect the current platform for Meilisearch binary download.
///
/// Returns the platform suffix used in GitHub release asset names.
///
/// # Errors
///
/// Returns an error if the platform is unsupported.
fn detect_platform() -> Result<&'static str, String> {
    if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        Ok("macos-apple-silicon")
    } else if cfg!(target_os = "macos") && cfg!(target_arch = "x86_64") {
        Ok("macos-amd64")
    } else if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
        Ok("linux-amd64")
    } else if cfg!(target_os = "linux") && cfg!(target_arch = "aarch64") {
        Ok("linux-aarch64")
    } else {
        Err(format!("Unsupported platform: {} / {}", std::env::consts::OS, std::env::consts::ARCH))
    }
}

/// Pinned Meilisearch version.
///
/// We pin to a known-good release to avoid breaking changes from `latest`.
/// Bump this deliberately after testing new Meilisearch versions.
const MEILISEARCH_VERSION: &str = "v1.13.3";

/// Download the Meilisearch binary for the current platform.
///
/// Uses the pinned version from [`MEILISEARCH_VERSION`]. The binary is
/// stored at `~/.context-pilot/meilisearch/bin/meilisearch`.
/// Skips if the binary already exists.
///
/// # Errors
///
/// Returns an error if the download fails or the platform is unsupported.
pub(crate) fn download_binary() -> Result<(), String> {
    let _root = ensure_global_dirs()?;
    let bin = binary_path()?;

    // Skip if already downloaded
    if bin.exists() {
        return Ok(());
    }

    let platform = detect_platform()?;
    let tag = MEILISEARCH_VERSION;

    let url = format!("https://github.com/meilisearch/meilisearch/releases/download/{tag}/meilisearch-{platform}");

    log::info!("Downloading Meilisearch {tag} for {platform}...");

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_mins(5))
        .user_agent("context-pilot/0.1")
        .build()
        .map_err(|e| format!("Cannot create HTTP client: {e}"))?;

    let resp = client.get(&url).send().map_err(|e| format!("Download failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("Download returned HTTP {}", resp.status()));
    }

    let bytes = resp.bytes().map_err(|e| format!("Cannot read download body: {e}"))?;

    std::fs::write(&bin, &bytes).map_err(|e| format!("Cannot write binary to {}: {e}", bin.display()))?;

    // Make executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(&bin, perms).map_err(|e| format!("Cannot chmod binary: {e}"))?;
    }

    log::info!("Meilisearch binary downloaded to {}", bin.display());
    Ok(())
}
