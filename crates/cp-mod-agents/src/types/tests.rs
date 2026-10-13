//! Unit tests for the thread types (extracted from `types.rs` for the
//! 500-line cap). `super` is the `types` module, so `use super::*` keeps the
//! exact same scope the tests had when inlined.

use super::*;

/// A message by `author` stamped `ts`, unacknowledged.
fn msg(author: ThreadAuthor, text: &str, ts: u64) -> ThreadMessage {
    ThreadMessage {
        author,
        content: Some(text.to_owned()),
        file_path: None,
        timestamp: ts,
        acknowledged: false,
        auto: false,
        has_been_pushed: false,
    }
}

/// State holding one parent thread `T1` with four messages (ts 10..=40),
/// archived + paused + `MyTurn` so the test can check none of it carries over.
fn parent_state() -> ThreadsState {
    let mut ts = ThreadsState::new();
    let mut parent = Thread::new("T1".to_owned(), "Parent".to_owned());
    parent.messages = vec![
        msg(ThreadAuthor::User, "q1", 10),
        msg(ThreadAuthor::Assistant, "a1", 20),
        msg(ThreadAuthor::User, "q2", 30),
        msg(ThreadAuthor::Assistant, "a2", 40),
    ];
    parent.status = ThreadStatus::MyTurn;
    parent.archived = true;
    parent.paused = true;
    ts.threads.push(parent);
    ts.next_id = 2;
    ts
}

#[test]
fn branch_copies_history_up_to_and_including_the_branch_point() {
    let mut ts = parent_state();
    let id = ts.branch("T1", 20, "Alt").unwrap();
    assert_eq!(id, "T2");
    assert_eq!(ts.next_id, 3);

    let branch = ts.threads.iter().find(|t| t.id == "T2").unwrap();
    let texts: Vec<_> = branch.messages.iter().map(|m| m.content.as_deref().unwrap_or("")).collect();
    assert_eq!(texts, ["q1", "a1"]);
    assert_eq!(branch.messages.iter().map(|m| m.timestamp).collect::<Vec<_>>(), [10, 20]);
    assert!(branch.messages.iter().all(|m| m.acknowledged));
    assert_eq!(branch.origin, Some(ThreadOrigin { thread_id: "T1".to_owned(), message_ts: 20 }));
}

#[test]
fn branch_is_a_fresh_active_thread_and_leaves_the_parent_untouched() {
    let mut ts = parent_state();
    let id = ts.branch("T1", 20, "Alt").unwrap();

    let branch = ts.threads.iter().find(|t| t.id == id).unwrap();
    assert_eq!(branch.name, "Alt");
    assert_eq!(branch.status, ThreadStatus::TheirTurn);
    assert!(!branch.archived);
    assert!(!branch.paused);

    let parent = ts.threads.iter().find(|t| t.id == "T1").unwrap();
    assert_eq!(parent.messages.len(), 4);
    assert!(parent.messages.iter().all(|m| !m.acknowledged));
}

#[test]
fn branch_at_last_message_copies_everything() {
    let mut ts = parent_state();
    let id = ts.branch("T1", 40, "Copy").unwrap();
    let branch = ts.threads.iter().find(|t| t.id == id).unwrap();
    assert_eq!(branch.messages.len(), 4);
}

#[test]
fn branch_rejects_unknown_thread_or_message() {
    let mut ts = parent_state();
    let unknown_thread = ts.branch("T9", 20, "x").unwrap_err();
    assert!(unknown_thread.contains("T9"));
    let unknown_message = ts.branch("T1", 25, "x").unwrap_err();
    assert!(unknown_message.contains("ts=25"));
    assert_eq!(ts.threads.len(), 1);
    assert_eq!(ts.next_id, 2);
}

#[test]
fn origin_round_trips_and_defaults_to_none() {
    let legacy: Thread =
        serde_json::from_str(r#"{"id":"T1","name":"n","status":"TheirTurn","messages":[],"created_at":1}"#).unwrap();
    assert_eq!(legacy.origin, None);
    assert!(!serde_json::to_string(&legacy).unwrap().contains("origin"));

    let mut ts = parent_state();
    let id = ts.branch("T1", 30, "b").unwrap();
    let branch = ts.threads.iter().find(|t| t.id == id).unwrap();
    let back: Thread = serde_json::from_str(&serde_json::to_string(branch).unwrap()).unwrap();
    assert_eq!(back.origin, branch.origin);
}
