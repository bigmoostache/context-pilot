//! Thread-owned task projection types — the read-only shape the web thread
//! view renders in its todo aside.
//!
//! Defined here in the I/O-free protocol crate rather than imported from
//! `cp-mod-todo` to keep the layering one-directional (modules depend on the
//! wire, never the reverse), exactly like [`ThreadTurn`](super::super::ThreadTurn).
//! Carried on the owning thread's
//! [`RosterThread::tasks`](super::RosterThread::tasks) and replaced wholesale
//! by each [`TaskListChanged`](super::super::oplog::OpEntryKind::TaskListChanged)
//! delta.

use serde::{Deserialize, Serialize};

/// The status of one thread-owned task on the wire.
///
/// The projection mirror of the todo module's `TodoStatus`, minus the
/// soft-deleted `Cancelled` state: cancelled tasks are **excluded** from the
/// projection entirely, so they never cross the wire (the backend is the source
/// of truth and the frontend renders verbatim).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[expect(
    clippy::exhaustive_enums,
    reason = "wire-protocol contract: WireTaskStatus carries an Unknown catch-all for N-1 forward-compat"
)]
pub enum WireTaskStatus {
    /// Not started (`[ ]`).
    Planned,
    /// The current work-in-progress item (`[~]`).
    InProgress,
    /// Completed (`[x]`).
    Done,
    /// A status value from a newer protocol version, or a state the projection
    /// is not expected to emit (N-1 forward-compat).
    #[serde(other)]
    Unknown,
}

/// One thread-owned task as projected onto the wire — the read-only shape the
/// web thread view renders in its todo aside.
///
/// A flat list carried on the owning thread's
/// [`RosterThread::tasks`](super::RosterThread::tasks); nesting is reconstructed
/// frontend-side from [`parent_id`](Self::parent_id) (roots have `None`).
/// Cancelled tasks are excluded upstream, so every `WireTask` on the wire is
/// `Planned`, `InProgress`, or `Done`.
/// A five-field wire value object built by struct literal at its cross-crate
/// call sites (the agent's task projection and the orchestrator's first-paint
/// read), so it is deliberately exhaustive: a five-argument constructor would
/// trip the forbidden `too_many_arguments` with no natural field grouping, and
/// `#[non_exhaustive]` would forbid the very literal construction those sites
/// need. A new field here is a deliberate wire change every peer recompiles for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[expect(
    clippy::exhaustive_structs,
    reason = "wire value object: all five fields are public, stable, and built by cross-crate consumers via a struct literal, so #[non_exhaustive] would forbid that construction and a five-arg constructor would trip too_many_arguments"
)]
pub struct WireTask {
    /// Task identifier (e.g. `"X12"`).
    pub id: String,

    /// Parent task id for nesting, or `None` for a top-level (root) task. The
    /// parent always belongs to the same thread.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,

    /// Short task title.
    pub name: String,

    /// Longer description (may be empty) — enables a tooltip in the aside.
    #[serde(default)]
    pub description: String,

    /// Task status (never `Cancelled` — those are excluded from the projection).
    pub status: WireTaskStatus,
}
