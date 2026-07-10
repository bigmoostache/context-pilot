//! Local release management — architecture detection, GitHub release listing,
//! download + extraction, and active-binary selection.
//!
//! The [`ReleaseStore`] manages the `~/.context-pilot/releases/` directory
//! where downloaded release binaries live. Each release is a tag-named
//! subdirectory containing the extracted `cpilot` (+ `cp-console-server`)
//! binaries from the GitHub release tarball.
//!
//! # Storage layout
//!
//! ```text
//! ~/.context-pilot/releases/
//! ├── config.json          ← arch + active tag
//! ├── v0.3.0-abc1234/
//! │   ├── cpilot
//! │   └── cp-console-server
//! └── v0.2.10-def5678/
//!     ├── cpilot
//!     └── cp-console-server
//! ```

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// GitHub repository for release asset lookup (public, no auth needed).
const GITHUB_REPO: &str = "bigmoostache/context-pilot";

/// User-Agent header required by the GitHub API.
const USER_AGENT: &str = "context-pilot-orchestrator";

// ── Config persistence ──────────────────────────────────────────────────

/// On-disk configuration for the release manager.
#[derive(Debug, Serialize, Deserialize)]
struct ReleaseConfig {
    /// Platform architecture string (e.g. `"macos-aarch64"`).
    arch: String,
    /// `true` when `arch` was auto-detected, `false` when manually set.
    arch_auto: bool,
    /// Tag of the currently selected (active) release, if any.
    active_tag: Option<String>,
}

impl Default for ReleaseConfig {
    fn default() -> Self {
        Self { arch: detect_arch(), arch_auto: true, active_tag: None }
    }
}

// ── Public types ────────────────────────────────────────────────────────

/// A locally downloaded release (tag + binary presence).
#[derive(Debug, Serialize)]
pub struct LocalRelease {
    /// The git tag (e.g. `"v0.3.0-abc1234"`).
    pub tag: String,
    /// Size of the `cpilot` binary in bytes (0 if missing).
    pub binary_size: u64,
}

/// A release available on GitHub (may or may not be downloaded locally).
#[derive(Debug, Serialize)]
pub struct RemoteRelease {
    /// The git tag.
    pub tag: String,
    /// Human-readable release name.
    pub name: String,
    /// ISO-8601 publication timestamp.
    pub published_at: String,
    /// Download URL for the architecture-matching asset, if any.
    pub asset_url: Option<String>,
    /// Asset size in bytes.
    pub asset_size: Option<u64>,
    /// Whether this is the latest release on GitHub.
    pub is_latest: bool,
}

// ── ReleaseStore ────────────────────────────────────────────────────────

/// Manages locally downloaded release binaries and queries GitHub for
/// available remote releases.
#[derive(Debug)]
pub struct ReleaseStore {
    /// Persisted config (arch, active tag).
    config: ReleaseConfig,
    /// Root directory (`~/.context-pilot/releases/`).
    dir: PathBuf,
    /// Path to `config.json` inside `dir`.
    config_path: PathBuf,
}

impl ReleaseStore {
    /// Load (or create) the release store from the given directory.
    ///
    /// A missing or corrupt config file silently yields defaults (auto-detect
    /// arch, no active tag).
    #[must_use]
    pub fn load(releases_dir: PathBuf) -> Self {
        let config_path = releases_dir.join("config.json");
        let config = std::fs::read(&config_path)
            .ok()
            .and_then(|b| serde_json::from_slice::<ReleaseConfig>(&b).ok())
            .unwrap_or_default();
        Self { config, dir: releases_dir, config_path }
    }

    /// The default releases directory (`~/.context-pilot/releases/`).
    pub fn default_dir() -> Option<PathBuf> {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".context-pilot/releases"))
    }

    // ── Arch ────────────────────────────────────────────────────────────

    /// Current architecture string (e.g. `"macos-aarch64"`).
    #[must_use]
    pub fn arch(&self) -> &str {
        &self.config.arch
    }

    /// Whether the architecture was auto-detected.
    #[must_use]
    pub fn is_arch_auto(&self) -> bool {
        self.config.arch_auto
    }

    /// Manually override the architecture and persist.
    pub fn set_arch(&mut self, arch: &str) {
        self.config.arch = arch.to_owned();
        self.config.arch_auto = false;
        self.persist();
    }

    /// Reset to auto-detected architecture and persist.
    pub fn auto_detect_arch(&mut self) {
        self.config.arch = detect_arch();
        self.config.arch_auto = true;
        self.persist();
    }

    // ── Local releases ──────────────────────────────────────────────────

    /// Scan the releases directory for downloaded releases.
    #[must_use]
    pub fn local_releases(&self) -> Vec<LocalRelease> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut releases = Vec::new();
        for entry in entries.flatten() {
            if !entry.file_type().map_or(false, |ft| ft.is_dir()) {
                continue;
            }
            let tag = entry.file_name().to_string_lossy().into_owned();
            // Skip non-tag directories (must start with 'v').
            if !tag.starts_with('v') {
                continue;
            }
            let binary = entry.path().join("cpilot");
            let binary_size = std::fs::metadata(&binary).map(|m| m.len()).unwrap_or(0);
            releases.push(LocalRelease { tag, binary_size });
        }
        releases.sort_by(|a, b| semver_sort_key(&b.tag).cmp(&semver_sort_key(&a.tag)));
        releases
    }

    /// Path to the `cpilot` (agent/TUI) binary for a given tag.
    #[must_use]
    pub fn binary_path(&self, tag: &str) -> PathBuf {
        self.dir.join(tag).join("cpilot")
    }

    /// Path to the `cp-orchestrator` binary inside a downloaded release.
    ///
    /// The release bundle is flat (`cpilot`, `cp-console-server`,
    /// `cp-orchestrator`, `web/`), so the orchestrator binary sits next to the
    /// agent binary in the tag directory. Used by the "Update & Restart
    /// Orchestrator" self-update flow to adopt a freshly downloaded orchestrator.
    #[must_use]
    pub fn orchestrator_binary_path(&self, tag: &str) -> PathBuf {
        self.dir.join(tag).join("cp-orchestrator")
    }

    /// The currently active tag, if any.
    #[must_use]
    pub fn active_tag(&self) -> Option<&str> {
        self.config.active_tag.as_deref()
    }

    /// Select a downloaded release as active. Returns the binary path.
    ///
    /// # Errors
    ///
    /// Returns an error if the tag's binary does not exist locally.
    pub fn select(&mut self, tag: &str) -> Result<PathBuf, String> {
        let binary = self.binary_path(tag);
        if !binary.exists() {
            return Err(format!("release {tag} is not downloaded (binary not found)"));
        }
        self.config.active_tag = Some(tag.to_owned());
        self.persist();
        Ok(binary)
    }

    /// Delete a locally downloaded release.
    ///
    /// # Errors
    ///
    /// Returns an error if the tag is the currently active release, or if
    /// the directory cannot be removed.
    pub fn delete(&mut self, tag: &str) -> Result<(), String> {
        if self.config.active_tag.as_deref() == Some(tag) {
            return Err("cannot delete the currently active release".to_owned());
        }
        let path = self.dir.join(tag);
        if !path.exists() {
            return Err(format!("release {tag} is not downloaded"));
        }
        std::fs::remove_dir_all(&path).map_err(|e| format!("failed to delete {tag}: {e}"))
    }

    // ── GitHub API ──────────────────────────────────────────────────────

    /// Fetch the list of releases from GitHub (public API, no auth).
    ///
    /// Returns remote releases with asset URLs matched to the current arch.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure or unexpected API response.
    pub fn fetch_remote_releases(&self) -> Result<Vec<RemoteRelease>, String> {
        let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases");
        let resp = reqwest::blocking::Client::new()
            .get(&url)
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/vnd.github+json")
            .send()
            .map_err(|e| format!("GitHub API request failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("GitHub API returned {}", resp.status()));
        }

        let releases: Vec<GitHubRelease> = resp.json().map_err(|e| format!("failed to parse GitHub response: {e}"))?;

        let arch = &self.config.arch;
        let mut result = Vec::with_capacity(releases.len());
        for (i, rel) in releases.iter().enumerate() {
            let matching_asset = rel.assets.iter().find(|a| a.name.contains(arch) && a.name.ends_with(".tar.gz"));
            result.push(RemoteRelease {
                tag: rel.tag_name.clone(),
                name: rel.name.clone().unwrap_or_else(|| rel.tag_name.clone()),
                published_at: rel.published_at.clone().unwrap_or_default(),
                asset_url: matching_asset.map(|a| a.browser_download_url.clone()),
                asset_size: matching_asset.map(|a| a.size),
                is_latest: i == 0,
            });
        }
        Ok(result)
    }

    /// Download a release tarball from `asset_url`, extract it to
    /// `releases_dir/{tag}/`, and set the binary executable.
    ///
    /// # Errors
    ///
    /// Returns an error on download failure, extraction failure, or if the
    /// release is already downloaded.
    pub fn download(&self, tag: &str, asset_url: &str) -> Result<(), String> {
        let dest = self.dir.join(tag);
        if dest.exists() {
            return Err(format!("release {tag} already downloaded"));
        }

        // Ensure the releases directory exists.
        std::fs::create_dir_all(&self.dir).map_err(|e| format!("mkdir releases: {e}"))?;

        // Download the tarball to a temp file.
        let tmp_tarball = self.dir.join(format!("{tag}.tar.gz.tmp"));
        let resp = reqwest::blocking::Client::new()
            .get(asset_url)
            .header("User-Agent", USER_AGENT)
            .send()
            .map_err(|e| format!("download failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("download returned HTTP {}", resp.status()));
        }

        let bytes = resp.bytes().map_err(|e| format!("download read failed: {e}"))?;
        std::fs::write(&tmp_tarball, &bytes).map_err(|e| format!("write tarball: {e}"))?;

        // Extract with system tar (available on macOS + Linux).
        std::fs::create_dir_all(&dest).map_err(|e| format!("mkdir {tag}: {e}"))?;

        let tar_status = std::process::Command::new("tar")
            .args(["xzf", &tmp_tarball.to_string_lossy(), "-C", &dest.to_string_lossy()])
            .status()
            .map_err(|e| format!("tar command failed: {e}"))?;

        // Clean up the temp tarball regardless of extraction result.
        let _rm = std::fs::remove_file(&tmp_tarball);

        if !tar_status.success() {
            // Clean up the partial extraction.
            let _rm = std::fs::remove_dir_all(&dest);
            return Err("tar extraction failed".to_owned());
        }

        // Set executable permission on the binary (Unix).
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let binary = dest.join("cpilot");
            if binary.exists() {
                let _r = std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755));
            }
            let console = dest.join("cp-console-server");
            if console.exists() {
                let _r = std::fs::set_permissions(&console, std::fs::Permissions::from_mode(0o755));
            }
        }

        Ok(())
    }

    // ── Persistence ─────────────────────────────────────────────────────

    /// Atomically write config to disk (`tmp` → `rename`).
    fn persist(&self) {
        let Ok(bytes) = serde_json::to_vec_pretty(&self.config) else {
            eprintln!("releases: serialize config failed");
            return;
        };
        if std::fs::create_dir_all(&self.dir).is_err() {
            eprintln!("releases: create dir failed");
            return;
        }
        let tmp = self.config_path.with_extension("json.tmp");
        if std::fs::write(&tmp, &bytes).is_err() {
            eprintln!("releases: write tmp failed: {}", tmp.display());
            return;
        }
        if let Err(e) = std::fs::rename(&tmp, &self.config_path) {
            eprintln!("releases: rename failed: {e}");
        }
    }
}

// ── GitHub API types (deserialization only) ──────────────────────────────

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    name: Option<String>,
    published_at: Option<String>,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

// ── Helpers ─────────────────────────────────────────────────────────────

/// Auto-detect the platform architecture from compile-time constants.
///
/// Maps to the release asset naming convention: `cpilot-{os}-{arch}.tar.gz`.
fn detect_arch() -> String {
    let os = match std::env::consts::OS {
        "macos" => "macos",
        "linux" => "linux",
        _ => "unknown",
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        _ => "unknown",
    };
    format!("{os}-{arch}")
}

/// All known architecture targets from the release matrix.
pub const KNOWN_ARCHS: &[&str] =
    &["macos-aarch64", "macos-x86_64", "linux-x86_64", "linux-aarch64", "linux-aarch64-musl"];

/// Parse a semver-like tag `"vMAJOR.MINOR.PATCH..."` into a comparable tuple.
///
/// Non-numeric or missing components default to 0. This gives correct
/// *descending* order when used with `.reverse()` or `Reverse(...)`.
pub fn semver_sort_key(tag: &str) -> (u32, u32, u32) {
    let stripped = tag.strip_prefix('v').unwrap_or(tag);
    let mut parts = stripped.splitn(3, '.');
    let major = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    // Patch may trail non-numeric chars (e.g. "10-rc1") — parse prefix digits.
    let patch = parts
        .next()
        .and_then(|s| s.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().ok())
        .unwrap_or(0);
    (major, minor, patch)
}

// ── Orchestrator self-update (adopt a downloaded cp-orchestrator) ─────────
//
// The "Update & Restart Orchestrator" button stages a freshly downloaded
// `cp-orchestrator` over the running install path and then re-execs it. Because
// you cannot overwrite a running executable in place (`ETXTBSY`), we write the
// new bytes to a sibling temp file and `rename` it over the install path — that
// atomically swaps the directory entry to the new inode while the running
// process keeps its old (now-unlinked) inode until it re-execs.
//
// Safety: before swapping we back the current binary up to `<name>.bak`, and we
// drop a `<name>.pending` marker holding a boot-attempt counter. On startup
// [`boot_check`] increments that counter; if a staged update fails to boot
// [`MAX_BOOT_ATTEMPTS`] times (crash-loop under a supervisor), it automatically
// restores the `.bak`, so a bad update self-heals instead of bricking the box.
// A healthy boot calls [`boot_commit`] to clear the marker and backup.

/// How many failed boot attempts of a staged update we tolerate before the
/// startup guard rolls back to the `.bak` binary.
const MAX_BOOT_ATTEMPTS: u32 = 2;

/// Sibling path for the backup of the previous orchestrator binary.
fn backup_path(install: &std::path::Path) -> PathBuf {
    with_suffix(install, "bak")
}

/// Sibling path for the boot-attempt marker of a staged update.
fn pending_path(install: &std::path::Path) -> PathBuf {
    with_suffix(install, "pending")
}

/// Append a `.suffix` to a path's file name (not `with_extension`, which would
/// clobber an existing extension — the binary has none, but be explicit).
fn with_suffix(path: &std::path::Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".");
    name.push(suffix);
    path.with_file_name(name)
}

/// Stage `src` (a downloaded `cp-orchestrator`) over the running `install`
/// binary via atomic rename, backing the current binary up to `<name>.bak` and
/// writing a fresh `<name>.pending` boot-attempt marker.
///
/// The running process is untouched (it keeps its open inode); the swap only
/// takes effect when the process re-execs the install path.
///
/// # Errors
///
/// Returns an error if `src` is missing/empty or any filesystem step fails. On
/// error the install path is left as it was (best-effort — the backup copy is
/// non-destructive).
pub fn stage_orchestrator_update(install: &std::path::Path, src: &std::path::Path) -> Result<(), String> {
    let meta = std::fs::metadata(src).map_err(|e| format!("stat {}: {e}", src.display()))?;
    if meta.len() == 0 {
        return Err(format!("{} is empty", src.display()));
    }

    // 1. Back up the current binary (copy, so `install` is never absent).
    let bak = backup_path(install);
    let _bytes =
        std::fs::copy(install, &bak).map_err(|e| format!("backup {} -> {}: {e}", install.display(), bak.display()))?;

    // 2. Write the new bytes to a sibling temp and make it executable.
    let staged = with_suffix(install, "new");
    let _bytes =
        std::fs::copy(src, &staged).map_err(|e| format!("stage {} -> {}: {e}", src.display(), staged.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _r = std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755));
    }

    // 3. Atomically swap the new binary into place (dodges ETXTBSY).
    std::fs::rename(&staged, install).map_err(|e| {
        let _cleanup = std::fs::remove_file(&staged);
        format!("promote {} -> {}: {e}", staged.display(), install.display())
    })?;

    // 4. Drop the boot-attempt marker (counter starts at 0).
    let _w = std::fs::write(pending_path(install), b"0");
    Ok(())
}

/// Startup guard: account for a staged update's boot attempt.
///
/// If a `.pending` marker exists, increment its counter. Once the counter
/// reaches [`MAX_BOOT_ATTEMPTS`] (the staged binary keeps crashing on boot),
/// restore the `.bak` binary over the install path and clear the markers so the
/// service self-heals back to the last-known-good binary. Call this **before**
/// binding, as early in `main` as possible.
pub fn boot_check(install: &std::path::Path) {
    let pending = pending_path(install);
    let Ok(raw) = std::fs::read_to_string(&pending) else {
        return; // No staged update in flight.
    };
    let attempts: u32 = raw.trim().parse::<u32>().unwrap_or(0).saturating_add(1);

    if attempts >= MAX_BOOT_ATTEMPTS {
        // The staged update is crash-looping — roll back to the backup.
        let bak = backup_path(install);
        if bak.exists() {
            if let Err(e) = std::fs::rename(&bak, install) {
                eprintln!("self-update: rollback {} -> {} failed: {e}", bak.display(), install.display());
            } else {
                eprintln!(
                    "self-update: staged orchestrator failed to boot {attempts}× — rolled back to previous binary"
                );
            }
        }
        let _rm = std::fs::remove_file(&pending);
    } else {
        // Still within the tolerance window — record this attempt.
        let _w = std::fs::write(&pending, attempts.to_string().as_bytes());
    }
}

/// Commit a staged update after a healthy boot: clear the `.pending` marker and
/// delete the `.bak` backup. Call once the process is known to be running
/// normally (e.g. after it has stayed up past a short grace period).
pub fn boot_commit(install: &std::path::Path) {
    let pending = pending_path(install);
    if !pending.exists() {
        return; // Nothing staged; normal boot.
    }
    let _rm_pending = std::fs::remove_file(&pending);
    let _rm_bak = std::fs::remove_file(backup_path(install));
    eprintln!("self-update: orchestrator update committed (previous binary backup removed)");
}

#[cfg(test)]
mod tests;
