//! CDP client wrapper around `headless_chrome`: connect, navigate, act, extract.
//!
//! Channel B of the two-channel model — talks directly to Chrome's `DevTools`
//! WebSocket, bypassing the console-server daemon entirely.

use std::sync::Arc;
use std::time::Duration;

use headless_chrome::browser::tab::point::Point;
use headless_chrome::protocol::cdp::Runtime;
use headless_chrome::{Browser, Tab};

/// Default per-operation timeout (navigation, element waits).
const OP_TIMEOUT: Duration = Duration::from_secs(8);

/// Max attempts (×100 ms) to let a URL settle after a navigation/redirect.
const SETTLE_POLLS: u32 = 20;

/// Cap on inline extraction / eval results returned to the conversation.
pub const INLINE_CAP_BYTES: usize = 8_000;

/// Run a closure that calls into `headless_chrome`, turning a PANIC into an `Err`.
///
/// `headless_chrome` 1.0.21 unwraps internally on a closed transport — e.g.
/// `register_missing_tabs` does `targets.unwrap()` (browser/mod.rs:305). Such a
/// panic on Context Pilot's single main thread would abort the WHOLE process.
/// We catch it here and return a clean recoverable error so the TUI survives a
/// dead Chrome connection. `AssertUnwindSafe` is sound: on panic we discard the
/// closure's captured state and surface an error — we never observe a
/// half-mutated value.
///
/// CRITICAL: the process-global panic hook (set in `main.rs`) tears the terminal
/// down — `disable_raw_mode` + `LeaveAlternateScreen` — on EVERY panic, including
/// ones we catch. So a recovered browser panic would still corrupt the live TUI.
/// We therefore swap in a quiet, log-only hook for the duration of the guarded
/// call and restore the previous hook afterwards. Browser ops run synchronously
/// on the main thread, so this swap window can't race a concurrent main-thread
/// panic; a rare background-thread panic in the window only loses terminal
/// teardown for that one panic — an acceptable trade for never trashing the TUI
/// on the (common) recoverable browser panic.
pub(crate) fn catch_panic<T>(label: &str, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(log_panic_quietly));
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    std::panic::set_hook(prev);
    caught.unwrap_or_else(|_| {
        Err(format!("browser connection lost ({label}) — Chrome or the tab closed. Reopen with browser_open."))
    })
}

/// Append a panic to `.context-pilot/errors/panic.log` WITHOUT touching the
/// terminal — used by `catch_panic` for recoverable `headless_chrome` panics so
/// the live TUI's raw-mode/alternate-screen state is left intact.
fn log_panic_quietly(info: &std::panic::PanicHookInfo<'_>) {
    let dir = std::path::Path::new(".context-pilot").join("errors");
    let _mk = std::fs::create_dir_all(&dir);
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let msg = format!("[{ts}] (browser, recovered) {info}\n---\n");
    let _w = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("panic.log"))
        .and_then(|mut f| std::io::Write::write_all(&mut f, msg.as_bytes()));
}

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
        catch_panic("connect", || {
            let browser = Browser::connect(ws_url.to_string()).map_err(|e| format!("CDP connect failed: {e}"))?;
            let tab = adopt_initial_tab(&browser)?;
            let _t = tab.set_default_timeout(OP_TIMEOUT);
            Ok(Self { _browser: browser, tab })
        })
    }

    /// Liveness probe: a cheap CDP round-trip. Returns `false` when the
    /// underlying WebSocket transport has self-closed (idle-timeout flips it
    /// shut permanently while Chrome itself stays alive), signalling the caller
    /// to drop this dead client and reconnect to the same `ws_url`.
    #[must_use]
    pub fn is_alive(&self) -> bool {
        catch_panic("is_alive", || Ok(self.tab.evaluate("1", false).is_ok())).unwrap_or(false)
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

    /// Click the first element matching `selector` using CDP mouse events.
    ///
    /// Uses proper CDP `Input.dispatchMouseEvent` to create trusted click events
    /// that work for downloads and other security-sensitive actions (unlike
    /// JavaScript `.click()` which Gmail/banks ignore). First finds the element
    /// and gets its coordinates, then dispatches real mouse press/release events
    /// at that point. A `disabled` element is rejected. Settles the URL after
    /// clicking to report the true landing URL for click-triggered navigation.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the selector is invalid, matches no element, the
    /// element is disabled, or coordinates cannot be determined.
    pub fn click(&self, selector: &str) -> Result<(), String> {
        let before = self.tab.get_url();
        
        // Get element bounds and disabled state
        let js = format!(
            "(() => {{ const el = document.querySelector({sel}); \
             if (!el) return JSON.stringify({{error: 'notfound'}}); \
             if (el.disabled) return JSON.stringify({{error: 'disabled'}}); \
             el.scrollIntoView({{block:'center'}}); \
             const rect = el.getBoundingClientRect(); \
             return JSON.stringify({{x: rect.left + rect.width/2, y: rect.top + rect.height/2}}); }})()",
            sel = serde_json::Value::String(selector.to_string())
        );
        
        let result = self.run_js(&js)?;
        let data: serde_json::Value = serde_json::from_str(result.as_str().unwrap_or("{}"))
            .map_err(|e| format!("Failed to parse element bounds: {e}"))?;
        
        if let Some(error) = data.get("error").and_then(|v| v.as_str()) {
            return match error {
                "notfound" => Err(format!("Element '{selector}' not found")),
                "disabled" => Err(format!("Element '{selector}' is disabled — click had no effect")),
                _ => Err(format!("Click failed: {error}")),
            };
        }
        
        let x = data.get("x").and_then(serde_json::Value::as_f64).ok_or("Missing x coordinate")?;
        let y = data.get("y").and_then(serde_json::Value::as_f64).ok_or("Missing y coordinate")?;
        
        // Use CDP mouse events for trusted clicks
        let point = Point { x, y };
        let _clicked = self.tab.click_point(point).map_err(|e| format!("Click failed: {e}"))?;
        
        let _settled = self.settle_after_nav(&before);
        Ok(())
    }

    /// Type `text` into the element matching `selector`; optionally submit.
    ///
    /// Focuses via JS `.focus()` (NOT `el.click()`, which on a link/button
    /// navigates and silently loses the text), sets `.value`, and dispatches
    /// `input`+`change` so framework-controlled fields react. Honesty guards
    /// added in PR1.6: a `disabled`/`readOnly` field is rejected (JS `.value`
    /// would otherwise bypass the lock and falsely "succeed"); `contenteditable`
    /// elements (no `.value`) are filled via `textContent` instead of being
    /// wrongly refused; and after setting `.value` we read it back — if the
    /// field transformed/rejected the input (e.g. `<input type=number>` clearing
    /// `"abc"`), we report the *actual* landed value instead of a false success.
    /// `submit` calls `form.requestSubmit()` (falling back to a synthetic Enter)
    /// so the page's submit handlers run. Settles the URL afterwards for any
    /// navigation.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the selector is invalid, matches nothing, the element
    /// is disabled/read-only, or it is not a text field.
    pub fn type_into(&self, selector: &str, text: &str, submit: bool) -> Result<String, String> {
        let before = self.tab.get_url();
        let js = format!(
            "(() => {{ const el = document.querySelector({sel}); if (!el) return 'notfound'; \
             if (el.disabled) return 'disabled'; if (el.readOnly) return 'readonly'; \
             if (!('value' in el)) {{ if (el.isContentEditable) {{ el.focus(); el.textContent = {txt}; \
               el.dispatchEvent(new Event('input', {{bubbles:true}})); return 'ok'; }} return 'nottypeable'; }} \
             el.focus(); try {{ el.value = {txt}; }} catch (e) {{ return 'nottypeable'; }} \
             el.dispatchEvent(new Event('input', {{bubbles:true}})); \
             el.dispatchEvent(new Event('change', {{bubbles:true}})); \
             const actual = el.value; \
             if ({do_submit}) {{ const f = el.form || el.closest('form'); \
               if (f && f.requestSubmit) f.requestSubmit(); \
               else if (f) f.submit(); \
               else el.dispatchEvent(new KeyboardEvent('keydown', {{key:'Enter',bubbles:true}})); }} \
             return actual === {txt} ? 'ok' : ('mismatch:' + actual); }})()",
            sel = serde_json::Value::String(selector.to_string()),
            txt = serde_json::Value::String(text.to_string()),
            do_submit = if submit { "true" } else { "false" }
        );
        let outcome = self.run_js(&js)?;
        let code = outcome.as_str().unwrap_or("");
        if let Some(actual) = code.strip_prefix("mismatch:") {
            if submit {
                let _settled = self.settle_after_nav(&before);
            }
            return Ok(format!(
                "Typed into '{selector}', but the field now reads \"{actual}\" — the input was \
                 transformed or rejected by the element (e.g. a number field clearing non-numeric text)."
            ));
        }
        match code {
            "ok" => {
                if submit {
                    let _settled = self.settle_after_nav(&before);
                }
                Ok(format!("Typed into '{selector}'{}", if submit { " + submit" } else { "" }))
            }
            "notfound" => Err(format!("Element '{selector}' not found")),
            "disabled" => Err(format!("Element '{selector}' is disabled — cannot type into it")),
            "readonly" => Err(format!("Element '{selector}' is read-only — cannot type into it")),
            "nottypeable" => Err(format!("Element '{selector}' is not a text field (no value property)")),
            _ => Err(format!("Typing into '{selector}' failed")),
        }
    }

    /// Evaluate a JS expression in the page; returns the JSON-serialized value.
    ///
    /// Two-pass approach: call with `returnByValue:false` first to preserve type
    /// metadata (subtype for Date/RegExp/DOM/Error, description). Exotics and
    /// functions use their rich `description` field directly. Plain objects/arrays
    /// (where description is generic "Object"/"Array(3)") trigger a second call
    /// with `returnByValue:true` to capture actual JSON. Primitives/unserializable
    /// values (NaN/Infinity/BigInt) render from the first call's value field.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the expression throws (the real JS error).
    pub fn eval(&self, expression: &str) -> Result<String, String> {
        // Phase 1: returnByValue:false preserves type metadata
        let ret = self.evaluate_call(expression, false)?;
        if let Some(exc) = ret.exception_details {
            return Err(format_exception(&exc));
        }
        let ro = &ret.result;

        // Decide if we need a second call for actual data
        let is_exotic = ro.subtype.as_ref().is_some_and(|s| {
            !matches!(s, Runtime::RemoteObjectSubtype::Array | Runtime::RemoteObjectSubtype::Null)
        }) || matches!(ro.Type, Runtime::RemoteObjectType::Function);

        let rendered = if is_exotic {
            // Exotic: use description (Date/RegExp/function/DOM)
            render_remote_object(ro)
        } else {
            // Plain object/array (or primitive): get actual data with returnByValue:true
            match self.evaluate_call(expression, true) {
                Ok(ret2) => {
                    if let Some(exc) = ret2.exception_details {
                        return Err(format_exception(&exc));
                    }
                    render_remote_object(&ret2.result)
                }
                Err(_) => render_remote_object(ro), // Fallback to metadata if retry fails
            }
        };

        if rendered.len() > INLINE_CAP_BYTES {
            return Ok(format!("{}…(truncated)", truncate_utf8(&rendered, INLINE_CAP_BYTES)));
        }
        Ok(rendered)
    }

    /// Run one `Runtime.evaluate` call, optionally serializing the result by value.
    ///
    /// # Errors
    ///
    /// Returns `Err` carrying the raw CDP error when the call itself fails.
    fn evaluate_call(&self, expression: &str, by_value: bool) -> Result<Runtime::EvaluateReturnObject, String> {
        self.tab
            .call_method(Runtime::Evaluate {
                expression: expression.to_string(),
                return_by_value: Some(by_value),
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
            .map_err(|e| format!("Eval failed: {e}"))
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

/// Render a `Runtime.evaluate` result into a user-facing string.
///
/// `returnByValue:true` is great for plain data (objects/arrays/primitives come
/// back as real JSON) but collapses every *exotic* object — `Date`, `RegExp`,
/// DOM nodes, `Error`, `Map`/`Set`, functions — to an empty `{}` (no enumerable
/// own-properties). For those we prefer the CDP `description` (`"/ab+c/gi"`,
/// `"Thu Jan 01 1970…"`, `"function foo() {…}"`). The discriminator is cheap:
/// keep `value` only when there's no subtype, or the subtype is `Array`/`Null`
/// (whose `value` IS the real data); any other subtype — or `type:function` —
/// means the `value` is a useless `{}`, so we use `description`.
fn render_remote_object(ro: &Runtime::RemoteObject) -> String {
    let exotic_subtype = ro
        .subtype
        .as_ref()
        .is_some_and(|s| !matches!(s, Runtime::RemoteObjectSubtype::Array | Runtime::RemoteObjectSubtype::Null));
    let is_function = matches!(ro.Type, Runtime::RemoteObjectType::Function);
    if (exotic_subtype || is_function)
        && let Some(d) = ro.description.as_ref()
    {
        return d.clone();
    }
    if let Some(v) = ro.value.as_ref() {
        return serde_json::to_string(v).unwrap_or_else(|_e| v.to_string());
    }
    if let Some(u) = ro.unserializable_value.as_ref() {
        return u.clone();
    }
    ro.description.clone().unwrap_or_else(|| "undefined".to_string())
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
