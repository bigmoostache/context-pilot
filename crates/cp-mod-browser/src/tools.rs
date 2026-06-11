//! Tool implementations: open, goto, snapshot, click, type, extract,
//! screenshot, eval, close.
//!
//! **Off-main-thread execution.** Every slow CDP op runs on a worker thread via
//! `spawn_async_tool`, so the TUI event loop keeps rendering and accepting input
//! while a navigation/click/extract is in flight. The main thread only does
//! cheap work up front — read `ws_url`, resolve an e-ref → selector, clone the
//! shared `Arc`s — then hands the blocking work to the worker and returns a
//! deferred sentinel. The result is delivered back through the watcher pipeline
//! (`ChannelWatcher`), exactly like the console/brave/firecrawl async tools.
//!
//! Inline results stay compact; heavy state goes to the Browser panel (see
//! `panel`), updated from the worker via the shared `Mutex<SharedBrowser>`.

use std::sync::{Arc, Mutex};

use cp_base::state::runtime::State;
use cp_base::tools::async_exec::{ToolOutput, spawn_async_tool};
use cp_base::tools::{ToolResult, ToolUse};

use crate::client::{Client, INLINE_CAP_BYTES, connect_shared, truncate_utf8};
use crate::types::{BrowserState, ConnSlot, SharedBrowser};
use crate::{lifecycle, snapshot};

/// Extraction results larger than this go to a file instead of inline.
const EXTRACT_INLINE_MAX: usize = INLINE_CAP_BYTES;

/// Worker-thread budget for a single CDP op (covers nav settle + op timeout).
const OP_TIMEOUT_SECS: u64 = 30;

/// Dispatch a browser tool call.
pub fn execute(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!(&format!("browser_{}", tool.name));
    match tool.name.as_str() {
        "browser_open" => open(tool, state),
        "browser_goto" => goto(tool, state),
        "browser_snapshot" => take_snapshot(tool, state),
        "browser_click" => click(tool, state),
        "browser_type" => type_text(tool, state),
        "browser_extract" => extract(tool, state),
        "browser_screenshot" => screenshot(tool, state),
        "browser_eval" => eval(tool, state),
        "browser_close" => close(tool, state),
        other => ToolResult::new(tool.id.clone(), format!("Unknown browser tool: {other}"), true),
    }
}

/// Handles shared by every worker closure: the connection slot, the op
/// serializer, the worker-written runtime data, and the `ws_url` to (re)connect.
///
/// Gathered on the main thread (cheap) and moved into the worker, so the worker
/// needs no access to `State`.
struct OpCtx {
    /// Shared, lockable slot holding the cached CDP connection.
    conn: ConnSlot,
    /// Serializes CDP ops so concurrent workers never interleave on the transport.
    op_lock: Arc<Mutex<()>>,
    /// Worker-written runtime data (e-refs, last action, url, title).
    shared: Arc<Mutex<SharedBrowser>>,
    /// `DevTools` WebSocket URL to (re)connect to.
    ws_url: String,
}

/// Build an [`OpCtx`] from current state, erroring if no Chrome is running.
fn op_ctx(state: &State) -> Result<OpCtx, String> {
    let bs = BrowserState::get(state);
    if !bs.is_running() {
        return Err("No browser running — call browser_open first.".to_string());
    }
    let ws_url = bs.meta.as_ref().map(|m| m.ws_url.clone()).ok_or_else(|| "No browser metadata".to_string())?;
    Ok(OpCtx {
        conn: Arc::clone(&bs.conn),
        op_lock: Arc::clone(&bs.op_lock),
        shared: Arc::clone(&bs.shared),
        ws_url,
    })
}

/// Run a CDP op on a worker thread and deliver the result asynchronously.
///
/// `op` receives the connected [`Client`] and a handle to the shared runtime
/// data (to write e-refs / url / title), and returns the LLM-facing string. It
/// runs under the op-lock (serialized) and inside `catch_panic` (a dead-Chrome
/// panic becomes a clean error, never a crash — see `client::catch_panic`).
///
/// The closure must be `Send + 'static`; `op` and `OpCtx` satisfy that
/// (`Client` is `Send + Sync`, the rest are `Arc`/`String`).
fn run_browser_op<F>(state: &mut State, tool: &ToolUse, op: F) -> ToolResult
where
    F: FnOnce(&Client, &Arc<Mutex<SharedBrowser>>) -> Result<String, String> + Send + 'static,
{
    let ctx = match op_ctx(state) {
        Ok(c) => c,
        Err(e) => return ToolResult::new(tool.id.clone(), e, true),
    };
    spawn_async_tool(state, tool, OP_TIMEOUT_SECS, move || {
        // Serialize CDP ops: one worker on the single transport at a time.
        let _op = ctx.op_lock.lock();
        let result = crate::client::catch_panic("op", || {
            let client = connect_shared(&ctx.conn, &ctx.ws_url)?;
            op(&client, &ctx.shared)
        });
        match result {
            Ok(content) => ToolOutput { content, is_error: false, create_panel: None, preserves_tempo: false },
            Err(e) => ToolOutput { content: e, is_error: true, create_panel: None, preserves_tempo: false },
        }
    })
}

/// Record the last action into shared runtime data (worker side).
fn note_shared(shared: &Arc<Mutex<SharedBrowser>>, action: &str) {
    if let Ok(mut s) = shared.lock() {
        s.last_action = action.to_string();
    }
}

/// Record url/title/last-action into shared runtime data (worker side).
fn note_nav(shared: &Arc<Mutex<SharedBrowser>>, client: &Client, action: &str) -> (String, String) {
    let (url, title) = (client.url(), client.title());
    if let Ok(mut s) = shared.lock() {
        s.url.clone_from(&url);
        s.title.clone_from(&title);
        s.last_action = action.to_string();
    }
    (url, title)
}

/// `browser_open`: launch or reuse Chrome (process spawn is a deliberate one-shot
/// on the main thread), then navigate asynchronously if a `url` was given.
fn open(tool: &ToolUse, state: &mut State) -> ToolResult {
    let headless = tool.input.get("headless").and_then(serde_json::Value::as_bool).unwrap_or(true);
    let nav_url = tool.input.get("url").and_then(|v| v.as_str()).map(ToString::to_string);
    let use_real_profile = tool.input.get("use_real_profile").and_then(serde_json::Value::as_bool).unwrap_or(false);

    let running = BrowserState::get(state).is_running();
    if !running {
        let bs = BrowserState::get_mut(state);
        if let Err(e) = lifecycle::spawn_chrome(bs, headless, use_real_profile) {
            return ToolResult::new(tool.id.clone(), e, true);
        }
    }
    crate::panel::ensure_panel(state);

    let base = if running {
        let actual_headless = BrowserState::get(state).meta.as_ref().is_some_and(|m| m.headless);
        if actual_headless == headless {
            "Browser already running — reusing it.".to_string()
        } else {
            format!(
                "Browser already running — reusing it (note: it is {}, the requested headless={} was ignored; \
                 close it first to change mode).",
                if actual_headless { "headless" } else { "headed" },
                headless
            )
        }
    } else {
        format!("Chrome launched ({}).", if headless { "headless" } else { "headed" })
    };

    crate::panel::mark_dirty(state);

    // No URL given: report the launch note synchronously (no CDP work).
    let Some(u) = nav_url else {
        note_shared(&BrowserState::get(state).shared, &base);
        return ToolResult::new(tool.id.clone(), base, false);
    };
    // Navigate on the worker thread; prepend the launch note to the result.
    run_browser_op(state, tool, move |c, shared| {
        c.goto(&u)?;
        let (landed, title) = note_nav(shared, c, &format!("open + goto {u}"));
        Ok(format!("{base} Now at {landed} — \"{title}\""))
    })
}

/// `browser_goto`: navigate and report URL + title.
fn goto(tool: &ToolUse, state: &mut State) -> ToolResult {
    let url = match tool.input.get("url").and_then(|v| v.as_str()) {
        Some(u) => u.to_string(),
        None => return ToolResult::new(tool.id.clone(), "Missing required 'url' parameter".to_string(), true),
    };
    run_browser_op(state, tool, move |c, shared| {
        c.goto(&url)?;
        let (url, title) = note_nav(shared, c, &format!("goto {url}"));
        Ok(format!("Now at {url} — \"{title}\". e-refs are stale: snapshot before clicking."))
    })
}

/// `browser_snapshot`: enumerate interactive elements, digest inline.
fn take_snapshot(tool: &ToolUse, state: &mut State) -> ToolResult {
    run_browser_op(state, tool, |c, shared| {
        let value = c.snapshot_json()?;
        let (url, title) = (c.url(), c.title());
        let erefs = snapshot::parse(&value);
        let digest = format!(
            "Snapshot of {url} — \"{title}\": {} interactive elements (see Browser panel)",
            erefs.len()
        );
        if let Ok(mut s) = shared.lock() {
            s.snapshot_text = snapshot::render_erefs(&erefs);
            s.set_erefs(erefs);
            s.url = url;
            s.title = title;
            s.last_action.clone_from(&digest);
        }
        Ok(digest)
    })
}

/// Resolve the `ref`/`selector` pair from tool input (reads shared e-refs).
/// Done on the main thread before spawning the worker.
fn resolve(tool: &ToolUse, state: &State) -> Result<String, String> {
    let eref = tool.input.get("ref").and_then(|v| v.as_str());
    let selector = tool.input.get("selector").and_then(|v| v.as_str());
    if eref.is_none() && selector.is_none() {
        return Err("Provide 'ref' (from browser_snapshot) or 'selector'".to_string());
    }
    let shared = BrowserState::get(state).shared.lock().map_err(|_e| "browser state poisoned".to_string())?;
    shared
        .resolve_selector(eref, selector)
        .ok_or_else(|| format!("Unknown ref '{}' — take a fresh browser_snapshot", eref.unwrap_or("?")))
}

/// `browser_click`: click by ref or selector.
fn click(tool: &ToolUse, state: &mut State) -> ToolResult {
    let sel = match resolve(tool, state) {
        Ok(s) => s,
        Err(e) => return ToolResult::new(tool.id.clone(), e, true),
    };
    run_browser_op(state, tool, move |c, shared| {
        c.click(&sel)?;
        let (url, title) = note_nav(shared, c, &format!("click {sel}"));
        Ok(format!("Clicked '{sel}'. Now at {url} — \"{title}\""))
    })
}

/// `browser_type`: type into an element, optionally submit.
fn type_text(tool: &ToolUse, state: &mut State) -> ToolResult {
    let text = match tool.input.get("text").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return ToolResult::new(tool.id.clone(), "Missing required 'text' parameter".to_string(), true),
    };
    let submit = tool.input.get("submit").and_then(serde_json::Value::as_bool).unwrap_or(false);
    let sel = match resolve(tool, state) {
        Ok(s) => s,
        Err(e) => return ToolResult::new(tool.id.clone(), e, true),
    };
    run_browser_op(state, tool, move |c, shared| {
        let outcome = c.type_into(&sel, &text, submit)?;
        let (url, title) = note_nav(shared, c, &outcome);
        Ok(format!("{outcome}. Now at {url} — \"{title}\""))
    })
}

/// `browser_extract`: page/element content, inline or to file.
fn extract(tool: &ToolUse, state: &mut State) -> ToolResult {
    let selector = tool.input.get("selector").and_then(|v| v.as_str()).map(ToString::to_string);
    let html = tool.input.get("format").and_then(|v| v.as_str()) == Some("html");
    run_browser_op(state, tool, move |c, shared| {
        let content = c.extract(selector.as_deref(), html)?;
        note_shared(shared, "extract");
        if content.len() <= EXTRACT_INLINE_MAX {
            return Ok(content);
        }
        let path = artifact_path(if html { "html" } else { "txt" })?;
        std::fs::write(&path, &content).map_err(|e| format!("Failed to write extract: {e}"))?;
        Ok(format!(
            "Extracted {} bytes — too long for inline, written to {} (first 500 chars):\n{}",
            content.len(),
            path,
            truncate_utf8(&content, 500)
        ))
    })
}

/// `browser_screenshot`: capture PNG to disk.
fn screenshot(tool: &ToolUse, state: &mut State) -> ToolResult {
    let full_page = tool.input.get("full_page").and_then(serde_json::Value::as_bool).unwrap_or(false);
    run_browser_op(state, tool, move |c, shared| {
        let png = c.screenshot(full_page)?;
        let path = artifact_path("png")?;
        std::fs::write(&path, &png).map_err(|e| format!("Failed to write screenshot: {e}"))?;
        note_shared(shared, "screenshot");
        Ok(format!("Screenshot saved to {} ({} bytes). Use the ocr tool to extract text.", path, png.len()))
    })
}

/// `browser_eval`: run JS, return capped JSON result.
fn eval(tool: &ToolUse, state: &mut State) -> ToolResult {
    let expr = match tool.input.get("expression").and_then(|v| v.as_str()) {
        Some(e) => e.to_string(),
        None => return ToolResult::new(tool.id.clone(), "Missing required 'expression' parameter".to_string(), true),
    };
    run_browser_op(state, tool, move |c, shared| {
        let out = c.eval(&expr)?;
        note_shared(shared, "eval");
        Ok(out)
    })
}

/// `browser_close`: kill Chrome, keep the profile for next time.
/// Synchronous — process teardown is fast and main-thread-owned.
fn close(tool: &ToolUse, state: &mut State) -> ToolResult {
    if BrowserState::get(state).handle.is_none() {
        return ToolResult::new(tool.id.clone(), "No browser was running.".to_string(), false);
    }
    let bs = BrowserState::get_mut(state);
    lifecycle::kill_chrome(bs);
    crate::panel::remove_panel(state);
    ToolResult::new(
        tool.id.clone(),
        "Browser closed. Profile (cookies/logins) kept for the next browser_open.".to_string(),
        false,
    )
}

/// Timestamped artifact path under `.context-pilot/browser/`.
fn artifact_path(ext: &str) -> Result<String, String> {
    let dir = std::path::PathBuf::from(cp_base::config::constants::STORE_DIR).join("browser");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create artifact dir: {e}"))?;
    let name = format!("capture_{}.{ext}", cp_base::panels::now_ms());
    Ok(dir.join(name).to_string_lossy().to_string())
}
