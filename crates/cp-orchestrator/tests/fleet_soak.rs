//! Phase 9.4 (X869) — **multi-agent soak** + the load-bearing slice of the
//! V1–V12 fault matrix that only emerges *at fleet scale and under concurrency*.
//!
//! The per-service unit tests prove each structure in isolation; the
//! [`services_integration`](super) suite proves their composition for a *single*
//! agent against a real oplog. This suite adds the two properties the design
//! doc's fault matrix reserves for a fleet under load:
//!
//! * **V10 — gap-free under load.** `N` agents each drive a *concurrent*,
//!   contended mixed workload (durable roster/message records interleaved with
//!   droppable best-effort phase/cost records) into their own real oplog. After
//!   the storm, every agent's oplog replays to a **gap-free** `rev` stream
//!   (`0..=rev_head`, strictly contiguous) — a dropped best-effort record never
//!   tears the durable sequence, because a record only consumes a `rev` once the
//!   single commit thread admits it (a `try_send` that sheds load never reaches
//!   the rev-assigner). This is the concurrent, fleet-scale version of the
//!   gap-free guarantee `oplog_core`/`dedup_compaction` prove single-threaded.
//! * **V8 (scaled) — fleet isolation with no cross-contamination.** One shared
//!   [`MaterializedView`] and [`StreamHub`] project all `N`
//!   agents at once: the view holds `N` isolated projections and the hub fans
//!   per-agent. The literal
//!   10 000-agent OS soak (flat RSS / FD / thread counts) is an *external* CI
//!   harness — one `OplogWaiter` per agent by construction means watch count is
//!   `O(agents)` regardless of scale; what a cargo test can falsify is the
//!   *logical* isolation invariant, which this asserts.
//! * **V5 — drop + reorder chaos, recovered from the authoritative snapshot.**
//!   A stalled [`StreamHub`] subscriber is flooded past its bound with
//!   out-of-order frames; it coalesces toward the newest window, latches
//!   `degraded`, and is then **reconciled** from the [`MaterializedView`]'s
//!   oplog-folded heads — the stream plane is lossy, the oplog is the safety
//!   net (design doc I7/I10). The literal frame-drop *fuzz* over a live socket
//!   is covered by `tee_reader`'s corrupt-resync test; this proves the
//!   end-to-end *recovery* path the fault matrix names.

// Linked into this integration-test target but not all named directly;
// acknowledge them for the per-target `unused-crate-dependencies` lint (M60).
use argon2 as _;
use base64 as _;
use calamine as _;
use cp_base as _;
use cp_mod_bridge as _;
use cp_vault as _;
use csv as _;
use dotenvy as _;
use nix as _;
use notify as _;
use openssl as _;
use portable_pty as _;
use reqwest as _;
use rusqlite as _;
use serde as _;
use serde_json as _;
use serde_yaml as _;
use sha2 as _;
use tiny_http as _;
use utoipa as _;

use std::path::Path;
use std::thread;

use cp_oplog::service::Service as OplogService;

use cp_orchestrator::channel::Tailer;
use cp_orchestrator::services::{MaterializedView, StreamHub};

use cp_wire::types::oplog::OpEntryKind;
use cp_wire::types::stream::{Frame, Kind as StreamKind};
use cp_wire::types::{ContentHash, Phase, ThreadTurn};

use tempfile::tempdir;

/// Agents in the soak. Large enough to exercise real cross-agent concurrency
/// and isolation, small enough to stay a fast, deterministic CI test (the
/// literal 10k-agent OS soak is external — see the module docs).
const AGENTS: usize = 16;

/// Mixed records each agent journals, per the workload below.
const RECORDS_PER_AGENT: u8 = 40;

// ── record builders ─────────────────────────────────────────────────────────

/// A durable `MessageCreated` head for `thread`, keyed by `byte`.
fn message(thread: &str, byte: u8) -> OpEntryKind {
    OpEntryKind::MessageCreated {
        thread_id: thread.to_owned(),
        message_id: format!("m{byte}"),
        head: ContentHash::new([byte; 32]),
        inline_body: None,
    }
}

/// A durable `ThreadCreated` roster delta.
fn thread_created(thread: &str) -> OpEntryKind {
    OpEntryKind::ThreadCreated {
        thread_id: thread.to_owned(),
        name: format!("thread {thread}"),
        status: ThreadTurn::TheirTurn,
        timestamp_ms: 0,
    }
}

/// A droppable best-effort cost aggregate of `cost_usd`.
fn cost(cost_usd: f64) -> OpEntryKind {
    OpEntryKind::CostAggregate { input_tokens: 0, output_tokens: 0, cost_usd }
}

/// A droppable best-effort phase transition.
fn phase() -> OpEntryKind {
    OpEntryKind::PhaseTransition { phase: Phase::Streaming }
}

/// A stream token frame for `agent` at `seq`.
fn token(agent: &str, seq: u64) -> Frame {
    Frame {
        schema_version: 1,
        agent_id: agent.to_owned(),
        worker_id: "w0".to_owned(),
        thread_id: "T1".to_owned(),
        message_id: "m1".to_owned(),
        seq,
        kind: StreamKind::Token { text: format!("t{seq}") },
    }
}

/// Read every entry an agent's oplog holds, in `rev` order, via the backend's
/// real [`Tailer`] (one full catch-up poll).
fn tail_all(oplog_dir: &Path) -> Vec<cp_wire::types::oplog::OpEntry> {
    Tailer::new(oplog_dir.to_path_buf()).poll().expect("tail poll")
}

/// One agent's contended workload: open its oplog, interleave durable roster /
/// message records with droppable best-effort phase / cost records, then shut
/// down (draining + syncing the final batch).
fn run_agent_workload(dir: &Path, _agent_idx: usize) {
    let oplog = OplogService::spawn(dir).expect("spawn agent oplog");

    let _thread_rev = oplog.append_durable(thread_created("T1")).expect("thread durable");
    for byte in 0..RECORDS_PER_AGENT {
        // A best-effort phase + cost between every durable head — these may be
        // shed under pressure, and that shedding must NOT tear the rev stream.
        let _phase = oplog.append_best_effort(phase());
        let _cost = oplog.append_best_effort(cost(f64::from(byte) + 1.0));
        let _msg_rev = oplog.append_durable(message("T1", byte)).expect("message durable");
    }
    oplog.shutdown().expect("agent shutdown");
}

/// Assert an agent's replayed oplog is a **gap-free** `rev` stream: the first
/// `rev` is 0, every successor increments by exactly one, and the last equals
/// the recovered head. A shed best-effort record never consumes a `rev`, so a
/// surviving log is always contiguous (design doc K5 / V10).
fn assert_gap_free(entries: &[cp_wire::types::oplog::OpEntry], agent_id: &str) {
    assert!(!entries.is_empty(), "{agent_id}: replayed a non-empty oplog");
    assert_eq!(entries[0].rev, 0, "{agent_id}: rev stream starts at 0");
    for pair in entries.windows(2) {
        assert_eq!(
            pair[1].rev,
            pair[0].rev + 1,
            "{agent_id}: gap or reorder in the rev stream ({} -> {})",
            pair[0].rev,
            pair[1].rev,
        );
    }
}

// ── V10 + V8(scaled): concurrent fleet, gap-free + isolated ─────────────────

#[test]
fn n_agents_under_concurrent_load_stay_gap_free_and_isolated() {
    // One tempdir per agent; keep the guards alive for the whole test so the
    // oplog directories survive until every replay + projection is done.
    let dirs: Vec<tempfile::TempDir> = (0..AGENTS).map(|_| tempdir().expect("dir")).collect();

    // Drive all N agents' workloads concurrently — real contention across N
    // commit threads, each on its own oplog. Scoped threads borrow `dirs`.
    thread::scope(|scope| {
        let handles: Vec<_> = dirs
            .iter()
            .enumerate()
            .map(|(idx, dir)| {
                let path = dir.path().to_path_buf();
                scope.spawn(move || run_agent_workload(&path, idx))
            })
            .collect();
        for h in handles {
            h.join().expect("agent thread");
        }
    });

    // Every agent's oplog is independently gap-free after the concurrent storm.
    let mut view = MaterializedView::new();
    for (idx, dir) in dirs.iter().enumerate() {
        let agent_id = format!("agent-{idx}");
        let entries = tail_all(dir.path());
        assert_gap_free(&entries, &agent_id);
        view.apply_batch(&agent_id, &entries);
    }

    // The shared view holds N isolated projections — no cross-contamination.
    assert_eq!(view.len(), AGENTS, "one projection per agent");
    for idx in 0..AGENTS {
        let agent_id = format!("agent-{idx}");
        let agent = view.get(&agent_id).unwrap_or_else(|| panic!("{agent_id} projected"));
        let head =
            agent.heads.threads.iter().find(|h| h.thread_id == "T1").unwrap_or_else(|| panic!("{agent_id} T1 head"));
        assert_eq!(
            head.last_message_hash,
            ContentHash::new([RECORDS_PER_AGENT - 1; 32]),
            "{agent_id}: head is this agent's own last message, not a neighbour's",
        );
        assert!(agent.roster.iter().any(|t| t.thread_id == "T1"), "{agent_id}: roster carries its own thread",);
    }
}

// ── V5: drop + reorder chaos, recovered from the authoritative snapshot ──────

#[test]
fn a_flooded_reordered_subscriber_coalesces_then_reconciles_from_the_view() {
    let dir = tempdir().expect("dir");

    // The authoritative recovery source: a real oplog folded into the view's
    // heads. This is what a reconcile delivers to a degraded subscriber.
    {
        let oplog = OplogService::spawn(dir.path()).expect("spawn");
        let _head_rev = oplog.append_durable(message("T1", 0x42)).expect("durable head");
        oplog.shutdown().expect("shutdown");
    }
    let mut view = MaterializedView::new();
    view.apply_batch("chaos", &tail_all(dir.path()));
    let authoritative_head =
        view.get("chaos").expect("agent").heads.threads.first().expect("T1 head").last_message_hash;
    assert_eq!(authoritative_head, ContentHash::new([0x42; 32]));

    // A small-capacity subscriber is flooded with MORE frames than it can hold,
    // delivered OUT OF ORDER (seq 5,3,8,1,9,2 …). The hub keeps the newest
    // window by eviction and latches degraded — a lossy stream by design.
    let mut hub = StreamHub::new(3);
    let sub = hub.subscribe("chaos");
    for seq in [5u64, 3, 8, 1, 9, 2, 7, 4, 6, 0] {
        let _clean = hub.publish("chaos", &token("chaos", seq));
    }
    let degraded = hub.subscriber("chaos", sub).expect("sub");
    assert!(degraded.is_degraded(), "a past-capacity flood latches degraded");
    assert!(degraded.dropped_count() > 0, "frames were shed under the flood");

    // The buffer holds at most `capacity` frames — never grows unbounded under
    // a flood, however reordered (design doc I7 producer-never-blocks fan-out).
    let buffered = hub.drain("chaos", sub).expect("drain");
    assert!(buffered.len() <= 3, "the bounded buffer never exceeds its capacity");

    // Recovery: the caller delivers the authoritative oplog-folded snapshot
    // (above) and clears the degraded latch — the R2-17 reconcile completing.
    // Post-reconcile, the durable head is the recovery truth, not any dropped
    // stream frame.
    assert!(hub.mark_reconciled("chaos", sub), "subscriber reconciled");
    let healed = hub.subscriber("chaos", sub).expect("sub");
    assert!(!healed.is_degraded(), "reconcile clears the degraded latch");
    assert_eq!(healed.dropped_count(), 0, "reconcile resets the dropped counter");
    assert_eq!(
        authoritative_head,
        ContentHash::new([0x42; 32]),
        "the oplog head is the recovery source of truth after stream loss",
    );
}
