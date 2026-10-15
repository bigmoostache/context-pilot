//! Auto-capture DDL as numbered migration files + sequential replay for recovery.
//!
//! Each successful DDL via `entity_sql` is saved as a numbered `.sql` file.
//! On recovery, pending migrations are replayed in order.

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::parse::{SqlKind, classify, split_statements, strip_leading_comments};

// =============================================================================
// Write a new migration
// =============================================================================

/// Save a DDL batch as a numbered migration file and record it in `_meta`.
///
/// Only the schema statements are written (see [`schema_only`]): replaying
/// row writes from a mixed batch would duplicate data the dump already holds.
/// The migration ID is `MAX(migration_id) + 1`; callers hold a write
/// transaction (`BEGIN IMMEDIATE`) so concurrent agents can't pick the same ID.
///
/// Returns the filename and full path (the caller removes the file if its
/// transaction rolls back).
///
/// # Errors
///
/// Returns an error if the file cannot be written or `_meta` cannot be updated.
/// The file is removed again when the `_meta` insert fails.
pub(crate) fn write_migration(
    conn: &Connection,
    migrations_dir: &Path,
    sql: &str,
) -> Result<(String, PathBuf), String> {
    let next_id = last_applied_id(conn).saturating_add(1);
    let timestamp = cp_mod_utilities::time::now_utc_compact();
    let filename = format!("{next_id:04}_{timestamp}.sql");
    let filepath = migrations_dir.join(&filename);

    let _r = std::fs::create_dir_all(migrations_dir);
    std::fs::write(&filepath, schema_only(sql)).map_err(|e| format!("Failed to write migration {filename}: {e}"))?;

    if let Err(e) =
        conn.execute("INSERT INTO _meta (migration_id, filename) VALUES (?1, ?2)", rusqlite::params![next_id, filename])
    {
        let _rm = std::fs::remove_file(&filepath);
        return Err(format!("Failed to record migration in _meta: {e}"));
    }

    Ok((filename, filepath))
}

/// Keep only the schema statements of a batch (`CREATE`/`ALTER`/`DROP`).
///
/// A `CREATE TRIGGER` body contains `;`, so the splitter cuts it into pieces:
/// every piece from the `CREATE TRIGGER` up to the one starting with `END` is
/// kept whole.
pub(crate) fn schema_only(sql: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    let mut in_trigger = false;
    for stmt in split_statements(sql) {
        let upper = strip_leading_comments(stmt).trim_start().to_uppercase();
        let words: Vec<&str> = upper.split_whitespace().take(4).collect();
        if in_trigger {
            kept.push(stmt);
            in_trigger = words.first() != Some(&"END");
            continue;
        }
        if classify(stmt) != SqlKind::Ddl {
            continue;
        }
        kept.push(stmt);
        let opens_trigger = words.first() == Some(&"CREATE") && words.contains(&"TRIGGER");
        in_trigger = opens_trigger && !upper.trim_end().ends_with("END");
    }
    kept.iter().flat_map(|s| [*s, ";\n"]).collect()
}

// =============================================================================
// Query last applied ID
// =============================================================================

/// Get the highest migration ID from `_meta`, or 0 if none applied.
pub(crate) fn last_applied_id(conn: &Connection) -> i64 {
    conn.query_row("SELECT COALESCE(MAX(migration_id), 0) FROM _meta", [], |row| row.get::<_, i64>(0))
        .unwrap_or_default()
}

// =============================================================================
// List migration files on disk
// =============================================================================

/// List migration `.sql` files in the migrations directory, sorted by name.
pub(crate) fn list_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "sql"))
        .collect();

    files.sort();
    files
}

// =============================================================================
// Apply pending migrations
// =============================================================================

/// Apply migration files that haven't been recorded in `_meta` yet.
///
/// Compares the max migration ID in `_meta` against numbered files on disk.
/// Files whose numeric prefix exceeds the last applied ID are executed in order.
///
/// # Errors
///
/// Returns an error if any migration file fails to execute.
pub(crate) fn apply_pending(conn: &Connection, dir: &Path) -> Result<u32, String> {
    let last_id = last_applied_id(conn);
    let files = list_files(dir);
    let mut applied = 0u32;

    for filepath in &files {
        let Some(file_id) = extract_migration_id(filepath) else {
            continue;
        };

        if file_id <= last_id {
            continue;
        }

        let sql = std::fs::read_to_string(filepath)
            .map_err(|e| format!("Failed to read migration {}: {e}", filepath.display()))?;

        conn.execute_batch(&sql).map_err(|e| format!("Failed to apply migration {}: {e}", filepath.display()))?;

        // Record in _meta
        let filename = filepath.file_name().map_or_else(String::new, |f| f.to_string_lossy().into_owned());

        let _rows = conn
            .execute(
                "INSERT OR IGNORE INTO _meta (migration_id, filename) VALUES (?1, ?2)",
                rusqlite::params![file_id, filename],
            )
            .map_err(|e| format!("Failed to record migration {file_id} in _meta: {e}"))?;

        applied = applied.saturating_add(1);
    }

    Ok(applied)
}

/// Extract the numeric migration ID from a filename like `0001_20260604T153000.sql`.
fn extract_migration_id(path: &Path) -> Option<i64> {
    let stem = path.file_stem()?.to_str()?;
    // Take chars before the first underscore
    let id_part: String = stem.chars().take_while(char::is_ascii_digit).collect();
    id_part.parse::<i64>().ok()
}

// =============================================================================
// Migration count
// =============================================================================

/// Count the number of applied migrations in `_meta`.
pub(crate) fn migration_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM _meta", [], |row| row.get::<_, i64>(0)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{Connection, schema_only, write_migration};

    /// Fresh migrations dir under the OS temp dir.
    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("cp-entities-mig-{tag}-{}", std::process::id()));
        let _rm = std::fs::remove_dir_all(&dir);
        dir
    }

    fn conn() -> Connection {
        let c = Connection::open_in_memory().expect("open");
        c.execute_batch("CREATE TABLE _meta (migration_id INTEGER PRIMARY KEY, filename TEXT NOT NULL);")
            .expect("_meta");
        c
    }

    /// Row writes in a mixed batch are not replayed by the migration.
    #[test]
    fn schema_only_drops_dml() {
        let out = schema_only("CREATE TABLE t (x); INSERT INTO t VALUES (1); ALTER TABLE t ADD y; DELETE FROM t");
        assert_eq!(out, "CREATE TABLE t (x);\nALTER TABLE t ADD y;\n");
    }

    /// A trigger body (which contains `;`) is kept whole.
    #[test]
    fn schema_only_keeps_trigger_body() {
        let sql = "CREATE TRIGGER tr AFTER INSERT ON t BEGIN INSERT INTO l VALUES (1); UPDATE l SET a = 2; END; \
                   INSERT INTO t VALUES (3)";
        let out = schema_only(sql);
        assert!(out.contains("INSERT INTO l VALUES (1);") && out.contains("UPDATE l SET a = 2;"), "{out}");
        assert!(out.contains("END;") && !out.contains("VALUES (3)"), "{out}");
    }

    /// Sequential migrations get increasing ids and one file each.
    #[test]
    fn ids_increase_and_files_written() {
        let dir = temp_dir("ids");
        let db = conn();
        let (first, _) = write_migration(&db, &dir, "CREATE TABLE a (x)").expect("first");
        let (second, _) = write_migration(&db, &dir, "CREATE TABLE b (x)").expect("second");
        assert!(first.starts_with("0001_") && second.starts_with("0002_"), "{first} {second}");
        assert_eq!(super::list_files(&dir).len(), 2);
        let _rm = std::fs::remove_dir_all(&dir);
    }

    /// A failed `_meta` insert leaves no orphan file behind.
    #[test]
    fn failed_meta_insert_removes_file() {
        let dir = temp_dir("fail");
        let c = Connection::open_in_memory().expect("open");
        let _e = write_migration(&c, &dir, "CREATE TABLE a (x)").expect_err("no _meta table");
        assert!(super::list_files(&dir).is_empty());
        let _rm = std::fs::remove_dir_all(&dir);
    }
}
