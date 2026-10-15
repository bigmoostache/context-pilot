//! Per-group message files: `.context-pilot/groups/<id>.json`.
//!
//! Group metadata stays in `config.json` ([`GroupMeta`]); each group's
//! messages live in their own file, rewritten only when its [`fingerprint`]
//! changed since the last write. Same scheme as the agents' former files.

use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash as _, Hasher as _};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use cp_base::config::constants::STORE_DIR;
use serde::{Deserialize, Serialize};

use crate::chat::GroupMessage;
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
    /// See [`Group::read_upto`]. Without it every cursor resets on reload and
    /// the whole history reads as unread again.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    read_upto: &'group BTreeMap<String, u64>,
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
            read_upto: &g.read_upto,
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

/// Owned read side of [`GroupFile`]: parsed once, straight into messages.
#[derive(Deserialize)]
struct GroupFileIn {
    /// Full ordered message log.
    messages: Vec<GroupMessage>,
}

/// Change detector over a group's messages, without serializing.
///
/// Hashes every persisted field, text included, so any push/retain/edit/ack
/// changes the value (a same-length edit too).
#[must_use]
pub fn fingerprint(messages: &[GroupMessage]) -> u64 {
    let mut h = std::hash::DefaultHasher::new();
    messages.len().hash(&mut h);
    for m in messages {
        m.timestamp.hash(&mut h);
        m.author_agent_id.hash(&mut h);
        m.content.hash(&mut h);
        m.file_path.hash(&mut h);
        [m.acknowledged, m.auto, m.has_been_pushed].hash(&mut h);
        m.author.to_string().hash(&mut h);
    }
    h.finish()
}

/// Fingerprint of each group's file as last written / loaded.
fn clean_map() -> &'static Mutex<HashMap<String, u64>> {
    static CLEAN: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    CLEAN.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Delete a removed group's message file and forget its clean stamp.
/// A missing file is fine; other I/O errors are logged.
pub fn remove_group_file(id: &str) {
    if let Ok(mut clean) = clean_map().lock() {
        let _prev = clean.remove(id);
    }
    match std::fs::remove_file(group_file(id)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => log::warn!("remove group file {id}: {e}"),
    }
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

/// Read one message file. `Ok(None)` = no file yet; `Err` = present but
/// unreadable or unparsable.
fn read_messages(path: &Path) -> Result<Option<Vec<GroupMessage>>, String> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    serde_json::from_slice::<GroupFileIn>(&bytes).map(|f| Some(f.messages)).map_err(|e| e.to_string())
}

/// Move a bad message file aside (`<id>.json.corrupt-<ms>`) so the next save,
/// which writes the group's now-empty log, cannot destroy the history.
fn quarantine(path: &Path, why: &str) {
    let mut aside = path.as_os_str().to_owned();
    aside.push(format!(".corrupt-{}", cp_base::panels::now_ms()));
    match std::fs::rename(path, &aside) {
        Ok(()) => log::error!("group file {} unreadable ({why}); moved to {}", path.display(), aside.display()),
        Err(e) => log::error!("group file {} unreadable ({why}); quarantine failed: {e}", path.display()),
    }
}

/// Fill `messages` of groups loaded slim from `config.json` from their files.
/// A file that exists but cannot be loaded is quarantined, never overwritten.
pub fn load_group_messages(groups: &mut [Group]) {
    let Ok(mut clean) = clean_map().lock() else { return };
    for g in groups.iter_mut().filter(|g| g.messages.is_empty()) {
        let path = group_file(&g.id);
        match read_messages(&path) {
            Ok(Some(msgs)) => {
                g.messages = msgs;
                let _prev = clean.insert(g.id.clone(), fingerprint(&g.messages));
            }
            Ok(None) => {}
            Err(why) => quarantine(&path, &why),
        }
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
        g.mark_read("A1");
        let json = serde_json::to_value(GroupMeta::from(&*g)).expect("serialize");
        let back: Group = serde_json::from_value(json).expect("deserialize");
        assert_eq!((back.id.as_str(), back.owner_agent_id.as_str()), ("G1", "A1"));
        assert!(back.messages.is_empty());
        assert_eq!(back.read_upto, g.read_upto, "read cursors survive a reload");
        assert_ne!(fingerprint(&g.messages), fingerprint(&[]));
    }

    #[test]
    fn fingerprint_sees_same_length_edit() {
        let mut msgs = vec![GroupMessage::user("abc".into())];
        let before = fingerprint(&msgs);
        if let Some(m) = msgs.first_mut() {
            m.content = Some("xyz".into());
        }
        assert_ne!(before, fingerprint(&msgs));
    }

    #[test]
    fn corrupt_file_is_quarantined_not_lost() {
        let dir = std::env::temp_dir().join(format!("cp-groups-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("G1.json");
        std::fs::write(&path, b"{ not json").expect("write");
        let err = read_messages(&path).expect_err("corrupt");
        quarantine(&path, &err);
        assert!(!path.exists(), "bad file moved aside");
        let kept = std::fs::read_dir(&dir).expect("ls").filter_map(Result::ok).count();
        assert_eq!(kept, 1, "original bytes kept under a .corrupt name");
        assert!(matches!(read_messages(&dir.join("missing.json")), Ok(None)));
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
