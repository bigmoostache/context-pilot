//! §19 observability — per-agent and fleet **metrics** endpoints.
//!
//! The design doc's observability clause (§19) requires the backend to surface,
//! per agent: durable cost-breaker state, stream health (subscriber count +
//! dropped/coalesced frames + degraded flag), and the view-vs-oplog rev lag —
//! so an operator (and the cockpit) can *see* a tripped breaker or a lagging
//! projection instead of inferring it from behaviour. This module exposes that
//! slice from state the backend already collects:
//!
//! * **breaker** — [`CostBreaker`](crate::services::CostBreaker) high-water
//!   spend, budget, and trip verdict (the durable R2-8 latch made visible: a
//!   tripped breaker is reported here, not left a silent backend state).
//! * **stream** — [`StreamHub`](crate::services::StreamHub) aggregate health:
//!   live subscriber count, total dropped frames, any-degraded.
//! * **rev** — the [`MaterializedView`](crate::services::MaterializedView)'s
//!   folded `rev` against the agent oplog's head `rev` (read fresh), and their
//!   lag; under the live tail the lag is ~0, so a persistent non-zero lag is
//!   the signal the projection is falling behind.
//! * **lifecycle / phase** — the agent's last folded lifecycle + phase, so the
//!   metrics view doubles as a liveness glance.
//!
//! Latency histograms (stream p50/p99, fsync latency) and the command-lifecycle
//! histogram are deliberately **not** synthesised here: they require new
//! timestamped instrumentation on the agent and backend hot paths, tracked as a
//! follow-up. This endpoint reports only figures the backend genuinely holds —
//! no fabricated metrics.
//!
//! * [`agent_metrics`] — one agent (`GET /api/agent/{id}/metrics`).
//! * [`fleet_metrics`] — every known agent (`GET /api/metrics`).

use std::path::Path;
use std::sync::Mutex;

use cp_wire::types::registry::Entry;

use crate::transport::Backend;
use crate::transport::rest::HttpReply;

/// `GET /api/agent/{id}/metrics` — the §19 observability snapshot for one agent.
pub fn agent_metrics(state: &Mutex<Backend>, agent_id: &str) -> HttpReply {
    let entry = match crate::transport::rest::resolve_entry(state, agent_id) {
        Ok(e) => e,
        Err(reply) => return reply,
    };
    HttpReply::ok(&build_metrics(state, agent_id, &entry))
}

/// `GET /api/metrics` — the §19 snapshot for every agent in the registry.
///
/// When auth is enabled the list is filtered to the caller's ACL-granted agents
/// (FR-12), mirroring [`fleet_meta`](super::meta::fleet_meta): a regular user
/// must not be able to enumerate every agent's id, cost, and budget via the
/// Usage page. System admins see all; auth-disabled passes everything.
pub fn fleet_metrics(state: &Mutex<Backend>, auth_user: Option<&crate::services::auth::types::User>) -> HttpReply {
    let dir = {
        let Ok(b) = state.lock() else {
            return HttpReply::error(500, "backend lock poisoned");
        };
        b.agents_dir.clone()
    };
    let entries = match list_entries(&dir) {
        Ok(e) => e,
        Err(_) => return HttpReply::ok(&serde_json::json!([])),
    };
    let ids: Vec<String> = entries.iter().map(|e| e.id.clone()).collect();
    let visible = crate::transport::auth::filter_fleet(state, &ids, auth_user);
    let visible: std::collections::HashSet<&str> = visible.iter().map(String::as_str).collect();
    let metrics: Vec<serde_json::Value> =
        entries.iter().filter(|e| visible.contains(e.id.as_str())).map(|e| build_metrics(state, &e.id, e)).collect();
    HttpReply::ok(&metrics)
}

// ── Builder ────────────────────────────────────────────────────────────

/// Assemble one agent's metrics object from the breaker, hub, and view.
///
/// Holds the backend lock once for every in-memory read, then computes the
/// oplog head `rev` from disk *outside* the lock (a metrics call must never
/// block a command path on I/O).
fn build_metrics(state: &Mutex<Backend>, agent_id: &str, entry: &Entry) -> serde_json::Value {
    let snapshot = state.lock().ok().map(|b| {
        let tripped = b.breaker.is_tripped(agent_id);
        let spend = b.breaker.spend_of(agent_id).unwrap_or(0.0);
        let budget = b.breaker.budget();
        let (subs, dropped, degraded) = b.hub.agent_stream_health(agent_id);
        let (view_rev, phase, lifecycle, in_tok, out_tok) = b.view.get(agent_id).map_or((0, None, None, 0, 0), |v| {
            (v.rev, v.phase, v.lifecycle, v.cost.input_tokens, v.cost.output_tokens)
        });
        (tripped, spend, budget, subs, dropped, degraded, view_rev, phase, lifecycle, in_tok, out_tok)
    });

    let Some((tripped, spend, budget, subs, dropped, degraded, view_rev, phase, lifecycle, in_tok, out_tok)) = snapshot
    else {
        return serde_json::json!({ "id": agent_id, "error": "backend lock poisoned" });
    };

    // Oplog head rev, read fresh from disk (outside the lock). A view that
    // trails the oplog head is the rev-lag signal.
    let oplog_head = oplog_head_rev(&entry.oplog_path);
    let rev_lag = oplog_head.map_or(0, |head| head.saturating_sub(view_rev));

    serde_json::json!({
        "id": agent_id,
        "breaker": {
            "tripped": tripped,
            "spendUsd": spend,
            "budgetUsd": budget,
        },
        "stream": {
            "subscribers": subs,
            "droppedFrames": dropped,
            "degraded": degraded,
        },
        "rev": {
            "view": view_rev,
            "oplogHead": oplog_head,
            "lag": rev_lag,
        },
        "tokens": {
            "input": in_tok,
            "output": out_tok,
        },
        "phase": phase,
        "lifecycle": lifecycle,
    })
}

/// Read an agent oplog's head `rev` by replaying its directory.
///
/// Returns `None` when the oplog can't be read (absent/torn) — reported as a
/// `null` head, which the cockpit renders as "unknown" rather than a false `0`.
/// Replay uses the bounded checkpoint fast-path, so this is a cheap read even
/// for a long-lived log.
fn oplog_head_rev(oplog_path: &str) -> Option<u64> {
    cp_oplog::replay::replay(Path::new(oplog_path)).ok().and_then(|r| r.rev_head)
}

/// List all registry entries in a directory (same scan as the meta endpoint).
fn list_entries(dir: &Path) -> std::io::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for item in std::fs::read_dir(dir)? {
        let item = item?;
        let path = item.path();
        let name = item.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.ends_with(".json") || name.ends_with(".tmp") {
            continue;
        }
        if let Ok(bytes) = std::fs::read(&path)
            && let Ok(record) = serde_json::from_slice::<Entry>(&bytes)
        {
            entries.push(record);
        }
    }
    Ok(entries)
}
