use super::*;
use tempfile::tempdir;

fn write_config(folder: &Path, value: &Value) {
    let dir = folder.join(CP_DIR);
    fs::create_dir_all(&dir).expect("mkdir");
    fs::write(dir.join(CONFIG_FILE), serde_json::to_vec(value).expect("ser")).expect("write");
}

fn write_worker(folder: &Path, worker: &str, value: &Value) {
    let dir = folder.join(CP_DIR).join(STATES_DIR);
    fs::create_dir_all(&dir).expect("mkdir");
    fs::write(dir.join(format!("{worker}.json")), serde_json::to_vec(value).expect("ser")).expect("write");
}

#[test]
fn read_config_caches_and_returns_on_mtime_hit() {
    let dir = tempdir().expect("dir");
    let folder = dir.path();
    let val = serde_json::json!({"modules": {"agents": []}});
    write_config(folder, &val);

    let mut reader = StateReader::new();
    let first = reader.read_config(folder).expect("read");
    assert_eq!(first, val);

    // Second read without file change → mtime hit, returns same.
    let second = reader.read_config(folder).expect("read");
    assert_eq!(second, val);
}

#[test]
fn read_config_re_parses_on_mtime_change() {
    let dir = tempdir().expect("dir");
    let folder = dir.path();
    let v1 = serde_json::json!({"version": 1u32});
    write_config(folder, &v1);

    let mut reader = StateReader::new();
    let _first = reader.read_config(folder).expect("read");

    // Mutate the file (mtime changes).
    let v2 = serde_json::json!({"version": 2u32});
    // Ensure mtime actually differs (some filesystems have 1s granularity).
    std::thread::sleep(std::time::Duration::from_millis(50));
    write_config(folder, &v2);

    let second = reader.read_config(folder).expect("read");
    assert_eq!(second, v2);
}

#[test]
fn read_config_falls_back_on_torn_read() {
    let dir = tempdir().expect("dir");
    let folder = dir.path();
    let good = serde_json::json!({"ok": true});
    write_config(folder, &good);

    let mut reader = StateReader::new();
    let _first = reader.read_config(folder).expect("read");

    // Corrupt the file (simulates mid-write torn read).
    std::thread::sleep(std::time::Duration::from_millis(50));
    let path = folder.join(CP_DIR).join(CONFIG_FILE);
    fs::write(&path, b"{{{{not json").expect("write");

    // Should fall back to the last good value.
    let fallback = reader.read_config(folder).expect("read");
    assert_eq!(fallback, good);
}

#[test]
fn read_worker_and_list_workers() {
    let dir = tempdir().expect("dir");
    let folder = dir.path();
    let w1 = serde_json::json!({"modules": {"todo": {}}});
    let w2 = serde_json::json!({"modules": {"spine": {}}});
    write_worker(folder, "abc123", &w1);
    write_worker(folder, "def456", &w2);

    let mut reader = StateReader::new();
    let workers = StateReader::list_workers(folder).expect("list");
    assert_eq!(workers, vec!["abc123", "def456"]);

    let read = reader.read_worker(folder, "abc123").expect("read");
    assert_eq!(read, w1);
}

#[test]
fn read_config_hydrates_legacy_threads_layout() {
    let dir = tempdir().expect("dir");
    let folder = dir.path();
    let cfg = serde_json::json!({"modules": {"threads": {"threads": [
        {"id": "T1", "name": "slim"},
        {"id": "T2", "name": "legacy", "messages": [{"content": "inline"}]},
        {"id": "T3", "name": "no file"},
    ]}}});
    write_config(folder, &cfg);
    let tdir = folder.join(CP_DIR).join(LEGACY_AGENTS_DIR);
    fs::create_dir_all(&tdir).expect("mkdir");
    let t1 = serde_json::json!({"id": "T1", "messages": [{"content": "from file"}]});
    fs::write(tdir.join("T1.json"), serde_json::to_vec(&t1).expect("ser")).expect("write");

    let mut reader = StateReader::new();
    let read = reader.read_config(folder).expect("read");
    let at = |i: &str| read.pointer(i).and_then(Value::as_str);
    assert_eq!(at("/modules/agents/agents/0/messages/0/content"), Some("from file"));
    assert_eq!(at("/modules/agents/agents/1/messages/0/content"), Some("inline"));
    assert!(read.pointer("/modules/agents/agents/2/messages").is_none());
}

#[test]
fn read_config_hydrates_from_human_group() {
    let dir = tempdir().expect("dir");
    let folder = dir.path();
    let cfg = serde_json::json!({"modules": {"agents": {"agents": [
        {"id": "A1", "name": "grouped", "human_group_id": "G4"},
        {"id": "A2", "name": "pre-groups"},
    ]}}});
    write_config(folder, &cfg);
    let gdir = folder.join(CP_DIR).join(GROUPS_DIR);
    fs::create_dir_all(&gdir).expect("mkdir");
    fs::write(gdir.join("G4.json"), br#"{"id":"G4","messages":[{"content":"from group"}]}"#).expect("write");
    let adir = folder.join(CP_DIR).join(AGENTS_DIR);
    fs::create_dir_all(&adir).expect("mkdir");
    fs::write(adir.join("A1.json"), br#"{"messages":[{"content":"stale backup"}]}"#).expect("write");
    fs::write(adir.join("A2.json"), br#"{"messages":[{"content":"from agent file"}]}"#).expect("write");

    let mut reader = StateReader::new();
    let read = reader.read_config(folder).expect("read");
    let at = |i: &str| read.pointer(i).and_then(Value::as_str);
    assert_eq!(at("/modules/agents/agents/0/messages/0/content"), Some("from group"));
    assert_eq!(at("/modules/agents/agents/1/messages/0/content"), Some("from agent file"));
}

#[test]
fn read_config_interleaves_activity_traces() {
    let dir = tempdir().expect("dir");
    let folder = dir.path();
    let cfg = serde_json::json!({"modules": {"agents": {"agents": [{"id": "A1", "human_group_id": "G4"}]}}});
    write_config(folder, &cfg);
    let gdir = folder.join(CP_DIR).join(GROUPS_DIR);
    fs::create_dir_all(&gdir).expect("mkdir");
    let chat = br#"{"messages":[{"content":"a","timestamp":1},{"content":"c","timestamp":5}]}"#;
    fs::write(gdir.join("G4.json"), chat).expect("write");
    let adir = folder.join(CP_DIR).join(ACTIVITY_DIR);
    fs::create_dir_all(&adir).expect("mkdir");
    let traces = br#"{"id":"A1","messages":[{"content":"b","timestamp":3},{"content":"d","timestamp":5}]}"#;
    fs::write(adir.join("A1.json"), traces).expect("write");

    let mut reader = StateReader::new();
    let read = reader.read_config(folder).expect("read");
    let order: Vec<&str> = read
        .pointer("/modules/agents/agents/0/messages")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(|m| m.get("content").and_then(Value::as_str)).collect())
        .unwrap_or_default();
    assert_eq!(order, vec!["a", "b", "c", "d"]);
}

#[test]
fn read_config_overlays_module_files() {
    let dir = tempdir().expect("dir");
    let folder = dir.path();
    let cfg = serde_json::json!({"modules": {"todo": {"todos": ["inline"]}, "memory": {"m": 1u64}}});
    write_config(folder, &cfg);
    let mdir = folder.join(CP_DIR).join(MODULES_DIR);
    fs::create_dir_all(&mdir).expect("mkdir");
    fs::write(mdir.join("todo.json"), br#"{"todos":["file"]}"#).expect("write");
    fs::write(mdir.join("tree.json"), br#"{"filter":"x"}"#).expect("write");

    let mut reader = StateReader::new();
    let read = reader.read_config(folder).expect("read");
    let at = |i: &str| read.pointer(i).and_then(Value::as_str);
    assert_eq!(at("/modules/todo/todos/0"), Some("file"));
    assert_eq!(at("/modules/tree/filter"), Some("x"));
    assert_eq!(read.pointer("/modules/memory/m").and_then(Value::as_u64), Some(1));
}

#[test]
fn list_workers_on_missing_dir_returns_empty() {
    let dir = tempdir().expect("dir");
    let workers = StateReader::list_workers(dir.path()).expect("list");
    assert!(workers.is_empty());
}
