//! `Send` staleness guard: an agent may only post into a group it has
//! seen up to date.
//!
//! A Send is refused when the sender has no `group` panel open on the target
//! group, or when the group holds messages the sender has not seen. In both
//! cases the guard opens/refreshes the panel and advances the read cursor (what
//! `Read` would do), so the agent's next turn sees the latest messages and can
//! re-send without calling `Read` itself.

use cp_base::state::runtime::State;
use cp_groups::panels::{GROUP_KIND, META_GROUP_ID};
use cp_groups::types::{Group, GroupKind, GroupsState};

/// Messages in `group` that `reader` has not seen.
///
/// Read cursor based, like [`Group::unread_for`], except that in a human group
/// an `acknowledged` message counts as seen: user messages reach the agent
/// inline (idle auto-read, streaming push), which never moves the cursor.
fn unseen(group: &Group, reader: &str) -> usize {
    let cursor = group.read_upto.get(reader).copied().unwrap_or(0);
    let human = group.kind == GroupKind::Human;
    group
        .messages
        .iter()
        .filter(|m| !m.auto && m.timestamp > cursor && m.author_agent_id.as_deref() != Some(reader))
        .filter(|m| !(human && m.acknowledged))
        .count()
}

/// Whether the executing agent has a `group` panel open on `gid`.
fn panel_open(state: &State, gid: &str) -> bool {
    state.agent().context.iter().any(|c| {
        c.context_type.as_str() == GROUP_KIND
            && c.metadata.get(META_GROUP_ID).and_then(serde_json::Value::as_str) == Some(gid)
    })
}

/// Refuse a `Send` from `me` into `gid` when `me`'s view of it is missing or
/// stale; returns the refusal message, or `None` when the Send may proceed.
///
/// On refusal the group's panel is opened (or refreshed) and the messages are
/// marked read, so the refusal itself brings the agent up to date.
pub(crate) fn guard(state: &mut State, me: &str, gid: &str) -> Option<String> {
    let opened = panel_open(state, gid);
    let new = GroupsState::get(state).find(gid).map_or(0, |g| unseen(g, me));
    if opened && new == 0 {
        return None;
    }
    if let Some(group) = GroupsState::get_mut(state).find_mut(gid) {
        group.mark_read(me);
        if group.kind == GroupKind::Human {
            for m in &mut group.messages {
                m.acknowledged = true;
            }
        }
    }
    let panel = cp_groups::panels::open_group_panel(state, gid);
    let why = if opened {
        format!("{gid} has {new} message(s) you had not seen")
    } else {
        format!("you had no panel open on {gid}, so you could not know its latest messages")
    };
    Some(format!(
        "NOT SENT: {why}. Panel {panel} now shows {gid} up to date (no need to call Read). \
         Review it, then Send again if your message still fits."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cp_groups::chat::{GroupAuthor, GroupMessage};

    fn msg(ts: u64, author: Option<&str>, acknowledged: bool) -> GroupMessage {
        GroupMessage {
            author: GroupAuthor::User,
            author_agent_id: author.map(str::to_owned),
            content: Some("hi".to_owned()),
            file_path: None,
            timestamp: ts,
            acknowledged,
            auto: false,
            has_been_pushed: false,
        }
    }

    #[test]
    fn unseen_counts_after_cursor_excluding_own() {
        let mut g = Group { kind: GroupKind::Custom, ..Group::default() };
        g.messages = vec![msg(1, Some("A2"), true), msg(2, Some("A1"), false), msg(3, Some("A2"), true)];
        let _prev = g.read_upto.insert("A1".to_owned(), 1);
        assert_eq!(unseen(&g, "A1"), 1, "own message skipped, ack ignored outside human groups");
        g.mark_read("A1");
        assert_eq!(unseen(&g, "A1"), 0);
    }

    #[test]
    fn human_group_acknowledged_counts_as_seen() {
        let mut g = Group { kind: GroupKind::Human, ..Group::default() };
        g.messages = vec![msg(5, None, true), msg(6, None, false)];
        assert_eq!(unseen(&g, "A1"), 1, "only the unacknowledged user message is unseen");
    }
}
