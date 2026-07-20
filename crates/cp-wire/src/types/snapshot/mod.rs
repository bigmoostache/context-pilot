//! Bounded replay snapshot — the full recoverable state folded from the oplog.
//!
//! Replaying the oplog reconstructs a small, bounded set of structures:
//!
//! * [`Heads`] — per-thread and per-panel content hashes (the latest state of
//!   each, **O(threads + panels)** rather than O(total-files); design doc I3,
//!   resolves K3);
//! * [`SeenSet`] — the dedup-tokens of command effects that are durable but not
//!   yet acknowledged-and-evicted, which makes command application
//!   **exactly-once** across a replay (design doc I4); and
//! * a [`RosterThread`] list — the bounded thread roster (one entry per live
//!   thread) so the backend can render the thread list after oplog
//!   **compaction** without folding the roster deltas from offset 0 (design
//!   doc I5 / §16).
//!
//! A [`Snapshot`] bundles all three. It is what a `Checkpoint` oplog record
//! carries as the first record of every rolled segment, so recovery reads only
//! the newest segment instead of folding the whole log (design doc I5 / GAP 1).
//! Both [`Heads`] and [`SeenSet`] must be in the checkpoint: if only [`Heads`]
//! were snapshotted, rebuilding the [`SeenSet`] would have to fold from offset
//! 0 — re-introducing the unbounded replay the checkpoint exists to prevent;
//! the [`RosterThread`] list extends that same guarantee to the thread list.

use serde::{Deserialize, Serialize};

use super::{ContentHash, ThreadTurn};

/// Wire-schema revision stamped onto freshly-constructed snapshot structures.
const SNAPSHOT_SCHEMA_VERSION: u32 = 1;

/// The complete bounded state recoverable from the oplog at a given `rev`.
///
/// Written verbatim into each rolled segment's leading `Checkpoint` record so
/// replay can resume from one segment (design doc I5 / GAP 1).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Snapshot {
    /// Per-thread / per-panel content heads as of this `rev`.
    pub heads: Heads,

    /// Dedup-tokens of durable, not-yet-evicted command effects.
    pub seen: SeenSet,

    /// The thread roster as of this `rev` — one entry per live thread (design
    /// doc §16 journals thread create/archive/restore; carrying it in the
    /// checkpoint is what lets a backend rebuild the thread list after oplog
    /// **compaction** without folding from offset 0, design doc I5).
    ///
    /// Additive + `serde(default)`: a checkpoint written by an older agent
    /// (before the roster was snapshotted) decodes to an empty roster, and an
    /// older backend reading a newer checkpoint ignores the field — N-1
    /// compatible in both directions (design doc §18).
    #[serde(default)]
    pub roster: Vec<RosterThread>,
}

impl Snapshot {
    /// Bundle the three recoverable structures into a checkpoint snapshot.
    ///
    /// The writer funnels checkpoints through this so [`Snapshot`] stays
    /// `#[non_exhaustive]` — a future recoverable structure is added here
    /// without breaking cross-crate callers.
    #[must_use]
    pub const fn new(heads: Heads, seen: SeenSet, roster: Vec<RosterThread>) -> Self {
        Self { heads, seen, roster }
    }
}

/// One thread's snapshotted roster entry — the lightweight per-thread metadata
/// needed to render a thread list (name, turn, archived, activity, count)
/// without hydrating any message body.
///
/// Folded from the thread-roster oplog deltas
/// ([`ThreadCreated`](super::oplog::OpEntryKind::ThreadCreated) and friends),
/// with `msg_count`/`last_activity_ms` accumulated from each subsequent
/// [`MessageCreated`](super::oplog::OpEntryKind::MessageCreated). The backend's
/// materialized view uses this struct directly as its roster element, so the
/// checkpoint-restored roster and the live-folded roster are the same type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RosterThread {
    /// Thread identifier (e.g. `"T7"`).
    pub thread_id: String,

    /// User-chosen thread label.
    pub name: String,

    /// Current turn ownership.
    pub status: ThreadTurn,

    /// Whether the thread is archived (soft-deleted: hidden from the active
    /// list but restorable).
    pub archived: bool,

    /// Whether the thread is paused — `MY_TURN` status no longer fires idle
    /// notifications, but the thread remains visible and functional.
    #[serde(default)]
    pub paused: bool,

    /// Epoch-ms of the latest activity — creation time, then bumped by each
    /// message.
    pub last_activity_ms: u64,

    /// Number of messages folded into this thread so far.
    pub msg_count: u32,
}

/// The facts a `ThreadCreated` oplog delta carries — the borrowed payload of
/// [`RosterThread::fold_created`].
///
/// A named carrier (rather than four loose arguments) keeps the fold signature
/// small and mirrors the
/// [`ThreadCreated`](super::oplog::OpEntryKind::ThreadCreated) variant the
/// caller match-binds: both the agent's replay fold and the backend's live
/// fold construct one inline from the same fields, so the two paths stay
/// identical.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct ThreadCreation<'src> {
    /// Thread identifier (e.g. `"T7"`).
    pub thread_id: &'src str,

    /// User-chosen thread label.
    pub name: &'src str,

    /// Turn ownership at creation.
    pub status: ThreadTurn,

    /// Epoch-ms the thread was created (seeds `last_activity_ms`).
    pub timestamp_ms: u64,
}

impl<'src> ThreadCreation<'src> {
    /// Build the borrowed `ThreadCreated` payload the fold consumes.
    /// Keeps [`ThreadCreation`] `#[non_exhaustive]` across the agent-replay
    /// and backend-fold call sites.
    #[must_use]
    pub const fn new(thread_id: &'src str, name: &'src str, status: ThreadTurn, timestamp_ms: u64) -> Self {
        Self { thread_id, name, status, timestamp_ms }
    }
}

/// The seven owned fields of a [`RosterThread`], bundled so
/// [`RosterThread::new`] takes one argument rather than seven.
///
/// A cross-crate builder (the backend's test fixtures) constructs this by
/// literal and hands it to [`RosterThread::new`]; it is deliberately
/// exhaustive so that literal construction is allowed, unlike the
/// `#[non_exhaustive]` [`RosterThread`] it feeds.
#[derive(Clone, Debug)]
#[expect(
    clippy::exhaustive_structs,
    reason = "roster-entry init bundle: a flat 7-field carrier built by literal cross-crate (backend test fixtures) then consumed by RosterThread::new; #[non_exhaustive] would forbid that literal (the very E0639 this bundle exists to route around) and a 7-arg constructor trips too_many_arguments (threshold 4)"
)]
pub struct RosterEntryInit {
    /// Thread identifier (e.g. `"T7"`).
    pub thread_id: String,
    /// User-chosen thread label.
    pub name: String,
    /// Current turn ownership.
    pub status: ThreadTurn,
    /// Whether the thread is archived (soft-deleted).
    pub archived: bool,
    /// Whether the thread is paused (no idle `MY_TURN` notifications).
    pub paused: bool,
    /// Epoch-ms of the latest activity.
    pub last_activity_ms: u64,
    /// Number of messages folded into this thread so far.
    pub msg_count: u32,
}

impl RosterThread {
    /// Build a roster entry from a [`RosterEntryInit`] bundle.
    ///
    /// A constructor keeps [`RosterThread`] `#[non_exhaustive]` across
    /// cross-crate builders (backend test fixtures build entries with full
    /// field control); the seven fields ride in the bundle so the signature
    /// stays under the argument cap.
    #[must_use]
    pub fn new(init: RosterEntryInit) -> Self {
        let RosterEntryInit { thread_id, name, status, archived, paused, last_activity_ms, msg_count } = init;
        Self { thread_id, name, status, archived, paused, last_activity_ms, msg_count }
    }

    /// Apply a `ThreadCreated` to a roster, **insert-or-update** so a duplicate
    /// delivery or a replay folds idempotently (a re-seen creation refreshes
    /// name/status and clears `archived`, never duplicates the entry).
    pub fn fold_created(roster: &mut Vec<Self>, created: ThreadCreation<'_>) {
        if let Some(existing) = roster.iter_mut().find(|e| e.thread_id == created.thread_id) {
            created.name.clone_into(&mut existing.name);
            existing.status = created.status;
            existing.archived = false;
        } else {
            roster.push(Self {
                thread_id: created.thread_id.to_owned(),
                name: created.name.to_owned(),
                status: created.status,
                archived: false,
                paused: false,
                last_activity_ms: created.timestamp_ms,
                msg_count: 0,
            });
        }
    }

    /// Set the `archived` flag for `thread_id`, if present (a no-op otherwise).
    pub fn fold_archived(roster: &mut [Self], thread_id: &str, archived: bool) {
        if let Some(existing) = roster.iter_mut().find(|e| e.thread_id == thread_id) {
            existing.archived = archived;
        }
    }

    /// Set the `paused` flag for `thread_id`, if present (a no-op otherwise).
    pub fn fold_paused(roster: &mut [Self], thread_id: &str, paused: bool) {
        if let Some(existing) = roster.iter_mut().find(|e| e.thread_id == thread_id) {
            existing.paused = paused;
        }
    }

    /// Remove `thread_id` from the roster entirely (permanent delete).
    pub fn fold_deleted(roster: &mut Vec<Self>, thread_id: &str) {
        roster.retain(|e| e.thread_id != thread_id);
    }

    /// Set the turn `status` for `thread_id`, if present (a no-op otherwise).
    pub fn fold_status(roster: &mut [Self], thread_id: &str, status: ThreadTurn) {
        if let Some(existing) = roster.iter_mut().find(|e| e.thread_id == thread_id) {
            existing.status = status;
        }
    }

    /// Record a message in `thread_id`: bump its count and advance its activity
    /// timestamp (a no-op if the thread is not in the roster — e.g. a message
    /// folded before its creation delta was seen).
    pub fn fold_message(roster: &mut [Self], thread_id: &str, timestamp_ms: u64) {
        if let Some(existing) = roster.iter_mut().find(|e| e.thread_id == thread_id) {
            existing.msg_count = existing.msg_count.saturating_add(1);
            existing.last_activity_ms = timestamp_ms;
        }
    }
}

// ── Heads ────────────────────────────────────────────────────────────────

/// Snapshot of an agent's current heads at a specific `rev`.
///
/// Each head is a content-addressed reference into the immutable body store;
/// hydrating a snapshot means fetching bodies by hash on demand (lazy,
/// rev-pinned — design doc I5).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
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
#[non_exhaustive]
pub struct ThreadHead {
    /// Thread identifier.
    pub thread_id: String,

    /// Content hash of the last message body in this thread.
    pub last_message_hash: ContentHash,
}

/// A single panel's head — the hash of its serialised content.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
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
#[non_exhaustive]
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

    /// The `rev` at which `token`'s effect first committed, or `None` if the
    /// token is not (or no longer) in the set.
    ///
    /// Command intake uses this to acknowledge a duplicate delivery with the
    /// `rev` the effect originally landed at, so a retrying commander learns
    /// where its (already-applied) command lives without re-journaling it.
    #[must_use]
    pub fn rev_of(&self, token: &str) -> Option<u64> {
        self.entries.iter().find(|entry| entry.token == token).map(|entry| entry.rev)
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
#[non_exhaustive]
pub struct SeenEntry {
    /// The client-supplied semantic dedup token.
    pub token: String,

    /// The `rev` of the entry that first committed this command's effect.
    pub rev: u64,
}

/// Roster-fold unit tests.
#[cfg(test)]
#[path = "tests.rs"]
mod tests;
