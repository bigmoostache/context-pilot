//! Oplog entry — the authoritative, append-only event record (tier ①).
//!
//! Each [`OpEntry`] is one atomic unit in the oplog WAL.  It carries a
//! monotonic `rev` (the append offset), a wall-clock timestamp for
//! diagnostics, and an [`OpEntryKind`] discriminant describing the event.
//! The oplog is `fsync`'d per commit-group (design doc I2/I8).

use serde::{Deserialize, Serialize};

use super::snapshot::Snapshot;
use super::snapshot::notes::WireNote;
use super::snapshot::todo::WireTask;
use super::{AgentTurn, ContentHash, LifecycleState, Phase};

/// A single oplog record.
///
/// Framed with a length prefix + CRC on the wire (design doc §21 Open-Q1,
/// locked v6); this struct is the *payload* inside that frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct OpEntry {
    /// Wire-schema revision for this struct.
    pub schema_version: u32,

    /// Monotonic oplog offset — assigned by the single main loop, never
    /// reused, never skipped (design doc I8/K9).
    pub rev: u64,

    /// Wall-clock milliseconds since the Unix epoch (informational — not
    /// load-bearing for ordering; `rev` is the authority).
    pub timestamp_ms: u64,

    /// What happened.
    pub kind: OpEntryKind,
}

impl OpEntry {
    /// Assemble an oplog record from its parts.
    ///
    /// The writer funnels every entry through this so the wire struct stays
    /// `#[non_exhaustive]` (a future field is a non-breaking addition here).
    #[must_use]
    pub const fn new(schema_version: u32, rev: u64, timestamp_ms: u64, kind: OpEntryKind) -> Self {
        Self { schema_version, rev, timestamp_ms, kind }
    }
}

/// The event an [`OpEntry`] records.
///
/// Internally tagged by `"kind"`.  An N-1 receiver that encounters a
/// variant it has never seen deserialises it as
/// [`Unknown`](OpEntryKind::Unknown).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
#[expect(
    clippy::exhaustive_enums,
    reason = "wire-protocol contract: OpEntryKind carries an Unknown catch-all for N-1 tolerance; its variant set is otherwise closed and constructed cross-crate (the agent emits every kind, observers fold them exhaustively), so #[non_exhaustive] would forbid that construction"
)]
pub enum OpEntryKind {
    /// A command was accepted and its effect applied (I6/I11).
    #[serde(rename = "command_effect")]
    CommandEffect {
        /// The transport-level command id that produced this effect.
        cmd_id: String,
        /// Semantic dedup token — the `seen`-set keys on this (I4).
        dedup_token: String,
    },

    /// A dedup token was marked as seen without a separate effect record
    /// (e.g. a duplicate delivery that was already applied).
    #[serde(rename = "seen_mark")]
    SeenMark {
        /// The deduplicated token.
        dedup_token: String,
    },

    /// The agent transitioned to a new execution phase.
    #[serde(rename = "phase_transition")]
    PhaseTransition {
        /// The phase the agent entered.
        phase: Phase,
    },

    /// A new message was created (finalised by the agent).
    #[serde(rename = "message_created")]
    MessageCreated {
        /// Agent the message belongs to.
        #[serde(rename = "agent_id", alias = "thread_id")]
        agent_id: String,
        /// Unique message identifier.
        message_id: String,
        /// Content-addressed hash of the message body (I3/I13).
        head: ContentHash,
        /// The message body, embedded **inline** when it is small enough to
        /// ride this entry's own `fdatasync` (the common chat case — I13's
        /// inline-small path). The bytes are UTF-8 JSON describing the message
        /// (author, text, timestamp, optional question/file-ref), so an
        /// observer renders the bubble with **zero hydration round-trip**.
        ///
        /// `None` when the body was large enough to **spill** to the
        /// content-addressed body store instead; an observer then hydrates it
        /// by [`head`](Self::MessageCreated::head) over `/body/{hash}`.
        ///
        /// `#[serde(default)]` keeps the field optional on the wire: an N-1
        /// reader that predates it simply sees `None` (spilled-style hydrate),
        /// and it is omitted entirely for spilled bodies.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        inline_body: Option<String>,
    },

    /// A message was deleted from an agent by the user.
    #[serde(rename = "message_deleted")]
    MessageDeleted {
        /// Agent the message was in.
        #[serde(rename = "agent_id", alias = "thread_id")]
        agent_id: String,
        /// Epoch-ms timestamp identifying the deleted message.
        message_ts: u64,
    },

    /// A new agent was opened.
    ///
    /// Carries enough to materialise the roster entry without a disk read: the
    /// roster is the lightweight per-agent metadata the `/agents` endpoint
    /// serves from the in-memory view (design doc §16 lists agent
    /// create/archive/restore as oplog-journaled actions; I8). Message count
    /// and last-activity are *derived* by folding subsequent
    /// [`MessageCreated`](Self::MessageCreated) entries — they are not repeated
    /// here.
    #[serde(rename = "agent_created", alias = "thread_created")]
    AgentCreated {
        /// Identifier of the new agent (e.g. `"T7"`).
        #[serde(rename = "agent_id", alias = "thread_id")]
        agent_id: String,
        /// User-chosen agent label.
        name: String,
        /// Initial turn ownership at creation.
        status: AgentTurn,
        /// Wall-clock creation time (epoch ms) — seeds the roster's
        /// last-activity until the first message lands.
        timestamp_ms: u64,
        /// Parent agent id when the agent was branched out of another one
        /// (`BranchAgent`); absent for an agent created from scratch.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branched_from: Option<String>,
        /// Parent agent id of a sub-agent (created by another agent's `Agent`
        /// tool); absent for a root. Lets observers nest it immediately.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_agent_id: Option<String>,
    },

    /// An agent was archived (soft-delete — hidden from the active list, kept
    /// for restore).
    #[serde(rename = "agent_archived", alias = "thread_archived")]
    AgentArchived {
        /// The archived agent.
        #[serde(rename = "agent_id", alias = "thread_id")]
        agent_id: String,
    },

    /// A previously-archived agent was restored to the active list.
    #[serde(rename = "agent_restored", alias = "thread_restored")]
    AgentRestored {
        /// The restored agent.
        #[serde(rename = "agent_id", alias = "thread_id")]
        agent_id: String,
    },

    /// An agent was paused — its `MY_TURN` status no longer fires idle
    /// notifications, but it remains visible and fully functional.
    #[serde(rename = "agent_paused", alias = "thread_paused")]
    AgentPaused {
        /// The paused agent.
        #[serde(rename = "agent_id", alias = "thread_id")]
        agent_id: String,
    },

    /// A previously-paused agent was resumed — its `MY_TURN` status fires
    /// idle notifications again.
    #[serde(rename = "agent_resumed", alias = "thread_resumed")]
    AgentResumed {
        /// The resumed agent.
        #[serde(rename = "agent_id", alias = "thread_id")]
        agent_id: String,
    },

    /// An agent's display name changed (TUI, web or `Agent` tool).
    #[serde(rename = "agent_renamed")]
    AgentRenamed {
        /// The renamed agent.
        agent_id: String,
        /// Its new display name.
        name: String,
    },

    /// An agent was permanently deleted — removed from the roster and all
    /// its messages discarded. Irreversible.
    #[serde(rename = "agent_deleted", alias = "thread_deleted")]
    AgentDeleted {
        /// The deleted agent.
        #[serde(rename = "agent_id", alias = "thread_id")]
        agent_id: String,
    },

    /// An agent's turn ownership changed (`MyTurn` ↔ `TheirTurn`).
    #[serde(rename = "agent_status_changed", alias = "thread_status_changed")]
    AgentStatusChanged {
        /// The affected agent.
        #[serde(rename = "agent_id", alias = "thread_id")]
        agent_id: String,
        /// The agent's new turn ownership.
        status: AgentTurn,
    },

    /// An agent's sidebar light flipped. Lit = the TUI agents list shows a
    /// working spinner OR a green (human's turn, not paused) circle; the web
    /// sidebar renders it as-is (brown lit, grey unlit), deriving nothing.
    #[serde(rename = "agent_lit_changed")]
    AgentLitChanged {
        /// The affected agent.
        agent_id: String,
        /// The agent's new light.
        lit: bool,
    },

    /// An agent's projected task list changed — the read-only todo items the
    /// web agent view renders. Carries the agent's **complete** current
    /// (cancelled-excluded) task list as a whole-list snapshot, so an observer
    /// folds it by replacing [`RosterAgent::tasks`](super::snapshot::RosterAgent::tasks)
    /// wholesale (see [`RosterAgent::fold_tasks`](super::snapshot::RosterAgent::fold_tasks)).
    ///
    /// Durable roster state (carried in the [`Checkpoint`](Self::Checkpoint)
    /// snapshot via the roster), emitted whenever the agent observes an agent's
    /// tasks change. The tasks themselves are also re-derivable from the
    /// agent's tier-② persistence, so a dropped delta self-heals on the next
    /// change.
    #[serde(rename = "task_list_changed")]
    TaskListChanged {
        /// The agent whose task list changed.
        #[serde(rename = "agent_id", alias = "thread_id")]
        agent_id: String,
        /// The agent's complete current task list (cancelled excluded).
        tasks: Vec<WireTask>,
    },

    /// An agent's projected scratchpad-note list changed — the read-only cells
    /// the web agent view renders in its Notes aside. Carries the agent's
    /// **complete** current note list as a whole-list snapshot, so an observer
    /// folds it by replacing [`RosterAgent::notes`](super::snapshot::RosterAgent::notes)
    /// wholesale (see [`RosterAgent::fold_notes`](super::snapshot::RosterAgent::fold_notes)).
    ///
    /// Durable roster state (carried in the [`Checkpoint`](Self::Checkpoint)
    /// snapshot via the roster), emitted whenever the agent observes an agent's
    /// scratchpad cells change. The notes themselves are also re-derivable from
    /// the agent's tier-② persistence, so a dropped delta self-heals on the next
    /// change. The twin of [`TaskListChanged`](Self::TaskListChanged).
    #[serde(rename = "notes_changed")]
    NotesChanged {
        /// The agent whose note list changed.
        #[serde(rename = "agent_id", alias = "thread_id")]
        agent_id: String,
        /// The agent's complete current scratchpad-note list.
        notes: Vec<WireNote>,
    },

    /// The agent's *focused* agent changed — which agent it is actively
    /// working right now (the UI highlights it). This is ephemeral, disposable
    /// UI state in the same class as [`PhaseTransition`](Self::PhaseTransition):
    /// it rides the **best-effort** durability path
    /// ([`Durability::of`](../../../cp_oplog/service/enum.Durability.html#method.of)),
    /// is **not** carried in a [`Checkpoint`](Self::Checkpoint) snapshot, and
    /// self-heals — a dropped or post-restart-missing focus is re-served from
    /// the agent's tier-② `FocusState` on the next disk read and re-emitted on
    /// the next focus change.
    #[serde(rename = "agent_focus_changed", alias = "thread_focus_changed")]
    AgentFocusChanged {
        /// The newly-focused agent, or `None` when focus was released (the
        /// agent is no longer actively working any single agent).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[serde(rename = "agent_id", alias = "thread_id")]
        agent_id: Option<String>,
    },

    /// Agent lifecycle state changed (boot, shutdown, etc.).
    #[serde(rename = "lifecycle")]
    Lifecycle {
        /// The new lifecycle state.
        state: LifecycleState,
    },

    /// Periodic cost snapshot — cumulative input/output tokens and spend.
    #[serde(rename = "cost_aggregate")]
    CostAggregate {
        /// Cumulative input tokens since agent boot.
        input_tokens: u64,
        /// Cumulative output tokens since agent boot.
        output_tokens: u64,
        /// Cumulative spend in USD since agent boot.
        cost_usd: f64,
    },

    /// The agent's live **context-window occupancy** — the authoritative
    /// `used / threshold / budget` token triple the agent itself computes and
    /// renders (the TUI sidebar's `167K / 190K / 200K` line and the Statistics
    /// header). Emitted on change so an observer (the web HUD) shows the
    /// *identical* figure the agent shows, with no re-computation that could
    /// drift — the only way to satisfy "the web meter must match ratatui
    /// exactly" (T297).
    ///
    /// Ephemeral, disposable working-set state in the same class as
    /// [`PhaseTransition`](Self::PhaseTransition): it rides the **best-effort**
    /// durability path ([`Durability::of`](../../../cp_oplog/service/enum.Durability.html#method.of)),
    /// is **not** carried in a [`Checkpoint`](Self::Checkpoint) snapshot, and
    /// self-heals — a dropped sample is superseded by the next emission and a
    /// cold backend simply shows `0` until the agent re-emits.
    #[serde(rename = "context_usage")]
    ContextUsage {
        /// Tokens currently occupying the context window (system prompt ×2 +
        /// tool definitions + every panel + chat — the agent's own sum).
        used_tokens: u64,
        /// The cleaning threshold (the middle figure; reverie triggers here).
        threshold_tokens: u64,
        /// The hard context budget (the denominator the meter fills toward).
        budget_tokens: u64,
        /// The **cache-hit** half of `used_tokens` — the stable always-cached
        /// prefix (system prompt ×2 + tool definitions) plus every panel the
        /// provider served from cache this turn. `hit + miss == used` exactly.
        /// The web HUD splits its `Used` figure into `Used (hit)` / `Used
        /// (miss)` from this, byte-identical to the ratatui sidebar's green/
        /// amber token-bar segments (T297). `#[serde(default)]` keeps the field
        /// optional for N-1 readers that predate the split (they see `0`).
        #[serde(default)]
        hit_tokens: u64,
        /// The **cache-miss** half of `used_tokens` — every panel (re)sent
        /// uncached this turn. See [`hit_tokens`](Self::ContextUsage::hit_tokens).
        #[serde(default)]
        miss_tokens: u64,
    },

    /// The agent's active **system** (persona prompt) changed — a new
    /// library system was loaded (or reverted to default) via the `system_load`
    /// tool or the web footer's `LoadBehaviour` command.
    ///
    /// Ephemeral, disposable UI signal in the same class as
    /// [`PhaseTransition`](Self::PhaseTransition): it rides the **best-effort**
    /// durability path, is **not** carried in a [`Checkpoint`](Self::Checkpoint)
    /// snapshot, and self-heals — a dropped delta is covered by the coarse
    /// `config.json` mtime backstop. Observers do **not** fold it (the active
    /// behaviour is a tier-② `config.json` read, not view state); they use it to
    /// **invalidate** their cached library/behaviour view so the next read
    /// surfaces the fresh active system — the event-driven twin of the mtime
    /// backstop, matching the thread-roster push discipline (T581).
    #[serde(rename = "behaviour_changed")]
    BehaviourChanged {
        /// The newly-active system (persona) id, or `None` on revert-to-default.
        /// Informational — an observer may simply refetch the library on any
        /// `behaviour_changed`, ignoring the payload.
        #[serde(default, alias = "agent_id", skip_serializing_if = "Option::is_none")]
        system_id: Option<String>,
    },

    /// State checkpoint — bounds replay length on restart (GAP 1 / I5).
    #[serde(rename = "checkpoint")]
    Checkpoint {
        /// Full recoverable snapshot (heads + seen-set) at this rev.
        snapshot: Snapshot,
    },

    /// Catch-all for variants from a newer protocol version.
    #[serde(other)]
    Unknown,
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
