//! Regression tests for the logs crate.

use std::fs;
use std::path::PathBuf;

use serde_json::json;

use crate::tools::{NewLog, parse_log_item, parse_log_items};
use crate::types::Importance;
use crate::{load_logs_chunked, migrate_if_needed};

/// Fresh scratch directory unique to one test.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cp-mod-logs-test-{}-{name}", std::process::id()));
    let _r = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn importance_parses_known_names_only() {
    assert_eq!(Importance::parse(" High "), Some(Importance::High));
    assert_eq!(Importance::parse("critical"), Some(Importance::Critical));
    assert_eq!(Importance::parse("hgih"), None);
    for name in Importance::NAMES {
        assert_eq!(Importance::parse(name).map(Importance::as_str), Some(name));
    }
}

#[test]
fn log_item_accepts_string_and_object() {
    let legacy = parse_log_item(&json!("fact")).unwrap();
    assert_eq!(legacy, Some(NewLog::new("fact".to_owned(), Importance::Medium)));
    let obj = parse_log_item(&json!({"content": "decision", "importance": "high"})).unwrap();
    assert_eq!(obj, Some(NewLog::new("decision".to_owned(), Importance::High)));
    assert_eq!(parse_log_item(&json!("   ")).unwrap(), None);
}

#[test]
fn invalid_importance_rejects_whole_batch() {
    let items = [json!({"content": "ok"}), json!({"content": "bad", "importance": "hgih"}), json!(3i32)];
    let errors = parse_log_items(&items).unwrap_err();
    assert_eq!(errors.len(), 2);
    assert!(errors.first().unwrap().starts_with("entry #2"));
    assert!(errors.last().unwrap().starts_with("entry #3"));
}

#[test]
fn migration_keeps_existing_chunks() {
    let dir = scratch("migrate");
    let chunk = r#"[{"id":"L1","timestamp_ms":5,"content":"old","tags":["x"],"summary":false}]"#;
    fs::write(dir.join("chunk_0.json"), chunk).unwrap();
    fs::write(dir.join("next_id.json"), r#"{"next_log_id": 2}"#).unwrap();
    migrate_if_needed(&dir);
    assert!(dir.join(".schema_v2").exists());
    assert_eq!(fs::read_to_string(dir.join("chunk_0.json")).unwrap(), chunk);
    let (logs, next) = load_logs_chunked(&dir);
    assert_eq!(logs.len(), 1);
    assert_eq!(logs.first().unwrap().importance, "medium");
    assert_eq!(next, 2);
    let _r = fs::remove_dir_all(&dir);
}

#[test]
fn corrupt_chunk_is_moved_aside_not_dropped() {
    let dir = scratch("corrupt");
    fs::write(dir.join("chunk_0.json"), r#"[{"id":"L1","timestamp_ms":1,"content":"a"}]"#).unwrap();
    fs::write(dir.join("chunk_1.json"), "{ truncated").unwrap();
    let (logs, _next) = load_logs_chunked(&dir);
    assert_eq!(logs.len(), 1);
    assert!(!dir.join("chunk_1.json").exists());
    let aside: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with("chunk_1.json.corrupt-"))
        .collect();
    assert_eq!(aside.len(), 1);
    assert_eq!(fs::read_to_string(aside.first().unwrap().path()).unwrap(), "{ truncated");
    let _r = fs::remove_dir_all(&dir);
}

#[test]
fn next_id_never_reuses_an_id_on_disk() {
    let dir = scratch("nextid");
    fs::write(dir.join("chunk_0.json"), r#"[{"id":"L7","timestamp_ms":1,"content":"a"}]"#).unwrap();
    fs::write(dir.join("next_id.json"), r#"{"next_log_id": 3}"#).unwrap();
    let (_logs, next) = load_logs_chunked(&dir);
    assert_eq!(next, 8);
    let _r = fs::remove_dir_all(&dir);
}
