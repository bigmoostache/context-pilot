//! Wire-protocol types shared between the agent bridge and the orchestrator.
//!
//! Every type carries a `schema_version` so receivers can detect forward drift
//! before attempting full decode.  Enums use `#[serde(tag = "kind")]` with a
//! catch-all `Unknown` variant (`#[serde(other)]`) so an N-1 receiver
//! gracefully round-trips a variant it has never seen.

pub mod ack;
pub mod body;
pub mod command;
pub mod heads;
pub mod oplog;
pub mod registry;
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
}

/// Hex-encode for JSON/YAML readability.
impl Serialize for ContentHash {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use core::fmt::Write as _;
        let mut hex = String::with_capacity(64);
        for &byte in &self.0 {
            // `write!` on `String` is infallible — discard the always-Ok result.
            _ = write!(hex, "{byte:02x}");
        }
        serializer.serialize_str(&hex)
    }
}

impl<'de> Deserialize<'de> for ContentHash {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
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

    fn deserialize_in_place<D: serde::Deserializer<'de>>(
        deserializer: D,
        place: &mut Self,
    ) -> Result<(), D::Error> {
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
    fn content_hash_hex_format() {
        let hash = ContentHash::new([0xab; 32]);
        let json = serde_json::to_string(&hash).expect("serialize");
        let expected = format!("\"{}\"", "ab".repeat(32));
        assert_eq!(json, expected);
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
        for state in [
            LifecycleState::Starting,
            LifecycleState::Running,
            LifecycleState::Stopping,
            LifecycleState::Stopped,
        ] {
            let json = serde_json::to_string(&state).expect("serialize");
            let back: LifecycleState = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(state, back);
        }
    }
}
