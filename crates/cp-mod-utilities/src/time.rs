//! Timestamp formatting and parsing.
//!
//! Replaces the `chrono` crate for the project's actual usage: UTC/local
//! timestamp formatting, strict RFC 3339 parsing, and epoch conversions.
//! Local time uses the host zone's offset AT the instant being converted
//! ([`crate::tz`]), so results stay right across DST transitions.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::calendar::{self, DAY_SECS};
use crate::tz::{self, Zone};

// ── Epoch helpers ────────────────────────────────────────────────────────

/// Current time as milliseconds since the Unix epoch.
#[must_use]
pub fn now_epoch_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// Current time as whole seconds since the Unix epoch.
fn now_secs() -> i64 {
    now_epoch_ms().div_euclid(1000)
}

// ── Internal: date-time components ───────────────────────────────────────

/// Decomposed date-time (private helper for compose/decompose).
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

/// 1970-01-01T00:00:00, the fallback when the clock is unreadable.
const EPOCH: Parts = Parts { year: 1970, month: 1, day: 1, hour: 0, minute: 0, second: 0 };

/// Split non-negative epoch seconds into calendar fields; `None` if negative.
fn decompose_utc(epoch_secs: i64) -> Option<Parts> {
    if epoch_secs < 0 {
        return None;
    }
    let secs_of_day = epoch_secs.rem_euclid(DAY_SECS);
    let (year, month, day) = calendar::civil_from_days(epoch_secs.div_euclid(DAY_SECS));
    Some(Parts {
        year: i32::try_from(year).ok()?,
        month: u8::try_from(month).ok()?,
        day: u8::try_from(day).ok()?,
        hour: u8::try_from(secs_of_day.div_euclid(3600)).ok()?,
        minute: u8::try_from(secs_of_day.rem_euclid(3600).div_euclid(60)).ok()?,
        second: u8::try_from(secs_of_day.rem_euclid(60)).ok()?,
    })
}

/// Whether every field names a real calendar instant (no Feb 31, no second 60).
fn is_valid(dt: &Parts) -> bool {
    let month = i64::from(dt.month);
    let date_ok = (1..=12).contains(&month)
        && dt.day >= 1
        && i64::from(dt.day) <= calendar::days_in_month(i64::from(dt.year), month);
    date_ok && dt.hour <= 23 && dt.minute <= 59 && dt.second <= 59
}

/// Epoch seconds of `dt` read as UTC; `None` for an invalid date or time.
fn compose_epoch_secs(dt: &Parts) -> Option<i64> {
    if !is_valid(dt) {
        return None;
    }
    let days = calendar::days_from_civil(i64::from(dt.year), i64::from(dt.month), i64::from(dt.day));
    days.checked_mul(DAY_SECS)?
        .checked_add(i64::from(dt.hour).checked_mul(3600)?)?
        .checked_add(i64::from(dt.minute).checked_mul(60)?)?
        .checked_add(i64::from(dt.second))
}

// ── UTC formatting ───────────────────────────────────────────────────────

/// Current UTC time in RFC 3339 seconds precision: `2026-06-04T09:00:00Z`.
#[must_use]
pub fn now_utc_rfc3339_secs() -> String {
    epoch_secs_to_rfc3339_secs(now_secs()).unwrap_or_default()
}

/// Current UTC time in compact format: `20260604T153000`.
///
/// Suitable for filenames and sortable identifiers.
#[must_use]
pub fn now_utc_compact() -> String {
    let dt = decompose_utc(now_secs()).unwrap_or(EPOCH);
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
    epoch_secs_to_rfc3339_secs(ms.div_euclid(1000))
}

/// Convert epoch seconds to RFC 3339 with millisecond precision (omitted when
/// `nanos` is 0).
///
/// Returns `None` for negative timestamps.
#[must_use]
pub fn epoch_secs_to_rfc3339_frac(epoch_secs: i64, nanos: u32) -> Option<String> {
    let dt = decompose_utc(epoch_secs)?;
    let base = format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second);
    if nanos == 0 { Some(format!("{base}Z")) } else { Some(format!("{base}.{:03}Z", nanos.div_euclid(1_000_000))) }
}

/// Convert epoch milliseconds to UTC date-only: `2026-06-04`.
///
/// Returns `None` for negative timestamps.
#[must_use]
pub fn epoch_ms_to_utc_date(ms: i64) -> Option<String> {
    let dt = decompose_utc(ms.div_euclid(1000))?;
    Some(format!("{:04}-{:02}-{:02}", dt.year, dt.month, dt.day))
}

/// Decompose epoch seconds into date-time components as a [`Local`] struct.
///
/// Pure UTC — no timezone adjustment (`utc_offset_secs` is set to 0).
/// Returns `None` for negative epochs.
#[must_use]
pub fn decompose_epoch_secs(epoch_secs: i64) -> Option<Local> {
    Some(Local::from_parts(decompose_utc(epoch_secs)?, 0))
}

// ── Local time ───────────────────────────────────────────────────────────

/// Decomposed local date/time with UTC offset.
///
/// Built only in-crate; callers read fields, never construct, so a new field
/// is not a breaking change.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
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
    /// Offset east of UTC in force at this instant, seconds.
    pub utc_offset_secs: i32,
}

impl Local {
    /// Attach an offset to decomposed fields.
    const fn from_parts(dt: Parts, utc_offset_secs: i32) -> Self {
        Self {
            year: dt.year,
            month: dt.month,
            day: dt.day,
            hour: dt.hour,
            minute: dt.minute,
            second: dt.second,
            utc_offset_secs,
        }
    }
}

/// Offset east of UTC (seconds) of the host zone at `epoch_secs`, DST-aware.
///
/// Use this, not [`local_utc_offset_secs`], when converting an instant other
/// than "now" (schedules, timestamps from the past or future).
#[must_use]
pub fn utc_offset_at_secs(epoch_secs: i64) -> i32 {
    tz::host_zone().offset_at(epoch_secs)
}

/// Offset east of UTC (seconds) of the host zone right now.
#[must_use]
pub fn local_utc_offset_secs() -> i32 {
    utc_offset_at_secs(now_secs())
}

/// Local wall-clock fields of `epoch_secs` in `zone`.
fn local_in(zone: &Zone, epoch_secs: i64) -> Option<Local> {
    let offset = zone.offset_at(epoch_secs);
    let dt = decompose_utc(epoch_secs.checked_add(i64::from(offset))?)?;
    Some(Local::from_parts(dt, offset))
}

/// Current local date/time.
#[must_use]
pub fn now_local() -> Local {
    local_in(&tz::host_zone(), now_secs()).unwrap_or(Local::from_parts(EPOCH, 0))
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
    let dt = local_in(&tz::host_zone(), ms.div_euclid(1000))?;
    Some(format!("{:02}:{:02}", dt.hour, dt.minute))
}

/// Convert epoch milliseconds to local `YYYY-MM-DD HH:MM:SS`.
///
/// Returns `None` for negative timestamps.
#[must_use]
pub fn epoch_ms_to_local_ymd_hms(ms: i64) -> Option<String> {
    let dt = local_in(&tz::host_zone(), ms.div_euclid(1000))?;
    Some(format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second))
}

// ── Parsing ──────────────────────────────────────────────────────────────

/// Digit-only field at byte `range` of `s`.
fn digits(s: &str, range: std::ops::Range<usize>) -> Option<u32> {
    let field = s.get(range)?;
    field.bytes().all(|byte| byte.is_ascii_digit()).then(|| field.parse().ok()).flatten()
}

/// Whether `s` has the `YYYY-MM-DDTHH:MM:SS` separators (`T`, `t` or space).
fn has_separators(s: &str) -> bool {
    let at = |idx: usize| s.as_bytes().get(idx).copied();
    at(4) == Some(b'-')
        && at(7) == Some(b'-')
        && matches!(at(10), Some(b'T' | b't' | b' '))
        && at(13) == Some(b':')
        && at(16) == Some(b':')
}

/// Byte ranges of year, month, day, hour, minute, second.
const FIELDS: [std::ops::Range<usize>; 6] = [0..4, 5..7, 8..10, 11..13, 14..16, 17..19];

/// Parse the strict `YYYY-MM-DDTHH:MM:SS` prefix into [`Parts`]. Values that
/// don't fit their field saturate and are then rejected by [`is_valid`].
fn parse_datetime_fields(s: &str) -> Option<Parts> {
    has_separators(s).then_some(())?;
    let mut values = [0u32; 6];
    for (value, range) in values.iter_mut().zip(FIELDS) {
        *value = digits(s, range)?;
    }
    let [year, month, day, hour, minute, second] = values;
    let narrow = |value: u32| u8::try_from(value).unwrap_or(u8::MAX);
    Some(Parts {
        year: i32::try_from(year).unwrap_or(i32::MAX),
        month: narrow(month),
        day: narrow(day),
        hour: narrow(hour),
        minute: narrow(minute),
        second: narrow(second),
    })
}

/// Offset (seconds east) of an RFC 3339 suffix: optional `.frac`, then `Z`,
/// `+HH:MM` or `+HHMM`. An absent offset is an error.
fn parse_tz_suffix(rest: &str) -> Option<i32> {
    let tz = rest.strip_prefix('.').map_or(Some(rest), |frac| {
        let len = frac.bytes().take_while(u8::is_ascii_digit).count();
        (len > 0).then(|| frac.get(len..)).flatten()
    })?;
    if tz.eq_ignore_ascii_case("z") {
        return Some(0);
    }
    let compact =
        if tz.as_bytes().get(3) == Some(&b':') { format!("{}{}", tz.get(..3)?, tz.get(4..)?) } else { tz.to_owned() };
    tz::parse_numeric_offset(&compact)
}

/// Parse an RFC 3339 datetime to epoch milliseconds.
///
/// Accepts `2026-06-04T09:00:00Z`, `2026-06-04T11:00:00+02:00`, `+0200`,
/// and fractional seconds (ignored). Rejects impossible dates (Feb 31),
/// second 60, wrong separators and a missing offset.
#[must_use]
pub fn parse_rfc3339_to_epoch_ms(s: &str) -> Option<i64> {
    let parts = parse_datetime_fields(s)?;
    let offset = parse_tz_suffix(s.get(19..)?)?;
    compose_epoch_secs(&parts)?.checked_sub(i64::from(offset))?.checked_mul(1000)
}

/// Local wall-clock `YYYY-MM-DDTHH:MM:SS` in `zone` to epoch milliseconds.
fn parse_local_in(zone: &Zone, s: &str) -> Option<i64> {
    if s.len() != 19 {
        return None;
    }
    let local = compose_epoch_secs(&parse_datetime_fields(s)?)?;
    zone.local_to_utc(local).checked_mul(1000)
}

/// Parse a local datetime `YYYY-MM-DDTHH:MM:SS` to epoch milliseconds, using
/// the host zone's offset at that wall-clock time (DST-aware).
///
/// Used by the coucou tool for scheduling.
#[must_use]
pub fn parse_local_datetime_to_epoch_ms(s: &str) -> Option<i64> {
    parse_local_in(&tz::host_zone(), s)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-01-01T00:00:00Z.
    const NEW_YEAR_MS: i64 = 1_767_225_600_000;
    /// 2026-03-29T01:00:00Z: Paris springs forward.
    const PARIS_SPRING: i64 = 1_774_746_000;

    fn paris() -> Zone {
        Zone::from_posix("CET-1CEST,M3.5.0,M10.5.0/3").unwrap_or_default()
    }

    #[test]
    fn epoch_roundtrip() {
        assert_eq!(epoch_ms_to_rfc3339(NEW_YEAR_MS).as_deref(), Some("2026-01-01T00:00:00Z"));
        assert_eq!(epoch_secs_to_rfc3339_frac(1_767_225_600, 250_000_000).as_deref(), Some("2026-01-01T00:00:00.250Z"));
        assert!(epoch_secs_to_rfc3339_secs(-1).is_none());
        assert!(epoch_ms_to_rfc3339(-1000).is_none());
    }

    #[test]
    fn rfc3339_offsets() {
        let cases: [(&str, Option<i64>); 8] = [
            ("2026-01-01T00:00:00Z", Some(NEW_YEAR_MS)),
            ("2026-01-01T02:00:00+02:00", Some(NEW_YEAR_MS)),
            ("2026-01-01T02:00:00+0200", Some(NEW_YEAR_MS)),
            ("2025-12-31T19:00:00-05:00", Some(NEW_YEAR_MS)),
            ("2026-01-01t00:00:00.123z", Some(NEW_YEAR_MS)),
            ("2026-01-01T00:00:00.Z", None),
            ("2026-01-01T00:00:00", None),
            ("2026-01-01T00:00:00+25:00", None),
        ];
        for (input, expected) in cases {
            assert_eq!(parse_rfc3339_to_epoch_ms(input), expected, "{input}");
        }
    }

    #[test]
    fn rfc3339_rejects_impossible_and_malformed() {
        let rejected = [
            "2026-02-31T00:00:00Z",
            "2026-02-29T00:00:00Z",
            "2026-01-01T00:00:60Z",
            "2026/01/01T00:00:00Z",
            "2026-01-01X00:00:00Z",
            "2026-01-01T00:+1:00Z",
            "2026-13-01T00:00:00Z",
        ];
        for input in rejected {
            assert_eq!(parse_rfc3339_to_epoch_ms(input), None, "{input}");
        }
        assert!(parse_rfc3339_to_epoch_ms("2028-02-29T00:00:00Z").is_some(), "2028 leap");
    }

    #[test]
    fn local_follows_dst() {
        let zone = paris();
        let before = local_in(&zone, PARIS_SPRING - 1);
        assert_eq!(before.map(|dt| (dt.hour, dt.minute, dt.utc_offset_secs)), Some((1, 59, 3_600i32)));
        let after = local_in(&zone, PARIS_SPRING);
        assert_eq!(after.map(|dt| (dt.hour, dt.minute, dt.utc_offset_secs)), Some((3, 0, 7_200i32)));
    }

    #[test]
    fn local_parse_uses_offset_at_that_date() {
        let zone = paris();
        // Winter: CET (+1). Summer: CEST (+2).
        assert_eq!(parse_local_in(&zone, "2026-01-01T01:00:00"), Some(NEW_YEAR_MS));
        let july_noon_utc = calendar::days_from_civil(2026, 7, 1) * DAY_SECS + 10 * 3600;
        assert_eq!(parse_local_in(&zone, "2026-07-01 12:00:00"), Some(july_noon_utc * 1000));
        assert_eq!(parse_local_in(&zone, "2026-07-01T12:00"), None, "seconds required");
        assert_eq!(parse_local_in(&zone, "2026-04-31T12:00:00"), None, "Apr 31");
    }
}
