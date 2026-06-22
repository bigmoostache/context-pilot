//! Phase 21 — exhaustive framing integrity (the V1 torn-tail property at byte
//! granularity).
//!
//! The inline unit tests in `framing.rs` prove the happy path plus a handful of
//! point failures. This suite proves the *property* the oplog's crash-safety
//! rests on: **no truncation or single-byte corruption of a framed buffer ever
//! yields a successfully-decoded frame carrying different content than was
//! written, and no input ever panics.** A frame either decodes byte-identical
//! or it is rejected — there is no third outcome.
//!
//! These are the failures that, undetected, would let a torn tail masquerade as
//! a valid record on replay (design doc V1).

// This integration target links `cp-wire`'s dependencies but exercises only
// its public API, so the per-target `unused-crate-dependencies` lint flags the
// transitive deps the test never names directly. Acknowledge them with the
// canonical `use … as _;` form (Cargo's own suggestion, not a lint silence).
use crc32c as _;
use serde as _;
use serde_json as _;
use sha2 as _;

use cp_wire::framing::{FRAME_HEADER_SIZE, FrameError, decode_entry, decode_raw, encode_entry, encode_raw};
use cp_wire::types::ContentHash;
use cp_wire::types::oplog::{OpEntry, OpEntryKind};

/// A representative multi-field entry whose payload spans the interesting
/// boundaries (string fields, a 32-byte hash, small integers).
fn rich_entry(rev: u64) -> OpEntry {
    let tag = u8::try_from(rev & 0xFF).unwrap_or(0);
    OpEntry {
        schema_version: 1,
        rev,
        timestamp_ms: 1_718_000_000_000_u64.wrapping_add(rev),
        kind: OpEntryKind::MessageCreated {
            thread_id: format!("T{rev}"),
            message_id: format!("m{rev}-body"),
            head: ContentHash::new([tag; 32]),
            inline_body: None,
        },
    }
}

/// Build a buffer of `n` concatenated frames, returning the bytes plus the
/// originals for equality checks.
fn multi_frame_buffer(n: u64) -> (Vec<u8>, Vec<OpEntry>) {
    let mut buf = Vec::new();
    let mut entries = Vec::new();
    for rev in 0..n {
        let entry = rich_entry(rev);
        buf.extend(encode_entry(&entry).expect("encode"));
        entries.push(entry);
    }
    (buf, entries)
}

/// Walk every whole frame a buffer contains, advancing by the reported consumed
/// length. Returns the entries decoded from the clean prefix and the byte
/// offset at which decoding stopped (the start of the first non-decodable
/// frame, or the buffer length if every frame decoded).
fn decode_prefix(buf: &[u8]) -> (Vec<OpEntry>, usize) {
    let mut entries = Vec::new();
    let mut offset = 0usize;
    loop {
        let remaining = buf.get(offset..).unwrap_or(&[]);
        match decode_entry(remaining) {
            Ok((entry, consumed)) => {
                entries.push(entry);
                offset = offset.wrapping_add(consumed);
            }
            Err(_stop) => break,
        }
    }
    (entries, offset)
}

#[test]
fn truncation_at_every_offset_never_panics_and_keeps_clean_prefix() {
    let (buf, entries) = multi_frame_buffer(4);

    // Truncate the buffer at *every* possible length and assert the clean
    // prefix that decodes is always a whole-frame prefix of the originals —
    // never a partial or fabricated frame.
    for cut in 0..=buf.len() {
        let truncated = buf.get(..cut).expect("slice in bounds");
        let (decoded, _stopped) = decode_prefix(truncated);

        assert!(
            decoded.len() <= entries.len(),
            "cut {cut}: decoded more frames ({}) than exist ({})",
            decoded.len(),
            entries.len(),
        );
        for (i, got) in decoded.iter().enumerate() {
            let want = entries.get(i).expect("prefix index valid");
            assert_eq!(got, want, "cut {cut}: frame {i} decoded to wrong content");
        }
    }
}

#[test]
fn truncation_boundaries_decode_exactly_the_completed_frames() {
    // At the exact byte length of k complete frames, exactly k frames decode.
    let (buf, entries) = multi_frame_buffer(5);

    let mut boundary = 0usize;
    for (k, entry) in entries.iter().enumerate() {
        boundary = boundary.wrapping_add(encode_entry(entry).expect("encode").len());
        let prefix = buf.get(..boundary).expect("boundary slice");
        let (decoded, stopped) = decode_prefix(prefix);
        assert_eq!(decoded.len(), k.wrapping_add(1), "expected {} whole frames", k.wrapping_add(1));
        assert_eq!(stopped, boundary, "decoder consumed the whole clean prefix");
    }
}

#[test]
fn single_byte_corruption_at_every_offset_is_detected_or_inert() {
    // Flip every byte of a single frame. The decode must either (a) reject it,
    // or (b) — only for a corruption that lands in a serde-irrelevant spot —
    // still yield the *identical* entry. It must never yield a *different*
    // entry, and never panic.
    let entry = rich_entry(7);
    let clean = encode_entry(&entry).expect("encode");

    for i in 0..clean.len() {
        let mut corrupt = clean.clone();
        if let Some(byte) = corrupt.get_mut(i) {
            *byte ^= 0xFF;
        }
        match decode_entry(&corrupt) {
            Ok((decoded, _consumed)) => {
                assert_eq!(
                    decoded, entry,
                    "byte {i}: corruption produced a DIFFERENT valid entry — silent data corruption",
                );
            }
            Err(_detected) => { /* rejected: the desired outcome */ }
        }
    }
}

#[test]
fn corruption_in_payload_is_always_caught_by_crc() {
    // Every byte of the *payload* region (past the 8-byte header) is CRC-
    // protected, so flipping any of them must surface as CrcMismatch — the
    // integrity guarantee that lets replay trust a frame that decodes.
    let payload = b"a moderately sized oplog payload with structure";
    let frame = encode_raw(payload).expect("encode");

    for i in FRAME_HEADER_SIZE..frame.len() {
        let mut corrupt = frame.clone();
        if let Some(byte) = corrupt.get_mut(i) {
            *byte ^= 0b0010_0000;
        }
        match decode_raw(&corrupt) {
            Err(FrameError::CrcMismatch { .. }) => {}
            other => panic!("payload byte {i}: expected CrcMismatch, got {other:?}"),
        }
    }
}

#[test]
fn trailing_garbage_after_a_valid_frame_does_not_corrupt_the_first_decode() {
    // A valid frame followed by arbitrary noise still decodes cleanly and
    // reports the exact consumed length, so a reader can resume past it.
    let entry = rich_entry(3);
    let mut buf = encode_entry(&entry).expect("encode");
    let clean_len = buf.len();
    buf.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x01]);

    let (decoded, consumed) = decode_entry(&buf).expect("first frame decodes");
    assert_eq!(decoded, entry);
    assert_eq!(consumed, clean_len, "consumed exactly the valid frame, not the garbage");
}

#[test]
fn subheader_buffers_are_incomplete_never_panic() {
    for len in 0..FRAME_HEADER_SIZE {
        let buf = vec![0u8; len];
        assert_eq!(decode_raw(&buf), Err(FrameError::Incomplete), "len {len} must be Incomplete");
    }
}

#[test]
fn oversized_length_prefix_is_rejected_not_allocated() {
    // A corrupt length field claiming u32::MAX must be rejected without
    // attempting a giant allocation. All-0xFF prefix == u32::MAX > the cap.
    let mut frame = encode_raw(b"tiny").expect("encode");
    if let Some(slot) = frame.get_mut(..4) {
        slot.copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
    }
    assert_eq!(decode_raw(&frame), Err(FrameError::PayloadTooLarge(u32::MAX)));
}

#[test]
fn zero_length_payload_round_trips() {
    let frame = encode_raw(&[]).expect("encode empty");
    let (payload, consumed) = decode_raw(&frame).expect("decode empty");
    assert!(payload.is_empty(), "empty payload preserved");
    assert_eq!(consumed, FRAME_HEADER_SIZE, "header-only frame");
}
