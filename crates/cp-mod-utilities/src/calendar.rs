//! Proleptic Gregorian calendar arithmetic shared by [`crate::time`],
//! [`crate::tz`] and [`crate::cron`].
//!
//! Day counts are relative to 1970-01-01 (day 0) and may be negative. The
//! conversions are Howard Hinnant's `days_from_civil` / `civil_from_days`
//! (400-year eras of 146 097 days, March-based years so Feb 29 is last).

/// Seconds in a day.
pub(crate) const DAY_SECS: i64 = 86_400;

/// `(year, month 1–12, day 1–31)` of a day count since 1970-01-01.
pub(crate) fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days.saturating_add(719_468);
    let era = shifted.div_euclid(146_097);
    let doe = shifted.rem_euclid(146_097);
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

/// Day count since 1970-01-01 of `year-month-day` (month 1–12). Out-of-range
/// days are not rejected: day 0 is the last day of the previous month.
pub(crate) const fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let yr = if month <= 2 { year.saturating_sub(1) } else { year };
    let era = yr.div_euclid(400);
    let yoe = yr.rem_euclid(400);
    let mp = if month > 2 { month.saturating_sub(3) } else { month.saturating_add(9) };
    let doy = mp.saturating_mul(153).saturating_add(2).div_euclid(5).saturating_add(day).saturating_sub(1);
    let doe = yoe
        .saturating_mul(365)
        .saturating_add(yoe.div_euclid(4))
        .saturating_sub(yoe.div_euclid(100))
        .saturating_add(doy);
    era.saturating_mul(146_097).saturating_add(doe).saturating_sub(719_468)
}

/// Gregorian leap year: divisible by 4, except centuries not divisible by 400.
pub(crate) const fn is_leap(year: i64) -> bool {
    year.rem_euclid(4) == 0 && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0)
}

/// Number of days in `month` (1–12) of `year`; 31 for an out-of-range month.
pub(crate) const fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 => {
            if is_leap(year) {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Day of week of a day count, 0 = Sunday (1970-01-01 was a Thursday).
pub(crate) const fn weekday(days: i64) -> i64 {
    days.saturating_add(4).rem_euclid(7)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_739), (2026, 10, 13));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(weekday(20_739), 2, "2026-10-13 is a Tuesday");
    }

    #[test]
    fn roundtrip_across_eras_and_leap_days() {
        let mut days = -800_000i64;
        while days < 800_000 {
            let (year, month, day) = civil_from_days(days);
            assert_eq!(days_from_civil(year, i64::from(month), i64::from(day)), days, "day {days}");
            days = days.saturating_add(997);
        }
        // Feb 29 exists in 2000 and 2028, not in 1900 or 2026.
        for (year, leap) in [(2000i64, true), (2028, true), (1900, false), (2026, false)] {
            assert_eq!(is_leap(year), leap, "{year}");
            let feb29 = days_from_civil(year, 2, 29);
            let expected = if leap { (year, 2, 29) } else { (year, 3, 1) };
            assert_eq!(civil_from_days(feb29), expected, "{year}");
        }
    }

    #[test]
    fn month_lengths() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(days_in_month(2026, 4), 30);
        assert_eq!(days_in_month(2026, 12), 31);
    }
}
