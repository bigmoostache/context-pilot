//! The per-agent **liveness verdict** — the pure decision at the heart of fleet
//! discovery (design doc §10).
//!
//! The backend never trusts a registry record's mere existence: a record can
//! outlive its agent when a crash skips the `Drop` that removes it. [`verdict`]
//! instead combines three independent signals, **all** of which must agree
//! before an agent counts as alive:
//!
//! 1. **the advertised pid is a live process** — else the agent has exited;
//! 2. **its heartbeat is fresh** — a beat older than the freshness window means
//!    the writer thread stopped; and
//! 3. **the heartbeat's `boot_id` matches the record's** — the **pid-reuse
//!    defence**: after an agent dies the OS may recycle its pid for an unrelated
//!    process (signal-0 then reports it "alive"), but that process cannot
//!    reproduce the dead agent's random `boot_id`, and a superseding agent in
//!    the same folder beats with a *new* `boot_id`. A `boot_id` mismatch marks
//!    the record stale even though its pid is live.
//!
//! [`verdict`] is pure — pid-liveness is resolved internally but the heartbeat
//! and clock are passed in — so the full verdict matrix is exhaustively
//! unit-testable without spawning processes or sleeping.

use std::time::Duration;

use nix::errno::Errno;
use nix::sys::signal::kill;
use nix::unistd::Pid;

use cp_wire::heartbeat::Heartbeat;
use cp_wire::types::registry::Entry;

/// The backend's verdict on whether a discovered agent is actually alive.
///
/// Only [`Live`](Liveness::Live) means "observe and command this agent"; every
/// other variant names *why* a record is not trustworthy, which the backend
/// surfaces for diagnostics (a recycled pid reads very differently from a hung
/// writer).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Liveness {
    /// All three signals agree: live pid, fresh heartbeat, matching `boot_id`.
    Live,

    /// The advertised pid is not a running process — the agent has exited.
    StalePid,

    /// No readable heartbeat, or one older than the freshness window — the
    /// writer thread is gone or wedged.
    StaleHeartbeat,

    /// The pid is live and beating, but with a different `boot_id` than the
    /// record claims — a recycled pid or a superseded record (pid-reuse
    /// defence), so this record is not our agent.
    BootIdMismatch,
}

impl Liveness {
    /// Whether the verdict is [`Live`](Liveness::Live).
    #[must_use]
    pub const fn is_live(self) -> bool {
        matches!(self, Self::Live)
    }
}

/// Derive a [`Liveness`] verdict from a record and its (optional) heartbeat.
///
/// The checks run cheapest-and-most-definitive first: a dead pid ends it, then
/// a missing/foreign beat, then a `boot_id` mismatch, then freshness. The
/// `now_ms` / `max_age` pair bounds heartbeat freshness ([`Heartbeat::is_fresh`]).
#[must_use]
pub fn verdict(entry: &Entry, heartbeat: Option<&Heartbeat>, now_ms: u64, max_age: Duration) -> Liveness {
    if !pid_alive(entry.pid) {
        return Liveness::StalePid;
    }
    let Some(hb) = heartbeat else {
        return Liveness::StaleHeartbeat;
    };
    if !hb.matches_boot(&entry.boot_id) {
        return Liveness::BootIdMismatch;
    }
    let max_age_ms = u64::try_from(max_age.as_millis()).unwrap_or(u64::MAX);
    if hb.is_fresh(now_ms, max_age_ms) { Liveness::Live } else { Liveness::StaleHeartbeat }
}

/// Whether `pid` names a live process.
///
/// Uses signal-0 (`kill(pid, None)`): it performs the kernel's permission and
/// existence checks without delivering a signal. `EPERM` means the process
/// exists but is not ours to signal — still **alive**; `ESRCH` (and any other
/// error, including an out-of-range pid) means **not alive**.
fn pid_alive(pid: u32) -> bool {
    let Ok(raw) = i32::try_from(pid) else {
        return false;
    };
    match kill(Pid::from_raw(raw), None) {
        Ok(()) | Err(Errno::EPERM) => true,
        Err(_other) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cp_wire::heartbeat::DEFAULT_MAX_AGE;
    use cp_wire::types::registry::AgentStatus;
    use std::path::Path;

    /// Boot ids of the exact 32-hex-char width the heartbeat record requires.
    const BOOT_A: &str = "0123456789abcdef0123456789abcdef";
    const BOOT_B: &str = "ffffffffffffffffffffffffffffffff";

    /// A pid that cannot name a live process (above any platform's pid_max).
    const DEAD_PID: u32 = 4_000_000_000;

    fn entry(pid: u32, boot_id: &str) -> Entry {
        Entry {
            schema_version: 1,
            id: "a".to_owned(),
            folder: "/tmp/agent".to_owned(),
            pid,
            boot_id: boot_id.to_owned(),
            model: "test-model".to_owned(),
            protocol_version: 1,
            binary_version: "0.0.0".to_owned(),
            socket_path: "/tmp/agent/stream.sock".to_owned(),
            oplog_path: "/tmp/agent/oplog".to_owned(),
            heartbeat_path: Path::new("/unused").to_string_lossy().into_owned(),
            cap_token: "tok".to_owned(),
            started_at_ms: 0,
            status: AgentStatus::Running,
        }
    }

    fn heartbeat(pid: u32, boot_id: &str, timestamp_ms: u64) -> Heartbeat {
        Heartbeat::new(timestamp_ms, 0, pid, boot_id.to_owned())
    }

    #[test]
    fn live_when_all_signals_agree() {
        let me = std::process::id();
        let hb = heartbeat(me, BOOT_A, 1_000);
        assert_eq!(verdict(&entry(me, BOOT_A), Some(&hb), 1_000, DEFAULT_MAX_AGE), Liveness::Live);
    }

    #[test]
    fn stale_pid_when_process_dead() {
        let hb = heartbeat(DEAD_PID, BOOT_A, 1_000);
        assert_eq!(verdict(&entry(DEAD_PID, BOOT_A), Some(&hb), 1_000, DEFAULT_MAX_AGE), Liveness::StalePid,);
    }

    #[test]
    fn stale_heartbeat_when_missing() {
        let me = std::process::id();
        assert_eq!(verdict(&entry(me, BOOT_A), None, 1_000, DEFAULT_MAX_AGE), Liveness::StaleHeartbeat,);
    }

    #[test]
    fn stale_heartbeat_when_old() {
        let me = std::process::id();
        let hb = heartbeat(me, BOOT_A, 0);
        // now is far past the beat → outside any sane freshness window.
        assert_eq!(verdict(&entry(me, BOOT_A), Some(&hb), 1_000_000, DEFAULT_MAX_AGE), Liveness::StaleHeartbeat,);
    }

    #[test]
    fn boot_mismatch_defeats_pid_reuse() {
        // A live pid (ours) beating with a different boot than the record →
        // the record is not our agent.
        let me = std::process::id();
        let hb = heartbeat(me, BOOT_B, 1_000);
        assert_eq!(verdict(&entry(me, BOOT_A), Some(&hb), 1_000, DEFAULT_MAX_AGE), Liveness::BootIdMismatch,);
    }
}
