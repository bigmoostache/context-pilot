//! SQL execution engine: classification, splitting, execution, error enrichment.

use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

use crate::errors::enrich_error;
use crate::{db, migrations};

// =============================================================================
// SQL classification
// =============================================================================

/// Broad category of a SQL statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SqlKind {
    /// `SELECT`, `EXPLAIN`, `PRAGMA` (read-only).
    Select,
    /// `INSERT`, `UPDATE`, `DELETE` (data manipulation).
    Dml,
    /// `CREATE`, `ALTER`, `DROP` (schema change).
    Ddl,
}

/// Classify a SQL statement by its first keyword.
///
/// Leading SQL comments (`--` line and `/* */` block) are stripped before
/// classification. CTEs (`WITH ... SELECT` vs `WITH ... INSERT`) are detected
/// by scanning for DML/DDL keywords after the CTE. Default is [`SqlKind::Dml`]
/// (conservative).
pub(crate) fn classify(sql: &str) -> SqlKind {
    let stripped = strip_leading_comments(sql);
    let upper = stripped.trim().to_uppercase();
    let first_word: String = upper.chars().take_while(char::is_ascii_alphabetic).collect();

    match first_word.as_str() {
        "SELECT" | "EXPLAIN" | "PRAGMA" => SqlKind::Select,
        "CREATE" | "ALTER" | "DROP" => SqlKind::Ddl,
        "WITH" => classify_cte(&upper),
        _ => SqlKind::Dml, // conservative: INSERT/UPDATE/DELETE/REPLACE and unknown
    }
}

/// Classify a CTE by scanning for DML/DDL keywords after `WITH`.
fn classify_cte(upper: &str) -> SqlKind {
    // Look for DDL keywords
    if upper.contains("CREATE ") || upper.contains("ALTER ") || upper.contains("DROP ") {
        return SqlKind::Ddl;
    }
    // Look for DML keywords
    if upper.contains("INSERT ") || upper.contains("UPDATE ") || upper.contains("DELETE ") || upper.contains("REPLACE ")
    {
        return SqlKind::Dml;
    }
    SqlKind::Select
}

/// Strip leading SQL comments from a string.
///
/// Removes `--` line comments and `/* ... */` block comments that appear
/// before the first actual SQL keyword. Handles multiple consecutive comments.
fn strip_leading_comments(sql: &str) -> &str {
    let mut s = sql.trim_start();
    loop {
        if s.starts_with("--") {
            // Skip to end of line
            s = s.find('\n').map_or("", |pos| s.get(pos.saturating_add(1)..).unwrap_or(""));
            s = s.trim_start();
        } else if s.starts_with("/*") {
            // Skip to closing */
            s = s.get(2..).unwrap_or("").find("*/").map_or("", |pos| s.get(pos.saturating_add(4)..).unwrap_or(""));
            s = s.trim_start();
        } else {
            break;
        }
    }
    s
}

// =============================================================================
// Statement splitting
// =============================================================================

/// Split SQL on `;` while respecting single-quoted string literals.
///
/// Handles `''` escape sequences inside strings.
pub(crate) fn split_statements(sql: &str) -> Vec<&str> {
    let mut results = Vec::new();
    let mut start = 0;
    let mut in_string = false;
    let chars: Vec<char> = sql.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars.get(i).copied().unwrap_or_default();

        if in_string {
            if ch == '\'' {
                // Check for escaped quote ('')
                if chars.get(i.saturating_add(1)).copied() == Some('\'') {
                    i = i.saturating_add(2);
                    continue;
                }
                in_string = false;
            }
        } else if ch == '\'' {
            in_string = true;
        } else if ch == ';' {
            let stmt = sql.get(start..i).unwrap_or_default().trim();
            if !stmt.is_empty() {
                results.push(stmt);
            }
            start = i.saturating_add(1);
        }

        i = i.saturating_add(1);
    }

    // Last statement (no trailing semicolon)
    let tail = sql.get(start..).unwrap_or_default().trim();
    if !tail.is_empty() {
        results.push(tail);
    }

    results
}

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

    let sql = tool.input.get("sql").and_then(serde_json::Value::as_str).unwrap_or_default();

    if sql.trim().is_empty() {
        return err(tool, "SQL parameter is empty.");
    }

    let es = crate::types::EntitiesState::get(state);
    let db_path = es.db_path.clone();
    let dump_path = es.dump_path.clone();
    let migrations_dir = es.migrations_dir.clone();

    let conn = match db::open(&db_path) {
        Ok(c) => c,
        Err(e) => return err(tool, &e),
    };

    let kind = classify(sql);

    let result_content = match kind {
        SqlKind::Select => execute_select(&conn, sql, state),
        SqlKind::Dml => execute_dml(&conn, sql),
        SqlKind::Ddl => execute_ddl(&conn, sql, &dump_path, &migrations_dir),
    };

    // Handle errors
    let (content, is_error) = match result_content {
        Ok(text) => (text, false),
        Err(e) => {
            let schema = db::introspect(&conn, &db_path);
            (enrich_error(&e, &schema), true)
        }
    };

    // Post-execution: sync to Meilisearch on writes
    if !is_error && kind != SqlKind::Select {
        let stmts = split_statements(sql);
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

    // Post-execution: refresh schema cache + touch panel
    let fresh_cache = db::introspect(&conn, &db_path);
    let es_mut = crate::types::EntitiesState::get_mut(state);
    es_mut.schema_cache = Some(fresh_cache);
    state.touch_panel(Kind::ENTITIES);

    ToolResult {
        tool_use_id: tool.id.clone(),
        content,
        display: None,
        tldr: None,
        is_error,
        preserves_tempo: false,
        tool_name: tool.name.clone(),
    }
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
    let upper = sql.to_uppercase();
    let has_returning = upper.contains("RETURNING");

    // Wrap multi-statement batches in an implicit transaction for atomicity,
    // unless the user already starts with BEGIN (explicit transaction control).
    let needs_implicit_tx =
        stmts.len() > 1 && !stmts.first().is_some_and(|s| s.trim().to_uppercase().starts_with("BEGIN"));

    if needs_implicit_tx {
        conn.execute_batch("BEGIN").map_err(|e| format!("{e}"))?;
    }

    let result = execute_dml_stmts(conn, &stmts, has_returning);

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
fn execute_dml_stmts(conn: &Connection, stmts: &[&str], has_returning: bool) -> Result<String, String> {
    let mut total_affected = 0usize;
    for (i, stmt) in stmts.iter().enumerate() {
        let is_last = i == stmts.len().saturating_sub(1);

        if is_last && has_returning {
            // Last statement with RETURNING — format as table
            let mut prep = conn.prepare(stmt).map_err(|e| format!("{e}"))?;
            let col_names: Vec<String> = prep.column_names().iter().map(|s| (*s).to_string()).collect();
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
            let table = format_markdown_table(&col_names, &rows_data);
            if total_affected > 0 {
                return Ok(format!("{total_affected} row(s) affected.\n\n{table}\n\n({count} returned)"));
            }
            return Ok(format!("{table}\n\n({count} returned)"));
        }

        let affected = conn.execute(stmt, []).map_err(|e| format!("{e}"))?;
        total_affected = total_affected.saturating_add(affected);
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

// =============================================================================
// Query formatting
// =============================================================================

/// Execute a query and format results as a markdown table.
fn query_to_markdown(conn: &Connection, sql: &str, state: &State) -> Result<String, String> {
    let mut stmt = conn.prepare(sql).map_err(|e| format!("{e}"))?;
    let col_names: Vec<String> = stmt.column_names().iter().map(|s| (*s).to_string()).collect();
    let mut rows_data: Vec<Vec<String>> = Vec::new();

    let mut rows = stmt.query([]).map_err(|e| format!("{e}"))?;
    while let Some(row) = rows.next().map_err(|e| format!("{e}"))? {
        let mut vals = Vec::with_capacity(col_names.len());
        for idx in 0..col_names.len() {
            vals.push(format_cell(row, idx));
        }
        rows_data.push(vals);
    }

    let count = rows_data.len();

    if count == 0 {
        // Provide context about total rows for filtered queries
        let table_hint = extract_table_name(sql);
        if let Some(tbl) = &table_hint {
            let es = crate::types::EntitiesState::get(state);
            if let Some(cache) = &es.schema_cache
                && let Some(info) = cache.tables.iter().find(|t| t.name.eq_ignore_ascii_case(tbl))
            {
                return Ok(format!("0 rows returned. (Table '{}' has {} total rows.)", info.name, info.row_count));
            }
        }
        return Ok("0 rows returned.".to_string());
    }

    // Cap inline results at 50 rows
    if count > 50 {
        let truncated = rows_data.get(..50).unwrap_or(&rows_data);
        let table = format_markdown_table(&col_names, truncated);
        return Ok(format!("{table}\n\n({count} rows, showing first 50)"));
    }

    let table = format_markdown_table(&col_names, &rows_data);
    Ok(format!("{table}\n\n({count} rows)"))
}

/// Format a single cell value for markdown table display.
fn format_cell(row: &rusqlite::Row<'_>, idx: usize) -> String {
    use rusqlite::types::ValueRef;

    let Ok(val) = row.get_ref(idx) else {
        return "NULL".to_string();
    };

    match val {
        ValueRef::Null => "NULL".to_string(),
        ValueRef::Integer(n) => n.to_string(),
        ValueRef::Real(f) => f.to_string(),
        ValueRef::Text(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        ValueRef::Blob(b) => format!("[BLOB {} bytes]", b.len()),
    }
}

/// Build a markdown table from column names and row data.
fn format_markdown_table(cols: &[String], rows: &[Vec<String>]) -> String {
    if cols.is_empty() {
        return String::new();
    }

    let mut out = String::new();

    // Header
    out.push_str("| ");
    out.push_str(&cols.join(" | "));
    out.push_str(" |\n");

    // Separator
    out.push('|');
    for _ in cols {
        out.push_str("------|");
    }
    out.push('\n');

    // Rows
    for row in rows {
        out.push_str("| ");
        out.push_str(&row.join(" | "));
        out.push_str(" |\n");
    }

    out
}

/// Try to extract the main table name from a SELECT query.
fn extract_table_name(sql: &str) -> Option<String> {
    let upper = sql.to_uppercase();
    let from_pos = upper.find("FROM ")?;
    let after_from = sql.get(from_pos.saturating_add(5)..)?;
    let name: String = after_from.trim().chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    if name.is_empty() { None } else { Some(name) }
}

// =============================================================================
// Helper
// =============================================================================

/// Build an error `ToolResult`.
fn err(tool: &ToolUse, msg: &str) -> ToolResult {
    ToolResult {
        tool_use_id: tool.id.clone(),
        content: msg.to_string(),
        display: None,
        tldr: None,
        is_error: true,
        preserves_tempo: false,
        tool_name: tool.name.clone(),
    }
}

use rusqlite::Connection;
use std::path::Path;
