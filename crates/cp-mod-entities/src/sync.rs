//! Meilisearch sync for entity data — incremental dirty-tracked, delete-then-add.
//!
//! Bridges the `SQLite` entity database to the shared Meilisearch server for
//! fuzzy discovery via the `search` tool. Each row becomes a document with
//! YAML-formatted `_all_text` for rich keyword search.

use cp_base::state::runtime::State;
use cp_mod_search::meili::api::MeiliClient;

use crate::db;
use crate::types::EntitiesState;

/// Maximum character length for a single column value in `_all_text`.
const VALUE_CAP: usize = 500;

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
            "minWordSizeForTypos": { "oneTypo": 4, "twoTypos": 8 }
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
    let filter = format!("entity_table = '{table_name}'");
    let _del_task = client.delete_documents_by_filter(index_uid, &filter)?;

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
    let filter = format!("entity_table = '{table_name}'");
    let _task = client.delete_documents_by_filter(index_uid, &filter)?;
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

    // Sync all current tables (upsert — never leaves index empty)
    let Ok(conn) = db::open(&db_path) else { return };
    let cache = db::introspect(&conn, &db_path);
    let current_tables: Vec<String> = cache.tables.iter().map(|t| t.name.clone()).collect();
    drop(conn);

    for table in &current_tables {
        if let Err(e) = sync_table(&client, &db_path, &index_uid, table) {
            log::warn!("Failed to sync table '{table}' to Meilisearch: {e}");
        }
    }

    // Clean up orphan tables via facet_distribution
    if let Ok(facets) = client.facet_distribution(&index_uid, &["entity_table"])
        && let Some(tables_map) = facets.get("entity_table").and_then(serde_json::Value::as_object)
    {
        for indexed_table in tables_map.keys() {
            if !current_tables.contains(indexed_table) {
                let _r = delete_table_docs(&client, &index_uid, indexed_table);
            }
        }
    }
}

// =============================================================================
// SQL table name extraction
// =============================================================================

/// Extract affected table names from SQL statements.
///
/// Parses `INSERT INTO`, `UPDATE`, `DELETE FROM`, `CREATE TABLE`,
/// `ALTER TABLE`, `DROP TABLE`, and `CREATE INDEX ON`.
pub(crate) fn extract_affected_tables(statements: &[&str]) -> Vec<String> {
    let mut tables = Vec::new();

    for stmt in statements {
        let upper = stmt.trim().to_uppercase();
        let words: Vec<&str> = upper.split_whitespace().collect();

        // INSERT INTO table / INSERT OR ... INTO table
        if let Some(pos) = words.iter().position(|w| *w == "INTO") {
            if let Some(name) = words.get(pos.saturating_add(1)) {
                tables.push(clean_identifier(name));
            }
            continue;
        }

        // UPDATE table / UPDATE OR ... table
        if words.first() == Some(&"UPDATE") {
            let skip = if words.get(1) == Some(&"OR") { 3 } else { 1 };
            if let Some(name) = words.get(skip) {
                tables.push(clean_identifier(name));
            }
            continue;
        }

        // DELETE FROM table
        if words.first() == Some(&"DELETE") {
            if let Some(pos) = words.iter().position(|w| *w == "FROM")
                && let Some(name) = words.get(pos.saturating_add(1))
            {
                tables.push(clean_identifier(name));
            }
            continue;
        }

        // CREATE TABLE / ALTER TABLE / DROP TABLE
        if let Some(pos) = words.iter().position(|w| *w == "TABLE") {
            let next = pos.saturating_add(1);
            // Skip optional IF NOT EXISTS / IF EXISTS
            let name_pos = if words.get(next) == Some(&"IF") {
                // IF NOT EXISTS → skip 3, IF EXISTS → skip 2
                if words.get(next.saturating_add(1)) == Some(&"NOT") {
                    next.saturating_add(3)
                } else {
                    next.saturating_add(2)
                }
            } else {
                next
            };
            if let Some(name) = words.get(name_pos) {
                tables.push(clean_identifier(name));
            }
            continue;
        }

        // CREATE INDEX ... ON table
        if let Some(pos) = words.iter().position(|w| *w == "ON")
            && words.first() == Some(&"CREATE")
            && let Some(name) = words.get(pos.saturating_add(1))
        {
            tables.push(clean_identifier(name));
        }
    }

    tables.sort();
    tables.dedup();
    tables
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

    let mut stmt = conn.prepare(&query).map_err(|e| format!("{e}"))?;
    let mut rows = stmt.query([]).map_err(|e| format!("{e}"))?;
    let mut docs = Vec::new();

    while let Some(row) = rows.next().map_err(|e| format!("{e}"))? {
        // Build row ID
        let row_id = if rowid_offset {
            let rid: i64 = row.get(0).unwrap_or(0);
            format!("{table}__{rid}")
        } else {
            // Use first column (PK) as ID
            let pk_val: String = row.get::<_, String>(1).unwrap_or_default();
            format!("{table}__{pk_val}")
        };

        // Build YAML-formatted _all_text
        let col_offset: usize = usize::from(rowid_offset);
        let mut yaml_parts = Vec::new();
        for (i, col_name) in col_names.iter().enumerate() {
            let idx = i.saturating_add(col_offset);
            let val = format_cell_value(row, idx);
            if !val.is_empty() && val != "NULL" {
                let capped = if val.len() > VALUE_CAP {
                    let truncated = val.get(..VALUE_CAP).unwrap_or(&val);
                    format!("{truncated}…")
                } else {
                    val
                };
                yaml_parts.push(format!("{col_name}: {capped}"));
            }
        }
        let all_text = yaml_parts.join("\n");

        // Sanitize doc ID for Meilisearch: [a-zA-Z0-9_-]
        let safe_id: String =
            row_id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();

        docs.push(serde_json::json!({
            "id": safe_id,
            "entity_table": table,
            "_all_text": all_text,
        }));
    }

    Ok(docs)
}

/// Format a single cell value as a string for `_all_text`.
fn format_cell_value(row: &rusqlite::Row<'_>, idx: usize) -> String {
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
            s.replace('\n', " ").replace('\r', "")
        }
        ValueRef::Blob(b) => format!("[BLOB {} bytes]", b.len()),
    }
}
