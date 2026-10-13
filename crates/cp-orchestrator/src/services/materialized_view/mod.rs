//! In-memory **materialized view** of the fleet — the backend's current-state
//! projection, rebuilt from each agent's oplog on restart.
//!
//! The view holds, per agent, the bounded state a UI needs *now*: the content
//! [`Heads`], the latest [`Phase`] and [`LifecycleState`], and the most recent
//! cost aggregate. It is folded from the [`OpEntry`] stream the
//! [`Tailer`](crate::tailer::Tailer) delivers.
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
//! [`FleetView::apply`].

use std::collections::HashMap;

use cp_wire::types::oplog::{OpEntry, OpEntryKind};
use cp_wire::types::snapshot::{Heads, RosterAgent};
use cp_wire::types::{LifecycleState, Phase};

/// One agent's roster entry — the lightweight per-agent metadata the
/// `/agents` endpoint serves directly from the view.
///
/// No tier-② disk read (design doc §16 journals agent create/archive/restore;
/// I5 keeps live reads off disk).
///
/// This is [`cp_wire::types::snapshot::RosterAgent`] — the *same* type the
/// checkpoint carries — so a roster rebuilt by folding live deltas and one
/// restored wholesale from a checkpoint are byte-identical, and the
/// checkpoint-restore is a plain clone (see [`FleetView::apply`]).
pub type RosterEntry = RosterAgent;

/// The latest cumulative cost figures reported by an agent.
///
/// A [`CostAggregate`](OpEntryKind::CostAggregate) is cumulative-since-boot, so
/// the **latest** entry supersedes earlier ones (the figures are not summed).
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, utoipa::ToSchema)]
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
pub struct ContextSnapshot {
    /// Tokens currently occupying the context window.
    #[serde(rename = "used_tokens")]
    pub used: u64,
    /// Cleaning threshold (reverie trigger point).
    #[serde(rename = "threshold_tokens")]
    pub threshold: u64,
    /// Hard context budget.
    #[serde(rename = "budget_tokens")]
    pub budget: u64,
    /// The cache-hit half of `used` (always-cached prefix + cached
    /// panels); `hit + miss == used`. Powers the web HUD's
    /// `Used (hit)` / `Used (miss)` split, identical to the ratatui token bar.
    #[serde(rename = "hit_tokens")]
    pub hit: u64,
    /// The cache-miss half of `used` (uncached panels this turn).
    #[serde(rename = "miss_tokens")]
    pub miss: u64,
}

impl ContextSnapshot {
    /// Assemble a snapshot from the `(used, threshold, budget, hit, miss)`
    /// figures carried by a [`ContextUsage`](OpEntryKind::ContextUsage) entry.
    /// Takes the five as a tuple so the constructor stays within the
    /// four-argument cap.
    #[must_use]
    const fn from_usage(figures: (u64, u64, u64, u64, u64)) -> Self {
        let (used, threshold, budget, hit, miss) = figures;
        Self { used, threshold, budget, hit, miss }
    }
}

/// One agent's current projected state.
///
/// Folded from the agent's [`OpEntry`] stream; every field reflects the most
/// recent relevant entry (heads accumulate per agent, the scalars are
/// latest-wins).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct FleetView {
    /// Highest `rev` folded into this view so far.
    pub rev: u64,

    /// Per-thread / per-panel content heads as of `rev`.
    pub heads: Heads,

    /// Agent roster — the live list `/agents` serves from (folded from the
    /// thread-roster deltas; design doc I5).
    pub roster: Vec<RosterEntry>,

    /// The agent the agent is currently *focused* on (actively working), or
    /// `None` when focus is released. Folded latest-wins from
    /// [`AgentFocusChanged`](OpEntryKind::AgentFocusChanged) — ephemeral,
    /// disposable UI state (like [`phase`](Self::phase)): it is **not** carried
    /// in a checkpoint, so after a backend cold start it is `None` until the
    /// agent emits its next focus change (the `/agents` handler falls back to
    /// the agent's tier-② `FocusState` to cover that window).
    #[serde(rename = "focused_thread_id")]
    pub focused_agent_id: Option<String>,

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

impl FleetView {
    /// Fold one oplog entry into this view, advancing `rev`.
    ///
    /// A [`Checkpoint`](OpEntryKind::Checkpoint) replaces the heads **and the
    /// roster** wholesale (authoritative reset, the mechanism behind
    /// count-bounded restart — a backend that cold-starts after oplog
    /// compaction rebuilds the agent list from the checkpoint, not from the
    /// original deltas); a [`MessageCreated`](OpEntryKind::MessageCreated)
    /// updates a single agent head and bumps that agent's roster activity;
    /// the thread-roster deltas (`AgentCreated`/`Archived`/`Restored`/
    /// `StatusChanged`) maintain the roster idempotently via the shared
    /// [`RosterAgent`] fold helpers (so this fold and the agent's replay fold
    /// stay identical); phase, lifecycle, cost, and focus are latest-wins.
    /// Durability-only records (`CommandEffect`, `SeenMark`) and forward-compat
    /// `Unknown` variants do not affect the projected state.
    pub fn apply(&mut self, entry: &OpEntry) {
        self.rev = self.rev.max(entry.rev);
        cp_macros::deref_match!(&entry.kind, {
            OpEntryKind::Checkpoint { ref snapshot } => {
                self.heads = snapshot.heads.clone();
                self.roster.clone_from(&snapshot.roster);
            }
            OpEntryKind::MessageCreated { ref agent_id, head, .. } => {
                self.heads.set_agent_head(agent_id, head);
                RosterAgent::fold_message(&mut self.roster, agent_id, entry.timestamp_ms);
            }
            OpEntryKind::AgentCreated { ref agent_id, ref name, status, timestamp_ms, .. } => {
                RosterAgent::fold_created(
                    &mut self.roster,
                    cp_wire::types::snapshot::AgentCreation::new(agent_id, name, status, timestamp_ms),
                );
            }
            OpEntryKind::AgentArchived { ref agent_id } => {
                RosterAgent::fold_archived(&mut self.roster, agent_id, true);
            }
            OpEntryKind::AgentRestored { ref agent_id } => {
                RosterAgent::fold_archived(&mut self.roster, agent_id, false);
            }
            OpEntryKind::AgentPaused { ref agent_id } => {
                RosterAgent::fold_paused(&mut self.roster, agent_id, true);
            }
            OpEntryKind::AgentResumed { ref agent_id } => {
                RosterAgent::fold_paused(&mut self.roster, agent_id, false);
            }
            OpEntryKind::AgentDeleted { ref agent_id } => {
                RosterAgent::fold_deleted(&mut self.roster, agent_id);
            }
            OpEntryKind::AgentStatusChanged { ref agent_id, status } => {
                RosterAgent::fold_status(&mut self.roster, agent_id, status);
            }
            OpEntryKind::TaskListChanged { ref agent_id, ref tasks } => {
                RosterAgent::fold_tasks(&mut self.roster, agent_id, tasks.clone());
            }
            OpEntryKind::NotesChanged { ref agent_id, ref notes } => {
                RosterAgent::fold_notes(&mut self.roster, agent_id, notes.clone());
            }
            OpEntryKind::AgentFocusChanged { ref agent_id } => {
                self.focused_agent_id.clone_from(agent_id);
            }
            OpEntryKind::PhaseTransition { phase } => self.phase = Some(phase),
            OpEntryKind::Lifecycle { state } => self.lifecycle = Some(state),
            OpEntryKind::CostAggregate { input_tokens, output_tokens, cost_usd } => {
                self.cost = CostSnapshot { input_tokens, output_tokens, cost_usd };
            }
            OpEntryKind::ContextUsage { used_tokens, threshold_tokens, budget_tokens, hit_tokens, miss_tokens } => {
                self.context =
                    ContextSnapshot::from_usage((used_tokens, threshold_tokens, budget_tokens, hit_tokens, miss_tokens));
            }
            // Durability-only records, message-delete, behaviour change,
            // and forward-compat unknowns do not affect the projected state.
            OpEntryKind::CommandEffect { .. }
            | OpEntryKind::SeenMark { .. }
            | OpEntryKind::MessageDeleted { .. }
            | OpEntryKind::BehaviourChanged { .. }
            | OpEntryKind::Unknown => {}
        });
    }
}

/// The fleet-wide materialized view: one [`FleetView`] per known agent.
///
/// Driven by the orchestrator loop, which polls each agent's
/// [`Tailer`](crate::tailer::Tailer) and feeds the resulting entries through
/// [`apply`](MaterializedView::apply).
#[derive(Debug, Default)]
pub struct MaterializedView {
    /// Per-agent projected state, keyed by `fleet_id`.
    agents: HashMap<String, FleetView>,
}

impl MaterializedView {
    /// Create an empty view.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold one entry into `fleet_id`'s view, creating it on first sight.
    pub fn apply(&mut self, fleet_id: &str, entry: &OpEntry) {
        self.agents.entry(fleet_id.to_owned()).or_default().apply(entry);
    }

    /// Fold a batch of entries (as returned by one
    /// [`Tailer::poll`](crate::tailer::Tailer::poll)) into `fleet_id`'s view.
    pub fn apply_batch(&mut self, fleet_id: &str, entries: &[OpEntry]) {
        if entries.is_empty() {
            return;
        }
        let view = self.agents.entry(fleet_id.to_owned()).or_default();
        for entry in entries {
            view.apply(entry);
        }
    }

    /// The current projection for `fleet_id`, or `None` if unknown.
    #[must_use]
    pub fn get(&self, fleet_id: &str) -> Option<&FleetView> {
        self.agents.get(fleet_id)
    }

    /// Drop an agent's projection (called when discovery reports it gone).
    ///
    /// Returns the removed view, if any.
    pub fn remove(&mut self, fleet_id: &str) -> Option<FleetView> {
        self.agents.remove(fleet_id)
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
    pub fn fleet_ids(&self) -> impl Iterator<Item = &str> {
        self.agents.keys().map(String::as_str)
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
