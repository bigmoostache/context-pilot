//! SQL execution engine: classification, splitting, execution, error enrichment.

use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};

use crate::errors::enrich_error;
mod exec;
use crate::db;
use crate::parse::{SqlKind, classify, split_statements};
use crate::result_panel::{self, LivePanelMeta};

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

/// Which SQL source the caller supplied (`sql` vs `request_path`).
#[derive(PartialEq, Eq)]
enum SqlSource {
    /// Only `sql` present — inline SQL.
    Inline,
    /// Only `request_path` present — read SQL from a file.
    File,
    /// Both supplied — a conflict.
    Both,
    /// Neither supplied — nothing to run.
    Neither,
}

impl SqlSource {
    /// Classify the source from the two presence flags.
    const fn from_flags(has_sql: bool, has_request: bool) -> Self {
        match (has_sql, has_request) {
            (true, true) => Self::Both,
            (true, false) => Self::Inline,
            (false, true) => Self::File,
            (false, false) => Self::Neither,
        }
    }
}

/// Which mutually-exclusive input flags the caller supplied.
struct ParamFlags {
    /// Resolved SQL source (inline / file / conflict / missing).
    source: SqlSource,
    /// `live=true` — auto-refreshing panel mode.
    live: bool,
    /// `output_path` present — file output mode.
    has_output: bool,
    /// `dry_run=true` — savepoint-rollback mode.
    dry_run: bool,
}

impl ParamFlags {
    /// Reject conflicting flag combinations. Returns the first conflict message.
    const fn validate(&self) -> Result<(), &'static str> {
        match self.source {
            SqlSource::Both => return Err("Cannot provide both `sql` and `request_path`. Use one or the other."),
            SqlSource::Neither => return Err("Must provide either `sql` or `request_path`."),
            SqlSource::Inline | SqlSource::File => {}
        }
        if self.live && self.has_output {
            return Err("`live` and `output_path` are incompatible.");
        }
        if self.live && self.dry_run {
            return Err("`live` and `dry_run` are incompatible.");
        }
        Ok(())
    }
}

/// Bundled context for routing a successful execution result to inline / panel /
/// file output. Groups the parameters so the router stays under the argument cap.
struct RouteCtx<'route> {
    /// The SQL that produced `text` (for panel titles + live re-execution).
    sql: &'route str,
    /// The formatted result text to route.
    text: &'route str,
    /// `live=true` — always create an auto-refreshing panel.
    live: bool,
    /// `output_path` was provided — write to file + static panel.
    has_output: bool,
    /// Destination path for `output_path` mode.
    output_path_str: &'route str,
    /// Database path (for live panel re-execution metadata).
    db_path: &'route Path,
    /// Statement kind (affects the inline post-mutation note).
    kind: SqlKind,
    /// Whether this was a dry run (affects the inline note).
    dry_run: bool,
}

/// Route a live query result: always a panel, storing the SQL for re-execution.
fn route_live(state: &mut State, ctx: &RouteCtx<'_>) -> (String, bool, bool) {
    let sql_preview: String = ctx.sql.chars().take(60).collect();
    let title = format!("entity_sql: {sql_preview}");
    let panel_id = result_panel::create_live_result_panel(
        state,
        &title,
        ctx.text,
        LivePanelMeta { sql: ctx.sql, db_path: &ctx.db_path.to_string_lossy() },
    );
    (format!("Live query panel created: {panel_id}. Auto-refreshes every 2s.{PANEL_WARNING}"), false, false)
}

/// Route an `output_path` result: write to file + static panel. Errors on a
/// write failure.
fn route_output(state: &mut State, ctx: &RouteCtx<'_>) -> Result<(String, bool, bool), String> {
    let out = Path::new(ctx.output_path_str);
    if let Some(parent) = out.parent() {
        let _d = std::fs::create_dir_all(parent);
    }
    std::fs::write(out, ctx.text).map_err(|e| format!("Failed to write to {}: {e}", ctx.output_path_str))?;
    let sql_preview: String = ctx.sql.chars().take(60).collect();
    let title = format!("entity_sql: {sql_preview}");
    let panel_id = result_panel::create_result_panel(state, &title, ctx.text);
    Ok((
        format!("Results written to `{}`. Also available in panel {panel_id}.{PANEL_WARNING}", ctx.output_path_str),
        false,
        false,
    ))
}

/// Route a default result by size: small → inline (keeps tempo), big → panel.
fn route_inline(state: &mut State, ctx: &RouteCtx<'_>) -> (String, bool, bool) {
    let line_count = ctx.text.lines().count();
    if line_count > INLINE_MAX_LINES || ctx.text.len() > INLINE_MAX_BYTES {
        let sql_preview: String = ctx.sql.chars().take(60).collect();
        let title = format!("entity_sql: {sql_preview}");
        let panel_id = result_panel::create_result_panel(state, &title, ctx.text);
        return (format!("{line_count} lines returned — results in panel {panel_id}.{PANEL_WARNING}"), false, false);
    }
    let note = if ctx.kind == SqlKind::Select || ctx.dry_run {
        ""
    } else {
        "\n\n(The Entities panel now reflects the updated database state.)"
    };
    (format!("{}{note}", ctx.text), false, true)
}

/// Dispatch a successful result to the right routing mode (live / file / inline).
fn route_ok(state: &mut State, ctx: &RouteCtx<'_>) -> Result<(String, bool, bool), String> {
    if ctx.live {
        Ok(route_live(state, ctx))
    } else if ctx.has_output {
        route_output(state, ctx)
    } else {
        Ok(route_inline(state, ctx))
    }
}

/// Post-execution classification: what the statement did and how it ran.
struct PostExec {
    /// Statement kind (Select skips sync).
    kind: SqlKind,
    /// Dry run — no persistent effect, skip sync.
    dry_run: bool,
    /// Execution errored — skip sync.
    is_error: bool,
}

/// Meilisearch sync after a successful mutation (skipped on dry-run / read-only).
fn sync_affected(state: &mut State, sql: &str, ctx: &PostExec, stmts: &[&str]) {
    if ctx.is_error || ctx.dry_run || ctx.kind == SqlKind::Select {
        return;
    }
    let affected = crate::sync::extract_affected_tables(stmts);
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

/// Refresh the cached schema after execution (skipped on dry-run).
fn refresh_schema_cache(state: &mut State, conn: &Connection, db_path: &Path, dry_run: bool) {
    if dry_run {
        return;
    }
    let fresh_cache = db::introspect(conn, db_path);
    let es_mut = crate::types::EntitiesState::get_mut(state);
    es_mut.schema_cache = Some(fresh_cache);
    state.touch_panel(Kind::ENTITIES);
}

/// Resolve the SQL text to run: read the file for `request_path` mode, or clone
/// the inline `sql` param. Returns an owned string (errors on a read failure,
/// missing file, or empty/whitespace-only SQL).
fn resolve_sql_text(has_request: bool, request_path: &str, sql_param: &str) -> Result<String, String> {
    let text = if has_request {
        let path = Path::new(request_path);
        if !path.exists() {
            return Err(format!("File not found: {request_path}"));
        }
        std::fs::read_to_string(path).map_err(|e| format!("Failed to read {request_path}: {e}"))?
    } else {
        sql_param.to_owned()
    };
    if text.trim().is_empty() {
        return Err("SQL is empty (file contained no SQL).".to_owned());
    }
    Ok(text)
}

/// Classify the SQL and enforce the live-mode restriction. Returns the kind, or
/// an error message when `live=true` is used with a non-SELECT statement.
fn classify_and_check(stmts: &[&str], sql: &str, live: bool) -> Result<SqlKind, &'static str> {
    let kind = classify(stmts.first().copied().unwrap_or(sql));
    if live && kind != SqlKind::Select {
        return Err("`live=true` is only supported for SELECT/EXPLAIN/PRAGMA queries.");
    }
    Ok(kind)
}

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
    let flags = ParamFlags { source: SqlSource::from_flags(has_sql, has_request), live, has_output, dry_run };
    if let Err(msg) = flags.validate() {
        return err(tool, msg);
    }

    // ── Resolve SQL source ──────────────────────────────────────────────
    let sql = match resolve_sql_text(has_request, request_path, sql_param) {
        Ok(s) => s,
        Err(e) => return err(tool, &e),
    };
    let sql = sql.as_str();

    // Split statements early for classification and empty-input detection.
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

    let kind = match classify_and_check(&stmts, sql, live) {
        Ok(k) => k,
        Err(msg) => return err(tool, msg),
    };

    // ── Execute ─────────────────────────────────────────────────────────
    let result_content = if dry_run {
        exec::execute_dry_run(&conn, sql, kind, state)
    } else {
        match kind {
            SqlKind::Select => exec::execute_select(&conn, sql, state),
            SqlKind::Dml => exec::execute_dml(&conn, sql),
            SqlKind::Ddl => exec::execute_ddl(&conn, sql, &dump_path, &migrations_dir),
        }
    };

    // ── Route results ───────────────────────────────────────────────────
    let (content, is_error, preserves_tempo) = match result_content {
        Ok(text) => {
            let ctx =
                RouteCtx { sql, text: &text, live, has_output, output_path_str, db_path: &db_path, kind, dry_run };
            match route_ok(state, &ctx) {
                Ok(routed) => routed,
                Err(e) => return err(tool, &e),
            }
        }
        Err(e) => {
            let schema = db::introspect(&conn, &db_path);
            (enrich_error(&e, &schema), true, false)
        }
    };

    // ── Post-execution bookkeeping ──────────────────────────────────────
    sync_affected(state, sql, &PostExec { kind, dry_run, is_error }, &stmts);
    refresh_schema_cache(state, &conn, &db_path, dry_run);

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
