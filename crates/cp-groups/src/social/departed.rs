//! Members leaving: drop their obligations, and the groups that die with them.

use super::{Substate, clear_turn};
use crate::types::{Group, GroupKind, HUMAN};

/// Drop every social obligation naming `agent_id` (it left the group). A
/// dispatch left with no candidate is dropped: empty would mean "everyone".
pub fn forget_member(group: &mut Group, agent_id: &str) {
    clear_turn(group, agent_id);
    group.social = std::mem::take(&mut group.social).into_iter().filter_map(|s| keep_without(s, agent_id)).collect();
}

/// `s` with `gone` removed, or `None` when nothing is left of it.
fn keep_without(s: Substate, gone: &str) -> Option<Substate> {
    match s {
        Substate::ComingBack { agent_id, at_ms, message, from } => {
            (agent_id != gone).then_some(Substate::ComingBack { agent_id, at_ms, message, from })
        }
        Substate::Recurrent { id, agent_id, creator_agent_id, cron, message } => {
            (agent_id != gone).then_some(Substate::Recurrent { id, agent_id, creator_agent_id, cron, message })
        }
        Substate::Dispatch { mut candidates } => {
            let had = !candidates.is_empty();
            candidates.retain(|c| c != gone);
            (!had || !candidates.is_empty()).then_some(Substate::Dispatch { candidates })
        }
        other @ (Substate::SimpleTurn { .. } | Substate::Gather { .. }) => Some(other),
    }
}

/// Apply the departure of every agent `alive` rejects.
///
/// A group goes when its owner is gone (no one could ever delete it), and a
/// coordination group goes when any member is gone. In surviving custom groups
/// the departed simply leave. Returns the ids of the removed groups, whose
/// message files the caller deletes.
pub fn purge(groups: &mut Vec<Group>, alive_agent: &dyn Fn(&str) -> bool) -> Vec<String> {
    let alive = |id: &str| id == HUMAN || alive_agent(id);
    let mut removed = Vec::new();
    groups.retain(|g| {
        let dies =
            !alive(&g.owner_agent_id) || (g.kind == GroupKind::Coordination && g.member_ids.iter().any(|m| !alive(m)));
        if dies {
            removed.push(g.id.clone());
        }
        !dies
    });
    for g in groups.iter_mut() {
        let gone: Vec<String> = g.member_ids.iter().filter(|m| !alive(m)).cloned().collect();
        for m in &gone {
            forget_member(g, m);
        }
        g.member_ids.retain(|m| !gone.contains(m));
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(id: &str, kind: GroupKind, owner: &str, members: &[&str]) -> Group {
        Group {
            id: id.into(),
            kind,
            owner_agent_id: owner.into(),
            member_ids: members.iter().map(|m| (*m).to_owned()).collect(),
            ..Group::default()
        }
    }

    #[test]
    fn forget_member_prunes_obligations() {
        let mut g = Group {
            social: vec![
                Substate::SimpleTurn { agent_id: "A2".into() },
                Substate::ComingBack { agent_id: "A2".into(), at_ms: 1, message: String::new(), from: None },
                Substate::Dispatch { candidates: vec!["A2".into()] },
                Substate::Dispatch { candidates: vec!["A2".into(), "A3".into()] },
            ],
            ..Group::default()
        };
        forget_member(&mut g, "A2");
        assert_eq!(g.social, vec![Substate::Dispatch { candidates: vec!["A3".into()] }]);
    }

    #[test]
    fn purge_drops_owned_and_coordination_groups_keeps_others() {
        let mut g3 = group("G3", GroupKind::Custom, "A1", &["A1", "A9", "A2"]);
        g3.social = vec![Substate::SimpleTurn { agent_id: "A9".into() }];
        let mut groups = vec![
            group("G1", GroupKind::Custom, "A9", &["A9", "A1"]),
            group("G2", GroupKind::Coordination, "A1", &["A1", "A9"]),
            g3,
            group("G4", GroupKind::Human, "A9", &["A9"]),
        ];
        let removed = purge(&mut groups, &|id: &str| id != "A9");
        assert_eq!(removed, vec!["G1".to_owned(), "G2".to_owned(), "G4".to_owned()]);
        assert_eq!(groups.len(), 1, "one group survives");
        let kept = groups.first().expect("survivor");
        assert_eq!(kept.member_ids, vec!["A1".to_owned(), "A2".to_owned()]);
        assert!(kept.social.is_empty(), "departed member's turn dropped");
    }
}
