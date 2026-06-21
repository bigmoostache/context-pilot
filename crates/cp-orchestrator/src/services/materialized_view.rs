//! In-memory **materialized view** of the fleet — the backend's current-state
//! projection, rebuilt from each agent's oplog on restart.
//!
//! The view holds, per agent, the bounded state a UI needs *now*: the content
//! [`Heads`], the latest [`Phase`] and [`LifecycleState`], and the most recent
//! cost aggregate. It is folded from the [`OpEntry`] stream the
//! [`Tailer`](crate::channel::Tailer) delivers.
//!
//! # Restart latency scales with agent *count*, not disk (I5)
//!
//! A [`Checkpoint`](OpEntryKind::Checkpoint) entry carries a full
//! [`Snapshot`](cp_wire::types::snapshot::Snapshot) and is folded as an
//! **authoritative reset** of an agent's heads. Because every rolled oplog
//! segment opens with such a checkpoint, replaying only an agent's newest
//! segment reconstructs its complete heads — so rebuilding the whole view costs
//! one short replay per agent, independent of total oplog size on disk. The
//! fold here is what makes that guarantee real at the view layer: see
//! [`AgentView::apply`].

use std::collections::HashMap;

use cp_wire::types::oplog::{OpEntry, OpEntryKind};
use cp_wire::types::snapshot::{Heads, RosterThread};
use cp_wire::types::{LifecycleState, Phase};

/// One thread's roster entry — the lightweight per-thread metadata the
/// `/threads` endpoint serves directly from the view, with no tier-② disk read
/// (design doc §16 journals thread create/archive/restore; I5 keeps live reads
/// off disk).
///
/// This is [`cp_wire::types::snapshot::RosterThread`] — the *same* type the
/// checkpoint carries — so a roster rebuilt by folding live deltas and one
/// restored wholesale from a checkpoint are byte-identical, and the
/// checkpoint-restore is a plain clone (see [`AgentView::apply`]).
pub type RosterEntry = RosterThread;

/// The latest cumulative cost figures reported by an agent.
///
/// A [`CostAggregate`](OpEntryKind::CostAggregate) is cumulative-since-boot, so
/// the **latest** entry supersedes earlier ones (the figures are not summed).
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize)]
pub struct CostSnapshot {
    /// Cumulative input tokens since the agent booted.
    pub input_tokens: u64,
    /// Cumulative output tokens since the agent booted.
    pub output_tokens: u64,
    /// Cumulative spend in USD since the agent booted.
    pub cost_usd: f64,
}

/// One agent's live **context-window occupancy** — the authoritative
/// `used / threshold / budget` token triple the agent computes and renders.
///
/// Folded latest-wins from [`ContextUsage`](OpEntryKind::ContextUsage). Like
/// [`CostSnapshot`] the latest entry supersedes earlier ones (cumulative state,
/// not summed). Ephemeral — not carried in a checkpoint, so a cold backend
/// shows zeros until the agent re-emits (the meter self-heals).
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize)]
pub struct ContextSnapshot {
    /// Tokens currently occupying the context window.
    pub used_tokens: u64,
    /// Cleaning threshold (reverie trigger point).
    pub threshold_tokens: u64,
    /// Hard context budget.
    pub budget_tokens: u64,
}

/// One agent's current projected state.
///
/// Folded from the agent's [`OpEntry`] stream; every field reflects the most
/// recent relevant entry (heads accumulate per thread, the scalars are
/// latest-wins).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct AgentView {
    /// Highest `rev` folded into this view so far.
    pub rev: u64,

    /// Per-thread / per-panel content heads as of `rev`.
    pub heads: Heads,

    /// Thread roster — the live list `/threads` serves from (folded from the
    /// thread-roster deltas; design doc I5).
    pub roster: Vec<RosterEntry>,

    /// The thread the agent is currently *focused* on (actively working), or
    /// `None` when focus is released. Folded latest-wins from
    /// [`ThreadFocusChanged`](OpEntryKind::ThreadFocusChanged) — ephemeral,
    /// disposable UI state (like [`phase`](Self::phase)): it is **not** carried
    /// in a checkpoint, so after a backend cold start it is `None` until the
    /// agent emits its next focus change (the `/threads` handler falls back to
    /// the agent's tier-② `FocusState` to cover that window).
    pub focused_thread_id: Option<String>,

    /// Most recent execution phase, or `None` before any phase transition.
    pub phase: Option<Phase>,

    /// Most recent lifecycle state, or `None` before any lifecycle entry.
    pub lifecycle: Option<LifecycleState>,

    /// Latest cumulative cost figures.
    pub cost: CostSnapshot,

    /// Latest context-window occupancy (`used`/`threshold`/`budget`). Folded
    /// latest-wins from [`ContextUsage`](OpEntryKind::ContextUsage); ephemeral
    /// (not checkpoint-carried), so it is `0`-valued after a cold start until
    /// the agent emits its next sample.
    pub context: ContextSnapshot,
}

impl AgentView {
    /// Fold one oplog entry into this view, advancing `rev`.
    ///
    /// A [`Checkpoint`](OpEntryKind::Checkpoint) replaces the heads **and the
    /// roster** wholesale (authoritative reset, the mechanism behind
    /// count-bounded restart — a backend that cold-starts after oplog
    /// compaction rebuilds the thread list from the checkpoint, not from the
    /// original deltas); a [`MessageCreated`](OpEntryKind::MessageCreated)
    /// updates a single thread head and bumps that thread's roster activity;
    /// the thread-roster deltas (`ThreadCreated`/`Archived`/`Restored`/
    /// `StatusChanged`) maintain the roster idempotently via the shared
    /// [`RosterThread`] fold helpers (so this fold and the agent's replay fold
    /// stay identical); phase, lifecycle, cost, and focus are latest-wins.
    /// Durability-only records (`CommandEffect`, `SeenMark`) and forward-compat
    /// `Unknown` variants do not affect the projected state.
    pub fn apply(&mut self, entry: &OpEntry) {
        self.rev = self.rev.max(entry.rev);
        match &entry.kind {
            OpEntryKind::Checkpoint { snapshot } => {
                self.heads = snapshot.heads.clone();
                self.roster.clone_from(&snapshot.roster);
            }
            OpEntryKind::MessageCreated { thread_id, head, .. } => {
                self.heads.set_thread_head(thread_id, *head);
                RosterThread::fold_message(&mut self.roster, thread_id, entry.timestamp_ms);
            }
            OpEntryKind::ThreadCreated { thread_id, name, status, timestamp_ms } => {
                RosterThread::fold_created(
                    &mut self.roster,
                    cp_wire::types::snapshot::ThreadCreation {
                        thread_id,
                        name,
                        status: *status,
                        timestamp_ms: *timestamp_ms,
                    },
                );
            }
            OpEntryKind::ThreadArchived { thread_id } => {
                RosterThread::fold_archived(&mut self.roster, thread_id, true);
            }
            OpEntryKind::ThreadRestored { thread_id } => {
                RosterThread::fold_archived(&mut self.roster, thread_id, false);
            }
            OpEntryKind::ThreadStatusChanged { thread_id, status } => {
                RosterThread::fold_status(&mut self.roster, thread_id, *status);
            }
            OpEntryKind::ThreadFocusChanged { thread_id } => {
                self.focused_thread_id.clone_from(thread_id);
            }
            OpEntryKind::PhaseTransition { phase } => {
                self.phase = Some(*phase);
            }
            OpEntryKind::Lifecycle { state } => {
                self.lifecycle = Some(*state);
            }
            OpEntryKind::CostAggregate { input_tokens, output_tokens, cost_usd } => {
                self.cost = CostSnapshot {
                    input_tokens: *input_tokens,
                    output_tokens: *output_tokens,
                    cost_usd: *cost_usd,
                };
            }
            OpEntryKind::ContextUsage { used_tokens, threshold_tokens, budget_tokens } => {
                self.context = ContextSnapshot {
                    used_tokens: *used_tokens,
                    threshold_tokens: *threshold_tokens,
                    budget_tokens: *budget_tokens,
                };
            }
            // Durability-only records and forward-compat unknowns do not
            // affect the projected state.
            OpEntryKind::CommandEffect { .. }
            | OpEntryKind::SeenMark { .. }
            | OpEntryKind::Unknown => {}
        }
    }
}

/// The fleet-wide materialized view: one [`AgentView`] per known agent.
///
/// Driven by the orchestrator loop, which polls each agent's
/// [`Tailer`](crate::channel::Tailer) and feeds the resulting entries through
/// [`apply`](MaterializedView::apply).
#[derive(Debug, Default)]
pub struct MaterializedView {
    /// Per-agent projected state, keyed by `agent_id`.
    agents: HashMap<String, AgentView>,
}

impl MaterializedView {
    /// Create an empty view.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold one entry into `agent_id`'s view, creating it on first sight.
    pub fn apply(&mut self, agent_id: &str, entry: &OpEntry) {
        self.agents.entry(agent_id.to_owned()).or_default().apply(entry);
    }

    /// Fold a batch of entries (as returned by one
    /// [`Tailer::poll`](crate::channel::Tailer::poll)) into `agent_id`'s view.
    pub fn apply_batch(&mut self, agent_id: &str, entries: &[OpEntry]) {
        if entries.is_empty() {
            return;
        }
        let view = self.agents.entry(agent_id.to_owned()).or_default();
        for entry in entries {
            view.apply(entry);
        }
    }

    /// The current projection for `agent_id`, or `None` if unknown.
    #[must_use]
    pub fn get(&self, agent_id: &str) -> Option<&AgentView> {
        self.agents.get(agent_id)
    }

    /// Drop an agent's projection (called when discovery reports it gone).
    ///
    /// Returns the removed view, if any.
    pub fn remove(&mut self, agent_id: &str) -> Option<AgentView> {
        self.agents.remove(agent_id)
    }

    /// Number of agents currently projected.
    #[must_use]
    pub fn len(&self) -> usize {
        self.agents.len()
    }

    /// Whether the view holds no agents.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }

    /// Iterator over the projected agent ids.
    pub fn agent_ids(&self) -> impl Iterator<Item = &str> {
        self.agents.keys().map(String::as_str)
    }
}

#[cfg(test)]
#[path = "materialized_view_tests.rs"]
mod tests;
