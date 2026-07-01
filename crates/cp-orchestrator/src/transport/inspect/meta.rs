//! Enriched **agent meta** endpoints — maquette-compatible `Agent` objects
//! combining registry records, oplog-projected state, thread inspection, and
//! git branch info.
//!
//! * [`agent_meta`] — one agent's enriched info (`GET /api/agent/{id}/meta`).
//! * [`fleet_meta`] — all known agents as an enriched array
//!   (`GET /api/fleet/meta`).

use std::sync::Mutex;

use cp_wire::types::registry::Entry;

use crate::transport::Backend;
use crate::transport::rest::HttpReply;

/// `GET /api/agent/{id}/meta` — enriched agent info for the fleet/agent views.
///
/// Combines registry record (name, folder, model), oplog-projected state
/// (phase, cost), thread inspection (count, any-MY_TURN → "needs-you"), and
/// `git` branch. Returns a JSON object matching the maquette `Agent` shape.
pub fn agent_meta(state: &Mutex<Backend>, agent_id: &str) -> HttpReply {
    let entry = match crate::transport::rest::resolve_entry(state, agent_id) {
        Ok(e) => e,
        Err(reply) => return reply,
    };
    let enriched = build_agent_meta(state, agent_id, &entry);
    HttpReply::ok(&enriched)
}

/// `GET /api/fleet/meta` — enriched agent list for the fleet dashboard.
///
/// Returns an array of maquette-compatible `Agent` objects, one per known
/// agent in the registry directory. Each is built the same way as the
/// per-agent `/meta` endpoint.
///
/// When auth is enabled the list is filtered to only agents the caller may
/// access (FR-12) — system admins see everything (FR-09), regular users see
/// only their ACL-granted agents. This is the enriched twin of
/// [`fleet`](crate::transport::rest::fleet) and the endpoint the web dashboard
/// actually reads, so it must apply the same ACL filter or a regular user would
/// see every agent's card (T346).
pub fn fleet_meta(state: &Mutex<Backend>, auth_user: Option<&crate::services::auth::types::User>) -> HttpReply {
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
    // Retired agents live in the orchestrator-owned store, not the registry —
    // exclude them from the ACTIVE fleet so they render only in the dashboard's
    // Retired section (served by [`fleet_retired`]).
    let active: Vec<&Entry> =
        entries.iter().filter(|e| !state.lock().is_ok_and(|b| b.retired.is_retired(&e.id))).collect();

    // ACL filter (FR-12) — drop agents the caller has no access to.
    let active_ids: Vec<String> = active.iter().map(|e| e.id.clone()).collect();
    let visible = crate::transport::auth::filter_fleet(state, &active_ids, auth_user);
    let visible: std::collections::HashSet<&str> = visible.iter().map(String::as_str).collect();

    let agents: Vec<serde_json::Value> =
        active.iter().filter(|e| visible.contains(e.id.as_str())).map(|e| build_agent_meta(state, &e.id, e)).collect();
    HttpReply::ok(&agents)
}

/// `GET /api/fleet/retired` — the dashboard's Retired (archived) section.
///
/// Returns one maquette-`Agent`-shaped object per retired agent, built from the
/// orchestrator's [`RetiredStore`](crate::services::RetiredStore) snapshot rather
/// than the live registry (a retired agent has no running process to inspect).
/// Each carries `status: "retired"` and a `retiredAt` epoch-ms so the frontend
/// can render and sort the section without a second lookup.
pub fn fleet_retired(state: &Mutex<Backend>, auth_user: Option<&crate::services::auth::types::User>) -> HttpReply {
    let records = {
        let Ok(b) = state.lock() else {
            return HttpReply::error(500, "backend lock poisoned");
        };
        b.retired.list()
    };
    // ACL filter (FR-12) — a regular user only sees retired agents they were
    // granted access to (the ACL entry survives retirement). System admins see
    // all; auth-disabled passes everything through.
    let ids: Vec<String> = records.iter().map(|r| r.id.clone()).collect();
    let visible = crate::transport::auth::filter_fleet(state, &ids, auth_user);
    let visible: std::collections::HashSet<&str> = visible.iter().map(String::as_str).collect();
    let agents: Vec<serde_json::Value> = records
        .iter()
        .filter(|r| visible.contains(r.id.as_str()))
        .map(|r| {
            // Dashboard name override wins over the folder-basename snapshot
            // captured at retire time — the user may rename a retired agent.
            let name =
                state.lock().ok().and_then(|b| b.names.get(&r.id).map(str::to_owned)).unwrap_or_else(|| r.name.clone());
            serde_json::json!({
                "id": r.id,
                "name": name,
                "folder": r.folder,
                "branch": "",
                "model": r.model,
                "provider": r.provider,
                "status": "retired",
                "costUsd": 0.0,
                "task": "",
                "threads": 0,
                "lastActivity": r.retired_at_ms,
                "retiredAt": r.retired_at_ms,
                "accent": "interactive",
            })
        })
        .collect();
    HttpReply::ok(&agents)
}

// ── Helpers ────────────────────────────────────────────────────────────

/// Build the enriched agent JSON for one agent.
fn build_agent_meta(state: &Mutex<Backend>, agent_id: &str, entry: &Entry) -> serde_json::Value {
    let folder = &entry.folder;
    let name_override = state.lock().ok().and_then(|b| b.names.get(agent_id).map(str::to_owned));
    let default_name = std::path::Path::new(folder).file_name().and_then(std::ffi::OsStr::to_str).unwrap_or(agent_id);
    let name = name_override.as_deref().unwrap_or(default_name);

    // Phase + lifecycle + cost + cumulative tokens from the materialized view
    // (brief lock). These ride the push plane (PhaseTransition / CostAggregate
    // deltas folded into the view), so serving them here keeps a COLD load /
    // backstop poll consistent with the live SSE deltas the frontend folds —
    // the same value arrives over both planes (T297 live HUD reactivity).
    let (phase, lifecycle, cost_usd, input_tokens, output_tokens, context) =
        state.lock().map_or((None, None, 0.0, 0, 0, Default::default()), |b| {
            b.view.get(agent_id).map_or((None, None, 0.0, 0, 0, Default::default()), |v| {
                (v.phase, v.lifecycle, v.cost.cost_usd, v.cost.input_tokens, v.cost.output_tokens, v.context)
            })
        });

    // Thread count + any-MY_TURN + last activity from config.json.
    let (threads_count, has_my_turn, last_activity_ms, task) = inspect_threads(state, folder);

    let branch = git_branch(folder);
    // The agent's configured LLM provider id (e.g. "claudecodev2", "claudecode")
    // — authoritative for the cockpit's model picker. Several providers share
    // identical model API names (every Claude-Code variant reuses the Anthropic
    // model roster), so the picker cannot infer the provider from `model` alone;
    // it needs this explicit id to select the right provider tab.
    let provider = read_provider(state, folder);

    // The agent's true current model apiName for its ACTIVE provider. The
    // registry `entry.model` is unreliable (it snapshots the Anthropic model
    // regardless of the live provider), so prefer the authoritative
    // `config.json` per-provider model — falling back to `entry.model` only
    // when config is unreadable or the provider has no dedicated field.
    let model = read_current_model(state, folder, &provider).unwrap_or_else(|| entry.model.clone());

    let status = derive_status(phase, lifecycle, has_my_turn);
    let accent = derive_accent(&status);
    let has_avatar = state.lock().is_ok_and(|b| b.avatars.has(agent_id));

    serde_json::json!({
        "id": agent_id,
        "name": name,
        "folder": folder,
        "branch": branch,
        "model": model,
        "provider": provider,
        "status": status,
        // Raw execution phase (idle/streaming/tooling) on TOP of the derived
        // status, so the live HUD can show the distinct phase (not just the
        // working/idle binary) and a cold load matches the SSE PhaseTransition
        // deltas the frontend folds (T297). `None` (pre-first-transition) → null.
        "phase": phase.map(phase_label),
        "costUsd": cost_usd,
        // Cumulative-since-boot tokens — folded from CostAggregate, the same
        // figures the live `cost_aggregate` delta carries, so the HUD's token
        // counters stay consistent across the push + cold-load planes (T297).
        "inputTokens": input_tokens,
        "outputTokens": output_tokens,
        // Live context-window occupancy — the agent's own authoritative
        // `used / threshold / budget` token triple (folded from ContextUsage,
        // the same figure the live delta carries). The web HUD shows THIS so its
        // meter is byte-identical to the agent's ratatui sidebar, not a frontend
        // re-sum that drifts (T297). Zero until the agent emits its first sample.
        "contextUsed": context.used_tokens,
        "contextThreshold": context.threshold_tokens,
        "contextBudget": context.budget_tokens,
        // The cache hit/miss split of contextUsed (hit + miss == used), folded
        // from the same ContextUsage delta. The web HUD shows `Used (hit)` /
        // `Used (miss)` from these, byte-identical to the ratatui sidebar's
        // green/amber token-bar segments (T297). Zero before the first sample.
        "contextHit": context.hit_tokens,
        "contextMiss": context.miss_tokens,
        "task": task,
        "threads": threads_count,
        "lastActivity": last_activity_ms,
        "accent": accent,
        "hasAvatar": has_avatar,
    })
}

/// Inspect an agent's threads from config.json for count, `MY_TURN` status,
/// last activity, and current task (focused thread name).
fn inspect_threads(state: &Mutex<Backend>, folder: &str) -> (usize, bool, u64, String) {
    let folder_path = std::path::Path::new(folder);
    let config = {
        let Ok(mut b) = state.lock() else {
            return (0, false, 0, String::new());
        };
        b.inspect_mut().read_config(folder_path).ok()
    };
    let Some(config) = config else {
        return (0, false, 0, String::new());
    };
    let empty_arr = Vec::new();
    let threads = config
        .get("modules")
        .and_then(|m| m.get("threads"))
        .and_then(|t| t.get("threads"))
        .and_then(serde_json::Value::as_array)
        .unwrap_or(&empty_arr);

    let count = threads.len();
    let has_my_turn = threads.iter().any(|t| t.get("status").and_then(serde_json::Value::as_str) == Some("MyTurn"));
    let last_activity = threads
        .iter()
        .filter_map(|t| {
            t.get("messages")
                .and_then(serde_json::Value::as_array)
                .and_then(|msgs| msgs.last())
                .and_then(|m| m.get("timestamp"))
                .and_then(serde_json::Value::as_u64)
        })
        .max()
        .unwrap_or(0);

    // Task = name of the first MY_TURN thread, or empty.
    let task = threads
        .iter()
        .find(|t| t.get("status").and_then(serde_json::Value::as_str) == Some("MyTurn"))
        .and_then(|t| t.get("name"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_owned();

    (count, has_my_turn, last_activity, task)
}

/// Read the agent's configured LLM provider id from `config.json`
/// (`modules.core.llm_provider`).
///
/// Returns an empty string when unreadable — the frontend then falls back to
/// inferring the provider from the model name. The id uses the wire serde form
/// (lowercase: `anthropic`, `claudecode`, `claudecodeapikey`, `claudecodev2`,
/// `grok`, `groq`, `deepseek`, `minimax`).
pub(crate) fn read_provider(state: &Mutex<Backend>, folder: &str) -> String {
    let folder_path = std::path::Path::new(folder);
    let config = {
        let Ok(mut b) = state.lock() else {
            return String::new();
        };
        b.inspect_mut().read_config(folder_path).ok()
    };
    config
        .as_ref()
        .and_then(|c| c.get("modules"))
        .and_then(|m| m.get("core"))
        .and_then(|c| c.get("llm_provider"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// Read the agent's current model for its ACTIVE provider and return its public
/// `apiName` (the form the web picker resolves a selection by).
///
/// `config.json`'s `modules.core` holds one model field per provider family,
/// storing the enum id (e.g. `claude-opus48`). The active field is chosen by
/// `provider` — the three Anthropic-family providers (`anthropic`,
/// `claudecode`, `claudecodeapikey`) share `anthropic_model`; every other
/// provider has its own. The stored enum id is mapped to its `apiName` via the
/// provider registry. Returns `None` when config is unreadable, the field is
/// absent, or the id is unknown — the caller then falls back to the registry
/// record's model string.
fn read_current_model(state: &Mutex<Backend>, folder: &str, provider: &str) -> Option<String> {
    let field = match provider {
        "anthropic" | "claudecode" | "claudecodeapikey" => "anthropic_model",
        "claudecodev2" => "claude_code_v2_model",
        "grok" => "grok_model",
        "groq" => "groq_model",
        "deepseek" => "deepseek_model",
        "minimax" => "minimax_model",
        _ => return None,
    };
    let folder_path = std::path::Path::new(folder);
    let config = {
        let mut b = state.lock().ok()?;
        b.inspect_mut().read_config(folder_path).ok()?
    };
    let model_id = config.get("modules")?.get("core")?.get(field).and_then(serde_json::Value::as_str)?;
    super::providers::resolve_api_name(provider, model_id).map(str::to_owned)
}

/// Read the current git branch of an agent's working directory.
///
/// Returns an empty string if `git` is unavailable or the folder is not a
/// repository — this is informational, never an error.
fn git_branch(folder: &str) -> String {
    std::process::Command::new("git")
        .args(["-C", folder, "rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()
        .and_then(
            |o| {
                if o.status.success() { String::from_utf8(o.stdout).ok().map(|s| s.trim().to_owned()) } else { None }
            },
        )
        .unwrap_or_default()
}

/// Derive the maquette `AgentStatus` from lifecycle, phase, and thread state.
///
/// An agent that has emitted a graceful-shutdown lifecycle (`Stopping`/
/// `Stopped`, I8) can never be "working": the authoritative oplog signal wins
/// over a stale phase that a torn shutdown might have left behind. Otherwise
/// the live `phase` decides "working", falling back to "needs-you" (any
/// MY_TURN thread) or "idle".
fn derive_status(
    phase: Option<cp_wire::types::Phase>,
    lifecycle: Option<cp_wire::types::LifecycleState>,
    has_my_turn: bool,
) -> String {
    use cp_wire::types::LifecycleState;
    if matches!(lifecycle, Some(LifecycleState::Stopping | LifecycleState::Stopped)) {
        return if has_my_turn { "needs-you".to_owned() } else { "idle".to_owned() };
    }
    match phase {
        Some(cp_wire::types::Phase::Streaming | cp_wire::types::Phase::Tooling) => "working".to_owned(),
        _ if has_my_turn => "needs-you".to_owned(),
        _ => "idle".to_owned(),
    }
}

/// Map a wire [`Phase`](cp_wire::types::Phase) to its lowercase label
/// (`idle`/`streaming`/`tooling`) — the exact serde form the frontend's
/// `applyAgentDelta` phase fold uses, so the cold-load `/meta` value and the
/// live SSE `PhaseTransition` delta resolve to the same string (T297).
fn phase_label(phase: cp_wire::types::Phase) -> &'static str {
    use cp_wire::types::Phase;
    match phase {
        Phase::Idle => "idle",
        Phase::Streaming => "streaming",
        Phase::Tooling => "tooling",
    }
}

/// Map a status string to the maquette accent token.
fn derive_accent(status: &str) -> &'static str {
    match status {
        "working" => "ok",
        "needs-you" => "signal",
        _ => "interactive",
    }
}

/// List all registry entries in a directory.
fn list_entries(dir: &std::path::Path) -> std::io::Result<Vec<Entry>> {
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
