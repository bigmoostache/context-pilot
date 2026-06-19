//! File / folder download: raw bytes for a file, a zip archive for a folder.

use std::path::Path;
use std::sync::Mutex;

use crate::transport::rest::HttpReply;
use crate::transport::Backend;

use super::support::{agent_folder, confined_path, extract_param};

/// Maximum file size for downloads (10 MiB).
const MAX_DOWNLOAD_BYTES: u64 = 10 * 1024 * 1024;

/// Maximum zip archive size for folder downloads (100 MiB).
const MAX_ZIP_BYTES: u64 = 100 * 1024 * 1024;

/// `GET /api/agent/{id}/fs/download?path=` — file or folder download.
///
/// **Files** are returned as raw bytes with `Content-Disposition: attachment`,
/// capped at [`MAX_DOWNLOAD_BYTES`].
///
/// **Folders** are zipped into a temporary archive (`/tmp`), returned as
/// `{dirname}.zip`, capped at [`MAX_ZIP_BYTES`], then cleaned up. Uses the
/// system `zip` command (available on macOS and most Linux distros).
///
/// Returns `Ok((bytes, filename))` on success, `Err(HttpReply)` on error.
pub fn fs_download(
    state: &Mutex<Backend>,
    agent_id: &str,
    query: &str,
) -> Result<(Vec<u8>, String), HttpReply> {
    let folder = agent_folder(state, agent_id)?;
    let relative = match extract_param(query, "path") {
        Some(p) if !p.is_empty() => p,
        _ => return Err(HttpReply::error(400, "missing path parameter")),
    };
    let target = match confined_path(&folder, &relative) {
        Some(p) => p,
        None => return Err(HttpReply::error(403, "path outside agent realm")),
    };

    if target.is_dir() {
        return zip_and_download(&target);
    }

    if !target.is_file() {
        return Err(HttpReply::error(404, "not a file or directory"));
    }

    let meta = std::fs::metadata(&target).map_err(|_| HttpReply::error(404, "file not found"))?;
    if meta.len() > MAX_DOWNLOAD_BYTES {
        return Err(HttpReply::error(413, "file too large for download"));
    }

    let bytes = std::fs::read(&target).map_err(|_| HttpReply::error(502, "read failed"))?;
    let filename = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("download")
        .to_owned();

    Ok((bytes, filename))
}

/// Zip a directory into `/tmp` and return its bytes + filename.
fn zip_and_download(dir: &Path) -> Result<(Vec<u8>, String), HttpReply> {
    let dirname = dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("folder");

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let tmp = format!("/tmp/cp-dl-{}-{now}.zip", std::process::id());

    let output = std::process::Command::new("zip")
        .args(["-r", "-q", &tmp, "."])
        .current_dir(dir)
        .output()
        .map_err(|_| HttpReply::error(502, "zip command not available"))?;

    if !output.status.success() {
        drop(std::fs::remove_file(&tmp));
        return Err(HttpReply::error(502, "zip failed"));
    }

    let meta = std::fs::metadata(&tmp).map_err(|_| {
        drop(std::fs::remove_file(&tmp));
        HttpReply::error(502, "zip read failed")
    })?;
    if meta.len() > MAX_ZIP_BYTES {
        drop(std::fs::remove_file(&tmp));
        return Err(HttpReply::error(413, "zipped folder too large"));
    }

    let bytes = std::fs::read(&tmp).map_err(|_| {
        drop(std::fs::remove_file(&tmp));
        HttpReply::error(502, "zip read failed")
    })?;
    drop(std::fs::remove_file(&tmp));

    let zip_name = format!("{dirname}.zip");
    Ok((bytes, zip_name))
}

/// `GET /api/agent/{id}/fs/raw?path=` — serve a file's raw bytes **inline**.
///
/// Unlike [`fs_download`] (which forces `Content-Disposition: attachment`), this
/// serves the bytes with an inferred `Content-Type` and no attachment header, so
/// the browser renders them directly — powering the Finder's in-pane **image**
/// (T286) and **PDF** (T281) previews via `<img>` / `<object>` tags pointed at
/// this URL. Confined to the agent realm and capped at [`MAX_DOWNLOAD_BYTES`].
///
/// Returns `Ok((bytes, content_type))` on success, `Err(HttpReply)` on error.
pub fn fs_raw(
    state: &Mutex<Backend>,
    agent_id: &str,
    query: &str,
) -> Result<(Vec<u8>, String), HttpReply> {
    let folder = agent_folder(state, agent_id)?;
    let relative = match extract_param(query, "path") {
        Some(p) if !p.is_empty() => p,
        _ => return Err(HttpReply::error(400, "missing path parameter")),
    };
    let target = match confined_path(&folder, &relative) {
        Some(p) => p,
        None => return Err(HttpReply::error(403, "path outside agent realm")),
    };
    if !target.is_file() {
        return Err(HttpReply::error(404, "not a file"));
    }

    let meta = std::fs::metadata(&target).map_err(|_| HttpReply::error(404, "file not found"))?;
    if meta.len() > MAX_DOWNLOAD_BYTES {
        return Err(HttpReply::error(413, "file too large to preview"));
    }

    let bytes = std::fs::read(&target).map_err(|_| HttpReply::error(502, "read failed"))?;
    let ctype = target
        .file_name()
        .and_then(|n| n.to_str())
        .map_or("application/octet-stream", content_type_for);

    Ok((bytes, ctype.to_owned()))
}

/// Infer an HTTP `Content-Type` from a filename's extension, restricted to the
/// kinds the Finder serves inline (images + PDF). Anything else falls back to
/// `application/octet-stream` (the browser then offers to download rather than
/// mis-render it).
fn content_type_for(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "tiff" | "tif" => "image/tiff",
        "avif" => "image/avif",
        "heic" => "image/heic",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}
