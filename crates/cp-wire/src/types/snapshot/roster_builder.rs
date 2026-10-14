//! [`RosterAgentBuilder`] — the fluent builder for [`RosterAgent`].
//!
//! Extracted from the sibling [`snapshot`](super) module to keep `mod.rs` under
//! the 500-line file cap. The builder is the construction path that lets
//! [`RosterAgent`] stay `#[non_exhaustive]`: cross-crate fixtures cannot use a
//! struct literal, and a wide positional constructor would trip
//! `too_many_arguments`, so the three identifying fields are required up front
//! (via [`RosterAgent::builder`]) and the remaining state fields default to
//! their natural zero and are overridden with the setters.

use super::notes::WireNote;
use super::todo::WireTask;
use super::{AgentTurn, RosterAgent};

/// Fluent builder for a [`RosterAgent`].
///
/// The three identifying fields (`agent_id`, `name`, `status`) are required up
/// front in [`RosterAgent::builder`]; the four state fields default to their
/// natural zero (`archived`/`paused` off, `last_activity_ms`/`msg_count` zero)
/// and are overridden with the setters. This keeps [`RosterAgent`]
/// `#[non_exhaustive]` — cross-crate callers (backend test fixtures) build it
/// through the builder instead of a forbidden literal — without a wide
/// positional constructor tripping `too_many_arguments`.
///
/// Fields are private, so the builder itself never triggers `exhaustive_structs`.
#[derive(Clone, Debug)]
pub struct RosterAgentBuilder {
    /// Agent identifier (e.g. `"T7"`).
    agent_id: String,
    /// User-chosen agent label.
    name: String,
    /// Current turn ownership.
    status: AgentTurn,
    /// Whether the agent is archived (soft-deleted).
    archived: bool,
    /// Whether the agent is paused (no idle `MY_TURN` notifications).
    paused: bool,
    /// Epoch-ms of the latest activity.
    last_activity_ms: u64,
    /// Number of messages folded into this agent so far.
    msg_count: u32,
    /// The agent's projected tasks (read-only todo items).
    tasks: Vec<WireTask>,
    /// The agent's projected scratchpad notes (read-only cells).
    notes: Vec<WireNote>,
    /// Parent agent of a sub-agent; `None` for a root.
    parent_agent_id: Option<String>,
}

impl RosterAgentBuilder {
    /// Start a builder from the three required identifying fields; the state
    /// fields default to their natural zero. Called by [`RosterAgent::builder`].
    #[must_use]
    pub(super) fn new<T, U>(agent_id: T, name: U, status: AgentTurn) -> Self
    where
        T: Into<String>,
        U: Into<String>,
    {
        Self {
            agent_id: agent_id.into(),
            name: name.into(),
            status,
            archived: false,
            paused: false,
            last_activity_ms: 0,
            msg_count: 0,
            tasks: Vec::new(),
            notes: Vec::new(),
            parent_agent_id: None,
        }
    }

    /// Mark the agent archived (soft-deleted). Default `false`.
    #[must_use]
    pub const fn archived(mut self, archived: bool) -> Self {
        self.archived = archived;
        self
    }

    /// Mark the agent paused (no idle `MY_TURN` notifications). Default `false`.
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

    /// Set the agent's projected tasks. Default empty.
    #[must_use]
    pub fn tasks(mut self, tasks: Vec<WireTask>) -> Self {
        self.tasks = tasks;
        self
    }

    /// Set the agent's projected scratchpad notes. Default empty.
    #[must_use]
    pub fn notes(mut self, notes: Vec<WireNote>) -> Self {
        self.notes = notes;
        self
    }

    /// Set the parent of a sub-agent. Default `None` (a root).
    #[must_use]
    pub fn parent_agent_id(mut self, parent_agent_id: Option<String>) -> Self {
        self.parent_agent_id = parent_agent_id;
        self
    }

    /// Finalise into a [`RosterAgent`]. Total (no fallible field), so it never
    /// panics.
    #[must_use]
    pub fn build(self) -> RosterAgent {
        RosterAgent {
            agent_id: self.agent_id,
            name: self.name,
            status: self.status,
            archived: self.archived,
            paused: self.paused,
            last_activity_ms: self.last_activity_ms,
            msg_count: self.msg_count,
            tasks: self.tasks,
            notes: self.notes,
            parent_agent_id: self.parent_agent_id,
        }
    }
}
