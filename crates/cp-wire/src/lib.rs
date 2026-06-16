//! Context Pilot orchestration **wire protocol** — the shared seam between the
//! agent-side bridge (`cp-mod-bridge`) and the standalone backend
//! (`cp-orchestrator`).
//!
//! This crate is **I/O-free and transport-agnostic on purpose**: it defines the
//! serializable message contract (commands, oplog entries, stream frames,
//! heads, bodies, acks, registry entries) and the version-compatibility rules,
//! but never touches the filesystem, sockets, or processes. Both sides depend
//! on it, so it must stay the most stable crate in the workspace — a change
//! here ripples to every component.
//!
//! Versioning is foundational rather than an afterthought: every wire type
//! carries a `schema_version` and the whole protocol carries a
//! [`PROTOCOL_VERSION`]. The compatibility window is **N-1 major** with a
//! **backend-upgrades-first** ordering invariant (see the design doc §18), so a
//! peer must be able to decide — before decoding a payload — whether it can
//! speak to the sender at all. [`accepts`] encodes that decision.
//!
//! *Phase 2:* the concrete message types ([`types`]) are defined — commands,
//! oplog entries, stream frames, heads, bodies, acks, and registry entries.
//! *Phase 11:* the fixed-size [`heartbeat`] liveness record is added.

pub mod framing;
pub mod heartbeat;
pub mod types;

/// The protocol's current major version.
///
/// Bumped only on a **breaking** wire change (a field removed or its meaning
/// changed). Additive, tolerantly-decoded changes do **not** bump it — they are
/// absorbed by serde's ignore-unknown-fields behaviour on the receiving side.
pub const PROTOCOL_VERSION: u32 = 1;

/// Whether a peer running [`PROTOCOL_VERSION`] can accept a payload produced at
/// `their_major`.
///
/// The compatibility window is **N-1 major**: we accept our own major and the
/// one immediately below it (a backend, which upgrades first, must keep talking
/// to not-yet-upgraded agents), and we reject anything newer than us. Returns
/// `true` iff `their_major` falls in `[PROTOCOL_VERSION - 1, PROTOCOL_VERSION]`.
///
/// Saturating subtraction keeps the lower bound at `0` when
/// `PROTOCOL_VERSION == 0`, so the window never wraps.
#[must_use]
pub const fn accepts(their_major: u32) -> bool {
    their_major <= PROTOCOL_VERSION && their_major >= PROTOCOL_VERSION.saturating_sub(1)
}

#[cfg(test)]
mod tests {
    use super::{PROTOCOL_VERSION, accepts};

    #[test]
    fn accepts_own_major() {
        assert!(accepts(PROTOCOL_VERSION), "a peer must accept its own major");
    }

    #[test]
    fn accepts_one_major_below() {
        assert!(
            accepts(PROTOCOL_VERSION.saturating_sub(1)),
            "the N-1 window must admit the immediately-older major",
        );
    }

    #[test]
    fn rejects_newer_major() {
        assert!(
            !accepts(PROTOCOL_VERSION.saturating_add(1)),
            "a newer major than ours must be rejected (we never upgraded yet)",
        );
    }

    #[test]
    fn rejects_two_majors_below() {
        // Only meaningful once the protocol has advanced past its first major;
        // below that, saturating arithmetic collapses the window to {0, 1}.
        if PROTOCOL_VERSION >= 2 {
            assert!(
                !accepts(PROTOCOL_VERSION - 2),
                "anything older than N-1 falls outside the support window",
            );
        }
    }
}
