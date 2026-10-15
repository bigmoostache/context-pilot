//! Meilisearch sync for entity data — incremental dirty-tracked, delete-then-add.
//!
//! Bridges the `SQLite` entity database to the shared Meilisearch server for
//! fuzzy discovery via the `search` tool. Each row becomes a document with
//! YAML-formatted `_all_text` for rich keyword search.

use cp_base::state::runtime::State;
use cp_mod_search::meili::api::MeiliClient;

use crate::db;
use crate::types::EntitiesState;

/// Write-effect detection (dirty / dropped tables) from `sqlite_master`.
pub(crate) mod affected;
#[cfg(test)]
mod tests;

/// Maximum character length for a single column value in `_all_text`.
const VALUE_CAP: usize = 500;

/// Meilisearch filter selecting one table's docs. Quotes and backslashes in
/// the name are escaped so a table name can't break out of the string.
fn table_filter(table: &str) -> String {
    let escaped = table.replace('\\', "\\\\").replace('\'', "\\'");
    format!("entity_table = '{escaped}'")
}

/// Truncate to at most [`VALUE_CAP`] characters (never splits a UTF-8 char).
fn cap_value(val: String) -> String {
    match val.char_indices().nth(VALUE_CAP) {
        Some((cut, _)) => format!("{}…", val.get(..cut).unwrap_or_default()),
        None => val,
    }
}

// =============================================================================
// Index management
// =============================================================================

/// Ensure the entities Meilisearch index exists with correct settings.
///
/// Idempotent — skips creation if the index already exists.
///
/// # Errors
///
/// Returns an error if the Meilisearch API calls fail.
pub(crate) fn ensure_index(port: u16, key: &str, index_uid: &str) -> Result<(), String> {
    let client = MeiliClient::new(port, key)?;

    if !client.index_exists(index_uid).unwrap_or(false) {
        let _task = client.create_index(index_uid, "id")?;
    }

    let settings = serde_json::json!({
        "searchableAttributes": ["_all_text"],
        "filterableAttributes": ["entity_table"],
        "sortableAttributes": [],
        "typoTolerance": {
            "enabled": true,
            "minWordSizeForTypos": { "oneTypo": 4i32, "twoTypos": 8i32 }
        }
    });
    let _task = client.update_settings(index_uid, &settings)?;

    Ok(())
}

// =============================================================================
// Per-table sync
// =============================================================================

/// Sync a single table to Meilisearch: delete old docs, then add current rows.
///
/// Uses `entity_table` filter to scope deletions. Row IDs use `rowid` with
/// a fallback to the declared primary key for `WITHOUT ROWID` tables.
///
/// # Errors
///
/// Returns an error if any Meilisearch or `SQLite` operation fails.
pub(crate) fn sync_table(
    client: &MeiliClient,
    db_path: &std::path::Path,
    index_uid: &str,
    table_name: &str,
) -> Result<(), String> {
    // Delete existing docs for this table
    let _del_task = client.delete_documents_by_filter(index_uid, &table_filter(table_name))?;

    // Open connection and read all rows
    let conn = db::open(db_path)?;
    let docs = build_docs(&conn, table_name)?;

    if docs.is_empty() {
        return Ok(());
    }

    let _add_task = client.add_documents(index_uid, &serde_json::Value::Array(docs))?;
    Ok(())
}

/// Delete all Meilisearch docs for a dropped table.
///
/// # Errors
///
/// Returns an error if the Meilisearch API call fails.
pub(crate) fn delete_table_docs(client: &MeiliClient, index_uid: &str, table_name: &str) -> Result<(), String> {
    let _task = client.delete_documents_by_filter(index_uid, &table_filter(table_name))?;
    Ok(())
}

// =============================================================================
// Flush + full reindex
// =============================================================================

/// Flush pending sync operations: process drops first, then dirty tables.
///
/// Clears entries on success, keeps them on failure for retry.
pub(crate) fn flush_sync(state: &mut State) {
    let Some((port, key)) = cp_mod_search::meili_credentials(state) else {
        return;
    };

    let es = EntitiesState::get(state);
    if es.dirty_tables.is_empty() && es.dropped_tables.is_empty() {
        return;
    }
    let index_uid = es.entities_index_uid.clone();
    if index_uid.is_empty() {
        return;
    }
    // Guard: don't open (and auto-create) the DB if it doesn't exist
    if !es.db_path.exists() {
        return;
    }

    let Ok(client) = MeiliClient::new(port, &key) else {
        return;
    };

    // Process drops first
    let dropped: Vec<String> = EntitiesState::get(state).dropped_tables.clone();
    let mut drop_ok = Vec::new();
    for table in &dropped {
        if delete_table_docs(&client, &index_uid, table).is_ok() {
            drop_ok.push(table.clone());
        }
    }
    let es_drop = EntitiesState::get_mut(state);
    es_drop.dropped_tables.retain(|t| !drop_ok.contains(t));

    // Process dirty tables
    let dirty: Vec<String> = EntitiesState::get(state).dirty_tables.iter().cloned().collect();
    let db_path = EntitiesState::get(state).db_path.clone();
    let mut sync_ok = Vec::new();
    for table in &dirty {
        if sync_table(&client, &db_path, &index_uid, table).is_ok() {
            sync_ok.push(table.clone());
        }
    }
    let es_sync = EntitiesState::get_mut(state);
    for table in &sync_ok {
        let _removed = es_sync.dirty_tables.remove(table);
    }
}

/// Sync every current table to Meilisearch (upsert — never leaves index empty).
fn sync_current_tables(client: &MeiliClient, db_path: &std::path::Path, index_uid: &str, current_tables: &[String]) {
    for table in current_tables {
        if let Err(e) = sync_table(client, db_path, index_uid, table) {
            log::warn!("Failed to sync table '{table}' to Meilisearch: {e}");
        }
    }
}

/// Delete Meilisearch docs for any indexed table no longer present in the DB.
fn cleanup_orphan_tables(client: &MeiliClient, index_uid: &str, current_tables: &[String]) {
    if let Ok(facets) = client.facet_distribution(index_uid, &["entity_table"])
        && let Some(tables_map) = facets.get("entity_table").and_then(serde_json::Value::as_object)
    {
        for indexed_table in tables_map.keys() {
            // Case-insensitive: Meili string filters ignore case, so deleting a
            // legacy lowercased `runs` would also wipe the live `Runs` docs.
            if !current_tables.iter().any(|t| t.eq_ignore_ascii_case(indexed_table)) {
                let _r = delete_table_docs(client, index_uid, indexed_table);
            }
        }
    }
}

/// Full reindex: sync all current tables + clean up orphans.
///
/// Called on init/reload for cold-start catch-up.
pub(crate) fn full_reindex(state: &State) {
    let Some((port, key)) = cp_mod_search::meili_credentials(state) else {
        return;
    };

    let es = EntitiesState::get(state);
    let index_uid = es.entities_index_uid.clone();
    if index_uid.is_empty() {
        return;
    }
    let db_path = es.db_path.clone();
    // Guard: don't open (and auto-create) the DB if it doesn't exist
    if !db_path.exists() {
        return;
    }

    // Ensure index exists
    if let Err(e) = ensure_index(port, &key, &index_uid) {
        log::warn!("Failed to ensure entities index: {e}");
        return;
    }

    let Ok(client) = MeiliClient::new(port, &key) else {
        return;
    };

    let Ok(conn) = db::open(&db_path) else { return };
    let cache = db::introspect(&conn, &db_path);
    let current_tables: Vec<String> = cache.tables.iter().map(|t| t.name.clone()).collect();
    drop(conn);

    sync_current_tables(&client, &db_path, &index_uid, &current_tables);
    cleanup_orphan_tables(&client, &index_uid, &current_tables);
}

// =============================================================================
// SQL table name extraction
// =============================================================================

/// Table touched by a DML statement (`INSERT INTO` / `UPDATE` / `DELETE FROM`).
fn dml_table(words: &[&str]) -> Option<String> {
    // INSERT INTO table / INSERT OR ... INTO table
    if let Some(pos) = words.iter().position(|w| *w == "INTO") {
        return words.get(pos.saturating_add(1)).map(|n| clean_identifier(n));
    }
    // UPDATE table / UPDATE OR ... table
    if words.first() == Some(&"UPDATE") {
        let skip = if words.get(1) == Some(&"OR") { 3 } else { 1 };
        return words.get(skip).map(|n| clean_identifier(n));
    }
    // DELETE FROM table
    if words.first() == Some(&"DELETE")
        && let Some(pos) = words.iter().position(|w| *w == "FROM")
    {
        return words.get(pos.saturating_add(1)).map(|n| clean_identifier(n));
    }
    None
}

/// Table touched by a DDL statement (`CREATE/ALTER/DROP TABLE`, `CREATE INDEX … ON`).
fn ddl_table(words: &[&str]) -> Option<String> {
    // CREATE TABLE / ALTER TABLE / DROP TABLE
    if let Some(pos) = words.iter().position(|w| *w == "TABLE") {
        let next = pos.saturating_add(1);
        // Skip optional IF NOT EXISTS / IF EXISTS
        let name_pos = if words.get(next) == Some(&"IF") {
            if words.get(next.saturating_add(1)) == Some(&"NOT") {
                next.saturating_add(3)
            } else {
                next.saturating_add(2)
            }
        } else {
            next
        };
        return words.get(name_pos).map(|n| clean_identifier(n));
    }
    // CREATE INDEX ... ON table
    if let Some(pos) = words.iter().position(|w| *w == "ON")
        && words.first() == Some(&"CREATE")
    {
        return words.get(pos.saturating_add(1)).map(|n| clean_identifier(n));
    }
    None
}

/// Table name named by one SQL statement, if any (DML first, then DDL),
/// lowercased. Callers resolve it to the stored case (see [`affected`]).
fn table_from_statement(stmt: &str) -> Option<String> {
    let upper = crate::parse::strip_leading_comments(stmt).trim().to_uppercase();
    let words: Vec<&str> = upper.split_whitespace().collect();
    dml_table(&words).or_else(|| ddl_table(&words))
}

/// Extract a clean table name from a raw SQL token.
///
/// Handles quoted identifiers (`"table"`, `` `table` ``), bracket notation
/// (`[table]`), and column-list suffixes (`table(col1, col2)`).
fn clean_identifier(raw: &str) -> String {
    // Take only identifier characters (alphanumeric, underscore, quotes, backticks).
    // Stops at '(' which starts a column list — e.g. INSERT INTO table(col1, col2).
    let ident: String = raw
        .chars()
        .take_while(|c| {
            c.is_ascii_alphanumeric() || *c == '_' || *c == '"' || *c == '\'' || *c == '`' || *c == '[' || *c == ']'
        })
        .collect();
    ident.trim_matches(|c: char| c == '"' || c == '\'' || c == '`' || c == '[' || c == ']').to_lowercase()
}

// =============================================================================
// Document building
// =============================================================================

/// Build Meilisearch documents from all rows in a table.
///
/// Each document has `id` (`{table}__{rowid}`), `entity_table`, and
/// YAML-formatted `_all_text`. Values are capped at [`VALUE_CAP`] chars.
///
/// # Errors
///
/// Returns an error if the `SQLite` query fails.
fn build_docs(conn: &rusqlite::Connection, table: &str) -> Result<Vec<serde_json::Value>, String> {
    // Get column names
    let col_query = format!("PRAGMA table_info('{table}')");
    let mut col_stmt = conn.prepare(&col_query).map_err(|e| format!("{e}"))?;
    let col_names: Vec<String> = col_stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| format!("{e}"))?
        .filter_map(Result::ok)
        .collect();
    drop(col_stmt);

    if col_names.is_empty() {
        return Ok(Vec::new());
    }

    // Try rowid-based query first, fall back for WITHOUT ROWID tables
    let row_query = format!("SELECT rowid, * FROM \"{table}\"");
    let has_rowid = conn.prepare(&row_query).is_ok();

    let (query, rowid_offset) =
        if has_rowid { (row_query, true) } else { (format!("SELECT * FROM \"{table}\""), false) };
    // WITHOUT ROWID tables always declare a primary key: build the id from it.
    let pk_idx = if has_rowid { Vec::new() } else { pk_column_indices(conn, table) };
    // Lowercased so ids stay stable across case variants of the same name.
    let id_table = table.to_lowercase();

    let mut stmt = conn.prepare(&query).map_err(|e| format!("{e}"))?;
    let mut rows = stmt.query([]).map_err(|e| format!("{e}"))?;
    let mut docs = Vec::new();

    while let Some(row) = rows.next().map_err(|e| format!("{e}"))? {
        let row_key = if rowid_offset {
            let rid: i64 = row.get(0).unwrap_or(0);
            rid.to_string()
        } else {
            pk_key(&pk_idx.iter().map(|&i| format_cell_value(row, i)).collect::<Vec<_>>())
        };
        let safe_id = format!("{}__{row_key}", sanitize_id(&id_table));

        // Build YAML-formatted _all_text
        let col_offset: usize = usize::from(rowid_offset);
        let mut yaml_parts = Vec::new();
        for (i, col_name) in col_names.iter().enumerate() {
            let idx = i.saturating_add(col_offset);
            let val = format_cell_value(row, idx);
            if !val.is_empty() && val != "NULL" {
                yaml_parts.push(format!("{col_name}: {}", cap_value(val)));
            }
        }
        let all_text = yaml_parts.join("\n");

        docs.push(serde_json::json!({
            "id": safe_id,
            "entity_table": table,
            "_all_text": all_text,
        }));
    }

    Ok(docs)
}

/// Zero-based result indices of the primary-key columns, in key order.
fn pk_column_indices(conn: &rusqlite::Connection, table: &str) -> Vec<usize> {
    let query =
        format!("SELECT cid, pk FROM pragma_table_info('{}') WHERE pk > 0 ORDER BY pk", table.replace('\'', "''"));
    let Ok(mut stmt) = conn.prepare(&query) else { return Vec::new() };
    let Ok(rows) = stmt.query_map([], |r| r.get::<_, i64>(0)) else { return Vec::new() };
    rows.filter_map(Result::ok).filter_map(|cid| usize::try_from(cid).ok()).collect()
}

/// Meilisearch doc ids allow only `[a-zA-Z0-9_-]` (max 511 bytes).
fn sanitize_id(raw: &str) -> String {
    raw.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect()
}

/// Row key from primary-key values. Readable (`k…`) when sanitizing loses
/// nothing; otherwise an FNV-1a hash (`h…`), so distinct keys never collide
/// after sanitizing (`a b` vs `a_b`).
fn pk_key(values: &[String]) -> String {
    let joined = values.join("\u{1f}");
    let readable = values.join("_");
    if values.len() == 1 && sanitize_id(&readable) == readable && readable.len() <= 200 {
        return format!("k{readable}");
    }
    let hash = joined.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    format!("h{hash:016x}")
}

/// Format a single cell value as a string for `_all_text`.
fn format_cell_value(row: &rusqlite::Row<'_>, idx: usize) -> String {
    use rusqlite::types::ValueRef;

    let Ok(val) = row.get_ref(idx) else {
        return "NULL".to_owned();
    };

    match val {
        ValueRef::Null => "NULL".to_owned(),
        ValueRef::Integer(n) => n.to_string(),
        ValueRef::Real(f) => f.to_string(),
        ValueRef::Text(bytes) => {
            let s = String::from_utf8_lossy(bytes);
            s.replace('\n', " ").replace('\r', "")
        }
        ValueRef::Blob(b) => format!("[BLOB {} bytes]", b.len()),
    }
}
