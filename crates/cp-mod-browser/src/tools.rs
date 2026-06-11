//! Tool implementations: open, goto, snapshot, click, type, extract,
//! screenshot, eval, close.
//!
//! Inline results stay compact; heavy state goes to the Browser panel
//! (see `panel`).

use std::fmt::Write as _;

use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

use crate::client::{Client, INLINE_CAP_BYTES, truncate_utf8};
use crate::types::BrowserState;
use crate::{lifecycle, snapshot};

/// Extraction results larger than this go to a file instead of inline.
const EXTRACT_INLINE_MAX: usize = INLINE_CAP_BYTES;

/// Dispatch a browser tool call.
pub fn execute(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!(&format!("browser_{}", tool.name));
    let out = match tool.name.as_str() {
        "browser_open" => open(tool, state),
        "browser_goto" => goto(tool, state),
        "browser_snapshot" => take_snapshot(state),
        "browser_click" => click(tool, state),
        "browser_type" => type_text(tool, state),
        "browser_extract" => extract(tool, state),
        "browser_screenshot" => screenshot(tool, state),
        "browser_eval" => eval(tool, state),
        "browser_close" => Ok(close(state)),
        other => Err(format!("Unknown browser tool: {other}")),
    };
    match out {
        Ok(msg) => ToolResult::new(tool.id.clone(), msg, false),
        Err(e) => ToolResult::new(tool.id.clone(), e, true),
    }
}

/// Get a connected client, lazily reconnecting CDP after a reload.
fn client(state: &mut State) -> Result<&Client, String> {
    let bs = BrowserState::get_mut(state);
    if bs.handle.as_ref().is_none_or(|h| h.get_status().is_terminal()) {
        return Err("No browser running — call browser_open first.".to_string());
    }
    if bs.client.is_none() {
        let ws = bs.meta.as_ref().map(|m| m.ws_url.clone()).ok_or_else(|| "No browser metadata".to_string())?;
        bs.client = Some(Client::connect(&ws)?);
    }
    bs.client.as_ref().ok_or_else(|| "CDP client unavailable".to_string())
}

/// Record the last action and refresh the panel digest.
fn note(state: &mut State, action: String) {
    let bs = BrowserState::get_mut(state);
    bs.last_action = action;
    crate::panel::mark_dirty(state);
}

/// `browser_open`: launch or reuse Chrome, optionally navigate.
fn open(tool: &ToolUse, state: &mut State) -> Result<String, String> {
    let headless = tool.input.get("headless").and_then(serde_json::Value::as_bool).unwrap_or(true);
    let url = tool.input.get("url").and_then(|v| v.as_str()).map(ToString::to_string);

    let running = {
        let bs = BrowserState::get(state);
        bs.handle.as_ref().is_some_and(|h| !h.get_status().is_terminal())
    };
    if !running {
        let bs = BrowserState::get_mut(state);
        let _ws = lifecycle::spawn_chrome(bs, headless)?;
    }
    crate::panel::ensure_panel(state);

    let mut msg = if running {
        "Browser already running — reusing it.".to_string()
    } else {
        format!("Chrome launched ({}).", if headless { "headless" } else { "headed" })
    };
    if let Some(u) = url {
        let c = client(state)?;
        c.goto(&u)?;
        let _w = write!(msg, " Now at {} — \"{}\"", c.url(), c.title());
    }
    note(state, msg.clone());
    Ok(msg)
}

/// `browser_goto`: navigate and report URL + title.
fn goto(tool: &ToolUse, state: &mut State) -> Result<String, String> {
    let url = tool.input.get("url").and_then(|v| v.as_str()).ok_or("Missing required 'url' parameter")?.to_string();
    let c = client(state)?;
    c.goto(&url)?;
    let msg = format!("Now at {} — \"{}\". e-refs are stale: snapshot before clicking.", c.url(), c.title());
    note(state, msg.clone());
    Ok(msg)
}

/// `browser_snapshot`: enumerate interactive elements, digest inline.
fn take_snapshot(state: &mut State) -> Result<String, String> {
    let (value, url, title) = {
        let c = client(state)?;
        (c.snapshot_json()?, c.url(), c.title())
    };
    let erefs = snapshot::parse(&value);
    let digest = format!("Snapshot of {} — \"{}\": {} interactive elements (see Browser panel)", url, title, erefs.len());
    let bs = BrowserState::get_mut(state);
    bs.snapshot_text = snapshot::render_erefs(&erefs);
    bs.set_erefs(erefs);
    note(state, digest.clone());
    Ok(digest)
}

/// Resolve the `ref`/`selector` pair from tool input.
fn resolve(tool: &ToolUse, state: &State) -> Result<String, String> {
    let eref = tool.input.get("ref").and_then(|v| v.as_str());
    let selector = tool.input.get("selector").and_then(|v| v.as_str());
    if eref.is_none() && selector.is_none() {
        return Err("Provide 'ref' (from browser_snapshot) or 'selector'".to_string());
    }
    BrowserState::get(state)
        .resolve_selector(eref, selector)
        .ok_or_else(|| format!("Unknown ref '{}' — take a fresh browser_snapshot", eref.unwrap_or("?")))
}

/// `browser_click`: click by ref or selector.
fn click(tool: &ToolUse, state: &mut State) -> Result<String, String> {
    let sel = resolve(tool, state)?;
    let c = client(state)?;
    c.click(&sel)?;
    let msg = format!("Clicked '{}'. Now at {} — \"{}\"", sel, c.url(), c.title());
    note(state, msg.clone());
    Ok(msg)
}

/// `browser_type`: type into an element, optionally submit.
fn type_text(tool: &ToolUse, state: &mut State) -> Result<String, String> {
    let text = tool.input.get("text").and_then(|v| v.as_str()).ok_or("Missing required 'text' parameter")?.to_string();
    let submit = tool.input.get("submit").and_then(serde_json::Value::as_bool).unwrap_or(false);
    let sel = resolve(tool, state)?;
    let c = client(state)?;
    c.type_into(&sel, &text, submit)?;
    let msg = format!(
        "Typed into '{}'{}. Now at {} — \"{}\"",
        sel,
        if submit { " + Enter" } else { "" },
        c.url(),
        c.title()
    );
    note(state, msg.clone());
    Ok(msg)
}

/// `browser_extract`: page/element content, inline or to file.
fn extract(tool: &ToolUse, state: &mut State) -> Result<String, String> {
    let selector = tool.input.get("selector").and_then(|v| v.as_str()).map(ToString::to_string);
    let html = tool.input.get("format").and_then(|v| v.as_str()) == Some("html");
    let c = client(state)?;
    let content = c.extract(selector.as_deref(), html)?;
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
}

/// `browser_screenshot`: capture PNG to disk.
fn screenshot(tool: &ToolUse, state: &mut State) -> Result<String, String> {
    let full_page = tool.input.get("full_page").and_then(serde_json::Value::as_bool).unwrap_or(false);
    let c = client(state)?;
    let png = c.screenshot(full_page)?;
    let path = artifact_path("png")?;
    std::fs::write(&path, &png).map_err(|e| format!("Failed to write screenshot: {e}"))?;
    let msg = format!("Screenshot saved to {} ({} bytes). Use the ocr tool to extract text.", path, png.len());
    note(state, msg.clone());
    Ok(msg)
}

/// `browser_eval`: run JS, return capped JSON result.
fn eval(tool: &ToolUse, state: &mut State) -> Result<String, String> {
    let expr = tool
        .input
        .get("expression")
        .and_then(|v| v.as_str())
        .ok_or("Missing required 'expression' parameter")?
        .to_string();
    let c = client(state)?;
    c.eval(&expr)
}

/// `browser_close`: kill Chrome, keep the profile for next time.
fn close(state: &mut State) -> String {
    let bs = BrowserState::get_mut(state);
    if bs.handle.is_none() {
        return "No browser was running.".to_string();
    }
    lifecycle::kill_chrome(bs);
    crate::panel::remove_panel(state);
    "Browser closed. Profile (cookies/logins) kept for the next browser_open.".to_string()
}

/// Timestamped artifact path under `.context-pilot/browser/`.
fn artifact_path(ext: &str) -> Result<String, String> {
    let dir = std::path::PathBuf::from(cp_base::config::constants::STORE_DIR).join("browser");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create artifact dir: {e}"))?;
    let name = format!("capture_{}.{ext}", cp_base::panels::now_ms());
    Ok(dir.join(name).to_string_lossy().to_string())
}
