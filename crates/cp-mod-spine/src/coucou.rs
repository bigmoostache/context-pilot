//! Coucou tool — scheduled notifications via the Watcher system.
//!
//! Two modes:
//! - `timer`: fire after a delay (e.g. "5m", "1h30m", "90s")
//! - `datetime`: fire at a specific time (ISO 8601)

use serde::{Deserialize, Serialize};

use cp_base::cast::SafeCast as _;
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
}

impl CoucouData {
    /// Convert into a live `CoucouWatcher` and register in `WatcherRegistry`.
    pub(crate) fn into_watcher(self) -> CoucouWatcher {
        let desc = format!("🔔 Coucou: \"{}\"", self.message);
        CoucouWatcher {
            watcher_id: self.watcher_id,
            message: self.message,
            registered_at_ms: self.registered_at_ms,
            fire_at_ms: self.fire_at_ms,
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
    // Try parsing with chrono-like manual parsing (we don't have chrono in this crate)
    // Format: YYYY-MM-DDTHH:MM:SS or YYYY-MM-DD HH:MM:SS or YYYY-MM-DDTHH:MM
    let s = s.trim().replace(' ', "T");

    let parts: Vec<&str> = s.split('T').collect();
    let [date_str, time_with_tz] = parts.as_slice() else {
        return Err("Expected format: YYYY-MM-DDTHH:MM:SS or YYYY-MM-DD HH:MM:SS".to_string());
    };

    let date_parts: Vec<&str> = date_str.split('-').collect();
    let [year_s, month_s, day_s] = date_parts.as_slice() else {
        return Err("Expected date format: YYYY-MM-DD".to_string());
    };

    let time_str = time_with_tz.trim_end_matches('Z');
    let time_parts: Vec<&str> = time_str.split(':').collect();
    let (hour_s, min_s, sec_s) = match time_parts.as_slice() {
        [hr, mn] => (*hr, *mn, "0"),
        [hr, mn, sc, ..] => (*hr, *mn, *sc),
        _ => return Err("Expected time format: HH:MM or HH:MM:SS".to_string()),
    };

    let year: i64 = year_s.parse().map_err(|_e| "Invalid year")?;
    let month: i64 = month_s.parse().map_err(|_e| "Invalid month")?;
    let day: i64 = day_s.parse().map_err(|_e| "Invalid day")?;
    let hour: i64 = hour_s.parse().map_err(|_e| "Invalid hour")?;
    let min: i64 = min_s.parse().map_err(|_e| "Invalid minute")?;
    let sec: i64 = sec_s.parse().map_err(|_e| "Invalid second")?;

    // Simple days-since-epoch calculation (good enough for scheduling)
    // Using a basic algorithm for dates after 2000
    let mut epoch_days: i64 = 0;
    for y in 1970..year {
        epoch_days = epoch_days.saturating_add(if is_leap_year(y) { 366 } else { 365 });
    }
    let month_days = [31, if is_leap_year(year) { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    for day_count in month_days.iter().take(month.saturating_sub(1).to_usize()) {
        if month > 12 {
            break;
        }
        epoch_days = epoch_days.saturating_add((*day_count).to_i64());
    }
    epoch_days = epoch_days.saturating_add(day.saturating_sub(1));

    let total_secs = epoch_days
        .saturating_mul(86400)
        .saturating_add(hour.saturating_mul(3600))
        .saturating_add(min.saturating_mul(60))
        .saturating_add(sec);
    if total_secs < 0 {
        return Err("DateTime is before epoch".to_string());
    }

    Ok(total_secs.to_u64().saturating_mul(1000))
}

/// Check whether a given year is a leap year.
#[expect(clippy::integer_division_remainder_used, reason = "calendar modulo arithmetic")]
const fn is_leap_year(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
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
        (now >= self.fire_at_ms).then(|| WatcherResult {
            description: format!("⏰ Coucou! {}", self.message),
            panel_id: None,
            tool_use_id: None,
            close_panel: false,
            create_panel: None,
            processed_already: false,
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
}

// ============================================================
// Tool execution
// ============================================================

/// Monotonic counter for generating unique coucou watcher IDs.
static COUCOU_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Execute the coucou tool — schedule a notification.
pub(crate) fn execute_coucou(tool: &ToolUse, state: &mut State) -> ToolResult {
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
    let desc = format!("🔔 Coucou {delay_desc}: \"{message}\"");

    let watcher = CoucouWatcher { watcher_id, message: message.clone(), registered_at_ms: now, fire_at_ms, desc };

    WatcherRegistry::get_mut(state).register(Box::new(watcher));

    ToolResult::new(tool.id.clone(), format!("Coucou scheduled {delay_desc}!\nMessage: \"{message}\""), false)
}
