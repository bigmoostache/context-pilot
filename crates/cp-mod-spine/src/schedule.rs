//! Fleet-shared coucou registry.
//!
//! Coucous used to live as `CoucouWatcher`s in each agent's own
//! `WatcherRegistry`, which is only polled while that agent is stepped — so a
//! background agent idle on THEIR_TURN never fired its reminders. They now
//! live here, ONE instance for the whole fleet (in `shared_module_data`),
//! polled once per tick by the app loop and delivered into each coucou's
//! target agent inbox.

use serde::{Deserialize, Serialize};

use cp_base::state::runtime::State;
use cp_mod_utilities::cron::Cron;
use cp_mod_utilities::time;

use crate::coucou::Record;

/// Every pending coucou of the fleet plus the id counter.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CoucouRegistry {
    /// Pending coucous, in scheduling order.
    pub pending: Vec<Record>,
    /// Next numeric suffix for `coucou_<n>` ids. Persisted, so ids stay unique
    /// across reloads (a process-local counter restarted at 0 and collided).
    pub next_id: u64,
    /// Immediate notices (e.g. "your coucou was deleted"), delivered with the
    /// next [`Self::take_due`]. Transient.
    #[serde(skip)]
    pub notices: Vec<FiredCoucou>,
}

/// A coucou that came due this tick, ready for delivery.
#[derive(Debug, Clone)]
pub struct FiredCoucou {
    /// Coucou id (`coucou_<n>`), used as the notification source for dedup.
    pub id: String,
    /// Target agent; `None` = whichever agent is focused.
    pub agent_id: Option<String>,
    /// Group in which the target gets the turn; `None` = its human group.
    pub group_id: Option<String>,
    /// Notification text.
    pub description: String,
}

/// Cron equivalent of a legacy `hourly`/`daily`/`weekly` recurrence, anchored
/// on the local time of `fire_at_ms` (with the UTC offset in effect at that
/// instant, so a DST change between then and now doesn't shift the hour).
fn legacy_cron(label: &str, fire_at_ms: u64) -> Option<String> {
    let secs = i64::try_from(fire_at_ms.checked_div(1000)?).ok()?;
    cron_from_local(label, secs, time::utc_offset_at_secs(secs))
}

/// Cron for `label` anchored on `secs` shifted by `offset_secs` (east of UTC).
fn cron_from_local(label: &str, secs: i64, offset_secs: i32) -> Option<String> {
    let local = secs.saturating_add(i64::from(offset_secs));
    let minute = local.rem_euclid(3600).div_euclid(60);
    let hour = local.rem_euclid(86_400).div_euclid(3600);
    // 1970-01-01 was a Thursday (4).
    let weekday = local.div_euclid(86_400).saturating_add(4).rem_euclid(7);
    match label {
        "hourly" => Some(format!("{minute} * * * *")),
        "daily" => Some(format!("{minute} {hour} * * *")),
        "weekly" => Some(format!("{minute} {hour} * * {weekday}")),
        _ => None,
    }
}

impl CoucouRegistry {
    /// Shared registry from `state`.
    #[must_use]
    pub fn get(state: &State) -> &Self {
        state.ext::<Self>()
    }

    /// Mutable shared registry from `state`.
    pub fn get_mut(state: &mut State) -> &mut Self {
        state.ext_mut::<Self>()
    }

    /// Allocate a fresh `coucou_<n>` id.
    pub fn alloc_id(&mut self) -> String {
        let id = format!("coucou_{}", self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// Raise `next_id` above every pending id, so restored or migrated records
    /// can never collide with freshly allocated ones.
    pub fn reseed_counter(&mut self) {
        let max_seen = self
            .pending
            .iter()
            .filter_map(|c| c.watcher_id.strip_prefix("coucou_")?.parse::<u64>().ok())
            .max()
            .map_or(0, |n| n.saturating_add(1));
        self.next_id = self.next_id.max(max_seen);
    }

    /// Rewrite legacy `hourly`/`daily`/`weekly` recurrences as cron (idempotent).
    /// `custom` intervals have no cron equivalent and keep `interval_ms`.
    pub fn migrate_legacy_recurrences(&mut self) {
        for c in &mut self.pending {
            if c.cron.is_none()
                && let Some(cron) = c.recurrence_label.as_deref().and_then(|l| legacy_cron(l, c.fire_at_ms))
            {
                c.cron = Some(cron);
                c.interval_ms = 0;
                c.recurrence_label = None;
            }
        }
    }

    /// Remove the coucou with `id`. Returns whether one was removed.
    pub fn cancel(&mut self, id: &str) -> bool {
        let before = self.pending.len();
        self.pending.retain(|c| c.watcher_id != id);
        self.pending.len() < before
    }

    /// Whether any coucou targets `agent_id` (or is unscoped). Drives the
    /// WAITING badge of the focused agent.
    #[must_use]
    pub fn has_pending_for(&self, agent_id: Option<&str>) -> bool {
        self.pending.iter().any(|c| c.agent_id.is_none() || c.agent_id.as_deref() == agent_id)
    }

    /// Pop every notice plus every coucou due at `now`: one-shots are removed,
    /// cron ones re-armed at their next match, legacy intervals at `now + interval`.
    pub fn take_due(&mut self, now: u64) -> Vec<FiredCoucou> {
        let mut fired = std::mem::take(&mut self.notices);
        let offset = time::local_utc_offset_secs();
        self.pending.retain_mut(|c| {
            if now < c.fire_at_ms {
                return true;
            }
            let mut notice = c.fired();
            let next = match c.cron.as_deref() {
                Some(expr) => match Cron::parse(expr) {
                    Ok(cron) => cron.next_after(now, offset),
                    Err(err) => {
                        notice.description =
                            format!("{} [invalid cron `{expr}` ({err}): reminder removed]", notice.description);
                        None
                    }
                },
                None => (c.interval_ms > 0).then(|| now.saturating_add(c.interval_ms)),
            };
            fired.push(notice);
            next.inspect(|at| c.fire_at_ms = *at).is_some()
        });
        fired
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(id: &str, fire_at_ms: u64, interval_ms: u64) -> Record {
        Record {
            watcher_id: id.to_owned(),
            message: "m".to_owned(),
            fire_at_ms,
            agent_id: Some("A1".to_owned()),
            interval_ms,
            ..Record::default()
        }
    }

    #[test]
    fn take_due_drops_one_shot_and_rearms_recurrent() {
        let mut cron = data("coucou_2", 10, 0);
        cron.cron = Some("* * * * *".to_owned());
        let mut reg = CoucouRegistry {
            pending: vec![data("coucou_0", 10, 0), data("coucou_1", 10, 100), cron],
            next_id: 3,
            notices: Vec::new(),
        };
        assert!(reg.take_due(5).is_empty());
        let fired = reg.take_due(120_000);
        assert_eq!(fired.len(), 3);
        let next: Vec<u64> = reg.pending.iter().map(|c| c.fire_at_ms).collect();
        assert_eq!(next, vec![120_100, 180_000]);
    }

    #[test]
    fn legacy_labels_become_cron() {
        let mut daily = data("coucou_0", 3_600_000 * 9 + 60_000 * 5, 86_400_000);
        daily.recurrence_label = Some("daily".to_owned());
        let mut reg = CoucouRegistry { pending: vec![daily], next_id: 1, notices: Vec::new() };
        reg.migrate_legacy_recurrences();
        let first = reg.pending.first();
        assert_eq!(first.map(|c| c.interval_ms), Some(0));
        assert!(first.and_then(|c| c.cron.as_deref()).is_some_and(|e| e.ends_with(" * * *")));
    }

    #[test]
    fn legacy_cron_uses_offset_of_the_original_instant() {
        // Thu 1970-01-01 23:05 UTC.
        let secs = 23 * 3600 + 5 * 60;
        assert_eq!(cron_from_local("daily", secs, 0).as_deref(), Some("5 23 * * *"));
        // Winter (+1h) vs summer (+2h) offsets land on different local days/hours.
        assert_eq!(cron_from_local("weekly", secs, 3600).as_deref(), Some("5 0 * * 5"));
        assert_eq!(cron_from_local("weekly", secs, 7200).as_deref(), Some("5 1 * * 5"));
        assert_eq!(cron_from_local("hourly", secs, -1800).as_deref(), Some("35 * * * *"));
        assert!(cron_from_local("monthly", secs, 0).is_none());
    }

    #[test]
    fn invalid_cron_is_reported_not_silently_dropped() {
        let mut bad = data("coucou_0", 10, 0);
        bad.cron = Some("not a cron".to_owned());
        let mut reg = CoucouRegistry { pending: vec![bad], next_id: 1, notices: Vec::new() };
        let fired = reg.take_due(20);
        assert!(reg.pending.is_empty());
        assert!(fired.first().is_some_and(|f| f.description.contains("invalid cron")));
    }

    #[test]
    fn reseed_avoids_collisions() {
        let mut reg = CoucouRegistry { pending: vec![data("coucou_7", 0, 0)], next_id: 0, notices: Vec::new() };
        reg.reseed_counter();
        assert_eq!(reg.alloc_id(), "coucou_8");
    }
}
