//! Timestamp formatting and parsing.
//!
//! Replaces the `chrono` crate for the project's actual usage: UTC/local
//! timestamp formatting, RFC 3339 / ISO 8601 parsing, and epoch conversions.
//! The local timezone offset is obtained once via `date +%z` (cached).

use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

// ── Epoch helpers ────────────────────────────────────────────────────────

/// Current time as milliseconds since the Unix epoch.
#[must_use]
pub fn now_epoch_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

// ── Internal: date-time components ───────────────────────────────────────

/// Decomposed UTC date-time (private helper for compose/decompose).
#[derive(Debug, Clone, Copy)]
struct Parts {
    /// Full year.
    year: i32,
    /// Month 1–12.
    month: u8,
    /// Day 1–31.
    day: u8,
    /// Hour 0–23.
    hour: u8,
    /// Minute 0–59.
    minute: u8,
    /// Second 0–59.
    second: u8,
}

// ── UTC formatting ───────────────────────────────────────────────────────

/// Current UTC time in RFC 3339 seconds precision: `2026-06-04T09:00:00Z`.
#[must_use]
pub fn now_utc_rfc3339_secs() -> String {
    let epoch_secs = now_epoch_ms().wrapping_div(1000);
    epoch_secs_to_rfc3339_secs(epoch_secs).unwrap_or_default()
}

/// Current UTC time in compact format: `20260604T153000`.
///
/// Suitable for filenames and sortable identifiers.
#[must_use]
pub fn now_utc_compact() -> String {
    let epoch_secs = now_epoch_ms().wrapping_div(1000);
    let Some(dt) = decompose_utc(epoch_secs) else {
        return "19700101T000000".to_string();
    };
    format!("{:04}{:02}{:02}T{:02}{:02}{:02}", dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second)
}

/// Convert epoch seconds to RFC 3339 seconds precision.
///
/// Returns `None` for negative timestamps.
#[must_use]
pub fn epoch_secs_to_rfc3339_secs(epoch_secs: i64) -> Option<String> {
    let dt = decompose_utc(epoch_secs)?;
    Some(format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second))
}

/// Convert epoch milliseconds to RFC 3339 seconds precision.
///
/// Returns `None` for negative timestamps.
#[must_use]
pub fn epoch_ms_to_rfc3339(ms: i64) -> Option<String> {
    epoch_secs_to_rfc3339_secs(ms.wrapping_div(1000))
}

/// Convert epoch seconds to RFC 3339 with fractional seconds.
///
/// Returns `None` for negative timestamps.
#[must_use]
pub fn epoch_secs_to_rfc3339_frac(epoch_secs: i64, nanos: u32) -> Option<String> {
    let dt = decompose_utc(epoch_secs)?;
    if nanos == 0 {
        Some(format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second))
    } else {
        let frac = nanos.wrapping_div(1_000_000);
        Some(format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{frac:03}Z",
            dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second
        ))
    }
}

/// Convert epoch milliseconds to UTC date-only: `2026-06-04`.
///
/// Returns `None` for negative timestamps.
#[must_use]
pub fn epoch_ms_to_utc_date(ms: i64) -> Option<String> {
    let dt = decompose_utc(ms.wrapping_div(1000))?;
    Some(format!("{:04}-{:02}-{:02}", dt.year, dt.month, dt.day))
}

/// Decompose epoch seconds into date-time components as a [`Local`] struct.
///
/// Pure UTC — no timezone adjustment (`utc_offset_secs` is set to 0).
/// Returns `None` for negative epochs.
#[must_use]
pub fn decompose_epoch_secs(epoch_secs: i64) -> Option<Local> {
    let dt = decompose_utc(epoch_secs)?;
    Some(Local {
        year: dt.year,
        month: dt.month,
        day: dt.day,
        hour: dt.hour,
        minute: dt.minute,
        second: dt.second,
        utc_offset_secs: 0,
    })
}

// ── Local time formatting ────────────────────────────────────────────────

/// Decomposed local date/time with UTC offset.
#[derive(Debug, Clone, Copy)]
pub struct Local {
    /// Full year (e.g. 2026).
    pub year: i32,
    /// Month 1–12.
    pub month: u8,
    /// Day 1–31.
    pub day: u8,
    /// Hour 0–23.
    pub hour: u8,
    /// Minute 0–59.
    pub minute: u8,
    /// Second 0–59.
    pub second: u8,
    /// UTC offset in seconds (e.g. +7200 for UTC+2).
    pub utc_offset_secs: i32,
}

/// Current local date/time.
#[must_use]
pub fn now_local() -> Local {
    let epoch_secs = now_epoch_ms().wrapping_div(1000);
    let offset = local_utc_offset_secs();
    let local_secs = epoch_secs.saturating_add(i64::from(offset));
    let Some(dt) = decompose_utc(local_secs) else {
        return Local { year: 1970, month: 1, day: 1, hour: 0, minute: 0, second: 0, utc_offset_secs: 0 };
    };
    Local {
        year: dt.year,
        month: dt.month,
        day: dt.day,
        hour: dt.hour,
        minute: dt.minute,
        second: dt.second,
        utc_offset_secs: offset,
    }
}

/// Current local time as `YYYY-MM-DD HH:MM:SS`.
#[must_use]
pub fn now_local_ymd_hms() -> String {
    let dt = now_local();
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second)
}

/// Current local time as `YYYY-MM-DD_HH-MM-SS`.
#[must_use]
pub fn now_local_ymd_hms_file() -> String {
    let dt = now_local();
    format!("{:04}-{:02}-{:02}_{:02}-{:02}-{:02}", dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second)
}

/// Convert epoch milliseconds to local `HH:MM`.
///
/// Returns `None` for negative timestamps.
#[must_use]
pub fn epoch_ms_to_local_hhmm(ms: i64) -> Option<String> {
    let epoch_secs = ms.wrapping_div(1000);
    let offset = local_utc_offset_secs();
    let local_secs = epoch_secs.saturating_add(i64::from(offset));
    let dt = decompose_utc(local_secs)?;
    Some(format!("{:02}:{:02}", dt.hour, dt.minute))
}

/// Convert epoch milliseconds to local `YYYY-MM-DD HH:MM:SS`.
///
/// Returns `None` for negative timestamps.
#[must_use]
pub fn epoch_ms_to_local_ymd_hms(ms: i64) -> Option<String> {
    let epoch_secs = ms.wrapping_div(1000);
    let offset = local_utc_offset_secs();
    let local_secs = epoch_secs.saturating_add(i64::from(offset));
    let dt = decompose_utc(local_secs)?;
    Some(format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second))
}

// ── Parsing ──────────────────────────────────────────────────────────────

/// Parse an RFC 3339 datetime to epoch milliseconds.
///
/// Accepts formats like `2026-06-04T09:00:00Z`, `2026-06-04T11:00:00+02:00`.
#[must_use]
pub fn parse_rfc3339_to_epoch_ms(s: &str) -> Option<i64> {
    if s.len() < 20 {
        return None;
    }
    let (parts, offset_secs) = parse_rfc3339_parts(s)?;
    let epoch = compose_epoch_secs(&parts)?;
    let adjusted = epoch.checked_sub(i64::from(offset_secs))?;
    Some(adjusted.saturating_mul(1000))
}

/// Parse a local datetime string `YYYY-MM-DDTHH:MM:SS` to epoch
/// milliseconds, assuming local timezone.
///
/// Used by the coucou tool for scheduling.
#[must_use]
pub fn parse_local_datetime_to_epoch_ms(s: &str) -> Option<i64> {
    if s.len() < 19 {
        return None;
    }
    let parts = Parts {
        year: s.get(..4)?.parse::<i32>().ok()?,
        month: s.get(5..7)?.parse::<u8>().ok()?,
        day: s.get(8..10)?.parse::<u8>().ok()?,
        hour: s.get(11..13)?.parse::<u8>().ok()?,
        minute: s.get(14..16)?.parse::<u8>().ok()?,
        second: s.get(17..19)?.parse::<u8>().ok()?,
    };
    let epoch = compose_epoch_secs(&parts)?;
    let offset = local_utc_offset_secs();
    let adjusted = epoch.checked_sub(i64::from(offset))?;
    Some(adjusted.saturating_mul(1000))
}

// ── Internal: date decomposition ─────────────────────────────────────────

/// Decompose epoch seconds into date-time components.
///
/// Based on Howard Hinnant's `civil_from_days` algorithm.
/// Returns `None` for negative epochs.
fn decompose_utc(epoch_secs: i64) -> Option<Parts> {
    if epoch_secs < 0 {
        return None;
    }
    let day_secs = epoch_secs.checked_rem(86400)?;
    let total_days = epoch_secs.wrapping_div(86400);

    let hour = day_secs.wrapping_div(3600);
    let minute = day_secs.checked_rem(3600)?.wrapping_div(60);
    let second = day_secs.checked_rem(60)?;
    let (year, month, day) = civil_from_days(total_days);

    Some(Parts {
        year,
        month,
        day,
        hour: u8::try_from(hour).ok()?,
        minute: u8::try_from(minute).ok()?,
        second: u8::try_from(second).ok()?,
    })
}

/// Convert a day count (since Unix epoch) to (year, month, day).
///
/// All arithmetic uses `i64` to avoid truncation casts.
/// Algorithm by Howard Hinnant.
/// <http://howardhinnant.github.io/date_algorithms.html#civil_from_days>
fn civil_from_days(days: i64) -> (i32, u8, u8) {
    let zone = days.saturating_add(719_468);
    let era = (if zone >= 0 { zone } else { zone.saturating_sub(146_096) }).wrapping_div(146_097);
    let doe = zone.saturating_sub(era.saturating_mul(146_097));
    let yoe = doe
        .saturating_sub(doe.wrapping_div(1460))
        .saturating_add(doe.wrapping_div(36524))
        .saturating_sub(doe.wrapping_div(146_096))
        .wrapping_div(365);
    let year_long = yoe.saturating_add(era.saturating_mul(400));
    let doy = doe
        .saturating_sub(365_i64.saturating_mul(yoe))
        .saturating_sub(yoe.wrapping_div(4))
        .saturating_add(yoe.wrapping_div(100));
    let mp = 5_i64.saturating_mul(doy).saturating_add(2).wrapping_div(153);
    let raw_day = doy.saturating_sub(153_i64.saturating_mul(mp).saturating_add(2).wrapping_div(5)).saturating_add(1);
    let month = if mp < 10 { mp.saturating_add(3) } else { mp.saturating_sub(9) };
    let year = if month <= 2 { year_long.saturating_add(1) } else { year_long };

    (i32::try_from(year).unwrap_or(i32::MAX), u8::try_from(month).unwrap_or(1), u8::try_from(raw_day).unwrap_or(1))
}

/// Compose date-time parts into epoch seconds.
///
/// Inverse of [`decompose_utc`]. Returns `None` for invalid dates.
const fn compose_epoch_secs(dt: &Parts) -> Option<i64> {
    if dt.month < 1 || dt.month > 12 || dt.day < 1 || dt.day > 31 || dt.hour > 23 || dt.minute > 59 || dt.second > 60 {
        return None;
    }
    let days = days_from_civil(dt.year as i64, dt.month as i64, dt.day as i64);
    let Some(day_secs) = days.checked_mul(86400) else {
        return None;
    };
    let h_secs = (dt.hour as i64).saturating_mul(3600);
    let m_secs = (dt.minute as i64).saturating_mul(60);
    let s_val = dt.second as i64;
    match day_secs.checked_add(h_secs) {
        Some(v1) => match v1.checked_add(m_secs) {
            Some(v2) => v2.checked_add(s_val),
            None => None,
        },
        None => None,
    }
}

/// Convert (year, month, day) to a day count since Unix epoch.
///
/// All arithmetic uses `i64` to avoid truncation casts.
/// Algorithm by Howard Hinnant.
/// <http://howardhinnant.github.io/date_algorithms.html#days_from_civil>
const fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let yr = if month <= 2 { year.saturating_sub(1) } else { year };
    let era = (if yr >= 0 { yr } else { yr.saturating_sub(399) }).wrapping_div(400);
    let yoe = yr.saturating_sub(era.saturating_mul(400));
    let mp = if month > 2 { month.saturating_sub(3) } else { month.saturating_add(9) };
    let doy = 153_i64.saturating_mul(mp).saturating_add(2).wrapping_div(5).saturating_add(day).saturating_sub(1);
    let doe = 365_i64
        .saturating_mul(yoe)
        .saturating_add(yoe.wrapping_div(4))
        .saturating_sub(yoe.wrapping_div(100))
        .saturating_add(doy);
    era.saturating_mul(146_097).saturating_add(doe).saturating_sub(719_468)
}

// ── Internal: RFC 3339 parsing ───────────────────────────────────────────

/// Parse RFC 3339 into components + UTC offset in seconds.
fn parse_rfc3339_parts(s: &str) -> Option<(Parts, i32)> {
    let parts = Parts {
        year: s.get(..4)?.parse::<i32>().ok()?,
        month: s.get(5..7)?.parse::<u8>().ok()?,
        day: s.get(8..10)?.parse::<u8>().ok()?,
        hour: s.get(11..13)?.parse::<u8>().ok()?,
        minute: s.get(14..16)?.parse::<u8>().ok()?,
        second: s.get(17..19)?.parse::<u8>().ok()?,
    };

    let tz_part = s.get(19..)?.trim();
    let offset_secs = parse_tz_suffix(tz_part)?;
    Some((parts, offset_secs))
}

/// Parse the timezone suffix of an RFC 3339 string into offset seconds.
///
/// Handles `Z`, `+HH:MM`, `-HH:MM`, and optional fractional seconds.
fn parse_tz_suffix(tz_part: &str) -> Option<i32> {
    if tz_part.is_empty() || tz_part == "Z" || tz_part == "z" {
        return Some(0);
    }

    // Skip optional fractional seconds (e.g. ".123").
    let tz = if let Some(rest) = tz_part.strip_prefix('.') {
        let digit_end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
        rest.get(digit_end..)?
    } else {
        tz_part
    };

    if tz == "Z" || tz == "z" || tz.is_empty() {
        return Some(0);
    }

    let sign: i32 = match tz.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let tz_rest = tz.get(1..)?;
    let oh = tz_rest.get(..2)?.parse::<i32>().ok()?;
    let om = tz_rest.get(3..5)?.parse::<i32>().ok()?;
    Some(sign.saturating_mul(oh.saturating_mul(3600).saturating_add(om.saturating_mul(60))))
}

// ── Internal: local timezone ─────────────────────────────────────────────

/// Get the local UTC offset in seconds, cached on first call.
///
/// Uses `date +%z` to retrieve the system timezone offset (e.g. `+0200`).
/// Falls back to 0 (UTC) if the command fails.
fn local_utc_offset_secs() -> i32 {
    /// Cached offset value.
    static OFFSET: OnceLock<i32> = OnceLock::new();
    *OFFSET.get_or_init(|| {
        std::process::Command::new("date")
            .arg("+%z")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| parse_tz_offset(s.trim()))
            .unwrap_or(0)
    })
}

/// Parse a timezone offset string like `+0200` or `-0530` into seconds.
fn parse_tz_offset(s: &str) -> Option<i32> {
    if s.len() < 5 {
        return None;
    }
    let sign: i32 = match s.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let hours = s.get(1..3)?.parse::<i32>().ok()?;
    let mins = s.get(3..5)?.parse::<i32>().ok()?;
    Some(sign.saturating_mul(hours.saturating_mul(3600).saturating_add(mins.saturating_mul(60))))
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_roundtrip() {
        let s = epoch_secs_to_rfc3339_secs(1_767_225_600);
        assert_eq!(s.as_deref(), Some("2026-01-01T00:00:00Z"));
    }

    #[test]
    fn epoch_ms_roundtrip() {
        let ms: i64 = 1_767_225_600_000;
        let s = epoch_ms_to_rfc3339(ms);
        assert_eq!(s.as_deref(), Some("2026-01-01T00:00:00Z"));
    }

    #[test]
    fn rfc3339_parse() {
        let ms = parse_rfc3339_to_epoch_ms("2026-01-01T00:00:00Z");
        assert_eq!(ms, Some(1_767_225_600_000));
    }

    #[test]
    fn rfc3339_parse_with_offset() {
        let ms = parse_rfc3339_to_epoch_ms("2026-01-01T02:00:00+02:00");
        assert_eq!(ms, Some(1_767_225_600_000));
    }

    #[test]
    fn civil_roundtrip() {
        for days in [0_i64, 1, 365, 730, 18628, 20000] {
            let (year, month, day) = civil_from_days(days);
            let rt = days_from_civil(i64::from(year), i64::from(month), i64::from(day));
            assert_eq!(days, rt, "roundtrip failed for day {days}");
        }
    }

    #[test]
    fn tz_offset_parsing() {
        assert_eq!(parse_tz_offset("+0200"), Some(7200));
        assert_eq!(parse_tz_offset("-0530"), Some(-19800));
        assert_eq!(parse_tz_offset("+0000"), Some(0));
    }

    #[test]
    fn negative_epoch_returns_none() {
        assert!(epoch_secs_to_rfc3339_secs(-1).is_none());
        assert!(epoch_ms_to_rfc3339(-1000).is_none());
    }
}
