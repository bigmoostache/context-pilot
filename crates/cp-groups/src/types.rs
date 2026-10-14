//! Group model: a conversation owned by one agent, shared by its members.

use serde::{Deserialize, Serialize};

use cp_base::state::runtime::State;

use crate::chat::GroupMessage;
use crate::social::Substate;

/// Pseudo-agent id of the human user as a group member / talking-stick target.
pub const HUMAN: &str = "HUMAN";

/// What a group is for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GroupKind {
    /// An agent's conversation with the human (one per agent, never deleted).
    #[default]
    Human,
    /// 1:1 manager ↔ sub-agent channel, created with the sub-agent.
    Coordination,
    /// Any other group, created with `Group_create`.
    Custom,
}

/// A conversation between agents (and possibly the human).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Group {
    /// Unique id, `G<n>`.
    pub id: String,
    /// Display name.
    pub name: String,
    /// One-line purpose.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// The single agent that owns the group.
    pub owner_agent_id: String,
    /// Member agent ids, [`HUMAN`] included when the user takes part.
    #[serde(default)]
    pub member_ids: Vec<String>,
    /// What the group is for.
    #[serde(default)]
    pub kind: GroupKind,
    /// Ordered messages. Persisted in `groups/<id>.json`, not in `config.json`
    /// (see [`crate::persist`]); empty after a slim deserialize.
    #[serde(default)]
    pub messages: Vec<GroupMessage>,
    /// Creation timestamp (epoch ms).
    #[serde(default)]
    pub created_at: u64,
    /// Pending obligations: who must act next (see [`crate::social`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub social: Vec<Substate>,
}

/// Fleet-shared group store, persisted via the groups module.
#[derive(Debug)]
pub struct GroupsState {
    /// Every group of the fleet.
    pub groups: Vec<Group>,
    /// Counter for the next `G<n>` id.
    pub next_id: u32,
}

impl Default for GroupsState {
    fn default() -> Self {
        Self::new()
    }
}

impl GroupsState {
    /// Empty store, id counter at 1.
    #[must_use]
    pub const fn new() -> Self {
        Self { groups: Vec::new(), next_id: 1 }
    }

    /// Shared ref from State's `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if `GroupsState` was never inserted into state.
    #[must_use]
    pub fn get(state: &State) -> &Self {
        state.ext::<Self>()
    }

    /// Mutable ref from State's `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if `GroupsState` was never inserted into state.
    pub fn get_mut(state: &mut State) -> &mut Self {
        state.ext_mut::<Self>()
    }

    /// Insert `draft` under a fresh `G<n>` id stamped now; returns the id.
    /// The draft's `id` and `created_at` are overwritten.
    pub fn create(&mut self, draft: Group) -> String {
        let id = format!("G{}", self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        self.groups.push(Group { id: id.clone(), created_at: cp_base::panels::now_ms(), ..draft });
        id
    }

    /// Create `agent_id`'s human group (owner = the agent, members = agent + [`HUMAN`]).
    pub fn create_human_group(&mut self, agent_id: &str, agent_name: &str) -> String {
        self.create(Group {
            name: agent_name.to_owned(),
            owner_agent_id: agent_id.to_owned(),
            member_ids: vec![agent_id.to_owned(), HUMAN.to_owned()],
            kind: GroupKind::Human,
            ..Group::default()
        })
    }

    /// The group with this id.
    #[must_use]
    pub fn find(&self, id: &str) -> Option<&Group> {
        self.groups.iter().find(|g| g.id == id)
    }

    /// `G5 "name"` label for notifications; the bare id when the group is gone.
    #[must_use]
    pub fn label(&self, id: &str) -> String {
        self.find(id).map_or_else(|| id.to_owned(), |g| format!("{id} \"{}\"", g.name))
    }

    /// Mutable group with this id.
    pub fn find_mut(&mut self, id: &str) -> Option<&mut Group> {
        self.groups.iter_mut().find(|g| g.id == id)
    }

    /// Groups `agent_id` belongs to, in creation order.
    pub fn of_member<'groups>(&'groups self, agent_id: &'groups str) -> impl Iterator<Item = &'groups Group> {
        self.groups.iter().filter(move |g| g.member_ids.iter().any(|m| m == agent_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_group_ids_and_membership() {
        let mut s = GroupsState::new();
        let g1 = s.create_human_group("A1", "first");
        let g2 = s.create(Group {
            name: "pair".into(),
            owner_agent_id: "A1".into(),
            member_ids: vec!["A1".into(), "A2".into()],
            kind: GroupKind::Custom,
            ..Group::default()
        });
        assert_eq!((g1.as_str(), g2.as_str()), ("G1", "G2"));
        assert_eq!(s.find("G1").map(|g| g.kind), Some(GroupKind::Human));
        assert_eq!(s.of_member("A2").count(), 1);
        assert_eq!(s.of_member(HUMAN).count(), 1);
    }
}
