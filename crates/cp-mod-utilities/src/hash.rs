//! Deterministic content hashing using FNV-1a.
//!
//! Replaces `sha2` for non-cryptographic content hashing (cache keys,
//! YAML storage keys, change detection). Uses 128-bit FNV-1a for low
//! collision probability with a compact 32-character hex output.

/// FNV-1a 128-bit offset basis (standard value from the spec).
const FNV_OFFSET: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;

/// FNV-1a 128-bit prime (standard value from the spec).
const FNV_PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;

/// Compute a deterministic 128-bit FNV-1a hash of raw bytes.
///
/// Returns a 32-character lowercase hex string.
#[must_use]
pub fn compute(data: &[u8]) -> String {
    let mut h = FNV_OFFSET;
    for &byte in data {
        h ^= u128::from(byte);
        h = h.wrapping_mul(FNV_PRIME);
    }
    format!("{h:032x}")
}

/// Convenience wrapper: hash a string slice.
#[must_use]
pub fn compute_str(s: &str) -> String {
    compute(s.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic() {
        let a = compute(b"hello world");
        let b = compute(b"hello world");
        assert_eq!(a, b, "same input must produce same hash");
    }

    #[test]
    fn different_inputs_differ() {
        let a = compute(b"hello");
        let b = compute(b"world");
        assert_ne!(a, b, "different inputs should produce different hashes");
    }

    #[test]
    fn output_length() {
        let h = compute(b"test");
        assert_eq!(h.len(), 32, "128-bit hash = 32 hex chars");
    }
}
