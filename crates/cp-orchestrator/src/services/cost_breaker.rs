//! Durable **cost breaker** — trips an agent that has exceeded its spend
//! budget, and stays tripped across a crash-loop.
//!
//! The breaker's truth is the agent's oplog: each
//! [`CostAggregate`](cp_wire::types::oplog::OpEntryKind::CostAggregate) entry is
//! cumulative-since-boot and `fdatasync`'d by the agent before the backend ever
//! sees it. The breaker folds those figures into a per-agent high-water mark,
//! so on backend restart it rebuilds the same trip state from the same durable
//! data — a tripped agent can never silently un-trip by losing the backend's
//! memory (design doc R2-8, fault V9).
//!
//! # Two safety properties
//!
//! * **Crash-loop latch.** Spend is tracked as a **high-water maximum**, never
//!   "latest". An agent that overspends, is `kill -9`'d, and re-boots reports a
//!   fresh cumulative cost of `0` — which cannot lower the mark, so the trip
//!   survives the loop ([`observe`](CostBreaker::observe)).
//! * **Fail-closed.** If the backend cannot read an agent's durable cost (an
//!   I/O fault), the agent is [`poison`](CostBreaker::poison)ed and
//!   [`check`](CostBreaker::check) returns [`Verdict::Tripped`] for it — under
//!   uncertainty the breaker denies, it does not allow.

use std::collections::{HashMap, HashSet};

use crate::services::materialized_view::MaterializedView;

/// The breaker's decision for one agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The agent is within budget and its cost is known — proceed.
    Allowed,
    /// The agent has exceeded budget, or its cost is indeterminate
    /// (fail-closed) — block.
    Tripped,
}

impl Verdict {
    /// Whether this verdict blocks the agent.
    #[must_use]
    pub const fn is_tripped(self) -> bool {
        matches!(self, Self::Tripped)
    }
}

/// Per-agent durable spend breaker.
#[derive(Debug)]
pub struct CostBreaker {
    /// Maximum cumulative USD an agent may spend before tripping.
    budget_usd: f64,

    /// High-water cumulative spend per agent (monotonic — see
    /// [`observe`](Self::observe)).
    spend: HashMap<String, f64>,

    /// Agents whose durable cost could not be read; forced
    /// [`Tripped`](Verdict::Tripped) until cleared (fail-closed).
    poisoned: HashSet<String>,
}

impl CostBreaker {
    /// Create a breaker with the given per-agent budget (USD).
    #[must_use]
    pub fn new(budget_usd: f64) -> Self {
        Self { budget_usd, spend: HashMap::new(), poisoned: HashSet::new() }
    }

    /// Record an agent's latest cumulative cost.
    ///
    /// Stored as a **high-water maximum**: a lower figure (e.g. `0` after a
    /// re-boot) never lowers the recorded spend, which is what makes a trip
    /// survive a crash-loop (V9).
    pub fn observe(&mut self, agent_id: &str, cumulative_cost_usd: f64) {
        let slot = self.spend.entry(agent_id.to_owned()).or_insert(0.0);
        if cumulative_cost_usd > *slot {
            *slot = cumulative_cost_usd;
        }
    }

    /// Re-derive spend for every agent from a materialized view.
    ///
    /// Used on backend restart: the view itself is rebuilt from the durable
    /// oplog, so folding its cost figures reconstructs the exact trip state the
    /// backend held before it died.
    pub fn rebuild_from_view(&mut self, view: &MaterializedView) {
        for agent_id in view.agent_ids() {
            if let Some(agent) = view.get(agent_id) {
                self.observe(agent_id, agent.cost.cost_usd);
            }
        }
    }

    /// Mark an agent's durable cost as unreadable — fail-closed until cleared.
    pub fn poison(&mut self, agent_id: &str) {
        let _inserted = self.poisoned.insert(agent_id.to_owned());
    }

    /// Clear a previously-[`poison`](Self::poison)ed agent (its cost became
    /// readable again).
    pub fn clear_poison(&mut self, agent_id: &str) {
        let _present = self.poisoned.remove(agent_id);
    }

    /// The breaker's verdict for `agent_id`.
    ///
    /// Order of checks: a poisoned agent is always tripped (fail-closed); else
    /// a recorded spend strictly over budget trips; else allowed (this includes
    /// an agent with no recorded cost yet — a definite zero, not uncertainty).
    #[must_use]
    pub fn check(&self, agent_id: &str) -> Verdict {
        if self.poisoned.contains(agent_id) {
            return Verdict::Tripped;
        }
        match self.spend.get(agent_id) {
            Some(&spent) if spent > self.budget_usd => Verdict::Tripped,
            _ => Verdict::Allowed,
        }
    }

    /// Whether `agent_id` is currently tripped.
    #[must_use]
    pub fn is_tripped(&self, agent_id: &str) -> bool {
        self.check(agent_id).is_tripped()
    }

    /// The recorded high-water spend for `agent_id`, if any.
    #[must_use]
    pub fn spend_of(&self, agent_id: &str) -> Option<f64> {
        self.spend.get(agent_id).copied()
    }

    /// The configured per-agent budget.
    #[must_use]
    pub const fn budget(&self) -> f64 {
        self.budget_usd
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cp_wire::types::oplog::{OpEntry, OpEntryKind};

    fn cost_entry(rev: u64, cost_usd: f64) -> OpEntry {
        OpEntry {
            schema_version: 1,
            rev,
            timestamp_ms: 0,
            kind: OpEntryKind::CostAggregate { input_tokens: 0, output_tokens: 0, cost_usd },
        }
    }

    #[test]
    fn under_budget_is_allowed() {
        let mut breaker = CostBreaker::new(5.0);
        breaker.observe("a1", 3.0);
        assert_eq!(breaker.check("a1"), Verdict::Allowed);
        assert!(!breaker.is_tripped("a1"));
    }

    #[test]
    fn over_budget_trips() {
        let mut breaker = CostBreaker::new(5.0);
        breaker.observe("a1", 7.0);
        assert_eq!(breaker.check("a1"), Verdict::Tripped);
        assert!(breaker.is_tripped("a1"));
    }

    #[test]
    fn unknown_agent_is_allowed_not_failed_closed() {
        // No cost entry yet ⇒ definite zero spend ⇒ allowed (uncertainty is a
        // *poison*, which is a different state).
        let breaker = CostBreaker::new(5.0);
        assert_eq!(breaker.check("never-seen"), Verdict::Allowed);
    }

    #[test]
    fn poison_fails_closed_even_with_zero_spend() {
        let mut breaker = CostBreaker::new(5.0);
        breaker.poison("a2");
        assert_eq!(breaker.check("a2"), Verdict::Tripped, "indeterminate ⇒ deny");

        breaker.clear_poison("a2");
        assert_eq!(breaker.check("a2"), Verdict::Allowed, "readable again ⇒ allow");
    }

    #[test]
    fn trip_survives_crash_loop_via_high_water_mark() {
        // V9: agent overspends, is killed, re-boots reporting cumulative 0.
        let mut breaker = CostBreaker::new(5.0);
        breaker.observe("a1", 8.0); // overspent in boot #1
        assert!(breaker.is_tripped("a1"));

        breaker.observe("a1", 0.0); // boot #2 — cumulative reset to 0
        assert!(breaker.is_tripped("a1"), "a lower post-reboot cost must not lower the high-water mark",);
        assert_eq!(breaker.spend_of("a1"), Some(8.0));
    }

    #[test]
    fn trip_rebuilds_from_durable_view_after_restart() {
        // Build a view from durable cost entries, then rebuild a *fresh*
        // breaker from it — the trip is reconstructed, never lost.
        let mut view = MaterializedView::new();
        view.apply("a1", &cost_entry(0, 9.0));

        let mut fresh_breaker = CostBreaker::new(5.0);
        assert_eq!(fresh_breaker.check("a1"), Verdict::Allowed, "empty before rebuild");
        fresh_breaker.rebuild_from_view(&view);
        assert_eq!(
            fresh_breaker.check("a1"),
            Verdict::Tripped,
            "trip state survives a backend restart that rebuilds from the oplog",
        );
    }
}
