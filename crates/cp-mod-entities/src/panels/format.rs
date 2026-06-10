//! SQL result formatting utilities.
//!
//! Shared by `tools.rs` (inline results) and `panel.rs` (live panel refresh).

use rusqlite::Connection;

/// Maximum rows rendered in a single result (inline or panel).
pub(crate) const MAX_RESULT_ROWS: usize = 200;

/// Execute a query and format results as a markdown table.
///
/// The optional `enrichment` tuple `(table_name, row_count)` is used to
/// provide context when a filtered query returns 0 rows.
pub(crate) fn query_to_markdown(
    conn: &Connection,
    sql: &str,
    enrichment: Option<(&str, u64)>,
) -> Result<String, String> {
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
        if let Some((name, total)) = enrichment {
            return Ok(format!("0 rows returned. (Table '{name}' has {total} total rows.)"));
        }
        return Ok("0 rows returned.".to_string());
    }

    // Cap results at MAX_RESULT_ROWS
    if count > MAX_RESULT_ROWS {
        let truncated = rows_data.get(..MAX_RESULT_ROWS).unwrap_or(&rows_data);
        let table = format_markdown_table(&col_names, truncated);
        return Ok(format!("{table}\n\n({count} rows, showing first {MAX_RESULT_ROWS})"));
    }

    let table = format_markdown_table(&col_names, &rows_data);
    Ok(format!("{table}\n\n({count} rows)"))
}

/// Format a single cell value for markdown table display.
pub(crate) fn format_cell(row: &rusqlite::Row<'_>, idx: usize) -> String {
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
pub(crate) fn format_markdown_table(cols: &[String], rows: &[Vec<String>]) -> String {
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

/// Try to extract the main table name from a `SELECT` query.
pub(crate) fn extract_table_name(sql: &str) -> Option<String> {
    let upper = sql.to_uppercase();
    let from_pos = upper.find("FROM ")?;
    let after_from = sql.get(from_pos.saturating_add(5)..)?;
    let name: String = after_from.trim().chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    if name.is_empty() { None } else { Some(name) }
}
