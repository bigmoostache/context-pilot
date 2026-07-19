//! Oplog **record framing** — length-prefix + CRC-32C integrity.
//!
//! Every oplog record on disk (or on the wire) is wrapped in a fixed
//! 8-byte header followed by the serialised payload:
//!
//! ```text
//! ┌──────────────┬──────────────┬─────────────────┐
//! │ len  (4B LE) │ crc  (4B LE) │ payload (len B) │
//! └──────────────┴──────────────┴─────────────────┘
//! ```
//!
//! * **`len`** — `u32` little-endian, byte length of the payload.
//! * **`crc`** — CRC-32C (Castagnoli) of the payload bytes.
//! * **`payload`** — a JSON-serialised [`OpEntry`](crate::types::oplog::OpEntry).
//!
//! On replay the reader walks the file consuming one frame at a time.
//! A torn tail (partial write, power loss) is detected as either an
//! incomplete header / payload or a CRC mismatch, and the reader stops
//! there — the last valid frame wins (design doc V1).
//!
//! Endianness is encoded by hand (`u32_to_le` / `u32_from_le`) rather than
//! via `to_le_bytes`/`from_le_bytes`: the byte order is a wire contract, so
//! it is made explicit and platform-independent in code, never inherited
//! from the host's native order.
//!
//! This module is **I/O-free**: it operates on byte slices and `Vec<u8>`,
//! never touching the filesystem.  Actual file management lives in the
//! bridge's `OplogWriter` / `OplogReader`.

use core::fmt;

use crate::types::oplog::OpEntry;

// ── constants ──────────────────────────────────────────────────────────

/// Byte size of the frame header (`len` + `crc`, both `u32` LE).
pub const FRAME_HEADER_SIZE: usize = 8;

/// Hard ceiling on a single payload to prevent OOM on a corrupt `len`
/// field.  16 MiB is vastly larger than any realistic oplog entry.
pub const MAX_PAYLOAD_SIZE: u32 = 16 * 1024 * 1024;

/// Byte mask isolating the low 8 bits of an integer.
const BYTE_MASK: u32 = 0xFF;

// ── little-endian codec (explicit, host-independent) ────────────────────

/// Encode a `u32` as four little-endian bytes.
///
/// The byte order is part of the on-disk contract, so it is spelled out
/// rather than delegated to `to_le_bytes` (which would read as
/// host-dependent to a reviewer). Each `& BYTE_MASK` guarantees the value
/// fits in a `u8`, so the cast is exact.
#[expect(
    clippy::as_conversions,
    reason = "const-fn narrowing after `& BYTE_MASK` is provably exact; try_from/From are not const-callable and to_le_bytes is a forbidden host-order shortcut (wire contract)"
)]
const fn u32_to_le(value: u32) -> [u8; 4] {
    [
        (value & BYTE_MASK) as u8,
        (value.wrapping_shr(8) & BYTE_MASK) as u8,
        (value.wrapping_shr(16) & BYTE_MASK) as u8,
        (value.wrapping_shr(24) & BYTE_MASK) as u8,
    ]
}

/// Decode four little-endian bytes into a `u32`.
fn u32_from_le(bytes: [u8; 4]) -> u32 {
    let [b0, b1, b2, b3] = bytes;
    u32::from(b0) | u32::from(b1).wrapping_shl(8) | u32::from(b2).wrapping_shl(16) | u32::from(b3).wrapping_shl(24)
}

// ── errors ─────────────────────────────────────────────────────────────

/// Framing-level decode/encode error.
#[derive(Debug, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_enums,
    reason = "framing error taxonomy is a closed wire contract: every decode failure mode is enumerated and callers match them exhaustively; a new variant is a deliberate breaking change, and #[non_exhaustive] would force cross-crate wildcard arms that the forbidden wildcard_enum_match_arm lint rejects"
)]
pub enum FrameError {
    /// Not enough bytes to read the header or the full payload.
    Incomplete,

    /// The `len` field exceeds [`MAX_PAYLOAD_SIZE`] (or `u32::MAX` on
    /// encode of an oversized payload).
    PayloadTooLarge(u32),

    /// The stored CRC does not match the computed one — the record is
    /// corrupt or was only partially written (torn tail).
    CrcMismatch {
        /// CRC stored in the frame header.
        expected: u32,
        /// CRC computed over the actual payload bytes.
        actual: u32,
    },

    /// JSON deserialisation of the payload failed.
    DeserializeError(String),

    /// JSON serialisation of the entry failed.
    SerializeError(String),
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Incomplete => f.write_str("incomplete frame (truncated)"),
            Self::PayloadTooLarge(n) => {
                write!(f, "payload length {n} exceeds {MAX_PAYLOAD_SIZE}-byte limit")
            }
            Self::CrcMismatch { expected, actual } => {
                write!(f, "CRC mismatch: header {expected:#010x}, computed {actual:#010x}")
            }
            Self::DeserializeError(msg) => write!(f, "deserialize: {msg}"),
            Self::SerializeError(msg) => write!(f, "serialize: {msg}"),
        }
    }
}

// ── raw frame helpers (byte-level, type-agnostic) ──────────────────────

/// A decoded raw frame: the borrowed payload slice plus the total number
/// of bytes the frame occupied (header + payload).
pub type RawFrame<'frame> = (&'frame [u8], usize);

/// Wrap arbitrary `payload` bytes in a `len + crc` frame.
///
/// Returns the complete frame (header + payload) ready to append.
///
/// # Errors
///
/// Returns [`FrameError::PayloadTooLarge`] if the payload exceeds
/// `u32::MAX` bytes (it cannot then carry a valid 32-bit length prefix).
pub fn encode_raw(payload: &[u8]) -> Result<Vec<u8>, FrameError> {
    let len = u32::try_from(payload.len()).map_err(|_ignored| FrameError::PayloadTooLarge(u32::MAX))?;
    let crc = crc32c::crc32c(payload);

    let mut buf = Vec::with_capacity(FRAME_HEADER_SIZE.wrapping_add(payload.len()));
    buf.extend_from_slice(&u32_to_le(len));
    buf.extend_from_slice(&u32_to_le(crc));
    buf.extend_from_slice(payload);
    Ok(buf)
}

/// Extract the raw payload from a framed buffer starting at offset 0.
///
/// On success returns `(payload_slice, total_bytes_consumed)`.  The
/// caller can advance past `total_bytes_consumed` to read the next
/// frame.
///
/// # Errors
///
/// * [`FrameError::Incomplete`] — fewer bytes than the header or
///   payload requires.
/// * [`FrameError::PayloadTooLarge`] — the `len` field exceeds the
///   safety cap.
/// * [`FrameError::CrcMismatch`] — integrity check failed (torn /
///   corrupt).
pub fn decode_raw(buf: &[u8]) -> Result<RawFrame<'_>, FrameError> {
    let len_bytes: [u8; 4] =
        buf.get(0..4).ok_or(FrameError::Incomplete)?.try_into().map_err(|_ignored| FrameError::Incomplete)?;
    let crc_bytes: [u8; 4] =
        buf.get(4..8).ok_or(FrameError::Incomplete)?.try_into().map_err(|_ignored| FrameError::Incomplete)?;

    let len = u32_from_le(len_bytes);
    let expected_crc = u32_from_le(crc_bytes);

    if len > MAX_PAYLOAD_SIZE {
        return Err(FrameError::PayloadTooLarge(len));
    }

    let len_usize = usize::try_from(len).map_err(|_ignored| FrameError::PayloadTooLarge(len))?;
    let total = FRAME_HEADER_SIZE.wrapping_add(len_usize);

    let payload = buf.get(FRAME_HEADER_SIZE..total).ok_or(FrameError::Incomplete)?;
    let actual_crc = crc32c::crc32c(payload);

    if expected_crc != actual_crc {
        return Err(FrameError::CrcMismatch { expected: expected_crc, actual: actual_crc });
    }

    Ok((payload, total))
}

// ── typed frame helpers (OpEntry-aware) ────────────────────────────────

/// Serialise an [`OpEntry`] to JSON and wrap it in a framed record.
///
/// # Errors
///
/// Returns [`FrameError::SerializeError`] if JSON serialisation fails,
/// or [`FrameError::PayloadTooLarge`] for an implausibly huge entry.
pub fn encode_entry(entry: &OpEntry) -> Result<Vec<u8>, FrameError> {
    let payload = serde_json::to_vec(entry).map_err(|e| FrameError::SerializeError(e.to_string()))?;
    encode_raw(&payload)
}

/// Decode one [`OpEntry`] from a framed buffer starting at offset 0.
///
/// Returns `(entry, bytes_consumed)` on success.
///
/// # Errors
///
/// Any [`FrameError`] variant — see [`decode_raw`] for the integrity
/// checks, plus [`FrameError::DeserializeError`] for malformed JSON.
pub fn decode_entry(buf: &[u8]) -> Result<(OpEntry, usize), FrameError> {
    let (payload, consumed) = decode_raw(buf)?;
    let entry: OpEntry = serde_json::from_slice(payload).map_err(|e| FrameError::DeserializeError(e.to_string()))?;
    Ok((entry, consumed))
}

// ── tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::oplog::OpEntryKind;
    use crate::types::{ContentHash, Phase};

    fn sample_entry() -> OpEntry {
        OpEntry {
            schema_version: 1,
            rev: 42,
            timestamp_ms: 1_718_000_000_000,
            kind: OpEntryKind::PhaseTransition { phase: Phase::Streaming },
        }
    }

    #[test]
    fn le_codec_round_trips() {
        for value in [0u32, 1, 0xFF, 0x1234_5678, u32::MAX, MAX_PAYLOAD_SIZE] {
            assert_eq!(u32_from_le(u32_to_le(value)), value);
        }
    }

    #[test]
    fn le_codec_is_little_endian() {
        assert_eq!(u32_to_le(0x0403_0201), [0x01, 0x02, 0x03, 0x04]);
    }

    #[test]
    fn round_trip_entry() {
        let entry = sample_entry();
        let frame = encode_entry(&entry).expect("encode");
        let (decoded, consumed) = decode_entry(&frame).expect("decode");
        assert_eq!(entry, decoded);
        assert_eq!(consumed, frame.len());
    }

    #[test]
    fn round_trip_raw() {
        let payload = b"hello, oplog";
        let frame = encode_raw(payload).expect("encode");
        let (decoded, consumed) = decode_raw(&frame).expect("decode");
        assert_eq!(decoded, payload);
        assert_eq!(consumed, frame.len());
    }

    #[test]
    fn incomplete_header() {
        assert_eq!(decode_raw(&[0, 0, 0]), Err(FrameError::Incomplete));
    }

    #[test]
    fn incomplete_payload() {
        let frame = encode_raw(b"full payload").expect("encode");
        let truncated = frame.get(..frame.len().wrapping_sub(1)).expect("truncate");
        assert_eq!(decode_raw(truncated), Err(FrameError::Incomplete));
    }

    #[test]
    fn corrupted_payload_byte() {
        let mut frame = encode_raw(b"clean data").expect("encode");
        let last = frame.len().wrapping_sub(1);
        if let Some(byte) = frame.get_mut(last) {
            *byte ^= 0xFF;
        }
        match decode_raw(&frame) {
            Err(FrameError::CrcMismatch { .. }) => {}
            other => panic!("expected CrcMismatch, got {other:?}"),
        }
    }

    #[test]
    fn corrupted_crc_bytes() {
        let mut frame = encode_raw(b"good payload").expect("encode");
        if let Some(byte) = frame.get_mut(4) {
            *byte ^= 0x01;
        }
        match decode_raw(&frame) {
            Err(FrameError::CrcMismatch { .. }) => {}
            other => panic!("expected CrcMismatch, got {other:?}"),
        }
    }

    #[test]
    fn payload_too_large() {
        let mut frame = encode_raw(b"x").expect("encode");
        let huge: u32 = MAX_PAYLOAD_SIZE.wrapping_add(1);
        if let Some(slot) = frame.get_mut(..4) {
            slot.copy_from_slice(&u32_to_le(huge));
        }
        assert_eq!(decode_raw(&frame), Err(FrameError::PayloadTooLarge(huge)));
    }

    #[test]
    fn unknown_opentry_kind_tolerant_in_frame() {
        let json = br#"{"schema_version":1,"rev":99,"timestamp_ms":0,"kind":{"kind":"future_event","data":42}}"#;
        let frame = encode_raw(json).expect("encode");
        let (entry, _consumed) = decode_entry(&frame).expect("tolerant decode");
        assert_eq!(entry.kind, OpEntryKind::Unknown);
    }

    #[test]
    fn multi_frame_sequential_decode() {
        let e1 = sample_entry();
        let e2 = OpEntry {
            schema_version: 1,
            rev: 43,
            timestamp_ms: 1_718_000_001_000,
            kind: OpEntryKind::MessageCreated {
                thread_id: "T1".into(),
                message_id: "m1".into(),
                head: ContentHash::new([0xAB; 32]),
                inline_body: None,
            },
        };

        let mut buf = encode_entry(&e1).expect("encode e1");
        buf.extend(encode_entry(&e2).expect("encode e2"));

        let (d1, c1) = decode_entry(&buf).expect("decode e1");
        assert_eq!(d1, e1);

        let rest = buf.get(c1..).expect("remaining bytes");
        let (d2, c2) = decode_entry(rest).expect("decode e2");
        assert_eq!(d2, e2);
        assert_eq!(c1.wrapping_add(c2), buf.len());
    }

    #[test]
    fn empty_buffer() {
        assert_eq!(decode_raw(&[]), Err(FrameError::Incomplete));
    }

    #[test]
    fn frame_error_display() {
        let e = FrameError::CrcMismatch { expected: 0xDEAD_BEEF, actual: 0xCAFE_BABE };
        let s = e.to_string();
        assert!(s.contains("CRC mismatch"));
    }
}
