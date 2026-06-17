//! Phase 25 — `cp-mod-bridge` boot + heartbeat coverage at the public-API
//! boundary, viewed **as the backend will see it**.
//!
//! The inline tests in `boot.rs`, `heartbeat.rs`, `register/identity.rs`, and
//! `register/registry.rs` each prove their component in isolation. This suite
//! proves the *discovery contract* those components jointly produce — the
//! artifacts the backend's registry + liveness code (Phase 15) consumes —
//! reading them back through [`cp_wire`] exactly as the backend does:
//!
//! * **A booted agent is fully discoverable.** Its `0600` registry record
//!   round-trips to a valid [`Entry`], every resource path it advertises exists
//!   on disk, and its heartbeat file holds a fresh beat whose `boot_id` matches
//!   the record — the three artifacts agree.
//! * **A fleet boots independently.** Two different folders booting into one
//!   agents directory get distinct ids, `boot_id`s, and `cap_token`s, and
//!   coexist — the folder lock is per-folder, not global.
//! * **A restart is a new identity.** After a boot drops, re-booting the same
//!   folder keeps the deterministic id but mints a fresh `boot_id` and
//!   `cap_token`, so liveness treats the restarted process as new (the
//!   pid-reuse / restart-identity property at the boot layer).
//! * **A beacon-produced beat drives the liveness verdict.** A real beat reads
//!   fresh now, stale against a tiny max-age in the future, and matches only
//!   its own `boot_id`.
//!
//! [`Entry`]: cp_wire::types::registry::Entry

// The bridge's regular dependencies are linked into this integration-test
// target; name the ones we don't reference directly to satisfy the per-target
// `unused-crate-dependencies` lint.
use cp_base as _;
use cp_oplog as _;
use cp_render as _;
use log as _;
use nix as _;

use std::fs;
use std::path::Path;
use std::time::SystemTime;

use cp_mod_bridge::boot::Boot;
use cp_wire::heartbeat::{DEFAULT_MAX_AGE, HEARTBEAT_LEN, Heartbeat};
use cp_wire::types::registry::{AgentStatus, Entry};
use cp_wire::PROTOCOL_VERSION;
use tempfile::TempDir;

// ── helpers ────────────────────────────────────────────────────────────────

/// Boot an agent in a fresh folder, writing its record into `agents`.
fn boot_in(folder: &Path, agents: &Path) -> Boot {
    Boot::start_in(folder, agents, "test-model").expect("boot")
}

/// Read agent `id`'s registry record back as the backend would.
fn read_entry(agents: &Path, id: &str) -> Entry {
    let path = agents.join(format!("{id}.json"));
    let text = fs::read_to_string(&path).expect("read registry record");
    serde_json::from_str(&text).expect("registry record parses as a wire Entry")
}

/// Decode the heartbeat file an agent advertised.
fn read_beat(heartbeat_path: &str) -> Heartbeat {
    let bytes = fs::read(heartbeat_path).expect("read heartbeat file");
    assert_eq!(bytes.len(), HEARTBEAT_LEN, "heartbeat file is exactly one fixed-size record");
    Heartbeat::decode(&bytes).expect("heartbeat decodes")
}

/// Wall-clock milliseconds since the Unix epoch.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// [`DEFAULT_MAX_AGE`] expressed in milliseconds for [`Heartbeat::is_fresh`].
fn default_max_age_ms() -> u64 {
    u64::try_from(DEFAULT_MAX_AGE.as_millis()).unwrap_or(u64::MAX)
}

// ── discovery contract ──────────────────────────────────────────────────────

#[test]
fn a_booted_agent_is_fully_discoverable() {
    let folder = TempDir::new().expect("folder");
    let agents = TempDir::new().expect("agents");
    let booted = boot_in(folder.path(), agents.path());

    // The registry record the backend reads is a valid, complete Entry.
    let entry = read_entry(agents.path(), booted.id());
    assert_eq!(entry.id, booted.id());
    assert_eq!(entry.protocol_version, PROTOCOL_VERSION);
    assert_eq!(entry.status, AgentStatus::Starting);
    assert_eq!(entry.cap_token.len(), 64, "256-bit cap_token");
    assert_eq!(entry.boot_id.len(), 32, "128-bit boot_id");

    // Every resource path it advertises actually exists.
    assert!(Path::new(&entry.oplog_path).exists(), "advertised oplog dir exists");
    assert!(Path::new(&entry.socket_path).exists(), "advertised socket exists");
    assert!(Path::new(&entry.heartbeat_path).exists(), "advertised heartbeat file exists");

    // The heartbeat agrees with the record: a fresh beat from the same boot.
    let beat = read_beat(&entry.heartbeat_path);
    assert!(beat.matches_boot(&entry.boot_id), "the beat carries the record's boot_id");
    assert_eq!(beat.pid, entry.pid, "the beat carries the record's pid");
    assert!(beat.is_fresh(now_ms(), default_max_age_ms()), "the first beat is fresh on boot");
}

// ── fleet independence ──────────────────────────────────────────────────────

#[test]
fn two_folders_boot_independently_into_one_fleet() {
    let folder_a = TempDir::new().expect("folder a");
    let folder_b = TempDir::new().expect("folder b");
    let agents = TempDir::new().expect("agents");

    // Both agents are alive simultaneously — the lock is per-folder.
    let a = boot_in(folder_a.path(), agents.path());
    let b = boot_in(folder_b.path(), agents.path());

    assert_ne!(a.id(), b.id(), "different folders yield different ids");
    assert_ne!(a.cap_token(), b.cap_token(), "each agent has its own bearer secret");
    assert_ne!(a.entry().boot_id, b.entry().boot_id, "each boot is a distinct identity");

    // Both records coexist in the fleet directory and round-trip independently.
    let entry_a = read_entry(agents.path(), a.id());
    let entry_b = read_entry(agents.path(), b.id());
    assert_eq!(entry_a.id, a.id());
    assert_eq!(entry_b.id, b.id());

    // Both heartbeats are fresh and bound to their own boot.
    assert!(read_beat(&entry_a.heartbeat_path).matches_boot(&entry_a.boot_id));
    assert!(read_beat(&entry_b.heartbeat_path).matches_boot(&entry_b.boot_id));
}

// ── restart identity ────────────────────────────────────────────────────────

#[test]
fn rebooting_a_folder_keeps_its_id_but_mints_a_fresh_identity() {
    let folder = TempDir::new().expect("folder");
    let agents = TempDir::new().expect("agents");

    let (id, first_boot_id, first_token) = {
        let first = boot_in(folder.path(), agents.path());
        (first.id().to_owned(), first.entry().boot_id.clone(), first.cap_token().to_owned())
    }; // first drops here → lock released, registry + socket removed.

    // The deterministic id survives a restart…
    let second = boot_in(folder.path(), agents.path());
    assert_eq!(second.id(), id, "folder_id is deterministic across restarts");

    // …but the secrets do not: a restart is a new identity to liveness, so a
    // recycled pid carrying the *old* boot_id cannot masquerade as still-alive.
    assert_ne!(second.entry().boot_id, first_boot_id, "a restart mints a fresh boot_id");
    assert_ne!(second.cap_token(), first_token, "a restart mints a fresh cap_token");

    // Exactly the new record is on disk, and its beat matches the new boot.
    let entry = read_entry(agents.path(), second.id());
    assert_eq!(entry.boot_id, second.entry().boot_id);
    assert!(read_beat(&entry.heartbeat_path).matches_boot(&entry.boot_id));
}

// ── beacon-driven liveness verdict ──────────────────────────────────────────

#[test]
fn a_real_beat_drives_the_freshness_and_boot_match_verdict() {
    let folder = TempDir::new().expect("folder");
    let agents = TempDir::new().expect("agents");
    let booted = boot_in(folder.path(), agents.path());
    let entry = read_entry(agents.path(), booted.id());
    let beat = read_beat(&entry.heartbeat_path);

    // Fresh against the real max-age now.
    assert!(beat.is_fresh(now_ms(), default_max_age_ms()), "a just-written beat is fresh");

    // Stale once the clock has advanced far past a tiny max-age: the verdict is
    // age-driven, exactly how the backend ages out a silent agent.
    let distant_future = beat.timestamp_ms.saturating_add(60_000);
    assert!(!beat.is_fresh(distant_future, 10), "a beat 60s old fails a 10ms max-age");

    // Bound to its own boot only — the pid-reuse defence.
    assert!(beat.matches_boot(&entry.boot_id), "matches its own boot_id");
    assert!(
        !beat.matches_boot("ffffffffffffffffffffffffffffffff"),
        "rejects a different boot_id",
    );
}
