# Meilisearch Integration — Design Document

> **Status**: Draft — ping-pong design phase
> **Branch**: `meilisearch`
> **Date**: 2026-05-06

## 1. Vision

Integrate Meilisearch as a core search backbone for Context Pilot, enabling:
1. **Log search** — Full-text search across all log entries (decisions, actions, events)
2. **Project indexing** — Full-text search across project files for codebase exploration

## 2. Decisions Log

### Round 1 — Foundational (2026-05-06)
| Decision | Choice | Notes |
|----------|--------|-------|
| Deployment | **Embedded** | CP downloads + manages Meilisearch binary (like Tuwunel). Zero user setup. |
| Module shape | **New module** (`cp-mod-search`) | Standalone crate owns Meilisearch lifecycle, indexes, and search tools. |
| Index depth | **Chunks** | Split files into semantic chunks (functions, classes, sections). |
| Index timing | **Background** | File watcher triggers incremental re-indexing. Initial full index on init. |
| Logs | **Migrate to Meilisearch** | Meilisearch becomes the primary store for logs. |

### Round 2 — Critical Details (2026-05-06)
| Decision | Choice | Notes |
|----------|--------|-------|
| OCR | **Datalab cloud API** | Use Surya Datalab API (DATALAB_API_KEY). No local Python/torch. HTTP-based. |
| File safety | **Explicit allowlist** | Only index files whose extension is in a known-safe list. Binary/unknown = skip. |
| Log storage | **Dual write** | JSON files remain source of truth. Meilisearch is search layer. Resilient. |
| Chunking | **Tree-sitter AST** | Parse code into semantic chunks (functions, structs, classes). Fallback for unsupported langs. |

### Round 3 — Schema & UX (2026-05-06)
| Decision | Choice | Notes |
|----------|--------|-------|
| Search tools | **One unified `search` tool** | Single tool with scope/filter params. Needs very well-designed definition + panel. |
| Result UX | **Dynamic panel** | Search results appear as a persistent, scrollable panel (like brave_search). |
| TS fallback | **Fixed-size (chars)** | Split on character count, break on line boundaries. Architecture supports custom splitters. |
| Reindex strategy | **Delete + re-insert** | On file change: delete all chunks for that path → re-chunk → re-insert. Brief gap acceptable. |

### Round 4 — Watcher & Infra (2026-05-06)
| Decision | Choice | Notes |
|----------|--------|-------|
| File watcher | **Separate `notify::Watcher`** | Existing infra is panel-centric (`should_invalidate_on_fs_change` → bool, no path forwarding, `&self` immutable). Adapting the Module trait is possible but invasive. Two `notify` instances have negligible overhead (OS-level, microseconds per event). Cleaner ownership: search module manages its own watcher, gets direct path events for re-indexing. |

### Round 5 — Server & Config (2026-05-06)
| Decision | Choice | Notes |
|----------|--------|-------|
| Search tool params | **`search(query, scope?, path_pattern?, limit?)`** | `scope`: all\|project\|logs. `path_pattern`: regex on relative path (replaces language param). `limit`: 1–50, default 20. *Superseded by Round 9: structured params.* |
| Server model | **Global (Tuwunel pattern)** | One Meilisearch at `~/.context-pilot/meilisearch/`. Per-project indexes: `cp_{hash8}_files`, `cp_{hash8}_logs`. First project downloads binary + starts. Subsequent reuse. |
| Extension config | **Config file, no dedicated tool** | Hardcoded defaults + overrides in `.context-pilot/search.toml`. LLM uses file tools to edit. Config path noted in search tool's YAML description. |
| Boot | **Like Tuwunel** | Launched at module init if not already running. Health check on connect. Background start, graceful degradation. |
| Orphan cleanup | **Auto on server start** | On startup, scan index list for project paths that no longer exist. Delete orphaned indexes. |

### Round 6 — Data Model (2026-05-06)
| Decision | Choice | Notes |
|----------|--------|-------|
| File schema | **Full metadata** | id, file_path, content, extension, chunk_type, chunk_name, line_start, line_end, char_start, char_end, last_modified_ms. Filterable: file_path, extension, chunk_type. Sortable: last_modified_ms. |
| Log schema | **Extended** | id, content, timestamp_ms, worker_id, importance, labels (freeform), tags (from curated list). Drop parent_id/children — no more summary hierarchy. |
| Log redesign | **Drop summarization** | Remove log_summarize and log_toggle tools. Replaced by Meilisearch full-text search + tag/label filtering. Maintain curated tag list at project level. |
| Indexer | **In-process thread** | Background thread in TUI process. Owns notify::Watcher. Receives file events via channel. Batches and indexes. |
| Splitter | **Trait chain** | `trait Splitter { fn split() -> Vec<Chunk>; fn supports() -> bool; }`. Chain: TreeSitter → FixedSize fallback. Extensible. |

### Round 7 — Log Redesign & Tags (2026-05-06)
| Decision | Choice | Notes |
|----------|--------|-------|
| Tags | **Single freeform field** | One `tags` array. No curated-vs-freeform split. LLM uses any tags. Filterable in Meilisearch. |
| Tag config | **config.json** | Curated suggested tags in main config (suggestions, not enforced). |
| Log tools | **Simplified** | log_summarize → removed. log_toggle → removed. log_create → updated (tags, importance). Log panel → removed, replaced by `search` tool with `scope=logs`. |
| Log lifecycle | **Keep forever** | All logs persist. Meilisearch handles scale. JSON files grow but manageable. |
| Sorting | **Essential** | `sort` param added to search tool: "relevance" (default), "date_asc", "date_desc". Logs sortable by timestamp_ms. Files sortable by last_modified_ms. |
| Date filtering | **Essential** | `from_date` and `to_date` params (ISO 8601) added to search tool. Filter logs by date range. Also works on file last_modified. |
| Log datetime | **Dual field** | `timestamp_ms` (numeric, sorting/filtering) + `datetime` (ISO 8601, display). Both stored. |

### Round 8 — Final Decisions (2026-05-06)
| Decision | Choice | Notes |
|----------|--------|-------|
| Panel design | **Rich** | Files: path, line range, snippet w/ highlights, chunk type/name. Logs: datetime, importance, tags, content. |
| CCH tool | **Keep + update** | Close_conversation_history survives. Add tags + importance params for the logs it creates. |
| Migration | **Clean slate** | Wipe old logs on migration. Fresh start with new schema. |

### Round 9 — Grey Area Resolution (2026-05-06)
| Decision | Choice | Notes |
|----------|--------|-------|
| path_pattern | **Drop regex → structured params** | Replace `path_pattern` (regex) with `path_prefix` (string) + `extension` (string). Maps to Meilisearch native filters. No client-side post-filtering needed. |
| Log indexing | **Filesystem only** | Remove `IndexerCmd::IndexLog`. Zero coupling. Watch `.context-pilot/logs/` via `notify`. |
| File size cap | **1 MB** | Skip files > 1MB. Covers 99% of code files. |
| Indexing overlay | **Keyboard shortcut (Ctrl+I)** | Floating overlay: queue depth, errors, indexed count, last activity. Dismissible. |
| Error handling | **Buffer + retry** | Buffer failed ops in memory. Retry every 5s, exponential backoff. Cap buffer at 1000 items. Flush on reconnect. |
| Panel context | **Full results + `include_context` param** | New tool param: `include_context` (bool, default true). true → YAML results in tool_result + panel. false → panel only ("peek" mode). |
| Server shutdown | **Never stop** | Runs until machine restart or manual kill. Tiny idle footprint (~20MB). |
| Init progress | **Overlay only (Ctrl+I)** | No spine notifications. User checks overlay for indexing progress. |
| Debounce | **200ms** | After last FS event, wait 200ms, then batch-process all accumulated changes. |
| MS version | **Auto-latest** | Check GitHub Releases API on first download. |
| Symlinks | **Skip** | Don't follow. Prevents infinite loops with recursive watching. |
| Persistence | **Full persist** | Serialize: port, key, hash, index_ready. Re-create on reload: watcher, indexer thread, channel. |
| Transition | **Hard cutover** | Once cp-mod-search lands, old LOGS panel is removed. No deprecation period. |
| Memories | **Out of scope** | Files + logs only. Memories as future third index. |

---

## 3. Architecture

### System Overview

```
┌──────────────────────────────────────────────────────────────────────┐
│                         TUI Process                                  │
│                                                                      │
│  ┌──────────────┐   ┌──────────────────┐   ┌───────────────────┐    │
│  │  cp-mod-logs  │   │  cp-mod-search    │   │  Other modules    │    │
│  │              │   │                  │   │                   │    │
│  │ log_create ──┼──►│ Indexer Thread   │   │                   │    │
│  │ (JSON write) │   │  ┌────────────┐  │   │                   │    │
│  │              │   │  │ notify::   │  │   │                   │    │
│  │ CCH tool ───┼──►│  │ Watcher    │  │   │                   │    │
│  │              │   │  └─────┬──────┘  │   │                   │    │
│  └──────────────┘   │        │         │   └───────────────────┘    │
│                     │  ┌─────▼──────┐  │                            │
│                     │  │ Splitter   │  │                            │
│                     │  │ Chain      │  │                            │
│                     │  └─────┬──────┘  │                            │
│                     │        │         │                            │
│                     │  ┌─────▼──────┐  │                            │
│                     │  │ Meilisearch│  │                            │
│                     │  │ HTTP Client│  │                            │
│                     │  └─────┬──────┘  │                            │
│                     │        │         │                            │
│                     │  search tool     │                            │
│                     │  result panel    │                            │
│                     └────────┼─────────┘                            │
└──────────────────────────────┼──────────────────────────────────────┘
                               │ HTTP (localhost)
                   ┌───────────▼───────────┐
                   │   Meilisearch Server   │
                   │   (global, one per     │
                   │    machine)            │
                   │                       │
                   │  Index: cp_{h}_files  │
                   │  Index: cp_{h}_logs   │
                   │  Index: cp_{h2}_files │
                   │  ...per-project       │
                   └───────────────────────┘
                   ~/.context-pilot/meilisearch/
```

### Component Responsibilities

| Component | Owner | Responsibility |
|-----------|-------|----------------|
| Meilisearch server | cp-mod-search | Download binary, start/stop, health checks, port management |
| Indexer thread | cp-mod-search | File watcher → chunking → batch index via Meilisearch HTTP API |
| Splitter chain | cp-mod-search | Tree-sitter AST → fixed-size fallback → custom splitters |
| Search tool | cp-mod-search | Query Meilisearch, create result panels |
| OCR pipeline | cp-mod-search | Detect OCR-able files → Datalab API → text → chunking pipeline |
| Log writes | cp-mod-logs | JSON file writes (source of truth). Unchanged except schema update. |
| Log indexing | cp-mod-search | Watch `.context-pilot/logs/` → parse JSON → index into Meilisearch |

### Coupling

- **cp-mod-search depends on**: `core` (for Module trait)
- **cp-mod-logs depends on**: `core` (unchanged)
- **Zero coupling between search and logs** — search discovers logs via filesystem watching

### Server Lifecycle (Global)

```
~/.context-pilot/meilisearch/
├── bin/meilisearch          # Downloaded binary
├── data/                    # Meilisearch data directory
├── master.key               # Auto-generated API master key
├── port                     # Assigned TCP port
├── pid                      # Server PID
└── projects.json            # Project path → index hash mapping (for orphan cleanup)
```

1. **First project init**: Download Meilisearch binary → generate master key → find free port → start server → save PID + port
2. **Subsequent project init**: Read port file → health check → reuse. If dead: restart.
3. **Orphan cleanup on start**: Read `projects.json` → for each project path, check if directory exists → delete indexes for missing projects
4. **Index creation**: On module init, create `cp_{hash8}_files` and `cp_{hash8}_logs` indexes if they don't exist

### Indexer Thread Architecture

```
┌─────────────────────────────────────────────────┐
│              Indexer Thread                       │
│                                                  │
│  notify::Watcher ──→ mpsc::Receiver             │
│       │                    │                     │
│       │              ┌─────▼──────┐              │
│       │              │ Debounce   │ 200ms        │
│       │              │ Buffer     │              │
│       │              └─────┬──────┘              │
│       │                    │                     │
│       │              ┌─────▼──────┐              │
│       │              │ Filter     │ allowlist    │
│       │              │ & Validate │ + size < 1MB │
│       │              │            │ + !symlink   │
│       │              └─────┬──────┘              │
│       │                    │                     │
│       │              ┌─────▼──────┐              │
│       │              │ Splitter   │ tree-sitter  │
│       │              │ Chain      │ or fixed-sz  │
│       │              └─────┬──────┘              │
│       │                    │                     │
│       │              ┌─────▼──────┐              │
│       │              │ Meilisearch│ HTTP batch   │
│       │              │ Client     │              │
│       │              └─────┬──────┘              │
│       │                    │                     │
│       │              ┌─────▼──────┐              │
│       │              │ Error      │ buffer+retry │
│       │              │ Handler    │ exp backoff  │
│       │              └────────────┘              │
└─────────────────────────────────────────────────┘
```

**Debounce**: After the last FS event, wait 200ms, then batch-process all accumulated changes. Prevents thrashing during `git checkout` or bulk operations.

**Error handling**: If Meilisearch is unreachable, buffer failed `IndexerCmd`s in memory (cap: 1000). Retry every 5s with exponential backoff (5s → 10s → 20s → ... → 5min max). Flush entire buffer on successful reconnect.

**File filtering pipeline**:
1. Check path against hardcoded exclusions (node_modules/, .git/, etc.)
2. Check extension against allowlist (hardcoded + search.toml overrides)
3. Check file size < 1MB
4. Check not a symlink
5. If all pass → split and index

### Indexing Status Overlay (Ctrl+I)

Keyboard shortcut opens a floating overlay (like Ctrl+H config view):

```
┌─ Indexing Status ───────────────────────────┐
│                                             │
│  Server:    http://127.0.0.1:7710 ● online │
│  Version:   Meilisearch v1.12.0            │
│                                             │
│  Files index:   1,234 chunks (456 files)   │
│  Logs index:    89 entries                  │
│                                             │
│  Queue:         0 pending                  │
│  Errors:        0                          │
│  Last indexed:  src/app/mod.rs (2s ago)    │
│                                             │
│  Initial scan:  ████████████████ 100%      │
│                                             │
│  Press Ctrl+I or Esc to dismiss            │
└─────────────────────────────────────────────┘
```

## 4. Data Model

### Files Index: `cp_{hash8}_files`

```json
{
  "id": "src/app/mod.rs:3",
  "file_path": "src/app/mod.rs",
  "content": "pub struct App {\n    state: State,\n    typewriter: TypewriterBuffer,\n    ...\n}",
  "extension": "rs",
  "chunk_type": "struct",
  "chunk_name": "App",
  "line_start": 42,
  "line_end": 85,
  "char_start": 1200,
  "char_end": 3400,
  "last_modified_ms": 1746537600000
}
```

**ID scheme**: `{relative_path}:{chunk_index}` — deterministic, enables clean delete-and-reinsert.

**Meilisearch settings**:
```json
{
  "searchableAttributes": ["content", "chunk_name", "file_path"],
  "filterableAttributes": ["file_path", "extension", "chunk_type"],
  "sortableAttributes": ["last_modified_ms"],
  "rankingRules": ["words", "typo", "proximity", "attribute", "sort", "exactness"],
  "typoTolerance": {
    "enabled": true,
    "minWordSizeForTypos": { "oneTypo": 4, "twoTypos": 8 }
  }
}
```

### Logs Index: `cp_{hash8}_logs`

```json
{
  "id": "L42",
  "content": "Decided to use tree-sitter for chunking",
  "timestamp_ms": 1746537600000,
  "datetime": "2026-05-06T12:00:00Z",
  "worker_id": "w1",
  "importance": "medium",
  "tags": ["architecture", "decision"]
}
```

**Meilisearch settings**:
```json
{
  "searchableAttributes": ["content", "tags"],
  "filterableAttributes": ["timestamp_ms", "importance", "tags", "worker_id"],
  "sortableAttributes": ["timestamp_ms"],
  "rankingRules": ["words", "typo", "proximity", "attribute", "sort", "exactness"]
}
```

### Extension Allowlist (Hardcoded Defaults)

**Code**: `rs`, `py`, `js`, `ts`, `jsx`, `tsx`, `go`, `java`, `c`, `h`, `cpp`, `hpp`, `cc`, `rb`, `php`, `swift`, `kt`, `scala`, `ex`, `exs`, `hs`, `ml`, `lua`, `dart`, `zig`, `nix`, `tf`, `sh`, `bash`, `zsh`, `sql`, `cs`, `fs`, `vb`, `pl`, `pm`, `r`, `jl`, `nim`, `sol`, `v`, `vy`, `move`

**Config/Data**: `toml`, `yaml`, `yml`, `json`, `xml`, `ini`, `cfg`, `conf`, `properties`

**Documentation**: `md`, `txt`, `rst`, `adoc`, `org`, `tex`

**Web**: `html`, `htm`, `css`, `scss`, `sass`, `less`, `svg`

**Build**: `dockerfile`, `makefile`, `cmake`, `gradle`, `sbt`

**Other**: `graphql`, `proto`, `thrift`

**OCR (requires DATALAB_API_KEY)**: `pdf`, `png`, `jpg`, `jpeg`, `tiff`, `bmp`, `webp`

### Hardcoded Path Exclusions (Always Skipped)

`node_modules/`, `.git/`, `vendor/`, `target/`, `dist/`, `build/`, `out/`, `__pycache__/`, `.next/`, `.nuxt/`, `.context-pilot/`, `*.min.js`, `*.min.css`, `*.map`, `*.lock`, `*.sum`

## 5. Module Design

### cp-mod-search (New Crate)

```
crates/cp-mod-search/
├── Cargo.toml
└── src/
    ├── lib.rs              # Module trait impl, init_state, tool defs
    ├── server.rs           # Meilisearch binary download, start/stop, health
    ├── indexer.rs           # Background thread: watcher → batch → API
    ├── client.rs            # Meilisearch HTTP API wrapper (index, search, delete)
    ├── splitter/
    │   ├── mod.rs           # Splitter trait + chain dispatcher
    │   ├── tree_sitter.rs   # AST-based chunking (all available grammars)
    │   └── fixed_size.rs    # 4000-char fallback, split on line boundaries
    ├── ocr.rs               # Datalab API client (upload → poll → text)
    ├── panel.rs             # Search result panel (rich layout)
    ├── tools.rs             # search tool execution
    ├── config.rs            # Extension allowlist, path exclusions, settings
    └── types.rs             # SearchState, Chunk, SearchResult, IndexDoc
```

**SearchState** (stored in module_data TypeMap):
```rust
// Persisted (save_module_data / load_module_data)
#[derive(Serialize, Deserialize)]
struct SearchPersistData {
    server_port: u16,
    master_key: String,
    project_hash: String,          // 8-char hash of project path
    index_ready: bool,             // true once initial indexing complete
}

// Runtime (re-created on load)
struct SearchState {
    persist: SearchPersistData,
    indexer_tx: Sender<IndexerCmd>, // channel to background indexer
    watcher: Option<RecommendedWatcher>,
    error_buffer: Vec<IndexerCmd>,  // failed ops awaiting retry
    retry_backoff_ms: u64,          // current retry interval
}

enum IndexerCmd {
    IndexFile(PathBuf),
    DeleteFile(PathBuf),
    DeleteAllLogs,  // clean-slate migration only
    Shutdown,
}
```

**Splitter trait**:
```rust
trait Splitter: Send + Sync {
    /// Check if this splitter handles the given extension
    fn supports(&self, extension: &str) -> bool;
    /// Split file content into chunks
    fn split(&self, content: &str, path: &Path) -> Vec<Chunk>;
}

struct Chunk {
    content: String,
    chunk_type: String,   // "function", "struct", "class", "raw", etc.
    chunk_name: String,   // name of the semantic unit (or empty for raw)
    line_start: u32,
    line_end: u32,
    char_start: u32,
    char_end: u32,
}

struct SplitterChain {
    splitters: Vec<Box<dyn Splitter>>,  // ordered by priority
}
```

### cp-mod-logs (Changes)

**Removed**:
- `log_summarize` tool + all summarization logic
- `log_toggle` tool + expand/collapse logic
- Fixed LOGS panel (log display moves to search results)
- `parent_id`, `children` fields from log types

**Updated**:
- `log_create`: Add `tags` (array of strings), `importance` (enum) params
- `Close_conversation_history`: Add `tags`, `importance` to log creation params
- Log JSON schema: `{id, content, timestamp_ms, datetime, worker_id, importance, tags}`

**Unchanged**:
- JSON file storage (dual-write source of truth)
- `build_log_write_ops()` for PersistenceWriter
- Chunk-based file storage (`chunk_{n}.json`)

### Migration (Clean Slate)

On first boot with cp-mod-search active:
1. Delete all files in `.context-pilot/logs/`
2. Reset `next_id.json` to `{"next_id": 1}`
3. Send `DeleteAllLogs` to indexer (clears Meilisearch log index)
4. Log migration is complete — new logs use new schema

## 6. Tools & UX

### `search` Tool Definition

```yaml
search:
  short_desc: "Search across project files and logs using Meilisearch full-text search"
  description: |
    Search the project codebase and/or logs. Files are automatically indexed
    in the background using tree-sitter AST chunking (semantic: functions,
    structs, classes) with a character-based fallback for unsupported languages.
    Logs are indexed with tags, importance, and full-text content.
    
    Results appear in a dynamic search panel (YAML-formatted, like brave_search).
    Use include_context=false for "peek" searches that don't consume context tokens.
    
    The extension allowlist is configurable in .context-pilot/search.toml.
    Press Ctrl+I to view indexing status, queue depth, and errors.
    
  params:
    query:
      type: string
      required: true
      description: "Search query. Supports natural language and exact phrases."
    scope:
      type: string
      enum: ["all", "project", "logs"]
      default: "all"
      description: "Where to search. 'all' searches both indexes."
    path_prefix:
      type: string
      description: "Filter files by path prefix (e.g., 'src/app/'). Project scope only."
    extension:
      type: string
      description: "Filter files by extension (e.g., 'rs', 'py'). Project scope only."
    sort:
      type: string
      enum: ["relevance", "date_asc", "date_desc"]
      default: "relevance"
      description: "Sort order. 'date_*' sorts by timestamp (logs) or last_modified (files)."
    from_date:
      type: string
      description: "ISO 8601 date. Only results after this date."
    to_date:
      type: string
      description: "ISO 8601 date. Only results before this date."
    include_context:
      type: boolean
      default: true
      description: "If true, results are included in the tool response (YAML). If false, results appear only in the panel ('peek' mode — saves context tokens)."
    limit:
      type: integer
      default: 20
      description: "Max results per scope (1-50)."
```

### Updated `log_create` Tool

```yaml
log_create:
  params:
    entries:
      type: array
      items:
        content:
          type: string
          required: true
          description: "Short, atomic log entry"
        importance:
          type: string
          enum: ["low", "medium", "high", "critical"]
          default: "medium"
          description: "Importance level"
        tags:
          type: array
          items: { type: string }
          description: "Freeform tags for categorization (e.g., ['decision', 'architecture'])"
```

### Updated `Close_conversation_history` Tool

Add `importance` and `tags` to the `logs` array items (same schema as `log_create`).

### Search Result Panel (Rich Layout)

```
======= [P19] Search: "tree-sitter chunking" (12 results) =======

── Project Files (8) ──────────────────────────────────────────────

 1  src/splitter/tree_sitter.rs [function] parse_and_split
    Lines 45–120 · rs · Modified 2h ago
    ...uses **tree-sitter** to parse the file into AST nodes
    and extract semantic **chunks** for indexing...

 2  src/splitter/mod.rs [struct] SplitterChain
    Lines 10–25 · rs · Modified 1d ago
    ...manages the **chunking** pipeline with **tree-sitter**
    as the primary splitter, falling back to fixed-size...

 3  docs/design-meilisearch.md [raw] chunk 4
    Lines 180–220 · md · Modified 30m ago
    ...Chunking: **Tree-sitter** AST — parse code into
    semantic **chunks** (functions, structs, classes)...

── Logs (4) ────────────────────────────────────────────────────────

 4  [L42] 2026-05-06 12:00 · medium · #architecture #decision
    Decided to use **tree-sitter** for **chunking**

 5  [L38] 2026-05-06 11:45 · high · #design
    Explored **chunking** strategies: AST vs fixed-size vs
    line-level. **Tree-sitter** chosen for precision.
```

### Config File: `.context-pilot/search.toml`

```toml
# Extra extensions to index (added to hardcoded defaults)
extra_extensions = ["vue", "svelte", "astro"]

# Chunk size for fixed-size fallback (chars)
fallback_chunk_size = 4000

# OCR: set DATALAB_API_KEY in .env to enable
# ocr_extensions are only active when the API key is present
```

## 7. Implementation Plan

### Phase 1 — Foundation
- [ ] Create `crates/cp-mod-search/` crate with Cargo.toml
- [ ] Register module in `src/modules/mod.rs` (22 → 23 modules)
- [ ] `server.rs`: GitHub Releases API check for latest Meilisearch version
- [ ] `server.rs`: Meilisearch binary download (platform detection: macOS arm64/x86, Linux amd64/arm64)
- [ ] `server.rs`: Start/stop, PID management, health check (`GET /health`, retry 500ms × 30)
- [ ] `server.rs`: Port assignment (find free port, save to `~/.context-pilot/meilisearch/port`)
- [ ] `server.rs`: Master key generation (random 32-byte, base64) + storage
- [ ] `client.rs`: HTTP wrapper for index CRUD, document CRUD, search, settings
- [ ] `types.rs`: SearchState (persist + runtime split), SearchPersistData, Chunk, IndexDoc, SearchResult, SearchMetrics
- [ ] `lib.rs`: Module trait impl skeleton, init_state (server start + index creation)
- [ ] `lib.rs`: save_module_data / load_module_data (persist port, key, hash, index_ready; re-create watcher, indexer thread, channel on load)

### Phase 2 — File Indexing
- [ ] `splitter/mod.rs`: Splitter trait + SplitterChain
- [ ] `splitter/fixed_size.rs`: 4000-char fallback (split on line boundaries)
- [ ] `splitter/tree_sitter.rs`: AST chunking (start with Rust, Python, JS/TS, Go, Java, C/C++)
- [ ] `config.rs`: Extension allowlist (hardcoded + search.toml override), path exclusions, 1MB file size cap
- [ ] `indexer.rs`: Background thread with mpsc channel, notify::Watcher (skip symlinks)
- [ ] `indexer.rs`: 200ms debounce — collect FS events, batch-process after quiet period
- [ ] `indexer.rs`: File filtering pipeline (exclusions → allowlist → size cap → symlink check)
- [ ] `indexer.rs`: File change → filter → split → batch index
- [ ] `indexer.rs`: Initial full-project scan on first boot
- [ ] `indexer.rs`: Delete + re-insert on file modification
- [ ] `indexer.rs`: Delete on file removal
- [ ] `indexer.rs`: Error buffer (cap 1000) + exponential backoff retry (5s → 5min max)
- [ ] `indexer.rs`: Expose SearchMetrics (indexed count, queue depth, errors, last activity)

### Phase 3 — Search Tool + Panel
- [ ] `yamls/tools/search.yaml`: Tool description text
- [ ] `tools.rs`: search tool execution (query → Meilisearch API → results)
- [ ] `tools.rs`: Scope routing (all → multi-index, project → files index, logs → logs index)
- [ ] `tools.rs`: Filter translation (path_prefix/extension → Meilisearch native filters, date range → timestamp filter)
- [ ] `tools.rs`: `include_context` param — true: YAML results in tool_result + panel, false: panel only ("peek" mode)
- [ ] `panel.rs`: SearchResultPanel with rich layout (file results + log results sections, YAML-formatted like brave_search)
- [ ] `panel.rs`: Highlighted matching terms in snippets (Meilisearch `_formatted` fields)
- [ ] Tool visualizer for search results
- [ ] `src/app/events.rs`: Ctrl+I → Action::ToggleIndexOverlay
- [ ] `src/ui/`: Indexing status overlay renderer (reads SearchMetrics from module_data)

### Phase 4 — Log Migration
- [ ] Update cp-mod-logs: remove log_summarize, log_toggle tools
- [ ] Update cp-mod-logs: remove LOGS fixed panel
- [ ] Update cp-mod-logs: add tags + importance to log_create params
- [ ] Update cp-mod-logs: add tags + importance to Close_conversation_history
- [ ] Update cp-mod-logs: new JSON schema (drop parent_id/children, add tags/importance/datetime)
- [ ] Clean slate migration: wipe `.context-pilot/logs/` on first boot with search active
- [ ] `indexer.rs`: Watch `.context-pilot/logs/` → parse chunk files → index into Meilisearch
- [ ] Update `yamls/tools/logs.yaml` (remove summarize/toggle descriptions)

### Phase 5 — OCR Pipeline
- [ ] `ocr.rs`: Datalab API client (upload file → poll for result → extract text)
- [ ] `ocr.rs`: Rate limiting + error handling + retry
- [ ] Integrate into indexer: detect OCR extensions → Datalab API → text → splitter → index
- [ ] Only active when `DATALAB_API_KEY` is set (graceful skip otherwise)

### Phase 6 — Polish & Expansion
- [ ] Orphan index cleanup on server start
- [ ] Expand tree-sitter to all available grammars
- [ ] `projects.json` management (register/unregister projects)
- [ ] Performance tuning: batch sizes, debounce intervals for file watcher
- [ ] Documentation in `docs/search-module.md`
- [ ] Config file documentation
- [ ] Update overview module to show search index stats
