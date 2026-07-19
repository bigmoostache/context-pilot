//! Wire-protocol types shared between the agent bridge and the orchestrator.
//!
//! Every type carries a `schema_version` so receivers can detect forward drift
//! before attempting full decode.  Enums use `#[serde(tag = "kind")]` with a
//! catch-all `Unknown` variant (`#[serde(other)]`) so an N-1 receiver
//! gracefully round-trips a variant it has never seen.

pub mod ack;
pub mod body;
pub mod command;
pub mod oplog;
pub mod registry;
pub mod snapshot;
pub mod stream;

use serde::{Deserialize, Serialize};

// ── shared primitives ───────────────────────────────────────────────────

/// Content-addressed body reference (SHA-256, 32 bytes).
///
/// Serialised as a 64-char lowercase hex string so that JSON fixtures stay
/// human-readable and the hash can round-trip through any text-based format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ContentHash(
    /// Raw SHA-256 digest.
    [u8; 32],
);

impl ContentHash {
    /// Wrap a raw 32-byte digest.
    #[must_use]
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The underlying bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// The SHA-256 content hash of `bytes` — the canonical content address.
    ///
    /// Both the agent (which stores bodies) and the backend (which hydrates
    /// them by hash) compute this, so it must be a fixed, collision-resistant
    /// digest, not the non-cryptographic FNV used for folder *naming*. The
    /// digest is always exactly 32 bytes, so the copy is infallible.
    #[must_use]
    pub fn of(bytes: &[u8]) -> Self {
        use sha2::{Digest as _, Sha256};
        let digest = Sha256::digest(bytes);
        let mut out = [0u8; 32];
        out.copy_from_slice(digest.as_slice());
        Self(out)
    }

    /// Lowercase-hex rendering of the digest (64 chars) — the on-disk filename
    /// of a spilled body and the same form [`Serialize`] emits.
    #[must_use]
    pub fn to_hex(self) -> String {
        use core::fmt::Write as _;
        let mut hex = String::with_capacity(64);
        for &byte in &self.0 {
            // `write!` on `String` is infallible — discard the always-Ok result.
            _ = write!(hex, "{byte:02x}");
        }
        hex
    }

    /// Parse a 64-char lowercase/uppercase hex string into a hash.
    ///
    /// Returns `None` if the length is wrong or any character is not hex — the
    /// inverse of [`to_hex`](Self::to_hex). Used by the transport layer to turn
    /// a `/body/{hash}` path segment back into a content address.
    #[must_use]
    pub fn from_hex(s: &str) -> Option<Self> {
        if s.len() != 64 {
            return None;
        }
        let mut bytes = [0u8; 32];
        for (i, chunk) in s.as_bytes().chunks_exact(2).enumerate() {
            let hi = hex_nibble(*chunk.first()?)?;
            let lo = hex_nibble(*chunk.get(1)?)?;
            *bytes.get_mut(i)? = hi.wrapping_shl(4) | lo;
        }
        Some(Self(bytes))
    }
}

/// Hex-encode for JSON/YAML readability.
impl Serialize for ContentHash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for ContentHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: serde::Deserializer<'de> {
        let s = <String as Deserialize>::deserialize(deserializer)?;
        if s.len() != 64 {
            return Err(serde::de::Error::invalid_length(s.len(), &"64 hex chars"));
        }
        let mut bytes = [0u8; 32];
        for (i, chunk) in s.as_bytes().chunks_exact(2).enumerate() {
            let hi = chunk
                .first()
                .copied()
                .and_then(hex_nibble)
                .ok_or_else(|| serde::de::Error::custom("invalid hex char"))?;
            let lo = chunk
                .get(1)
                .copied()
                .and_then(hex_nibble)
                .ok_or_else(|| serde::de::Error::custom("invalid hex char"))?;
            if let Some(slot) = bytes.get_mut(i) {
                *slot = hi.wrapping_shl(4) | lo;
            }
        }
        Ok(Self(bytes))
    }

    fn deserialize_in_place<D>(deserializer: D, place: &mut Self) -> Result<(), D::Error> where D: serde::Deserializer<'de> {
        *place = Self::deserialize(deserializer)?;
        Ok(())
    }
}

/// Decode a single hex ASCII byte to its 4-bit value.
const fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b.wrapping_sub(b'0')),
        b'a'..=b'f' => Some(b.wrapping_sub(b'a').wrapping_add(10)),
        b'A'..=b'F' => Some(b.wrapping_sub(b'A').wrapping_add(10)),
        _ => None,
    }
}

// ── Phase (shared between oplog + stream) ───────────────────────────────

/// Agent execution phase — the three states visible to observers.
///
/// The authoritative record lives in the oplog (tier ①); the stream plane
/// carries a `PhaseHint` for low-latency display that self-heals from the
/// oplog on any drop (design doc I10/K6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// No LLM call or tool execution in progress.
    Idle,
    /// An LLM response is being streamed.
    Streaming,
    /// One or more tool calls are executing.
    Tooling,
}

/// Lifecycle state observable by the backend (oplog-recorded, not just a
/// heartbeat inference).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum LifecycleState {
    /// Agent is initialising (bridge boot sequence).
    Starting,
    /// Fully operational.
    Running,
    /// Graceful shutdown requested.
    Stopping,
    /// Clean exit completed.
    Stopped,
}

/// Whose turn it is on a thread — the wire mirror of the threads module's
/// `ThreadStatus`.
///
/// Carried by [`OpEntryKind::ThreadCreated`](oplog::OpEntryKind::ThreadCreated)
/// and [`ThreadStatusChanged`](oplog::OpEntryKind::ThreadStatusChanged) so the
/// backend's materialized roster knows, without a disk read, whether a thread
/// is waiting on the human (`MyTurn`) or owned by the agent (`TheirTurn`).
/// Defined here in the I/O-free protocol crate rather than imported from
/// `cp-mod-threads` to keep the layering one-directional (modules depend on the
/// wire, never the reverse).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum ThreadTurn {
    /// The thread is waiting on the human — it needs a user response.
    MyTurn,
    /// The agent owns the thread (working it / will respond).
    TheirTurn,
    /// A turn value from a newer protocol version (N-1 forward-compat).
    #[serde(other)]
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_hash_round_trip() {
        let bytes: [u8; 32] = core::array::from_fn(|i| i as u8);
        let hash = ContentHash::new(bytes);
        let json = serde_json::to_string(&hash).expect("serialize");
        let back: ContentHash = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(hash, back);
    }

    #[test]
    fn content_hash_of_matches_known_vectors() {
        // Canonical SHA-256 test vectors.
        assert_eq!(ContentHash::of(b"").to_hex(), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",);
        assert_eq!(
            ContentHash::of(b"abc").to_hex(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        );
    }

    #[test]
    fn content_hash_to_hex_matches_serialize() {
        let hash = ContentHash::of(b"some body bytes");
        let json = serde_json::to_string(&hash).expect("serialize");
        assert_eq!(json, format!("\"{}\"", hash.to_hex()));
    }

    #[test]
    fn content_hash_hex_format() {
        let hash = ContentHash::new([0xab; 32]);
        let json = serde_json::to_string(&hash).expect("serialize");
        let expected = format!("\"{}\"", "ab".repeat(32));
        assert_eq!(json, expected);
    }

    #[test]
    fn content_hash_from_hex_round_trips() {
        let hash = ContentHash::of(b"round trip me");
        let parsed = ContentHash::from_hex(&hash.to_hex()).expect("valid hex parses");
        assert_eq!(hash, parsed);
    }

    #[test]
    fn content_hash_from_hex_rejects_bad_input() {
        assert!(ContentHash::from_hex("too short").is_none(), "wrong length");
        assert!(ContentHash::from_hex(&"zz".repeat(32)).is_none(), "non-hex chars");
    }

    #[test]
    fn content_hash_rejects_short_hex() {
        let result = serde_json::from_str::<ContentHash>("\"abcd\"");
        assert!(result.is_err());
    }

    #[test]
    fn phase_round_trip() {
        for phase in [Phase::Idle, Phase::Streaming, Phase::Tooling] {
            let json = serde_json::to_string(&phase).expect("serialize");
            let back: Phase = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(phase, back);
        }
    }

    #[test]
    fn lifecycle_round_trip() {
        for state in
            [LifecycleState::Starting, LifecycleState::Running, LifecycleState::Stopping, LifecycleState::Stopped]
        {
            let json = serde_json::to_string(&state).expect("serialize");
            let back: LifecycleState = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(state, back);
        }
    }

    #[test]
    fn thread_turn_round_trip() {
        for turn in [ThreadTurn::MyTurn, ThreadTurn::TheirTurn] {
            let json = serde_json::to_string(&turn).expect("serialize");
            let back: ThreadTurn = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(turn, back);
        }
        // snake_case wire spelling is stable (the frontend depends on it).
        assert_eq!(serde_json::to_string(&ThreadTurn::MyTurn).expect("ser"), "\"my_turn\"");
        assert_eq!(serde_json::to_string(&ThreadTurn::TheirTurn).expect("ser"), "\"their_turn\"");
    }

    #[test]
    fn thread_turn_unknown_is_tolerant() {
        // An N-1 receiver folds a future turn value to Unknown rather than failing.
        let back: ThreadTurn = serde_json::from_str("\"some_future_turn\"").expect("tolerant decode");
        assert_eq!(back, ThreadTurn::Unknown);
    }
}
