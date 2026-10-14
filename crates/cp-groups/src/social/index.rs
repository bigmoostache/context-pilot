//! Whole-store social queries built in one pass over every group, for the
//! per-tick spine steps that would otherwise rescan all groups per agent.

use std::collections::HashSet;

use super::Substate;
use crate::types::Group;

/// Every agent some substate of some group holds: the `MY_TURN` agents, in
/// one O(groups + substates) pass. `set.contains(id)` is equivalent to
/// [`runnable`](super::runnable) on the same groups and id.
#[must_use]
pub fn runnable_set(groups: &[Group]) -> HashSet<&str> {
    let mut set = HashSet::new();
    for sub in groups.iter().flat_map(|g| g.social.iter()) {
        cp_macros::deref_match!(sub, {
            Substate::SimpleTurn { ref agent_id } => {
                let _new = set.insert(agent_id.as_str());
            }
            Substate::Gather { ref pending, .. } => set.extend(pending.iter().map(String::as_str)),
            Substate::ComingBack { .. } | Substate::Dispatch { .. } | Substate::Recurrent { .. } => {}
        });
    }
    set
}

/// Whether any group holds a substate [`tick`](super::tick) can advance
/// (`ComingBack`, `Recurrent`, `Dispatch`). `false` makes `tick` a no-op.
#[must_use]
pub fn has_timed(groups: &[Group]) -> bool {
    groups
        .iter()
        .flat_map(|g| g.social.iter())
        .any(|s| matches!(*s, Substate::ComingBack { .. } | Substate::Recurrent { .. } | Substate::Dispatch { .. }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Group `id` carrying `social`.
    fn group(id: &str, social: Vec<Substate>) -> Group {
        Group { id: id.into(), social, ..Group::default() }
    }

    #[test]
    fn runnable_set_matches_runnable() {
        let turn_only = vec![group("G1", vec![Substate::SimpleTurn { agent_id: "A1".into() }])];
        let mut groups = turn_only.clone();
        groups.push(group(
            "G2",
            vec![
                Substate::Gather { pending: vec!["A2".into(), "A3".into()], origin_agent_id: "A0".into() },
                Substate::ComingBack { agent_id: "A4".into(), at_ms: 9, message: String::new(), from: None },
            ],
        ));
        let set = runnable_set(&groups);
        for id in ["A1", "A2", "A3", "A4", "A5"] {
            assert_eq!(set.contains(id), super::super::runnable(&groups, id), "{id}");
        }
        assert!(!has_timed(&turn_only));
        assert!(has_timed(&groups));
    }
}
