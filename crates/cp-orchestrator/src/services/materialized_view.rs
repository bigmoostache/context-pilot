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
use cp_wire::types::snapshot::Heads;
use cp_wire::types::{LifecycleState, Phase};

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

    /// Most recent execution phase, or `None` before any phase transition.
    pub phase: Option<Phase>,

    /// Most recent lifecycle state, or `None` before any lifecycle entry.
    pub lifecycle: Option<LifecycleState>,

    /// Latest cumulative cost figures.
    pub cost: CostSnapshot,
}

impl AgentView {
    /// Fold one oplog entry into this view, advancing `rev`.
    ///
    /// A [`Checkpoint`](OpEntryKind::Checkpoint) replaces the heads wholesale
    /// (authoritative reset, the mechanism behind count-bounded restart); a
    /// [`MessageCreated`](OpEntryKind::MessageCreated) updates a single thread
    /// head; phase, lifecycle, and cost are latest-wins. Durability-only
    /// records (`CommandEffect`, `SeenMark`) and forward-compat `Unknown`
    /// variants do not affect the projected state.
    pub fn apply(&mut self, entry: &OpEntry) {
        self.rev = self.rev.max(entry.rev);
        match &entry.kind {
            OpEntryKind::Checkpoint { snapshot } => {
                self.heads = snapshot.heads.clone();
            }
            OpEntryKind::MessageCreated { thread_id, head, .. } => {
                self.heads.set_thread_head(thread_id, *head);
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
mod tests {
    use super::*;
    use cp_wire::types::snapshot::Snapshot;
    use cp_wire::types::ContentHash;

    /// Build an [`OpEntry`] with the given rev and kind.
    fn entry(rev: u64, kind: OpEntryKind) -> OpEntry {
        OpEntry { schema_version: 1, rev, timestamp_ms: 0, kind }
    }

    fn message(thread: &str, byte: u8) -> OpEntryKind {
        OpEntryKind::MessageCreated {
            thread_id: thread.to_owned(),
            message_id: format!("m{byte}"),
            head: ContentHash::new([byte; 32]),
        }
    }

    #[test]
    fn message_created_sets_thread_head() {
        let mut view = MaterializedView::new();
        view.apply("a1", &entry(0, message("T1", 0x11)));

        let agent = view.get("a1").expect("agent present");
        assert_eq!(agent.rev, 0);
        let head = agent
            .heads
            .threads
            .iter()
            .find(|h| h.thread_id == "T1")
            .expect("thread head present");
        assert_eq!(head.last_message_hash, ContentHash::new([0x11; 32]));
    }

    #[test]
    fn checkpoint_resets_heads_authoritatively() {
        let mut view = MaterializedView::new();
        // Stale head folded first.
        view.apply("a1", &entry(0, message("T1", 0x11)));

        // A checkpoint carrying a different head set must replace, not merge.
        let mut snapshot = Snapshot::default();
        snapshot.heads.set_thread_head("T2", ContentHash::new([0x22; 32]));
        view.apply("a1", &entry(5, OpEntryKind::Checkpoint { snapshot }));

        let agent = view.get("a1").expect("agent present");
        assert_eq!(agent.rev, 5);
        assert!(
            agent.heads.threads.iter().all(|h| h.thread_id != "T1"),
            "checkpoint must drop the pre-checkpoint head",
        );
        assert_eq!(agent.heads.threads.len(), 1);
        assert_eq!(agent.heads.threads.first().expect("T2").thread_id, "T2");
    }

    #[test]
    fn phase_and_lifecycle_are_latest_wins() {
        let mut view = MaterializedView::new();
        view.apply("a1", &entry(0, OpEntryKind::PhaseTransition { phase: Phase::Streaming }));
        view.apply("a1", &entry(1, OpEntryKind::PhaseTransition { phase: Phase::Tooling }));
        view.apply("a1", &entry(2, OpEntryKind::Lifecycle { state: LifecycleState::Running }));

        let agent = view.get("a1").expect("agent present");
        assert_eq!(agent.phase, Some(Phase::Tooling));
        assert_eq!(agent.lifecycle, Some(LifecycleState::Running));
    }

    #[test]
    fn cost_aggregate_is_latest_not_summed() {
        let mut view = MaterializedView::new();
        view.apply(
            "a1",
            &entry(0, OpEntryKind::CostAggregate { input_tokens: 100, output_tokens: 10, cost_usd: 1.0 }),
        );
        view.apply(
            "a1",
            &entry(1, OpEntryKind::CostAggregate { input_tokens: 250, output_tokens: 30, cost_usd: 2.5 }),
        );

        let agent = view.get("a1").expect("agent present");
        // Cumulative-since-boot ⇒ latest wins, never 350/40/3.5.
        assert_eq!(agent.cost.input_tokens, 250);
        assert_eq!(agent.cost.output_tokens, 30);
        assert!((agent.cost.cost_usd - 2.5).abs() < f64::EPSILON);
    }

    #[test]
    fn apply_batch_folds_in_order_and_tracks_max_rev() {
        let mut view = MaterializedView::new();
        let batch = [
            entry(3, message("T1", 0x01)),
            entry(7, message("T1", 0x02)),
            entry(9, OpEntryKind::PhaseTransition { phase: Phase::Idle }),
        ];
        view.apply_batch("a1", &batch);

        let agent = view.get("a1").expect("agent present");
        assert_eq!(agent.rev, 9, "rev tracks the maximum folded");
        assert_eq!(
            agent.heads.threads.first().expect("T1").last_message_hash,
            ContentHash::new([0x02; 32]),
            "later message overwrites the earlier head",
        );
        assert_eq!(agent.phase, Some(Phase::Idle));
    }

    #[test]
    fn durability_only_and_unknown_entries_are_inert() {
        let mut view = MaterializedView::new();
        view.apply(
            "a1",
            &entry(0, OpEntryKind::CommandEffect { cmd_id: "c".into(), dedup_token: "d".into() }),
        );
        view.apply("a1", &entry(1, OpEntryKind::SeenMark { dedup_token: "d".into() }));
        view.apply("a1", &entry(2, OpEntryKind::Unknown));

        let agent = view.get("a1").expect("agent present");
        assert_eq!(agent.rev, 2, "rev still advances");
        assert!(agent.heads.threads.is_empty());
        assert_eq!(agent.phase, None);
        assert_eq!(agent.lifecycle, None);
        assert_eq!(agent.cost, CostSnapshot::default());
    }

    #[test]
    fn remove_drops_agent() {
        let mut view = MaterializedView::new();
        view.apply("a1", &entry(0, message("T1", 0x11)));
        assert_eq!(view.len(), 1);

        let removed = view.remove("a1");
        assert!(removed.is_some());
        assert!(view.is_empty());
        assert!(view.get("a1").is_none());
    }
}
