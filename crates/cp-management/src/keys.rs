//! Delete confirmation keys.
//!
//! Deleting an agent whose subtree has more than one agent, or a busy agent,
//! needs a key issued by the refused call. A key is bound to the
//! `(caller, target)` pair it was issued for, valid for [`TTL_MS`] and spent
//! on use: it cannot confirm any other delete.

use std::hash::{BuildHasher as _, Hasher as _};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// Key validity window (5 minutes).
pub const TTL_MS: u64 = 300_000;

/// One issued key.
struct Issued {
    /// The key text handed to the caller.
    key: String,
    /// Agent the key was issued to.
    caller: String,
    /// Agent whose deletion it confirms.
    target: String,
    /// Expiry, epoch ms.
    expires_ms: u64,
}

/// Issued keys (process-wide: one store for every agent of the fleet).
static KEYS: Mutex<Vec<Issued>> = Mutex::new(Vec::new());

/// Distinguishes keys issued within the same millisecond.
static NONCE: AtomicU64 = AtomicU64::new(0);

/// A fresh random key for `caller` deleting `target`, valid until
/// `now_ms + TTL_MS`.
#[must_use]
pub fn issue(caller: &str, target: &str, now_ms: u64) -> String {
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(now_ms);
    h.write_u64(NONCE.fetch_add(1, Ordering::Relaxed));
    let key = format!("{:016x}", h.finish());
    if let Ok(mut keys) = KEYS.lock() {
        keys.retain(|k| k.expires_ms > now_ms);
        keys.push(Issued {
            key: key.clone(),
            caller: caller.to_owned(),
            target: target.to_owned(),
            expires_ms: now_ms.saturating_add(TTL_MS),
        });
    }
    key
}

/// Spend `key` if it is still valid and was issued for exactly this
/// `(caller, target)`. Expired keys are dropped either way; a key presented
/// for another pair stays unspent.
#[must_use]
pub fn consume(key: &str, caller: &str, target: &str, now_ms: u64) -> bool {
    let Ok(mut keys) = KEYS.lock() else { return false };
    keys.retain(|k| k.expires_ms > now_ms);
    let before = keys.len();
    keys.retain(|k| !(k.key == key && k.caller == caller && k.target == target));
    keys.len() < before
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_is_single_use() {
        let key = issue("A1", "A2", 1_000);
        assert!(consume(&key, "A1", "A2", 2_000), "fresh key accepted");
        assert!(!consume(&key, "A1", "A2", 2_000), "spent key refused");
    }

    #[test]
    fn key_expires() {
        let key = issue("A1", "A2", 10_000);
        assert!(!consume(&key, "A1", "A2", 10_000u64.saturating_add(TTL_MS)), "expired key refused");
    }

    #[test]
    fn unknown_key_refused() {
        assert!(!consume("nope", "A1", "A2", 0), "never issued");
    }

    #[test]
    fn key_bound_to_target_and_caller() {
        let key = issue("A1", "A2", 1_000);
        assert!(!consume(&key, "A1", "A3", 1_500), "other target refused");
        assert!(!consume(&key, "A9", "A2", 1_500), "other caller refused");
        assert!(consume(&key, "A1", "A2", 1_500), "misuse does not spend the key");
    }

    #[test]
    fn keys_are_distinct_within_a_millisecond() {
        assert_ne!(issue("A1", "A2", 5_000), issue("A1", "A2", 5_000));
    }
}
