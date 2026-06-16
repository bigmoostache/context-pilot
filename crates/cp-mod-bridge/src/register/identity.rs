//! Agent identity primitives minted at boot: the stable folder id, the
//! per-boot id, and the command capability token.
//!
//! * The **id** is a stable, collision-resistant digest of the agent's
//!   canonical folder path (FNV-1a, 64-bit, lowercase hex). It names the
//!   registry file `~/.context-pilot/agents/<id>.json`; the backend reads the
//!   `id` field from that file and never recomputes it, so the only requirement
//!   is determinism per path (design doc §10).
//! * The **`boot_id`** is 128 fresh random bits, minted once per process start.
//!   Liveness (Phase 11) compares it against a re-used pid so a recycled pid
//!   cannot masquerade as a still-running agent (defeats pid reuse, design doc
//!   §10 / D11).
//! * The **`cap_token`** is 256 fresh random bits — the bearer secret a
//!   commander must present (design doc I9). It is written to the `0600`
//!   registry file and never logged.
//!
//! Randomness comes from `/dev/urandom`, mirroring `cp-mod-search`'s Meili
//! master-key generation — no `rand`/`getrandom` dependency, and the kernel CSPRNG
//! is the right source for a security token.

use std::fs::File;
use std::io::Read as _;

use crate::error::{Error, BootResult};

/// FNV-1a 64-bit offset basis.
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64-bit prime.
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Bytes of randomness behind a [`cap_token`] (256 bits).
const CAP_TOKEN_BYTES: usize = 32;

/// Bytes of randomness behind a [`boot_id`] (128 bits).
const BOOT_ID_BYTES: usize = 16;

/// Stable lowercase-hex FNV-1a digest of `path` — the agent's registry id.
///
/// FNV-1a is not cryptographic, but the id is a *naming* key, not a secret:
/// it only needs to be deterministic for a given canonical path and unlikely
/// to collide across the handful of agents on one machine.
#[must_use]
pub fn folder_id(path: &str) -> String {
    let mut hash = FNV_OFFSET;
    for byte in path.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}")
}

/// Mint a fresh 256-bit capability token as 64 lowercase-hex chars.
///
/// # Errors
///
/// Returns [`Error::Io`] if `/dev/urandom` cannot be read.
pub fn mint_cap_token() -> BootResult<String> {
    random_hex::<CAP_TOKEN_BYTES>("mint cap_token")
}

/// Mint a fresh 128-bit boot id as 32 lowercase-hex chars.
///
/// # Errors
///
/// Returns [`Error::Io`] if `/dev/urandom` cannot be read.
pub fn mint_boot_id() -> BootResult<String> {
    random_hex::<BOOT_ID_BYTES>("mint boot_id")
}

/// Read `N` bytes from `/dev/urandom` and hex-encode them.
fn random_hex<const N: usize>(context: &str) -> BootResult<String> {
    let mut buf = [0u8; N];
    let mut file =
        File::open("/dev/urandom").map_err(|e| Error::io(format!("open urandom for {context}"), e))?;
    file.read_exact(&mut buf).map_err(|e| Error::io(format!("read urandom for {context}"), e))?;

    let mut hex = String::with_capacity(N.wrapping_mul(2));
    for byte in buf {
        use std::fmt::Write as _;
        // `write!` to a `String` is infallible — the result is always `Ok`.
        let _ignored = write!(hex, "{byte:02x}");
    }
    Ok(hex)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_id_is_deterministic_and_hex() {
        let a = folder_id("/home/user/project");
        let b = folder_id("/home/user/project");
        assert_eq!(a, b, "same path yields the same id");
        assert_eq!(a.len(), 16, "64-bit digest is 16 hex chars");
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()), "id is lowercase hex");
    }

    #[test]
    fn folder_id_distinguishes_paths() {
        assert_ne!(folder_id("/a"), folder_id("/b"));
    }

    #[test]
    fn cap_token_is_64_hex_chars_and_fresh() {
        let a = mint_cap_token().expect("mint a");
        let b = mint_cap_token().expect("mint b");
        assert_eq!(a.len(), 64, "256 bits is 64 hex chars");
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b, "two mints must differ (random source)");
    }

    #[test]
    fn boot_id_is_32_hex_chars_and_fresh() {
        let a = mint_boot_id().expect("mint a");
        let b = mint_boot_id().expect("mint b");
        assert_eq!(a.len(), 32, "128 bits is 32 hex chars");
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }
}
