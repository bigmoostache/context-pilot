//! POSIX `TZ` rule strings (`CET-1CEST,M3.5.0,M10.5.0/3`): the `$TZ` value
//! form and the `TZif` v2+ footer that extends a zone past its last transition.

use crate::calendar::{self, DAY_SECS};

/// Day of a POSIX DST rule within a year.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuleDate {
    /// `Jn`: day 1–365, Feb 29 never counted.
    Julian1(i64),
    /// `n`: zero-based day 0–365, Feb 29 counted.
    Julian0(i64),
    /// `Mm.w.d`: weekday `day` (0 = Sunday) of week `week` (1–5, 5 = last) of `month`.
    Weekday {
        /// Month 1–12.
        month: i64,
        /// Week 1–5.
        week: i64,
        /// Weekday 0–6.
        day: i64,
    },
}

impl RuleDate {
    /// Day count since 1970-01-01 of this rule date in `year`.
    fn day_in(self, year: i64) -> i64 {
        let jan1 = calendar::days_from_civil(year, 1, 1);
        match self {
            Self::Julian1(n) => {
                let leap_shift = i64::from(calendar::is_leap(year) && n >= 60);
                jan1.saturating_add(n).saturating_sub(1).saturating_add(leap_shift)
            }
            Self::Julian0(n) => jan1.saturating_add(n),
            Self::Weekday { month, week, day } => {
                let first = calendar::days_from_civil(year, month, 1);
                let delta = day.saturating_sub(calendar::weekday(first)).rem_euclid(7);
                let candidate = first.saturating_add(delta).saturating_add(week.saturating_sub(1).saturating_mul(7));
                let last = first.saturating_add(calendar::days_in_month(year, month)).saturating_sub(1);
                if candidate > last { candidate.saturating_sub(7) } else { candidate }
            }
        }
    }
}

/// Daylight-saving half of a POSIX rule.
#[derive(Debug, Clone, Copy)]
struct Dst {
    /// Offset east of UTC while DST is in force, seconds.
    offset: i32,
    /// Day DST starts.
    start: RuleDate,
    /// Local (standard) time of day DST starts, seconds.
    start_secs: i64,
    /// Day DST ends.
    end: RuleDate,
    /// Local (daylight) time of day DST ends, seconds.
    end_secs: i64,
}

/// A parsed POSIX `TZ` rule.
#[derive(Debug, Clone, Copy)]
pub(super) struct Rule {
    /// Standard offset east of UTC, seconds.
    std_offset: i32,
    /// Daylight-saving rule, if the zone observes DST.
    dst: Option<Dst>,
}

impl Rule {
    /// Standard offset east of UTC, seconds.
    pub(super) const fn std_offset(self) -> i32 {
        self.std_offset
    }

    /// Offset in force at `utc` (epoch seconds).
    pub(super) fn offset_at(self, utc: i64) -> i32 {
        let Some(dst) = self.dst else { return self.std_offset };
        let std_local = utc.saturating_add(i64::from(self.std_offset));
        let (year, _, _) = calendar::civil_from_days(std_local.div_euclid(DAY_SECS));
        let start = dst
            .start
            .day_in(year)
            .saturating_mul(DAY_SECS)
            .saturating_add(dst.start_secs)
            .saturating_sub(i64::from(self.std_offset));
        let end = dst
            .end
            .day_in(year)
            .saturating_mul(DAY_SECS)
            .saturating_add(dst.end_secs)
            .saturating_sub(i64::from(dst.offset));
        // Southern-hemisphere rules start late in the year and end early.
        let in_dst = if start <= end { start <= utc && utc < end } else { utc < end || utc >= start };
        if in_dst { dst.offset } else { self.std_offset }
    }
}

/// Byte cursor over a POSIX `TZ` rule string.
struct Cursor<'src> {
    /// Rule bytes.
    bytes: &'src [u8],
    /// Read position.
    pos: usize,
}

impl Cursor<'_> {
    /// Next byte without consuming it.
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    /// Consume `expected` if it is next.
    fn eat(&mut self, expected: u8) -> bool {
        let hit = self.peek() == Some(expected);
        if hit {
            self.pos = self.pos.saturating_add(1);
        }
        hit
    }

    /// Consume bytes while `pred` holds; returns how many.
    fn skip_while(&mut self, pred: fn(u8) -> bool) -> usize {
        let start = self.pos;
        while self.peek().is_some_and(pred) {
            self.pos = self.pos.saturating_add(1);
        }
        self.pos.saturating_sub(start)
    }

    /// Zone abbreviation: 3+ letters, or `<...>`.
    fn name(&mut self) -> Option<()> {
        if self.eat(b'<') {
            (self.skip_while(|byte| byte != b'>') > 0 && self.eat(b'>')).then_some(())
        } else {
            (self.skip_while(|byte| byte.is_ascii_alphabetic()) >= 3).then_some(())
        }
    }

    /// Unsigned decimal number (at least one digit).
    fn number(&mut self) -> Option<i64> {
        let start = self.pos;
        let len = self.skip_while(|byte| byte.is_ascii_digit());
        if len == 0 {
            return None;
        }
        let digits = self.bytes.get(start..start.checked_add(len)?)?;
        std::str::from_utf8(digits).ok()?.parse().ok()
    }

    /// Optional `+`/`-` sign as a multiplier (default `+`).
    fn sign(&mut self) -> i64 {
        if self.eat(b'-') {
            return -1;
        }
        let _plus = self.eat(b'+');
        1
    }

    /// `:nn` component, or 0 when absent.
    fn sub_field(&mut self) -> Option<i64> {
        if self.eat(b':') { self.number() } else { Some(0) }
    }

    /// `[+-]hh[:mm[:ss]]` as signed seconds.
    fn hms(&mut self) -> Option<i64> {
        let sign = self.sign();
        let hours = self.number()?;
        let minutes = self.sub_field()?;
        let seconds = self.sub_field()?;
        Some(
            hours
                .saturating_mul(3600)
                .saturating_add(minutes.saturating_mul(60))
                .saturating_add(seconds)
                .saturating_mul(sign),
        )
    }

    /// POSIX offset (positive = WEST of UTC) as seconds EAST of UTC.
    fn offset(&mut self) -> Option<i32> {
        i32::try_from(self.hms()?.checked_neg()?).ok()
    }

    /// Number within `range`.
    fn bounded(&mut self, range: std::ops::RangeInclusive<i64>) -> Option<i64> {
        self.number().filter(|value| range.contains(value))
    }

    /// `m.w.d` after an `M`.
    fn weekday_rule(&mut self) -> Option<RuleDate> {
        let month = self.bounded(1..=12)?;
        self.eat(b'.').then_some(())?;
        let week = self.bounded(1..=5)?;
        self.eat(b'.').then_some(())?;
        let day = self.bounded(0..=6)?;
        Some(RuleDate::Weekday { month, week, day })
    }

    /// `Mm.w.d`, `Jn` or `n`.
    fn rule_date(&mut self) -> Option<RuleDate> {
        if self.eat(b'M') {
            self.weekday_rule()
        } else if self.eat(b'J') {
            self.bounded(1..=365).map(RuleDate::Julian1)
        } else {
            self.bounded(0..=365).map(RuleDate::Julian0)
        }
    }

    /// Rule date, then optional `/time` (default 02:00).
    fn transition(&mut self) -> Option<(RuleDate, i64)> {
        let date = self.rule_date()?;
        let secs = if self.eat(b'/') { self.hms()? } else { 7200 };
        Some((date, secs))
    }

    /// DST part after the standard offset: `dst [offset] [,start[/time],end[/time]]`.
    fn dst(&mut self, std_offset: i32) -> Option<Dst> {
        self.name()?;
        let offset =
            if self.peek().is_some_and(|byte| byte != b',') { self.offset()? } else { std_offset.checked_add(3600)? };
        // No explicit rule: the POSIX-implementation default is the US one.
        let ((start, start_secs), (end, end_secs)) = if self.eat(b',') {
            let first = self.transition()?;
            self.eat(b',').then_some(())?;
            (first, self.transition()?)
        } else {
            (
                (RuleDate::Weekday { month: 3, week: 2, day: 0 }, 7200),
                (RuleDate::Weekday { month: 11, week: 1, day: 0 }, 7200),
            )
        };
        Some(Dst { offset, start, start_secs, end, end_secs })
    }
}

/// Parse a POSIX `TZ` rule (`std offset [dst [offset] [,start[/time],end[/time]]]`).
pub(super) fn parse(spec: &str) -> Option<Rule> {
    let mut cursor = Cursor { bytes: spec.as_bytes(), pos: 0 };
    cursor.name()?;
    let std_offset = cursor.offset()?;
    if cursor.peek().is_none() {
        return Some(Rule { std_offset, dst: None });
    }
    let dst = cursor.dst(std_offset)?;
    cursor.peek().is_none().then_some(Rule { std_offset, dst: Some(dst) })
}
