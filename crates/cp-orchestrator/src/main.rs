//! Standalone orchestration backend binary — discovers, observes, and commands
//! a fleet of Context Pilot agents.
//!
//! This is the **backend** half of the orchestration architecture (design doc
//! §4). Its machinery lives in the [`cp_orchestrator`] library (so it is
//! unit-testable without spawning processes); this binary is the thin entry
//! point that will wire those pieces into a runtime:
//!
//! - [`AgentRegistry`](cp_orchestrator::registry::AgentRegistry) — discovers
//!   agents via `~/.context-pilot/agents/` and derives per-agent liveness.
//! - `AgentChannel` — per-agent oplog tail + stream + command channel (later).
//! - `AgentSupervisor` — lifecycle (spawn/stop/restart), allow-list gated.
//! - `StreamHub` — fan-out from per-agent UDS to N frontend WebSockets.
//! - `CostBreaker` — durable fleet-wide spend circuit-breaker.
//! - `MaterializedView` — in-memory cache rebuilt from oplog heads.
//!
//! Phase 15: the registry exists and self-tests; the binary still boots, prints
//! its identity, and exits — the live scan loop arrives with the supervisor.

use cp_orchestrator::registry;

// `nix`, `serde`, `serde_json`, `cp_oplog`, and `tiny_http` are dependencies of
// this package's *library* half (`liveness`, `registry`, `services`,
// `transport`); the binary does not name them directly, so the per-target
// `unused-crate-dependencies` lint needs an explicit acknowledgement here (the
// canonical `use … as _;` form Cargo itself suggests, not a lint suppression).
// `tempfile` is a dev-dependency used only by the library's tests, acknowledged
// under `cfg(test)` so non-test builds never reference it.
use nix as _;
use cp_oplog as _;
use serde as _;
use serde_json as _;
use tiny_http as _;
#[cfg(test)]
use tempfile as _;

fn main() {
    eprintln!(
        "cp-orchestrator v{} (protocol v{})",
        env!("CARGO_PKG_VERSION"),
        cp_wire::PROTOCOL_VERSION,
    );
    match registry::default_agents_dir() {
        Ok(dir) => eprintln!("agents directory: {}", dir.display()),
        Err(e) => eprintln!("agents directory unavailable: {e}"),
    }
    eprintln!("Phase 15 — AgentRegistry ready; no runtime loop yet.");
}
