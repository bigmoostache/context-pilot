//! On-demand service-connectivity **vitals** endpoint (`GET
//! /api/agent/{id}/vitals`).
//!
//! Where [`metrics`](super::metrics) reports the *durable* observability slice
//! the backend already holds (breaker, stream health, rev lag), this endpoint
//! answers a different, *live* question the operator asks by pressing a button:
//! "can each service this agent depends on actually be reached right now?"
//!
//! It runs **real probes** and reports each as `ok` / `error` / `unavailable`,
//! and — crucially — never fabricates a green. A check the backend genuinely
//! cannot perform from where it sits returns `unavailable` with an honest
//! reason, mirroring the inspection plane's derived-state contract.
//!
//! # What is probed, and how honestly
//!
//! | Service | Probe | Plane it proves |
//! |---|---|---|
//! | Orchestrator | self (we are responding) | always `ok` |
//! | Agent main loop (connection) | heartbeat file freshness + `boot_id` match | the agent process is alive |
//! | Agent loop status | folded phase + lifecycle from the view | a *status* read, not a connection |
//! | LLM provider | TCP connect to the provider host `:443` | network path to the picked provider |
//! | Voyage / Datalab / Brave / Firecrawl | TCP connect to the API host `:443` | network path to the capability |
//! | Meilisearch | TCP connect to `127.0.0.1:<port-file>` | the local search daemon is up |
//! | Console server | `connect()` the agent's `console/server.sock` | the per-agent console daemon is up |
//!
//! A TCP connect to `:443` is a deliberate, key-independent reachability check:
//! it proves DNS + routing + the remote listener without spending a token or
//! needing the agent's API key (which lives in the agent's environment, not the
//! orchestrator's). It answers exactly the "connection to …" question asked.
//!
//! The frontend adds the two checks only *it* can observe (the dev server is up
//! because it is running, and the round-trip latency of this very request), so
//! the rendered table covers all twelve services.

use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use cp_wire::heartbeat::{DEFAULT_MAX_AGE, Heartbeat};
use cp_wire::types::registry::Entry;

use crate::transport::Backend;
use crate::transport::rest::HttpReply;

/// Per-probe network timeout. Bounds DNS+connect for a single remote check.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// Overall deadline for collecting every concurrent network probe. A probe that
/// has not reported by this point (e.g. a stalled DNS resolver) is filled in as
/// a timeout rather than hanging the endpoint.
const COLLECT_DEADLINE: Duration = Duration::from_secs(6);

/// `GET /api/agent/{id}/vitals` — run live connectivity checks for one agent.
///
/// Returns a JSON array of vital objects (`{name, category, status, latencyMs?,
/// detail?}`). Unknown agent → `404`.
pub fn agent_vitals(state: &Mutex<Backend>, agent_id: &str) -> HttpReply {
    let entry = match crate::transport::rest::resolve_entry(state, agent_id) {
        Ok(e) => e,
        Err(reply) => return reply,
    };

    let mut vitals: Vec<serde_json::Value> = Vec::new();

    // 1. Orchestrator — we are responding, so it is trivially up.
    vitals.push(vital("Orchestrator", "orchestrator", "ok", Some(0), "serving"));

    // 2-3. Agent-side reads (heartbeat + folded status), under one short lock.
    let provider = read_provider(state, &entry);
    vitals.push(agent_connection_vital(&entry));
    vitals.push(agent_status_vital(state, agent_id));

    // 4. Local infra probes that need a discovered address.
    vitals.push(meilisearch_vital());
    vitals.push(console_vital(&entry.folder));

    // 5. Remote host reachability — run concurrently, collected under a deadline.
    let (provider_label, provider_host) = provider_host(&provider);
    let remotes: Vec<(String, String, String)> = vec![
        (provider_label.to_owned(), "llm".to_owned(), provider_host.to_owned()),
        ("Voyage".to_owned(), "service".to_owned(), "api.voyageai.com".to_owned()),
        ("Datalab".to_owned(), "service".to_owned(), "www.datalab.to".to_owned()),
        ("Brave".to_owned(), "service".to_owned(), "api.search.brave.com".to_owned()),
        ("Firecrawl".to_owned(), "service".to_owned(), "api.firecrawl.dev".to_owned()),
    ];
    vitals.extend(probe_remotes(remotes));

    HttpReply::ok(&vitals)
}

// ── Agent-side checks ───────────────────────────────────────────────────

/// Read the agent's configured LLM provider id from its `config.json`
/// (`modules.core.llm_provider`), defaulting to `anthropic` when unreadable.
fn read_provider(state: &Mutex<Backend>, entry: &Entry) -> String {
    let folder = PathBuf::from(&entry.folder);
    let Ok(mut b) = state.lock() else {
        return "anthropic".to_owned();
    };
    b.inspect_mut()
        .read_config(&folder)
        .ok()
        .and_then(|cfg| {
            cfg.get("modules")
                .and_then(|m| m.get("core"))
                .and_then(|c| c.get("llm_provider"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "anthropic".to_owned())
}

/// Probe the agent main-loop *connection* via its heartbeat file: a fresh beat
/// whose `boot_id` matches the registry record means the process is alive.
fn agent_connection_vital(entry: &Entry) -> serde_json::Value {
    let Ok(bytes) = std::fs::read(&entry.heartbeat_path) else {
        return vital("Agent main loop", "agent", "unavailable", None, "no heartbeat file");
    };
    let Ok(hb) = Heartbeat::decode(&bytes) else {
        return vital("Agent main loop", "agent", "error", None, "corrupt heartbeat");
    };
    let now_ms = now_ms();
    let max_age_ms = u64::try_from(DEFAULT_MAX_AGE.as_millis()).unwrap_or(5_000);
    let age_ms = now_ms.saturating_sub(hb.timestamp_ms);
    if !hb.matches_boot(&entry.boot_id) {
        return vital("Agent main loop", "agent", "error", None, "boot_id mismatch (pid reused)");
    }
    if hb.is_fresh(now_ms, max_age_ms) {
        vital("Agent main loop", "agent", "ok", None, &format!("heartbeat {age_ms}ms old"))
    } else {
        vital("Agent main loop", "agent", "error", None, &format!("stale heartbeat ({age_ms}ms old)"))
    }
}

/// Report the agent loop's *status* (phase + lifecycle from the folded view).
///
/// This is a status read, not a connection: `ok` carries the live phase as its
/// detail; a `Stopped`/`Stopping` lifecycle surfaces as `error`.
fn agent_status_vital(state: &Mutex<Backend>, agent_id: &str) -> serde_json::Value {
    let snapshot = state.lock().ok().and_then(|b| b.view.get(agent_id).map(|v| (v.phase, v.lifecycle)));
    let Some((phase, lifecycle)) = snapshot else {
        return vital("Agent loop status", "agent", "unavailable", None, "no view yet");
    };
    let phase_str = phase.map_or("idle", phase_label);
    let life_str = lifecycle.map(lifecycle_label);
    let stopped = matches!(life_str, Some("stopping" | "stopped"));
    let detail = match life_str {
        Some(l) => format!("{phase_str} · {l}"),
        None => phase_str.to_owned(),
    };
    let status = if stopped { "error" } else { "ok" };
    vital("Agent loop status", "agent", status, None, &detail)
}

// ── Local infra checks ──────────────────────────────────────────────────

/// Probe Meilisearch by TCP-connecting to the port it advertises in
/// `~/.context-pilot/meilisearch/port`. No port file → it was never started.
fn meilisearch_vital() -> serde_json::Value {
    let Some(port) = meili_port() else {
        return vital("Meilisearch", "infra", "unavailable", None, "no port file (not started)");
    };
    let started = Instant::now();
    match TcpStream::connect_timeout(&(std::net::Ipv4Addr::LOCALHOST, port).into(), PROBE_TIMEOUT) {
        Ok(_) => vital("Meilisearch", "infra", "ok", Some(elapsed_ms(started)), &format!("127.0.0.1:{port}")),
        Err(e) => vital("Meilisearch", "infra", "error", None, &format!("connect failed: {e}")),
    }
}

/// Read the Meilisearch port from `~/.context-pilot/meilisearch/port`.
fn meili_port() -> Option<u16> {
    let path = home_dir()?.join(".context-pilot/meilisearch/port");
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Probe the agent's console server by connecting its Unix socket at
/// `<folder>/.context-pilot/console/server.sock`.
fn console_vital(folder: &str) -> serde_json::Value {
    let sock = Path::new(folder).join(".context-pilot/console/server.sock");
    if !sock.exists() {
        return vital("Console server", "infra", "unavailable", None, "no socket (not started)");
    }
    let started = Instant::now();
    match std::os::unix::net::UnixStream::connect(&sock) {
        Ok(_) => vital("Console server", "infra", "ok", Some(elapsed_ms(started)), "socket connected"),
        Err(e) => vital("Console server", "infra", "error", None, &format!("connect failed: {e}")),
    }
}

// ── Remote reachability probes ──────────────────────────────────────────

/// Run every remote host probe concurrently and collect them under a deadline.
///
/// Each probe runs on its own thread (so the wall time is the slowest single
/// probe, not their sum) and reports over a channel. Any probe that misses the
/// [`COLLECT_DEADLINE`] — e.g. a wedged DNS resolver — is filled in as a
/// timeout so the endpoint always returns promptly.
fn probe_remotes(remotes: Vec<(String, String, String)>) -> Vec<serde_json::Value> {
    let total = remotes.len();
    let (tx, rx) = mpsc::channel::<(usize, serde_json::Value)>();
    for (idx, (label, category, host)) in remotes.into_iter().enumerate() {
        let tx = tx.clone();
        let _handle = thread::spawn(move || {
            let v = tcp_check(&label, &category, &host, 443);
            let _sent = tx.send((idx, v));
        });
    }
    drop(tx); // so the channel closes once every probe thread has reported

    let mut slots: Vec<Option<serde_json::Value>> = (0..total).map(|_| None).collect();
    let deadline = Instant::now() + COLLECT_DEADLINE;
    let mut received = 0;
    while received < total {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match rx.recv_timeout(remaining) {
            Ok((idx, v)) => {
                if let Some(slot) = slots.get_mut(idx) {
                    *slot = Some(v);
                    received += 1;
                }
            }
            Err(_) => break, // deadline elapsed or all senders dropped
        }
    }

    slots
        .into_iter()
        .map(|s| s.unwrap_or_else(|| vital("Service", "service", "unavailable", None, "probe timed out")))
        .collect()
}

/// TCP-connect `host:port` with a bounded timeout — a key-independent
/// reachability check (proves DNS + routing + a live listener).
fn tcp_check(label: &str, category: &str, host: &str, port: u16) -> serde_json::Value {
    let started = Instant::now();
    let addrs = match (host, port).to_socket_addrs() {
        Ok(it) => it,
        Err(e) => return vital(label, category, "error", None, &format!("dns failed: {e}")),
    };
    let Some(addr) = addrs.into_iter().next() else {
        return vital(label, category, "error", None, "dns resolved to no address");
    };
    match TcpStream::connect_timeout(&addr, PROBE_TIMEOUT) {
        Ok(_) => vital(label, category, "ok", Some(elapsed_ms(started)), &format!("{host}:{port} reachable")),
        Err(e) => vital(label, category, "error", None, &format!("{host}:{port} {e}")),
    }
}

// ── Helpers ───────────────────────────────────────────────────────────

/// Map a provider id (`modules.core.llm_provider`) to a display label + the API
/// host whose `:443` reachability stands in for "can we reach the provider".
fn provider_host(provider: &str) -> (&'static str, &'static str) {
    match provider {
        "grok" => ("xAI Grok", "api.x.ai"),
        "groq" => ("Groq", "api.groq.com"),
        "deepseek" => ("DeepSeek", "api.deepseek.com"),
        "minimax" => ("MiniMax", "api.minimax.io"),
        // anthropic + all claude-code variants hit the Anthropic API host.
        _ => ("Anthropic", "api.anthropic.com"),
    }
}

/// Wire label for a folded [`Phase`](cp_wire::types::Phase).
fn phase_label(phase: cp_wire::types::Phase) -> &'static str {
    match phase {
        cp_wire::types::Phase::Idle => "idle",
        cp_wire::types::Phase::Streaming => "streaming",
        cp_wire::types::Phase::Tooling => "tooling",
    }
}

/// Wire label for a folded [`LifecycleState`](cp_wire::types::LifecycleState).
fn lifecycle_label(state: cp_wire::types::LifecycleState) -> &'static str {
    match state {
        cp_wire::types::LifecycleState::Starting => "starting",
        cp_wire::types::LifecycleState::Running => "running",
        cp_wire::types::LifecycleState::Stopping => "stopping",
        cp_wire::types::LifecycleState::Stopped => "stopped",
    }
}

/// Build one vital JSON object.
fn vital(name: &str, category: &str, status: &str, latency_ms: Option<u64>, detail: &str) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "category": category,
        "status": status,
        "latencyMs": latency_ms,
        "detail": detail,
    })
}

/// Milliseconds elapsed since `started`, saturating into `u64`.
fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Current wall-clock time in epoch milliseconds.
fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).ok().and_then(|d| u64::try_from(d.as_millis()).ok()).unwrap_or(0)
}

/// The user's home directory (`$HOME`), if set.
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}
