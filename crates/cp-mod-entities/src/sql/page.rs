//! Paged, read-only SQL for the web's `openui` `SqlTable` component.
//!
//! Runs on the bridge intake thread (never the agent's main loop), on its own
//! short-lived connection opened `query_only`, so a page fetch can neither
//! write nor contend with the `entity_sql` writer for long. The caller's SQL is
//! wrapped as a subquery: one `COUNT(*)` for the pager total, one
//! `LIMIT/OFFSET` slice for the rows.

use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use rusqlite::Connection;
use rusqlite::types::ValueRef;

use super::parse::{SqlKind, classify, split_statements};

/// How long a page query may run before `SQLite` is told to abort it.
const DEADLINE: Duration = Duration::from_secs(3);

/// Writer-lock wait before a page query gives up.
const BUSY_TIMEOUT: Duration = Duration::from_millis(500);

/// Longest cell text sent to the browser (chars); longer cells are truncated.
const MAX_CELL_CHARS: usize = 500;

/// One page of rows: column names, display cells (`None` = NULL) and the total.
#[derive(Debug, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_structs,
    reason = "plain result tuple consumed by the agent binary, which maps it field-by-field onto the wire SqlPage; no invariants to protect"
)]
pub struct Page {
    /// Column names, in result order.
    pub columns: Vec<String>,
    /// Row cells as display text.
    pub rows: Vec<Vec<Option<String>>>,
    /// Rows the whole query yields.
    pub total: u64,
}

/// Run one page of `sql` against the database at `db_path`.
///
/// # Errors
/// A human-readable reason when the SQL is not a single read-only statement,
/// fails to prepare or run, or exceeds the deadline.
pub fn fetch(db_path: &Path, sql: &str, offset: u64, limit: u32) -> Result<Page, String> {
    let body = single_select(sql)?;
    let conn = open(db_path)?;
    // Watchdog: interrupt the query if it outlives DEADLINE. Dropping `done`
    // (on return) wakes the watchdog early so it exits without interrupting.
    let interrupt = conn.get_interrupt_handle();
    let (done, wait) = mpsc::channel::<()>();
    let _watchdog = thread::spawn(move || {
        if wait.recv_timeout(DEADLINE) == Err(mpsc::RecvTimeoutError::Timeout) {
            interrupt.interrupt();
        }
    });
    let page = run_page(&conn, body, offset, limit);
    drop(done);
    page
}

/// Count and slice `body` on an already-open connection.
fn run_page(conn: &Connection, body: &str, offset: u64, limit: u32) -> Result<Page, String> {
    let total: i64 =
        conn.query_row(&format!("SELECT COUNT(*) FROM ({body})"), [], |row| row.get(0)).map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(&format!("SELECT * FROM ({body}) LIMIT ?1 OFFSET ?2")).map_err(|e| e.to_string())?;
    let columns: Vec<String> = stmt.column_names().into_iter().map(str::to_owned).collect();
    let width = columns.len();
    let offset_param = i64::try_from(offset).unwrap_or(i64::MAX);
    let mut rows_iter = stmt.query((i64::from(limit), offset_param)).map_err(|e| e.to_string())?;
    let mut rows = Vec::new();
    while let Some(row) = rows_iter.next().map_err(|e| e.to_string())? {
        let cells = (0..width).map(|i| row.get_ref(i).ok().and_then(cell_text)).collect();
        rows.push(cells);
    }
    Ok(Page { columns, rows, total: u64::try_from(total).unwrap_or(0) })
}

/// Validate `sql` as exactly one read-only statement; return it without the trailing `;`.
fn single_select(sql: &str) -> Result<&str, String> {
    let statements = split_statements(sql);
    let &[only] = statements.as_slice() else {
        return Err("SqlTable takes exactly one SELECT statement".to_owned());
    };
    let body = only.trim().trim_end_matches(';').trim();
    let upper = body.to_uppercase();
    // PRAGMA/EXPLAIN classify as reads but cannot be wrapped in a subquery.
    if classify(body) != SqlKind::Select || upper.starts_with("PRAGMA") || upper.starts_with("EXPLAIN") {
        return Err("SqlTable only runs read-only SELECT queries".to_owned());
    }
    Ok(body)
}

/// Open a dedicated read-only connection.
fn open(db_path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(db_path).map_err(|e| format!("cannot open entity database: {e}"))?;
    conn.busy_timeout(BUSY_TIMEOUT).map_err(|e| e.to_string())?;
    conn.execute_batch("PRAGMA query_only = ON;").map_err(|e| e.to_string())?;
    Ok(conn)
}

/// Display text for one cell; `None` for SQL `NULL`.
fn cell_text(value: ValueRef<'_>) -> Option<String> {
    let text = match value {
        ValueRef::Null => return None,
        ValueRef::Integer(i) => i.to_string(),
        ValueRef::Real(f) => f.to_string(),
        ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
        ValueRef::Blob(b) => format!("<blob {} bytes>", b.len()),
    };
    Some(if text.chars().count() > MAX_CELL_CHARS {
        let cut: String = text.chars().take(MAX_CELL_CHARS).collect();
        format!("{cut}…")
    } else {
        text
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("e.db");
        let c = Connection::open(&path).expect("open");
        c.execute_batch(
            "CREATE TABLE t(id INTEGER, name TEXT); \
             INSERT INTO t VALUES (1,'a'),(2,'b'),(3,NULL),(4,'d'),(5,'e');",
        )
        .expect("seed");
        (dir, path)
    }

    #[test]
    fn pages_with_total() {
        let (_dir, path) = db();
        let page = fetch(&path, "SELECT * FROM t ORDER BY id;", 2, 2).expect("page");
        assert_eq!(page.columns, vec!["id", "name"]);
        assert_eq!(page.total, 5);
        assert_eq!(page.rows, vec![vec![Some("3".to_owned()), None], vec![Some("4".to_owned()), Some("d".to_owned())]]);
    }

    #[test]
    fn rejects_writes() {
        let (_dir, path) = db();
        let reason = fetch(&path, "DELETE FROM t", 0, 10).unwrap_err();
        assert!(reason.contains("read-only"), "{reason}");
    }

    #[test]
    fn rejects_multiple_statements() {
        let (_dir, path) = db();
        let reason = fetch(&path, "SELECT 1; SELECT 2", 0, 10).unwrap_err();
        assert!(reason.contains("exactly one"), "{reason}");
    }

    #[test]
    fn rejects_pragma() {
        let (_dir, path) = db();
        let reason = fetch(&path, "PRAGMA table_info(t)", 0, 10).unwrap_err();
        assert!(reason.contains("read-only"), "{reason}");
    }

    #[test]
    fn aggregate_query_counts_one_row() {
        let (_dir, path) = db();
        assert_eq!(fetch(&path, "SELECT COUNT(*) AS n FROM t", 0, 10).expect("ok").total, 1);
    }
}
