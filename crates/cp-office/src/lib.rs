//! Office documents (`.docx` / `.xlsx` / `.pptx`) handled as plain files.
//!
//! The file on disk is the source of truth. Agents edit it directly with any
//! tool; the web editors (`BetterOffice`, running in the browser) load its bytes
//! and save the whole file back. This crate guards that hand-off:
//!
//! * [`OfficeKind`] — which of the three OOXML formats a path is.
//! * [`version`] — a cheap fingerprint (mtime + size) the browser polls to
//!   notice an agent's edit, and sends back on save.
//! * [`read`] / [`write`] — capped read; validated, conflict-checked, atomic
//!   write (temp file + rename, so a reader never sees half a document).

use std::fmt;
use std::io::{Cursor, Write as _};
use std::path::Path;
use std::time::UNIX_EPOCH;

#[cfg(test)]
mod tests;

/// Largest document read or written (200 MiB) — far above real decks, low
/// enough that one request can't exhaust the orchestrator's memory.
pub const MAX_BYTES: u64 = 200 * 1024 * 1024;

/// The three OOXML formats the web editors handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfficeKind {
    /// Word document.
    Docx,
    /// Excel workbook.
    Xlsx,
    /// `PowerPoint` deck.
    Pptx,
}

impl OfficeKind {
    /// Kind from the path's extension (case-insensitive); `None` for anything else.
    #[must_use]
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "docx" => Some(Self::Docx),
            "xlsx" => Some(Self::Xlsx),
            "pptx" => Some(Self::Pptx),
            _ => None,
        }
    }

    /// Part every valid package of this kind contains (main document part).
    const fn main_part_prefix(self) -> &'static str {
        match self {
            Self::Docx => "word/",
            Self::Xlsx => "xl/",
            Self::Pptx => "ppt/",
        }
    }
}

/// Why an office read or write was refused.
#[derive(Debug)]
pub enum OfficeError {
    /// Not a `.docx` / `.xlsx` / `.pptx` path.
    Unsupported,
    /// The file does not exist (read) or is not a regular file.
    NotFound,
    /// Over [`MAX_BYTES`].
    TooLarge,
    /// The bytes are not a package of the path's kind.
    Invalid(String),
    /// The file changed since the caller's base version; carries the current one.
    Conflict(String),
    /// Filesystem fault.
    Io(std::io::Error),
}

impl OfficeError {
    /// HTTP status that best describes the error.
    #[must_use]
    pub const fn status(&self) -> u16 {
        match *self {
            Self::Unsupported => 415,
            Self::NotFound => 404,
            Self::TooLarge => 413,
            Self::Invalid(_) => 422,
            Self::Conflict(_) => 409,
            Self::Io(_) => 502,
        }
    }
}

impl fmt::Display for OfficeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        cp_macros::deref_match!(self, {
            Self::Unsupported => f.write_str("not a docx/xlsx/pptx file"),
            Self::NotFound => f.write_str("file not found"),
            Self::TooLarge => f.write_str("document too large"),
            Self::Invalid(ref why) => write!(f, "invalid document: {why}"),
            Self::Conflict(ref current) => write!(f, "file changed on disk (now {current})"),
            Self::Io(ref e) => write!(f, "io error: {e}"),
        })
    }
}

impl std::error::Error for OfficeError {}

impl From<std::io::Error> for OfficeError {
    fn from(e: std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::NotFound { Self::NotFound } else { Self::Io(e) }
    }
}

/// A document's bytes plus the version they were read at.
#[derive(Debug)]
pub struct OfficeFile {
    /// Raw package bytes.
    pub bytes: Vec<u8>,
    /// [`version`] at read time.
    pub version: String,
    /// Format.
    pub kind: OfficeKind,
}

/// Fingerprint `"<mtime_ns>-<len>"`: changes whenever anyone rewrites the file.
///
/// # Errors
///
/// [`OfficeError::NotFound`] / [`OfficeError::Io`] when the file can't be stat'ed.
pub fn version(path: &Path) -> Result<String, OfficeError> {
    let meta = std::fs::metadata(path)?;
    if !meta.is_file() {
        return Err(OfficeError::NotFound);
    }
    let mtime = meta.modified()?.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    Ok(format!("{mtime}-{}", meta.len()))
}

/// Read a document with its version.
///
/// # Errors
///
/// Unsupported extension, missing file, over [`MAX_BYTES`], or an IO fault.
pub fn read(path: &Path) -> Result<OfficeFile, OfficeError> {
    let kind = OfficeKind::from_path(path).ok_or(OfficeError::Unsupported)?;
    let before = version(path)?;
    if std::fs::metadata(path)?.len() > MAX_BYTES {
        return Err(OfficeError::TooLarge);
    }
    let bytes = std::fs::read(path)?;
    Ok(OfficeFile { bytes, version: before, kind })
}

/// Replace a document with `bytes`; returns the new version.
///
/// When `base` is given and the file's current version differs, nothing is
/// written ([`OfficeError::Conflict`]) — an agent edited it meanwhile. The
/// bytes must be a zip package with `[Content_Types].xml` and the kind's main
/// folder. The write goes to a sibling temp file, is synced, then renamed over
/// the target.
///
/// # Errors
///
/// Unsupported extension, oversize, invalid package, conflict, or IO fault.
pub fn write(path: &Path, bytes: &[u8], base: Option<&str>) -> Result<String, OfficeError> {
    let kind = OfficeKind::from_path(path).ok_or(OfficeError::Unsupported)?;
    if u64::try_from(bytes.len()).map_or(true, |n| n > MAX_BYTES) {
        return Err(OfficeError::TooLarge);
    }
    validate(kind, bytes)?;
    if let Some(expected) = base {
        let current = version(path)?;
        if current != expected {
            return Err(OfficeError::Conflict(current));
        }
    }
    let dir = path.parent().ok_or(OfficeError::NotFound)?;
    let name = path.file_name().and_then(|n| n.to_str()).ok_or(OfficeError::NotFound)?;
    let staging = dir.join(format!(".{name}.cp-office-{}.tmp", std::process::id()));
    let written = write_synced(&staging, bytes).and_then(|()| std::fs::rename(&staging, path));
    if let Err(e) = written {
        drop(std::fs::remove_file(&staging));
        return Err(OfficeError::Io(e));
    }
    version(path)
}

/// Create/truncate `staging`, write `bytes`, fsync.
fn write_synced(staging: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = std::fs::File::create(staging)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Check `bytes` is an OOXML package of `kind`.
fn validate(kind: OfficeKind, bytes: &[u8]) -> Result<(), OfficeError> {
    let archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| OfficeError::Invalid(format!("not a zip package: {e}")))?;
    let mut has_types = false;
    let mut has_main = false;
    for name in archive.file_names() {
        has_types |= name == "[Content_Types].xml";
        has_main |= name.starts_with(kind.main_part_prefix());
    }
    if !has_types {
        return Err(OfficeError::Invalid("missing [Content_Types].xml".to_owned()));
    }
    if !has_main {
        return Err(OfficeError::Invalid(format!("missing {} parts", kind.main_part_prefix())));
    }
    Ok(())
}
