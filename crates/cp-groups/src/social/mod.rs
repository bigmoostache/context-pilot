//! Social state: who may act next in a group (T860 phase D).
//!
//! Replaces the per-agent `MY_TURN`/`THEIR_TURN` flag. Each group carries a
//! list of [`Substate`]s; an agent is *runnable* when any substate of any
//! group holds it. `Send` rewrites the sender's substates through [`Next`].

use cp_mod_utilities::cron::Cron;
use serde::{Deserialize, Serialize};

use crate::types::{Group, GroupKind, HUMAN};

pub mod departed;

/// One pending obligation inside a group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Substate {
    /// `agent_id` holds the talking stick: it must act.
    SimpleTurn {
        /// Agent expected to act.
        agent_id: String,
    },
    /// `agent_id` gets the stick back at `at_ms`, with a reminder message.
    ComingBack {
        /// Agent to wake.
        agent_id: String,
        /// Due time (epoch ms).
        at_ms: u64,
        /// Message posted into the group when due.
        message: String,
        /// Id of the [`Substate::Recurrent`] that armed it, if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<String>,
    },
    /// `agent_id` gets `message` on every match of `cron`: [`tick`] keeps one
    /// `ComingBack` armed for it. Created by `GIVE_RECURRING_TASK` and
    /// `Coucou(cron=…)` alike (both go through `coucou::schedule`).
    Recurrent {
        /// Coucou id (`coucou_<n>`), the handle `Coucou_delete` takes.
        id: String,
        /// Agent woken on each match.
        agent_id: String,
        /// Agent that scheduled it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        creator_agent_id: Option<String>,
        /// 5-field POSIX cron, host local time.
        cron: String,
        /// Reminder text.
        message: String,
    },
    /// Every `pending` agent must answer; then `origin_agent_id` takes the stick.
    Gather {
        /// Agents that have not answered yet.
        pending: Vec<String>,
        /// Agent that asked, resumed once everyone answered.
        origin_agent_id: String,
    },
    /// The first idle candidate takes the stick (resolved last in the tick).
    Dispatch {
        /// Eligible agents, in preference order.
        candidates: Vec<String>,
    },
}

impl Substate {
    /// Whether this substate makes `agent_id` runnable right now.
    #[must_use]
    pub fn holds(&self, agent_id: &str) -> bool {
        cp_macros::deref_match!(self, {
            Self::SimpleTurn { agent_id: ref a } => a == agent_id,
            Self::Gather { ref pending, .. } => pending.iter().any(|p| p == agent_id),
            Self::ComingBack { .. } | Self::Dispatch { .. } | Self::Recurrent { .. } => false,
        })
    }
}

/// What the sender of a message expects next (`Send.next`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    /// `STILL_MY_TURN`: the sender keeps working.
    StillMyTurn,
    /// `GIVE_TALKING_STICK_TO <id>`: hand the turn to one member (or `HUMAN`).
    GiveTalkingStickTo(String),
    /// `GATHER <ids>`: wait for every listed member, then resume the sender.
    Gather(Vec<String>),
    /// `DISPATCH <ids>`: the first idle listed member takes it.
    Dispatch(Vec<String>),
    /// `OWNER_DONE`: owner closes the exchange; no one is expected to act.
    OwnerDone,
    /// `GIVE_RECURRING_TASK(<id>, <cron>, <message>)`: `agent_id` (possibly the
    /// sender) gets `message` in this group on every match of the 5-field
    /// POSIX `cron`. Scheduled as a coucou by the caller of [`apply_send`].
    GiveRecurringTask {
        /// Agent woken on each match.
        agent_id: String,
        /// 5-field POSIX cron, host local time.
        cron: String,
        /// Reminder text.
        message: String,
    },
}

impl Next {
    /// Agents this `next` expects to act (empty for `STILL_MY_TURN` and
    /// `OWNER_DONE`).
    #[must_use]
    pub fn targets(&self) -> &[String] {
        cp_macros::deref_match!(self, {
            Self::StillMyTurn | Self::OwnerDone => &[],
            Self::GiveTalkingStickTo(ref id) | Self::GiveRecurringTask { agent_id: ref id, .. } => {
                std::slice::from_ref(id)
            }
            Self::Gather(ref ids) | Self::Dispatch(ref ids) => ids,
        })
    }
}

/// Syntax help returned with every parse error.
pub const NEXT_SYNTAX: &str = "expected one of: STILL_MY_TURN | GIVE_TALKING_STICK_TO <id> | \
     GATHER <id>, <id>... | DISPATCH <id>, <id>... | OWNER_DONE | \
     GIVE_RECURRING_TASK(<id>, <min hour day month weekday>, <message>)";

/// Keyword of [`Next::GiveRecurringTask`], parsed apart: its cron and message
/// hold spaces and punctuation the generic id splitter would mangle.
const RECURRING: &str = "GIVE_RECURRING_TASK";

/// Parse `(<id>, <cron>, <message>)` (parentheses optional).
fn parse_recurring(args: &str) -> Result<Next, String> {
    let trimmed = args.trim();
    let inner = trimmed.strip_prefix('(').and_then(|s| s.strip_suffix(')')).unwrap_or(trimmed);
    let mut parts = inner.splitn(3, ',').map(str::trim);
    let (Some(id), Some(cron), Some(message)) = (parts.next(), parts.next(), parts.next()) else {
        return Err(format!("{RECURRING} takes (<id>, <cron>, <message>); {NEXT_SYNTAX}"));
    };
    if id.is_empty() || message.is_empty() || cron.split_whitespace().count() != 5 {
        return Err(format!("{RECURRING} needs an id, a 5-field cron and a message; {NEXT_SYNTAX}"));
    }
    Ok(Next::GiveRecurringTask { agent_id: id.to_owned(), cron: cron.to_owned(), message: message.to_owned() })
}

/// Parse the `next` DSL. Keyword is case-insensitive; ids may be separated by
/// spaces or commas and wrapped in `()`, `[]` or `{}`.
///
/// # Errors
///
/// Unknown keyword, missing ids, or ids given to a keyword that takes none.
pub fn parse_next(raw: &str) -> Result<Next, String> {
    let head = raw.trim_start();
    if let Some(prefix) = head.get(..RECURRING.len())
        && prefix.eq_ignore_ascii_case(RECURRING)
    {
        return parse_recurring(head.get(RECURRING.len()..).unwrap_or_default());
    }
    let mut words = raw.split(|c: char| c.is_whitespace() || "()[]{},".contains(c)).filter(|w| !w.is_empty());
    let keyword = words.next().unwrap_or_default().to_ascii_uppercase();
    let ids: Vec<String> = words.map(str::to_owned).collect();
    let next = match keyword.as_str() {
        "STILL_MY_TURN" if ids.is_empty() => Next::StillMyTurn,
        "OWNER_DONE" if ids.is_empty() => Next::OwnerDone,
        "GIVE_TALKING_STICK_TO" => match <[String; 1]>::try_from(ids) {
            Ok([id]) => Next::GiveTalkingStickTo(id),
            Err(_) => return Err(format!("GIVE_TALKING_STICK_TO takes exactly one id; {NEXT_SYNTAX}")),
        },
        "GATHER" if !ids.is_empty() => Next::Gather(ids),
        "DISPATCH" if !ids.is_empty() => Next::Dispatch(ids),
        "STILL_MY_TURN" | "OWNER_DONE" => return Err(format!("{keyword} takes no ids; {NEXT_SYNTAX}")),
        "GATHER" | "DISPATCH" => return Err(format!("{keyword} needs agent ids; {NEXT_SYNTAX}")),
        _ => return Err(format!("unknown next `{raw}`; {NEXT_SYNTAX}")),
    };
    Ok(next)
}

/// Check `next` is legal for `sender` in `group`, without mutating anything.
///
/// # Errors
///
/// Sender or target not a member, `HUMAN` targeted by a non-owner, or
/// `OWNER_DONE` from a non-owner / in a human group.
pub fn validate(group: &Group, sender: &str, next: &Next) -> Result<(), String> {
    let is_member = |id: &str| group.member_ids.iter().any(|m| m == id);
    let is_owner = group.owner_agent_id == sender;
    if !is_member(sender) {
        return Err(format!("{sender} is not a member of {}", group.id));
    }
    if *next == Next::OwnerDone && !is_owner {
        return Err(format!("OWNER_DONE is owner-only ({} owns {})", group.owner_agent_id, group.id));
    }
    if *next == Next::OwnerDone && group.kind == GroupKind::Human {
        return Err("OWNER_DONE is refused in human groups: give the stick to HUMAN instead".to_owned());
    }
    let targets = next.targets();
    if matches!(*next, Next::GiveRecurringTask { .. }) && targets.iter().any(|t| t == HUMAN) {
        return Err(format!("{RECURRING} cannot target HUMAN"));
    }
    for t in targets {
        if !is_member(t) {
            return Err(format!("{t} is not a member of {} (members: {})", group.id, group.member_ids.join(", ")));
        }
        if t == HUMAN && !is_owner {
            return Err(format!("only the owner ({}) may give the stick to HUMAN", group.owner_agent_id));
        }
    }
    Ok(())
}

/// Drop `agent_id`'s obligations: its `SimpleTurn`s go, it leaves every
/// `Gather`, and a `Gather` left empty resumes its origin.
pub fn clear_turn(group: &mut Group, agent_id: &str) {
    let old = std::mem::take(&mut group.social);
    for sub in old {
        let kept = match sub {
            Substate::SimpleTurn { agent_id: a } if a == agent_id => None,
            Substate::Gather { mut pending, origin_agent_id } => {
                pending.retain(|p| p != agent_id);
                Some(if pending.is_empty() {
                    Substate::SimpleTurn { agent_id: origin_agent_id }
                } else {
                    Substate::Gather { pending, origin_agent_id }
                })
            }
            other @ (Substate::SimpleTurn { .. }
            | Substate::ComingBack { .. }
            | Substate::Dispatch { .. }
            | Substate::Recurrent { .. }) => Some(other),
        };
        if let Some(s) = kept {
            push_unique(group, s);
        }
    }
}

/// Give `agent_id` the stick (idempotent). `HUMAN` is never runnable: no-op.
pub fn give_turn(group: &mut Group, agent_id: &str) {
    if agent_id != HUMAN {
        push_unique(group, Substate::SimpleTurn { agent_id: agent_id.to_owned() });
    }
}

/// Append `sub` unless an equal substate is already present.
fn push_unique(group: &mut Group, sub: Substate) {
    if !group.social.contains(&sub) {
        group.social.push(sub);
    }
}

/// Apply a message sent by `sender` with `next`: validate, clear the sender's
/// obligations, then record what `next` asks for.
///
/// # Errors
///
/// See [`validate`]; the group is untouched on error.
pub fn apply_send(group: &mut Group, sender: &str, next: Next) -> Result<(), String> {
    validate(group, sender, &next)?;
    clear_turn(group, sender);
    let others = |ids: Vec<String>| -> Vec<String> { ids.into_iter().filter(|i| i != sender && i != HUMAN).collect() };
    match next {
        Next::StillMyTurn => give_turn(group, sender),
        Next::GiveTalkingStickTo(id) => give_turn(group, &id),
        Next::Gather(ids) => {
            let pending = others(ids);
            if pending.is_empty() {
                give_turn(group, sender);
            } else {
                push_unique(group, Substate::Gather { pending, origin_agent_id: sender.to_owned() });
            }
        }
        Next::Dispatch(ids) => {
            let candidates = others(ids);
            if !candidates.is_empty() {
                push_unique(group, Substate::Dispatch { candidates });
            }
        }
        Next::OwnerDone | Next::GiveRecurringTask { .. } => {}
    }
    Ok(())
}

/// Whether any substate of any group makes `agent_id` runnable.
#[must_use]
pub fn runnable(groups: &[Group], agent_id: &str) -> bool {
    groups.iter().any(|g| g.social.iter().any(|s| s.holds(agent_id)))
}

/// A `ComingBack` that fell due during [`tick`]: post `message` into the group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fired {
    /// Group the reminder belongs to.
    pub group_id: String,
    /// Agent woken.
    pub agent_id: String,
    /// Reminder text.
    pub message: String,
    /// Id of the [`Substate::Recurrent`] behind it (`Coucou_delete` handle).
    pub recurrent_id: Option<String>,
}

/// Advance time-driven substates once.
///
/// Due `ComingBack`s become `SimpleTurn`s (returned as [`Fired`]); each
/// `Recurrent` then re-arms its `ComingBack` at the next cron match after
/// `now_ms` (`offset_secs` = local UTC offset) unless one is still pending;
/// finally each `Dispatch` goes to its first candidate that `is_idle` and is
/// not already runnable anywhere. Dispatch runs last so every other relaunch
/// decision of the tick is already visible.
pub fn tick<Idle>(groups: &mut [Group], (now_ms, offset_secs): (u64, i32), is_idle: &Idle) -> Vec<Fired>
where
    Idle: Fn(&str) -> bool,
{
    let mut fired = Vec::new();
    for g in groups.iter_mut() {
        let old = std::mem::take(&mut g.social);
        for sub in old {
            let next = match sub {
                Substate::ComingBack { agent_id, at_ms, message, from } if at_ms <= now_ms => {
                    fired.push(Fired {
                        group_id: g.id.clone(),
                        agent_id: agent_id.clone(),
                        message,
                        recurrent_id: from,
                    });
                    Substate::SimpleTurn { agent_id }
                }
                other @ (Substate::SimpleTurn { .. }
                | Substate::ComingBack { .. }
                | Substate::Gather { .. }
                | Substate::Dispatch { .. }
                | Substate::Recurrent { .. }) => other,
            };
            push_unique(g, next);
        }
        arm_recurrents(g, now_ms, offset_secs);
    }
    resolve_dispatch(groups, is_idle);
    fired
}

/// Arm the next `ComingBack` of every `Recurrent` in `group` that has none
/// pending. A cron that no longer parses or never fires stays unarmed.
fn arm_recurrents(group: &mut Group, now_ms: u64, offset_secs: i32) {
    let armed = |id: &str, social: &[Substate]| {
        social.iter().any(|s| matches!(s, Substate::ComingBack { from: Some(f), .. } if f == id))
    };
    let due: Vec<Substate> = group
        .social
        .iter()
        .filter_map(|s| {
            let (id, agent_id, cron, message) = cp_macros::deref_match!(s, {
                Substate::Recurrent { ref id, ref agent_id, ref cron, ref message, .. } => (id, agent_id, cron, message),
                Substate::SimpleTurn { .. }
                | Substate::ComingBack { .. }
                | Substate::Gather { .. }
                | Substate::Dispatch { .. } => return None,
            });
            if armed(id, &group.social) {
                return None;
            }
            let at_ms = Cron::parse(cron).ok()?.next_after(now_ms, offset_secs)?;
            Some(Substate::ComingBack {
                agent_id: agent_id.clone(),
                at_ms,
                message: message.clone(),
                from: Some(id.clone()),
            })
        })
        .collect();
    for sub in due {
        push_unique(group, sub);
    }
}

/// Remove the `Recurrent` with `id` and its armed `ComingBack` from whichever
/// group holds it. Returns the removed `Recurrent` and its group id.
pub fn remove_recurrent(groups: &mut [Group], id: &str) -> Option<(String, Substate)> {
    for g in groups.iter_mut() {
        let Some(pos) = g.social.iter().position(|s| matches!(s, Substate::Recurrent { id: r, .. } if r == id)) else {
            continue;
        };
        let removed = g.social.remove(pos);
        g.social.retain(|s| !matches!(s, Substate::ComingBack { from: Some(f), .. } if f == id));
        return Some((g.id.clone(), removed));
    }
    None
}

/// Hand each `Dispatch` to its first idle, not-yet-runnable candidate.
fn resolve_dispatch<Idle>(groups: &mut [Group], is_idle: &Idle)
where
    Idle: Fn(&str) -> bool,
{
    for gi in 0..groups.len() {
        let pending: Vec<(usize, Vec<String>)> = groups.get(gi).map_or_else(Vec::new, |g| {
            g.social
                .iter()
                .enumerate()
                .filter_map(|(k, s)| {
                    cp_macros::deref_match!(s, {
                        Substate::Dispatch { ref candidates } => Some((k, candidates.clone())),
                        Substate::SimpleTurn { .. }
                        | Substate::ComingBack { .. }
                        | Substate::Gather { .. }
                        | Substate::Recurrent { .. } => None,
                    })
                })
                .collect()
        });
        for (k, candidates) in pending {
            let Some(pick) = candidates.into_iter().find(|c| is_idle(c) && !runnable(groups, c)) else { continue };
            if let Some(slot) = groups.get_mut(gi).and_then(|g| g.social.get_mut(k)) {
                *slot = Substate::SimpleTurn { agent_id: pick };
            }
        }
    }
}

#[cfg(test)]
mod tests;
