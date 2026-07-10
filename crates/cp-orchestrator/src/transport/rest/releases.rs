//! Release management REST handlers (T427).
//!
//! Five endpoints to manage locally downloaded release binaries:
//!
//! * `GET  /api/releases`          — merged local + remote list, arch, active tag
//! * `PUT  /api/releases/arch`     — set architecture (manual or auto-detect)
//! * `POST /api/releases/download` — download a release tarball by tag
//! * `PUT  /api/releases/select`   — set the active binary for future agent launches
//! * `DELETE /api/releases/{tag}`  — remove a locally downloaded release
//!
//! All endpoints are admin-only — the router gates them behind the auth check
//! before dispatching here.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::Deserialize;

use super::{Backend, HttpReply};
use crate::services::releases::{KNOWN_ARCHS, semver_sort_key};
use crate::supervisor;

/// `GET /api/releases` — list all releases (local + remote merged), current
/// architecture, and selected version.
///
/// Fetches the remote release list from GitHub on every call (cached by
/// TanStack on the frontend). Local releases are scanned from the releases
/// directory. The response merges both: each release carries `local` (bool)
/// and `selected` (bool) flags alongside the remote metadata.
pub(crate) fn list_releases(state: &Mutex<Backend>) -> HttpReply {
    let Ok(b) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };

    let arch = b.releases.arch().to_owned();
    let arch_auto = b.releases.is_arch_auto();
    let active_tag = b.releases.active_tag().map(str::to_owned);
    let current_binary = b.agent_binary.to_string_lossy().into_owned();
    let locals = b.releases.local_releases();

    // Fetch remote releases (lock released — network I/O).
    let remotes = b.releases.fetch_remote_releases();
    drop(b);

    // Build the merged response.
    let remote_list = remotes.unwrap_or_default();
    let mut releases = serde_json::Map::new();

    // Seed with remote releases.
    for r in &remote_list {
        let local = locals.iter().any(|l| l.tag == r.tag);
        let selected = active_tag.as_deref() == Some(&r.tag);
        let entry = serde_json::json!({
            "tag": r.tag,
            "name": r.name,
            "publishedAt": r.published_at,
            "assetUrl": r.asset_url,
            "assetSize": r.asset_size,
            "isLatest": r.is_latest,
            "local": local,
            "selected": selected,
        });
        let _prev = releases.insert(r.tag.clone(), entry);
    }

    // Add any local-only releases not in remotes.
    for l in &locals {
        if !releases.contains_key(&l.tag) {
            let selected = active_tag.as_deref() == Some(l.tag.as_str());
            let entry = serde_json::json!({
                "tag": l.tag,
                "name": l.tag,
                "publishedAt": null,
                "assetUrl": null,
                "assetSize": null,
                "isLatest": false,
                "local": true,
                "selected": selected,
                "binarySize": l.binary_size,
            });
            let _prev = releases.insert(l.tag.clone(), entry);
        }
    }

    // Flatten into a sorted array (newest first by publish date, then semver).
    let mut release_list: Vec<serde_json::Value> = releases.into_values().collect();
    release_list.sort_by(|a, b| {
        let pa = a.get("publishedAt").and_then(|v| v.as_str()).unwrap_or("");
        let pb = b.get("publishedAt").and_then(|v| v.as_str()).unwrap_or("");
        // Primary: published date descending (ISO 8601 sorts lexicographically).
        // Fallback: semver descending for releases without a publish date.
        pb.cmp(pa).then_with(|| {
            let ta = a.get("tag").and_then(|v| v.as_str()).unwrap_or("");
            let tb = b.get("tag").and_then(|v| v.as_str()).unwrap_or("");
            semver_sort_key(tb).cmp(&semver_sort_key(ta))
        })
    });

    HttpReply::ok(&serde_json::json!({
        "arch": arch,
        "archAuto": arch_auto,
        "activeTag": active_tag,
        "currentBinary": current_binary,
        "knownArchs": KNOWN_ARCHS,
        "releases": release_list,
    }))
}

/// `PUT /api/releases/arch` — set architecture manually or reset to auto-detect.
///
/// Body: `{ "arch": "linux-x86_64" }` or `{ "auto": true }` to auto-detect.
pub(crate) fn set_arch(state: &Mutex<Backend>, body: &[u8]) -> HttpReply {
    #[derive(Deserialize)]
    struct Req {
        arch: Option<String>,
        auto: Option<bool>,
    }
    let Ok(req) = serde_json::from_slice::<Req>(body) else {
        return HttpReply::error(400, "expected {\"arch\":\"...\"} or {\"auto\":true}");
    };

    let Ok(mut b) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };

    if req.auto == Some(true) {
        b.releases.auto_detect_arch();
    } else if let Some(arch) = &req.arch {
        if !KNOWN_ARCHS.contains(&arch.as_str()) {
            return HttpReply::error(400, &format!("unknown arch: {arch}"));
        }
        b.releases.set_arch(arch);
    } else {
        return HttpReply::error(400, "expected {\"arch\":\"...\"} or {\"auto\":true}");
    }

    HttpReply::ok(&serde_json::json!({
        "arch": b.releases.arch(),
        "archAuto": b.releases.is_arch_auto(),
    }))
}

/// `POST /api/releases/download` — download a specific release by tag.
///
/// Body: `{ "tag": "v0.3.0-abc1234" }`. The handler blocks while downloading
/// and extracting (runs on a tiny_http thread, not the main loop).
pub(crate) fn download_release(state: &Mutex<Backend>, body: &[u8]) -> HttpReply {
    #[derive(Deserialize)]
    struct Req {
        tag: String,
    }
    let Ok(req) = serde_json::from_slice::<Req>(body) else {
        return HttpReply::error(400, "expected {\"tag\":\"...\"}");
    };

    // First, find the asset URL from the remote releases list.
    let (store_arch, asset_url) = {
        let Ok(b) = state.lock() else {
            return HttpReply::error(500, "backend lock poisoned");
        };
        let arch = b.releases.arch().to_owned();

        // Check if already downloaded.
        if b.releases.binary_path(&req.tag).exists() {
            return HttpReply::error(409, &format!("release {} already downloaded", req.tag));
        }

        // Fetch the remote release list to find the asset URL.
        let remotes = b.releases.fetch_remote_releases();
        drop(b);

        let url = remotes.ok().and_then(|rs| rs.into_iter().find(|r| r.tag == req.tag).and_then(|r| r.asset_url));
        (arch, url)
    };

    let Some(url) = asset_url else {
        return HttpReply::error(404, &format!("no asset found for tag {} on arch {store_arch}", req.tag));
    };

    // Download + extract (blocking, lock-free).
    let Ok(b) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };
    match b.releases.download(&req.tag, &url) {
        Ok(()) => HttpReply::ok(&serde_json::json!({
            "status": "downloaded",
            "tag": req.tag,
        })),
        Err(e) => HttpReply::error(502, &e),
    }
}

/// `PUT /api/releases/select` — set the active binary for future agent launches.
///
/// Body: `{ "tag": "v0.3.0-abc1234" }`. Updates `Backend.agent_binary` and
/// the supervisor's allow-list so the next `create` or `restart` uses this
/// binary.
pub(crate) fn select_release(state: &Mutex<Backend>, body: &[u8]) -> HttpReply {
    #[derive(Deserialize)]
    struct Req {
        tag: String,
    }
    let Ok(req) = serde_json::from_slice::<Req>(body) else {
        return HttpReply::error(400, "expected {\"tag\":\"...\"}");
    };

    let Ok(mut b) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };

    let binary_path = match b.releases.select(&req.tag) {
        Ok(path) => path,
        Err(e) => return HttpReply::error(400, &e),
    };

    // Update the agent binary and supervisor allow-list.
    b.agent_binary = binary_path.clone();
    b.supervisor = supervisor::AgentSupervisor::new(&[binary_path.clone()]);

    HttpReply::ok(&serde_json::json!({
        "status": "selected",
        "tag": req.tag,
        "binaryPath": binary_path.to_string_lossy(),
    }))
}

/// `DELETE /api/releases/{tag}` — remove a locally downloaded release.
pub(crate) fn delete_release(state: &Mutex<Backend>, tag: &str) -> HttpReply {
    let Ok(mut b) = state.lock() else {
        return HttpReply::error(500, "backend lock poisoned");
    };

    match b.releases.delete(tag) {
        Ok(()) => HttpReply::ok(&serde_json::json!({ "status": "deleted", "tag": tag })),
        Err(e) => HttpReply::error(400, &e),
    }
}

/// `POST /api/releases/deploy` — select a release and restart the entire fleet.
///
/// Combines [`select_release`] + a loop of [`restart_agent`](super::restart_agent)
/// into one atomic deploy action. If `tag` is provided, the release is selected
/// first; otherwise the currently active release is used (agents are just
/// restarted on the current binary).
///
/// Returns a summary of which agents were restarted and any errors encountered.
pub(crate) fn deploy_fleet(state: &Mutex<Backend>, body: &[u8]) -> HttpReply {
    #[derive(Deserialize)]
    struct Req {
        tag: Option<String>,
    }
    let req = serde_json::from_slice::<Req>(body).unwrap_or(Req { tag: None });

    // 1. Select the release if a tag was provided.
    let active_tag = if let Some(ref tag) = req.tag {
        let Ok(mut b) = state.lock() else {
            return HttpReply::error(500, "backend lock poisoned");
        };
        let binary_path = match b.releases.select(tag) {
            Ok(p) => p,
            Err(e) => return HttpReply::error(400, &e),
        };
        b.agent_binary = binary_path.clone();
        b.supervisor = supervisor::AgentSupervisor::new(&[binary_path]);
        tag.clone()
    } else {
        let Ok(b) = state.lock() else {
            return HttpReply::error(500, "backend lock poisoned");
        };
        b.releases.active_tag().unwrap_or("(current)").to_owned()
    };

    // 2. Collect running agent IDs + shared config under one lock.
    let (agent_ids, agents_dir, binary) = {
        let Ok(b) = state.lock() else {
            return HttpReply::error(500, "backend lock poisoned");
        };
        let ids: Vec<String> = b.view.agent_ids().map(str::to_owned).collect();
        (ids, b.agents_dir.clone(), b.agent_binary.clone())
    };

    // 3. Restart each agent (same logic as restart_agent, batched).
    let mut restarted: Vec<serde_json::Value> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let agents_dir_str = agents_dir.to_string_lossy().into_owned();

    for id in &agent_ids {
        let entry = match super::resolve_entry(state, id) {
            Ok(e) => e,
            Err(_) => {
                errors.push(format!("{id}: not found in registry"));
                continue;
            }
        };
        let folder = PathBuf::from(&entry.folder);
        let key = folder.to_string_lossy().into_owned();

        // Kill old process (lock-free — may block up to the stop grace).
        supervisor::kill_pid(entry.pid);

        // Drop stale supervised record.
        if let Ok(mut b) = state.lock() {
            if b.supervisor.is_supervised(&key) {
                let _stopped = b.supervisor.stop(&key);
            }
        }

        // Respawn on the same folder with the (potentially new) binary.
        let env: [(&str, &str); 2] = [("CP_BRIDGE", "1"), ("CP_AGENTS_DIR", &agents_dir_str)];
        match state.lock() {
            Ok(mut b) => match b.supervisor.spawn_pty(key, &binary, &folder, &env) {
                Ok(pid) => restarted.push(serde_json::json!({ "id": id, "pid": pid })),
                Err(e) => errors.push(format!("{id}: spawn failed: {e}")),
            },
            Err(_) => errors.push(format!("{id}: backend lock poisoned")),
        }
    }

    HttpReply::ok(&serde_json::json!({
        "status": "deployed",
        "tag": active_tag,
        "restarted": restarted,
        "errors": errors,
    }))
}

/// `POST /api/releases/restart-orchestrator` — restart the orchestrator process
/// **in place** so it actually comes back up.
///
/// Sends the HTTP response first, then (after a short delay so the response
/// reaches the client) re-executes the running binary via `execv`. This
/// **replaces the current process image with a fresh one on the same PID** —
/// the listening socket is closed automatically on `exec` (Rust opens it
/// `SOCK_CLOEXEC`), freeing the port, and the new image re-binds and re-reads
/// its config from the environment, which is inherited across `exec`.
///
/// Why not a bare `SIGTERM`? The previous implementation signalled itself and
/// relied on an external supervisor (procd) to respawn. On any host without
/// such a supervisor — a dev machine, or a deployment where the service is not
/// under an auto-restart supervisor — SIGTERM simply killed the orchestrator,
/// leaving the frontend with no backend. Re-exec works with **or without** a
/// supervisor, and because the PID is preserved it never trips procd's
/// crash-loop back-off either.
///
/// If the re-exec fails (e.g. `current_exe` cannot be resolved, or `execv`
/// errors) we fall back to the old `SIGTERM` behaviour so a supervised host can
/// still respawn us.
pub(crate) fn restart_orchestrator(_state: &Mutex<Backend>) -> HttpReply {
    use std::os::unix::process::CommandExt as _;

    let _restart = std::thread::spawn(|| {
        // Let the HTTP 200 flush to the client before the socket goes away.
        std::thread::sleep(std::time::Duration::from_millis(200));

        match std::env::current_exe() {
            Ok(exe) => {
                // Forward the original arguments; the environment (which carries
                // all orchestrator config) is inherited automatically.
                let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
                // `exec` only ever returns on failure — on success it never comes
                // back because the process image is replaced.
                let err = std::process::Command::new(&exe).args(&args).exec();
                eprintln!("restart_orchestrator: exec of {} failed: {err}; falling back to SIGTERM", exe.display());
            }
            Err(e) => {
                eprintln!("restart_orchestrator: current_exe() failed: {e}; falling back to SIGTERM");
            }
        }

        // Fallback: if re-exec did not take over, signal ourselves so a
        // supervisor (procd) can respawn the service the old way.
        let _sent = nix::sys::signal::kill(nix::unistd::Pid::this(), nix::sys::signal::Signal::SIGTERM);
    });
    HttpReply::ok(&serde_json::json!({ "status": "restarting" }))
}
