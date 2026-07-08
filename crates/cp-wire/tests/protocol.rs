//! Phase 21 — `cp-wire` protocol coverage at the public-API boundary.
//!
//! The per-type inline tests prove each struct round-trips; this suite fills
//! the gaps an integration view exposes:
//!
//! * **every** [`OpEntryKind`] variant survives a JSON round-trip (the inline
//!   tests cover only `PhaseTransition`, `MessageCreated`, and `Unknown`),
//!   including the `Checkpoint` that carries a whole [`Snapshot`];
//! * the [`accepts`] N-1 compatibility window holds across a sweep of majors,
//!   so a future bump can't silently widen or invert it;
//! * [`ContentHash::from_hex`] is the exact inverse of
//!   [`ContentHash::to_hex`] and rejects malformed input rather than panicking;
//! * the [`SeenSet`] eviction barrier retires exactly the acknowledged tokens
//!   and no others.
//!
//! Together with `framing_integrity.rs` this is the Phase 21 forcing function:
//! the wire contract every other crate depends on is exercised, not assumed.

// This integration target links `cp-wire`'s dependencies but exercises only
// its public API; `serde_json` is used directly below, the rest are not, so the
// per-target `unused-crate-dependencies` lint needs the canonical `as _`
// acknowledgement for them (Cargo's suggested form, not a lint silence).
use crc32c as _;
use serde as _;
use sha2 as _;
use utoipa as _;

use cp_wire::types::oplog::{OpEntry, OpEntryKind};
use cp_wire::types::snapshot::{Heads, SeenSet, Snapshot};
use cp_wire::types::{ContentHash, LifecycleState, Phase};
use cp_wire::{PROTOCOL_VERSION, accepts};

/// JSON-round-trip an [`OpEntry`] and assert byte-identical recovery.
fn round_trips(entry: &OpEntry) {
    let json = serde_json::to_string(entry).expect("serialize");
    let back: OpEntry = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(entry, &back, "OpEntry did not survive a JSON round trip");
}

#[test]
fn every_opentry_kind_round_trips() {
    // One representative of *every* discriminant — the variants the inline
    // tests skip are the ones a regression would most quietly break.
    let mut snapshot = Snapshot::default();
    snapshot.heads.set_thread_head("T1", ContentHash::of(b"head-body"));
    snapshot.seen.mark("dedup-in-snapshot", 4);

    let kinds = [
        OpEntryKind::CommandEffect { cmd_id: "cmd-1".to_owned(), dedup_token: "dt-1".to_owned() },
        OpEntryKind::SeenMark { dedup_token: "dt-2".to_owned() },
        OpEntryKind::PhaseTransition { phase: Phase::Tooling },
        OpEntryKind::MessageCreated {
            thread_id: "T7".to_owned(),
            message_id: "m-7".to_owned(),
            head: ContentHash::of(b"the message body"),
            inline_body: None,
        },
        OpEntryKind::Lifecycle { state: LifecycleState::Stopping },
        OpEntryKind::CostAggregate {
            input_tokens: 1_024,
            output_tokens: 256,
            // 12.5 is exact in binary float, so the round trip is bit-exact.
            cost_usd: 12.5,
        },
        OpEntryKind::Checkpoint { snapshot },
    ];

    for (i, kind) in kinds.into_iter().enumerate() {
        let rev = u64::try_from(i).unwrap_or(0);
        round_trips(&OpEntry { schema_version: 1, rev, timestamp_ms: 1_000_u64.wrapping_add(rev), kind });
    }
}

#[test]
fn checkpoint_carries_a_faithful_snapshot() {
    // The Checkpoint variant is load-bearing for replay (GAP 1): the heads and
    // the seen-set it carries must survive transport exactly, or a fast-path
    // replay would reseed from corrupt state.
    let mut snapshot = Snapshot::default();
    snapshot.heads.set_thread_head("T1", ContentHash::of(b"a"));
    snapshot.heads.set_thread_head("T2", ContentHash::of(b"b"));
    snapshot.seen.mark("tok-a", 1);
    snapshot.seen.mark("tok-b", 9);

    let entry = OpEntry {
        schema_version: 1,
        rev: 100,
        timestamp_ms: 5,
        kind: OpEntryKind::Checkpoint { snapshot: snapshot.clone() },
    };
    let json = serde_json::to_string(&entry).expect("serialize");
    let back: OpEntry = serde_json::from_str(&json).expect("deserialize");

    match back.kind {
        OpEntryKind::Checkpoint { snapshot: recovered } => {
            assert_eq!(recovered, snapshot, "snapshot mutated in transit");
        }
        other => panic!("checkpoint decoded as a different variant: {other:?}"),
    }
}

#[test]
fn accepts_window_is_exactly_n_minus_one() {
    // Sweep a band of majors around our own and assert the membership rule
    // `PV-1 <= their <= PV` holds at every point — a future PROTOCOL_VERSION
    // bump that widened or inverted the window would fail here.
    for their in 0..=PROTOCOL_VERSION.wrapping_add(4) {
        let expected = their <= PROTOCOL_VERSION && their >= PROTOCOL_VERSION.saturating_sub(1);
        assert_eq!(accepts(their), expected, "accepts({their}) wrong for PROTOCOL_VERSION {PROTOCOL_VERSION}",);
    }
}

#[test]
fn content_hash_hex_is_a_faithful_bijection() {
    // to_hex -> from_hex recovers the identical hash for a spread of inputs.
    for seed in 0..64u8 {
        let hash = ContentHash::of(&[seed; 7]);
        let hex = hash.to_hex();
        assert_eq!(hex.len(), 64, "a SHA-256 renders as 64 hex chars");
        let parsed = ContentHash::from_hex(&hex).expect("our own hex must parse");
        assert_eq!(parsed, hash, "from_hex is not the inverse of to_hex");
    }
}

#[test]
fn content_hash_from_hex_accepts_uppercase_and_rejects_malformed() {
    // Uppercase hex is a valid encoding of the same digest (case-insensitive
    // nibble parse), so it must parse to the identical hash.
    let lower = ContentHash::of(b"case test").to_hex();
    let upper = lower.to_uppercase();
    assert_eq!(
        ContentHash::from_hex(&upper),
        ContentHash::from_hex(&lower),
        "uppercase hex must decode to the same hash as lowercase",
    );

    // Malformed inputs return None, never panic.
    assert_eq!(ContentHash::from_hex(""), None, "empty is not 64 chars");
    assert_eq!(ContentHash::from_hex("abc"), None, "too short");
    assert_eq!(ContentHash::from_hex(&"g".repeat(64)), None, "non-hex char");
    assert_eq!(ContentHash::from_hex(&"a".repeat(63)), None, "odd length");
    assert_eq!(ContentHash::from_hex(&"a".repeat(65)), None, "too long");
}

#[test]
fn seen_set_eviction_retires_exactly_the_acknowledged_tokens() {
    // The dedup barrier evicts strictly by acknowledged-rev: a token at rev R
    // survives until the ack barrier reaches R, and not a moment before. This
    // is what keeps a redelivery after a long outage deduplicated (I4/R2-1).
    let mut seen = SeenSet::default();
    seen.mark("r3", 3);
    seen.mark("r5", 5);
    seen.mark("r9", 9);
    assert_eq!(seen.len(), 3);

    seen.evict_through(4); // acks through rev 4
    assert!(!seen.contains("r3"), "rev 3 <= ack 4 is evicted");
    assert!(seen.contains("r5"), "rev 5 > ack 4 survives");
    assert!(seen.contains("r9"), "rev 9 > ack 4 survives");
    assert_eq!(seen.rev_of("r5"), Some(5), "surviving token keeps its commit rev");

    seen.evict_through(9); // acks through rev 9 — everything retires
    assert!(seen.is_empty(), "all acknowledged tokens evicted");
}

#[test]
fn heads_upsert_is_last_write_wins_per_thread() {
    // A thread's head is overwritten in place, never duplicated — the property
    // that keeps the head set bounded at one entry per thread (I3).
    let mut heads = Heads::default();
    heads.set_thread_head("T1", ContentHash::of(b"first"));
    heads.set_thread_head("T1", ContentHash::of(b"second"));
    heads.set_thread_head("T2", ContentHash::of(b"other"));

    let json = serde_json::to_string(&heads).expect("serialize");
    let back: Heads = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.threads.len(), 2, "T1 must be updated in place, not duplicated");
    assert_eq!(heads, back, "heads round-trip");
}
