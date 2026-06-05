//! Entity module state types.

use std::collections::HashSet;
use std::path::PathBuf;

use cp_base::state::runtime::State;

// =============================================================================
// EntitiesState — stored in the State TypeMap
// =============================================================================

/// Runtime state for the entities module.
///
/// Stored in `State` via `state.set_ext()`. Use [`EntitiesState::get`] and
/// [`EntitiesState::get_mut`] for typed access.
#[derive(Debug)]
pub struct EntitiesState {
    /// Path to the `SQLite` database file (`.context-pilot/entities.db`).
    pub db_path: PathBuf,
    /// Path to the full schema + data dump (`.context-pilot/shared/entities/schema.sql`).
    pub dump_path: PathBuf,
    /// Directory for auto-generated migration files (`.context-pilot/shared/entities/migrations/`).
    pub migrations_dir: PathBuf,
    /// Cached schema introspection (populated on init and after DDL).
    pub schema_cache: Option<SchemaCache>,
    /// Meilisearch index UID for entities (`cp_{hash}_entities`). Empty until search wired.
    pub entities_index_uid: String,
    /// Tables with unsynced writes (DML/DDL since last flush). In-memory only.
    pub dirty_tables: HashSet<String>,
    /// Tables dropped since last sync. In-memory only.
    pub dropped_tables: Vec<String>,
}

impl EntitiesState {
    /// Create a new `EntitiesState` with the given database path.
    #[must_use]
    pub fn new(db_path: PathBuf, dump_path: PathBuf, migrations_dir: PathBuf) -> Self {
        Self {
            db_path,
            dump_path,
            migrations_dir,
            schema_cache: None,
            entities_index_uid: String::new(),
            dirty_tables: HashSet::new(),
            dropped_tables: Vec::new(),
        }
    }

    /// Borrow the `EntitiesState` from the global `State`.
    ///
    /// # Panics
    ///
    /// Panics if `EntitiesState` was never inserted via `state.set_ext()`.
    #[must_use]
    pub fn get(state: &State) -> &Self {
        state.ext::<Self>()
    }

    /// Mutably borrow the `EntitiesState` from the global `State`.
    ///
    /// # Panics
    ///
    /// Panics if `EntitiesState` was never inserted via `state.set_ext()`.
    #[must_use]
    pub fn get_mut(state: &mut State) -> &mut Self {
        state.ext_mut::<Self>()
    }

    /// Total row count across all tables (from cache).
    #[must_use]
    pub fn total_rows(&self) -> u64 {
        self.schema_cache.as_ref().map_or(0, |sc| sc.tables.iter().map(|t| t.row_count).sum())
    }

    /// Total table count (from cache).
    #[must_use]
    pub fn table_count(&self) -> usize {
        self.schema_cache.as_ref().map_or(0, |sc| sc.tables.len())
    }
}

// =============================================================================
// SchemaCache — introspection snapshot
// =============================================================================

/// Cached snapshot of the `SQLite` schema, refreshed after DDL and on init.
#[derive(Debug, Clone)]
pub struct SchemaCache {
    /// All user tables (excludes `sqlite_%` and `_meta`).
    pub tables: Vec<TableInfo>,
    /// Database file size in bytes.
    pub db_size_bytes: u64,
}

/// Metadata for a single user table.
#[derive(Debug, Clone)]
pub struct TableInfo {
    /// Table name.
    pub name: String,
    /// Current row count.
    pub row_count: u64,
    /// Column definitions.
    pub columns: Vec<ColumnInfo>,
    /// Foreign key constraints.
    pub foreign_keys: Vec<ForeignKeyInfo>,
}

/// Column metadata from `PRAGMA table_info`.
#[derive(Debug, Clone)]
pub struct ColumnInfo {
    /// Column name.
    pub name: String,
    /// Declared column type (e.g., `TEXT`, `INTEGER`).
    pub col_type: String,
    /// Whether this column is (part of) the primary key.
    pub is_pk: bool,
    /// Whether `NOT NULL` constraint is set.
    pub is_not_null: bool,
}

/// Foreign key metadata from `PRAGMA foreign_key_list`.
#[derive(Debug, Clone)]
pub struct ForeignKeyInfo {
    /// Column in this table.
    pub from_col: String,
    /// Referenced table.
    pub to_table: String,
    /// Referenced column.
    pub to_col: String,
}
