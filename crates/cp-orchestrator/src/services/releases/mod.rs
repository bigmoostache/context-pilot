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

/// On-disk configuration (arch, active tag, auto-update policy) — see
/// [`config`].
mod config;
use config::ReleaseConfig;
pub use config::{MaintenanceWindow, UpdateMode};

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

    /// The releases root directory this store manages.
    #[must_use]
    pub(crate) fn dir(&self) -> &std::path::Path {
        &self.dir
    }

    // ── Auto-update policy (update-policy v3, O4.1) ─────────────────────

    /// The box's auto-update posture (`auto` / `manual` / `paused`).
    #[must_use]
    pub fn update_mode(&self) -> UpdateMode {
        self.config.update_mode
    }

    /// Set the auto-update posture and persist.
    pub fn set_update_mode(&mut self, mode: UpdateMode) {
        self.config.update_mode = mode;
        self.persist();
    }

    /// The channel this box follows (`stable` or `nightly`).
    #[must_use]
    pub fn channel(&self) -> &str {
        &self.config.channel
    }

    /// Whether an admin channel switch is awaiting its first check — the next
    /// evaluation adopts the new channel's head regardless of version ordering.
    #[must_use]
    pub fn pending_channel_switch(&self) -> bool {
        self.config.pending_channel_switch
    }

    /// Switch the channel this box follows and persist. Arms the crossgrade
    /// flag and drops the now-stale "update available" hint (it pertained to
    /// the old channel) so the pane doesn't offer a foreign version until the
    /// next check on the new channel resolves.
    ///
    /// # Errors
    ///
    /// Returns an error if `channel` is not one of `stable` / `nightly`.
    pub fn set_channel(&mut self, channel: &str) -> Result<(), String> {
        if !matches!(channel, "stable" | "nightly") {
            return Err(format!("unknown channel {channel:?} (expected stable or nightly)"));
        }
        if self.config.channel == channel {
            return Ok(());
        }
        self.config.channel = channel.to_owned();
        self.config.pending_channel_switch = true;
        self.persist();
        let mut st = updater::UpdateState::load(&self.dir);
        st.available = None;
        st.available_notes_url = None;
        st.save(&self.dir);
        Ok(())
    }

    /// Clear the crossgrade flag once a check on the new channel has resolved.
    pub fn clear_pending_switch(&mut self) {
        if self.config.pending_channel_switch {
            self.config.pending_channel_switch = false;
            self.persist();
        }
    }

    /// Hours between channel polls.
    #[must_use]
    pub fn poll_interval_hours(&self) -> u32 {
        self.config.poll_interval_hours
    }

    /// The box-local maintenance window auto-applies are confined to.
    #[must_use]
    pub fn window(&self) -> &MaintenanceWindow {
        &self.config.window
    }

    /// Set the maintenance window and persist. Rejects malformed bounds.
    ///
    /// # Errors
    ///
    /// Returns an error if either bound is not a valid `HH:MM`.
    pub fn set_window(&mut self, window: MaintenanceWindow) -> Result<(), String> {
        if !window.is_valid() {
            return Err(format!("invalid window bounds: {} – {}", window.start, window.end));
        }
        self.config.window = window;
        self.persist();
        Ok(())
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
    // The appliance ships the static musl bundle (deploy/ansible/tasks/
    // fetch.yml) — a musl-built binary must self-identify as `-musl` or the
    // updater would OTA the box onto the gnu lane (different bundle contents:
    // the musl one has no meilisearch). Compile-time is exactly right here:
    // the running binary IS the lane.
    let libc = if cfg!(target_env = "musl") { "-musl" } else { "" };
    format!("{os}-{arch}{libc}")
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

/// Orchestrator self-update — stage a downloaded `cp-orchestrator` over the
/// running install path with atomic-rename + `.bak` rollback (see module docs).
mod self_update;
pub use self_update::{boot_check, boot_commit, boot_commit_when_healthy, stage_orchestrator_update};

/// Signed update-manifest schema (update-policy §5.3).
mod manifest;
pub use manifest::{Manifest, ManifestArtifact};

/// Manifest-signing trust anchor (update-policy §5.4).
mod signing;
pub use signing::UPDATE_PUBKEY;

/// The on-box updater: fetch → verify → download → apply (update-policy §5.5).
pub mod updater;

#[cfg(test)]
pub(crate) use self_update::{backup_path, pending_path};

#[cfg(test)]
mod tests;
