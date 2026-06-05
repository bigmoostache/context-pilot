# cp-mod-entities — Design Document

> **Status:** Implemented
> **Date:** 2026-06-04  
> **Crate:** `crates/cp-mod-entities/`  
> **Depends on:** cp-base, cp-render, cp-mod-search, rusqlite

---

## 1. Vision

Give the AI a **persistent relational database** for structured domain knowledge.

The AI currently has three storage mechanisms:

| Mechanism | Structure | Queries | Updates | Relationships |
|-----------|-----------|---------|---------|---------------|
| Memories | Flat key-value | No | Yes | No |
| Scratchpad | Ephemeral cells | No | Yes | No |
| Logs | Append-only | Search only | No | No |

None support relational queries. *"Which engineers at French companies work on active projects?"* has no answer path today.

**cp-mod-entities** fills this gap: embedded SQLite, one `entity_sql` tool for arbitrary SQL, automatic Meilisearch sync for fuzzy discovery, and a fixed panel with live schema + sample data. The AI owns the schema — nothing is hard-coded.

Not every project needs entities. They shine when the AI accumulates structured knowledge that requires **cross-entity queries** — people, companies, systems, dependencies. For isolated facts, memories are simpler and sufficient.

### Why Now

- **Dependency budget recovered.** Typst removal dropped 163 packages (553 → 348). rusqlite adds ~5.
- **Meilisearch exists.** Global server, per-project indexes, background sync — all operational. Entity sync piggybacks.
- **LLMs write SQL fluently.** SQL is the natural structured interface.
- **`cc` already in tree.** rusqlite `bundled` compiles SQLite via `cc` (used by openssl-sys). Zero new build tooling.

---

## 2. Principles

1. **AI owns the schema** — no hard-coded entity types. Conventions, not constraints.
2. **SQL is the interface** — one tool, full power. LLMs are excellent at SQL.
3. **Meilisearch for discovery** — auto-indexed for fuzzy search via existing infrastructure.
4. **Single-file persistence** — SQLite at `.context-pilot/entities.db`.
5. **Zero external services** — SQLite compiles into the binary.

---

## 3. Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Storage engine | **SQLite (rusqlite, bundled)** | ACID, full SQL, in-process, 24+ years maturity. Meilisearch explicitly unsuitable as primary store (no ACID, async indexing). |
| Schema management | **Auto migrations + dump** | Every DDL auto-captured as a numbered migration file. Full dump (schema + data) on save. DB is source of truth; files are derived. Recovery: dump (primary) → migrations (fallback) → fresh start. ~220 lines. Industry-standard Rails model. |
| Meilisearch sync | **Incremental, dirty-tracked, delete-then-add** | Per-table sync: delete old docs → add current rows. Dirty tracking with retry on failure. Cold start: upsert all + orphan cleanup via facet_distribution. 3s HTTP timeout. YAML-formatted `_all_text` (500-char value cap). Meilisearch down → skip silently, dirty state preserved. |
| Schema guidance | **Suggested, not enforced** | Tool description includes conventions. AI decides. |
| Sample data in panel | **Yes, capped** | First 3 rows per table in panel context. Prevents wasted "exploration SELECTs." Capped: skip tables >10 columns, truncate values at 50 chars. |
| Error enrichment | **Fuzzy suggestions** | On "table/column not found" errors, suggest closest match from schema. Include schema in all error responses. |
| Git tracking | **Gitignore** | Binary files don't belong in git. AI can recreate schema. |

**Open:** Embedder for entities index — keyword search may suffice for short entity text. Decide during Phase 3.

---

## 4. Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                        AI / LLM                               │
│                                                                │
│  entity_sql("SELECT p.name, c.name FROM people p              │
│              JOIN companies c ON p.company_id = c.id           │
│              WHERE c.country = 'France'")                      │
└───────────────────────────┬──────────────────────────────────┘
                            │ tool call
                            ▼
┌──────────────────────────────────────────────────────────────┐
│                     cp-mod-entities                            │
│                                                                │
│  ┌──────────────┐   ┌──────────────┐   ┌──────────────────┐  │
│  │  tools.rs     │   │  panel.rs     │   │  sync.rs          │  │
│  │  SQL executor │   │  Schema view  │   │  Meili bridge     │  │
│  └──────┬───────┘   └──────┬───────┘   └────────┬─────────┘  │
│         │                  │                      │            │
│         ▼                  ▼                      ▼            │
│  ┌────────────────────────────────────────────────────────┐   │
│  │  db.rs — Connection factory + PRAGMAs + bootstrap       │   │
│  └────────────────────────┬───────────────────────────────┘   │
│                           │                                    │
│                           ▼                                    │
│  ┌────────────────────────────────────────────────────────┐   │
│  │  SQLite (WAL mode, FK ON, busy_timeout 5s)              │   │
│  │  .context-pilot/entities.db                              │   │
│  └────────────────────────────────────────────────────────┘   │
│                           │                                    │
│                    on write: fire-and-forget                    │
│                           ▼                                    │
│  ┌────────────────────────────────────────────────────────┐   │
│  │  Meilisearch index: cp_{project_hash}_entities          │   │
│  └────────────────────────────────────────────────────────┘   │
└──────────────────────────────────────────────────────────────┘
```

### Crate layout

```
crates/cp-mod-entities/src/
├── lib.rs           ~200 lines   Module trait impl (mirrors cp-mod-memory/src/lib.rs)
├── db.rs            ~300 lines   Connection factory, bootstrap, introspection, dump/restore
├── migrations.rs    ~100 lines   Auto-capture DDL, sequential replay, _meta tracking
├── tools.rs         ~350 lines   SQL execution, classification, formatting
├── panel.rs         ~200 lines   Fixed Entities panel
├── sync.rs          ~200 lines   Meilisearch sync (mirrors sync_logs_to_meilisearch)
└── types.rs         ~100 lines   State types
```

---

## 5. Data Model

### 5.1 SQLite

**Connection model:** Per-call (Connection is `!Send`). Open → PRAGMAs → operate → drop.

**PRAGMAs:** `journal_mode=WAL`, `foreign_keys=ON`, `busy_timeout=5000`, `journal_size_limit=64MB`.

**Bootstrap:** `_meta` table tracks schema version and applied migrations:

```sql
CREATE TABLE IF NOT EXISTS _meta (
    migration_id INTEGER PRIMARY KEY,
    filename TEXT NOT NULL,
    applied_at TEXT NOT NULL DEFAULT (datetime('now'))
);
```

Everything else is AI-created.

**Introspection:** `sqlite_master` for table names, `PRAGMA table_info` for columns, `PRAGMA foreign_key_list` for FKs, `COUNT(*)` for row counts. Excludes `sqlite_%` and `_meta`.

**Integrity:** `PRAGMA integrity_check` on load. If corrupt → log warning, re-create. Self-healing, never panic.

**Checkpoint:** `PRAGMA wal_checkpoint(PASSIVE)` on save. Module returns `Value::Null` — SQLite persists itself.

### 5.2 State

```rust
pub struct EntitiesState {
    pub db_path: PathBuf,
    pub dump_path: PathBuf,
    pub migrations_dir: PathBuf,
    pub schema_cache: Option<SchemaCache>,
    pub entities_index_uid: String, // "cp_{hash}_entities"
    pub dirty_tables: HashSet<String>,
    pub dropped_tables: Vec<String>,
}

pub struct SchemaCache {
    pub tables: Vec<TableInfo>,
    pub db_size_bytes: u64,
}

pub struct TableInfo {
    pub name: String,
    pub row_count: u64,
    pub columns: Vec<ColumnInfo>,
    pub foreign_keys: Vec<ForeignKeyInfo>,
}

pub struct ColumnInfo {
    pub name: String,
    pub col_type: String,
    pub is_pk: bool,
    pub is_not_null: bool,
}

pub struct ForeignKeyInfo {
    pub from_col: String,
    pub to_table: String,
    pub to_col: String,
}
```

No `Connection` in state (`!Send`). No JSON to persist (SQLite is self-persisting). DB path: `cwd / ".context-pilot" / "entities.db"`.

### 5.3 Schema Persistence (Migrations + Dump)

Two complementary mechanisms — migrations capture the **story**, the dump captures the **state**.

**File layout:**

```
.context-pilot/shared/entities/
├── schema.sql                          ← current state: CREATE + INSERT
└── migrations/
    ├── 0001_20260604T153000.sql        ← CREATE TABLE companies (...)
    ├── 0002_20260604T153100.sql        ← CREATE TABLE people (...)
    └── 0003_20260605T100000.sql        ← ALTER TABLE companies ADD COLUMN founded
```

**Migrations** — auto-captured after every successful DDL via `entity_sql`:
- One file per DDL tool call (multi-statement DDL = one file)
- Sequential numbering from `_meta` table (atomic via SQLite transaction)
- Timestamp in filename for human readability
- Written ONLY after successful execution — never for failed DDL
- Git-diffable: each schema change = one small file

**schema.sql** — auto-generated full dump:
- `CREATE TABLE IF NOT EXISTS` for all tables (including `_meta`)
- `INSERT OR IGNORE` for all rows (including `_meta` entries)
- Regenerated after every DDL (immediate) and on `save_module_data` (if DML occurred)
- **Data cap:** if dump exceeds 1 MB, omit INSERT statements + include warning comment
- `PRAGMA foreign_keys = OFF` wrapper for safe restore ordering

**Recovery priority:**

| DB state | schema.sql | migrations/ | Action |
|----------|-----------|-------------|--------|
| Has tables | Any | Any | Use DB. Regenerate files if missing. |
| Empty | Exists | Any | Apply schema.sql. Then apply migrations newer than last `_meta` entry. |
| Empty | Missing | Exist | Replay all migrations in order. |
| Empty | Missing | Missing | Fresh start. |
| Corrupt | Exists | Any | Delete DB, apply schema.sql, then pending migrations. |

**The crash gap:** DDL at T=1 creates migration file immediately. TUI crashes at T=2 before save. schema.sql is stale. On restart: DB is fine (WAL). If DB also lost: schema.sql (stale) + pending migration 0004 = full schema recovery.

**The AI never interacts with any of this.** It's infrastructure.

### 5.4 Meilisearch

**Document format** (one per SQLite row):

```json
{
  "id": "companies__42",
  "entity_table": "companies",
  "_all_text": "name: Acme Corp\ncountry: France\nfounded: 2019"
}
```

Primary key: `{table}__{rowid}`. `_all_text` is the full row serialized as YAML key-value pairs — preserves column names in indexed text for richer search (e.g. "country France" matches).

**Index settings:**

```json
{
  "searchableAttributes": ["_all_text"],
  "filterableAttributes": ["entity_table"],
  "sortableAttributes": [],
  "typoTolerance": { "enabled": true, "minWordSizeForTypos": { "oneTypo": 4, "twoTypos": 8 } }
}
```

**Sync strategy — incremental with dirty tracking:**

State: `dirty_tables: HashSet<String>` (tables with unsynced writes), `dropped_tables: Vec<String>` (tables dropped since last sync). Both in-memory, lost on crash (cold start catches up).

**Per-table sync (delete-then-add):** For each dirty table: `delete_by_filter("entity_table = '{table}'")` then `add_documents(current_rows)`. Handles INSERT/UPDATE/DELETE correctly — no orphaned docs from deleted rows. Short HTTP timeout (3s, localhost). On failure: table stays dirty for retry.

**After `entity_sql`:** Extract affected table name(s) from SQL. Mark dirty (DML/DDL) or dropped (DROP TABLE). Call `flush_sync()` — process drops first (delete by filter), then dirty tables (delete-then-add). Clear on success, keep on failure.

**Cold start (init/reload):** Upsert-then-cleanup. Sync all current tables (overwrite, never empty). Then `facet_distribution("entity_table")` to discover orphan tables in Meilisearch (tables no longer in SQLite) and delete their docs. `ensure_index()` with settings if missing.

**Meilisearch down?** Skip silently. Dirty state preserved. Next flush retries. SQL operations never blocked.

---

## 6. Tool: `entity_sql`

### Definition

```yaml
entity_sql:
  description: >
    Execute SQL against the project's entity database (SQLite). The database
    is empty on first use — create your own schema. See the Entities panel
    for current schema, sample data, and getting-started tips.

    Use entities for structured data with relationships that need querying.
    Use memories for isolated facts. Use logs for events and decisions.

    Supports full SQLite: JOINs, CTEs, window functions, foreign keys,
    triggers, views. Multi-statement (semicolons) executes atomically.
    Use RETURNING * on INSERT/UPDATE to see results without a separate SELECT.
    Use CREATE TABLE IF NOT EXISTS for idempotent schema setup.
    Schema changes are auto-tracked for reproducibility.
  params:
    sql:
      type: string
      required: true
```

### Execution semantics

| SQL type | Detection | Return value | Triggers sync? | Persistence |
|----------|-----------|-------------|----------------|-------------|
| SELECT / EXPLAIN / PRAGMA | Trimmed uppercase starts with keyword | Markdown table (≤50 rows inline, >50 → `entity_result` panel) | No | — |
| INSERT / UPDATE / DELETE | Starts with DML keyword | `"N row(s) affected."` | Yes | Dirty flag |
| CREATE / ALTER / DROP / CREATE INDEX | Starts with DDL keyword | Full schema summary | Yes | Migration file + dump |
| WITH ... SELECT (CTE) | Starts with WITH, no DML keywords | Markdown table | No | — |
| WITH ... INSERT/UPDATE/DELETE | Starts with WITH, contains DML | Affected rows | Yes | Dirty flag |
| Error | SQLite returns error | `is_error: true` + enriched error (see below) | No | — |

**Conservative fallback:** if classification is ambiguous, treat as write (sync is idempotent).

### Error enrichment

Raw SQLite errors are wrapped with context for self-correction: fuzzy-match suggestions on unknown table/column names (Levenshtein ≤2), constraint details on violations, and the current schema summary appended to every error.

### Multi-statement handling

Split on `;` respecting single-quoted string literals (state machine tracking `in_string`, handling `''` escapes). All statements execute within a single implicit transaction — any error rolls back the entire batch. Return the result of the last statement.

### Result format

```
| col1 | col2 | col3 |
|------|------|------|
| val  | val  | val  |

(N rows)
```

NULL → `NULL`. BLOB → `[BLOB N bytes]`. No alignment padding.

**Empty results:** `"0 rows returned. (Table 'X' has Y total rows.)"` — tells the AI the table isn't empty, just the filter matched nothing. Prevents unnecessary follow-up SELECTs.

**INSERT/UPDATE with RETURNING:** If the SQL includes a `RETURNING` clause, format the returned rows as a table (same as SELECT). This is the preferred pattern — the tool description recommends it.

### Lifecycle

Every `entity_sql` call: open connection → classify → execute → format result → refresh panel (`touch_panel(Kind::ENTITIES)`) → if DDL: write migration file + regenerate schema.sql → if write: fire-and-forget Meilisearch sync + set dirty flag → drop connection.

On `save_module_data`: if dirty flag set → regenerate schema.sql (captures DML changes) → clear flag.

Instrumented with `flame!("entity_sql")`.

---

## 7. Panel: Entities

Fixed panel. `Kind::ENTITIES`, `fixed_order = Some(5)` (after Memories), `needs_cache = false`.

### Populated state

Every user table (excluding `_meta`, `sqlite_%`) with name, row count, columns (name, type, PK), foreign keys. Footer: totals, DB size, migration count.

**LLM context** — schema + sample data for quick orientation:

```
Entity Database (3 tables, 89 rows, 48 KB):

companies (23 rows):
  id INTEGER PK, name TEXT, country TEXT, founded INTEGER
  FK: ceo_id → people(id)
  Sample: (1, 'Acme Corp', 'France', 2019), (2, 'Globex', 'US', 2015), (3, 'Initech', 'UK', 2021)

people (45 rows):
  id INTEGER PK, name TEXT, role TEXT, company_id INTEGER
  FK: company_id → companies(id)
  Sample: (1, 'John Doe', 'CTO', 1), (2, 'Jane Smith', 'Engineer', 2), (3, 'Bob Lee', 'PM', 1)

projects (21 rows):
  id INTEGER PK, name TEXT, status TEXT, company_id INTEGER, lead_id INTEGER
  FK: company_id → companies(id), lead_id → people(id)
  Sample: (1, 'Phoenix', 'active', 1, 1), (2, 'Atlas', 'planning', 2, 2)
```

Sample data: first 3 rows per table, values truncated at 50 chars, skip for tables >10 columns, `(empty)` for empty tables.

### Empty state (smart — carries the usage guidance)

When the database has no user tables, the panel becomes the AI's onboarding guide. This keeps the tool description lean (~14 lines) while providing rich guidance exactly when needed:

```
Entity Database (empty)

No entity tables yet. Use entity_sql to create your schema.

Quick start:
  CREATE TABLE companies (id INTEGER PRIMARY KEY, name TEXT NOT NULL, country TEXT);
  CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT, role TEXT,
    company_id INTEGER REFERENCES companies(id));
  INSERT INTO companies (name, country) VALUES ('Acme', 'France') RETURNING *;

Tips:
  - INTEGER PRIMARY KEY = auto-increment (don't use AUTOINCREMENT)
  - FOREIGN KEY constraints model relationships
  - SQLite types: TEXT, INTEGER, REAL, BLOB (VARCHAR(N) length is ignored)
  - Use RETURNING * on INSERT/UPDATE to see results immediately
  - For graph patterns: edges(source_id, target_id, rel_type)
```

**IR blocks:** `Block::KeyValue` for table headers, `Block::Line` for columns. Table names → `Accent`, types → `Code`, FKs → `Muted`.

---

## 8. Module Integration

### Cargo.toml

```toml
[dependencies]
cp-base = { path = "../cp-base" }
cp-render = { path = "../cp-render" }
cp-mod-search = { path = "../cp-mod-search" }
rusqlite = { workspace = true, features = ["bundled", "column_decltype"] }
serde_json = { workspace = true }
crossterm = { workspace = true }
log = { workspace = true }
```

`rusqlite` declared as workspace dep in root Cargo.toml. `bundled` compiles SQLite via `cc`. `column_decltype` for type introspection.

### Registration

Follow cp-mod-memory pattern. Key specifics: `Kind::ENTITIES`, `fixed_order=5`, `id="entities"`, `dependencies=["search"]`, `is_global=true`, `is_core=false`. Tool category: `("Entity", "Persistent relational entity database")`. Overview: `"Entities: N tables, M rows\n"` or `None`. YAML validation count 19→20.

### Cross-Module Concerns

**MeiliClient:** Add `pub fn meili_client(state: &State) -> Option<MeiliClient>` to cp-mod-search. Currently `pub(crate)`.

**Search scope:** cp-mod-entities exposes `pub fn entities_index_uid(state: &State) -> Option<String>`. Search module calls this when scope includes entities. `None` → silently skipped. Adds `scope="entities"` to search tool.

**Visualizer:** Table headers → `Accent`, row counts → `Success`, NULLs → `Muted + dimmed`, schema → `Code`.

---

## 9. Risks

| Risk | Impact | Mitigation |
|------|--------|------------|
| SQLite C compilation fails on cross-compilation | High | `cc` already cross-compiles OpenSSL in CI. SQLite amalgamation is simpler. Test early in Phase 1. |
| rusqlite exceeds dep budget (>8 new crates) | Medium | Audit `cargo tree -p rusqlite --depth 1` before merging. |

---

## 10. Justified Decisions — Dropped Alternatives

| Alternative | Why dropped |
|-------------|-------------|
| **No ORM / no schema management** | "AI owns it all" sounds elegant but fails on reproducibility. DB corruption = total schema loss. No git-trackable history. No cross-project portability. Not professional. |
| **Dump only (no migrations)** | A backup, not schema management. No audit trail — "when was this column added?" requires digging git log of one monolithic file. Can't bridge the crash gap (DDL after last save is lost). |
| **Migrations only (no dump)** | No data recovery. AI spends an hour populating 200 entities, DB corrupts, migrations replay empty tables. Unacceptable. |
| **Declarative schema file (Model A — file is source of truth)** | Requires schema diffing engine (500+ lines) to reconcile file vs DB. SQLite ALTER TABLE limitations make automatic reconciliation fragile. Over-engineering for a utility module. |
| **Full ORM (Diesel/SeaORM)** | Schema defined in Rust structs, compile-time validation. Defeats the purpose — AI can't change schema at runtime. Massive complexity. |
| **Migration files created by the AI explicitly** | Adds a second tool (`entity_migrate`), doubles cognitive load. The AI already writes DDL via `entity_sql` — auto-capturing it is zero-overhead. |
| **YAML schema definition** | YAML to describe SQL schemas is a pointless translation layer. SQL is already a schema definition language. |
| **Track .db binary in git** | Binary diffs are useless. Merge conflicts unresolvable. File grows. Git-lfs requires setup (violates "zero user setup"). |

---

## 11. Implementation Plan

Three phases. Each ends with a verifiable milestone. Every step references exact file paths, function signatures, and codebase patterns.

### Phase 1: Crate scaffold + wiring

**Goal:** Empty module compiles, panel shows "empty", tool definition visible.

**Files created:**
- `crates/cp-mod-entities/Cargo.toml`
- `crates/cp-mod-entities/src/lib.rs`
- `crates/cp-mod-entities/src/types.rs`
- `yamls/tools/entities.yaml`

**Files modified:**
- `Cargo.toml` (workspace root)
- `crates/cp-base/src/state/context.rs`
- `crates/cp-base/src/lib.rs`
- `src/modules/mod.rs`
- `yamls/themes.yaml`

**Steps:**

1. **Cargo.toml (workspace root)** — Add `"crates/cp-mod-entities"` to `[workspace].members` (after `cp-mod-search`). Add workspace dep: `rusqlite = { version = "0.33", features = ["bundled", "column_decltype"] }`. Add binary dep: `cp-mod-entities = { path = "crates/cp-mod-entities" }`.

2. **`crates/cp-mod-entities/Cargo.toml`** — Follow `cp-mod-memory/Cargo.toml` pattern. Dependencies: `cp-base`, `cp-render`, `cp-mod-search` (path dep), `rusqlite` (workspace), `serde_json` (workspace), `crossterm` (workspace), `log` (workspace), `cp-mod-utilities` (workspace).

3. **`crates/cp-mod-entities/src/types.rs`** — Define `EntitiesState`, `SchemaCache`, `TableInfo`, `ColumnInfo`, `ForeignKeyInfo`. Use `state.set_ext()` / `state.ext::<T>()` pattern from `cp-mod-memory/src/types.rs`. Add `EntitiesState::get(state) -> &Self` and `get_mut(state) -> &mut Self` helpers (same as `MemoryState`).

4. **`crates/cp-mod-entities/src/lib.rs`** — Skeleton Module trait impl:
   - `id() → "entities"`, `name() → "Entities"`, `is_global() → true`, `is_core() → false`
   - `dependencies() → &["search"]` (needs Meilisearch)
   - `init_state()` → `state.set_ext(EntitiesState::new(db_path))` where `db_path = cwd / ".context-pilot" / "entities.db"`
   - `tool_definitions()` → one tool `entity_sql` via `ToolDefinition::from_yaml("entity_sql", t).short_desc("Execute SQL on entity database").category("Entity").param("sql", ParamType::String, true).build()`
   - `execute_tool()` → stub returning "Not yet implemented"
   - `create_panel()` → stub returning empty Block::Line
   - `fixed_panel_types() → vec![Kind::new(Kind::ENTITIES)]`
   - `fixed_panel_defaults() → vec![(Kind::new(Kind::ENTITIES), "Entities", false)]`
   - `context_type_metadata()` → `TypeMeta { context_type: "entities", icon_id: "entities", is_fixed: true, needs_cache: false, fixed_order: Some(5), display_name: "entities", short_name: "entities", needs_async_wait: false }`
   - `tool_category_descriptions() → vec![("Entity", "Persistent relational entity database")]`
   - Static `TOOL_TEXTS: LazyLock<ToolTexts>` from `include_str!("../../../yamls/tools/entities.yaml")`

5. **`yamls/tools/entities.yaml`** — Tool description. Follow `yamls/tools/memory.yaml` format. Single tool `entity_sql` with `sql` parameter.

6. **`crates/cp-base/src/state/context.rs`** — Add `pub const ENTITIES: &str = "entities";` after `QUEUE` (line 142). Add `pub const ENTITY_RESULT: &str = "entity_result";` for future dynamic panel.

7. **`crates/cp-base/src/lib.rs`** — Add `("entities", include_str!("../../../yamls/tools/entities.yaml")),` to the tool YAML test array (line ~220, alphabetical — after `core`). Test count 19 → 20.

8. **`src/modules/mod.rs`** — Add `pub(crate) use cp_mod_entities::EntitiesModule;` (after `SearchModule` import, line ~44). Add `Box::new(EntitiesModule),` in `all_modules()` after `Box::new(SearchModule)` (after line 129).

9. **`yamls/themes.yaml`** — Add `entities: "🗃️"` under each theme's `context:` map (6 themes, ~lines 15/66/117/168/219/270). After existing `spine:` entry.

10. **Audit deps:** `cargo tree -p rusqlite --features bundled,column_decltype --depth 2 --no-default-features`. Expected: rusqlite → libsqlite3-sys (→ cc, pkg-config) + hashlink + fallible-iterator + fallible-streaming-iterator. Total ≤ 8 new crates.

**Verify:** `cargo build --release` clean. `cargo test` passes (YAML validation count updated). Panel shows empty state. Tool appears in Tools panel under "Entity" category.

### Phase 2: Core (DB + Tool + Panel + Schema Persistence)

**Goal:** `entity_sql` fully functional — CREATE, INSERT, SELECT, ALTER, DROP all work. Panel shows live schema + sample data. Migrations auto-generated. Schema dump on save.

**Files created:**
- `crates/cp-mod-entities/src/db.rs`
- `crates/cp-mod-entities/src/migrations.rs`
- `crates/cp-mod-entities/src/tools.rs`
- `crates/cp-mod-entities/src/panel.rs`

**Steps:**

1. **`db.rs` — Connection factory (~80 lines):**
   - `pub(crate) fn open(db_path: &Path) -> Result<rusqlite::Connection, String>` — open, set PRAGMAs (`journal_mode=WAL`, `foreign_keys=ON`, `busy_timeout=5000`, `journal_size_limit=67108864`), create `_meta` table (`migration_id INTEGER PRIMARY KEY, filename TEXT NOT NULL, applied_at TEXT NOT NULL DEFAULT (datetime('now'))`).
   - Connection is `!Send` — open per-call, never store in state.

2. **`db.rs` — Introspection (~120 lines):**
   - `pub(crate) fn introspect(conn: &Connection) -> SchemaCache` — query `sqlite_master` for tables (exclude `sqlite_%`, `_meta`), `PRAGMA table_info(t)` for columns, `PRAGMA foreign_key_list(t)` for FKs, `SELECT COUNT(*) FROM t` for row counts. Compute DB file size via `std::fs::metadata`. Return `SchemaCache { tables, db_size_bytes }`.
   - `pub(crate) fn integrity_check(conn: &Connection) -> bool` — `PRAGMA integrity_check` returns `"ok"` on success.

3. **`db.rs` — Dump and restore (~100 lines):**
   - `pub(crate) fn dump_to_file(db_path: &Path, dump_path: &Path) -> Result<(), String>` — open connection, query all tables from `sqlite_master`, emit `CREATE TABLE IF NOT EXISTS` + `INSERT OR IGNORE` for each (including `_meta`). Wrap in `PRAGMA foreign_keys = OFF/ON`. Skip INSERT statements if total file would exceed 1 MB (write warning comment instead). Write to `dump_path`.
   - `pub(crate) fn restore_from_file(conn: &Connection, dump_path: &Path) -> Result<(), String>` — read file, `conn.execute_batch()` with FK off. Used during recovery.

4. **`migrations.rs` (~100 lines):**
   - `pub(crate) fn write_migration(conn: &Connection, migrations_dir: &Path, sql: &str) -> Result<String, String>` — next ID from `SELECT COALESCE(MAX(migration_id), 0) + 1 FROM _meta`, timestamp from `cp_mod_utilities::time`, filename `{id:04}_{timestamp}.sql`, write file, INSERT into `_meta`. Return filename.
   - `pub(crate) fn list_files(dir: &Path) -> Vec<PathBuf>` — sorted glob of `*.sql` in dir.
   - `pub(crate) fn apply_pending(conn: &Connection, dir: &Path) -> Result<u32, String>` — compare `_meta` max ID vs file list, apply unapplied in order, INSERT each into `_meta`. Return count applied.
   - `pub(crate) fn last_applied_id(conn: &Connection) -> i64` — `SELECT COALESCE(MAX(migration_id), 0) FROM _meta`.

5. **`tools.rs` — SQL classification (~40 lines):**
   - `fn classify(sql: &str) -> SqlKind` — enum `SqlKind { Select, Dml, Ddl, Error }`. Trim, uppercase first word. SELECT/EXPLAIN/PRAGMA → Select. INSERT/UPDATE/DELETE → Dml. CREATE/ALTER/DROP → Ddl. WITH → check for DML keywords after CTE. Default → Dml (conservative).

6. **`tools.rs` — Multi-statement splitting (~30 lines):**
   - `fn split_statements(sql: &str) -> Vec<&str>` — split on `;` with string literal awareness (track `in_string` flag, handle `''` escapes). Filter empty. Same pattern as described in §6.

7. **`tools.rs` — Execution (~200 lines):**
   - `pub(crate) fn execute(tool: &ToolUse, state: &mut State) -> ToolResult` with `let _fg = cp_base::flame!("entity_sql");`
   - Extract `sql` param. Open connection via `db::open(&es.db_path)`.
   - Split statements. Execute in implicit transaction (`conn.execute_batch` or per-statement `conn.execute` / `conn.prepare` + iterate).
   - Per `SqlKind`:
     - **Select:** `conn.prepare(sql)` → iterate rows → format as markdown table. Cap at 50 rows inline. If >50: create `DynPanel` with `context_type: "entity_result"` (follow search_result pattern from `cp-mod-search/src/tools.rs:349-365`).
     - **Dml:** `conn.execute(sql, [])` → `changes()` for affected row count. If SQL contains `RETURNING`: format returned rows as table. Set sync flag.
     - **Ddl:** `conn.execute_batch(sql)` → write migration via `migrations::write_migration()` → regenerate dump via `db::dump_to_file()`. Set sync flag.
   - On error: wrap with `enrich_error()` (see below). Include schema summary.
   - After success: `state.touch_panel(Kind::ENTITIES)` to trigger panel refresh. Update `schema_cache` in state.

8. **`tools.rs` — Error enrichment (~40 lines):**
   - `fn enrich_error(err: &str, schema: &SchemaCache) -> String` — parse error for "no such table: X" or "no such column: X" patterns. Collect all table/column names from schema. Levenshtein distance ≤ 2 → suggest closest. Append schema summary to all errors.

9. **`tools.rs` — Result formatting (~40 lines):**
   - `fn format_table(stmt: &Statement, rows: Vec<Vec<String>>, count: usize) -> String` — column headers from `stmt.column_names()`, values formatted as markdown table. NULL → `NULL`, BLOB → `[BLOB N bytes]`. Footer: `(N rows)`. Empty: `"0 rows returned. (Table 'X' has Y total rows.)"`.

10. **`panel.rs` (~200 lines):**
    - `pub(crate) struct EntitiesPanel;` implementing `Panel` trait.
    - `blocks()` — follow `cp-mod-memory/src/panel.rs` pattern. Empty state: tips/quick-start (as described in §7). Populated: table headers as `Block::KeyValue`, columns as `Block::Line`, FKs as muted lines, sample data. Footer: totals + migration count.
    - `context()` — return `ContextItem` with schema + sample data text (3 rows/table, 50-char truncation, skip >10 columns). Follow memory's `format_memories_for_context` pattern.
    - `refresh()` — open connection, call `db::introspect()`, update `EntitiesState.schema_cache`. Update token count in Entry. Follow memory's `refresh()` exactly (find Entry by `Kind::ENTITIES`, call `update_if_changed`).
    - `title()` → `"Entities"`
    - `needs_cache()` → `false`, `max_freezes()` → `0`

11. **`lib.rs` — Complete Module trait:**
    - `init_state()` — create `.context-pilot/` dir if needed, set `EntitiesState` with `db_path`, `dump_path` (`.context-pilot/shared/entities/schema.sql`), `migrations_dir` (`.context-pilot/shared/entities/migrations/`). Create dirs. Run recovery logic:
      1. Try `db::open()` + `db::integrity_check()` — if OK, use DB as-is, regenerate dump/migrations if missing.
      2. If DB empty (no user tables in `sqlite_master` besides `_meta`): try restore from `dump_path`, then `migrations::apply_pending()`.
      3. If DB corrupt: delete DB file, re-create, restore from dump, apply pending migrations.
    - `save_module_data()` — always call `db::dump_to_file()` (reads from SQLite, always current). Return `Value::Null` (SQLite is self-persisting).
    - `load_module_data()` — same recovery logic as `init_state()`. Idempotent.
    - `execute_tool()` — match `"entity_sql"` → `tools::execute()`.
    - `overview_context_section()` — `"Entities: N tables, M rows\n"` or `None`.
    - `tool_visualizers()` — optional, can add later.

12. **Add `entity_result` dynamic panel type** — register in `context_type_metadata()` and `dynamic_panel_types()`. Create a simple content panel (same pattern as `SearchResultPanel` — stores content in metadata, renders from cache).

**Verify:** Create tables, insert data, query with JOINs. Panel shows schema + sample data. `git log .context-pilot/shared/entities/` shows migration files. Corrupt DB and restart → auto-recovery. All 6 callbacks green.

### Phase 3: Meilisearch + search scope + polish

**Goal:** Entity data discoverable via `search(scope="entities")`. Full integration complete.

**Files created:**
- `crates/cp-mod-entities/src/sync.rs`

**Files modified:**
- `crates/cp-mod-search/src/meili/client.rs` (make `MeiliClient` + key methods `pub`)
- `crates/cp-mod-search/src/lib.rs` (add `pub fn meili_credentials()`)
- `crates/cp-mod-search/src/tools.rs` (add `"entities"` scope)
- `yamls/tools/search.yaml` (add `"entities"` to scope enum description)
- `docs/search-module.md` (document entities index)

**Steps:**

1. **Expose Meilisearch API from cp-mod-search:**
   - `crates/cp-mod-search/src/meili/client.rs` — change `pub(crate) struct MeiliClient` to `pub struct MeiliClient`. Change `new()`, `create_index()`, `update_settings()`, `index_exists()`, `add_documents()`, `delete_documents_by_filter()` from `pub(crate)` to `pub`.
   - `crates/cp-mod-search/src/meili/mod.rs` — add `pub use client::MeiliClient;` re-export.
   - `crates/cp-mod-search/src/lib.rs` — add:
     ```rust
     pub fn meili_credentials(state: &State) -> Option<(u16, String)> {
         let ss = state.get_ext::<SearchState>()?;
         (ss.persist.port > 0).then(|| (ss.persist.port, ss.persist.master_key.clone()))
     }
     pub fn project_hash(state: &State) -> Option<String> {
         state.get_ext::<SearchState>().map(|ss| ss.persist.project_hash.clone())
     }
     ```
   - Re-export: `pub use meili::client::MeiliClient;` from `cp-mod-search/src/lib.rs`.

2. **`crates/cp-mod-entities/src/sync.rs` (~180 lines):**
   - `pub(crate) fn ensure_index(port: u16, key: &str, index_uid: &str) -> Result<(), String>` — create `MeiliClient::new(port, key)`. `create_index(uid, "id")` + `update_settings()` with: `searchableAttributes: ["_all_text"]`, `filterableAttributes: ["entity_table"]`.
   - `pub(crate) fn sync_table(port: u16, key: &str, db_path: &Path, index_uid: &str, table_name: &str) -> Result<(), String>` — delete-then-add: `delete_by_filter("entity_table = '{table}'")`, then open connection, SELECT all rows, build YAML docs, `add_documents()`. 3s HTTP timeout. rowid-based doc IDs with WITHOUT ROWID fallback.
   - `pub(crate) fn flush_sync(state: &mut State)` — get credentials via `cp_mod_search::meili_credentials(state)`. Process `dropped_tables` first (delete by filter), then `dirty_tables` (sync_table). Clear entries on success, keep on failure.
   - `pub(crate) fn full_reindex(state: &mut State)` — ensure_index, sync ALL current tables (upsert), then `facet_distribution("entity_table")` to find orphan tables and delete their docs.
   - `pub(crate) fn extract_affected_tables(statements: &[&str]) -> Vec<String>` — parse table names from all SQL statements. Handles INSERT INTO, UPDATE, DELETE FROM, CREATE/ALTER/DROP TABLE, quoted identifiers.
   - Helper: `fn build_docs(conn, table, columns) -> Vec<serde_json::Value>` — YAML-formatted `_all_text` (500-char value cap, newlines replaced).

3. **Wire sync into tools.rs:**
   - After successful execution: call `sync::extract_affected_tables()` on the SQL statements.
   - For DML/DDL (non-DROP): add affected tables to `dirty_tables`.
   - For DROP TABLE: add to `dropped_tables`, remove from `dirty_tables`.
   - Call `sync::flush_sync(state)` — processes drops first, then dirty tables.

4. **Wire cold start into lib.rs:**
   - In `init_state()` / `load_module_data()`: compute `entities_index_uid = format!("cp_{}_entities", project_hash)`. Call `sync::full_reindex(state)`.
   - In `save_module_data()`: call `sync::flush_sync(state)` (last chance before shutdown).

5. **Add `"entities"` search scope (convention-based, zero coupling):**
   - `crates/cp-mod-search/src/tools.rs` — add `let search_entities = scope == "all" || scope == "entities";` alongside existing `search_files`/`search_logs`. When `search_entities`: construct `cp_{project_hash}_entities` by convention, query it. If index doesn't exist (Meilisearch returns `index_not_found`), skip silently with empty results. Merge entity results with file/log results.
   - `yamls/tools/search.yaml` — update `scope` description: `"'all' (files, logs, entities), 'project' (files), 'logs', 'entities'"`.
   - No import of cp-mod-entities from cp-mod-search. Both agree on the `cp_{hash}_entities` naming convention.

6. **Tool visualizer** (`lib.rs`):
   - Register `("entity_sql", visualize_entity_output)`. Color: table headers → Accent, row counts → green, NULLs → muted+dimmed, errors → red. Follow `visualize_memory_output` pattern from `cp-mod-memory/src/lib.rs:180-210`.

7. **Overview section** (`lib.rs`):
   - `overview_context_section()` → `"Entities: N tables, M rows\n"` from `schema_cache`. Return `None` if no user tables.

8. **Documentation:**
   - Update `docs/search-module.md` to mention entities index.
   - Update `docs/design-entities.md` status to "Implemented".

**Verify:** `entity_sql("CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT)")` → migration file appears. `entity_sql("INSERT INTO test VALUES (1, 'hello')")` → Meilisearch receives document. `search(query="hello", scope="entities")` → finds the row. `search(query="hello", scope="all")` → includes entity results alongside files and logs. All 6 callbacks green. `cargo test` 48/48. Commit + push.

---

## 12. Future Extensions

| Extension | When |
|-----------|------|
| Graph visualization in panel (ASCII/IR) | User demand |
| Explicit export/import commands (CSV, JSON, selective SQL) | Data portability beyond auto-dump |
| Spine notifications on entity changes | Automation use cases |
| FTS5 full-text search columns | Large text fields |

---

## Appendix A: Reference Implementations

| Pattern | Source file |
|---------|-----------|
| Module trait impl | `crates/cp-mod-memory/src/lib.rs` |
| Tool execution + `flame!()` | `crates/cp-mod-memory/src/tools.rs` |
| Fixed panel (blocks, context, refresh) | `crates/cp-mod-memory/src/panel.rs` |
| Meilisearch index creation + settings | `crates/cp-mod-search/src/meili/bootstrap.rs` |
| Fire-and-forget document upsert | `crates/cp-mod-search/src/lib.rs::sync_logs_to_meilisearch` |
| MeiliClient API | `crates/cp-mod-search/src/meili/client.rs` |
| Search tool scope handling | `crates/cp-mod-search/src/tools.rs` |
| Module registration | `src/modules/mod.rs::all_modules()` |
| Kind constant | `cp-base/src/state/context.rs` |
| Tool YAML format | `yamls/tools/memory.yaml` |
| YAML validation test | `cp-base/src/lib.rs` (compile-time, 19 → 20 files) |

### Appendix B: Dependency Audit (pre-merge)

```bash
cargo tree -p rusqlite --features bundled --depth 2 --no-default-features
```

Expected: rusqlite → libsqlite3-sys → cc (already in tree) + hashlink, fallible-iterator, fallible-streaming-iterator. Total ≤ 8 new crates.
