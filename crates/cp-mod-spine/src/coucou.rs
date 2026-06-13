//! Coucou tool — scheduled notifications via the Watcher system.
//!
//! Two modes:
//! - `timer`: fire after a delay (e.g. "5m", "1h30m", "90s")
//! - `datetime`: fire at a specific time (ISO 8601)

use serde::{Deserialize, Serialize};

use cp_base::panels::now_ms;
use cp_base::state::runtime::State;
use cp_base::state::watchers::{Watcher, WatcherRegistry, WatcherResult};
use cp_base::tools::{ToolResult, ToolUse};

// ============================================================
// Persistable coucou data — saved in worker JSON via SpineState
// ============================================================

/// Serializable coucou record. Stored in `SpineState.pending_coucous`
/// and re-registered into `WatcherRegistry` on load.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CoucouData {
    /// Unique watcher ID for registry lookup.
    pub watcher_id: String,
    /// The user's reminder message.
    pub message: String,
    /// When this coucou was registered (ms since epoch).
    pub registered_at_ms: u64,
    /// When the notification should fire (ms since epoch).
    pub fire_at_ms: u64,
    /// Optional thread ID — scopes the notification to a thread.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
}

impl CoucouData {
    /// Convert into a live `CoucouWatcher` and register in `WatcherRegistry`.
    pub(crate) fn into_watcher(self) -> CoucouWatcher {
        let desc = if let Some(tid) = &self.thread_id {
            format!("🔔 Coucou (thread {tid}): \"{}\"", self.message)
        } else {
            format!("🔔 Coucou: \"{}\"", self.message)
        };
        CoucouWatcher {
            watcher_id: self.watcher_id,
            message: self.message,
            registered_at_ms: self.registered_at_ms,
            fire_at_ms: self.fire_at_ms,
            thread_id: self.thread_id,
            desc,
        }
    }
}

/// Collect all active `CoucouWatcher` data from the `WatcherRegistry`
/// for persistence. Filters by `source_tag` == "coucou".
pub(crate) fn collect_pending_coucous(state: &State) -> Vec<CoucouData> {
    let registry = WatcherRegistry::get(state);
    registry
        .active_watchers()
        .iter()
        .filter(|w| w.source_tag() == "coucou")
        .filter_map(|w| {
            Some(CoucouData {
                watcher_id: w.id().to_string(),
                message: w.message()?.to_string(),
                registered_at_ms: w.registered_ms(),
                fire_at_ms: w.fire_at_ms()?,
                thread_id: w.thread_id().map(str::to_string),
            })
        })
        .collect()
}

/// Parse a human-friendly duration string into milliseconds.
/// Supports: "30s", "5m", "1h", "1h30m", "2h15m30s", "90s", "120"
fn parse_duration_ms(s: &str) -> Result<u64, String> {
    let s = s.trim();

    // Pure numeric → treat as seconds
    if let Ok(secs) = s.parse::<u64>() {
        if secs == 0 {
            return Err("Duration must be greater than 0".to_string());
        }
        return Ok(secs.saturating_mul(1000));
    }

    let mut total_ms: u64 = 0;
    let mut current_num = String::new();

    for ch in s.chars() {
        if ch.is_ascii_digit() {
            current_num.push(ch);
        } else {
            let val: u64 = current_num.parse().map_err(|_e| format!("Invalid number in duration: '{s}'"))?;
            current_num.clear();
            match ch {
                'h' | 'H' => total_ms = total_ms.saturating_add(val.saturating_mul(3_600_000)),
                'm' | 'M' => total_ms = total_ms.saturating_add(val.saturating_mul(60_000)),
                's' | 'S' => total_ms = total_ms.saturating_add(val.saturating_mul(1_000)),
                _ => return Err(format!("Unknown duration unit '{ch}'. Use h/m/s.")),
            }
        }
    }

    // Trailing number without unit → seconds
    if !current_num.is_empty() {
        let val: u64 = current_num.parse().map_err(|_e| format!("Invalid number in duration: '{s}'"))?;
        total_ms = total_ms.saturating_add(val.saturating_mul(1_000));
    }

    if total_ms == 0 {
        return Err("Duration must be greater than 0".to_string());
    }

    Ok(total_ms)
}

/// Parse an ISO 8601 datetime string into milliseconds since epoch.
/// Supports: "2026-02-20T08:00:00", "2026-02-20 08:00:00", "2026-02-20T08:00"
fn parse_datetime_ms(s: &str) -> Result<u64, String> {
    let s = s.trim().replace(' ', "T");

    // Pad missing seconds: "2026-02-20T08:00" → "2026-02-20T08:00:00"
    let normalized = if s.matches(':').count() == 1 { format!("{s}:00") } else { s };
    // Strip trailing Z if present
    let normalized = normalized.trim_end_matches('Z');

    // Treat as UTC by appending 'Z', matching the original behavior
    let rfc3339 = format!("{normalized}Z");
    let ms = cp_mod_utilities::time::parse_rfc3339_to_epoch_ms(&rfc3339)
        .ok_or_else(|| format!("Invalid datetime: '{normalized}'. Expected format: YYYY-MM-DDTHH:MM:SS"))?;

    u64::try_from(ms).map_err(|_e| "DateTime is before epoch".to_string())
}

/// Format milliseconds as a human-friendly duration string.
fn format_duration(ms: u64) -> String {
    let total_secs = cp_base::panels::time_arith::ms_to_secs(ms);
    let (hours, minutes, secs) = cp_base::panels::time_arith::secs_to_hms_unwrapped(total_secs);

    if hours > 0 && minutes > 0 && secs > 0 {
        format!("{hours}h{minutes}m{secs}s")
    } else if hours > 0 && minutes > 0 {
        format!("{hours}h{minutes}m")
    } else if hours > 0 {
        format!("{hours}h")
    } else if minutes > 0 && secs > 0 {
        format!("{minutes}m{secs}s")
    } else if minutes > 0 {
        format!("{minutes}m")
    } else {
        format!("{secs}s")
    }
}

// ============================================================
// CoucouWatcher — implements cp_base::state::watchers::Watcher trait
// ============================================================

/// A watcher that fires a notification at a specific time.
pub(crate) struct CoucouWatcher {
    /// Unique watcher ID.
    pub watcher_id: String,
    /// The user's message to deliver.
    pub message: String,
    /// When this watcher was registered (ms since epoch).
    pub registered_at_ms: u64,
    /// When the notification should fire (ms since epoch).
    pub fire_at_ms: u64,
    /// Optional thread ID — scopes the notification to a thread.
    pub thread_id: Option<String>,
    /// Human-readable description.
    pub desc: String,
}

impl Watcher for CoucouWatcher {
    fn is_easy_bash(&self) -> bool {
        false
    }

    fn is_persistent(&self) -> bool {
        false
    }

    fn suicide(&self, _state: &State) -> bool {
        false
    }

    fn id(&self) -> &str {
        &self.watcher_id
    }

    fn description(&self) -> &str {
        &self.desc
    }

    fn is_blocking(&self) -> bool {
        false // Coucou is always async — fires a spine notification
    }

    fn tool_use_id(&self) -> Option<&str> {
        None
    }

    fn check(&self, _state: &State) -> Option<WatcherResult> {
        let now = now_ms();
        (now >= self.fire_at_ms).then(|| {
            let desc = self.thread_id.as_ref().map_or_else(
                || format!("⏰ Coucou! {}", self.message),
                |tid| format!("⏰ Coucou (thread {tid})! {}", self.message),
            );
            WatcherResult {
                description: desc,
                panel_id: None,
                tool_use_id: None,
                close_panel: false,
                create_panel: None,
                create_dyn_panel: None,
                processed_already: false,
                kill_session: None,
                preserves_tempo: false,
            }
        })
    }

    fn check_timeout(&self) -> Option<WatcherResult> {
        // No timeout — the check itself handles the time condition
        None
    }

    fn registered_ms(&self) -> u64 {
        self.registered_at_ms
    }

    fn source_tag(&self) -> &'static str {
        "coucou"
    }

    fn fire_at_ms(&self) -> Option<u64> {
        Some(self.fire_at_ms)
    }

    fn message(&self) -> Option<&str> {
        Some(&self.message)
    }

    fn thread_id(&self) -> Option<&str> {
        self.thread_id.as_deref()
    }
}

// ============================================================
// Tool execution
// ============================================================

/// Monotonic counter for generating unique coucou watcher IDs.
static COUCOU_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Execute the coucou tool — schedule a notification or cancel an existing one.
pub(crate) fn execute_coucou(tool: &ToolUse, state: &mut State) -> ToolResult {
    // === Cancel path ===
    if let Some(cancel_id) = tool.input.get("cancel_id").and_then(|v| v.as_str()) {
        let removed = WatcherRegistry::get_mut(state).remove_by_id(cancel_id);
        return if removed {
            ToolResult::new(tool.id.clone(), format!("Cancelled coucou '{cancel_id}'"), false)
        } else {
            ToolResult::new(tool.id.clone(), format!("Coucou '{cancel_id}' not found"), true)
        };
    }

    // === Schedule path ===
    let Some(mode) = tool.input.get("mode").and_then(|v| v.as_str()) else {
        return ToolResult::new(
            tool.id.clone(),
            "Missing required 'mode' parameter. Use 'timer' or 'datetime'.".to_string(),
            true,
        );
    };

    let message = match tool.input.get("message").and_then(|v| v.as_str()) {
        Some(m) => m.to_string(),
        None => {
            return ToolResult::new(tool.id.clone(), "Missing required 'message' parameter.".to_string(), true);
        }
    };

    let thread_id = tool.input.get("thread_id").and_then(|v| v.as_str()).map(String::from);

    let now = now_ms();
    let fire_at_ms: u64;
    let delay_desc: String;

    match mode {
        "timer" => {
            let Some(delay_str) = tool.input.get("delay").and_then(|v| v.as_str()) else {
                return ToolResult::new(
                    tool.id.clone(),
                    "Missing 'delay' parameter for timer mode. Examples: '30s', '5m', '1h30m'".to_string(),
                    true,
                );
            };

            match parse_duration_ms(delay_str) {
                Ok(delay_ms) => {
                    fire_at_ms = now.saturating_add(delay_ms);
                    delay_desc = format!("in {}", format_duration(delay_ms));
                }
                Err(e) => {
                    return ToolResult::new(tool.id.clone(), format!("Invalid delay '{delay_str}': {e}"), true);
                }
            }
        }
        "datetime" => {
            let Some(dt_str) = tool.input.get("datetime").and_then(|v| v.as_str()) else {
                return ToolResult::new(
                    tool.id.clone(),
                    "Missing 'datetime' parameter. Format: YYYY-MM-DDTHH:MM:SS".to_string(),
                    true,
                );
            };

            match parse_datetime_ms(dt_str) {
                Ok(target_ms) => {
                    if target_ms <= now {
                        return ToolResult::new(tool.id.clone(), format!("DateTime '{dt_str}' is in the past!"), true);
                    }
                    fire_at_ms = target_ms;
                    let remaining = format_duration(target_ms.saturating_sub(now));
                    delay_desc = format!("at {dt_str} ({remaining})");
                }
                Err(e) => {
                    return ToolResult::new(tool.id.clone(), format!("Invalid datetime '{dt_str}': {e}"), true);
                }
            }
        }
        _ => {
            return ToolResult::new(
                tool.id.clone(),
                format!("Unknown mode '{mode}'. Use 'timer' or 'datetime'."),
                true,
            );
        }
    }

    let counter = COUCOU_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let watcher_id = format!("coucou_{counter}");
    let desc = thread_id.as_ref().map_or_else(
        || format!("🔔 Coucou {delay_desc}: \"{message}\""),
        |tid| format!("🔔 Coucou {delay_desc} (thread {tid}): \"{message}\""),
    );

    let watcher = CoucouWatcher {
        watcher_id: watcher_id.clone(),
        message: message.clone(),
        registered_at_ms: now,
        fire_at_ms,
        thread_id,
        desc,
    };

    WatcherRegistry::get_mut(state).register(Box::new(watcher));

    ToolResult::new(
        tool.id.clone(),
        format!("Coucou scheduled {delay_desc}!\nMessage: \"{message}\"\nID: {watcher_id}"),
        false,
    )
}
