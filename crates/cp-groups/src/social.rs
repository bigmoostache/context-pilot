//! Social state: who may act next in a group (T860 phase D).
//!
//! Replaces the per-agent `MY_TURN`/`THEIR_TURN` flag. Each group carries a
//! list of [`Substate`]s; an agent is *runnable* when any substate of any
//! group holds it. `Send` rewrites the sender's substates through [`Next`].

use serde::{Deserialize, Serialize};

use crate::types::{Group, GroupKind, HUMAN};

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
            Self::ComingBack { .. } | Self::Dispatch { .. } => false,
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
}

/// Syntax help returned with every parse error.
pub const NEXT_SYNTAX: &str = "expected one of: STILL_MY_TURN | GIVE_TALKING_STICK_TO <id> | \
     GATHER <id>, <id>... | DISPATCH <id>, <id>... | OWNER_DONE";

/// Parse the `next` DSL. Keyword is case-insensitive; ids may be separated by
/// spaces or commas and wrapped in `()`, `[]` or `{}`.
///
/// # Errors
///
/// Unknown keyword, missing ids, or ids given to a keyword that takes none.
pub fn parse_next(raw: &str) -> Result<Next, String> {
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
    let targets: &[String] = cp_macros::deref_match!(next, {
        Next::StillMyTurn | Next::OwnerDone => &[],
        Next::GiveTalkingStickTo(ref id) => std::slice::from_ref(id),
        Next::Gather(ref ids) | Next::Dispatch(ref ids) => ids,
    });
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
            other @ (Substate::SimpleTurn { .. } | Substate::ComingBack { .. } | Substate::Dispatch { .. }) => {
                Some(other)
            }
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
        Next::OwnerDone => {}
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
}

/// Advance time-driven substates once.
///
/// Due `ComingBack`s become `SimpleTurn`s
/// (returned as [`Fired`]); then each `Dispatch` goes to its first candidate
/// that `is_idle` and is not already runnable anywhere. Dispatch runs last so
/// every other relaunch decision of the tick is already visible.
pub fn tick<Idle>(groups: &mut [Group], now_ms: u64, is_idle: &Idle) -> Vec<Fired>
where
    Idle: Fn(&str) -> bool,
{
    let mut fired = Vec::new();
    for g in groups.iter_mut() {
        let old = std::mem::take(&mut g.social);
        for sub in old {
            let next = match sub {
                Substate::ComingBack { agent_id, at_ms, message } if at_ms <= now_ms => {
                    fired.push(Fired { group_id: g.id.clone(), agent_id: agent_id.clone(), message });
                    Substate::SimpleTurn { agent_id }
                }
                other @ (Substate::SimpleTurn { .. }
                | Substate::ComingBack { .. }
                | Substate::Gather { .. }
                | Substate::Dispatch { .. }) => other,
            };
            push_unique(g, next);
        }
    }
    resolve_dispatch(groups, is_idle);
    fired
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
                        Substate::SimpleTurn { .. } | Substate::ComingBack { .. } | Substate::Gather { .. } => None,
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
mod tests {
    use super::*;
    use crate::types::GroupsState;

    /// Custom group G1 owned by A1 with members A1, A2, A3 and HUMAN.
    fn group() -> Group {
        let mut s = GroupsState::new();
        let id = s.create(Group {
            name: "t".into(),
            owner_agent_id: "A1".into(),
            member_ids: ["A1", "A2", "A3", HUMAN].map(String::from).to_vec(),
            kind: GroupKind::Custom,
            ..Group::default()
        });
        s.find(&id).cloned().expect("created")
    }

    #[test]
    fn parses_dsl_forms() {
        assert_eq!(parse_next("still_my_turn"), Ok(Next::StillMyTurn));
        assert_eq!(parse_next("GIVE_TALKING_STICK_TO {A2}"), Ok(Next::GiveTalkingStickTo("A2".into())));
        assert_eq!(parse_next("GATHER(A2, A3)"), Ok(Next::Gather(vec!["A2".into(), "A3".into()])));
        assert_eq!(parse_next("DISPATCH [A2 A3]"), Ok(Next::Dispatch(vec!["A2".into(), "A3".into()])));
    }

    #[test]
    fn rejects_bad_dsl() {
        for bad in ["GATHER", "OWNER_DONE A2", "GIVE_TALKING_STICK_TO A2 A3", "NOPE"] {
            let err = parse_next(bad).expect_err(bad);
            assert!(err.contains("expected one of"), "{err}");
        }
    }

    #[test]
    fn gather_resumes_origin_when_all_answered() {
        let mut g = group();
        apply_send(&mut g, "A1", Next::Gather(vec!["A2".into(), "A3".into()])).expect("ok");
        let gs = std::slice::from_ref(&g);
        assert!(runnable(gs, "A2") && runnable(gs, "A3") && !runnable(gs, "A1"));
        let err = apply_send(&mut g, "A2", Next::OwnerDone).expect_err("A2 not owner");
        assert!(err.contains("owner-only"), "{err}");
        apply_send(&mut g, "A2", Next::GiveTalkingStickTo("A3".into())).expect("ok");
        apply_send(&mut g, "A3", Next::GiveTalkingStickTo("A1".into())).expect("ok");
        assert_eq!(g.social, vec![Substate::SimpleTurn { agent_id: "A1".into() }]);
    }

    #[test]
    fn human_stick_is_owner_only_and_not_runnable() {
        let mut g = group();
        let err = apply_send(&mut g, "A2", Next::GiveTalkingStickTo(HUMAN.into())).expect_err("non-owner");
        assert!(err.contains("only the owner"), "{err}");
        apply_send(&mut g, "A1", Next::StillMyTurn).expect("ok");
        apply_send(&mut g, "A1", Next::GiveTalkingStickTo(HUMAN.into())).expect("ok");
        assert!(g.social.is_empty());
        let outsider = apply_send(&mut g, "A9", Next::StillMyTurn).expect_err("non-member");
        assert!(outsider.contains("not a member"), "{outsider}");
    }

    #[test]
    fn tick_fires_coming_back_then_dispatches_to_idle_non_runnable() {
        let mut g = group();
        g.social.push(Substate::ComingBack { agent_id: "A2".into(), at_ms: 5, message: "wake".into() });
        apply_send(&mut g, "A1", Next::Dispatch(vec!["A2".into(), "A3".into()])).expect("ok");
        let mut gs = vec![g];
        let fired = tick(&mut gs, 10, &|_: &str| true);
        assert_eq!(fired.len(), 1);
        let g0 = gs.first().expect("group");
        assert!(g0.social.contains(&Substate::SimpleTurn { agent_id: "A2".into() }));
        assert!(g0.social.contains(&Substate::SimpleTurn { agent_id: "A3".into() }), "A2 busy, so A3");
    }
}
