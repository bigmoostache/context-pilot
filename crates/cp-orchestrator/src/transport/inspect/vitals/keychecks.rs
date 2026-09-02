//! Authenticated **API-key validation** probes for the vitals board.
//!
//! Where the host-reachability checks in [`super`] only prove DNS + routing +
//! a live TLS listener, these probes prove the thing an operator actually cares
//! about: *is the configured API key valid, and whose account does it belong
//! to?* Each probe resolves the key from the orchestrator's [`cp_vault`] (the
//! single source of truth for secrets — agents are read-through caches) and
//! makes one cheap authenticated request, then reports minimal account/identity
//! info as the vital's `detail`.
//!
//! The honesty contract from [`super`] is preserved to the letter:
//!
//! | Condition | status | detail |
//! |---|---|---|
//! | key absent from vault | `unavailable` | `no API key configured` |
//! | network / TLS failure | `error` | `unreachable: …` |
//! | `401` / `403` | `error` | `invalid key (HTTP …)` |
//! | Brave `422` `SUBSCRIPTION_TOKEN_INVALID` | `error` | `invalid key (HTTP 422)` |
//! | other non-2xx | `error` | `HTTP …` |
//! | `2xx` | `ok` | identity / account info (username, credits, quota) |
//!
//! Probes run concurrently on their own threads and are collected under a
//! shared deadline (same pattern as [`super::probe_remotes`]), so the wall time
//! is the slowest single probe rather than their sum.

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use super::vital;

/// Per-probe HTTP timeout — bounds a single authenticated round-trip.
const KEY_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Overall deadline for collecting every concurrent key probe. A probe that has
/// not reported by this point is filled in as a timeout rather than hanging the
/// endpoint.
const COLLECT_DEADLINE: Duration = Duration::from_secs(9);

/// A resolved key check: the display label, its category tag, the canonical
/// vault key name, and the probe function to run when the key is present.
struct KeyProbe {
    /// Human-readable service label shown on the vitals board.
    label: &'static str,
    /// Category tag grouping this vital (`vcs`, `service`).
    category: &'static str,
    /// Canonical vault key name the secret is resolved under.
    canonical: &'static str,
    /// The probe to run when the key is present.
    probe: fn(&str) -> ProbeResult,
}

/// Outcome of one authenticated probe: a status string plus a human detail.
struct ProbeResult {
    /// The vital status string (`ok` / `error`).
    status: &'static str,
    /// Human-readable detail (identity, quota, or failure reason).
    detail: String,
}

impl ProbeResult {
    /// A successful probe carrying an identity/quota detail.
    fn ok(detail: impl Into<String>) -> Self {
        Self { status: "ok", detail: detail.into() }
    }
    /// A failed probe carrying the honest failure detail.
    fn error(detail: impl Into<String>) -> Self {
        Self { status: "error", detail: detail.into() }
    }
}

/// Run every authenticated key-validation probe concurrently and return their
/// vitals. Keys are resolved from the vault up front (cheap, lock-free) so each
/// thread owns its key string; a missing key short-circuits to `unavailable`
/// without spawning a request.
pub(super) fn probe_keyed_services() -> Vec<serde_json::Value> {
    let probes: [KeyProbe; 5] = [
        KeyProbe { label: "GitHub", category: "vcs", canonical: "github", probe: github_check },
        KeyProbe { label: "Brave", category: "service", canonical: "brave", probe: brave_check },
        KeyProbe { label: "Firecrawl", category: "service", canonical: "firecrawl", probe: firecrawl_check },
        KeyProbe { label: "Voyage", category: "service", canonical: "voyage", probe: voyage_check },
        KeyProbe { label: "Datalab", category: "service", canonical: "datalab", probe: datalab_check },
    ];

    let total = probes.len();
    let (tx, rx) = mpsc::channel::<(usize, serde_json::Value)>();

    for (idx, kp) in probes.into_iter().enumerate() {
        // Resolve the key on the collecting thread (vault access is lock-free).
        let key = cp_vault::vault().get(kp.canonical).map(|s| s.expose().to_owned());
        let tx_thread = tx.clone();
        let _handle = thread::spawn(move || {
            let v = key.map_or_else(
                || vital((kp.label, kp.category), "unavailable", None, "no API key configured"),
                |k| {
                    let started = Instant::now();
                    let r = (kp.probe)(&k);
                    let latency = u64::try_from(started.elapsed().as_millis()).ok();
                    vital((kp.label, kp.category), r.status, latency, &r.detail)
                },
            );
            let _sent = tx_thread.send((idx, v));
        });
    }
    drop(tx);

    let mut slots: Vec<Option<serde_json::Value>> = vec![None; total];
    let deadline = Instant::now().checked_add(COLLECT_DEADLINE);
    let mut received = 0usize;
    while received < total {
        let remaining = deadline.map_or(Duration::ZERO, |d| d.saturating_duration_since(Instant::now()));
        if remaining.is_zero() {
            break;
        }
        match rx.recv_timeout(remaining) {
            Ok((idx, v)) => {
                if let Some(slot) = slots.get_mut(idx) {
                    *slot = Some(v);
                    received = received.saturating_add(1);
                }
            }
            Err(_) => break,
        }
    }

    slots
        .into_iter()
        .map(|slot| slot.unwrap_or_else(|| vital(("Service", "service"), "unavailable", None, "probe timed out")))
        .collect()
}

// ── HTTP helper ─────────────────────────────────────────────────────────

/// A blocking reqwest client bounded by [`KEY_PROBE_TIMEOUT`].
fn client() -> Option<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder().timeout(KEY_PROBE_TIMEOUT).build().ok()
}

/// Classify a non-success status into the honest error detail. Returns `None`
/// for a 2xx (the caller then reads the body), `Some(detail)` otherwise.
fn classify_failure(status: reqwest::StatusCode) -> Option<String> {
    if status.is_success() {
        return None;
    }
    if status.as_u16() == 401 || status.as_u16() == 403 {
        Some(format!("invalid key (HTTP {})", status.as_u16()))
    } else {
        Some(format!("HTTP {}", status.as_u16()))
    }
}

// ── Per-service probes ──────────────────────────────────────────────────

/// GitHub: `GET /user` reveals the token's associated account (`login`).
fn github_check(token: &str) -> ProbeResult {
    let Some(c) = client() else {
        return ProbeResult::error("client init failed");
    };
    let sent = c
        .get("https://api.github.com/user")
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "context-pilot-orchestrator")
        .header("Accept", "application/vnd.github+json")
        .send();
    let resp = match sent {
        Ok(r) => r,
        Err(e) => return ProbeResult::error(format!("unreachable: {e}")),
    };
    if let Some(fail) = classify_failure(resp.status()) {
        return ProbeResult::error(fail);
    }
    let Ok(json) = resp.json::<serde_json::Value>() else {
        return ProbeResult::ok("key valid");
    };
    let login = json.get("login").and_then(serde_json::Value::as_str);
    let email = json.get("email").and_then(serde_json::Value::as_str);
    match (login, email) {
        (Some(name), Some(mail)) => ProbeResult::ok(format!("@{name} · {mail}")),
        (Some(name), _) => ProbeResult::ok(format!("@{name}")),
        _ => ProbeResult::ok("key valid"),
    }
}

/// Firecrawl: `GET /v2/team/credit-usage` proves the key and reports credits.
fn firecrawl_check(token: &str) -> ProbeResult {
    let Some(c) = client() else {
        return ProbeResult::error("client init failed");
    };
    let sent = c
        .get("https://api.firecrawl.dev/v2/team/credit-usage")
        .header("Authorization", format!("Bearer {token}"))
        .send();
    let resp = match sent {
        Ok(r) => r,
        Err(e) => return ProbeResult::error(format!("unreachable: {e}")),
    };
    if let Some(fail) = classify_failure(resp.status()) {
        return ProbeResult::error(fail);
    }
    let Ok(json) = resp.json::<serde_json::Value>() else {
        return ProbeResult::ok("key valid");
    };
    // v2 shape: { "data": { "remaining_credits": N } }
    let credits = json
        .get("data")
        .and_then(|d| d.get("remaining_credits"))
        .or_else(|| json.get("remaining_credits"))
        .and_then(serde_json::Value::as_i64);
    credits.map_or_else(|| ProbeResult::ok("key valid"), |n| ProbeResult::ok(format!("valid · {n} credits left")))
}

/// Voyage: a minimal authenticated embedding proves the key (no whoami exists).
fn voyage_check(token: &str) -> ProbeResult {
    let Some(c) = client() else {
        return ProbeResult::error("client init failed");
    };
    let body = serde_json::json!({ "model": "voyage-3-lite", "input": ["ping"] });
    let sent = c
        .post("https://api.voyageai.com/v1/embeddings")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send();
    let resp = match sent {
        Ok(r) => r,
        Err(e) => return ProbeResult::error(format!("unreachable: {e}")),
    };
    if let Some(fail) = classify_failure(resp.status()) {
        return ProbeResult::error(fail);
    }
    // A 2xx with the auth header accepted is all that proves the key; Voyage
    // exposes no account identity, so there is nothing further to report.
    ProbeResult::ok("key valid")
}

/// Brave: a minimal authenticated search proves the key; the `X-RateLimit-*`
/// response headers report remaining quota (Brave exposes no account endpoint).
fn brave_check(token: &str) -> ProbeResult {
    let Some(c) = client() else {
        return ProbeResult::error("client init failed");
    };
    let sent = c
        .get("https://api.search.brave.com/res/v1/web/search?q=ping&count=1")
        .header("X-Subscription-Token", token)
        .header("Accept", "application/json")
        .send();
    let resp = match sent {
        Ok(r) => r,
        Err(e) => return ProbeResult::error(format!("unreachable: {e}")),
    };
    // Brave signals an INVALID subscription token with HTTP 422
    // (`SUBSCRIPTION_TOKEN_INVALID`), not the conventional 401/403 that
    // `classify_failure` recognises — so a bad key would otherwise surface as a
    // cryptic "HTTP 422" instead of the honesty contract's "invalid key" row.
    // Intercept a 422 whose body marks it as an authentication failure and
    // report it as an invalid key; a 422 for any OTHER reason (a genuinely
    // malformed request) still falls through to the generic classifier so a real
    // request bug stays distinguishable.
    let status = resp.status();
    if status.as_u16() == 422 {
        let body = resp.text().unwrap_or_default();
        if body.contains("SUBSCRIPTION_TOKEN_INVALID") || body.contains("\"component\":\"authentication\"") {
            return ProbeResult::error("invalid key (HTTP 422)");
        }
        return ProbeResult::error("HTTP 422");
    }
    if let Some(fail) = classify_failure(status) {
        return ProbeResult::error(fail);
    }
    // "X-RateLimit-Remaining" is "perSecond, perMonth" — the monthly figure is
    // the operator-meaningful quota left.
    let remaining = resp
        .headers()
        .get("x-ratelimit-remaining")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next_back().unwrap_or(s).trim().to_owned());
    match remaining {
        Some(q) if !q.is_empty() => ProbeResult::ok(format!("valid · {q} queries left")),
        _ => ProbeResult::ok("key valid"),
    }
}

/// Datalab: an authenticated request confirms the key. Datalab exposes no
/// whoami, so we hit an auth-gated endpoint and read the status: a `401`/`403`
/// means a bad key, anything else authenticated means the key is accepted.
fn datalab_check(token: &str) -> ProbeResult {
    let Some(c) = client() else {
        return ProbeResult::error("client init failed");
    };
    // A bogus request-id under the auth-gated results path: a valid key yields a
    // 4xx that is NOT 401/403 (e.g. 404 "not found"); a bad key yields 401/403.
    let sent = c
        .get("https://www.datalab.to/api/v1/marker/00000000-0000-0000-0000-000000000000")
        .header("X-Api-Key", token)
        .send();
    let resp = match sent {
        Ok(r) => r,
        Err(e) => return ProbeResult::error(format!("unreachable: {e}")),
    };
    let code = resp.status().as_u16();
    if code == 401 || code == 403 {
        ProbeResult::error(format!("invalid key (HTTP {code})"))
    } else {
        // 2xx or a non-auth 4xx (bad/missing request id) means the key was
        // accepted by the gateway.
        ProbeResult::ok("key valid")
    }
}
