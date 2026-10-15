//! `.env` file read/write utilities with advisory file locking.
//!
//! Handles reading and persisting credentials to `~/.context-pilot/.env`.
//! Readers and writers (orchestrator + N agents) serialize on a sidecar
//! `.env.lock` (`flock` via `fs2`): shared for reads, exclusive for writes.
//! The lock lives on a sidecar, not on `.env` itself, because writes replace
//! `.env` by `rename` (see [`write_private_atomic`]) and a lock held on the old
//! inode would no longer guard the new file.

use std::fs;
use std::path::{Path, PathBuf};

use fs2::FileExt as _;

use crate::secure_fs::write_private_atomic;
use crate::types::VaultError;

/// Path to the global environment file: `~/.context-pilot/.env`.
///
/// Test builds use a per-process temp file instead: `Vault::set` tests used to
/// clobber the real file (e.g. `BRAVE_API_KEY=key-123`) on every
/// `cargo test --workspace`.
fn global_env_path() -> PathBuf {
    if cfg!(test) {
        return std::env::temp_dir().join(format!("cp-vault-test-{}.env", std::process::id()));
    }
    cp_env::env().core.home.join(".context-pilot").join(".env")
}

/// Open (creating if needed) the sidecar lock file next to `env_path`.
fn open_lock(env_path: &Path) -> Result<fs::File, VaultError> {
    let lock_path = env_path.with_extension("lock");
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent).map_err(|e| VaultError::Io(format!("cannot create dir: {e}")))?;
    }
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|e| VaultError::Io(format!("cannot open {}: {e}", lock_path.display())))
}

/// Read a specific key from the global `.env` file (without dotenvy).
///
/// Fallback for keys dotenvy did not load (e.g. added after boot). Takes the
/// shared lock so it never sees a writer mid-flight.
pub(crate) fn read_env_key(key: &str) -> Option<String> {
    let path = global_env_path();
    let lock = open_lock(&path).ok()?;
    lock.lock_shared().ok()?;
    let content = fs::read_to_string(&path).ok();
    drop(lock);
    find_value(&content?, key)
}

/// Value of `key` in `.env` text: last-wins is not needed, first match wins
/// like the writer's in-place update.
fn find_value(content: &str, key: &str) -> Option<String> {
    content.lines().find_map(|line| line_value(line, key))
}

/// Value of `line` when it assigns `key` (`KEY=v` or `export KEY=v`).
fn line_value(line: &str, key: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let body = trimmed.strip_prefix("export ").map_or(trimmed, str::trim_start);
    let raw = body.strip_prefix(key)?.strip_prefix('=')?;
    Some(unquote(raw))
}

/// Persist a key-value pair to `~/.context-pilot/.env`.
///
/// Under the exclusive sidecar lock: read, update the entry in place (or
/// append it), then replace the file atomically, owner-only from creation. A
/// crash leaves the old or the new file, never a truncated one.
pub(crate) fn write_env_entry(key: &str, value: &str) -> Result<(), VaultError> {
    let path = global_env_path();
    let lock = open_lock(&path)?;
    lock.lock_exclusive().map_err(|e| VaultError::Io(format!("flock failed: {e}")))?;

    let existing = fs::read_to_string(&path).unwrap_or_default();
    let content = zeroize::Zeroizing::new(upsert_line(&existing, key, value));
    let result = write_private_atomic(&path, content.as_bytes());

    // Lock is released when `lock` is dropped.
    drop(lock);
    result
}

/// `existing` with `key` set to `value`: replaced in place, else appended.
fn upsert_line(existing: &str, key: &str, value: &str) -> String {
    let new_line = format_env_line(key, value);
    let mut found = false;
    let mut lines: Vec<String> = existing
        .lines()
        .map(|line| {
            if !found && line_value(line, key).is_some() {
                found = true;
                new_line.clone()
            } else {
                line.to_owned()
            }
        })
        .collect();

    if !found {
        lines.push(new_line);
    }

    let mut content = lines.join("\n");
    content.push('\n');
    content
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

/// Parse a raw `.env` value: quoted values are unescaped (anything after the
/// closing quote is ignored), unquoted values drop an inline ` #` comment.
fn unquote(s: &str) -> String {
    let trimmed = s.trim();
    if let Some(rest) = trimmed.strip_prefix('"') {
        return closing_quote(rest, '"').map_or_else(|| trimmed.to_owned(), unescape);
    }
    if let Some(rest) = trimmed.strip_prefix('\'') {
        // Single quotes are literal: no escapes.
        return closing_quote(rest, '\'').map_or_else(|| trimmed.to_owned(), str::to_owned);
    }
    let value = trimmed.find(" #").map_or(trimmed, |at| trimmed.get(..at).unwrap_or(trimmed));
    value.trim_end().to_owned()
}

/// Text before the first unescaped `quote` in `rest`, or `None` if unterminated.
fn closing_quote(rest: &str, quote: char) -> Option<&str> {
    let mut escaped = false;
    for (at, c) in rest.char_indices() {
        match (escaped, c) {
            (true, _) => escaped = false,
            (false, '\\') if quote == '"' => escaped = true,
            (false, ch) if ch == quote => return rest.get(..at),
            _ => {}
        }
    }
    None
}

/// Undo [`format_env_line`]'s escaping.
fn unescape(inner: &str) -> String {
    inner.replace("\\\"", "\"").replace("\\\\", "\\")
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

    #[test]
    fn unquote_strips_inline_comment_only_when_unquoted() {
        assert_eq!(unquote("abc # note"), "abc");
        assert_eq!(unquote("\"abc # kept\" # note"), "abc # kept");
        assert_eq!(unquote("a#b"), "a#b");
    }

    #[test]
    fn unquote_single_quotes_are_literal() {
        assert_eq!(unquote("'a\\\"b'"), "a\\\"b");
    }

    #[test]
    fn find_value_handles_export_and_comments() {
        let text = "# KEY=commented\nexport KEY=v1\nKEYX=other\n";
        assert_eq!(find_value(text, "KEY"), Some("v1".to_owned()));
        assert_eq!(find_value(text, "KEYX"), Some("other".to_owned()));
        assert_eq!(find_value(text, "MISSING"), None);
    }

    #[test]
    fn upsert_replaces_in_place_and_appends() {
        let text = "A=1\nexport B=2\n";
        assert_eq!(upsert_line(text, "B", "3"), "A=1\nB=3\n");
        assert_eq!(upsert_line(text, "C", "4"), "A=1\nexport B=2\nC=4\n");
        assert_eq!(upsert_line("", "C", "4"), "C=4\n");
    }

    /// Regression: the write used to truncate in place (crash = empty `.env`)
    /// and left the file at umask permissions until a trailing chmod.
    #[test]
    fn write_then_read_round_trips_owner_only() {
        write_env_entry("CP_VAULT_TEST_KEY", "first value").unwrap();
        write_env_entry("CP_VAULT_TEST_KEY", "second").unwrap();
        assert_eq!(read_env_key("CP_VAULT_TEST_KEY"), Some("second".to_owned()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = fs::metadata(global_env_path()).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }
}
