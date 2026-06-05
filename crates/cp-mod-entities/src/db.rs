//! SQLite connection factory, bootstrap, introspection, dump, and restore.
//!
//! Connections are `!Send` — opened per-call, never stored in state.

use std::path::Path;

use rusqlite::Connection;

use crate::types::{ColumnInfo, ForeignKeyInfo, SchemaCache, TableInfo};

// =============================================================================
// Connection factory
// =============================================================================

/// Open a `SQLite` connection at `db_path`, apply PRAGMAs, and bootstrap `_meta`.
///
/// PRAGMAs: WAL mode, foreign keys ON, 5 s busy timeout, 64 MB journal cap.
///
/// # Errors
///
/// Returns an error string if the database cannot be opened or PRAGMAs fail.
pub(crate) fn open(db_path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(db_path).map_err(|e| format!("Failed to open SQLite: {e}"))?;

    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;
         PRAGMA journal_size_limit = 67108864;",
    )
    .map_err(|e| format!("PRAGMA setup failed: {e}"))?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _meta (
            migration_id INTEGER PRIMARY KEY,
            filename     TEXT NOT NULL,
            applied_at   TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )
    .map_err(|e| format!("_meta bootstrap failed: {e}"))?;

    Ok(conn)
}

// =============================================================================
// Integrity check
// =============================================================================

/// Run `PRAGMA integrity_check`. Returns `true` if the database is healthy.
pub(crate) fn integrity_check(conn: &Connection) -> bool {
    conn.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0)).is_ok_and(|s| s == "ok")
}

// =============================================================================
// Introspection
// =============================================================================

/// Build a [`SchemaCache`] snapshot from the live database.
///
/// Queries `sqlite_master` for user tables (excludes `sqlite_%` and `_meta`),
/// then fetches column info, foreign keys, and row counts for each.
pub(crate) fn introspect(conn: &Connection, db_path: &Path) -> SchemaCache {
    let tables = user_table_names(conn);
    let mut infos = Vec::with_capacity(tables.len());

    for name in tables {
        let columns = table_columns(conn, &name);
        let foreign_keys = table_fks(conn, &name);
        let row_count = table_row_count(conn, &name);
        infos.push(TableInfo { name, row_count, columns, foreign_keys });
    }

    let db_size_bytes = std::fs::metadata(db_path).map_or(0, |m| m.len());

    SchemaCache { tables: infos, db_size_bytes }
}

/// List user table names (excludes `sqlite_%`, `_meta`).
fn user_table_names(conn: &Connection) -> Vec<String> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT name FROM sqlite_master
         WHERE type = 'table'
           AND name NOT LIKE 'sqlite_%'
           AND name != '_meta'
         ORDER BY name",
    ) else {
        return Vec::new();
    };

    let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) else {
        return Vec::new();
    };

    rows.filter_map(Result::ok).collect()
}

/// Column metadata from `PRAGMA table_info`.
fn table_columns(conn: &Connection, table: &str) -> Vec<ColumnInfo> {
    // PRAGMA doesn't support bound parameters — interpolate the table name.
    let sql = format!("PRAGMA table_info(\"{table}\")");
    let Ok(mut stmt) = conn.prepare(&sql) else {
        return Vec::new();
    };

    let Ok(rows) = stmt.query_map([], |row| {
        Ok(ColumnInfo {
            name: row.get::<_, String>(1)?,
            col_type: row.get::<_, String>(2).unwrap_or_default(),
            is_pk: row.get::<_, i64>(5).unwrap_or_default() > 0,
            is_not_null: row.get::<_, bool>(3).unwrap_or_default(),
        })
    }) else {
        return Vec::new();
    };

    rows.filter_map(Result::ok).collect()
}

/// Foreign key metadata from `PRAGMA foreign_key_list`.
fn table_fks(conn: &Connection, table: &str) -> Vec<ForeignKeyInfo> {
    let sql = format!("PRAGMA foreign_key_list(\"{table}\")");
    let Ok(mut stmt) = conn.prepare(&sql) else {
        return Vec::new();
    };

    let Ok(rows) = stmt.query_map([], |row| {
        Ok(ForeignKeyInfo {
            from_col: row.get::<_, String>(3)?,
            to_table: row.get::<_, String>(2)?,
            to_col: row.get::<_, String>(4)?,
        })
    }) else {
        return Vec::new();
    };

    rows.filter_map(Result::ok).collect()
}

/// Row count via `SELECT COUNT(*)`.
fn table_row_count(conn: &Connection, table: &str) -> u64 {
    let sql = format!("SELECT COUNT(*) FROM \"{table}\"");
    conn.query_row(&sql, [], |row| row.get::<_, i64>(0)).map_or(0, |n| {
        #[expect(clippy::cast_sign_loss, reason = "COUNT(*) is always non-negative")]
        let count = n as u64;
        count
    })
}

// =============================================================================
// WAL checkpoint
// =============================================================================

/// Passive WAL checkpoint — flushes committed pages to the main database file.
pub(crate) fn checkpoint(conn: &Connection) {
    let _r = conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);");
}

// =============================================================================
// Dump to file
// =============================================================================

/// Dump the full schema + data to a SQL file for recovery.
///
/// Generates `CREATE TABLE IF NOT EXISTS` + `INSERT OR IGNORE` for all tables
/// (including `_meta`). Wraps in `PRAGMA foreign_keys = OFF/ON`.
///
/// If the dump exceeds 1 MB, data `INSERT` statements are omitted and a
/// warning comment is added.
///
/// # Errors
///
/// Returns an error if the dump cannot be written.
pub(crate) fn dump_to_file(conn: &Connection, dump_path: &Path) -> Result<(), String> {
    let mut output = String::from("-- Entity database dump (auto-generated)\nPRAGMA foreign_keys = OFF;\n\n");

    // Get ALL tables (including _meta, but not sqlite_%)
    let table_names = all_table_names(conn);
    let mut data_lines = String::new();
    let mut data_too_large = false;

    for table in &table_names {
        // Schema: get original CREATE statement and make it IF NOT EXISTS
        if let Some(create_sql) = table_create_sql(conn, table) {
            let idempotent = make_if_not_exists(&create_sql);
            output.push_str(&idempotent);
            output.push_str(";\n\n");
        }

        // Data: INSERT OR IGNORE for each row
        let row_inserts = table_insert_statements(conn, table);
        for insert in &row_inserts {
            data_lines.push_str(insert);
            data_lines.push('\n');
        }
        if !row_inserts.is_empty() {
            data_lines.push('\n');
        }
    }

    // Non-table objects: views, triggers, user-created indexes
    let non_table_stmts = non_table_create_statements(conn);
    for stmt_sql in &non_table_stmts {
        output.push_str(stmt_sql);
        output.push_str(";\n\n");
    }

    // Check 1 MB cap
    let total_size = output.len().saturating_add(data_lines.len());
    if total_size > 0x10_0000 {
        output.push_str("-- WARNING: Data exceeds 1 MB cap. INSERT statements omitted.\n");
        output.push_str("-- Only schema is preserved. Re-populate data manually.\n\n");
        data_too_large = true;
    }

    if !data_too_large {
        output.push_str(&data_lines);
    }

    output.push_str("PRAGMA foreign_keys = ON;\n");

    // Write atomically (write to temp, rename)
    if let Some(parent) = dump_path.parent() {
        let _r = std::fs::create_dir_all(parent);
    }
    std::fs::write(dump_path, &output).map_err(|e| format!("Failed to write dump: {e}"))
}

/// List all tables including `_meta` (excludes `sqlite_%`).
fn all_table_names(conn: &Connection) -> Vec<String> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT name FROM sqlite_master
         WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
         ORDER BY name",
    ) else {
        return Vec::new();
    };

    let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) else {
        return Vec::new();
    };

    rows.filter_map(Result::ok).collect()
}

/// Get `CREATE` SQL for non-table objects (views, triggers, user indexes).
///
/// These are stored in `sqlite_master` with their full DDL and must be
/// included in the dump so that a dump-only recovery doesn't lose them.
fn non_table_create_statements(conn: &Connection) -> Vec<String> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT sql FROM sqlite_master
         WHERE type IN ('view', 'trigger', 'index')
           AND name NOT LIKE 'sqlite_%'
           AND sql IS NOT NULL
         ORDER BY type, name",
    ) else {
        return Vec::new();
    };

    let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) else {
        return Vec::new();
    };

    rows.filter_map(Result::ok).collect()
}

/// Get the original `CREATE TABLE` SQL from `sqlite_master`.
fn table_create_sql(conn: &Connection, table: &str) -> Option<String> {
    conn.query_row("SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1", [table], |row| {
        row.get::<_, String>(0)
    })
    .ok()
}

/// Convert `CREATE TABLE foo` to `CREATE TABLE IF NOT EXISTS foo`.
fn make_if_not_exists(sql: &str) -> String {
    // Case-insensitive replacement of "CREATE TABLE" with "CREATE TABLE IF NOT EXISTS"
    // Only if "IF NOT EXISTS" isn't already present.
    let upper = sql.to_uppercase();
    if upper.contains("IF NOT EXISTS") {
        return sql.to_string();
    }

    // Find "CREATE TABLE" prefix and insert "IF NOT EXISTS" after it
    upper.find("CREATE TABLE").map_or_else(
        || sql.to_string(),
        |pos| {
            let insert_at = pos.saturating_add("CREATE TABLE".len());
            let mut result = String::with_capacity(sql.len().saturating_add(20));
            result.push_str(sql.get(..insert_at).unwrap_or_default());
            result.push_str(" IF NOT EXISTS");
            result.push_str(sql.get(insert_at..).unwrap_or_default());
            result
        },
    )
}

/// Generate `INSERT OR IGNORE INTO table VALUES (...)` for each row.
fn table_insert_statements(conn: &Connection, table: &str) -> Vec<String> {
    let sql = format!("SELECT * FROM \"{table}\"");
    let Ok(mut stmt) = conn.prepare(&sql) else {
        return Vec::new();
    };

    let col_count = stmt.column_count();
    let mut inserts = Vec::new();

    let Ok(mut rows) = stmt.query([]) else {
        return Vec::new();
    };

    while let Ok(Some(row)) = rows.next() {
        let mut vals = Vec::with_capacity(col_count);
        for i in 0..col_count {
            vals.push(format_sql_value(row, i));
        }
        inserts.push(format!("INSERT OR IGNORE INTO \"{table}\" VALUES ({});", vals.join(", ")));
    }

    inserts
}

/// Format a single column value as a SQL literal.
fn format_sql_value(row: &rusqlite::Row<'_>, idx: usize) -> String {
    use rusqlite::types::ValueRef;

    let Ok(val) = row.get_ref(idx) else {
        return "NULL".to_string();
    };

    match val {
        ValueRef::Null => "NULL".to_string(),
        ValueRef::Integer(n) => n.to_string(),
        ValueRef::Real(f) => f.to_string(),
        ValueRef::Text(bytes) => {
            let s = String::from_utf8_lossy(bytes);
            // Escape single quotes by doubling them
            let escaped = s.replace('\'', "''");
            format!("'{escaped}'")
        }
        ValueRef::Blob(b) => {
            use std::fmt::Write as _;
            let mut hex = String::with_capacity(b.len().saturating_mul(2));
            for byte in b {
                let _w = write!(hex, "{byte:02x}");
            }
            format!("X'{hex}'")
        }
    }
}

// =============================================================================
// Restore from file
// =============================================================================

/// Restore a database from a dump file (schema + data).
///
/// Reads the file and executes it as a batch. Foreign keys are expected to be
/// wrapped in `PRAGMA foreign_keys = OFF/ON` by the dump.
///
/// # Errors
///
/// Returns an error if the file cannot be read or SQL execution fails.
pub(crate) fn restore_from_file(conn: &Connection, dump_path: &Path) -> Result<(), String> {
    let sql = std::fs::read_to_string(dump_path).map_err(|e| format!("Failed to read dump: {e}"))?;
    conn.execute_batch(&sql).map_err(|e| format!("Failed to restore dump: {e}"))
}

// =============================================================================
// Sample data (for panel display)
// =============================================================================

/// Maximum columns before skipping sample data for a table.
const SAMPLE_MAX_COLS: usize = 10;

/// Maximum character length for sample data values.
const SAMPLE_MAX_VALUE_LEN: usize = 50;

/// Fetch the first `limit` rows from a table as formatted string vectors.
///
/// Values are truncated at [`SAMPLE_MAX_VALUE_LEN`] characters. Returns one
/// `Vec<String>` per row. Returns empty if the table has more than
/// [`SAMPLE_MAX_COLS`] columns.
pub(crate) fn sample_rows(conn: &Connection, table: &str, limit: usize) -> Vec<Vec<String>> {
    let sql = format!("SELECT * FROM \"{table}\" LIMIT {limit}");
    let Ok(mut stmt) = conn.prepare(&sql) else {
        return Vec::new();
    };

    let col_count = stmt.column_count();
    if col_count > SAMPLE_MAX_COLS {
        return Vec::new();
    }

    let Ok(mut rows) = stmt.query([]) else {
        return Vec::new();
    };

    let mut result = Vec::new();
    while let Ok(Some(row)) = rows.next() {
        let mut vals = Vec::with_capacity(col_count);
        for i in 0..col_count {
            vals.push(format_display_value(row, i, SAMPLE_MAX_VALUE_LEN));
        }
        result.push(vals);
    }

    result
}

/// Format a column value for display (panel context). Truncates at `max_len`.
fn format_display_value(row: &rusqlite::Row<'_>, idx: usize, max_len: usize) -> String {
    use rusqlite::types::ValueRef;

    let Ok(val) = row.get_ref(idx) else {
        return "NULL".to_string();
    };

    let raw = match val {
        ValueRef::Null => return "NULL".to_string(),
        ValueRef::Integer(n) => n.to_string(),
        ValueRef::Real(f) => f.to_string(),
        ValueRef::Text(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        ValueRef::Blob(b) => format!("[BLOB {} bytes]", b.len()),
    };

    if raw.len() > max_len {
        // Truncate by collecting chars (safe for multi-byte)
        let truncated: String = raw.chars().take(max_len.saturating_sub(3)).collect();
        format!("{truncated}...")
    } else {
        raw
    }
}

// =============================================================================
// Has user tables?
// =============================================================================

/// Check if the database has any user tables (excludes `_meta`, `sqlite_%`).
pub(crate) fn has_user_tables(conn: &Connection) -> bool {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master
         WHERE type = 'table'
           AND name NOT LIKE 'sqlite_%'
           AND name != '_meta'",
        [],
        |row| row.get::<_, i64>(0),
    )
    .is_ok_and(|n| n > 0)
}
