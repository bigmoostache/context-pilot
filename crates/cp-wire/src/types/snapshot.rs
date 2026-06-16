//! Bounded replay snapshot — the full recoverable state folded from the oplog.
//!
//! Replaying the oplog reconstructs two bounded structures and nothing else:
//!
//! * [`Heads`] — per-thread and per-panel content hashes (the latest state of
//!   each, **O(threads + panels)** rather than O(total-files); design doc I3,
//!   resolves K3); and
//! * [`SeenSet`] — the dedup-tokens of command effects that are durable but not
//!   yet acknowledged-and-evicted, which makes command application
//!   **exactly-once** across a replay (design doc I4).
//!
//! A [`Snapshot`] bundles both. It is what a `Checkpoint` oplog record carries
//! as the first record of every rolled segment, so recovery reads only the
//! newest segment instead of folding the whole log (design doc I5 / GAP 1).
//! Both structures must be in the checkpoint: if only [`Heads`] were
//! snapshotted, rebuilding the [`SeenSet`] would have to fold from offset 0 —
//! re-introducing the unbounded replay the checkpoint exists to prevent.

use serde::{Deserialize, Serialize};

use super::ContentHash;

/// Wire-schema revision stamped onto freshly-constructed snapshot structures.
const SNAPSHOT_SCHEMA_VERSION: u32 = 1;

/// The complete bounded state recoverable from the oplog at a given `rev`.
///
/// Written verbatim into each rolled segment's leading `Checkpoint` record so
/// replay can resume from one segment (design doc I5 / GAP 1).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    /// Per-thread / per-panel content heads as of this `rev`.
    pub heads: Heads,

    /// Dedup-tokens of durable, not-yet-evicted command effects.
    pub seen: SeenSet,
}

// ── Heads ────────────────────────────────────────────────────────────────

/// Snapshot of an agent's current heads at a specific `rev`.
///
/// Each head is a content-addressed reference into the immutable body store;
/// hydrating a snapshot means fetching bodies by hash on demand (lazy,
/// rev-pinned — design doc I5).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heads {
    /// Wire-schema revision for this struct.
    pub schema_version: u32,

    /// Per-thread last-message head.
    pub threads: Vec<ThreadHead>,

    /// Per-panel content head.
    pub panels: Vec<PanelHead>,
}

impl Default for Heads {
    /// An empty head set — the state of a freshly-booted agent before any
    /// message or panel exists.
    fn default() -> Self {
        Self { schema_version: SNAPSHOT_SCHEMA_VERSION, threads: Vec::new(), panels: Vec::new() }
    }
}

impl Heads {
    /// Set (or insert) the last-message head for `thread_id`.
    ///
    /// Replay folds a `MessageCreated` entry through this: the most recent
    /// message of a thread overwrites the previous head, so the head set stays
    /// bounded at one entry per thread (design doc I3). Insertion order is
    /// deterministic (append-on-first-sight, update-in-place), so two replays
    /// of the same log produce byte-identical heads.
    pub fn set_thread_head(&mut self, thread_id: &str, last_message_hash: ContentHash) {
        if let Some(existing) = self.threads.iter_mut().find(|head| head.thread_id == thread_id) {
            existing.last_message_hash = last_message_hash;
        } else {
            self.threads.push(ThreadHead { thread_id: thread_id.to_owned(), last_message_hash });
        }
    }
}

/// A single thread's head — the hash of its most recent message body.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadHead {
    /// Thread identifier.
    pub thread_id: String,

    /// Content hash of the last message body in this thread.
    pub last_message_hash: ContentHash,
}

/// A single panel's head — the hash of its serialised content.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelHead {
    /// Panel identifier.
    pub panel_id: String,

    /// Content hash of the panel's current serialised state.
    pub hash: ContentHash,
}

// ── SeenSet ────────────────────────────────────────────────────────────────

/// The set of command dedup-tokens whose effects are durable.
///
/// A command carries a client-supplied `dedup_token` (a *semantic* key for one
/// logical command). Before applying an effect, the agent checks
/// [`SeenSet::contains`]; a duplicate at-least-once delivery is a no-op. The set
/// is **evicted by acknowledged-`rev`, never by time** ([`SeenSet::evict_through`]):
/// a token retires only once the backend has durably confirmed its effect
/// consumed, so a replay after *any* outage duration is still deduplicated
/// (design doc I4 / R2-1). This is what makes command effects exactly-once
/// across a deadman re-exec.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeenSet {
    /// Wire-schema revision for this struct.
    pub schema_version: u32,

    /// One entry per durable, not-yet-evicted command.
    pub entries: Vec<SeenEntry>,
}

impl Default for SeenSet {
    /// An empty seen-set — no command has yet committed an effect.
    fn default() -> Self {
        Self { schema_version: SNAPSHOT_SCHEMA_VERSION, entries: Vec::new() }
    }
}

impl SeenSet {
    /// Record that `token`'s effect committed at `rev`.
    ///
    /// Idempotent: a token already present keeps its **original** `rev` (the
    /// `rev` of the first, real commit), so folding a duplicate delivery — or
    /// replaying the same log twice — never changes the set. This is why a
    /// `CommandEffect` and a later duplicate `SeenMark` for the same token both
    /// fold safely.
    pub fn mark(&mut self, token: &str, rev: u64) {
        if !self.contains(token) {
            self.entries.push(SeenEntry { token: token.to_owned(), rev });
        }
    }

    /// Whether `token`'s effect has already been applied.
    #[must_use]
    pub fn contains(&self, token: &str) -> bool {
        self.entries.iter().any(|entry| entry.token == token)
    }

    /// Drop every token whose effect committed at or before `ack_rev`.
    ///
    /// Called at compaction (a later phase) once the backend has durably
    /// acknowledged consuming effects through `ack_rev`: such tokens can never
    /// be legitimately re-delivered, so retiring them bounds the set without
    /// weakening dedup for anything still in flight (design doc I4, R2-1).
    pub fn evict_through(&mut self, ack_rev: u64) {
        self.entries.retain(|entry| entry.rev > ack_rev);
    }

    /// Number of live (un-evicted) tokens.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the set holds no live tokens.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// A single dedup-token and the `rev` at which its effect first committed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeenEntry {
    /// The client-supplied semantic dedup token.
    pub token: String,

    /// The `rev` of the entry that first committed this command's effect.
    pub rev: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heads_round_trip() {
        let heads = Heads {
            schema_version: 1,
            threads: vec![ThreadHead {
                thread_id: "T1".into(),
                last_message_hash: ContentHash::new([0x11; 32]),
            }],
            panels: vec![PanelHead { panel_id: "P5".into(), hash: ContentHash::new([0x22; 32]) }],
        };
        let json = serde_json::to_string(&heads).expect("serialize");
        let back: Heads = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(heads, back);
    }

    #[test]
    fn empty_heads_round_trip() {
        let heads = Heads::default();
        let json = serde_json::to_string(&heads).expect("serialize");
        let back: Heads = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(heads, back);
    }

    #[test]
    fn seen_set_marks_and_contains() {
        let mut seen = SeenSet::default();
        assert!(!seen.contains("cmd-a"));
        seen.mark("cmd-a", 3);
        assert!(seen.contains("cmd-a"));
        assert!(!seen.contains("cmd-b"));
    }

    #[test]
    fn seen_set_mark_is_idempotent_and_keeps_first_rev() {
        let mut seen = SeenSet::default();
        seen.mark("cmd-a", 3);
        seen.mark("cmd-a", 9); // duplicate delivery at a later rev
        assert_eq!(seen.len(), 1, "duplicate token must not add a second entry");
        assert_eq!(seen.entries.first().expect("entry").rev, 3, "first rev is kept");
    }

    #[test]
    fn seen_set_eviction_is_rev_based_not_time_based() {
        // V3: a token survives an arbitrarily long outage because eviction is
        // gated on acknowledged-rev, never wall-clock. We never advance the
        // ack barrier past the token's rev, so it stays — regardless of how
        // much (simulated) time elapsed.
        let mut seen = SeenSet::default();
        seen.mark("cmd-late", 10);
        seen.evict_through(9); // backend has only acked through rev 9
        assert!(seen.contains("cmd-late"), "un-acked token survives any time gap");
        seen.evict_through(10); // now its rev is acknowledged
        assert!(!seen.contains("cmd-late"), "acked token is evicted");
    }

    #[test]
    fn snapshot_round_trip() {
        let mut snapshot = Snapshot::default();
        snapshot.heads.set_thread_head("T1", ContentHash::new([0x33; 32]));
        snapshot.seen.mark("cmd-x", 5);
        let json = serde_json::to_string(&snapshot).expect("serialize");
        let back: Snapshot = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(snapshot, back);
    }
}
