//! Entities module — persistent relational database for structured domain knowledge.
//!
//! One tool: `entity_sql` for arbitrary SQL against an embedded `SQLite` database.
//! The AI owns the schema — nothing is hard-coded. Automatic Meilisearch sync
//! for fuzzy discovery. Fixed panel with live schema + sample data.

/// SQLite connection factory, bootstrap, introspection, dump, and restore.
mod db;
/// SQL error enrichment: fuzzy suggestions and schema context.
mod errors;
/// Auto-capture DDL as numbered migration files + sequential replay for recovery.
mod migrations;
/// Fixed Entities panel — live schema, sample data, and empty-state guide.
mod panel;
/// Meilisearch sync: incremental dirty-tracked, delete-then-add per table.
mod sync;
/// SQL execution engine: classification, splitting, execution, error enrichment.
mod tools;
/// State types: `EntitiesState`, `SchemaCache`, `TableInfo`, `ColumnInfo`, `ForeignKeyInfo`.
pub mod types;

use types::EntitiesState;

use cp_base::modules::Module;
use cp_base::panels::Panel;
use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::pre_flight::Verdict;
use cp_base::tools::{ParamType, ToolDefinition, ToolTexts};
use cp_base::tools::{ToolResult, ToolUse};

/// Lazily parsed tool descriptions from the entities YAML definition.
static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> =
    std::sync::LazyLock::new(|| ToolTexts::parse(include_str!("../../../yamls/tools/entities.yaml")));

/// Entities module: persistent relational entity database.
#[derive(Debug, Clone, Copy)]
pub struct EntitiesModule;

impl Module for EntitiesModule {
    fn id(&self) -> &'static str {
        "entities"
    }
    fn name(&self) -> &'static str {
        "Entities"
    }
    fn description(&self) -> &'static str {
        "Persistent relational entity database"
    }
    fn is_global(&self) -> bool {
        true
    }

    fn init_state(&self, state: &mut State) {
        let cwd = std::env::current_dir().unwrap_or_default();
        let cp_dir = cwd.join(".context-pilot");
        let db_path = cp_dir.join("entities.db");
        let shared_dir = cp_dir.join("shared").join("entities");
        let dump_path = shared_dir.join("schema.sql");
        let migrations_dir = shared_dir.join("migrations");

        // Ensure directories exist
        let _mkdir_shared = std::fs::create_dir_all(&shared_dir);
        let _mkdir_mig = std::fs::create_dir_all(&migrations_dir);

        state.set_ext(EntitiesState::new(db_path.clone(), dump_path.clone(), migrations_dir.clone()));

        // Compute entities index UID from project hash
        if let Some(hash) = cp_mod_search::project_hash(state) {
            EntitiesState::get_mut(state).entities_index_uid = format!("cp_{hash}_entities");
        }

        // Recovery + introspection
        recover_database(&db_path, &dump_path, &migrations_dir);
        if let Ok(conn) = db::open(&db_path) {
            let cache = db::introspect(&conn, &db_path);
            EntitiesState::get_mut(state).schema_cache = Some(cache);
        }
    }

    fn reset_state(&self, state: &mut State) {
        self.init_state(state);
    }

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        // Regenerate dump + WAL checkpoint
        let es = EntitiesState::get(state);
        if let Ok(conn) = db::open(&es.db_path) {
            let _r = db::dump_to_file(&conn, &es.dump_path);
            db::checkpoint(&conn);
        }
        serde_json::Value::Null
    }

    fn load_module_data(&self, _data: &serde_json::Value, state: &mut State) {
        // Re-initialize (idempotent recovery + introspection)
        self.init_state(state);
        // Cold-start Meilisearch sync
        sync::full_reindex(state);
    }

    fn fixed_panel_types(&self) -> Vec<Kind> {
        vec![Kind::new(Kind::ENTITIES)]
    }

    fn fixed_panel_defaults(&self) -> Vec<(Kind, &'static str, bool)> {
        vec![(Kind::new(Kind::ENTITIES), "Entities", false)]
    }

    fn create_panel(&self, context_type: &Kind) -> Option<Box<dyn Panel>> {
        match context_type.as_str() {
            Kind::ENTITIES => Some(Box::new(panel::EntitiesPanel)),
            _ => None,
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let t = &*TOOL_TEXTS;
        vec![
            ToolDefinition::from_yaml("entity_sql", t)
                .short_desc("Execute SQL on entity database")
                .category("Entity")
                .param("sql", ParamType::String, true)
                .build(),
        ]
    }

    fn pre_flight(&self, _tool: &ToolUse, _state: &State) -> Option<Verdict> {
        None
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "entity_sql" => Some(tools::execute(tool, state)),
            _ => None,
        }
    }

    fn tool_visualizers(&self) -> Vec<(&'static str, cp_base::modules::ToolVisualizer)> {
        vec![("entity_sql", visualize_entity_output)]
    }

    fn context_type_metadata(&self) -> Vec<cp_base::state::context::TypeMeta> {
        vec![cp_base::state::context::TypeMeta {
            context_type: "entities",
            icon_id: "entities",
            is_fixed: true,
            needs_cache: false,
            fixed_order: Some(5),
            display_name: "entities",
            short_name: "entities",
            needs_async_wait: false,
        }]
    }

    fn overview_context_section(&self, state: &State) -> Option<String> {
        let es = EntitiesState::get(state);
        let tc = es.table_count();
        if tc == 0 {
            return None;
        }
        Some(format!("Entities: {} tables, {} rows\n", tc, es.total_rows()))
    }

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![("Entity", "Persistent relational entity database")]
    }

    fn dependencies(&self) -> &[&'static str] {
        &["search"]
    }

    fn is_core(&self) -> bool {
        false
    }

    fn save_worker_data(&self, _state: &State) -> serde_json::Value {
        serde_json::Value::Null
    }

    fn load_worker_data(&self, _data: &serde_json::Value, _state: &mut State) {}

    fn dynamic_panel_types(&self) -> Vec<Kind> {
        vec![]
    }

    fn context_display_name(&self, _context_type: &str) -> Option<&'static str> {
        None
    }

    fn context_detail(&self, _ctx: &cp_base::state::context::Entry) -> Option<String> {
        None
    }

    fn overview_render_sections(&self, _state: &State) -> Vec<(u8, Vec<cp_render::Block>)> {
        vec![]
    }

    fn on_close_context(
        &self,
        _ctx: &cp_base::state::context::Entry,
        _state: &mut State,
    ) -> Option<Result<String, String>> {
        None
    }

    fn on_user_message(&self, _state: &mut State) {}

    fn on_stream_stop(&self, _state: &mut State) {}

    fn on_tool_progress(&self, _tool_name: &str, _input_so_far: &str, _state: &mut State) {}

    fn on_tool_complete(&self, _tool_name: &str, _state: &mut State) {}

    fn watch_paths(&self, _state: &State) -> Vec<cp_base::panels::WatchSpec> {
        vec![]
    }

    fn should_invalidate_on_fs_change(
        &self,
        _ctx: &cp_base::state::context::Entry,
        _changed_path: &str,
        _is_dir_event: bool,
    ) -> bool {
        false
    }

    fn watcher_immediate_refresh(&self) -> bool {
        true
    }
}

// =============================================================================
// Tool visualizer
// =============================================================================

/// Custom conversation display for `entity_sql` tool results.
///
/// Colors table headers, row counts, errors, and NULL values.
fn visualize_entity_output(content: &str, width: usize) -> Vec<cp_render::Block> {
    use cp_render::{Block, Semantic, Span};

    content
        .lines()
        .map(|line| {
            if line.is_empty() {
                return Block::empty();
            }

            // Truncate long lines
            let display = if line.len() > width {
                format!("{}…", line.get(..line.floor_char_boundary(width.saturating_sub(1))).unwrap_or(""))
            } else {
                line.to_string()
            };

            // Error lines
            if display.starts_with("SQL error:") || display.starts_with("FAILED") {
                return Block::Line(vec![Span::styled(display, Semantic::Error)]);
            }

            // Suggestion lines
            if display.starts_with("Did you mean") {
                return Block::Line(vec![Span::styled(display, Semantic::Info)]);
            }

            // Schema summary header
            if display.starts_with("Current schema:") {
                return Block::Line(vec![Span::styled(display, Semantic::Accent).bold()]);
            }

            // Success messages
            if display.starts_with("Schema updated") || display.ends_with("row(s) affected.") {
                return Block::Line(vec![Span::styled(display, Semantic::Success)]);
            }

            // Row count footer
            if display.starts_with('(') && (display.contains("rows") || display.contains("returned")) {
                return Block::Line(vec![Span::muted(display)]);
            }

            // Table separator line
            if display.starts_with('|') && display.contains("------") {
                return Block::Line(vec![Span::muted(display).dim()]);
            }

            // Default table rows — highlight NULLs
            if display.starts_with('|') && display.contains("NULL") {
                let parts: Vec<Span> = display
                    .split("NULL")
                    .enumerate()
                    .flat_map(|(i, part)| {
                        let mut spans = Vec::new();
                        if i > 0 {
                            spans.push(Span::muted("NULL".into()).dim());
                        }
                        if !part.is_empty() {
                            spans.push(Span::new(part.to_string()));
                        }
                        spans
                    })
                    .collect();
                return Block::Line(parts);
            }

            Block::text(display)
        })
        .collect()
}

// =============================================================================
// Database recovery
// =============================================================================

/// Recover the entity database using the priority table from the design doc.
///
/// Priority: DB (if healthy) → dump → migrations → fresh start.
fn recover_database(db_path: &std::path::Path, dump_path: &std::path::Path, migrations_dir: &std::path::Path) {
    let Ok(conn) = db::open(db_path) else {
        log::warn!("Failed to open entity database — will retry on next access");
        return;
    };

    // Integrity check
    if !db::integrity_check(&conn) {
        log::warn!("Entity database corrupt, attempting recovery");
        drop(conn);
        let _rm = std::fs::remove_file(db_path);
        let Ok(fresh) = db::open(db_path) else {
            return;
        };
        if dump_path.exists()
            && let Err(e) = db::restore_from_file(&fresh, dump_path)
        {
            log::warn!("Failed to restore dump: {e}");
        }
        let _apply = migrations::apply_pending(&fresh, migrations_dir);
        return;
    }

    // DB is healthy — check if it has user tables
    if db::has_user_tables(&conn) {
        // DB is fine. Regenerate dump if missing.
        if !dump_path.exists() {
            let _dump = db::dump_to_file(&conn, dump_path);
        }
        return;
    }

    // DB is empty — try recovery from files
    if dump_path.exists()
        && let Err(e) = db::restore_from_file(&conn, dump_path)
    {
        log::warn!("Failed to restore dump: {e}");
    }

    // Apply any pending migrations beyond what the dump contained
    if let Err(e) = migrations::apply_pending(&conn, migrations_dir) {
        log::warn!("Failed to apply pending migrations: {e}");
    }
}
