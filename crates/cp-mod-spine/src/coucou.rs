//! Coucou tool — scheduled notifications held in the fleet-shared
//! [`CoucouRegistry`](crate::schedule::CoucouRegistry).
//!
//! Two modes:
//! - `timer`: fire after a delay (e.g. "5m", "1h30m", "90s")
//! - `datetime`: fire at a specific time (ISO 8601)

use serde::{Deserialize, Serialize};

use cp_base::panels::now_ms;
use cp_base::state::runtime::State;

use crate::schedule::CoucouRegistry;
use cp_base::tools::{ToolResult, ToolUse};

// ============================================================
// Persistable coucou data — saved in worker JSON via SpineState
// ============================================================

/// Serializable coucou record, held in the fleet-shared [`CoucouRegistry`].
/// Also the shape of the legacy per-thread `pending_coucous` array, migrated
/// into the registry on boot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    /// Unique coucou id (`coucou_<n>`); field name kept for legacy compat.
    pub watcher_id: String,
    /// The user's reminder message.
    pub message: String,
    /// When this coucou was registered (ms since epoch).
    pub registered_at_ms: u64,
    /// When the notification should fire (ms since epoch).
    pub fire_at_ms: u64,
    /// Target thread. Stamped with the scheduling thread when the tool call
    /// omits `thread_id`; `None` only for coucous scheduled outside any thread.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    /// Repeat interval in milliseconds. 0 = one-shot (no recurrence).
    #[serde(default)]
    pub interval_ms: u64,
    /// Human-readable recurrence label (e.g. "hourly", "daily", "every 30m").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recurrence_label: Option<String>,
}

/// Parse a human-friendly duration string into milliseconds.
/// Supports: "30s", "5m", "1h", "1h30m", "2h15m30s", "90s", "120"
fn parse_duration_ms(s: &str) -> Result<u64, String> {
    let trimmed = s.trim();

    // Pure numeric → treat as seconds
    if let Ok(secs) = trimmed.parse::<u64>() {
        if secs == 0 {
            return Err("Duration must be greater than 0".to_owned());
        }
        return Ok(secs.saturating_mul(1000));
    }

    let mut total_ms: u64 = 0;
    let mut current_num = String::new();

    for ch in trimmed.chars() {
        if ch.is_ascii_digit() {
            current_num.push(ch);
        } else {
            let val: u64 = current_num.parse().map_err(|_e| format!("Invalid number in duration: '{trimmed}'"))?;
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
        let val: u64 = current_num.parse().map_err(|_e| format!("Invalid number in duration: '{trimmed}'"))?;
        total_ms = total_ms.saturating_add(val.saturating_mul(1_000));
    }

    if total_ms == 0 {
        return Err("Duration must be greater than 0".to_owned());
    }

    Ok(total_ms)
}

/// Parse an ISO 8601 datetime string into milliseconds since epoch.
/// Supports: "2026-02-20T08:00:00", "2026-02-20 08:00:00", "2026-02-20T08:00"
fn parse_datetime_ms(s: &str) -> Result<u64, String> {
    let joined = s.trim().replace(' ', "T");

    // Pad missing seconds: "2026-02-20T08:00" → "2026-02-20T08:00:00"
    let padded = if joined.matches(':').count() == 1 { format!("{joined}:00") } else { joined };
    // Strip trailing Z if present
    let normalized = padded.trim_end_matches('Z');

    // Treat as UTC by appending 'Z', matching the original behavior
    let rfc3339 = format!("{normalized}Z");
    let ms = cp_mod_utilities::time::parse_rfc3339_to_epoch_ms(&rfc3339)
        .ok_or_else(|| format!("Invalid datetime: '{normalized}'. Expected format: YYYY-MM-DDTHH:MM:SS"))?;

    u64::try_from(ms).map_err(|_e| "DateTime is before epoch".to_owned())
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
// Tool execution
// ============================================================

/// Minimum recurrence interval to prevent notification spam (60 seconds).
const MIN_RECURRENCE_MS: u64 = 60_000;

/// Parsed recurrence: interval in ms (0 = one-shot) + human-readable label.
type Recurrence = (u64, Option<String>);

/// Resolved schedule: absolute fire time (ms since epoch) + delay description.
type FireTime = (u64, String);

/// Boxed error result — keeps the fallible parse signatures under the
/// `type_complexity` threshold.
type CoucouErr = Box<ToolResult>;

/// Parse the `recurrence` (+ `interval` for custom) params into an interval in
/// ms (0 = one-shot) and a human-readable label. Returns the error `ToolResult`
/// on an unknown recurrence or an invalid/too-short custom interval.
fn parse_recurrence(tool: &ToolUse) -> Result<Recurrence, CoucouErr> {
    let recurrence_str = tool.input.get("recurrence").and_then(|v| v.as_str()).unwrap_or("once");
    match recurrence_str {
        "once" => Ok((0, None)),
        "hourly" => Ok((3_600_000, Some("hourly".to_owned()))),
        "daily" => Ok((86_400_000, Some("daily".to_owned()))),
        "weekly" => Ok((604_800_000, Some("weekly".to_owned()))),
        "custom" => {
            let Some(interval_str) = tool.input.get("interval").and_then(|v| v.as_str()) else {
                return Err(Box::new(ToolResult::new(
                    tool.id.clone(),
                    "Missing 'interval' parameter for custom recurrence. Examples: '30m', '2h', '1d'".to_owned(),
                    true,
                )));
            };
            match parse_duration_ms(interval_str) {
                Ok(ms) if ms < MIN_RECURRENCE_MS => Err(Box::new(ToolResult::new(
                    tool.id.clone(),
                    format!(
                        "Recurrence interval '{interval_str}' is too short. Minimum is 60s to prevent notification spam."
                    ),
                    true,
                ))),
                Ok(ms) => Ok((ms, Some(format!("every {}", format_duration(ms))))),
                Err(e) => Err(Box::new(ToolResult::new(
                    tool.id.clone(),
                    format!("Invalid interval '{interval_str}': {e}"),
                    true,
                ))),
            }
        }
        _ => Err(Box::new(ToolResult::new(
            tool.id.clone(),
            format!("Unknown recurrence '{recurrence_str}'. Use 'once', 'hourly', 'daily', 'weekly', or 'custom'."),
            true,
        ))),
    }
}

/// Resolve the absolute fire time (ms since epoch) and a human-readable delay
/// description for the given `mode` ("timer" or "datetime"). Returns the error
/// `ToolResult` on a missing/invalid delay or datetime, or a past datetime.
fn resolve_fire_time(tool: &ToolUse, mode: &str, now: u64) -> Result<FireTime, CoucouErr> {
    match mode {
        "timer" => {
            let Some(delay_str) = tool.input.get("delay").and_then(|v| v.as_str()) else {
                return Err(Box::new(ToolResult::new(
                    tool.id.clone(),
                    "Missing 'delay' parameter for timer mode. Examples: '30s', '5m', '1h30m'".to_owned(),
                    true,
                )));
            };
            match parse_duration_ms(delay_str) {
                Ok(delay_ms) => Ok((now.saturating_add(delay_ms), format!("in {}", format_duration(delay_ms)))),
                Err(e) => {
                    Err(Box::new(ToolResult::new(tool.id.clone(), format!("Invalid delay '{delay_str}': {e}"), true)))
                }
            }
        }
        "datetime" => {
            let Some(dt_str) = tool.input.get("datetime").and_then(|v| v.as_str()) else {
                return Err(Box::new(ToolResult::new(
                    tool.id.clone(),
                    "Missing 'datetime' parameter. Format: YYYY-MM-DDTHH:MM:SS".to_owned(),
                    true,
                )));
            };
            match parse_datetime_ms(dt_str) {
                Ok(target_ms) if target_ms <= now => Err(Box::new(ToolResult::new(
                    tool.id.clone(),
                    format!("DateTime '{dt_str}' is in the past!"),
                    true,
                ))),
                Ok(target_ms) => {
                    let remaining = format_duration(target_ms.saturating_sub(now));
                    Ok((target_ms, format!("at {dt_str} ({remaining})")))
                }
                Err(e) => {
                    Err(Box::new(ToolResult::new(tool.id.clone(), format!("Invalid datetime '{dt_str}': {e}"), true)))
                }
            }
        }
        _ => Err(Box::new(ToolResult::new(
            tool.id.clone(),
            format!("Unknown mode '{mode}'. Use 'timer' or 'datetime'."),
            true,
        ))),
    }
}

/// Execute the coucou tool — schedule a notification or cancel an existing one.
pub(crate) fn execute_coucou(tool: &ToolUse, state: &mut State) -> ToolResult {
    // === Cancel path ===
    if let Some(cancel_id) = tool.input.get("cancel_id").and_then(|v| v.as_str()) {
        let removed = CoucouRegistry::get_mut(state).cancel(cancel_id);
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
            "Missing required 'mode' parameter. Use 'timer' or 'datetime'.".to_owned(),
            true,
        );
    };

    let message = match tool.input.get("message").and_then(|v| v.as_str()) {
        Some(m) => m.to_owned(),
        None => {
            return ToolResult::new(tool.id.clone(), "Missing required 'message' parameter.".to_owned(), true);
        }
    };

    // Unscoped calls target the calling thread, so a background thread's
    // reminder lands back in its own inbox, not whichever thread is focused.
    let thread_id = tool
        .input
        .get("thread_id")
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(|| state.resident_thread_id.clone());

    let (interval_ms, recurrence_label) = match parse_recurrence(tool) {
        Ok(r) => r,
        Err(e) => return *e,
    };

    let now = now_ms();
    let (fire_at_ms, delay_desc) = match resolve_fire_time(tool, mode, now) {
        Ok(r) => r,
        Err(e) => return *e,
    };

    let registry = CoucouRegistry::get_mut(state);
    let watcher_id = registry.alloc_id();
    let recurrence_suffix = recurrence_label.as_deref().map_or(String::new(), |r| format!(" [{r}]"));
    registry.pending.push(Record {
        watcher_id: watcher_id.clone(),
        message: message.clone(),
        registered_at_ms: now,
        fire_at_ms,
        thread_id,
        interval_ms,
        recurrence_label,
    });

    let recurrence_info =
        if interval_ms > 0 { format!("\nRecurrence: {}", recurrence_suffix.trim()) } else { String::new() };

    ToolResult::new(
        tool.id.clone(),
        format!("Coucou scheduled {delay_desc}!\nMessage: \"{message}\"\nID: {watcher_id}{recurrence_info}"),
        false,
    )
}
