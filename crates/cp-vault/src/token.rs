//! Shared secret that gates `GET /api/vault/snapshot`.
//!
//! The snapshot returns every credential in clear, so it must never be served
//! on "is the caller a user?" alone: with access control off every caller is
//! anonymous. The orchestrator creates a random token at boot
//! ([`ensure`]); agents it spawns read the same file ([`read`]) and present it
//! as a `Bearer` header. Same-host trust is the filesystem (`0600`, owner's
//! `~/.context-pilot`). A remote agent needs the file provisioned out of band.

use std::fmt::Write as _;
use std::fs;
use std::io::Read as _;
use std::path::PathBuf;

use crate::secure_fs::write_private_atomic;
use crate::types::{SecretString, VaultError};

/// Token file name relative to `~/.context-pilot/`.
const TOKEN_FILENAME: &str = "vault-bridge.token";

/// Random bytes per token (hex-encoded: twice as many chars).
const TOKEN_BYTES: usize = 32;

/// Shortest token [`verify`] accepts, so an empty or truncated file never
/// authorizes anything.
const MIN_TOKEN_CHARS: usize = 32;

/// Path of the token file.
fn token_path() -> PathBuf {
    if cfg!(test) {
        return std::env::temp_dir().join(format!("cp-vault-test-{}.token", std::process::id()));
    }
    cp_env::env().core.home.join(".context-pilot").join(TOKEN_FILENAME)
}

/// Read the token, if the file exists and holds a usable one.
#[must_use]
pub fn read() -> Option<SecretString> {
    let raw = fs::read_to_string(token_path()).ok()?;
    let token = raw.trim();
    (token.len() >= MIN_TOKEN_CHARS).then(|| SecretString::new(token.to_owned()))
}

/// Return the existing token or mint and persist a new one (orchestrator boot).
///
/// # Errors
///
/// [`VaultError::Io`] if no randomness source is available or the file cannot
/// be written; the snapshot then stays closed (every request is refused).
pub fn ensure() -> Result<SecretString, VaultError> {
    if let Some(existing) = read() {
        return Ok(existing);
    }
    let token = SecretString::new(random_hex()?);
    write_private_atomic(&token_path(), token.expose().as_bytes())?;
    Ok(token)
}

/// Constant-time check of a presented bearer token against the token file.
///
/// `false` when no token exists, so a missing file fails closed.
#[must_use]
pub fn verify(presented: Option<&str>) -> bool {
    let (Some(expected), Some(given)) = (read(), presented) else { return false };
    constant_time_eq(expected.expose().as_bytes(), given.as_bytes())
}

/// Byte comparison whose duration does not depend on where inputs differ.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// [`TOKEN_BYTES`] from the OS CSPRNG, hex-encoded.
fn random_hex() -> Result<String, VaultError> {
    let mut bytes = [0u8; TOKEN_BYTES];
    fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| VaultError::Io(format!("no randomness source: {e}")))?;
    let mut hex = String::with_capacity(TOKEN_BYTES * 2);
    for b in bytes {
        let _infallible = write!(hex, "{b:02x}");
    }
    Ok(hex)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_is_stable_and_verifies() {
        let first = ensure().unwrap();
        let second = ensure().unwrap();
        assert_eq!(first.expose(), second.expose());
        assert_eq!(first.expose().len(), TOKEN_BYTES * 2);
        assert!(verify(Some(first.expose())));
        assert!(!verify(Some("wrong")));
        assert!(!verify(None));
    }

    #[test]
    fn constant_time_eq_rejects_length_and_content_mismatch() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
    }
}
