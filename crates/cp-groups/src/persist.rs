//! Per-group message files: `.context-pilot/groups/<id>.json`.
//!
//! Group metadata stays in `config.json` ([`GroupMeta`]); each group's
//! messages live in their own file, rewritten only when its [`fingerprint`]
//! changed since the last write. Same scheme as the agents' former files.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use cp_base::config::constants::STORE_DIR;
use serde::Serialize;

use crate::chat::{GroupAuthor, GroupMessage};
use crate::social::Substate;
use crate::types::{Group, GroupKind};

/// Directory (under the store dir) holding one message file per group.
pub const GROUPS_DIR: &str = "groups";

/// Path of a group's message file.
#[must_use]
pub fn group_file(id: &str) -> PathBuf {
    PathBuf::from(STORE_DIR).join(GROUPS_DIR).join(format!("{id}.json"))
}

/// Borrowing view of a [`Group`] without its messages: what `config.json` stores.
#[derive(Debug, Serialize)]
pub struct GroupMeta<'group> {
    /// See [`Group::id`].
    id: &'group str,
    /// See [`Group::name`].
    name: &'group str,
    /// See [`Group::description`].
    #[serde(skip_serializing_if = "str::is_empty")]
    description: &'group str,
    /// See [`Group::owner_agent_id`].
    owner_agent_id: &'group str,
    /// See [`Group::member_ids`].
    member_ids: &'group [String],
    /// See [`Group::kind`].
    kind: GroupKind,
    /// See [`Group::created_at`].
    created_at: u64,
    /// See [`Group::social`].
    #[serde(skip_serializing_if = "<[Substate]>::is_empty")]
    social: &'group [Substate],
}

impl<'group> From<&'group Group> for GroupMeta<'group> {
    fn from(g: &'group Group) -> Self {
        Self {
            id: &g.id,
            name: &g.name,
            description: &g.description,
            owner_agent_id: &g.owner_agent_id,
            member_ids: &g.member_ids,
            kind: g.kind,
            created_at: g.created_at,
            social: &g.social,
        }
    }
}

/// On-disk body of a group message file.
#[derive(Serialize)]
struct GroupFile<'group> {
    /// Owning group id.
    id: &'group str,
    /// Full ordered message log.
    messages: &'group [GroupMessage],
}

/// Cheap change detector over a group's messages, without serializing.
///
/// Folds every field a mutation can touch (count, timestamps, content and
/// path lengths, flags) so any push/retain/edit/ack changes the value.
#[must_use]
pub fn fingerprint(messages: &[GroupMessage]) -> u64 {
    const K: u64 = 0x0100_0000_01b3;
    let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ u64::try_from(messages.len()).unwrap_or(u64::MAX);
    for m in messages {
        let len = |s: Option<&String>| s.map_or(u64::MAX, |v| u64::try_from(v.len()).unwrap_or(u64::MAX));
        let flags = u64::from(m.acknowledged)
            | (u64::from(m.auto) << 1u8)
            | (u64::from(m.has_been_pushed) << 2u8)
            | (u64::from(m.author == GroupAuthor::Assistant) << 3u8);
        for v in [m.timestamp, len(m.content.as_ref()), len(m.file_path.as_ref()), flags] {
            h = (h ^ v).wrapping_mul(K);
        }
    }
    h
}

/// Fingerprint of each group's file as last written / loaded.
fn clean_map() -> &'static Mutex<HashMap<String, u64>> {
    static CLEAN: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    CLEAN.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Serialize the message files of groups changed since their last write.
///
/// Marks them clean on return: callers must hand every op to a writer that
/// never drops it (the persistence writer's durable lane).
#[must_use]
pub fn dirty_group_file_ops(groups: &[Group]) -> Vec<(PathBuf, Vec<u8>)> {
    let Ok(mut clean) = clean_map().lock() else { return Vec::new() };
    let mut ops = Vec::new();
    for g in groups {
        let fp = fingerprint(&g.messages);
        if clean.get(&g.id) == Some(&fp) {
            continue;
        }
        let Ok(bytes) = serde_json::to_vec(&GroupFile { id: &g.id, messages: &g.messages }) else { continue };
        let _prev = clean.insert(g.id.clone(), fp);
        ops.push((group_file(&g.id), bytes));
    }
    ops
}

/// Fill `messages` of groups loaded slim from `config.json` from their files.
pub fn load_group_messages(groups: &mut [Group]) {
    let Ok(mut clean) = clean_map().lock() else { return };
    for g in groups.iter_mut().filter(|g| g.messages.is_empty()) {
        let Ok(bytes) = std::fs::read(group_file(&g.id)) else { continue };
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) else { continue };
        let Some(msgs) = v.get("messages").and_then(|m| serde_json::from_value(m.clone()).ok()) else { continue };
        g.messages = msgs;
        let _prev = clean.insert(g.id.clone(), fingerprint(&g.messages));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::GroupsState;

    #[test]
    fn meta_round_trips_without_messages() {
        let mut s = GroupsState::new();
        let id = s.create_human_group("A1", "first");
        let g = s.find_mut(&id).expect("created");
        g.messages.push(GroupMessage::user("hi".into()));
        let json = serde_json::to_value(GroupMeta::from(&*g)).expect("serialize");
        let back: Group = serde_json::from_value(json).expect("deserialize");
        assert_eq!((back.id.as_str(), back.owner_agent_id.as_str()), ("G1", "A1"));
        assert!(back.messages.is_empty());
        assert_ne!(fingerprint(&g.messages), fingerprint(&[]));
    }
}
