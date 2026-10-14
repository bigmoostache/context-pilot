//! 5-field POSIX cron expressions (`minute hour day-of-month month day-of-week`).
//!
//! Supports `*`, numbers, ranges `a-b`, steps `*/n` and `a-b/n`, and comma
//! lists. Day-of-week is 0–7 (0 and 7 are Sunday). As in POSIX cron, when both
//! day fields are restricted a day matches if EITHER does. Evaluated in the
//! host's local time (see [`crate::time::local_utc_offset_secs`]).

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
    /// Day-of-month field was `*` (POSIX OR rule only applies when both are restricted).
    any_day: bool,
    /// Day-of-week field was `*`.
    any_weekday: bool,
}

/// Seconds in a day.
const DAY_SECS: i64 = 86_400;

/// How far ahead [`Cron::next_after`] searches (≈ 5 years, covers Feb 29).
const SEARCH_DAYS: i64 = 366 * 5;

/// Bitmask of `value` (0–63).
fn bit(value: u32) -> u64 {
    1u64.checked_shl(value).unwrap_or(0)
}

/// Parse one comma-separated field into a bitmask over `min..=max`.
fn parse_field(field: &str, (min, max): (u32, u32), name: &str) -> Result<u64, String> {
    let mut mask = 0u64;
    for part in field.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((r, s)) => {
                let step: u32 = s.parse().map_err(|_e| format!("bad step `{s}` in {name}"))?;
                if step == 0 {
                    return Err(format!("step 0 in {name}"));
                }
                (r, step)
            }
            None => (part, 1),
        };
        let (lo, hi) = if range == "*" {
            (min, max)
        } else if let Some((a, b)) = range.split_once('-') {
            let lo: u32 = a.parse().map_err(|_e| format!("bad value `{a}` in {name}"))?;
            let hi: u32 = b.parse().map_err(|_e| format!("bad value `{b}` in {name}"))?;
            (lo, hi)
        } else {
            let v: u32 = range.parse().map_err(|_e| format!("bad value `{range}` in {name}"))?;
            // `5/15` means 5, 20, 35, 50: a lone start with a step runs to max.
            (v, if step > 1 { max } else { v })
        };
        if lo > hi {
            return Err(format!("{name} `{range}` is a reversed range (write {hi}-{lo})"));
        }
        if lo < min || hi > max {
            return Err(format!("{name} `{range}` out of range {min}-{max}"));
        }
        let mut v = lo;
        while v <= hi {
            mask |= bit(v);
            v = v.saturating_add(step);
        }
    }
    Ok(mask)
}

impl Cron {
    /// Parse a 5-field expression, e.g. `0 * * * *` (hourly) or `30 8 * * 1-5`.
    ///
    /// # Errors
    ///
    /// Wrong field count, unparsable values, out-of-range values or a zero step.
    pub fn parse(expr: &str) -> Result<Self, String> {
        let fields: Vec<&str> = expr.split_whitespace().collect();
        let &[minute, hour, day, month, weekday] = fields.as_slice() else {
            return Err(format!(
                "cron `{expr}` must have 5 fields: minute hour day-of-month month day-of-week (e.g. `0 9 * * 1-5`)"
            ));
        };
        // Left to right, so an error names the first bad field.
        let minutes = parse_field(minute, (0, 59), "minute")?;
        let hours = parse_field(hour, (0, 23), "hour")?;
        let days = parse_field(day, (1, 31), "day-of-month")?;
        let months = parse_field(month, (1, 12), "month")?;
        let raw_weekdays = parse_field(weekday, (0, 7), "day-of-week")?;
        // 7 is Sunday too: fold it onto 0.
        let weekdays = (raw_weekdays & !bit(7)) | if raw_weekdays & bit(7) == 0 { 0 } else { bit(0) };
        Ok(Self { minutes, hours, days, months, weekdays, any_day: day == "*", any_weekday: weekday == "*" })
    }

    /// Whether the calendar day (`day` of month, `month`, `weekday` 0=Sunday) matches.
    fn day_matches(&self, day: u32, month: u32, weekday: u32) -> bool {
        if self.months & bit(month) == 0 {
            return false;
        }
        let by_day = self.days & bit(day) != 0;
        let by_weekday = self.weekdays & bit(weekday) != 0;
        match (self.any_day, self.any_weekday) {
            (true, true) => true,
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

    /// Next firing time strictly after `after_ms` (epoch ms), for a host at
    /// `offset_secs` from UTC. `None` if nothing matches within ~5 years
    /// (e.g. `0 0 31 2 *`).
    #[must_use]
    pub fn next_after(&self, after_ms: u64, offset_secs: i32) -> Option<u64> {
        let after_secs = i64::try_from(after_ms.checked_div(1000)?).ok()?;
        // Local time of the next whole minute.
        let local = after_secs.saturating_add(i64::from(offset_secs));
        let start = local.saturating_sub(local.rem_euclid(60)).saturating_add(60);
        let first_day = start.div_euclid(DAY_SECS);
        let mut from_minute = u32::try_from(start.rem_euclid(DAY_SECS).checked_div(60)?).ok()?;
        for day_index in first_day..first_day.saturating_add(SEARCH_DAYS) {
            let (_, month, day) = civil_from_days(day_index);
            // 1970-01-01 was a Thursday (4).
            let weekday = u32::try_from(day_index.saturating_add(4).rem_euclid(7)).ok()?;
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

    /// First allowed minute-of-day (0–1439) at or after `from`, same day.
    fn first_minute_from(&self, from: u32) -> Option<u32> {
        let (mut hour, mut minute) = (from.checked_div(60)?, from.checked_rem(60)?);
        while let Some(h) = Self::first_at_or_after(self.hours, hour) {
            if h > 23 {
                return None;
            }
            if h > hour {
                minute = 0;
            }
            if let Some(m) = Self::first_at_or_after(self.minutes, minute).filter(|m| *m < 60) {
                return Some(h.saturating_mul(60).saturating_add(m));
            }
            (hour, minute) = (h.saturating_add(1), 0);
        }
        None
    }
}

/// `(year, month 1–12, day 1–31)` of a day count since 1970-01-01
/// (Howard Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days.saturating_add(719_468);
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = doe
        .saturating_sub(doe.div_euclid(1460))
        .saturating_add(doe.div_euclid(36_524))
        .saturating_sub(doe.div_euclid(146_096))
        .div_euclid(365);
    let doy = doe
        .saturating_sub(yoe.saturating_mul(365).saturating_add(yoe.div_euclid(4)).saturating_sub(yoe.div_euclid(100)));
    let mp = doy.saturating_mul(5).saturating_add(2).div_euclid(153);
    let day = doy.saturating_sub(mp.saturating_mul(153).saturating_add(2).div_euclid(5)).saturating_add(1);
    let month = if mp < 10 { mp.saturating_add(3) } else { mp.saturating_sub(9) };
    let year = yoe.saturating_add(era.saturating_mul(400)).saturating_add(i64::from(month <= 2));
    (year, u32::try_from(month).unwrap_or(1), u32::try_from(day).unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-10-13T17:52:00Z (a Tuesday).
    const T0: u64 = 1_791_913_920_000;

    fn next(expr: &str, after: u64) -> Option<u64> {
        Cron::parse(expr).ok()?.next_after(after, 0)
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_739), (2026, 10, 13));
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
        let c = next("0 0 15 * 1", T0);
        assert_eq!(c, next("0 0 15 * *", T0));
        assert!(next("0 0 31 2 *", T0).is_none());
        assert_eq!(Cron::parse("0 9 * *").ok(), None);
        assert_eq!(Cron::parse("60 * * * *").ok(), None);
        assert_eq!(Cron::parse("*/0 * * * *").ok(), None);
    }

    #[test]
    fn error_messages_name_the_right_problem() {
        // Reversed range: not "out of range".
        let reversed = Cron::parse("5-1 * * * *").unwrap_err();
        assert!(reversed.contains("reversed range"), "{reversed}");
        // A garbage expression blames the FIRST bad field (minute), not the last.
        let garbage = Cron::parse("foo bar baz qux quux").unwrap_err();
        assert!(garbage.contains("minute"), "{garbage}");
    }
}
