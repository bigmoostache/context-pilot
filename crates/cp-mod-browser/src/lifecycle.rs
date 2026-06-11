//! Chrome process lifecycle: binary discovery, spawn, reconnect, kill.
//!
//! Chrome is owned by `cp-console-server` so it survives TUI reloads; this
//! module manages it exclusively through the public console client API
//! (`SessionHandle`). All browser *control* happens over CDP, directly
//! between the TUI and Chrome — it never transits the daemon (see `client`).

use std::path::PathBuf;

use cp_base::config::constants;
use cp_base::panels::now_ms;
use cp_mod_console::manager::{self, SessionHandle};

use crate::types::{BROWSER_KEY_PREFIX, BrowserState, ChromeMeta};

/// Candidate Chrome-family binary locations, probed in order.
const CHROME_CANDIDATES: &[&str] = &[
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
    "/usr/bin/google-chrome",
    "/usr/bin/google-chrome-stable",
    "/usr/bin/chromium",
    "/usr/bin/chromium-browser",
    "/usr/bin/brave-browser",
];

/// Marker line Chrome prints on stderr once the `DevTools` endpoint is ready.
const DEVTOOLS_MARKER: &str = "DevTools listening on ";

/// How long to wait for Chrome to print its `DevTools` WebSocket URL.
const WS_URL_WAIT_SECS: u64 = 15;

/// Locate a usable Chrome/Chromium/Brave binary.
/// Honors a `$CP_CHROME` override, then probes common install paths.
///
/// # Errors
///
/// Returns `Err` when no binary is found.
pub fn find_chrome() -> Result<String, String> {
    if let Ok(p) = std::env::var("CP_CHROME")
        && !p.is_empty()
    {
        return Ok(p);
    }
    for cand in CHROME_CANDIDATES {
        if std::path::Path::new(cand).exists() {
            return Ok((*cand).to_string());
        }
    }
    Err("No Chrome/Chromium/Brave binary found. Install Chrome or set $CP_CHROME to the binary path.".to_string())
}

/// Absolute path to the persistent Chrome profile directory
/// (`.context-pilot/browser/profile` — gitignored; keeps cookies/logins).
fn profile_dir() -> PathBuf {
    let base = PathBuf::from(constants::STORE_DIR).join("browser").join("profile");
    if base.is_absolute() { base } else { std::env::current_dir().unwrap_or_default().join(base) }
}

/// Spawn Chrome via the console server and discover its `DevTools` WebSocket URL.
/// On success, stores `meta` + `handle` in `bs` and returns the ws URL.
///
/// # Errors
///
/// Returns `Err` if no Chrome binary is found, the daemon can't spawn it,
/// or the `DevTools` URL doesn't appear in the log within the wait budget.
pub fn spawn_chrome(bs: &mut BrowserState, headless: bool) -> Result<String, String> {
    let chrome = find_chrome()?;
    let profile = profile_dir();
    std::fs::create_dir_all(&profile).map_err(|e| format!("Failed to create profile dir: {e}"))?;

    let key = format!("{BROWSER_KEY_PREFIX}{}", bs.next_session_id);
    bs.next_session_id = bs.next_session_id.saturating_add(1);

    // --remote-debugging-port=0: Chrome picks a free port and prints the
    // full ws:// URL on stderr, which the daemon redirects into the log file.
    let mut command = format!(
        "'{chrome}' --remote-debugging-port=0 --user-data-dir='{}' --no-first-run --no-default-browser-check",
        profile.display()
    );
    if headless {
        command.push_str(" --headless=new");
    }

    manager::find_or_create_server()?;
    let handle = SessionHandle::spawn(key.clone(), command.clone(), None)?;
    let ws_url = wait_for_ws_url(&handle.log_path)?;

    let meta = ChromeMeta {
        session_key: key,
        pid: handle.pid().unwrap_or(0),
        command,
        log_path: handle.log_path.clone(),
        started_at: now_ms(),
        ws_url: ws_url.clone(),
        headless,
    };
    bs.meta = Some(meta);
    bs.handle = Some(handle);
    Ok(ws_url)
}

/// Reconnect to a still-running Chrome after a TUI reload.
/// Returns `true` if the process is alive and the handle was restored.
pub fn reconnect_chrome(bs: &mut BrowserState, meta: ChromeMeta) -> bool {
    let handle = SessionHandle::reconnect(manager::ReconnectMeta {
        name: meta.session_key.clone(),
        command: meta.command.clone(),
        cwd: None,
        pid: meta.pid,
        log_path_str: meta.log_path.clone(),
        started_at: meta.started_at,
    });
    if handle.get_status().is_terminal() {
        return false;
    }
    bs.meta = Some(meta);
    bs.handle = Some(handle);
    true
}

/// Kill the managed Chrome process and clear all connection state.
pub fn kill_chrome(bs: &mut BrowserState) {
    if let Some(handle) = bs.handle.take() {
        handle.kill();
    }
    bs.meta = None;
    bs.client = None;
    bs.erefs.clear();
    bs.eref_selectors.clear();
    bs.snapshot_text.clear();
}

/// Reap orphaned `browser_*` daemon sessions that aren't in `known`.
/// Scoped to our namespace — console's `c_*` sessions are never touched.
pub fn cleanup_orphans<S: std::hash::BuildHasher>(known: &std::collections::HashSet<String, S>) {
    manager::kill_orphaned_processes(known, BROWSER_KEY_PREFIX);
}

/// Poll the session log until Chrome prints its `DevTools` WebSocket URL.
fn wait_for_ws_url(log_path: &str) -> Result<String, String> {
    let deadline = std::time::Instant::now().checked_add(std::time::Duration::from_secs(WS_URL_WAIT_SECS));
    loop {
        if let Some(url) = extract_ws_url(&std::fs::read_to_string(log_path).unwrap_or_default()) {
            return Ok(url);
        }
        if deadline.is_none_or(|d| std::time::Instant::now() >= d) {
            return Err(format!(
                "Chrome did not print a DevTools URL within {WS_URL_WAIT_SECS}s — check the log at {log_path}"
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Extract the `ws://` URL from Chrome's `DevTools listening on …` line.
fn extract_ws_url(log: &str) -> Option<String> {
    for line in log.lines() {
        if let Some(idx) = line.find(DEVTOOLS_MARKER) {
            let rest = line.get(idx.saturating_add(DEVTOOLS_MARKER.len())..)?;
            let url = rest.trim();
            if url.starts_with("ws://") {
                return Some(url.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::extract_ws_url;

    #[test]
    fn extracts_ws_url_from_stderr_line() {
        let log = "some noise\nDevTools listening on ws://127.0.0.1:53217/devtools/browser/abc-def\nmore";
        assert_eq!(
            extract_ws_url(log).as_deref(),
            Some("ws://127.0.0.1:53217/devtools/browser/abc-def"),
            "should extract the full ws URL"
        );
    }

    #[test]
    fn returns_none_without_marker() {
        assert!(extract_ws_url("no devtools line here").is_none(), "no marker means no URL");
    }
}
