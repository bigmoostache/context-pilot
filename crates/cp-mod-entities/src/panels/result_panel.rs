//! Entity result panel — dynamic panel for large SQL query results.
//!
//! Supports both static (one-shot) and live (auto-refreshing) panels.
//! Live panels store the SQL query in metadata and re-execute it every 2 seconds
//! via the background cache thread.

use cp_base::panels::{
    CacheRequest, CacheUpdate, ContextItem, Panel, hash_content, paginate_content, update_if_changed,
};
use cp_base::state::context::{Entry, Kind, compute_total_pages, estimate_tokens, make_default_entry};
use cp_base::state::runtime::State;
use cp_render::{Block, Semantic, Span};

use crate::db;

/// Context type identifier for entity result panels.
pub(crate) const ENTITY_RESULT_TYPE: &str = "entity_result";

/// Metadata key used to persist panel content across reloads.
const META_CONTENT: &str = "result_content";

/// Metadata key: SQL query for live-refreshing panels.
const META_SQL: &str = "live_sql";

/// Metadata key: path to the `SQLite` database file for live panels.
const META_DB_PATH: &str = "live_db_path";

/// Metadata key: boolean flag — `true` for auto-refreshing live panels.
const META_IS_LIVE: &str = "is_live";

// =============================================================================
// Panel creation helpers
// =============================================================================

/// Create a dynamic entity result panel with the given content.
///
/// Returns the panel ID string (e.g., "P15").
pub(crate) fn create_result_panel(state: &mut State, title: &str, content: &str) -> String {
    create_panel_inner(state, title, content, None)
}

/// Optional metadata for a live (auto-refreshing) result panel.
pub(crate) struct LivePanelMeta<'sql> {
    /// SQL query to re-execute periodically.
    pub sql: &'sql str,
    /// Path to the `SQLite` database file.
    pub db_path: &'sql str,
}

/// Create a live (auto-refreshing) entity result panel.
///
/// Stores the SQL query and database path in metadata so the background cache
/// thread can re-execute the query periodically. Returns the panel ID.
pub(crate) fn create_live_result_panel(
    state: &mut State,
    title: &str,
    content: &str,
    live: LivePanelMeta<'_>,
) -> String {
    create_panel_inner(state, title, content, Some(live))
}

/// Shared implementation for creating static and live result panels.
fn create_panel_inner(state: &mut State, title: &str, content: &str, live: Option<LivePanelMeta<'_>>) -> String {
    let panel_id = state.next_available_context_id();
    let uid = format!("UID_{}_P", state.global_next_uid);
    state.global_next_uid = state.global_next_uid.saturating_add(1);

    let mut elem = make_default_entry(&panel_id, Kind::new(ENTITY_RESULT_TYPE), title, false);
    elem.uid = Some(uid);
    elem.cached_content = Some(content.to_string());
    elem.token_count = estimate_tokens(content);
    elem.full_token_count = elem.token_count;
    elem.total_pages = compute_total_pages(elem.token_count);
    elem.source_hash = Some(hash_content(content));

    // Persist content for fallback restore on reload
    drop(elem.metadata.insert(META_CONTENT.to_string(), serde_json::Value::String(content.to_string())));

    // Live refresh metadata (only for live panels)
    if let Some(meta) = live {
        drop(elem.metadata.insert(META_SQL.to_string(), serde_json::Value::String(meta.sql.to_string())));
        drop(elem.metadata.insert(META_DB_PATH.to_string(), serde_json::Value::String(meta.db_path.to_string())));
        drop(elem.metadata.insert(META_IS_LIVE.to_string(), serde_json::Value::Bool(true)));
    }

    state.context.push(elem);
    panel_id
}

// =============================================================================
// Live query refresh (runs on background cache thread)
// =============================================================================

/// Re-execute a live query and return a cache update if the result changed.
fn refresh_live_query(req: LiveQueryRequest) -> Option<CacheUpdate> {
    let db_path = std::path::PathBuf::from(&req.db_path);
    let conn = db::open(&db_path).ok()?;

    let content =
        crate::format::query_to_markdown(&conn, &req.sql, None).unwrap_or_else(|e| format!("Query error: {e}"));

    // Hash-based change detection (same pattern as file/console panels)
    let new_hash = hash_content(&content);
    if req.current_source_hash.as_ref() == Some(&new_hash) {
        return Some(CacheUpdate::Unchanged { context_id: req.context_id });
    }

    let token_count = estimate_tokens(&content);
    Some(CacheUpdate::Content { context_id: req.context_id, content, token_count })
}

// =============================================================================
// Panel implementation
// =============================================================================

/// Cache request: restore content from metadata after reload.
struct EntityRestoreRequest {
    /// Panel context ID (e.g., "P15").
    context_id: String,
    /// Full panel content to restore.
    content: String,
}

/// Cache request for live panels — re-executes SQL on background thread.
struct LiveQueryRequest {
    /// Panel context ID (e.g., "P15").
    context_id: String,
    /// SQL query to re-execute periodically.
    sql: String,
    /// Path to the `SQLite` database file.
    db_path: String,
    /// Hash of the currently cached content (for change detection).
    current_source_hash: Option<String>,
}

/// Panel renderer for entity SQL result panels.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EntityResultPanel;

impl Panel for EntityResultPanel {
    fn needs_cache(&self) -> bool {
        true
    }

    fn build_cache_request(&self, ctx: &Entry, _state: &State) -> Option<CacheRequest> {
        let is_live = ctx.metadata.get(META_IS_LIVE).and_then(serde_json::Value::as_bool).unwrap_or(false);

        if is_live {
            // Live panel — re-execute SQL on every cache cycle
            let sql = ctx.metadata.get(META_SQL)?.as_str()?;
            let db_path = ctx.metadata.get(META_DB_PATH)?.as_str()?;
            Some(CacheRequest {
                context_type: Kind::new(ENTITY_RESULT_TYPE),
                data: Box::new(LiveQueryRequest {
                    context_id: ctx.id.clone(),
                    sql: sql.to_string(),
                    db_path: db_path.to_string(),
                    current_source_hash: ctx.source_hash.clone(),
                }),
            })
        } else {
            // Static panel — only restore if content missing (post-reload)
            if ctx.cached_content.is_some() {
                return None;
            }
            let content = ctx.metadata.get(META_CONTENT)?.as_str()?;
            Some(CacheRequest {
                context_type: Kind::new(ENTITY_RESULT_TYPE),
                data: Box::new(EntityRestoreRequest { context_id: ctx.id.clone(), content: content.to_string() }),
            })
        }
    }

    fn apply_cache_update(&self, update: CacheUpdate, ctx: &mut Entry, _state: &mut State) -> bool {
        if let CacheUpdate::Content { content, token_count, .. } = update {
            ctx.source_hash = Some(hash_content(&content));
            ctx.cached_content = Some(content.clone());
            ctx.full_token_count = token_count;
            ctx.total_pages = compute_total_pages(token_count);
            ctx.current_page = 0;
            if ctx.total_pages > 1 {
                let page_content =
                    paginate_content(ctx.cached_content.as_deref().unwrap_or(""), ctx.current_page, ctx.total_pages);
                ctx.token_count = estimate_tokens(&page_content);
            } else {
                ctx.token_count = token_count;
            }
            ctx.cache_deprecated = false;
            let _ = update_if_changed(ctx, &content);
            true
        } else {
            false
        }
    }

    fn refresh_cache(&self, request: CacheRequest) -> Option<CacheUpdate> {
        // Try live query first
        let data = match request.data.downcast::<LiveQueryRequest>() {
            Ok(req) => return refresh_live_query(*req),
            Err(remaining) => remaining,
        };
        // Fall back to static restore from metadata
        let req = data.downcast::<EntityRestoreRequest>().ok()?;
        let token_count = estimate_tokens(&req.content);
        Some(CacheUpdate::Content { context_id: req.context_id.clone(), content: req.content.clone(), token_count })
    }

    fn handle_key(&self, key: &crossterm::event::KeyEvent, _state: &State) -> Option<cp_base::state::actions::Action> {
        cp_base::panels::scroll_key_action(key)
    }

    fn blocks(&self, state: &State) -> Vec<Block> {
        let ctx = state.context.get(state.selected_context).filter(|c| c.context_type == Kind::new(ENTITY_RESULT_TYPE));

        let Some(ctx) = ctx else {
            return vec![Block::styled_text("No entity result panel".into(), Semantic::Muted)];
        };

        let Some(content) = &ctx.cached_content else {
            return vec![Block::Line(vec![Span::muted("Loading...".into()).italic()])];
        };

        content.lines().map(|line| Block::text(format!(" {line}"))).collect()
    }

    fn title(&self, state: &State) -> String {
        state.context.get(state.selected_context).map_or_else(|| "Entity Result".to_string(), |ctx| ctx.name.clone())
    }

    fn max_freezes(&self) -> u8 {
        3
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        state
            .context
            .iter()
            .filter(|c| c.context_type == Kind::new(ENTITY_RESULT_TYPE))
            .filter_map(|c| {
                let content = c.cached_content.as_ref()?;
                let output = paginate_content(content, c.current_page, c.total_pages);
                Some(ContextItem::new(&c.id, &c.name, output, c.last_refresh_ms))
            })
            .collect()
    }

    fn refresh(&self, _state: &mut State) {}

    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        // Live panels re-execute SQL every 2s. Static panels return None from
        // build_cache_request (cached_content already set), so no work is done.
        Some(2000)
    }

    fn suicide(&self, _ctx: &Entry, _state: &State) -> bool {
        false
    }
}
