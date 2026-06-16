//! Agent **heartbeat** record — a fixed-size, in-place liveness beacon.
//!
//! The registry entry (see [`crate::types::registry`]) is written **once** at
//! boot and only rewritten on a status change — it must **not** churn. Liveness
//! therefore rides a separate, tiny file the agent rewrites at a fixed cadence:
//! `<folder>/heartbeat`. The backend reads it to decide whether an agent that
//! *claims* to be running actually is.
//!
//! # Why a fixed-size, in-place record (no rename, no mtime)
//!
//! Freshness is read from the record's **own** `timestamp_ms` field, never from
//! the file's mtime: mtime is coarse, clock-dependent, and lost across some
//! copy/restore paths, whereas an in-band timestamp is exact and survives any
//! transport. The record is a **fixed [`HEARTBEAT_LEN`] bytes**, so the writer
//! `seek(0)` + overwrites it in place every tick — no `tmp`+rename churn (which
//! would thrash the directory and race a concurrent reader) and no append
//! growth. A torn in-place write (a crash mid-overwrite) is caught by the
//! trailing CRC-32C on read and reported as "no valid heartbeat", never as a
//! falsely-fresh one.
//!
//! # Binary layout (little-endian, [`HEARTBEAT_LEN`] = 60 bytes)
//!
//! ```text
//! ┌────────┬───────────────┬──────────┬───────┬───────────────┬────────┐
//! │ ver 4B │ timestamp 8B  │ seq 8B   │ pid 4 │ boot_id 32B   │ crc 4B │
//! └────────┴───────────────┴──────────┴───────┴───────────────┴────────┘
//! ```
//!
//! * **`ver`** — [`HEARTBEAT_SCHEMA_VERSION`]; a reader rejects an unknown one.
//! * **`timestamp_ms`** — wall-clock ms of this beat (the freshness signal).
//! * **`sequence`** — monotonic beat counter (a reader can see progress even if
//!   two reads land within the same millisecond).
//! * **`pid`** — the agent's process id, duplicated here so the record is
//!   self-describing without the registry.
//! * **`boot_id`** — the 32 hex chars minted at boot; binds this heartbeat to
//!   the registry entry's identity and **defeats pid reuse** (a recycled pid
//!   running an unrelated process cannot forge a matching `boot_id`).
//! * **`crc`** — CRC-32C over the preceding 56 bytes (torn-write detection).
//!
//! This module is **I/O-free**: it encodes to / decodes from a byte array; the
//! agent-side writer thread (`cp-mod-bridge`) and the backend reader
//! (`cp-orchestrator`) own the actual file.

use core::fmt;
use core::time::Duration;

// ── constants ──────────────────────────────────────────────────────────

/// Wire-schema revision stamped onto every heartbeat record.
pub const HEARTBEAT_SCHEMA_VERSION: u32 = 1;

/// Number of bytes in a `boot_id` field — 128 random bits as 32 lowercase-hex
/// characters (matches `cp-mod-bridge`'s `mint_boot_id`).
pub const BOOT_ID_LEN: usize = 32;

/// Total fixed size of an encoded heartbeat record, in bytes.
pub const HEARTBEAT_LEN: usize = 60;

/// Default cadence at which the agent rewrites its heartbeat.
pub const DEFAULT_CADENCE: Duration = Duration::from_secs(1);

/// Default maximum age before a heartbeat is considered stale.
///
/// Five times the [`DEFAULT_CADENCE`]: a single missed beat (a GC pause, a
/// briefly-busy disk) does not flap the verdict, but a genuinely hung or dead
/// agent crosses it within a few seconds.
pub const DEFAULT_MAX_AGE: Duration = Duration::from_secs(5);

// ── byte-offsets of each field (documented once, used by encode/decode) ──

/// Offset of the `schema_version` field.
const OFF_VERSION: usize = 0;
/// Offset of the `timestamp_ms` field.
const OFF_TIMESTAMP: usize = 4;
/// Offset of the `sequence` field.
const OFF_SEQUENCE: usize = 12;
/// Offset of the `pid` field.
const OFF_PID: usize = 20;
/// Offset of the `boot_id` field.
const OFF_BOOT_ID: usize = 24;
/// Offset of the trailing CRC (also the length of the CRC-covered prefix).
const OFF_CRC: usize = OFF_BOOT_ID + BOOT_ID_LEN;

// ── errors ─────────────────────────────────────────────────────────────

/// Why a heartbeat record could not be encoded or decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The `boot_id` was not exactly [`BOOT_ID_LEN`] ASCII bytes (encode only).
    BadBootId,

    /// The buffer was not exactly [`HEARTBEAT_LEN`] bytes (decode only).
    BadLength(usize),

    /// The schema version in the record is not one this build understands.
    UnknownVersion(u32),

    /// The trailing CRC did not match the record body (a torn write or
    /// corruption) — the record must be treated as absent.
    CrcMismatch {
        /// CRC stored in the record.
        expected: u32,
        /// CRC recomputed over the body.
        actual: u32,
    },

    /// The `boot_id` bytes were not valid UTF-8 (decode only).
    BootIdNotUtf8,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadBootId => write!(f, "boot_id must be exactly {BOOT_ID_LEN} ASCII bytes"),
            Self::BadLength(n) => {
                write!(f, "heartbeat must be exactly {HEARTBEAT_LEN} bytes, got {n}")
            }
            Self::UnknownVersion(v) => write!(f, "unknown heartbeat schema version {v}"),
            Self::CrcMismatch { expected, actual } => {
                write!(f, "heartbeat CRC mismatch: stored {expected:#010x}, computed {actual:#010x}")
            }
            Self::BootIdNotUtf8 => write!(f, "boot_id bytes are not valid UTF-8"),
        }
    }
}

// ── the record ───────────────────────────────────────────────────────────

/// One decoded heartbeat beat.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Heartbeat {
    /// Schema revision of this record.
    pub schema_version: u32,

    /// Wall-clock milliseconds since the Unix epoch when this beat was written.
    pub timestamp_ms: u64,

    /// Monotonic beat counter since the writer started.
    pub sequence: u64,

    /// The agent's process id.
    pub pid: u32,

    /// The 32-hex-char boot id binding this beat to its registry identity.
    pub boot_id: String,
}

impl Heartbeat {
    /// Encode into a fixed [`HEARTBEAT_LEN`]-byte array.
    ///
    /// # Errors
    ///
    /// Returns [`Error::BadBootId`] if `boot_id` is not exactly
    /// [`BOOT_ID_LEN`] bytes (every minted `boot_id` is, so this only guards a
    /// programming error).
    pub fn encode(&self) -> Result<[u8; HEARTBEAT_LEN], Error> {
        let boot = self.boot_id.as_bytes();
        if boot.len() != BOOT_ID_LEN {
            return Err(Error::BadBootId);
        }

        let mut buf = [0u8; HEARTBEAT_LEN];
        write_u32(&mut buf, OFF_VERSION, self.schema_version);
        write_u64(&mut buf, OFF_TIMESTAMP, self.timestamp_ms);
        write_u64(&mut buf, OFF_SEQUENCE, self.sequence);
        write_u32(&mut buf, OFF_PID, self.pid);
        if let Some(slot) = buf.get_mut(OFF_BOOT_ID..OFF_CRC) {
            slot.copy_from_slice(boot);
        }

        let crc = crc_of(&buf);
        write_u32(&mut buf, OFF_CRC, crc);
        Ok(buf)
    }

    /// Decode a heartbeat from `buf`, validating length, version, and CRC.
    ///
    /// # Errors
    ///
    /// * [`Error::BadLength`] if `buf` is not [`HEARTBEAT_LEN`] bytes.
    /// * [`Error::CrcMismatch`] for a torn or corrupt record.
    /// * [`Error::UnknownVersion`] for a schema this build predates.
    /// * [`Error::BootIdNotUtf8`] if the `boot_id` bytes are not UTF-8.
    pub fn decode(buf: &[u8]) -> Result<Self, Error> {
        if buf.len() != HEARTBEAT_LEN {
            return Err(Error::BadLength(buf.len()));
        }

        let expected = read_u32(buf, OFF_CRC);
        let actual = crc_of(buf);
        if expected != actual {
            return Err(Error::CrcMismatch { expected, actual });
        }

        let schema_version = read_u32(buf, OFF_VERSION);
        if schema_version != HEARTBEAT_SCHEMA_VERSION {
            return Err(Error::UnknownVersion(schema_version));
        }

        let boot_bytes = buf.get(OFF_BOOT_ID..OFF_CRC).unwrap_or(&[]);
        let boot_id =
            core::str::from_utf8(boot_bytes).map_err(|_ignored| Error::BootIdNotUtf8)?;

        Ok(Self {
            schema_version,
            timestamp_ms: read_u64(buf, OFF_TIMESTAMP),
            sequence: read_u64(buf, OFF_SEQUENCE),
            pid: read_u32(buf, OFF_PID),
            boot_id: boot_id.to_owned(),
        })
    }

    /// Whether this beat is recent enough relative to `now_ms`.
    ///
    /// A clock that ran backwards (`timestamp_ms > now_ms`) yields `0` elapsed
    /// via saturating subtraction, so a future-dated beat reads as fresh rather
    /// than wrapping to a huge age.
    #[must_use]
    pub const fn is_fresh(&self, now_ms: u64, max_age_ms: u64) -> bool {
        now_ms.saturating_sub(self.timestamp_ms) <= max_age_ms
    }

    /// Whether this beat was written by the same boot as `boot_id`.
    ///
    /// The pid-reuse defence: a recycled pid running an unrelated process
    /// cannot reproduce the random `boot_id` the dead agent advertised, so a
    /// mismatch marks the registry entry as stale rather than live.
    #[must_use]
    pub fn matches_boot(&self, boot_id: &str) -> bool {
        self.boot_id == boot_id
    }
}

// ── fixed little-endian field codec (explicit, host-independent) ─────────
//
// The byte order is a wire contract, so it is spelled out by hand rather than
// delegated to `to_le_bytes`/`from_le_bytes` (which read as host-dependent to a
// reviewer, and which the workspace forbids for exactly that reason — see
// `framing.rs`).

/// Byte mask isolating the low 8 bits of a `u32`.
const BYTE_MASK_U32: u32 = 0xFF;

/// Byte mask isolating the low 8 bits of a `u64`.
const BYTE_MASK_U64: u64 = 0xFF;

/// Encode a `u32` as four little-endian bytes. Each `& BYTE_MASK_U32` proves
/// the value fits in a `u8`, so the cast is exact.
const fn u32_to_le(value: u32) -> [u8; 4] {
    [
        (value & BYTE_MASK_U32) as u8,
        (value.wrapping_shr(8) & BYTE_MASK_U32) as u8,
        (value.wrapping_shr(16) & BYTE_MASK_U32) as u8,
        (value.wrapping_shr(24) & BYTE_MASK_U32) as u8,
    ]
}

/// Decode four little-endian bytes into a `u32`.
fn u32_from_le(bytes: [u8; 4]) -> u32 {
    let [b0, b1, b2, b3] = bytes;
    u32::from(b0)
        | u32::from(b1).wrapping_shl(8)
        | u32::from(b2).wrapping_shl(16)
        | u32::from(b3).wrapping_shl(24)
}

/// Encode a `u64` as eight little-endian bytes.
const fn u64_to_le(value: u64) -> [u8; 8] {
    [
        (value & BYTE_MASK_U64) as u8,
        (value.wrapping_shr(8) & BYTE_MASK_U64) as u8,
        (value.wrapping_shr(16) & BYTE_MASK_U64) as u8,
        (value.wrapping_shr(24) & BYTE_MASK_U64) as u8,
        (value.wrapping_shr(32) & BYTE_MASK_U64) as u8,
        (value.wrapping_shr(40) & BYTE_MASK_U64) as u8,
        (value.wrapping_shr(48) & BYTE_MASK_U64) as u8,
        (value.wrapping_shr(56) & BYTE_MASK_U64) as u8,
    ]
}

/// Decode eight little-endian bytes into a `u64`.
fn u64_from_le(bytes: [u8; 8]) -> u64 {
    let [b0, b1, b2, b3, b4, b5, b6, b7] = bytes;
    u64::from(b0)
        | u64::from(b1).wrapping_shl(8)
        | u64::from(b2).wrapping_shl(16)
        | u64::from(b3).wrapping_shl(24)
        | u64::from(b4).wrapping_shl(32)
        | u64::from(b5).wrapping_shl(40)
        | u64::from(b6).wrapping_shl(48)
        | u64::from(b7).wrapping_shl(56)
}

/// CRC-32C over the record body — everything before the trailing CRC field.
fn crc_of(buf: &[u8]) -> u32 {
    crc32c::crc32c(buf.get(..OFF_CRC).unwrap_or(buf))
}

/// Write a `u32` little-endian at `offset` (a no-op if `buf` is too short,
/// which cannot happen for the fixed-size records this module builds).
fn write_u32(buf: &mut [u8; HEARTBEAT_LEN], offset: usize, value: u32) {
    if let Some(slot) = buf.get_mut(offset..offset.wrapping_add(4)) {
        slot.copy_from_slice(&u32_to_le(value));
    }
}

/// Write a `u64` little-endian at `offset`.
fn write_u64(buf: &mut [u8; HEARTBEAT_LEN], offset: usize, value: u64) {
    if let Some(slot) = buf.get_mut(offset..offset.wrapping_add(8)) {
        slot.copy_from_slice(&u64_to_le(value));
    }
}

/// Read a `u32` little-endian at `offset`, or `0` if out of range.
fn read_u32(buf: &[u8], offset: usize) -> u32 {
    buf.get(offset..offset.wrapping_add(4))
        .and_then(|s| <[u8; 4]>::try_from(s).ok())
        .map_or(0, u32_from_le)
}

/// Read a `u64` little-endian at `offset`, or `0` if out of range.
fn read_u64(buf: &[u8], offset: usize) -> u64 {
    buf.get(offset..offset.wrapping_add(8))
        .and_then(|s| <[u8; 8]>::try_from(s).ok())
        .map_or(0, u64_from_le)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Heartbeat {
        Heartbeat {
            schema_version: HEARTBEAT_SCHEMA_VERSION,
            timestamp_ms: 1_718_000_000_000,
            sequence: 42,
            pid: 12_345,
            boot_id: "0123456789abcdef0123456789abcdef".to_owned(),
        }
    }

    #[test]
    fn round_trips() {
        let hb = sample();
        let buf = hb.encode().expect("encode");
        assert_eq!(buf.len(), HEARTBEAT_LEN);
        let back = Heartbeat::decode(&buf).expect("decode");
        assert_eq!(hb, back);
    }

    #[test]
    fn rejects_wrong_length() {
        assert_eq!(Heartbeat::decode(&[0u8; 10]), Err(Error::BadLength(10)));
    }

    #[test]
    fn rejects_bad_boot_id_length_on_encode() {
        let mut hb = sample();
        hb.boot_id = "tooshort".to_owned();
        assert_eq!(hb.encode(), Err(Error::BadBootId));
    }

    #[test]
    fn detects_corruption() {
        let mut buf = sample().encode().expect("encode");
        // Flip a byte in the timestamp; the CRC must catch it.
        if let Some(b) = buf.get_mut(OFF_TIMESTAMP) {
            *b ^= 0xFF;
        }
        match Heartbeat::decode(&buf) {
            Err(Error::CrcMismatch { .. }) => {}
            other => panic!("expected CrcMismatch, got {other:?}"),
        }
    }

    #[test]
    fn rejects_unknown_version() {
        let mut hb = sample();
        hb.schema_version = 999;
        let buf = hb.encode().expect("encode");
        assert_eq!(Heartbeat::decode(&buf), Err(Error::UnknownVersion(999)));
    }

    #[test]
    fn freshness_uses_in_band_timestamp() {
        let hb = sample();
        assert!(hb.is_fresh(hb.timestamp_ms, 5_000), "same instant is fresh");
        assert!(hb.is_fresh(hb.timestamp_ms + 5_000, 5_000), "exactly at the bound is fresh");
        assert!(!hb.is_fresh(hb.timestamp_ms + 5_001, 5_000), "just past the bound is stale");
        // A backwards clock saturates to zero elapsed rather than wrapping.
        assert!(hb.is_fresh(hb.timestamp_ms.saturating_sub(10_000), 5_000));
    }

    #[test]
    fn boot_matching_defeats_pid_reuse() {
        let hb = sample();
        assert!(hb.matches_boot("0123456789abcdef0123456789abcdef"));
        assert!(!hb.matches_boot("ffffffffffffffffffffffffffffffff"));
    }

    #[test]
    fn layout_is_exactly_sixty_bytes() {
        // The documented offsets must tile the record with no gap or overlap.
        assert_eq!(OFF_CRC, 56);
        assert_eq!(OFF_CRC + 4, HEARTBEAT_LEN);
    }
}
