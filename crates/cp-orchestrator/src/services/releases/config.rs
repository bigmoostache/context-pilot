//! Release-manager persistent configuration (`releases/config.json`) —
//! architecture + active tag (legacy) extended with the auto-update policy
//! knobs (update-policy v3 decision 3): mode, channel, poll cadence, and the
//! box-local maintenance window.
//!
//! Migration is idempotent by construction: every new field is
//! `#[serde(default)]`-ed, so a legacy `config.json` (arch + active tag only)
//! loads with `update_mode=auto`, `channel=stable`, a 6-hour poll and the
//! 03:00–05:00 window — and is rewritten with the full shape on the next
//! persist. No error path, no version bump.

use serde::{Deserialize, Serialize};

/// How the box treats a published update (update-policy §5.5/§5.9).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateMode {
    /// Apply automatically inside the maintenance window (the default).
    #[default]
    Auto,
    /// Surface "update available", apply only on an explicit admin action.
    Manual,
    /// Check + surface state, never apply — the escape hatch.
    Paused,
}

impl UpdateMode {
    /// Stable lowercase name (mirrors the serde encoding).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Manual => "manual",
            Self::Paused => "paused",
        }
    }
}

/// A daily box-local time window, `"HH:MM"` bounds, end exclusive. A window
/// with `start > end` wraps past midnight (e.g. `23:00`–`01:00`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaintenanceWindow {
    /// Inclusive opening bound, `"HH:MM"` box-local.
    pub start: String,
    /// Exclusive closing bound, `"HH:MM"` box-local.
    pub end: String,
}

impl Default for MaintenanceWindow {
    fn default() -> Self {
        Self { start: "03:00".to_owned(), end: "05:00".to_owned() }
    }
}

impl MaintenanceWindow {
    /// Whether `now_minutes` (minutes since box-local midnight) falls inside
    /// the window. Malformed bounds fail closed (never in the window).
    #[must_use]
    pub fn contains(&self, now_minutes: u16) -> bool {
        let (Some(start), Some(end)) = (parse_hhmm(&self.start), parse_hhmm(&self.end)) else {
            return false;
        };
        if start <= end { (start..end).contains(&now_minutes) } else { now_minutes >= start || now_minutes < end }
    }

    /// Both bounds parse as `HH:MM`.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        parse_hhmm(&self.start).is_some() && parse_hhmm(&self.end).is_some()
    }

    /// Minutes from `now_minutes` until the window next opens (0 inside it).
    #[must_use]
    pub fn minutes_until_open(&self, now_minutes: u16) -> u16 {
        if self.contains(now_minutes) {
            return 0;
        }
        let Some(start) = parse_hhmm(&self.start) else {
            return u16::MAX; // fail closed: effectively "not tonight"
        };
        if start > now_minutes { start - now_minutes } else { 24 * 60 - now_minutes + start }
    }
}

/// Parse `"HH:MM"` into minutes since midnight.
pub(crate) fn parse_hhmm(s: &str) -> Option<u16> {
    let (h, m) = s.split_once(':')?;
    if h.len() != 2 || m.len() != 2 {
        return None;
    }
    let hours: u16 = h.parse().ok()?;
    let minutes: u16 = m.parse().ok()?;
    if hours > 23 || minutes > 59 {
        return None;
    }
    Some(hours * 60 + minutes)
}

/// Serde default: the `stable` channel.
fn default_channel() -> String {
    "stable".to_owned()
}

/// Serde default: poll the channel every 6 hours (plus the boot poll).
fn default_poll_interval_hours() -> u32 {
    6
}

/// On-disk configuration for the release manager.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ReleaseConfig {
    /// Platform architecture string (e.g. `"linux-aarch64"`).
    pub(crate) arch: String,
    /// `true` when `arch` was auto-detected, `false` when manually set.
    pub(crate) arch_auto: bool,
    /// Tag of the currently selected (active) release, if any.
    pub(crate) active_tag: Option<String>,
    /// Auto-update posture (default `auto` — update-policy v3 decision 3).
    #[serde(default)]
    pub(crate) update_mode: UpdateMode,
    /// Channel this box follows (only `stable` is published today).
    #[serde(default = "default_channel")]
    pub(crate) channel: String,
    /// Hours between channel polls (a boot poll always happens too).
    #[serde(default = "default_poll_interval_hours")]
    pub(crate) poll_interval_hours: u32,
    /// Box-local nightly window auto-applies are confined to.
    #[serde(default)]
    pub(crate) window: MaintenanceWindow,
}

impl Default for ReleaseConfig {
    fn default() -> Self {
        Self {
            arch: super::detect_arch(),
            arch_auto: true,
            active_tag: None,
            update_mode: UpdateMode::default(),
            channel: default_channel(),
            poll_interval_hours: default_poll_interval_hours(),
            window: MaintenanceWindow::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default nightly window: inclusive start, exclusive end.
    #[test]
    fn window_contains_default() {
        let w = MaintenanceWindow::default();
        assert!(!w.contains(2 * 60 + 59));
        assert!(w.contains(3 * 60), "start is inclusive");
        assert!(w.contains(4 * 60 + 59));
        assert!(!w.contains(5 * 60), "end is exclusive");
        assert!(!w.contains(12 * 60));
    }

    /// A window that spans midnight wraps correctly; malformed bounds fail
    /// closed.
    #[test]
    fn window_wraps_and_fails_closed() {
        let w = MaintenanceWindow { start: "23:00".to_owned(), end: "01:00".to_owned() };
        assert!(w.contains(23 * 60 + 30));
        assert!(w.contains(0));
        assert!(!w.contains(1 * 60));
        assert!(!w.contains(12 * 60));

        let bad = MaintenanceWindow { start: "25:00".to_owned(), end: "05:00".to_owned() };
        assert!(!bad.is_valid());
        assert!(!bad.contains(4 * 60), "malformed window is never open");
    }

    /// Time until the window opens, same-day and across midnight.
    #[test]
    fn window_minutes_until_open() {
        let w = MaintenanceWindow::default(); // 03:00–05:00
        assert_eq!(w.minutes_until_open(3 * 60 + 30), 0, "already open");
        assert_eq!(w.minutes_until_open(60), 120, "01:00 → 03:00");
        assert_eq!(w.minutes_until_open(23 * 60), 4 * 60, "23:00 → 03:00 next day");
    }
}
