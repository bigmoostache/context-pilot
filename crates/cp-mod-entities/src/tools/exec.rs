//! Per-kind SQL execution: dry-run, SELECT, DML, DDL, and query→markdown.
//!
//! Split from `tools.rs` for the line budget. `tools::execute` classifies the
//! statement and dispatches here; the routing/bookkeeping stays in `tools.rs`.

use rusqlite::Connection;
use std::path::Path;

use cp_base::state::runtime::State;

use crate::format::{self, extract_table_name, format_cell, format_markdown_table};
use crate::parse::{SqlKind, split_statements};
use crate::{db, migrations};

// =============================================================================
// Dry-run execution
// =============================================================================

/// Execute SQL inside a savepoint that is immediately rolled back.
///
/// Returns the same result the normal path would, but with a `[DRY RUN]`
/// header and no persistent side effects. Works for all SQL types — `SQLite`
/// supports transactional DDL.
pub(super) fn execute_dry_run(conn: &Connection, sql: &str, kind: SqlKind, state: &State) -> Result<String, String> {
    conn.execute_batch("SAVEPOINT dry_run_sp").map_err(|e| format!("{e}"))?;

    let stmts = split_statements(sql);
    let result = match kind {
        SqlKind::Select => execute_select(conn, sql, state),
        SqlKind::Dml => execute_dml_stmts(conn, &stmts),
        SqlKind::Ddl => {
            conn.execute_batch(sql).map_err(|e| format!("{e}")).map(|()| "Schema changes would be applied.".to_owned())
        }
    };

    // Always roll back — even on error the savepoint must be cleaned up
    let _rb = conn.execute_batch("ROLLBACK TO dry_run_sp");
    let _rel = conn.execute_batch("RELEASE dry_run_sp");

    result.map(|text| format!("[DRY RUN — no changes applied]\n\n{text}"))
}

// =============================================================================
// Per-kind execution
// =============================================================================

/// Execute a SELECT / EXPLAIN / PRAGMA query and format results as markdown.
pub(super) fn execute_select(conn: &Connection, sql: &str, state: &State) -> Result<String, String> {
    let stmts = split_statements(sql);
    let last = stmts.last().copied().unwrap_or(sql);

    // Execute all but the last (side-effect statements like pragmas)
    for stmt in stmts.iter().take(stmts.len().saturating_sub(1)) {
        conn.execute_batch(stmt).map_err(|e| format!("{e}"))?;
    }

    // Execute the last statement as a query
    query_to_markdown(conn, last, state)
}

/// Execute a DML statement. Handles `RETURNING` clauses.
///
/// Multi-statement batches are wrapped in an implicit transaction for
/// atomicity (all-or-nothing), unless the user already controls
/// transactions explicitly with `BEGIN`.
pub(super) fn execute_dml(conn: &Connection, sql: &str) -> Result<String, String> {
    let stmts = split_statements(sql);

    // Wrap multi-statement batches in an implicit transaction for atomicity,
    // unless the user already starts with BEGIN (explicit transaction control).
    let needs_implicit_tx =
        stmts.len() > 1 && !stmts.first().is_some_and(|s| s.trim().to_uppercase().starts_with("BEGIN"));

    if needs_implicit_tx {
        conn.execute_batch("BEGIN").map_err(|e| format!("{e}"))?;
    }

    let result = execute_dml_stmts(conn, &stmts);

    if needs_implicit_tx {
        match result.as_ref() {
            Ok(_) => conn.execute_batch("COMMIT").map_err(|e| format!("{e}"))?,
            Err(_) => {
                let _rb = conn.execute_batch("ROLLBACK");
            }
        }
    }

    result
}

/// Inner loop for DML execution — separated so the caller can wrap in a transaction.
///
/// Each statement is dispatched by whether it **returns rows**, decided from the
/// prepared statement's `column_count()` rather than a brittle batch-level
/// `RETURNING` substring scan. This lets a DML batch end with (or contain) a
/// plain `SELECT` — e.g. `DELETE …; INSERT …; SELECT * FROM t;` — without hitting
/// rusqlite's "Execute returned results" error from calling `execute()` on a
/// row-returning statement.
fn execute_dml_stmts(conn: &Connection, stmts: &[&str]) -> Result<String, String> {
    let mut total_affected = 0usize;
    for (i, stmt) in stmts.iter().enumerate() {
        let is_last = i == stmts.len().saturating_sub(1);

        let mut prep = conn.prepare(stmt).map_err(|e| format!("{e}"))?;
        let returns_rows = prep.column_count() > 0;

        if returns_rows {
            if is_last {
                // Final row-returning statement (SELECT or RETURNING) — format as table.
                let col_names: Vec<String> = prep.column_names().iter().map(|s| (*s).to_owned()).collect();
                let mut rows_data: Vec<Vec<String>> = Vec::new();

                let mut rows = prep.query([]).map_err(|e| format!("{e}"))?;
                while let Some(row) = rows.next().map_err(|e| format!("{e}"))? {
                    let mut vals = Vec::with_capacity(col_names.len());
                    for idx in 0..col_names.len() {
                        vals.push(format_cell(row, idx));
                    }
                    rows_data.push(vals);
                }

                let count = rows_data.len();
                let capped = rows_data.get(..format::MAX_RESULT_ROWS).unwrap_or(&rows_data);
                let table = format_markdown_table(&col_names, capped);
                let suffix = if count > format::MAX_RESULT_ROWS {
                    format!("({count} returned, showing first {})", format::MAX_RESULT_ROWS)
                } else {
                    format!("({count} returned)")
                };
                if total_affected > 0 {
                    return Ok(format!("{total_affected} row(s) affected.\n\n{table}\n\n{suffix}"));
                }
                return Ok(format!("{table}\n\n{suffix}"));
            }

            // Non-last row-returning statement: run it for its side effects, discard rows.
            let mut rows = prep.query([]).map_err(|e| format!("{e}"))?;
            while rows.next().map_err(|e| format!("{e}"))?.is_some() {}
        } else {
            let affected = prep.execute([]).map_err(|e| format!("{e}"))?;
            total_affected = total_affected.saturating_add(affected);
        }
    }

    Ok(format!("{total_affected} row(s) affected."))
}

/// Execute DDL, record its migration, and regenerate the dump.
///
/// The schema change, the `_meta` row and the migration file commit together
/// under `BEGIN IMMEDIATE`, which takes the shared DB's write lock up front:
/// two agents can't both read the same `MAX(migration_id)`, and a failure
/// rolls back the change and removes the file. A batch that controls its own
/// transactions runs as-is; its migration is then recorded in a second
/// `BEGIN IMMEDIATE`.
pub(super) fn execute_ddl(
    conn: &Connection,
    sql: &str,
    dump_path: &Path,
    migrations_dir: &Path,
) -> Result<String, String> {
    let filename = if has_tx_control(sql) {
        conn.execute_batch(sql).map_err(|e| format!("{e}"))?;
        in_write_tx(conn, |c| migrations::write_migration(c, migrations_dir, sql))?
    } else {
        in_write_tx(conn, |c| {
            c.execute_batch(sql).map_err(|e| format!("{e}"))?;
            migrations::write_migration(c, migrations_dir, sql)
        })?
    };

    if let Err(e) = db::dump_to_file(conn, dump_path) {
        log::warn!("Failed to regenerate dump after DDL: {e}");
    }

    Ok(format!("Schema updated. Migration saved: {filename}"))
}

/// Run `body` inside `BEGIN IMMEDIATE … COMMIT`. On error (or a failed
/// commit) roll back and delete the migration file `body` wrote.
fn in_write_tx(
    conn: &Connection,
    body: impl FnOnce(&Connection) -> Result<(String, std::path::PathBuf), String>,
) -> Result<String, String> {
    conn.execute_batch("BEGIN IMMEDIATE").map_err(|e| format!("{e}"))?;
    let (filename, path) = match body(conn) {
        Ok(written) => written,
        Err(e) => {
            let _rb = conn.execute_batch("ROLLBACK");
            return Err(e);
        }
    };
    if let Err(e) = conn.execute_batch("COMMIT") {
        let _rb = conn.execute_batch("ROLLBACK");
        let _rm = std::fs::remove_file(&path);
        return Err(format!("{e}"));
    }
    Ok(filename)
}

/// Whether the batch manages transactions itself (nesting `BEGIN` would fail).
fn has_tx_control(sql: &str) -> bool {
    split_statements(sql).iter().any(|s| {
        let kw = crate::parse::strip_leading_comments(s)
            .trim_start()
            .chars()
            .take_while(char::is_ascii_alphabetic)
            .collect::<String>()
            .to_uppercase();
        matches!(kw.as_str(), "BEGIN" | "COMMIT" | "ROLLBACK" | "SAVEPOINT" | "RELEASE")
    })
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Execute a query and format results as a markdown table.
fn query_to_markdown(conn: &Connection, sql: &str, state: &State) -> Result<String, String> {
    // Build enrichment hint for empty-result context
    let enrichment = extract_table_name(sql).and_then(|tbl| {
        let es = crate::types::EntitiesState::get(state);
        let cache = es.schema_cache.as_ref()?;
        let info = cache.tables.iter().find(|t| t.name.eq_ignore_ascii_case(&tbl))?;
        Some((info.name.clone(), info.row_count))
    });
    let hint = enrichment.as_ref().map(|entry| (entry.0.as_str(), entry.1));
    format::query_to_markdown(conn, sql, hint)
}

#[cfg(test)]
mod tests {
    use super::{Connection, execute_dml};

    fn conn() -> Connection {
        let c = Connection::open_in_memory().expect("open in-memory db");
        c.execute_batch("CREATE TABLE t (a TEXT, b TEXT);").expect("create table");
        c
    }

    /// Regression: a DML batch ending in a plain `SELECT` (no `RETURNING`) must
    /// format the trailing query as a table instead of erroring with rusqlite's
    /// "Execute returned results — did you mean to call query?".
    #[test]
    fn dml_batch_with_trailing_select() {
        let c = conn();
        let out = execute_dml(
            &c,
            "INSERT INTO t(a,b) VALUES ('p','q'); INSERT INTO t(a,b) VALUES ('x','y'); SELECT a FROM t ORDER BY a;",
        )
        .expect("batch should succeed");
        assert!(out.contains("2 row(s) affected"), "got: {out}");
        assert!(out.contains("| p |") && out.contains("| x |"), "got: {out}");
        assert!(out.contains("(2 returned)"), "got: {out}");
    }

    /// A trailing `SELECT` after a single DML statement (the verify-your-work
    /// pattern) must also work.
    #[test]
    fn single_dml_then_select() {
        let c = conn();
        let out = execute_dml(&c, "INSERT INTO t(a,b) VALUES ('z','w'); SELECT b FROM t;").expect("should succeed");
        assert!(out.contains("| w |"), "got: {out}");
    }

    /// `RETURNING` still formats as a table after dropping the `has_returning` flag.
    #[test]
    fn returning_still_formats_table() {
        let c = conn();
        let out =
            execute_dml(&c, "INSERT INTO t(a,b) VALUES ('r','s') RETURNING a, b;").expect("returning should succeed");
        assert!(out.contains("| r | s |"), "got: {out}");
    }

    /// A failing DDL batch rolls back its earlier statements and writes no
    /// migration (B5 atomicity).
    #[test]
    fn failed_ddl_rolls_back_and_writes_nothing() {
        let c = conn();
        c.execute_batch("CREATE TABLE _meta (migration_id INTEGER PRIMARY KEY, filename TEXT NOT NULL);")
            .expect("_meta");
        let dir = std::env::temp_dir().join(format!("cp-entities-ddl-{}", std::process::id()));
        let _rm = std::fs::remove_dir_all(&dir);
        let dump = dir.join("dump.sql");
        let _e = super::execute_ddl(&c, "CREATE TABLE ok (x); CREATE TABLE t (dup)", &dump, &dir)
            .expect_err("duplicate table must fail");
        let ok_exists: i64 =
            c.query_row("SELECT COUNT(*) FROM sqlite_master WHERE name = 'ok'", [], |r| r.get(0)).expect("query");
        assert_eq!(ok_exists, 0);
        assert!(crate::migrations::list_files(&dir).is_empty());
        let ok = super::execute_ddl(&c, "CREATE TABLE ok (x)", &dump, &dir).expect("ddl");
        assert!(ok.contains("0001_"), "{ok}");
        let _rm2 = std::fs::remove_dir_all(&dir);
    }

    /// A `;` inside a trailing line comment must not break the batch, and the
    /// final `SELECT` must still render (combines splitter + dispatch fixes).
    #[test]
    fn comment_semicolon_plus_trailing_select() {
        let c = conn();
        let out = execute_dml(&c, "INSERT INTO t(a,b) VALUES ('m','n'); -- note; with ; semicolons\nSELECT a FROM t;")
            .expect("should succeed");
        assert!(out.contains("| m |"), "got: {out}");
    }
}
