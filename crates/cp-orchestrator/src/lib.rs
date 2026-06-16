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
//! * [`channel`] — the per-agent [`AgentChannel`](channel::AgentChannel): oplog
//!   tail ([`Tailer`](channel::Tailer)), rev-pinned body hydrate, and command
//!   send.
//! * [`supervisor`] — the
//!   [`AgentSupervisor`](supervisor::AgentSupervisor): spawn / stop / restart /
//!   adopt of agent processes.
//! * [`services`] — the runtime services layer:
//!   [`MaterializedView`](services::MaterializedView) (fleet-state projection),
//!   [`CostBreaker`](services::CostBreaker) (durable spend breaker), and
//!   [`StreamHub`](services::StreamHub) (stream fan-out).

pub mod channel;
pub mod liveness;
pub mod registry;
pub mod services;
pub mod supervisor;
pub mod transport;
