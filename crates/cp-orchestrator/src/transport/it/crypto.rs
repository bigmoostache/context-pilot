//! SHA-256 + base64 helpers for the CA fingerprint (M4).
//!
//! The maintenance plane must report the private CA root's SHA-256 fingerprint
//! so the operator can verify it out-of-band — and it must match
//! `openssl x509 -fingerprint -sha256`, i.e. the digest of the certificate's
//! **DER** bytes. Computing that needs base64-decoding the PEM body to DER and a
//! SHA-256.
//!
//! These are thin wrappers over the `sha2` and `base64` crates (already direct
//! dependencies for PKCE), keeping a small stable surface (`sha256`,
//! `base64_decode`, `colon_hex_upper`) for [`super::ca`] and
//! [`super::network`].

use base64::Engine as _;
use sha2::{Digest as _, Sha256};

/// Compute the SHA-256 digest of `data`.
#[must_use]
pub(crate) fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

/// Decode standard base64 (ignoring embedded whitespace/newlines, as PEM bodies
/// are line-wrapped). Returns `None` on any invalid character.
#[must_use]
pub(crate) fn base64_decode(input: &str) -> Option<Vec<u8>> {
    let stripped: String = input.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    base64::engine::general_purpose::STANDARD.decode(stripped.as_bytes()).ok()
}

/// Uppercase hex with colon separators, matching `openssl … -fingerprint`
/// output (e.g. `AB:CD:…`).
#[must_use]
pub(crate) fn colon_hex_upper(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len().saturating_mul(3));
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 {
            s.push(':');
        }
        let _written = write!(s, "{b:02X}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_known_vectors() {
        // NIST test vectors.
        assert_eq!(
            colon_hex_upper(&sha256(b"abc")).replace(':', "").to_lowercase(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            colon_hex_upper(&sha256(b"")).replace(':', "").to_lowercase(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn base64_round_trips_known_values() {
        assert_eq!(base64_decode("aGVsbG8=").as_deref(), Some(&b"hello"[..]));
        assert_eq!(base64_decode("TWFu").as_deref(), Some(&b"Man"[..]));
        // Whitespace/newlines are ignored (PEM bodies are wrapped).
        assert_eq!(base64_decode("aGVs\nbG8=").as_deref(), Some(&b"hello"[..]));
        // Invalid character.
        assert!(base64_decode("not base64!").is_none());
    }

    #[test]
    fn colon_hex_is_openssl_shaped() {
        assert_eq!(colon_hex_upper(&[0xab, 0x01, 0xff]), "AB:01:FF");
        assert_eq!(colon_hex_upper(&[]), "");
    }
}
