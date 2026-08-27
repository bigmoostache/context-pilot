//! [`MemoSeeds`] — the compact seed-state bitfield for the bridge's
//! observe-on-change live-emission chokepoints.
//!
//! Each live emitter (messages, statuses, focus, archived, paused, behaviour,
//! identity, tasks) seeds its per-thread diff memo on the first post-boot pass
//! *without* emitting, so a (re)started agent does not replay its whole backlog
//! onto the oplog. This bitfield records which of those first-pass seeds have
//! happened. Extracted from `lib.rs` to keep that file under the 500-line cap;
//! it is bridge boot/runtime bookkeeping, hence its home under `boot`.

/// Seed flags for observe-on-change memos.
///
/// Each flag is `false` until the corresponding chokepoint runs its first
/// pass (seed without emit), then `true` for the remainder of the session.
/// Stored as a compact bitfield to stay under the `struct_excessive_bools`
/// lint (eight independent seed flags).
#[derive(Debug, Default, Clone, Copy)]
pub struct MemoSeeds(u8);

impl MemoSeeds {
    /// Bit position: messages memo.
    const MESSAGES: u8 = 1 << 0;
    /// Bit position: statuses memo.
    const STATUSES: u8 = 1 << 1;
    /// Bit position: focus memo.
    const FOCUS: u8 = 1 << 2;
    /// Bit position: archived memo.
    const ARCHIVED: u8 = 1 << 3;
    /// Bit position: paused memo.
    const PAUSED: u8 = 1 << 4;
    /// Bit position: behaviour (active-agent) memo.
    const BEHAVIOUR: u8 = 1 << 5;
    /// Bit position: identity (self-identity) memo.
    const IDENTITY: u8 = 1 << 6;
    /// Bit position: task-list memo.
    const TASKS: u8 = 1 << 7;

    /// Messages memo seeded (`emit_messages`).
    #[must_use]
    pub const fn messages(self) -> bool {
        self.0 & Self::MESSAGES != 0
    }
    /// Thread-status memo seeded (`emit_thread_status`).
    #[must_use]
    pub const fn statuses(self) -> bool {
        self.0 & Self::STATUSES != 0
    }
    /// Focus memo seeded (`emit_thread_focus`).
    #[must_use]
    pub const fn focus(self) -> bool {
        self.0 & Self::FOCUS != 0
    }
    /// Archived memo seeded (`emit_thread_archived`).
    #[must_use]
    pub const fn archived(self) -> bool {
        self.0 & Self::ARCHIVED != 0
    }
    /// Paused memo seeded (`emit_thread_paused`).
    #[must_use]
    pub const fn paused(self) -> bool {
        self.0 & Self::PAUSED != 0
    }
    /// Behaviour (active-agent) memo seeded (`emit_behaviour`).
    #[must_use]
    pub const fn behaviour(self) -> bool {
        self.0 & Self::BEHAVIOUR != 0
    }
    /// Identity (self-identity) memo seeded (`emit_identity`).
    #[must_use]
    pub const fn identity(self) -> bool {
        self.0 & Self::IDENTITY != 0
    }
    /// Task-list memo seeded (`emit_task_lists`).
    #[must_use]
    pub const fn tasks(self) -> bool {
        self.0 & Self::TASKS != 0
    }

    /// Mark messages as seeded.
    pub const fn seed_messages(&mut self) {
        self.0 |= Self::MESSAGES;
    }
    /// Mark statuses as seeded.
    pub const fn seed_statuses(&mut self) {
        self.0 |= Self::STATUSES;
    }
    /// Mark focus as seeded.
    pub const fn seed_focus(&mut self) {
        self.0 |= Self::FOCUS;
    }
    /// Mark archived as seeded.
    pub const fn seed_archived(&mut self) {
        self.0 |= Self::ARCHIVED;
    }
    /// Mark paused as seeded.
    pub const fn seed_paused(&mut self) {
        self.0 |= Self::PAUSED;
    }
    /// Mark behaviour (active-agent) as seeded.
    pub const fn seed_behaviour(&mut self) {
        self.0 |= Self::BEHAVIOUR;
    }
    /// Mark identity (self-identity) as seeded.
    pub const fn seed_identity(&mut self) {
        self.0 |= Self::IDENTITY;
    }
    /// Mark task-lists as seeded.
    pub const fn seed_tasks(&mut self) {
        self.0 |= Self::TASKS;
    }
}
