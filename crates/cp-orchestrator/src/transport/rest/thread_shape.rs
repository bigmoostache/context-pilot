//! Thread-list shaping — pure transforms from agent state + live roster to the
//! maquette `ThreadDetail` JSON shape.
//!
//! The `/threads` handler ([`threads`](super::threads)) combines two
//! sources: the agent's on-disk `config.json` (full message logs) and the
//! in-memory [`MaterializedView`](crate::services::MaterializedView) roster
//! (the live, up-to-the-millisecond thread list). The functions here perform
//! the reshaping and the merge, kept separate from the request-handling code so
//! both stay small and independently testable.

use cp_wire::types::ThreadTurn;

use crate::services::materialized_view::RosterEntry;

/// Merge the live view roster into the disk-derived thread list (X848).
///
/// For each roster entry: if a disk thread with that id exists, refresh its
/// `status`, `archived`, and `lastActivity` from the view (the authoritative,
/// up-to-the-millisecond source — design doc I5); otherwise synthesise a
/// log-less `ThreadDetail` so a thread created since the last disk flush still
/// appears immediately.
pub(super) fn overlay_roster(
    details: &mut Vec<serde_json::Value>,
    roster: &[RosterEntry],
    agent_id: &str,
) {
    for entry in roster {
        let existing = details.iter_mut().find(|d| {
            d.get("id").and_then(serde_json::Value::as_str) == Some(entry.thread_id.as_str())
        });
        match existing {
            Some(detail) => {
                if let Some(obj) = detail.as_object_mut() {
                    let _prev = obj.insert("status".to_owned(), roster_status_value(entry.status));
                    let _prev =
                        obj.insert("archived".to_owned(), serde_json::Value::Bool(entry.archived));
                    // Activity is the later of the two: disk has real message
                    // timestamps; the view bumps on creation/restore.
                    let disk_activity =
                        obj.get("lastActivity").and_then(serde_json::Value::as_u64).unwrap_or(0);
                    let _prev = obj.insert(
                        "lastActivity".to_owned(),
                        serde_json::Value::from(disk_activity.max(entry.last_activity_ms)),
                    );
                }
            }
            None => details.push(synthesize_from_roster(entry, agent_id)),
        }
    }
}

/// Build a `ThreadDetail` from a roster entry alone — no message bodies yet.
fn synthesize_from_roster(entry: &RosterEntry, agent_id: &str) -> serde_json::Value {
    serde_json::json!({
        "id": entry.thread_id,
        "name": entry.name,
        "status": roster_status_value(entry.status),
        "agentId": agent_id,
        "lastMessage": "",
        "lastActivity": entry.last_activity_ms,
        "messageCount": entry.msg_count,
        "unread": 0,
        "archived": entry.archived,
        "log": serde_json::Value::Array(Vec::new()),
    })
}

/// Map a wire [`ThreadTurn`] to the maquette status string.
fn roster_status_value(status: ThreadTurn) -> serde_json::Value {
    let s = match status {
        ThreadTurn::MyTurn => "MY_TURN",
        _ => "THEIR_TURN",
    };
    serde_json::Value::String(s.to_owned())
}

/// Reshape one raw thread from agent state to the maquette `ThreadDetail`
/// shape: snake_case → camelCase, computed fields (`messageCount`, `unread`,
/// `lastMessage`, `lastActivity`), and messages mapped to `log`.
pub(super) fn reshape_thread(raw: &serde_json::Value, agent_id: &str) -> serde_json::Value {
    let messages = raw.get("messages").and_then(serde_json::Value::as_array);
    let msg_count = messages.map_or(0, Vec::len);
    let unread = messages.map_or(0, |msgs| {
        msgs.iter()
            .filter(|m| m.get("acknowledged") == Some(&serde_json::Value::Bool(false)))
            .count()
    });
    let last_msg = messages
        .and_then(|msgs| msgs.last())
        .and_then(|m| m.get("content"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let last_activity = messages
        .and_then(|msgs| msgs.last())
        .and_then(|m| m.get("timestamp"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);

    let log: Vec<serde_json::Value> = messages
        .map(|msgs| msgs.iter().enumerate().map(|(i, m)| reshape_message(m, i)).collect())
        .unwrap_or_default();

    let status_str =
        match raw.get("status").and_then(serde_json::Value::as_str).unwrap_or("TheirTurn") {
            "MyTurn" => "MY_TURN",
            _ => "THEIR_TURN",
        };

    serde_json::json!({
        "id": raw.get("id").and_then(serde_json::Value::as_str).unwrap_or(""),
        "name": raw.get("name").and_then(serde_json::Value::as_str).unwrap_or(""),
        "status": status_str,
        "agentId": agent_id,
        "lastMessage": last_msg,
        "lastActivity": last_activity,
        "messageCount": msg_count,
        "unread": unread,
        "archived": raw.get("archived").and_then(serde_json::Value::as_bool).unwrap_or(false),
        "log": log,
    })
}

/// Reshape one thread message to the maquette `ThreadMsg` shape.
fn reshape_message(raw: &serde_json::Value, index: usize) -> serde_json::Value {
    let role = match raw.get("author").and_then(serde_json::Value::as_str).unwrap_or("User") {
        "Assistant" => "assistant",
        _ => "user",
    };
    let mut msg = serde_json::json!({
        "id": format!("msg_{index}"),
        "role": role,
        "content": raw.get("content").and_then(serde_json::Value::as_str).unwrap_or(""),
        "timestamp": raw.get("timestamp").and_then(serde_json::Value::as_u64).unwrap_or(0),
        "auto": raw.get("auto").and_then(serde_json::Value::as_bool).unwrap_or(false),
    });
    if let Some(fp) = raw.get("file_path").and_then(serde_json::Value::as_str) {
        let _prev = msg
            .as_object_mut()
            .expect("just built")
            .insert("fileRef".to_owned(), serde_json::Value::String(fp.to_owned()));
    }
    if let Some(q) = raw.get("question") {
        if !q.is_null() {
            let _prev = msg
                .as_object_mut()
                .expect("just built")
                .insert("questions".to_owned(), serde_json::json!([q]));
        }
    }
    msg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_synthesises_view_only_thread() {
        // A thread present in the roster but absent on disk is appended with an
        // empty log — the instant-appearance path.
        let mut details: Vec<serde_json::Value> = Vec::new();
        let roster = [RosterEntry {
            thread_id: "T9".into(),
            name: "fresh".into(),
            status: ThreadTurn::TheirTurn,
            archived: false,
            last_activity_ms: 4_242,
            msg_count: 0,
        }];
        overlay_roster(&mut details, &roster, "a1");
        assert_eq!(details.len(), 1);
        let d = &details[0];
        assert_eq!(d["id"], "T9");
        assert_eq!(d["name"], "fresh");
        assert_eq!(d["status"], "THEIR_TURN");
        assert_eq!(d["agentId"], "a1");
        assert_eq!(d["lastActivity"], 4_242);
        assert!(d["log"].as_array().expect("log array").is_empty());
    }

    #[test]
    fn overlay_refreshes_status_archived_and_activity_on_disk_thread() {
        // A disk thread keeps its log but takes the view's fresher status,
        // archived flag, and (later) activity.
        let mut details = vec![serde_json::json!({
            "id": "T1",
            "name": "old",
            "status": "THEIR_TURN",
            "agentId": "a1",
            "lastActivity": 100u64,
            "archived": false,
            "log": [{"id": "msg_0", "role": "user", "content": "hi", "timestamp": 100u64}],
        })];
        let roster = [RosterEntry {
            thread_id: "T1".into(),
            name: "old".into(),
            status: ThreadTurn::MyTurn,
            archived: true,
            last_activity_ms: 500,
            msg_count: 1,
        }];
        overlay_roster(&mut details, &roster, "a1");
        assert_eq!(details.len(), 1, "no duplicate appended for a matched thread");
        let d = &details[0];
        assert_eq!(d["status"], "MY_TURN", "status refreshed from the view");
        assert_eq!(d["archived"], true, "archived refreshed from the view");
        assert_eq!(d["lastActivity"], 500, "activity is the later of disk/view");
        assert_eq!(d["log"].as_array().expect("log").len(), 1, "disk log preserved");
    }

    #[test]
    fn reshape_thread_maps_fields_and_messages() {
        let raw = serde_json::json!({
            "id": "T1",
            "name": "Plan",
            "status": "MyTurn",
            "archived": false,
            "messages": [
                {"author": "User", "content": "hi", "timestamp": 10u64, "acknowledged": true},
                {"author": "Assistant", "content": "yo", "timestamp": 20u64, "acknowledged": false},
            ],
        });
        let d = reshape_thread(&raw, "a1");
        assert_eq!(d["id"], "T1");
        assert_eq!(d["status"], "MY_TURN");
        assert_eq!(d["messageCount"], 2);
        assert_eq!(d["unread"], 1, "one unacknowledged message");
        assert_eq!(d["lastMessage"], "yo");
        assert_eq!(d["lastActivity"], 20);
        let log = d["log"].as_array().expect("log");
        assert_eq!(log[0]["role"], "user");
        assert_eq!(log[1]["role"], "assistant");
    }
}
