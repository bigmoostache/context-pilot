//! Host time zone: offset from UTC at any instant, DST-aware.
//!
//! Replaces a process-lifetime `date +%z` snapshot, which kept the
//! pre-transition offset forever once the clocks changed. The zone is read
//! from, in order: `$TZ` (`:path`, zoneinfo name, or POSIX rule string),
//! `/etc/localtime` (`TZif`), a one-off `date +%z`, then UTC. The loaded zone
//! is cached for [`ZONE_TTL`], so a host time-zone change is picked up
//! without a restart; offsets are then computed per instant.

mod posix;
mod tzif;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a loaded zone is trusted before the files are re-read.
const ZONE_TTL: Duration = Duration::from_mins(10);

/// Where zoneinfo names from `$TZ` (e.g. `Europe/Paris`) are resolved.
const ZONEINFO_DIRS: [&str; 2] = ["/usr/share/zoneinfo", "/var/db/timezone/zoneinfo"];

/// A loaded time zone. The default is UTC.
#[derive(Debug, Clone, Default)]
pub(crate) struct Zone {
    /// `(utc_secs, offset_after)` transitions, ascending.
    transitions: Vec<(i64, i32)>,
    /// Offset before the first transition.
    initial: i32,
    /// Rule for instants after the last transition (`TZif` footer or `$TZ`).
    rule: Option<posix::Rule>,
}

impl Zone {
    /// Offset east of UTC in force at `utc` (epoch seconds).
    pub(crate) fn offset_at(&self, utc: i64) -> i32 {
        let idx = self.transitions.partition_point(|transition| transition.0 <= utc);
        if idx == self.transitions.len()
            && let Some(rule) = self.rule
        {
            return rule.offset_at(utc);
        }
        idx.checked_sub(1).and_then(|prev| self.transitions.get(prev)).map_or(self.initial, |transition| transition.1)
    }

    /// UTC instant of local wall-clock `local` (seconds). In the fall-back
    /// hour either valid instant may be returned; in the spring-forward gap
    /// the time is shifted by the gap.
    pub(crate) fn local_to_utc(&self, local: i64) -> i64 {
        let guess = local.saturating_sub(i64::from(self.offset_at(local)));
        local.saturating_sub(i64::from(self.offset_at(guess)))
    }

    /// Zone from a POSIX `TZ` rule string, `None` if malformed.
    pub(crate) fn from_posix(spec: &str) -> Option<Self> {
        let rule = posix::parse(spec)?;
        Some(Self { transitions: Vec::new(), initial: rule.std_offset(), rule: Some(rule) })
    }
}

/// Cached host zone and when it was loaded.
type Cached = Option<(Instant, Arc<Zone>)>;

/// The host zone, reloaded at most every [`ZONE_TTL`].
pub(crate) fn host_zone() -> Arc<Zone> {
    /// Last loaded zone and when.
    static CACHE: Mutex<Cached> = Mutex::new(None);
    let cached = CACHE.lock().ok().and_then(|guard| {
        guard.as_ref().filter(|entry| entry.0.elapsed() < ZONE_TTL).map(|entry| Arc::clone(&entry.1))
    });
    if let Some(zone) = cached {
        return zone;
    }
    let fresh = Arc::new(load());
    if let Ok(mut guard) = CACHE.lock() {
        *guard = Some((Instant::now(), Arc::clone(&fresh)));
    }
    fresh
}

/// Read the host zone from its sources (see module docs).
fn load() -> Zone {
    cp_env::env()
        .core
        .tz
        .as_deref()
        .and_then(from_tz_var)
        .or_else(|| read_tzif("/etc/localtime"))
        .or_else(from_date_cmd)
        .unwrap_or_default()
}

/// Zone named by a `$TZ` value. Empty means UTC (POSIX).
fn from_tz_var(tz: &str) -> Option<Zone> {
    let spec = tz.strip_prefix(':').unwrap_or(tz);
    if spec.is_empty() {
        return Some(Zone::default());
    }
    if spec.starts_with('/') {
        return read_tzif(spec);
    }
    if !spec.contains("..")
        && let Some(zone) = ZONEINFO_DIRS.iter().find_map(|dir| read_tzif(&format!("{dir}/{spec}")))
    {
        return Some(zone);
    }
    Zone::from_posix(spec)
}

/// Fixed-offset zone from `date +%z` (last resort before UTC).
fn from_date_cmd() -> Option<Zone> {
    let out = std::process::Command::new("date").arg("+%z").output().ok()?;
    let offset = parse_numeric_offset(String::from_utf8(out.stdout).ok()?.trim())?;
    Some(Zone { initial: offset, ..Zone::default() })
}

/// Parse `+HHMM` / `-HHMM` (the `date +%z` shape) into seconds east of UTC.
pub(crate) fn parse_numeric_offset(s: &str) -> Option<i32> {
    let (sign, rest) = match s.as_bytes().first().copied()? {
        b'+' => (1i32, s.get(1..)?),
        b'-' => (-1i32, s.get(1..)?),
        _ => return None,
    };
    if rest.len() != 4 || !rest.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let hours: i32 = rest.get(..2)?.parse().ok()?;
    let mins: i32 = rest.get(2..)?.parse().ok()?;
    (hours <= 23 && mins <= 59)
        .then(|| sign.saturating_mul(hours.saturating_mul(3600).saturating_add(mins.saturating_mul(60))))
}

/// Load a `TZif` file.
fn read_tzif(path: &str) -> Option<Zone> {
    tzif::parse(&std::fs::read(path).ok()?)
}

#[cfg(test)]
mod tests;
