//! Thread-list shaping — pure transforms from agent state + live roster to the
//! maquette `ThreadDetail` JSON shape.
//!
//! The `/threads` handler ([`threads`](super::threads)) combines two
//! sources: the agent's on-disk `config.json` (full message logs) and the
//! in-memory [`MaterializedView`](crate::services::materialized_view::MaterializedView) roster
//! (the live, up-to-the-millisecond thread list). The functions here perform
//! the reshaping and the merge, kept separate from the request-handling code so
//! both stay small and independently testable.

use cp_wire::types::ThreadTurn;
use cp_wire::types::snapshot::todo::{WireTask, WireTaskStatus};

use crate::services::materialized_view::RosterEntry;

/// Map a [`WireTaskStatus`] to its maquette wire string (lowercase snake_case,
/// matching the agent's `TodoStatus` serde). `Unknown` degrades to `"planned"`
/// rather than failing — a newer protocol status renders as a plain item.
fn task_status_str(status: WireTaskStatus) -> &'static str {
    match status {
        WireTaskStatus::Planned | WireTaskStatus::Unknown => "planned",
        WireTaskStatus::InProgress => "in_progress",
        WireTaskStatus::Done => "done",
    }
}

/// Reshape one projected [`WireTask`] into the maquette `ThreadTask` JSON shape
/// (snake_case → camelCase `parentId`), the read-only todo item the web aside
/// renders. Cancelled tasks never reach here (excluded upstream).
fn reshape_task(task: &WireTask) -> serde_json::Value {
    serde_json::json!({
        "id": task.id,
        "parentId": task.parent_id,
        "name": task.name,
        "description": task.description,
        "status": task_status_str(task.status),
    })
}

/// Build the maquette `tasks` array for a roster entry — its projected tasks in
/// order, each reshaped to the camelCase `ThreadTask` shape.
fn roster_tasks_value(entry: &RosterEntry) -> serde_json::Value {
    serde_json::Value::Array(entry.tasks.iter().map(reshape_task).collect())
}

/// Merge the live view roster into the disk-derived thread list (X848).
///
/// For each roster entry: if a disk thread with that id exists, refresh its
/// `status`, `archived`, and `lastActivity` from the view (the authoritative,
/// up-to-the-millisecond source — design doc I5); otherwise synthesise a
/// log-less `ThreadDetail` so a thread created since the last disk flush still
/// appears immediately.
pub(crate) fn overlay_roster(details: &mut Vec<serde_json::Value>, roster: &[RosterEntry], agent_id: &str) {
    for entry in roster {
        let existing = details
            .iter_mut()
            .find(|d| d.get("id").and_then(serde_json::Value::as_str) == Some(entry.thread_id.as_str()));
        match existing {
            Some(detail) => {
                if let Some(obj) = detail.as_object_mut() {
                    drop(obj.insert("status".to_owned(), roster_status_value(entry.status)));
                    drop(obj.insert("archived".to_owned(), serde_json::Value::Bool(entry.archived)));
                    drop(obj.insert("paused".to_owned(), serde_json::Value::Bool(entry.paused)));
                    // Activity is the later of the two: disk has real message
                    // timestamps; the view bumps on creation/restore.
                    let disk_activity = obj.get("lastActivity").and_then(serde_json::Value::as_u64).unwrap_or(0);
                    drop(obj.insert(
                        "lastActivity".to_owned(),
                        serde_json::Value::from(disk_activity.max(entry.last_activity_ms)),
                    ));
                    // The view's projected task list is the fresher authority
                    // (delta-fed) — it overrides any disk-derived first-paint
                    // tasks.
                    drop(obj.insert("tasks".to_owned(), roster_tasks_value(entry)));
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
        "paused": entry.paused,
        "log": serde_json::Value::Array(Vec::new()),
        "tasks": roster_tasks_value(entry),
    })
}

/// Attach first-paint tasks to disk-derived thread details from the agent's
/// `config.json` todo module (`modules.todo.todos`).
///
/// Thread-owned todos live **separately** from the thread record in the shared
/// todo module, so [`reshape_thread`] cannot see them — it defaults `tasks` to
/// empty. This fills that gap for the cold-start / pre-delta window: it groups
/// the flat todo list by `thread_id`, excludes cancelled items (the projection
/// contract), reshapes each to the camelCase `ThreadTask` shape, and writes the
/// per-thread array onto the matching detail. The live [`overlay_roster`] then
/// overrides these with the fresher view-fed task lists where the roster has
/// them.
///
/// `config` is the parsed `config.json`; a missing todo module is tolerated
/// (details keep their empty `tasks`).
pub(crate) fn attach_disk_tasks(details: &mut [serde_json::Value], config: Option<&serde_json::Value>) {
    let Some(todos) = config
        .and_then(|c| c.get("modules"))
        .and_then(|m| m.get("todo"))
        .and_then(|t| t.get("todos"))
        .and_then(serde_json::Value::as_array)
    else {
        return;
    };

    for detail in details.iter_mut() {
        let Some(tid) = detail.get("id").and_then(serde_json::Value::as_str).map(str::to_owned) else {
            continue;
        };
        let tasks: Vec<serde_json::Value> = todos
            .iter()
            .filter(|todo| todo.get("thread_id").and_then(serde_json::Value::as_str) == Some(tid.as_str()))
            .filter(|todo| todo.get("status").and_then(serde_json::Value::as_str) != Some("cancelled"))
            .map(disk_task_json)
            .collect();
        if let Some(obj) = detail.as_object_mut() {
            let _prev = obj.insert("tasks".to_owned(), serde_json::Value::Array(tasks));
        }
    }
}

/// Reshape one raw `config.json` todo item into the maquette `ThreadTask` shape
/// (`parent_id` → `parentId`; status kept as its snake_case wire string).
fn disk_task_json(todo: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "id": todo.get("id").and_then(serde_json::Value::as_str).unwrap_or(""),
        "parentId": todo.get("parent_id").and_then(serde_json::Value::as_str),
        "name": todo.get("name").and_then(serde_json::Value::as_str).unwrap_or(""),
        "description": todo.get("description").and_then(serde_json::Value::as_str).unwrap_or(""),
        "status": todo.get("status").and_then(serde_json::Value::as_str).unwrap_or("planned"),
    })
}

/// Map a wire [`ThreadTurn`] to the maquette status string.
fn roster_status_value(status: ThreadTurn) -> serde_json::Value {
    let s = match status {
        ThreadTurn::MyTurn => "MY_TURN",
        ThreadTurn::TheirTurn | ThreadTurn::Unknown => "THEIR_TURN",
    };
    serde_json::Value::String(s.to_owned())
}

/// Reshape one raw thread from agent state to the maquette `ThreadDetail`
/// shape: `snake_case` → camelCase, computed fields (`messageCount`, `unread`,
/// `lastMessage`, `lastActivity`), and messages mapped to `log`.
pub(crate) fn reshape_thread(raw: &serde_json::Value, agent_id: &str) -> serde_json::Value {
    let messages = raw.get("messages").and_then(serde_json::Value::as_array);
    let msg_count = messages.map_or(0, Vec::len);
    let unread = messages.map_or(0, |msgs| {
        msgs.iter().filter(|m| m.get("acknowledged") == Some(&serde_json::Value::Bool(false))).count()
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

    let log: Vec<serde_json::Value> =
        messages.map(|msgs| msgs.iter().enumerate().map(|(i, m)| reshape_message(m, i)).collect()).unwrap_or_default();

    let status_str = match raw.get("status").and_then(serde_json::Value::as_str).unwrap_or("TheirTurn") {
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
        "paused": raw.get("paused").and_then(serde_json::Value::as_bool).unwrap_or(false),
        "log": log,
        "tasks": serde_json::Value::Array(Vec::new()),
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
        "author": role,
        "text": raw.get("content").and_then(serde_json::Value::as_str).unwrap_or(""),
        "ts": raw.get("timestamp").and_then(serde_json::Value::as_u64).unwrap_or(0),
        "auto": raw.get("auto").and_then(serde_json::Value::as_bool).unwrap_or(false),
    });
    if let Some(fp) = raw.get("file_path").and_then(serde_json::Value::as_str)
        && let Some(obj) = msg.as_object_mut()
    {
        drop(obj.insert("fileRef".to_owned(), serde_json::Value::String(fp.to_owned())));
    }
    msg
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Read a string field without indexing (`indexing_slicing` is forbid, even
    /// in tests); `None` when absent or not a string.
    fn str_at<'val>(v: &'val serde_json::Value, key: &str) -> Option<&'val str> {
        v.get(key).and_then(serde_json::Value::as_str)
    }

    /// Read a `u64` field without indexing; `Some(u64)` keeps the compared
    /// literal typed, so `default_numeric_fallback` never fires.
    fn u64_at(v: &serde_json::Value, key: &str) -> Option<u64> {
        v.get(key).and_then(serde_json::Value::as_u64)
    }

    /// Read a bool field without indexing.
    fn bool_at(v: &serde_json::Value, key: &str) -> Option<bool> {
        v.get(key).and_then(serde_json::Value::as_bool)
    }

    /// Read an array field without indexing.
    fn arr_at<'val>(v: &'val serde_json::Value, key: &str) -> Option<&'val Vec<serde_json::Value>> {
        v.get(key).and_then(serde_json::Value::as_array)
    }

    #[test]
    fn overlay_synthesises_view_only_thread() {
        // A thread present in the roster but absent on disk is appended with an
        // empty log — the instant-appearance path.
        let mut details: Vec<serde_json::Value> = Vec::new();
        let roster = [RosterEntry::builder("T9", "fresh", ThreadTurn::TheirTurn).last_activity_ms(4_242).build()];
        overlay_roster(&mut details, &roster, "a1");
        let d = details.first().expect("one synthesised detail");
        assert_eq!(str_at(d, "id"), Some("T9"));
        assert_eq!(str_at(d, "name"), Some("fresh"));
        assert_eq!(str_at(d, "status"), Some("THEIR_TURN"));
        assert_eq!(str_at(d, "agentId"), Some("a1"));
        assert_eq!(u64_at(d, "lastActivity"), Some(4_242));
        assert_eq!(arr_at(d, "log").map(Vec::len), Some(0), "empty log");
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
        let roster = [RosterEntry::builder("T1", "old", ThreadTurn::MyTurn)
            .archived(true)
            .last_activity_ms(500)
            .msg_count(1)
            .build()];
        overlay_roster(&mut details, &roster, "a1");
        let d = details.first().expect("no duplicate appended for a matched thread");
        assert_eq!(str_at(d, "status"), Some("MY_TURN"), "status refreshed from the view");
        assert_eq!(bool_at(d, "archived"), Some(true), "archived refreshed from the view");
        assert_eq!(u64_at(d, "lastActivity"), Some(500), "activity is the later of disk/view");
        assert_eq!(arr_at(d, "log").map(Vec::len), Some(1), "disk log preserved");
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
        assert_eq!(str_at(&d, "id"), Some("T1"));
        assert_eq!(str_at(&d, "status"), Some("MY_TURN"));
        assert_eq!(u64_at(&d, "messageCount"), Some(2));
        assert_eq!(u64_at(&d, "unread"), Some(1), "one unacknowledged message");
        assert_eq!(str_at(&d, "lastMessage"), Some("yo"));
        assert_eq!(u64_at(&d, "lastActivity"), Some(20));
        assert_log_roles(&d);
    }

    /// The two log entries map `User`/`Assistant` to `user`/`assistant` roles —
    /// factored out of the test body so the closures don't inflate its
    /// cognitive complexity past the strict threshold.
    fn assert_log_roles(d: &serde_json::Value) {
        let log = arr_at(d, "log").expect("log array");
        assert_eq!(log.first().and_then(|m| str_at(m, "author")), Some("user"));
        assert_eq!(log.get(1).and_then(|m| str_at(m, "author")), Some("assistant"));
    }
}
