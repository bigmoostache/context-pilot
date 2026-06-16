//! Standalone orchestration backend library — fleet discovery, observation, and
//! command, split from the [`main`](../main.rs) binary so its machinery is unit-
//! testable without spawning a process.
//!
//! This is the **backend** half of the orchestration architecture (design doc
//! §4). It is the peer of the agent-side `cp-mod-bridge`: where the bridge
//! *writes* an agent's registry record, heartbeat, and oplog, the backend
//! *reads and tails* them across a whole fleet. The crate names that asymmetry
//! explicitly — every module here is backend-only and never linked into an
//! agent.
//!
//! # What lives here
//!
//! * [`liveness`] — the pure per-agent **liveness verdict** (live pid **and**
//!   fresh heartbeat **and** matching `boot_id`), the decision at the heart of
//!   discovery.
//! * [`registry`] — the **`AgentRegistry`** (design doc §10, roadmap P5-T1):
//!   scans `~/.context-pilot/agents/`, applies the verdict to each record, and
//!   diffs successive passes into appeared / disappeared / status-changed /
//!   stale events.
//!
//! Later phases add the per-agent oplog tail + hydrate (`AgentChannel`),
//! lifecycle control (`AgentSupervisor`), and the `StreamHub` / `CostBreaker` /
//! `MaterializedView` services.

pub mod liveness;
pub mod registry;
