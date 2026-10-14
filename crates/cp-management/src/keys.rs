//! Delete confirmation keys.
//!
//! Deleting an agent that still has sub-agents needs a key, issued by the
//! refused call. A key is valid for [`TTL_MS`], for any agent of the fleet
//! (process-wide store), and is spent on use.

use std::hash::{BuildHasher as _, Hasher as _};
use std::sync::Mutex;

/// Key validity window (5 minutes).
pub const TTL_MS: u64 = 300_000;

/// Issued keys with their expiry (epoch ms).
static KEYS: Mutex<Vec<(String, u64)>> = Mutex::new(Vec::new());

/// A fresh random key, valid until `now_ms + TTL_MS`.
#[must_use]
pub fn issue(now_ms: u64) -> String {
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(now_ms);
    let key = format!("{:010x}", h.finish() & 0xff_ffff_ffff);
    if let Ok(mut keys) = KEYS.lock() {
        keys.retain(|k| k.1 > now_ms);
        keys.push((key.clone(), now_ms.saturating_add(TTL_MS)));
    }
    key
}

/// Spend `key` if it is still valid. Expired keys are dropped either way.
#[must_use]
pub fn consume(key: &str, now_ms: u64) -> bool {
    let Ok(mut keys) = KEYS.lock() else { return false };
    keys.retain(|k| k.1 > now_ms);
    let before = keys.len();
    keys.retain(|k| k.0 != key);
    keys.len() < before
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_is_single_use() {
        let key = issue(1_000);
        assert!(consume(&key, 2_000), "fresh key accepted");
        assert!(!consume(&key, 2_000), "spent key refused");
    }

    #[test]
    fn key_expires() {
        let key = issue(10_000);
        assert!(!consume(&key, 10_000u64.saturating_add(TTL_MS)), "expired key refused");
    }

    #[test]
    fn unknown_key_refused() {
        assert!(!consume("nope", 0), "never issued");
    }
}
