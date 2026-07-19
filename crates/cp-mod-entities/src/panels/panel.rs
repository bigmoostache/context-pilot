//! Fixed Entities panel — live schema, sample data, and empty-state guide.

use cp_base::panels::{ContextItem, Panel};
use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_render::{Block, Semantic, Span};

use std::fmt::Write as _;

use crate::types::EntitiesState;
use crate::{db, migrations};

/// Fixed panel showing entity schema + sample data.
#[derive(Debug)]
pub(crate) struct EntitiesPanel;

impl Panel for EntitiesPanel {
    fn title(&self, _state: &State) -> String {
        "Entities".to_owned()
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let es = EntitiesState::get(state);
        let content = build_context_text(es);
        let entry = state.context.iter().find(|e| e.context_type.as_str() == Kind::ENTITIES);
        let (id, last_refresh_ms) = entry.map_or_else(|| (String::new(), 0), |e| (e.id.clone(), e.last_refresh_ms));
        vec![ContextItem::new(id, "Entities", content, last_refresh_ms)]
    }

    fn blocks(&self, state: &State) -> Vec<Block> {
        let es = EntitiesState::get(state);

        if es.table_count() == 0 {
            return empty_state_blocks();
        }

        populated_blocks(es)
    }

    fn refresh(&self, state: &mut State) {
        // Re-introspect the database and update the cache.
        // Guard: don't open (and auto-create) the DB if it doesn't exist —
        // that would create an empty DB, and a subsequent save would overwrite
        // the good dump file with empty data, destroying the recovery source.
        let db_path = EntitiesState::get(state).db_path.clone();
        if !db_path.exists() {
            return;
        }

        if let Ok(conn) = db::open(&db_path) {
            let fresh = db::introspect(&conn, &db_path);
            EntitiesState::get_mut(state).schema_cache = Some(fresh);
        }

        // Update context entry
        let content = build_context_text(EntitiesState::get(state));
        let tokens = cp_base::state::context::estimate_tokens(&content);

        if let Some(ctx) = state.context.iter_mut().find(|e| e.context_type.as_str() == Kind::ENTITIES) {
            ctx.cached_content = Some(content);
            ctx.token_count = tokens;
            ctx.full_token_count = tokens;
        }
    }

    fn needs_cache(&self) -> bool {
        false
    }

    fn max_freezes(&self) -> u8 {
        2
    }

    fn handle_key(&self, _key: &crossterm::event::KeyEvent, _state: &State) -> Option<cp_base::state::actions::Action> {
        None
    }

    fn refresh_cache(&self, _request: cp_base::panels::CacheRequest) -> Option<cp_base::panels::CacheUpdate> {
        None
    }

    fn build_cache_request(
        &self,
        _ctx: &cp_base::state::context::Entry,
        _state: &State,
    ) -> Option<cp_base::panels::CacheRequest> {
        None
    }

    fn apply_cache_update(
        &self,
        _update: cp_base::panels::CacheUpdate,
        _ctx: &mut cp_base::state::context::Entry,
        _state: &mut State,
    ) -> bool {
        false
    }

    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        // Live panels re-execute SQL every 2s. Static panels return None from
        // build_cache_request (cached_content already set), so no work is done.
        Some(2000)
    }

    fn suicide(&self, _ctx: &cp_base::state::context::Entry, _state: &State) -> bool {
        false
    }
}

// =============================================================================
// Context text (sent to LLM)
// =============================================================================

/// Build the text sent to the LLM as context for the Entities panel.
fn build_context_text(es: &EntitiesState) -> String {
    let Some(cache) = &es.schema_cache else {
        return "Entity Database (empty)\n\nNo entity tables yet. Use entity_sql to create your schema.".to_owned();
    };

    if cache.tables.is_empty() {
        return "Entity Database (empty)\n\nNo entity tables yet. Use entity_sql to create your schema.".to_owned();
    }

    let total_rows: u64 = cache.tables.iter().map(|t| t.row_count).sum();
    let kb = cache.db_size_bytes.wrapping_div(1024);

    let mut out = format!("Entity Database ({} tables, {} rows, {} KB):\n\n", cache.tables.len(), total_rows, kb);

    // Open connection for sample data
    let conn = db::open(&es.db_path).ok();

    for table in &cache.tables {
        // Table header: name (row_count):
        let _header = writeln!(out, "{} ({} rows):", table.name, table.row_count);

        // Columns
        let col_desc: Vec<String> = table
            .columns
            .iter()
            .map(|c| {
                let mut s = format!("{} {}", c.name, c.col_type);
                if c.is_pk {
                    s.push_str(" PK");
                }
                s
            })
            .collect();
        let _cols = writeln!(out, "  {}", col_desc.join(", "));

        // Foreign keys
        for fk in &table.foreign_keys {
            let _fk = writeln!(out, "  FK: {} → {}({})", fk.from_col, fk.to_table, fk.to_col);
        }

        // Sample data (3 rows, 50 char truncation, skip >10 columns)
        if let Some(c) = &(conn) {
            let samples = db::sample_rows(c, &table.name, 3);
            if !samples.is_empty() {
                for row in &samples {
                    let _sample = writeln!(out, "  Sample: ({})", row.join(", "));
                }
            } else if table.row_count > 0 && table.columns.len() > 10 {
                out.push_str("  (wide table, sample omitted)\n");
            } else {
                out.push_str("  (empty)\n");
            }
        }

        out.push('\n');
    }

    out
}

// =============================================================================
// Populated blocks (IR rendering)
// =============================================================================

/// Build the display blocks for a single table (name + columns + foreign keys).
fn table_blocks(table: &crate::types::TableInfo) -> Vec<Block> {
    let mut blocks = vec![Block::Line(vec![
        Span::styled(table.name.clone(), Semantic::Accent).bold(),
        Span::new(format!(" ({} rows)", table.row_count)),
    ])];

    for col in &table.columns {
        let pk_marker = if col.is_pk { " PK" } else { "" };
        let nn_marker = if col.is_not_null { " NOT NULL" } else { "" };
        blocks.push(Block::Line(vec![
            Span::new(format!("  {} ", col.name)),
            Span::styled(format!("{}{pk_marker}{nn_marker}", col.col_type), Semantic::Code),
        ]));
    }

    for fk in &table.foreign_keys {
        blocks.push(Block::Line(vec![Span::styled(
            format!("  FK: {} → {}({})", fk.from_col, fk.to_table, fk.to_col),
            Semantic::Muted,
        )]));
    }

    blocks.push(Block::empty());
    blocks
}

/// Blocks for a populated database.
fn populated_blocks(es: &EntitiesState) -> Vec<Block> {
    let Some(cache) = &es.schema_cache else {
        return vec![Block::text("Entity Database (loading...)".to_owned())];
    };

    let total_rows: u64 = cache.tables.iter().map(|t| t.row_count).sum();
    let kb = cache.db_size_bytes.wrapping_div(1024);

    let mut blocks = vec![
        Block::Line(vec![
            Span::styled(
                format!("Entity Database ({} tables, {} rows, {} KB)", cache.tables.len(), total_rows, kb),
                Semantic::Accent,
            )
            .bold(),
        ]),
        Block::empty(),
    ];

    // Get migration count
    if let Ok(conn) = db::open(&es.db_path) {
        let mig_count = migrations::migration_count(&conn);
        if mig_count > 0 {
            blocks.push(Block::Line(vec![Span::styled(format!("{mig_count} migration(s) tracked"), Semantic::Muted)]));
            blocks.push(Block::empty());
        }
    }

    for table in &cache.tables {
        blocks.extend(table_blocks(table));
    }

    blocks
}

// =============================================================================
// Empty state blocks (onboarding)
// =============================================================================

/// Blocks for the empty-state panel (onboarding guide).
fn empty_state_blocks() -> Vec<Block> {
    vec![
        Block::text("Entity Database (empty)".to_owned()),
        Block::empty(),
        Block::text("No entity tables yet. Use entity_sql to create your schema.".to_owned()),
        Block::empty(),
        Block::Line(vec![Span::new("Quick start:".to_owned()).bold()]),
        Block::Line(vec![Span::styled(
            "  CREATE TABLE companies (id INTEGER PRIMARY KEY, name TEXT NOT NULL, country TEXT);".to_owned(),
            Semantic::Code,
        )]),
        Block::Line(vec![Span::styled(
            "  CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT, role TEXT,".to_owned(),
            Semantic::Code,
        )]),
        Block::Line(vec![Span::styled("    company_id INTEGER REFERENCES companies(id));".to_owned(), Semantic::Code)]),
        Block::Line(vec![Span::styled(
            "  INSERT INTO companies (name, country) VALUES ('Acme', 'France') RETURNING *;".to_owned(),
            Semantic::Code,
        )]),
        Block::empty(),
        Block::Line(vec![Span::new("Tips:".to_owned()).bold()]),
        Block::text("  - INTEGER PRIMARY KEY = auto-increment (don't use AUTOINCREMENT)".to_owned()),
        Block::text("  - FOREIGN KEY constraints model relationships".to_owned()),
        Block::text("  - SQLite types: TEXT, INTEGER, REAL, BLOB (VARCHAR(N) length is ignored)".to_owned()),
        Block::text("  - Use RETURNING * on INSERT/UPDATE to see results immediately".to_owned()),
        Block::text("  - For graph patterns: edges(source_id, target_id, rel_type)".to_owned()),
    ]
}
