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
//! * [`inspect`] — read-only, mtime-cached **inspection** of an agent's
//!   on-disk persistence files (tier-② state: config, workers, shared,
//!   messages, panels).
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

pub mod inspect;
pub mod registry;
pub mod runtime;
pub mod services;
pub mod supervisor;
pub mod transport;

// Re-export channel at the crate root so external consumers (tests, runtime)
// that imported `cp_orchestrator::channel` continue to compile unchanged.
pub use registry::channel;

// Re-export liveness at the crate root so external consumers (tests) that
// imported `cp_orchestrator::liveness` continue to compile unchanged.
pub use registry::liveness;

// `openssl` with the `vendored` feature compiles OpenSSL from source during
// cross-compilation (reqwest → native-tls → openssl-sys). Without a direct
// reference the per-target `unused-crate-dependencies` lint fires on the lib
// target. Neither lib nor bin code calls OpenSSL directly — the crate exists
// solely to activate vendored compilation.
use openssl as _;

// `dotenvy` loads `.env` files in the binary's `main()` — the lib half never
// calls it directly, so the per-target lint needs this acknowledgement.
use dotenvy as _;

// `cp-mod-bridge` is a dev-dependency the `tests/registry_channel.rs` integration
// suite uses to boot a real agent across the backend↔agent seam. The library's
// own `#[cfg(test)]` modules never name it, so the per-target
// `unused-crate-dependencies` lint on the lib-test target needs this explicit
// acknowledgement (the canonical `use … as _;` form, not a lint suppression).
#[cfg(test)]
use cp_mod_bridge as _;
