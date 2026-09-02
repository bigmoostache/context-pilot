//! [`RosterThreadBuilder`] — the fluent builder for [`RosterThread`].
//!
//! Extracted from the sibling [`snapshot`](super) module to keep `mod.rs` under
//! the 500-line file cap. The builder is the construction path that lets
//! [`RosterThread`] stay `#[non_exhaustive]`: cross-crate fixtures cannot use a
//! struct literal, and a wide positional constructor would trip
//! `too_many_arguments`, so the three identifying fields are required up front
//! (via [`RosterThread::builder`]) and the remaining state fields default to
//! their natural zero and are overridden with the setters.

use super::notes::WireNote;
use super::todo::WireTask;
use super::{RosterThread, ThreadTurn};

/// Fluent builder for a [`RosterThread`].
///
/// The three identifying fields (`thread_id`, `name`, `status`) are required up
/// front in [`RosterThread::builder`]; the four state fields default to their
/// natural zero (`archived`/`paused` off, `last_activity_ms`/`msg_count` zero)
/// and are overridden with the setters. This keeps [`RosterThread`]
/// `#[non_exhaustive]` — cross-crate callers (backend test fixtures) build it
/// through the builder instead of a forbidden literal — without a wide
/// positional constructor tripping `too_many_arguments`.
///
/// Fields are private, so the builder itself never triggers `exhaustive_structs`.
#[derive(Clone, Debug)]
pub struct RosterThreadBuilder {
    /// Thread identifier (e.g. `"T7"`).
    thread_id: String,
    /// User-chosen thread label.
    name: String,
    /// Current turn ownership.
    status: ThreadTurn,
    /// Whether the thread is archived (soft-deleted).
    archived: bool,
    /// Whether the thread is paused (no idle `MY_TURN` notifications).
    paused: bool,
    /// Epoch-ms of the latest activity.
    last_activity_ms: u64,
    /// Number of messages folded into this thread so far.
    msg_count: u32,
    /// The thread's projected tasks (read-only todo items).
    tasks: Vec<WireTask>,
    /// The thread's projected scratchpad notes (read-only cells).
    notes: Vec<WireNote>,
}

impl RosterThreadBuilder {
    /// Start a builder from the three required identifying fields; the state
    /// fields default to their natural zero. Called by [`RosterThread::builder`].
    #[must_use]
    pub(super) fn new<T, U>(thread_id: T, name: U, status: ThreadTurn) -> Self
    where
        T: Into<String>,
        U: Into<String>,
    {
        Self {
            thread_id: thread_id.into(),
            name: name.into(),
            status,
            archived: false,
            paused: false,
            last_activity_ms: 0,
            msg_count: 0,
            tasks: Vec::new(),
            notes: Vec::new(),
        }
    }

    /// Mark the thread archived (soft-deleted). Default `false`.
    #[must_use]
    pub const fn archived(mut self, archived: bool) -> Self {
        self.archived = archived;
        self
    }

    /// Mark the thread paused (no idle `MY_TURN` notifications). Default `false`.
    #[must_use]
    pub const fn paused(mut self, paused: bool) -> Self {
        self.paused = paused;
        self
    }

    /// Set the epoch-ms of the latest activity. Default `0`.
    #[must_use]
    pub const fn last_activity_ms(mut self, last_activity_ms: u64) -> Self {
        self.last_activity_ms = last_activity_ms;
        self
    }

    /// Set the folded-message count. Default `0`.
    #[must_use]
    pub const fn msg_count(mut self, msg_count: u32) -> Self {
        self.msg_count = msg_count;
        self
    }

    /// Set the thread's projected tasks. Default empty.
    #[must_use]
    pub fn tasks(mut self, tasks: Vec<WireTask>) -> Self {
        self.tasks = tasks;
        self
    }

    /// Set the thread's projected scratchpad notes. Default empty.
    #[must_use]
    pub fn notes(mut self, notes: Vec<WireNote>) -> Self {
        self.notes = notes;
        self
    }

    /// Finalise into a [`RosterThread`]. Total (no fallible field), so it never
    /// panics.
    #[must_use]
    pub fn build(self) -> RosterThread {
        RosterThread {
            thread_id: self.thread_id,
            name: self.name,
            status: self.status,
            archived: self.archived,
            paused: self.paused,
            last_activity_ms: self.last_activity_ms,
            msg_count: self.msg_count,
            tasks: self.tasks,
            notes: self.notes,
        }
    }
}
