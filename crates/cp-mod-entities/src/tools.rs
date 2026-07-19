//! SQL execution engine: classification, splitting, execution, error enrichment.

use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

use crate::errors::enrich_error;
use crate::format::{self, extract_table_name, format_cell, format_markdown_table};
use crate::parse::{SqlKind, classify, split_statements};
use crate::result_panel::{self, LivePanelMeta};
use crate::{db, migrations};

// =============================================================================
// Constants
// =============================================================================

/// Results exceeding either limit go to a panel instead of inline.
/// Matches console `easy_bash` thresholds.
const INLINE_MAX_LINES: usize = 150;
/// Maximum inline byte count before routing to a panel.
const INLINE_MAX_BYTES: usize = 8000;

/// Warning appended to panel-creating tool results.
const PANEL_WARNING: &str = "\n\nIMPORTANT: Results live in this panel. Act on the information FIRST \
    (write files, answer questions, store in scratchpad, etc.), THEN close the panel. Closing it \
    IMMEDIATELY and IRREVERSIBLY erases all content from your context — you cannot recall it from \
    memory afterward. Never close-then-act; always act-then-close.";

// =============================================================================
// Main execution entry point
// =============================================================================

/// Execute the `entity_sql` tool call.
///
/// Opens a per-call connection, classifies the SQL, executes, formats the
/// result, and handles post-execution bookkeeping (panel refresh, migration
/// capture, dump regeneration, schema cache update).
pub(crate) fn execute(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("entity_sql");

    // ── Parse parameters ────────────────────────────────────────────────
    let sql_param = tool.input.get("sql").and_then(serde_json::Value::as_str).unwrap_or_default();
    let request_path = tool.input.get("request_path").and_then(serde_json::Value::as_str).unwrap_or_default();
    let live = tool.input.get("live").and_then(serde_json::Value::as_bool).unwrap_or(false);
    let output_path_str = tool.input.get("output_path").and_then(serde_json::Value::as_str).unwrap_or_default();
    let dry_run = tool.input.get("dry_run").and_then(serde_json::Value::as_bool).unwrap_or(false);

    let has_sql = !sql_param.trim().is_empty();
    let has_request = !request_path.trim().is_empty();
    let has_output = !output_path_str.trim().is_empty();

    // ── Validate mutual exclusions ──────────────────────────────────────
    if has_sql && has_request {
        return err(tool, "Cannot provide both `sql` and `request_path`. Use one or the other.");
    }
    if !has_sql && !has_request {
        return err(tool, "Must provide either `sql` or `request_path`.");
    }
    if live && has_output {
        return err(tool, "`live` and `output_path` are incompatible.");
    }
    if live && dry_run {
        return err(tool, "`live` and `dry_run` are incompatible.");
    }

    // ── Resolve SQL source ──────────────────────────────────────────────
    let sql_owned: String;
    let sql: &str = if has_request {
        let path = Path::new(request_path);
        if !path.exists() {
            return err(tool, &format!("File not found: {request_path}"));
        }
        match std::fs::read_to_string(path) {
            Ok(content) => {
                sql_owned = content;
                &sql_owned
            }
            Err(e) => return err(tool, &format!("Failed to read {request_path}: {e}")),
        }
    } else {
        sql_param
    };

    if sql.trim().is_empty() {
        return err(tool, "SQL is empty (file contained no SQL).");
    }

    // Split statements early for classification and empty-input detection.
    // This filters out comment-only and empty segments.
    let stmts = split_statements(sql);
    if stmts.is_empty() {
        return err(tool, "No SQL statements found (input is only comments).");
    }

    let es = crate::types::EntitiesState::get(state);
    let db_path = es.db_path.clone();
    let dump_path = es.dump_path.clone();
    let migrations_dir = es.migrations_dir.clone();

    let conn = match db::open(&db_path) {
        Ok(c) => c,
        Err(e) => return err(tool, &e),
    };

    // Classify based on the first clean statement (not raw input)
    // so that leading semicolons / comments don't confuse classification.
    let kind = classify(stmts.first().copied().unwrap_or(sql));

    // ── Validate live restriction ───────────────────────────────────────
    if live && kind != SqlKind::Select {
        return err(tool, "`live=true` is only supported for SELECT/EXPLAIN/PRAGMA queries.");
    }

    // ── Execute ─────────────────────────────────────────────────────────
    let result_content = if dry_run {
        execute_dry_run(&conn, sql, kind, state)
    } else {
        match kind {
            SqlKind::Select => execute_select(&conn, sql, state),
            SqlKind::Dml => execute_dml(&conn, sql),
            SqlKind::Ddl => execute_ddl(&conn, sql, &dump_path, &migrations_dir),
        }
    };

    // ── Route results ───────────────────────────────────────────────────
    let (content, is_error, preserves_tempo) = match result_content {
        Ok(text) => {
            if live {
                // Live → always panel, store SQL for periodic re-execution
                let sql_preview: String = sql.chars().take(60).collect();
                let title = format!("entity_sql: {sql_preview}");
                let panel_id = result_panel::create_live_result_panel(
                    state,
                    &title,
                    &text,
                    LivePanelMeta { sql, db_path: &db_path.to_string_lossy() },
                );
                let summary = format!("Live query panel created: {panel_id}. Auto-refreshes every 2s.{PANEL_WARNING}");
                (summary, false, false)
            } else if has_output {
                // output_path → write to file + create static panel
                let out = Path::new(output_path_str);
                if let Some(parent) = out.parent() {
                    let _d = std::fs::create_dir_all(parent);
                }
                if let Err(e) = std::fs::write(out, &text) {
                    return err(tool, &format!("Failed to write to {output_path_str}: {e}"));
                }
                let sql_preview: String = sql.chars().take(60).collect();
                let title = format!("entity_sql: {sql_preview}");
                let panel_id = result_panel::create_result_panel(state, &title, &text);
                let summary = format!(
                    "Results written to `{output_path_str}`. Also available in panel {panel_id}.{PANEL_WARNING}"
                );
                (summary, false, false)
            } else {
                // Size-based routing: small → inline + tempo, big → panel
                let line_count = text.lines().count();
                let byte_count = text.len();
                if line_count > INLINE_MAX_LINES || byte_count > INLINE_MAX_BYTES {
                    let sql_preview: String = sql.chars().take(60).collect();
                    let title = format!("entity_sql: {sql_preview}");
                    let panel_id = result_panel::create_result_panel(state, &title, &text);
                    let summary = format!("{line_count} lines returned — results in panel {panel_id}.{PANEL_WARNING}");
                    (summary, false, false)
                } else {
                    let note = if kind == SqlKind::Select || dry_run {
                        ""
                    } else {
                        "\n\n(The Entities panel now reflects the updated database state.)"
                    };
                    (format!("{text}{note}"), false, true)
                }
            }
        }
        Err(e) => {
            let schema = db::introspect(&conn, &db_path);
            (enrich_error(&e, &schema), true, false)
        }
    };

    // ── Post-execution: Meilisearch sync (skip on dry_run or read-only) ─
    if !is_error && !dry_run && kind != SqlKind::Select {
        let affected = crate::sync::extract_affected_tables(&stmts);
        let upper = sql.to_uppercase();
        let is_drop = upper.contains("DROP TABLE") || upper.contains("DROP TABLE IF EXISTS");
        let es_sync = crate::types::EntitiesState::get_mut(state);
        for table in &affected {
            if is_drop {
                es_sync.dropped_tables.push(table.clone());
                let _removed = es_sync.dirty_tables.remove(table);
            } else {
                let _inserted = es_sync.dirty_tables.insert(table.clone());
            }
        }
        crate::sync::flush_sync(state);
    }

    // ── Post-execution: refresh schema cache (skip on dry_run) ──────────
    if !dry_run {
        let fresh_cache = db::introspect(&conn, &db_path);
        let es_mut = crate::types::EntitiesState::get_mut(state);
        es_mut.schema_cache = Some(fresh_cache);
        state.touch_panel(Kind::ENTITIES);
    }

    ToolResult {
        tool_use_id: tool.id.clone(),
        content,
        display: None,
        tldr: None,
        is_error,
        preserves_tempo,
        tool_name: tool.name.clone(),
    }
}

// =============================================================================
// Dry-run execution
// =============================================================================

/// Execute SQL inside a savepoint that is immediately rolled back.
///
/// Returns the same result the normal path would, but with a `[DRY RUN]`
/// header and no persistent side effects. Works for all SQL types — `SQLite`
/// supports transactional DDL.
fn execute_dry_run(conn: &Connection, sql: &str, kind: SqlKind, state: &State) -> Result<String, String> {
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
fn execute_select(conn: &Connection, sql: &str, state: &State) -> Result<String, String> {
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
fn execute_dml(conn: &Connection, sql: &str) -> Result<String, String> {
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
        match &result {
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

/// Execute DDL. Writes migration + regenerates dump.
fn execute_ddl(conn: &Connection, sql: &str, dump_path: &Path, migrations_dir: &Path) -> Result<String, String> {
    conn.execute_batch(sql).map_err(|e| format!("{e}"))?;

    // Write migration file
    let filename = migrations::write_migration(conn, migrations_dir, sql)?;

    // Regenerate full dump
    if let Err(e) = db::dump_to_file(conn, dump_path) {
        log::warn!("Failed to regenerate dump after DDL: {e}");
    }

    Ok(format!("Schema updated. Migration saved: {filename}"))
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
    let hint = enrichment.as_ref().map(|(name, count)| (name.as_str(), *count));
    format::query_to_markdown(conn, sql, hint)
}

/// Build an error `ToolResult`.
fn err(tool: &ToolUse, msg: &str) -> ToolResult {
    ToolResult {
        tool_use_id: tool.id.clone(),
        content: msg.to_owned(),
        display: None,
        tldr: None,
        is_error: true,
        preserves_tempo: false,
        tool_name: tool.name.clone(),
    }
}

use rusqlite::Connection;
use std::path::Path;

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
