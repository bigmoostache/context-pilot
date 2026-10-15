//! Deterministic content hashing using FNV-1a.
//!
//! NOT cryptographic. FNV-1a is fast and well distributed for accidental
//! collisions (cache keys, storage keys, change detection), but anyone who
//! controls the input can forge collisions cheaply. Never use it for
//! integrity, signatures, deduplication of untrusted data or as a
//! security-relevant content address. 128-bit output, 32 hex chars.

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
    fn reference_vectors() {
        assert_eq!(compute(b""), "6c62272e07bb014262b821756295c58d", "empty = offset basis");
        assert_eq!(compute(b"a"), "d228cb696f1a8caf78912b704e4a8964");
        assert_eq!(compute_str("foobar"), "343e1662793c64bf6f0d3597ba446f18");
    }

    #[test]
    fn output_length() {
        let h = compute(b"test");
        assert_eq!(h.len(), 32, "128-bit hash = 32 hex chars");
    }
}
