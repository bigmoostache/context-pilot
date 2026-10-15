//! Disk cache for OCR results and crash-safe file writes.
//!
//! Results live in `<dir>/<content-hash>_<mode>.txt`. Every write goes
//! through [`atomic_write`], so a crash mid-write can never leave a
//! truncated file that a later lookup would serve as a hit.

use std::path::{Path, PathBuf};

/// Global cache directory for OCR results: `~/.context-pilot/ocr-cache/`.
pub(crate) fn default_dir() -> PathBuf {
    cp_env::env().core.home.join(".context-pilot/ocr-cache")
}

/// Cache key for a document: content hash (FNV-1a 128-bit) + output format.
pub(crate) fn key(data: &[u8], format: &str) -> String {
    format!("{}_{format}", cp_mod_utilities::hash::compute(data))
}

/// Cached result for `key`, if any. Empty entries (written by older builds
/// when the API omitted the result field) count as misses.
pub(crate) fn read(dir: &Path, key: &str) -> Option<String> {
    std::fs::read_to_string(dir.join(format!("{key}.txt"))).ok().filter(|text| !text.is_empty())
}

/// Store a result. Failures are logged, never fatal: the cache is an
/// optimisation, the output file is what the caller asked for.
pub(crate) fn write(dir: &Path, key: &str, text: &str) {
    if let Err(e) = std::fs::create_dir_all(dir) {
        log::warn!("Cannot create OCR cache dir {}: {e}", dir.display());
        return;
    }
    let path = dir.join(format!("{key}.txt"));
    if let Err(e) = atomic_write(&path, text.as_bytes()) {
        log::warn!("Cannot write OCR cache {}: {e}", path.display());
    }
}

/// Write `data` to a sibling temp file, then rename it over `path`.
///
/// # Errors
///
/// Returns the I/O error of the write or the rename (temp file removed).
pub(crate) fn atomic_write(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let name = path.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or("ocr");
    let staging = path.with_file_name(format!(".{name}.{}.staging", std::process::id()));
    std::fs::write(&staging, data)?;
    std::fs::rename(&staging, path).inspect_err(|_e| {
        let _r = std::fs::remove_file(&staging);
    })
}
