//! Which tables a successful write touched, read from `SQLite` itself.
//!
//! SQL-text parsing alone misses writes: `WITH … DELETE`, rows written by
//! triggers, rows removed by `ON DELETE CASCADE`. It also loses the
//! table's real case and can't tell which table in a mixed batch was dropped.
//! So drops and creates come from diffing `sqlite_master` before and after
//! the call. Statement parsing is only a hint: whenever a write could reach
//! a table it doesn't name, every table is marked dirty.

use std::collections::BTreeSet;

use rusqlite::Connection;

use crate::db;

/// Tables to re-index and tables whose docs must be deleted, in their stored
/// (`sqlite_master`) case.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct WriteEffect {
    /// Tables that still exist and may have changed rows.
    pub dirty: BTreeSet<String>,
    /// Tables present before the call and gone after it.
    pub dropped: BTreeSet<String>,
}

/// Leading keywords of statements that never change rows.
const NO_WRITE_KEYWORDS: &[&str] =
    &["SELECT", "EXPLAIN", "PRAGMA", "BEGIN", "COMMIT", "END", "ROLLBACK", "SAVEPOINT", "RELEASE", "ANALYZE"];

/// Compute the effect of a successful write.
///
/// `before` is the user-table list taken just before execution; `stmts` is
/// the split batch that ran.
pub(crate) fn detect(conn: &Connection, before: &[String], stmts: &[&str]) -> WriteEffect {
    let after = db::user_table_names(conn);
    let dropped: BTreeSet<String> = before.iter().filter(|t| !after.contains(t)).cloned().collect();
    let all_dirty = || after.iter().cloned().collect::<BTreeSet<String>>();

    if has_indirect_writers(conn) {
        return WriteEffect { dirty: all_dirty(), dropped };
    }

    let mut dirty: BTreeSet<String> = after.iter().filter(|t| !before.contains(t)).cloned().collect();
    for stmt in stmts {
        if !writes_rows(stmt) {
            continue;
        }
        let resolved = super::table_from_statement(stmt).and_then(|name| canonical(&after, &name));
        match resolved {
            Some(table) => {
                let _new = dirty.insert(table);
            }
            // DML naming no surviving table we can find (CTE, unusual syntax):
            // re-index everything rather than miss rows.
            None if is_dml(stmt) => return WriteEffect { dirty: all_dirty(), dropped },
            None => {}
        }
    }
    WriteEffect { dirty, dropped }
}

/// Stored-case name of `name` among `tables` (`SQLite` names are
/// case-insensitive).
fn canonical(tables: &[String], name: &str) -> Option<String> {
    tables.iter().find(|t| t.eq_ignore_ascii_case(name)).cloned()
}

/// First keyword of a statement, upper-cased, comments skipped.
fn first_keyword(stmt: &str) -> String {
    crate::parse::strip_leading_comments(stmt)
        .trim_start()
        .chars()
        .take_while(char::is_ascii_alphabetic)
        .collect::<String>()
        .to_uppercase()
}

/// Whether a statement can change table contents or schema.
fn writes_rows(stmt: &str) -> bool {
    let kw = first_keyword(stmt);
    !kw.is_empty() && !NO_WRITE_KEYWORDS.contains(&kw.as_str())
}

/// Whether a statement is row-level DML (including a `WITH` prefix).
fn is_dml(stmt: &str) -> bool {
    matches!(first_keyword(stmt).as_str(), "INSERT" | "UPDATE" | "DELETE" | "REPLACE" | "WITH")
}

/// Any trigger, or any foreign key with a non-default `ON DELETE`/`ON UPDATE`
/// action: a write to one table can then change rows in another.
fn has_indirect_writers(conn: &Connection) -> bool {
    let triggers: i64 =
        conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger'", [], |r| r.get(0)).unwrap_or(0);
    if triggers > 0 {
        return true;
    }
    let cascading: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master m, pragma_foreign_key_list(m.name) f
             WHERE m.type = 'table'
               AND (f.on_delete NOT IN ('NO ACTION', 'RESTRICT')
                 OR f.on_update NOT IN ('NO ACTION', 'RESTRICT'))",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    cascading > 0
}
