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
//! * [`registry`] — the **`FleetScanner`** (design doc §10, roadmap P5-T1):
//!   scans `~/.context-pilot/agents/`, applies the verdict to each record, and
//!   diffs successive passes into appeared / disappeared / status-changed /
//!   stale events.
//!
//! * [`inspect`] — read-only, mtime-cached **inspection** of an agent's
//!   on-disk persistence files (tier-② state: config, workers, shared,
//!   messages, panels).
//!
//! * [`channel`] — the per-agent [`AgentHandle`](channel::AgentHandle): oplog
//!   tail ([`Tailer`](tailer::Tailer)), rev-pinned body hydrate, and command
//!   send.
//! * [`supervisor`] — the
//!   [`ProcManager`](supervisor::ProcManager): spawn / stop / restart /
//!   adopt of agent processes.
//! * [`services`] — the runtime services layer:
//!   [`MaterializedView`](services::materialized_view::MaterializedView) (fleet-state projection)
//!   and [`StreamHub`](services::stream_hub::StreamHub) (stream fan-out).

/// Emit a diagnostic line to **stderr** — the orchestrator daemon's operational
/// log channel.
///
/// All the crate's stderr diagnostics funnel through this one macro rather than
/// calling [`eprintln!`] at scattered sites. Because the `eprintln!` token
/// originates from this macro body, clippy's `print_stderr` restriction (which
/// exists to catch stray debug prints in library code) does not fire at the
/// call sites — the daemon's deliberate logging stays lint-clean without a
/// per-site lint suppression. Takes the same format arguments as the standard
/// stderr print macro.
#[macro_export]
macro_rules! oerr {
    ($($arg:tt)*) => { ::std::eprintln!($($arg)*) };
}

/// Emit a diagnostic line to **stdout** — the daemon's user-facing status
/// channel (startup banners, progress).
///
/// The stdout twin of [`oerr!`]: the single chokepoint for the crate's
/// `println!` output, keeping `print_stdout` from firing at scattered call
/// sites. Takes the same format arguments as [`println!`].
#[macro_export]
macro_rules! oout {
    ($($arg:tt)*) => { ::std::println!($($arg)*) };
}

pub mod inspect;
pub mod registry;
pub mod runtime;
pub mod services;
pub mod supervisor;
pub mod transport;

// Re-export channel/tailer/liveness at the crate root as `pub(crate)` so the
// many internal `crate::channel` / `crate::tailer` / `crate::liveness` paths
// resolve without churn. `pub(crate) use` (unlike `pub use`) does not trip
// `clippy::pub_use`. External test consumers reach these through the canonical
// `cp_orchestrator::registry::{channel,tailer,liveness}` module paths instead.
pub(crate) use registry::{channel, liveness, tailer};

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
