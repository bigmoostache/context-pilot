//! CDP client wrapper around `headless_chrome`: connect, navigate, act, extract.
//!
//! Channel B of the two-channel model — talks directly to Chrome's `DevTools`
//! WebSocket, bypassing the console-server daemon entirely.

use std::sync::Arc;
use std::time::Duration;

use headless_chrome::protocol::cdp::Runtime;
use headless_chrome::{Browser, Tab};

/// Default per-operation timeout (navigation, element waits).
const OP_TIMEOUT: Duration = Duration::from_secs(8);

/// Max attempts (×100 ms) to let a URL settle after a navigation/redirect.
const SETTLE_POLLS: u32 = 20;

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

    /// Run a JS expression with `returnByValue` and surface thrown errors.
    ///
    /// Unlike `tab.evaluate` (which hardcodes `return_by_value:false` and drops
    /// `exceptionDetails`), this returns objects/arrays/primitives serialized by
    /// value and turns a thrown JS error into a clean one-line `Err`. The
    /// `user_gesture` flag lets `.click()` / `form.submit()` count as
    /// user-initiated (some sites gate navigation on a gesture).
    ///
    /// # Errors
    ///
    /// Returns `Err` when the call fails or the script throws.
    fn run_js(&self, js: &str) -> Result<serde_json::Value, String> {
        let ret = self
            .tab
            .call_method(Runtime::Evaluate {
                expression: js.to_string(),
                return_by_value: Some(true),
                await_promise: Some(true),
                user_gesture: Some(true),
                generate_preview: Some(false),
                silent: Some(false),
                include_command_line_api: Some(false),
                object_group: None,
                context_id: None,
                throw_on_side_effect: None,
                timeout: None,
                disable_breaks: None,
                repl_mode: None,
                allow_unsafe_eval_blocked_by_csp: None,
                unique_context_id: None,
                serialization_options: None,
            })
            .map_err(|e| format!("Eval failed: {e}"))?;
        if let Some(exc) = ret.exception_details {
            return Err(format_exception(&exc));
        }
        Ok(ret.result.value.unwrap_or(serde_json::Value::Null))
    }

    /// Settle the page URL after an action that MAY navigate, reporting the
    /// true landing URL even across redirect chains.
    ///
    /// Two phases keyed on the pre-action URL (a plain "poll until stable"
    /// races: a click-triggered navigation hasn't even begun in the first
    /// 100 ms, so the URL still reads as the old page and looks "stable" —
    /// the BUG#8 stale-URL report). Phase 1 waits up to ~1.5 s for the URL to
    /// *change* (navigation begins); if it never does, the action didn't
    /// navigate and we return immediately. Phase 2 then polls until the URL
    /// stops changing, following 30x/meta redirect chains to the real landing.
    fn settle_after_nav(&self, url_before: &str) -> String {
        // Phase 1 — detect that a navigation actually started.
        let mut cur = url_before.to_string();
        let mut navigated = false;
        for _ in 0..15 {
            std::thread::sleep(Duration::from_millis(100));
            cur = self.tab.get_url();
            if cur != url_before {
                navigated = true;
                break;
            }
        }
        if !navigated {
            return cur;
        }
        // Phase 2 — navigation began; settle through any redirect chain.
        let mut last = cur;
        for _ in 0..SETTLE_POLLS {
            std::thread::sleep(Duration::from_millis(100));
            let now = self.tab.get_url();
            if now == last {
                return now;
            }
            last = now;
        }
        last
    }

    /// Navigate to `url` and wait for the page to settle.
    ///
    /// `navigate_to` surfaces real navigation failures (bad scheme, DNS). The
    /// subsequent settle is BEST-EFFORT: `wait_until_navigated` is flaky and
    /// times out even on pages that loaded fine (about:blank, heavy pages,
    /// data: URLs), so we never propagate its error — we poll the URL to a
    /// stable value instead. Fixes the false "Page load timed out".
    ///
    /// # Errors
    ///
    /// Returns `Err` only when `navigate_to` itself fails (invalid URL / DNS).
    pub fn goto(&self, url: &str) -> Result<(), String> {
        let before = self.tab.get_url();
        let _t = self.tab.navigate_to(url).map_err(|e| format!("Navigation failed: {e}"))?;
        let _settled = self.settle_after_nav(&before);
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

    /// Click the first element matching `selector` (via in-page JS).
    ///
    /// Uses `querySelector().click()` through the Runtime domain rather than
    /// `wait_for_element` (DOM domain), which times out 20 s on elements that
    /// provably exist and hangs on typos. A missing element errors instantly; a
    /// malformed selector surfaces the real `SyntaxError`. After clicking we
    /// settle the URL so click-triggered redirects report the true landing URL.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the selector is invalid or matches no element.
    pub fn click(&self, selector: &str) -> Result<(), String> {
        let before = self.tab.get_url();
        let js = format!(
            "(() => {{ const el = document.querySelector({sel}); if (!el) return false; \
             el.scrollIntoView({{block:'center'}}); el.click(); return true; }})()",
            sel = serde_json::Value::String(selector.to_string())
        );
        let found = self.run_js(&js)?;
        if found != serde_json::Value::Bool(true) {
            return Err(format!("Element '{selector}' not found"));
        }
        let _settled = self.settle_after_nav(&before);
        Ok(())
    }

    /// Type `text` into the element matching `selector`; optionally submit.
    ///
    /// Focuses via JS `.focus()` (NOT `el.click()`, which on a link/button
    /// navigates and silently loses the text), sets `.value`, and dispatches
    /// `input`+`change` so framework-controlled fields react. `submit` calls
    /// `form.requestSubmit()` (falling back to a synthetic Enter) so the page's
    /// submit handlers run. Settles the URL afterwards for any navigation.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the selector is invalid, matches nothing, or the
    /// element is not a text field.
    pub fn type_into(&self, selector: &str, text: &str, submit: bool) -> Result<(), String> {
        let before = self.tab.get_url();
        let js = format!(
            "(() => {{ const el = document.querySelector({sel}); if (!el) return 'notfound'; \
             if (!('value' in el)) return 'nottypeable'; el.focus(); \
             try {{ el.value = {txt}; }} catch (e) {{ return 'nottypeable'; }} \
             el.dispatchEvent(new Event('input', {{bubbles:true}})); \
             el.dispatchEvent(new Event('change', {{bubbles:true}})); \
             if ({do_submit}) {{ const f = el.form || el.closest('form'); \
               if (f && f.requestSubmit) f.requestSubmit(); \
               else if (f) f.submit(); \
               else el.dispatchEvent(new KeyboardEvent('keydown', {{key:'Enter',bubbles:true}})); }} \
             return 'ok'; }})()",
            sel = serde_json::Value::String(selector.to_string()),
            txt = serde_json::Value::String(text.to_string()),
            do_submit = if submit { "true" } else { "false" }
        );
        match self.run_js(&js)?.as_str() {
            Some("ok") => {
                if submit {
                    let _settled = self.settle_after_nav(&before);
                }
                Ok(())
            }
            Some("notfound") => Err(format!("Element '{selector}' not found")),
            Some("nottypeable") => {
                Err(format!("Element '{selector}' is not a text field (no value property)"))
            }
            _ => Err(format!("Typing into '{selector}' failed")),
        }
    }

    /// Evaluate a JS expression in the page; returns the JSON-serialized value.
    ///
    /// Uses `Runtime.evaluate` with `returnByValue:true` directly (not the old
    /// `JSON.stringify(<expr>)` wrap, which turned every statement — `throw`,
    /// `if`, `let`, `for` — into a misleading `SyntaxError` and silently dropped
    /// functions/`NaN`/`Infinity`). Now: objects/arrays come back by value,
    /// `NaN`/`Infinity` via `unserializableValue`, functions via their source
    /// description, and real runtime/syntax errors surface as a clean message.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the expression throws (the real JS error).
    pub fn eval(&self, expression: &str) -> Result<String, String> {
        let ret = self
            .tab
            .call_method(Runtime::Evaluate {
                expression: expression.to_string(),
                return_by_value: Some(true),
                await_promise: Some(true),
                user_gesture: Some(true),
                generate_preview: Some(false),
                silent: Some(false),
                include_command_line_api: Some(false),
                object_group: None,
                context_id: None,
                throw_on_side_effect: None,
                timeout: None,
                disable_breaks: None,
                repl_mode: None,
                allow_unsafe_eval_blocked_by_csp: None,
                unique_context_id: None,
                serialization_options: None,
            })
            .map_err(|e| format!("Eval failed: {e}"))?;
        if let Some(exc) = ret.exception_details {
            return Err(format_exception(&exc));
        }
        let ro = ret.result;
        let rendered = if let Some(v) = ro.value {
            serde_json::to_string(&v).unwrap_or_else(|_e| v.to_string())
        } else if let Some(u) = ro.unserializable_value {
            u
        } else if let Some(d) = ro.description {
            d
        } else {
            "undefined".to_string()
        };
        if rendered.len() > INLINE_CAP_BYTES {
            return Ok(format!("{}…(truncated)", truncate_utf8(&rendered, INLINE_CAP_BYTES)));
        }
        Ok(rendered)
    }

    /// Extract page (or `selector`-scoped) content as plain text or HTML.
    ///
    /// Text mode reads `.value` for form fields (input/textarea/select) and
    /// `innerText` otherwise — so extracting an `<input>` no longer returns a
    /// silent empty string. HTML mode returns `outerHTML`. A malformed selector
    /// surfaces the real `SyntaxError`; a matched-but-empty element returns an
    /// explicit note instead of a confusing blank.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the selector is invalid or matches nothing.
    pub fn extract(&self, selector: Option<&str>, html: bool) -> Result<String, String> {
        let target = selector.map_or_else(
            || "document.body".to_string(),
            |s| format!("document.querySelector({})", serde_json::Value::String(s.to_string())),
        );
        let getter = if html {
            "n.outerHTML".to_string()
        } else {
            "(('value' in n) ? n.value : (n.innerText ?? n.textContent ?? ''))".to_string()
        };
        let expr = format!("(() => {{ const n = {target}; return n === null ? null : {getter}; }})()");
        match self.run_js(&expr)? {
            serde_json::Value::String(s) if s.is_empty() => {
                Ok("(element matched but has no text content)".to_string())
            }
            serde_json::Value::String(s) => Ok(s),
            serde_json::Value::Null => Err("No element matched the selector".to_string()),
            other @ (serde_json::Value::Bool(_)
            | serde_json::Value::Number(_)
            | serde_json::Value::Array(_)
            | serde_json::Value::Object(_)) => Ok(other.to_string()),
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

/// Render a CDP `ExceptionDetails` into a clean one-line error message.
///
/// Prefers the thrown value's `description` (e.g. "Error: boom\n  at …"),
/// trimmed to its first line, and falls back to the generic `text` field
/// (e.g. "Uncaught"). Keeps thrown JS errors readable instead of dumping a
/// multi-line stack inline.
fn format_exception(exc: &Runtime::ExceptionDetails) -> String {
    exc.exception
        .as_ref()
        .and_then(|e| e.description.as_ref())
        .map_or_else(|| exc.text.clone(), |d| d.lines().next().unwrap_or(d).to_string())
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
