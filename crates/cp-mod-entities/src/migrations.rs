//! Auto-capture DDL as numbered migration files + sequential replay for recovery.
//!
//! Each successful DDL via `entity_sql` is saved as a numbered `.sql` file.
//! On recovery, pending migrations are replayed in order.

use std::path::{Path, PathBuf};

use rusqlite::Connection;

// =============================================================================
// Write a new migration
// =============================================================================

/// Save a DDL statement as a numbered migration file and record it in `_meta`.
///
/// The migration ID is derived from `MAX(migration_id) + 1` in `_meta`.
/// The filename includes a timestamp for human readability.
///
/// # Errors
///
/// Returns an error if the file cannot be written or `_meta` cannot be updated.
pub(crate) fn write_migration(conn: &Connection, migrations_dir: &Path, sql: &str) -> Result<String, String> {
    let next_id = last_applied_id(conn).saturating_add(1);
    let timestamp = cp_mod_utilities::time::now_utc_compact();
    let filename = format!("{next_id:04}_{timestamp}.sql");
    let filepath = migrations_dir.join(&filename);

    // Ensure directory exists
    let _r = std::fs::create_dir_all(migrations_dir);

    // Write the migration file
    std::fs::write(&filepath, sql).map_err(|e| format!("Failed to write migration {filename}: {e}"))?;

    // Record in _meta
    let _rows = conn
        .execute("INSERT INTO _meta (migration_id, filename) VALUES (?1, ?2)", rusqlite::params![next_id, filename])
        .map_err(|e| format!("Failed to record migration in _meta: {e}"))?;

    Ok(filename)
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
