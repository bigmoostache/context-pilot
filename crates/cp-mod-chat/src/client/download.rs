//! Tuwunel binary download and extraction.
//!
//! Downloads the pinned Tuwunel release from GitHub on first run and
//! decompresses it with the system `zstd` command. The binary is
//! installed to `~/.context-pilot/bin/tuwunel` and reused across all
//! projects on the same machine.

use std::path::Path;

use crate::server;

/// Pinned Tuwunel release version shipped with this Context Pilot version.
const TUWUNEL_VERSION: &str = "v1.5.1";

/// GitHub release URL template for the Tuwunel binary (zstd-compressed).
///
/// `VERSION_PLACEHOLDER` and `ARCH_PLACEHOLDER` are filled at runtime.
/// We use the statically-linked GNU build for maximum portability.
const TUWUNEL_URL_TEMPLATE: &str = "https://github.com/matrix-construct/tuwunel/releases/download/VERSION_PLACEHOLDER/VERSION_PLACEHOLDER-release-all-ARCH_PLACEHOLDER-linux-gnu-tuwunel.zst";

/// Placeholder token for the version in [`TUWUNEL_URL_TEMPLATE`].
const VERSION_PLACEHOLDER: &str = "VERSION_PLACEHOLDER";

/// Placeholder token for the architecture in [`TUWUNEL_URL_TEMPLATE`].
const ARCH_PLACEHOLDER: &str = "ARCH_PLACEHOLDER";

/// Ensure the Tuwunel binary exists at `~/.context-pilot/bin/tuwunel`.
///
/// If absent, downloads the pinned release from GitHub and decompresses
/// it with `zstd`. The download is ~26 MB (compressed) / ~87 MB (binary).
/// This runs once per machine; subsequent calls are a no-op.
///
/// # Errors
///
/// Returns a description if the download fails, `zstd` is missing, or
/// decompression fails.
pub(crate) fn ensure_binary() -> Result<(), String> {
    let bin_path = server::binary_path().ok_or("Cannot determine home directory for Tuwunel binary")?;
    if bin_path.exists() {
        return Ok(());
    }

    let bin_dir = bin_path.parent().ok_or("Invalid binary path")?;
    std::fs::create_dir_all(bin_dir).map_err(|e| format!("Cannot create {}: {e}", bin_dir.display()))?;

    log::info!("Tuwunel binary not found — downloading {TUWUNEL_VERSION}...");

    let url = build_download_url()?;
    let zst_path = bin_path.with_extension("zst");

    download_file(&url, &zst_path)?;
    decompress_zstd(&zst_path, &bin_path)?;

    // Clean up the compressed archive
    let _r = std::fs::remove_file(&zst_path);

    // Make the binary executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(&bin_path, perms).map_err(|e| format!("Cannot set executable permission: {e}"))?;
    }

    log::info!("Tuwunel {TUWUNEL_VERSION} installed at {}", bin_path.display());
    Ok(())
}

/// Build the download URL for the current CPU architecture.
fn build_download_url() -> Result<String, String> {
    let arch = std::env::consts::ARCH;
    let arch_slug = match arch {
        "x86_64" => "x86_64-v2",
        "aarch64" => "aarch64",
        _ => return Err(format!("Unsupported architecture for Tuwunel: {arch}")),
    };
    let version = TUWUNEL_VERSION;
    Ok(TUWUNEL_URL_TEMPLATE.replace(VERSION_PLACEHOLDER, version).replace(ARCH_PLACEHOLDER, arch_slug))
}

/// Download a file from a URL to a local path (blocking).
fn download_file(url: &str, dest: &Path) -> Result<(), String> {
    let resp = reqwest::blocking::get(url).map_err(|e| format!("Download failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("Download returned HTTP {}", resp.status()));
    }

    let bytes = resp.bytes().map_err(|e| format!("Failed to read download body: {e}"))?;
    std::fs::write(dest, &bytes).map_err(|e| format!("Cannot write {}: {e}", dest.display()))?;

    log::info!("Downloaded {} ({} bytes)", dest.display(), bytes.len());
    Ok(())
}

/// Decompress a `.zst` file using the system `zstd` command.
///
/// Falls back to a clear error message if `zstd` is not installed.
fn decompress_zstd(src: &Path, dest: &Path) -> Result<(), String> {
    let status = std::process::Command::new("zstd")
        .arg("-d")
        .arg(src.as_os_str())
        .arg("-o")
        .arg(dest.as_os_str())
        .arg("--force")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .status()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "zstd command not found. Install it: sudo apt install zstd (or equivalent)".to_string()
            } else {
                format!("Failed to run zstd: {e}")
            }
        })?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("zstd decompression failed (exit code: {})", status.code().unwrap_or(-1)))
    }
}
