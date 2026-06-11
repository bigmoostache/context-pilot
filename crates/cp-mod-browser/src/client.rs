//! CDP client wrapper around `headless_chrome`: connect, navigate, act, extract.
//!
//! Channel B of the two-channel model — talks directly to Chrome's `DevTools`
//! WebSocket, bypassing the console-server daemon entirely.

use std::sync::Arc;
use std::time::Duration;

use headless_chrome::{Browser, Tab};

/// Default per-operation timeout (navigation, element waits).
const OP_TIMEOUT: Duration = Duration::from_secs(20);

/// Cap on inline extraction / eval results returned to the conversation.
pub const INLINE_CAP_BYTES: usize = 8_000;

/// Live CDP connection to a running Chrome.
pub struct Client {
    /// Browser-level CDP connection (kept alive for the client's lifetime).
    _browser: Browser,
    /// Active tab all tools operate on.
    tab: Arc<Tab>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client").field("tab_url", &self.tab.get_url()).finish_non_exhaustive()
    }
}

impl Client {
    /// Connect to an already-running Chrome via its `DevTools` WebSocket URL.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the WebSocket handshake fails or no tab is available.
    pub fn connect(ws_url: &str) -> Result<Self, String> {
        let browser = Browser::connect(ws_url.to_string()).map_err(|e| format!("CDP connect failed: {e}"))?;
        let tab = adopt_initial_tab(&browser)?;
        let _t = tab.set_default_timeout(OP_TIMEOUT);
        Ok(Self { _browser: browser, tab })
    }

    /// Liveness probe: a cheap CDP round-trip. Returns `false` when the
    /// underlying WebSocket transport has self-closed (idle-timeout flips it
    /// shut permanently while Chrome itself stays alive), signalling the caller
    /// to drop this dead client and reconnect to the same `ws_url`.
    #[must_use]
    pub fn is_alive(&self) -> bool {
        self.tab.evaluate("1", false).is_ok()
    }

    /// Navigate to `url` and wait for the page to settle.
    ///
    /// # Errors
    ///
    /// Returns `Err` on navigation failure or timeout.
    pub fn goto(&self, url: &str) -> Result<(), String> {
        let _t = self.tab.navigate_to(url).map_err(|e| format!("Navigation failed: {e}"))?;
        let _n = self.tab.wait_until_navigated().map_err(|e| format!("Page load timed out: {e}"))?;
        Ok(())
    }

    /// Current page URL.
    #[must_use]
    pub fn url(&self) -> String {
        self.tab.get_url()
    }

    /// Current page title (empty string when unavailable).
    #[must_use]
    pub fn title(&self) -> String {
        self.tab.get_title().unwrap_or_default()
    }

    /// Click the first element matching `selector`.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the element is not found or not clickable.
    pub fn click(&self, selector: &str) -> Result<(), String> {
        let el = self.tab.wait_for_element(selector).map_err(|e| format!("Element '{selector}' not found: {e}"))?;
        let _e = el.click().map_err(|e| format!("Click on '{selector}' failed: {e}"))?;
        // A click may trigger navigation — settle if it does, ignore if not.
        let _nav = self.tab.wait_until_navigated().ok();
        Ok(())
    }

    /// Type `text` into the element matching `selector`; optionally press Enter.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the element is not found or typing fails.
    pub fn type_into(&self, selector: &str, text: &str, submit: bool) -> Result<(), String> {
        let el = self.tab.wait_for_element(selector).map_err(|e| format!("Element '{selector}' not found: {e}"))?;
        let _e = el.click().map_err(|e| format!("Focus on '{selector}' failed: {e}"))?;
        let _t = self.tab.type_str(text).map_err(|e| format!("Typing failed: {e}"))?;
        if submit {
            let _k = self.tab.press_key("Enter").map_err(|e| format!("Enter press failed: {e}"))?;
            let _nav = self.tab.wait_until_navigated().ok();
        }
        Ok(())
    }

    /// Evaluate a JS expression in the page; returns the JSON-serialized value.
    ///
    /// # Errors
    ///
    /// Returns `Err` when evaluation throws.
    pub fn eval(&self, expression: &str) -> Result<String, String> {
        // CDP's `tab.evaluate` does not set `returnByValue`, so object/array
        // results come back with `value=None` (only a className description
        // like "Object"). Stringify in-page so the result returns by value as
        // a JSON string, which we hand back verbatim.
        let wrapped = format!("JSON.stringify({expression})");
        let obj = self.tab.evaluate(&wrapped, false).map_err(|e| format!("Eval failed: {e}"))?;
        let rendered = match obj.value {
            Some(serde_json::Value::String(json)) => json,
            Some(other) => other.to_string(),
            None => obj.description.unwrap_or_else(|| "undefined".to_string()),
        };
        Ok(truncate_utf8(&rendered, INLINE_CAP_BYTES).to_string())
    }

    /// Extract page (or `selector`-scoped) content as plain text or HTML.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the underlying eval fails or nothing matches.
    pub fn extract(&self, selector: Option<&str>, html: bool) -> Result<String, String> {
        let target = selector.map_or_else(
            || "document.body".to_string(),
            |s| format!("document.querySelector({})", serde_json::Value::String(s.to_string())),
        );
        let prop = if html { "outerHTML" } else { "innerText" };
        let expr = format!("(() => {{ const n = {target}; return n ? n.{prop} : null; }})()");
        let obj = self.tab.evaluate(&expr, false).map_err(|e| format!("Extract failed: {e}"))?;
        match obj.value {
            Some(serde_json::Value::String(s)) => Ok(s),
            Some(serde_json::Value::Null) | None => Err("No element matched the selector".to_string()),
            Some(other) => Ok(other.to_string()),
        }
    }

    /// Capture a PNG screenshot (viewport, or full surface when `full_page`).
    ///
    /// # Errors
    ///
    /// Returns `Err` when capture fails.
    pub fn screenshot(&self, full_page: bool) -> Result<Vec<u8>, String> {
        use headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption::Png;
        self.tab.capture_screenshot(Png, None, None, full_page).map_err(|e| format!("Screenshot failed: {e}"))
    }

    /// Run the snapshot script and return its parsed JSON result.
    ///
    /// The in-page script returns a JSON **string** (via `JSON.stringify`) so
    /// CDP serializes it by value — `tab.evaluate` does not set
    /// `returnByValue`, so a raw array/object would come back with no value.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the script fails to evaluate or returns no value.
    pub fn snapshot_json(&self) -> Result<serde_json::Value, String> {
        let obj =
            self.tab.evaluate(crate::snapshot::SNAPSHOT_JS, false).map_err(|e| format!("Snapshot failed: {e}"))?;
        let json = match obj.value {
            Some(serde_json::Value::String(s)) => s,
            Some(other) => return Ok(other),
            None => return Err("Snapshot returned no value".to_string()),
        };
        serde_json::from_str(&json).map_err(|e| format!("Snapshot JSON parse failed: {e}"))
    }
}

/// Adopt Chrome's own initial tab rather than spawning a duplicate.
///
/// Right after `Browser::connect` the crate's tab list is briefly empty —
/// Chrome's initial "New Tab" target isn't registered yet. We poll
/// `register_missing_tabs` + `get_tabs` for it, adopting the first tab that
/// appears, and fall back to a fresh `new_tab` only if none ever shows up.
/// Without this we'd `new_tab()` a SECOND tab and drive it, leaving the
/// user's visible "New Tab" orphaned in headed mode.
fn adopt_initial_tab(browser: &Browser) -> Result<Arc<Tab>, String> {
    for _ in 0..50 {
        browser.register_missing_tabs();
        if let Ok(guard) = browser.get_tabs().lock()
            && let Some(tab) = guard.first().cloned()
        {
            return Ok(tab);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    browser.new_tab().map_err(|e| format!("Failed to acquire initial tab: {e}"))
}

/// Truncate to at most `max_bytes` without splitting a UTF-8 char.
#[must_use]
pub fn truncate_utf8(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    s.get(..end).unwrap_or("")
}
