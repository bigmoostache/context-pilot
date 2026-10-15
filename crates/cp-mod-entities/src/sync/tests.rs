//! Regression tests for the round-2 sync bugs (doc ids, casing, drops,
//! indirect writes, value cap, filter escaping).

use rusqlite::Connection;

use super::affected::detect;
use super::{VALUE_CAP, build_docs, cap_value, table_filter};
use crate::db;

/// In-memory DB with FKs enforced, like `db::open`.
fn conn(schema: &str) -> Connection {
    let c = Connection::open_in_memory().expect("open in-memory db");
    c.execute_batch("PRAGMA foreign_keys = ON;").expect("pragma");
    c.execute_batch(schema).expect("schema");
    c
}

/// Run a batch and return its write effect as plain vectors.
fn effect(c: &Connection, sql: &str) -> (Vec<String>, Vec<String>) {
    let before = db::user_table_names(c);
    c.execute_batch(sql).expect("batch");
    let stmts = crate::parse::split_statements(sql);
    let e = detect(c, &before, &stmts);
    (e.dirty.into_iter().collect(), e.dropped.into_iter().collect())
}

fn ids(c: &Connection, table: &str) -> Vec<String> {
    build_docs(c, table)
        .expect("docs")
        .iter()
        .map(|d| d.get("id").and_then(serde_json::Value::as_str).unwrap_or_default().to_owned())
        .collect()
}

/// B1: integer-keyed WITHOUT ROWID rows get distinct ids.
#[test]
fn without_rowid_integer_pk_ids_distinct() {
    let c = conn("CREATE TABLE w (n INTEGER PRIMARY KEY, v TEXT) WITHOUT ROWID; INSERT INTO w VALUES (1,'a'),(2,'b');");
    assert_eq!(ids(&c, "w"), vec!["w__k1", "w__k2"]);
}

/// B1: composite and unsafe keys never collide after sanitizing.
#[test]
fn without_rowid_composite_and_unsafe_keys() {
    let c = conn(
        "CREATE TABLE w (a TEXT, b TEXT, PRIMARY KEY (a, b)) WITHOUT ROWID;
         INSERT INTO w VALUES ('x y','1'),('x_y','1');",
    );
    let got = ids(&c, "w");
    assert_eq!(got.len(), 2);
    assert_ne!(got.first(), got.get(1));
}

/// B2: doc ids use the lowercased table, `entity_table` the stored case.
#[test]
fn mixed_case_table_ids_stable() {
    let c = conn("CREATE TABLE Runs (x TEXT); INSERT INTO Runs VALUES ('a');");
    let docs = build_docs(&c, "Runs").expect("docs");
    let doc = docs.first().expect("one doc");
    assert_eq!(doc.get("id").and_then(serde_json::Value::as_str), Some("runs__1"));
    assert_eq!(doc.get("entity_table").and_then(serde_json::Value::as_str), Some("Runs"));
}

/// B2: a lowercase write resolves to the stored case.
#[test]
fn dirty_name_uses_stored_case() {
    let c = conn("CREATE TABLE Runs (x TEXT);");
    let (dirty, dropped) = effect(&c, "insert into runs values ('a')");
    assert_eq!(dirty, vec!["Runs"]);
    assert!(dropped.is_empty());
}

/// B3: in `DROP a; CREATE b` only `a` is dropped and `b` is dirty.
#[test]
fn mixed_drop_create_batch() {
    let c = conn("CREATE TABLE a (x); CREATE TABLE keep (y);");
    let (dirty, dropped) = effect(&c, "DROP TABLE a; CREATE TABLE b (z)");
    assert_eq!(dirty, vec!["b"]);
    assert_eq!(dropped, vec!["a"]);
}

/// B4: a CTE-prefixed delete marks every table dirty.
#[test]
fn cte_delete_marks_all_dirty() {
    let c = conn("CREATE TABLE t (x); CREATE TABLE u (y); INSERT INTO t VALUES (1);");
    let (dirty, _) = effect(&c, "WITH d AS (SELECT 1) DELETE FROM t WHERE x IN (SELECT * FROM d)");
    assert_eq!(dirty, vec!["t", "u"]);
}

/// B4: a trigger writing another table marks it dirty.
#[test]
fn trigger_target_marked_dirty() {
    let c = conn(
        "CREATE TABLE t (x); CREATE TABLE log (y);
         CREATE TRIGGER tr AFTER INSERT ON t BEGIN INSERT INTO log VALUES (NEW.x); END;",
    );
    let (dirty, _) = effect(&c, "INSERT INTO t VALUES (1)");
    assert!(dirty.contains(&"log".to_owned()), "got {dirty:?}");
}

/// B4: a cascading delete marks the child table dirty.
#[test]
fn cascade_child_marked_dirty() {
    let c = conn(
        "CREATE TABLE p (id INTEGER PRIMARY KEY);
         CREATE TABLE ch (pid INTEGER REFERENCES p(id) ON DELETE CASCADE);
         INSERT INTO p VALUES (1); INSERT INTO ch VALUES (1);",
    );
    let (dirty, _) = effect(&c, "DELETE FROM p");
    assert_eq!(dirty, vec!["ch", "p"]);
}

/// Plain write without triggers/cascades stays narrow.
#[test]
fn plain_write_is_narrow() {
    let c = conn("CREATE TABLE t (x); CREATE TABLE u (y);");
    let (dirty, _) = effect(&c, "INSERT INTO t VALUES (1); SELECT * FROM u");
    assert_eq!(dirty, vec!["t"]);
}

/// Value cap counts chars and never splits a multi-byte char.
#[test]
fn cap_value_char_boundary() {
    let long = "\u{e9}".repeat(VALUE_CAP.saturating_add(10));
    let capped = cap_value(long);
    assert_eq!(capped.chars().count(), VALUE_CAP.saturating_add(1));
    assert!(capped.ends_with('\u{2026}'));
    assert_eq!(cap_value("short".to_owned()), "short");
}

/// Quotes and backslashes in a table name are escaped in the filter.
#[test]
fn filter_escapes_quotes() {
    assert_eq!(table_filter("a'b\\c"), "entity_table = 'a\\'b\\\\c'");
}
