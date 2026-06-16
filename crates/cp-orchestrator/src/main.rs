//! Standalone orchestration backend — discovers, observes, and commands
//! a fleet of Context Pilot agents.
//!
//! This binary is the **backend** half of the orchestration architecture
//! (design doc §4).  It owns:
//!
//! - **`AgentRegistry`** — discovers agents via `~/.context-pilot/agents/`.
//! - **`AgentChannel`** — per-agent oplog tail + stream + command channel.
//! - **`AgentSupervisor`** — lifecycle (spawn/stop/restart), allow-list gated.
//! - **`StreamHub`** — fan-out from per-agent UDS to N frontend WebSockets.
//! - **`CostBreaker`** — durable fleet-wide spend circuit-breaker.
//! - **`MaterializedView`** — in-memory cache rebuilt from oplog heads.
//!
//! Phase 3 scaffold: boots, logs a startup message, exits cleanly.

fn main() {
    eprintln!(
        "cp-orchestrator v{} (protocol v{})",
        env!("CARGO_PKG_VERSION"),
        cp_wire::PROTOCOL_VERSION,
    );
    eprintln!("Phase 3 scaffold — no runtime yet.");
}
