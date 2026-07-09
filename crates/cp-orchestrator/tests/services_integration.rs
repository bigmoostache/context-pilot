//! Phase 29 — the backend **services** (`MaterializedView`,
//! `StreamHub`) driven *together* against a **real oplog**, through their
//! public APIs.
//!
//! The inline unit tests in each service module prove every method against
//! crafted [`OpEntry`](cp_wire::types::oplog::OpEntry) literals. This suite
//! proves the parts only composition exercises: the services folded from bytes
//! that actually round-tripped through `cp-oplog`'s append + replay, the
//! restart-rebuild path, and the cross-service wiring (R2-17 reconcile,
//! per-agent isolation).
//!
//! * **Restart rebuild end-to-end (I5).** A real oplog is replayed through
//!   the [`Tailer`] into a fresh [`MaterializedView`] — exactly the
//!   backend-restart path.
//! * **A real segment-roll checkpoint folds to the correct heads (I5).** A roll
//!   stamps a leading authoritative `Checkpoint`; folding the full stream
//!   (checkpoint included) still yields the ground-truth final heads.
//! * **The R2-17 reconcile loop.** A degraded [`StreamHub`] subscriber is
//!   reconciled from the authoritative snapshot the [`MaterializedView`] holds,
//!   folded from the real oplog.
//! * **Fleet isolation across all three services.** Two agents, two oplogs:
//!   the view keeps two isolated projections and the hub fans per-agent.

// Linked into this integration-test target but not named directly; acknowledge
// them for the per-target `unused-crate-dependencies` lint.
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

use cp_oplog::service::Service as OplogService;

use cp_orchestrator::channel::Tailer;
use cp_orchestrator::services::{MaterializedView, StreamHub};

use cp_wire::types::oplog::OpEntryKind;
use cp_wire::types::stream::{Frame, Kind as StreamKind};
use cp_wire::types::{ContentHash, Phase};

use tempfile::tempdir;

/// A `MessageCreated` head for `thread` keyed by `byte`.
fn message(thread: &str, byte: u8) -> OpEntryKind {
    OpEntryKind::MessageCreated {
        thread_id: thread.to_owned(),
        message_id: format!("m{byte}"),
        head: ContentHash::new([byte; 32]),
        inline_body: None,
    }
}

/// A cumulative-since-boot cost aggregate of `cost_usd`.
fn cost(cost_usd: f64) -> OpEntryKind {
    OpEntryKind::CostAggregate { input_tokens: 0, output_tokens: 0, cost_usd }
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
        kind: StreamKind::Token { text: "x".to_owned() },
    }
}

/// Read every entry an agent's oplog holds, in `rev` order, via the backend's
/// real [`Tailer`] (one full catch-up poll).
fn tail_all(oplog_dir: &Path) -> Vec<cp_wire::types::oplog::OpEntry> {
    Tailer::new(oplog_dir.to_path_buf()).poll().expect("tail poll")
}

// ── 1. restart rebuild end-to-end (I5 + V9) ─────────────────────────────────

#[test]
fn a_fresh_view_rebuilds_from_a_real_oplog_after_restart() {
    let dir = tempdir().expect("dir");

    // The agent journals a realistic mix: heads, a phase, rising cost.
    {
        let oplog = OplogService::spawn(dir.path()).expect("spawn");
        let _r0 = oplog.append_durable(message("T1", 0x11)).expect("msg");
        let _r1 = oplog.append_durable(message("T1", 0x12)).expect("msg"); // newer head wins
        let _r2 = oplog.append_durable(message("T2", 0x21)).expect("msg");
        let _r3 = oplog.append_durable(OpEntryKind::PhaseTransition { phase: Phase::Streaming }).expect("phase");
        let _r4 = oplog.append_durable(cost(3.0)).expect("cost");
        let _r5 = oplog.append_durable(cost(9.0)).expect("cost"); // over a 5.0 budget
        oplog.shutdown().expect("shutdown");
    }

    // Backend restart: replay the durable oplog through the Tailer into a fresh
    // view.
    let entries = tail_all(dir.path());
    let mut view = MaterializedView::new();
    view.apply_batch("a1", &entries);

    let agent = view.get("a1").expect("agent projected");
    assert_eq!(agent.phase, Some(Phase::Streaming), "latest phase survives replay");
    let t1 = agent.heads.threads.iter().find(|h| h.thread_id == "T1").expect("T1 head");
    assert_eq!(t1.last_message_hash, ContentHash::new([0x12; 32]), "newest T1 head wins");
    assert!(agent.heads.threads.iter().any(|h| h.thread_id == "T2"), "T2 head present");
    assert!((agent.cost.cost_usd - 9.0).abs() < f64::EPSILON, "latest cumulative cost");
}

// ── 2. a real segment-roll checkpoint folds to the correct heads (I5) ───────

#[test]
fn a_real_rolled_checkpoint_folds_to_ground_truth_heads() {
    let dir = tempdir().expect("dir");

    // A tiny segment limit forces several rolls; each rolled segment opens with
    // a leading authoritative Checkpoint snapshot of the live heads.
    let last_byte: u8 = 12;
    {
        let oplog = OplogService::spawn_with_segment_limit(dir.path(), 64).expect("spawn");
        for byte in 1..=last_byte {
            let _r = oplog.append_durable(message("T1", byte)).expect("msg");
        }
        oplog.shutdown().expect("shutdown");
    }

    let entries = tail_all(dir.path());
    // The test is only meaningful if a real checkpoint rode the stream.
    assert!(
        entries.iter().any(|e| matches!(e.kind, OpEntryKind::Checkpoint { .. })),
        "a segment roll must have stamped at least one checkpoint",
    );

    let mut view = MaterializedView::new();
    view.apply_batch("a1", &entries);

    // Despite the authoritative mid-stream checkpoint resets, the folded head is
    // the last message written — the view layer's I5 reset is correct.
    let agent = view.get("a1").expect("agent");
    let t1 = agent.heads.threads.iter().find(|h| h.thread_id == "T1").expect("T1 head");
    assert_eq!(
        t1.last_message_hash,
        ContentHash::new([last_byte; 32]),
        "folding a real rolled checkpoint stream yields the ground-truth final head",
    );
}

// ── 3. the R2-17 reconcile loop, sourced from a real oplog snapshot ─────────

#[test]
fn a_degraded_subscriber_reconciles_from_the_views_oplog_snapshot() {
    let dir = tempdir().expect("dir");
    {
        let oplog = OplogService::spawn(dir.path()).expect("spawn");
        let _r = oplog.append_durable(message("T1", 0x33)).expect("msg");
        oplog.shutdown().expect("shutdown");
    }

    // The authoritative snapshot a reconcile would deliver: the view's heads,
    // folded from the durable oplog.
    let mut view = MaterializedView::new();
    view.apply_batch("a1", &tail_all(dir.path()));
    let authoritative_head = view.get("a1").expect("agent").heads.threads.first().expect("T1 head").last_message_hash;
    assert_eq!(authoritative_head, ContentHash::new([0x33; 32]));

    // A tiny-capacity subscriber overflows and goes degraded.
    let mut hub = StreamHub::new(1);
    let sub = hub.subscribe("a1");
    let _c0 = hub.publish("a1", &token("a1", 0));
    let _c1 = hub.publish("a1", &token("a1", 1)); // overflow → degraded
    assert!(hub.subscriber("a1", sub).expect("sub").is_degraded(), "overflow degraded it");

    // The caller delivers the authoritative snapshot (above) then clears the
    // flag — the R2-17 reconcile completing.
    assert!(hub.mark_reconciled("a1", sub), "subscriber found");
    let sub_state = hub.subscriber("a1", sub).expect("sub");
    assert!(!sub_state.is_degraded(), "reconcile clears degraded");
    assert_eq!(sub_state.dropped_count(), 0, "reconcile resets the dropped counter");
}

// ── 4. fleet isolation across all three services ────────────────────────────

#[test]
fn two_agents_stay_isolated_across_view_and_hub() {
    let spender = tempdir().expect("spender dir");
    let thrifty = tempdir().expect("thrifty dir");
    {
        let o1 = OplogService::spawn(spender.path()).expect("spawn");
        let _a = o1.append_durable(message("T1", 0x01)).expect("msg");
        let _b = o1.append_durable(cost(20.0)).expect("cost"); // over budget
        o1.shutdown().expect("shutdown");

        let o2 = OplogService::spawn(thrifty.path()).expect("spawn");
        let _c = o2.append_durable(message("T1", 0x02)).expect("msg");
        let _d = o2.append_durable(cost(1.0)).expect("cost"); // under budget
        o2.shutdown().expect("shutdown");
    }

    // One view holds two isolated projections.
    let mut view = MaterializedView::new();
    view.apply_batch("spender", &tail_all(spender.path()));
    view.apply_batch("thrifty", &tail_all(thrifty.path()));
    assert_eq!(view.len(), 2, "two distinct agent projections");

    // The hub fans per-agent: a frame for `spender` never reaches `thrifty`.
    let mut hub = StreamHub::new(8);
    let s_sub = hub.subscribe("spender");
    let t_sub = hub.subscribe("thrifty");
    let clean = hub.publish("spender", &token("spender", 0));
    assert_eq!(clean, 1, "only the spender's single subscriber admits it");
    assert_eq!(hub.drain("spender", s_sub).expect("spender drain").len(), 1);
    assert!(hub.drain("thrifty", t_sub).expect("thrifty drain").is_empty(), "thrifty untouched");
}
