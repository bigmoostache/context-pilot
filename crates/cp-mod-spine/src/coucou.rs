//! Coucou tools — reminders held in the fleet-shared
//! [`CoucouRegistry`](crate::schedule::CoucouRegistry).
//!
//! `Coucou(message, agent_id?, group_id?, delay | datetime | cron)` schedules a
//! reminder for any agent (default: the caller), bound to a group both belong
//! to (default: the target's human group). One-shots use `delay` or a local
//! `datetime`; recurring ones a 5-field POSIX `cron`. `Coucou_delete` is
//! allowed for the creator or the target; a target deleting someone else's
//! coucou notifies the creator. [`schedule`] is shared with `Send`'s
//! `GIVE_RECURRING_TASK`.

use serde::{Deserialize, Serialize};

use cp_base::panels::now_ms;
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};
use cp_groups::types::{GroupKind, GroupsState};
use cp_mod_utilities::cron::Cron;
use cp_mod_utilities::time;

use crate::schedule::{CoucouRegistry, FiredCoucou};

/// Serializable coucou record, held in the fleet-shared [`CoucouRegistry`].
/// Also the shape of the legacy per-agent `pending_coucous` array, migrated
/// into the registry on boot.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Record {
    /// Unique coucou id (`coucou_<n>`); field name kept for legacy compat.
    pub watcher_id: String,
    /// The reminder message.
    pub message: String,
    /// When this coucou was registered (ms since epoch).
    pub registered_at_ms: u64,
    /// When the notification fires next (ms since epoch).
    pub fire_at_ms: u64,
    /// Target agent; `None` only for legacy coucous scheduled outside any agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(alias = "thread_id")]
    pub agent_id: Option<String>,
    /// Agent that scheduled it (`None` for pre-T860-G records).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creator_agent_id: Option<String>,
    /// Group the reminder belongs to: the target gets the turn there on fire.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    /// 5-field POSIX cron of a recurring coucou (host local time).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cron: Option<String>,
    /// Legacy fixed repeat interval (ms) of pre-cron `custom` recurrences; 0 = none.
    #[serde(default)]
    pub interval_ms: u64,
    /// Legacy recurrence label (`hourly`, `daily`, `weekly`, `every 30m`),
    /// rewritten to [`Self::cron`] on load where expressible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recurrence_label: Option<String>,
}

impl Record {
    /// The notification this coucou delivers when it comes due.
    #[must_use]
    pub fn fired(&self) -> FiredCoucou {
        let id = &self.watcher_id;
        let group = self.group_id.as_deref().map_or_else(String::new, |g| format!(" in {g}"));
        let from = self
            .creator_agent_id
            .as_deref()
            .filter(|c| self.agent_id.as_deref() != Some(*c))
            .map_or_else(String::new, |c| format!(", from {c}"));
        let repeat =
            self.cron.as_deref().map_or_else(String::new, |c| format!(" [cron `{c}`; Coucou_delete {id} stops it]"));
        FiredCoucou {
            id: id.clone(),
            agent_id: self.agent_id.clone(),
            group_id: self.group_id.clone(),
            description: format!("⏰ Coucou {id}{group}{from}! {}{repeat}", self.message),
        }
    }
}

/// When a coucou fires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum When {
    /// Once, at this epoch-ms time.
    At(u64),
    /// Recurring, on this 5-field POSIX cron (host local time).
    Cron(String),
}

/// A coucou to [`schedule`].
#[derive(Debug, Clone)]
pub struct Draft {
    /// Scheduling agent (`None` outside any agent).
    pub creator: Option<String>,
    /// Agent the reminder is for.
    pub target: String,
    /// Group to bind it to; `None` = the target's human group.
    pub group_id: Option<String>,
    /// Reminder text.
    pub message: String,
    /// One-shot time or cron.
    pub when: When,
}

/// The human group owned by `agent_id`.
fn human_group_of(groups: &GroupsState, agent_id: &str) -> Option<String> {
    groups.groups.iter().find(|g| g.kind == GroupKind::Human && g.owner_agent_id == agent_id).map(|g| g.id.clone())
}

/// The group `draft` binds to: the requested one (target and creator must
/// both be members) or the target's human group (which proves it exists).
fn resolve_group(groups: &GroupsState, draft: &Draft) -> Result<String, String> {
    let Some(gid) = draft.group_id.as_deref() else {
        return human_group_of(groups, &draft.target)
            .ok_or_else(|| format!("unknown agent {} (it has no human group)", draft.target));
    };
    let group = groups.find(gid).ok_or_else(|| format!("group {gid} not found"))?;
    for who in std::iter::once(draft.target.as_str()).chain(draft.creator.as_deref()) {
        if !group.member_ids.iter().any(|m| m == who) {
            return Err(format!("{who} is not a member of {gid} (members: {})", group.member_ids.join(", ")));
        }
    }
    Ok(gid.to_owned())
}

/// Validate `draft` and register it; returns the stored record.
///
/// # Errors
///
/// Unknown target or group, a non-member target/creator, a past time, or an
/// invalid / never-firing cron.
pub fn schedule(state: &mut State, draft: Draft) -> Result<Record, String> {
    let group_id = resolve_group(GroupsState::get(state), &draft)?;
    let now = now_ms();
    let (fire_at_ms, cron) = match draft.when {
        When::At(at) if at <= now => return Err("that time is already past".to_owned()),
        When::At(at) => (at, None),
        When::Cron(expr) => {
            let next = Cron::parse(&expr)?
                .next_after(now, time::local_utc_offset_secs())
                .ok_or_else(|| format!("cron `{expr}` never fires"))?;
            (next, Some(expr.split_whitespace().collect::<Vec<_>>().join(" ")))
        }
    };
    let registry = CoucouRegistry::get_mut(state);
    let record = Record {
        watcher_id: registry.alloc_id(),
        message: draft.message,
        registered_at_ms: now,
        fire_at_ms,
        agent_id: Some(draft.target),
        creator_agent_id: draft.creator,
        group_id: Some(group_id),
        cron,
        ..Record::default()
    };
    registry.pending.push(record.clone());
    Ok(record)
}

/// Parse a duration like `30s`, `5m`, `1h30m` or a bare number of seconds.
fn parse_duration_ms(s: &str) -> Result<u64, String> {
    let trimmed = s.trim();
    if let Ok(secs) = trimmed.parse::<u64>() {
        return if secs == 0 {
            Err("duration must be greater than 0".to_owned())
        } else {
            Ok(secs.saturating_mul(1000))
        };
    }
    let mut total_ms: u64 = 0;
    let mut current = String::new();
    for ch in trimmed.chars() {
        if ch.is_ascii_digit() {
            current.push(ch);
            continue;
        }
        let val: u64 = current.parse().map_err(|_e| format!("invalid duration '{trimmed}'"))?;
        current.clear();
        let unit: u64 = match ch {
            'h' | 'H' => 3_600_000,
            'm' | 'M' => 60_000,
            's' | 'S' => 1_000,
            _ => return Err(format!("unknown duration unit '{ch}' (use h/m/s)")),
        };
        total_ms = total_ms.saturating_add(val.saturating_mul(unit));
    }
    if !current.is_empty() {
        let val: u64 = current.parse().map_err(|_e| format!("invalid duration '{trimmed}'"))?;
        total_ms = total_ms.saturating_add(val.saturating_mul(1_000));
    }
    if total_ms == 0 { Err("duration must be greater than 0".to_owned()) } else { Ok(total_ms) }
}

/// Parse `YYYY-MM-DDTHH:MM[:SS]` as host local time (a trailing `Z` = UTC).
fn parse_datetime_ms(s: &str) -> Result<u64, String> {
    let joined = s.trim().replace(' ', "T");
    let utc = joined.ends_with('Z');
    let bare = joined.trim_end_matches('Z');
    let padded = if bare.matches(':').count() == 1 { format!("{bare}:00") } else { bare.to_owned() };
    let parsed = if utc {
        time::parse_rfc3339_to_epoch_ms(&format!("{padded}Z"))
    } else {
        time::parse_local_datetime_to_epoch_ms(&padded)
    };
    let ms = parsed.ok_or_else(|| format!("invalid datetime '{s}' (expected YYYY-MM-DDTHH:MM:SS, local time)"))?;
    u64::try_from(ms).map_err(|_e| "datetime is before 1970".to_owned())
}

/// Trimmed non-empty string argument `key`.
fn str_arg<'tool>(tool: &'tool ToolUse, key: &str) -> Option<&'tool str> {
    tool.input.get(key).and_then(serde_json::Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

/// Exactly one of `delay`, `datetime`, `cron`.
fn parse_when(tool: &ToolUse, now: u64) -> Result<When, String> {
    match (str_arg(tool, "delay"), str_arg(tool, "datetime"), str_arg(tool, "cron")) {
        (Some(d), None, None) => Ok(When::At(now.saturating_add(parse_duration_ms(d)?))),
        (None, Some(dt), None) => Ok(When::At(parse_datetime_ms(dt)?)),
        (None, None, Some(c)) => Ok(When::Cron(c.to_owned())),
        _ => Err("give exactly one of delay, datetime or cron".to_owned()),
    }
}

/// One-line summary of a scheduled record.
fn describe(r: &Record) -> String {
    let at = i64::try_from(r.fire_at_ms).ok().and_then(time::epoch_ms_to_local_ymd_hms).unwrap_or_default();
    let target = r.agent_id.as_deref().unwrap_or("?");
    let group = r.group_id.as_deref().unwrap_or("?");
    let repeat = r.cron.as_deref().map_or_else(String::new, |c| format!(", repeats on cron `{c}`"));
    format!("Scheduled {} for {target} in {group}: next {at} (local){repeat}. Message: \"{}\"", r.watcher_id, r.message)
}

/// `Coucou` body; `Ok` = result text.
fn try_coucou(tool: &ToolUse, state: &mut State) -> Result<String, String> {
    let message = str_arg(tool, "message").ok_or("Coucou requires a message")?.to_owned();
    let creator = state.executing_agent_id().map(str::to_owned);
    let target = str_arg(tool, "agent_id")
        .map(str::to_owned)
        .or_else(|| creator.clone())
        .ok_or("Coucou needs agent_id when no agent is executing")?;
    let when = parse_when(tool, now_ms())?;
    let group_id = str_arg(tool, "group_id").map(str::to_owned);
    let record = schedule(state, Draft { creator, target, group_id, message, when })?;
    Ok(describe(&record))
}

/// `Coucou_delete` body; `Ok` = result text.
fn try_delete(tool: &ToolUse, state: &mut State) -> Result<String, String> {
    let id = str_arg(tool, "coucou_id").ok_or("Coucou_delete requires coucou_id")?.to_owned();
    let caller = state.executing_agent_id().map(str::to_owned);
    let registry = CoucouRegistry::get_mut(state);
    let record = registry
        .pending
        .iter()
        .find(|c| c.watcher_id == id)
        .cloned()
        .ok_or_else(|| format!("coucou {id} not found"))?;
    let is_caller = |who: Option<&String>| who.is_some() && who == caller.as_ref();
    let allowed =
        record.agent_id.is_none() || is_caller(record.creator_agent_id.as_ref()) || is_caller(record.agent_id.as_ref());
    if !allowed {
        return Err(format!(
            "only the creator ({}) or the target ({}) may delete {id}",
            record.creator_agent_id.as_deref().unwrap_or("unknown"),
            record.agent_id.as_deref().unwrap_or("none"),
        ));
    }
    let _removed = registry.cancel(&id);
    if let Some(creator) = record.creator_agent_id.filter(|c| Some(c) != caller.as_ref()) {
        let by = caller.as_deref().unwrap_or("someone");
        registry.notices.push(FiredCoucou {
            id: format!("{id}_deleted"),
            agent_id: Some(creator),
            group_id: record.group_id,
            description: format!("🗑 {by} deleted your coucou {id} (\"{}\")", record.message),
        });
    }
    Ok(format!("Deleted coucou {id}"))
}

/// Execute `Coucou`: schedule a reminder.
pub(crate) fn execute_coucou(tool: &ToolUse, state: &mut State) -> ToolResult {
    match try_coucou(tool, state) {
        Ok(text) => ToolResult::new(tool.id.clone(), text, false),
        Err(e) => ToolResult::new(tool.id.clone(), e, true),
    }
}

/// Execute `Coucou_delete`: remove a reminder (creator or target only).
pub(crate) fn execute_coucou_delete(tool: &ToolUse, state: &mut State) -> ToolResult {
    match try_delete(tool, state) {
        Ok(text) => ToolResult::new(tool.id.clone(), text, false),
        Err(e) => ToolResult::new(tool.id.clone(), e, true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(parse_duration_ms("1h30m"), Ok(5_400_000));
        assert_eq!(parse_duration_ms("90"), Ok(90_000));
        assert_eq!(parse_duration_ms("0").ok(), None);
        assert_eq!(parse_duration_ms("5x").ok(), None);
    }

    #[test]
    fn fired_text_names_group_creator_and_cron() {
        let r = Record {
            watcher_id: "coucou_3".into(),
            message: "check".into(),
            agent_id: Some("A2".into()),
            creator_agent_id: Some("A1".into()),
            group_id: Some("G9".into()),
            cron: Some("0 * * * *".into()),
            ..Record::default()
        };
        let f = r.fired();
        assert_eq!(f.group_id.as_deref(), Some("G9"));
        assert!(f.description.starts_with("\u{23f0} Coucou coucou_3 in G9, from A1! check [cron `0 * * * *`"));
    }
}
