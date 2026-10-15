//! 5-field POSIX cron expressions (`minute hour day-of-month month day-of-week`).
//!
//! Supports `*`, numbers, names (`JAN`–`DEC`, `SUN`–`SAT`, any case), ranges
//! `a-b`, steps `*/n` and `a-b/n`, comma lists, and the `@yearly`,
//! `@annually`, `@monthly`, `@weekly`, `@daily`, `@midnight` and `@hourly`
//! shortcuts. Day-of-week is 0–7 (0 and 7 are Sunday). As in Vixie cron, when
//! both day fields are restricted a day matches if EITHER does, and a field
//! starting with `*` (including `*/n`) counts as unrestricted for that rule.

use crate::calendar::{self, DAY_SECS};
use crate::tz::{self, Zone};

/// A parsed cron expression: one bit per allowed value of each field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cron {
    /// Minutes 0–59.
    minutes: u64,
    /// Hours 0–23.
    hours: u64,
    /// Days of month 1–31.
    days: u64,
    /// Months 1–12.
    months: u64,
    /// Days of week 0–6 (Sunday = 0).
    weekdays: u64,
    /// Day-of-month field started with `*` (OR rule only applies when both are restricted).
    any_day: bool,
    /// Day-of-week field started with `*`.
    any_weekday: bool,
}

/// How far ahead [`Cron::next_after`] searches (≈ 5 years, covers Feb 29).
const SEARCH_DAYS: i64 = 366 * 5;

/// Month names; index 0 is month 1.
const MONTH_NAMES: [&str; 12] = ["JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC"];

/// Weekday names; index 0 is Sunday.
const WEEKDAY_NAMES: [&str; 7] = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"];

/// One field's grammar: allowed range, accepted names, label for errors.
struct Field {
    /// Smallest allowed value.
    min: u32,
    /// Largest allowed value.
    max: u32,
    /// Names; `names[i]` stands for `min + i`.
    names: &'static [&'static str],
    /// Field label used in error messages.
    label: &'static str,
}

/// Bitmask of `value` (0–63).
fn bit(value: u32) -> u64 {
    1u64.checked_shl(value).unwrap_or(0)
}

impl Field {
    /// A number or (case-insensitive) name.
    fn value(&self, token: &str) -> Result<u32, String> {
        if let Ok(number) = token.parse::<u32>() {
            return Ok(number);
        }
        self.names
            .iter()
            .position(|name| name.eq_ignore_ascii_case(token))
            .and_then(|idx| u32::try_from(idx).ok())
            .map(|idx| self.min.saturating_add(idx))
            .ok_or_else(|| format!("bad value `{token}` in {}", self.label))
    }

    /// Parse one comma-separated field into a bitmask over `min..=max`.
    fn parse(&self, field: &str) -> Result<u64, String> {
        let mut mask = 0u64;
        for part in field.split(',') {
            let (range, step) = match part.split_once('/') {
                Some((head, raw_step)) => {
                    let step: u32 =
                        raw_step.parse().map_err(|_e| format!("bad step `{raw_step}` in {}", self.label))?;
                    if step == 0 {
                        return Err(format!("step 0 in {}", self.label));
                    }
                    (head, step)
                }
                None => (part, 1),
            };
            let (lo, hi) = if range == "*" {
                (self.min, self.max)
            } else if let Some((first, last)) = range.split_once('-') {
                (self.value(first)?, self.value(last)?)
            } else {
                let start = self.value(range)?;
                // `5/15` means 5, 20, 35, 50: a lone start with a step runs to max.
                (start, if step > 1 { self.max } else { start })
            };
            if lo > hi {
                return Err(format!("{} `{range}` is a reversed range (write {hi}-{lo})", self.label));
            }
            if lo < self.min || hi > self.max {
                return Err(format!("{} `{range}` out of range {}-{}", self.label, self.min, self.max));
            }
            let mut value = lo;
            while value <= hi {
                mask |= bit(value);
                value = value.saturating_add(step);
            }
        }
        Ok(mask)
    }
}

/// UTC epoch ms of wall-clock epoch ms `wall_ms` in `zone`.
fn wall_ms_to_utc_ms(zone: &Zone, wall_ms: u64) -> Option<u64> {
    let utc = zone.local_to_utc(i64::try_from(wall_ms.checked_div(1000)?).ok()?);
    u64::try_from(utc).ok()?.checked_mul(1000)
}

/// Expansion of an `@` shortcut, `None` if unknown.
fn expand_macro(name: &str) -> Option<&'static str> {
    match name.to_ascii_lowercase().as_str() {
        "@yearly" | "@annually" => Some("0 0 1 1 *"),
        "@monthly" => Some("0 0 1 * *"),
        "@weekly" => Some("0 0 * * 0"),
        "@daily" | "@midnight" => Some("0 0 * * *"),
        "@hourly" => Some("0 * * * *"),
        _ => None,
    }
}

impl Cron {
    /// Parse a 5-field expression, e.g. `0 * * * *` (hourly), `30 8 * * MON-FRI`
    /// or `@daily`.
    ///
    /// # Errors
    ///
    /// Wrong field count, unknown shortcut, unparsable values, out-of-range
    /// values or a zero step.
    pub fn parse(expr: &str) -> Result<Self, String> {
        let trimmed = expr.trim();
        let body = if trimmed.starts_with('@') {
            expand_macro(trimmed).ok_or_else(|| {
                format!("unknown cron shortcut `{trimmed}` (use @yearly, @monthly, @weekly, @daily or @hourly)")
            })?
        } else {
            trimmed
        };
        let fields: Vec<&str> = body.split_whitespace().collect();
        let &[minute, hour, day, month, weekday] = fields.as_slice() else {
            return Err(format!(
                "cron `{expr}` must have 5 fields: minute hour day-of-month month day-of-week (e.g. `0 9 * * 1-5`)"
            ));
        };
        // Left to right, so an error names the first bad field.
        let minutes = Field { min: 0, max: 59, names: &[], label: "minute" }.parse(minute)?;
        let hours = Field { min: 0, max: 23, names: &[], label: "hour" }.parse(hour)?;
        let days = Field { min: 1, max: 31, names: &[], label: "day-of-month" }.parse(day)?;
        let months = Field { min: 1, max: 12, names: &MONTH_NAMES, label: "month" }.parse(month)?;
        let raw_weekdays = Field { min: 0, max: 7, names: &WEEKDAY_NAMES, label: "day-of-week" }.parse(weekday)?;
        // 7 is Sunday too: fold it onto 0.
        let weekdays = (raw_weekdays & !bit(7)) | if raw_weekdays & bit(7) == 0 { 0 } else { bit(0) };
        Ok(Self {
            minutes,
            hours,
            days,
            months,
            weekdays,
            any_day: day.starts_with('*'),
            any_weekday: weekday.starts_with('*'),
        })
    }

    /// Whether the calendar day (`day` of month, `month`, `weekday` 0=Sunday) matches.
    fn day_matches(&self, day: u32, month: u32, weekday: u32) -> bool {
        if self.months & bit(month) == 0 {
            return false;
        }
        let by_day = self.days & bit(day) != 0;
        let by_weekday = self.weekdays & bit(weekday) != 0;
        match (self.any_day, self.any_weekday) {
            (true, true) => by_day && by_weekday,
            (true, false) => by_weekday,
            (false, true) => by_day,
            (false, false) => by_day || by_weekday,
        }
    }

    /// First allowed value of `mask` at or above `from` (below 64).
    fn first_at_or_after(mask: u64, from: u32) -> Option<u32> {
        let shifted = mask.checked_shr(from).unwrap_or(0);
        (shifted != 0).then(|| from.saturating_add(shifted.trailing_zeros()))
    }

    /// Next firing time strictly after `after_ms` (epoch ms), for a host at a
    /// FIXED `offset_secs` from UTC. `None` if nothing matches within ~5 years
    /// (e.g. `0 0 31 2 *`). For the host's DST-aware zone use
    /// [`Cron::next_after_local`].
    #[must_use]
    pub fn next_after(&self, after_ms: u64, offset_secs: i32) -> Option<u64> {
        let after_secs = i64::try_from(after_ms.checked_div(1000)?).ok()?;
        // Local time of the next whole minute.
        let local = after_secs.saturating_add(i64::from(offset_secs));
        let start = local.saturating_sub(local.rem_euclid(60)).saturating_add(60);
        let first_day = start.div_euclid(DAY_SECS);
        let mut from_minute = u32::try_from(start.rem_euclid(DAY_SECS).checked_div(60)?).ok()?;
        for day_index in first_day..first_day.saturating_add(SEARCH_DAYS) {
            let (_, month, day) = calendar::civil_from_days(day_index);
            let weekday = u32::try_from(calendar::weekday(day_index)).ok()?;
            if self.day_matches(day, month, weekday)
                && let Some(minute_of_day) = self.first_minute_from(from_minute)
            {
                let local_secs =
                    day_index.saturating_mul(DAY_SECS).saturating_add(i64::from(minute_of_day).saturating_mul(60));
                let utc = local_secs.saturating_sub(i64::from(offset_secs));
                return u64::try_from(utc).ok()?.checked_mul(1000);
            }
            from_minute = 0;
        }
        None
    }

    /// Next firing time strictly after `after_ms` in the host time zone,
    /// using the offset in force at each candidate (DST-aware).
    #[must_use]
    pub fn next_after_local(&self, after_ms: u64) -> Option<u64> {
        self.next_after_in(&tz::host_zone(), after_ms)
    }

    /// [`Cron::next_after_local`] for an explicit zone.
    ///
    /// The search runs on the wall clock, so each wall time fires at most once:
    /// a time inside the spring-forward gap fires right after the gap (shifted
    /// by it), and the repeated fall-back hour does not fire a second time.
    fn next_after_in(&self, zone: &Zone, after_ms: u64) -> Option<u64> {
        let after_secs = i64::try_from(after_ms.checked_div(1000)?).ok()?;
        let wall_secs = after_secs.checked_add(i64::from(zone.offset_at(after_secs)))?;
        let mut wall_ms = u64::try_from(wall_secs).ok()?.checked_mul(1000)?;
        // A fall-back hour can map the next wall time to an instant already
        // passed; a couple of extra steps always clear it.
        for _ in 0u8..3u8 {
            let next_wall = self.next_after(wall_ms, 0)?;
            let utc_ms = wall_ms_to_utc_ms(zone, next_wall)?;
            if utc_ms > after_ms {
                return Some(utc_ms);
            }
            wall_ms = next_wall;
        }
        None
    }

    /// First allowed minute-of-day (0–1439) at or after `from`, same day.
    fn first_minute_from(&self, from: u32) -> Option<u32> {
        let (mut hour, mut minute) = (from.checked_div(60)?, from.checked_rem(60)?);
        while let Some(found_hour) = Self::first_at_or_after(self.hours, hour) {
            if found_hour > 23 {
                return None;
            }
            if found_hour > hour {
                minute = 0;
            }
            if let Some(found_minute) = Self::first_at_or_after(self.minutes, minute).filter(|found| *found < 60) {
                return Some(found_hour.saturating_mul(60).saturating_add(found_minute));
            }
            (hour, minute) = (found_hour.saturating_add(1), 0);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-10-13T17:52:00Z (a Tuesday).
    const T0: u64 = 1_791_913_920_000;
    /// 2026-03-29T01:00:00Z: Paris springs forward (02:00 CET -> 03:00 CEST).
    const PARIS_SPRING_MS: u64 = 1_774_746_000_000;
    /// 2026-10-25T01:00:00Z: Paris falls back (03:00 CEST -> 02:00 CET).
    const PARIS_FALL_MS: u64 = 1_792_890_000_000;
    /// One hour in ms.
    const HOUR: u64 = 3_600_000;

    fn next(expr: &str, after: u64) -> Option<u64> {
        Cron::parse(expr).ok()?.next_after(after, 0)
    }

    fn paris() -> Zone {
        Zone::from_posix("CET-1CEST,M3.5.0,M10.5.0/3").unwrap_or_default()
    }

    fn next_paris(expr: &str, after: u64) -> Option<u64> {
        Cron::parse(expr).ok()?.next_after_in(&paris(), after)
    }

    #[test]
    fn every_minute_and_hourly() {
        assert_eq!(next("* * * * *", T0), Some(T0 + 60_000));
        assert_eq!(next("0 * * * *", T0), Some(T0 + 8 * 60_000));
        assert_eq!(next("*/15 * * * *", T0), Some(T0 + 8 * 60_000));
    }

    #[test]
    fn daily_weekday_and_offset() {
        // 09:00 the next day (Wednesday).
        let nine = T0 + (24 * 60 - (17 * 60 + 52) + 9 * 60) * 60_000;
        assert_eq!(next("0 9 * * *", T0), Some(nine));
        assert_eq!(next("0 9 * * 3", T0), Some(nine));
        // Monday 09:00 = Wednesday + 5 days; 7 also means Sunday.
        assert_eq!(next("0 9 * * 1", T0), Some(nine + 5 * 86_400_000));
        assert_eq!(next("0 9 * * 7", T0), next("0 9 * * 0", T0));
        // At UTC+2, 20:00 local = 18:00 UTC, 8 minutes after T0.
        assert_eq!(Cron::parse("0 20 * * *").ok().and_then(|c| c.next_after(T0, 7200)), Some(T0 + 8 * 60_000));
    }

    #[test]
    fn posix_day_or_rule_and_errors() {
        // Day 15 OR Monday: Monday 19th comes after the 15th (Thursday).
        assert_eq!(next("0 0 15 * 1", T0), next("0 0 15 * *", T0));
        assert!(next("0 0 31 2 *", T0).is_none());
        assert_eq!(Cron::parse("0 9 * *").ok(), None);
        assert_eq!(Cron::parse("60 * * * *").ok(), None);
        assert_eq!(Cron::parse("*/0 * * * *").ok(), None);
    }

    #[test]
    fn star_step_day_field_is_unrestricted_for_the_or_rule() {
        // `*/1` day + Monday = Mondays only (Vixie), not every day.
        assert_eq!(next("0 9 */1 * 1", T0), next("0 9 * * 1", T0));
        // `*/2` day + `*` weekday = odd days only.
        let odd = Cron::parse("0 0 */2 * *").ok();
        assert_eq!(odd.map(|cron| cron.day_matches(14, 10, 3)), Some(false));
        assert_eq!(odd.map(|cron| cron.day_matches(15, 10, 4)), Some(true));
    }

    #[test]
    fn names_and_shortcuts() {
        let same = [
            ("30 8 * * MON-FRI", "30 8 * * 1-5"),
            ("0 0 1 jan,Jul *", "0 0 1 1,7 *"),
            ("0 0 * * sun", "0 0 * * 0"),
            ("@daily", "0 0 * * *"),
            ("@MIDNIGHT", "0 0 * * *"),
            ("@hourly", "0 * * * *"),
            ("@weekly", "0 0 * * 0"),
            ("@monthly", "0 0 1 * *"),
            ("@annually", "0 0 1 1 *"),
        ];
        for (named, numeric) in same {
            assert_eq!(Cron::parse(named).ok(), Cron::parse(numeric).ok(), "{named}");
        }
        for bad in ["@reboot", "0 0 * * FOO"] {
            assert_eq!(Cron::parse(bad).ok(), None, "{bad}");
        }
    }

    #[test]
    fn error_messages_name_the_right_problem() {
        let reversed = Cron::parse("5-1 * * * *").unwrap_err();
        assert!(reversed.contains("reversed range"), "{reversed}");
        let garbage = Cron::parse("foo bar baz qux quux").unwrap_err();
        assert!(garbage.contains("minute"), "{garbage}");
    }

    #[test]
    fn local_schedule_tracks_dst() {
        // 09:00 Paris the day before spring-forward is 08:00Z; the day after, 07:00Z.
        let before = next_paris("0 9 * * *", PARIS_SPRING_MS - 20 * HOUR);
        assert_eq!(before, Some(PARIS_SPRING_MS + 7 * HOUR - 24 * HOUR));
        let after = next_paris("0 9 * * *", PARIS_SPRING_MS);
        assert_eq!(after, Some(PARIS_SPRING_MS + 6 * HOUR));
        // After fall-back, 09:00 CET = 08:00Z.
        assert_eq!(next_paris("0 9 * * *", PARIS_FALL_MS), Some(PARIS_FALL_MS + 7 * HOUR));
    }

    #[test]
    fn local_schedule_gap_and_overlap() {
        // 02:30 does not exist on spring-forward day: fires once at 03:30 CEST (01:30Z).
        let gap = next_paris("30 2 * * *", PARIS_SPRING_MS - 2 * HOUR);
        assert_eq!(gap, Some(PARIS_SPRING_MS + 1_800_000));
        // Hourly across fall-back: strictly increasing, never repeats an instant.
        let mut at = PARIS_FALL_MS - 3 * HOUR;
        for _ in 0u8..6u8 {
            let fired = next_paris("0 * * * *", at);
            assert!(fired.is_some_and(|ms| ms > at), "{fired:?} after {at}");
            at = fired.unwrap_or(u64::MAX);
        }
    }
}
