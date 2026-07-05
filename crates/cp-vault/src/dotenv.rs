//! `.env` file read/write utilities with advisory file locking.
//!
//! Handles reading and persisting credentials to `~/.context-pilot/.env`.
//! Write operations use `flock()` (via `fs2`) to prevent concurrent corruption
//! when multiple processes (orchestrator + N agents) modify the file simultaneously.

use std::fs;
use std::io::Write as _;
use std::path::PathBuf;

use fs2::FileExt as _;

use crate::types::VaultError;

/// Path to the global environment file: `~/.context-pilot/.env`.
pub(crate) fn global_env_path() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".context-pilot").join(".env"))
}

/// Read a specific key from the global `.env` file (without dotenvy).
///
/// This is a fallback for cases where dotenvy hasn't loaded the file yet.
/// Parses simple `KEY=value` and `KEY="quoted value"` formats.
pub(crate) fn read_env_key(key: &str) -> Option<String> {
    let path = global_env_path()?;
    let content = fs::read_to_string(&path).ok()?;
    let prefix = format!("{key}=");

    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with(&prefix) && !trimmed.starts_with('#') {
            let raw_value = trimmed.get(prefix.len()..)?;
            return Some(unquote(raw_value));
        }
    }
    None
}

/// Persist a key-value pair to `~/.context-pilot/.env`.
///
/// Creates the directory and file if they don't exist.  Uses advisory file
/// locking (`flock`) to serialize concurrent writers.  Existing entries are
/// updated in-place; new entries are appended.
pub(crate) fn write_env_entry(key: &str, value: &str) -> Result<(), VaultError> {
    let path = global_env_path().ok_or_else(|| VaultError::Io("HOME not set".to_owned()))?;

    // Ensure parent directory exists.
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| VaultError::Io(format!("cannot create dir: {e}")))?;
    }

    // Open (or create) the file and acquire an exclusive lock.
    let lock_file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|e| VaultError::Io(format!("cannot open {}: {e}", path.display())))?;

    lock_file.lock_exclusive().map_err(|e| VaultError::Io(format!("flock failed: {e}")))?;

    // Read current content under lock.
    let existing = fs::read_to_string(&path).unwrap_or_default();

    let prefix = format!("{key}=");
    let new_line = format_env_line(key, value);
    let mut found = false;

    let mut lines: Vec<String> = existing
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with(&prefix) && !trimmed.starts_with('#') {
                found = true;
                new_line.clone()
            } else {
                line.to_owned()
            }
        })
        .collect();

    if !found {
        // Ensure trailing newline before appending.
        if lines.last().is_some_and(|l| !l.is_empty()) {
            lines.push(String::new());
        }
        lines.push(new_line);
    }

    let mut content = lines.join("\n");
    if !content.ends_with('\n') {
        content.push('\n');
    }

    // Write atomically (overwrite in place — rename would break the lock).
    let mut file =
        fs::File::create(&path).map_err(|e| VaultError::Io(format!("cannot write {}: {e}", path.display())))?;
    file.write_all(content.as_bytes()).map_err(|e| VaultError::Io(format!("write failed: {e}")))?;

    // Lock is released when lock_file is dropped.
    drop(lock_file);

    // Set restrictive permissions (owner-only read/write).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _r = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }

    Ok(())
}

/// Format an env line, quoting the value if it contains shell-sensitive characters.
fn format_env_line(key: &str, value: &str) -> String {
    if value.contains(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '#' || c == '$') {
        let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
        format!("{key}=\"{escaped}\"")
    } else {
        format!("{key}={value}")
    }
}

/// Remove surrounding quotes and unescape a .env value.
fn unquote(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.len() >= 2 {
        let first = trimmed.as_bytes().first().copied();
        let last = trimmed.as_bytes().last().copied();
        if (first == Some(b'"') && last == Some(b'"')) || (first == Some(b'\'') && last == Some(b'\'')) {
            let inner_start = 1;
            let inner_end = trimmed.len().saturating_sub(1);
            let inner = trimmed.get(inner_start..inner_end).unwrap_or_default();
            return inner.replace("\\\"", "\"").replace("\\\\", "\\");
        }
    }
    trimmed.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_plain_value() {
        assert_eq!(format_env_line("KEY", "abc123"), "KEY=abc123");
    }

    #[test]
    fn format_quotes_spaces() {
        assert_eq!(format_env_line("KEY", "has space"), "KEY=\"has space\"");
    }

    #[test]
    fn format_escapes_inner_quotes() {
        assert_eq!(format_env_line("KEY", "has\"quote"), "KEY=\"has\\\"quote\"");
    }

    #[test]
    fn unquote_double_quoted() {
        assert_eq!(unquote("\"hello world\""), "hello world");
    }

    #[test]
    fn unquote_escaped() {
        assert_eq!(unquote("\"say \\\"hi\\\"\""), "say \"hi\"");
    }

    #[test]
    fn unquote_plain() {
        assert_eq!(unquote("plain"), "plain");
    }
}
