use super::collect_included_tool_ids;
use crate::state::{Message, MsgStatus, ToolResultRecord, ToolUseRecord};

/// Assistant tool-call message carrying one `tool_use` per id.
fn call(ids: &[&str]) -> Message {
    let uses =
        ids.iter().map(|id| ToolUseRecord::new((*id).to_owned(), "t".to_owned(), serde_json::json!({}))).collect();
    Message::new_tool_call("c".to_owned(), None, uses)
}

/// User tool-result message answering one id.
fn result(id: &str) -> Message {
    Message::new_tool_result("r".to_owned(), None, vec![ToolResultRecord::new(id.to_owned(), "ok".to_owned(), false)])
}

#[test]
fn answered_call_included_all_its_ids() {
    let got = collect_included_tool_ids(&[call(&["a", "b"]), result("a")], &[]);
    assert!(got.contains("a") && got.contains("b"));
}

#[test]
fn unanswered_call_excluded() {
    // Truncated by max_tokens: no result follows, must not be sent.
    let got = collect_included_tool_ids(&[call(&["a"])], &[]);
    assert!(got.is_empty());
}

#[test]
fn result_before_call_does_not_count() {
    let got = collect_included_tool_ids(&[result("a"), call(&["a"])], &[]);
    assert!(!got.contains("a"));
}

#[test]
fn deleted_or_detached_result_does_not_count() {
    for status in [MsgStatus::Deleted, MsgStatus::Detached] {
        let mut res = result("a");
        res.status = status;
        let got = collect_included_tool_ids(&[call(&["a"]), res], &[]);
        assert!(got.is_empty());
    }
}

#[test]
fn deleted_call_skipped() {
    let mut c = call(&["a"]);
    c.status = MsgStatus::Deleted;
    let got = collect_included_tool_ids(&[c, result("a")], &[]);
    assert!(got.is_empty());
}

#[test]
fn pending_ids_always_included() {
    let got = collect_included_tool_ids(&[call(&["a"])], &["a".to_owned()]);
    assert!(got.contains("a"));
}
