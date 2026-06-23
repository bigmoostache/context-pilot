//! Shared low-level helpers for the auth subsystem — random bytes, hex
//! encoding, UUID formatting, and clock.

use std::time::{SystemTime, UNIX_EPOCH};

/// Fill `buf` with bytes from `/dev/urandom`.  Falls back to a nanosecond
/// clock spread on read failure (degraded but never panicking — mirrors
/// `transport::ticket::random_token`).
pub(super) fn fill_random(buf: &mut [u8]) {
    if std::fs::File::open("/dev/urandom")
        .and_then(|mut file| std::io::Read::read_exact(&mut file, buf))
        .is_err()
    {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0u128, |dur| dur.as_nanos());
        for (idx, slot) in buf.iter_mut().enumerate() {
            let shift = u32::try_from((idx % 16).saturating_mul(8)).unwrap_or(0);
            *slot = u8::try_from(seed.wrapping_shr(shift) & 0xff).unwrap_or(0);
        }
    }
}

/// Lowercase-hex encode a byte slice.
pub(super) fn to_hex(bytes: &[u8]) -> String {
    use core::fmt::Write as _;
    let mut hex = String::with_capacity(bytes.len().saturating_mul(2));
    for &byte in bytes {
        let _ok = write!(hex, "{byte:02x}");
    }
    hex
}

/// Generate `n` random bytes and return their hex encoding.
pub(super) fn random_hex(n_bytes: usize) -> String {
    let mut buf = vec![0u8; n_bytes];
    fill_random(&mut buf);
    to_hex(&buf)
}

/// Format 16 bytes as a UUID v4 string with dashes.
pub(super) fn format_uuid(bytes: &[u8; 16]) -> String {
    // xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
    // 0..4     4..6  6..8  8..10 10..16
    let hex = to_hex(bytes);
    let chars: Vec<char> = hex.chars().collect();
    let mut out = String::with_capacity(36);
    for (idx, ch) in chars.iter().enumerate() {
        if idx == 8 || idx == 12 || idx == 16 || idx == 20 {
            out.push('-');
        }
        out.push(*ch);
    }
    out
}

/// Milliseconds since the Unix epoch, saturating at 0 on a pre-epoch clock.
pub(super) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |dur| u64::try_from(dur.as_millis()).unwrap_or(u64::MAX))
}
