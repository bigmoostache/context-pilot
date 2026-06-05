//! Dynamic search result panel + YAML result formatting.
//!
//! Displays formatted Meilisearch results (file chunks + log entries)
//! in a scrollable panel.  Content is persisted in metadata so it
//! survives TUI reloads.
//!
//! The `format_results` function builds the YAML string consumed by
//! both the panel and the tool result when `hide_contents` is true.

use std::collections::BTreeMap;

use crossterm::event::KeyEvent;

use cp_base::panels::{
    CacheRequest, CacheUpdate, ContextItem, Panel, paginate_content, scroll_key_action, update_if_changed,
};
use cp_base::state::actions::Action;
use cp_base::state::context::{Entry, Kind, compute_total_pages, estimate_tokens};
use cp_base::state::runtime::State;

use crate::types::SearchResult;

/// Context type identifier for search result panels.
pub(crate) const SEARCH_PANEL_TYPE: &str = "search_result";

/// Metadata key used to persist panel content across reloads.
const META_CONTENT: &str = "result_content";

/// Panel renderer for search result panels.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SearchResultPanel;

/// Cache request for restoring content from metadata after reload.
struct RestoreRequest {
    /// Panel context ID (e.g. `"P15"`).
    context_id: String,
    /// The full search result content to restore.
    content: String,
}

impl Panel for SearchResultPanel {
    fn needs_cache(&self) -> bool {
        true
    }

    fn build_cache_request(&self, ctx: &Entry, _state: &State) -> Option<CacheRequest> {
        // Only restore if cached_content was cleared (post-reload)
        if ctx.cached_content.is_some() {
            return None;
        }
        let content = ctx.metadata.get(META_CONTENT)?.as_str()?;
        Some(CacheRequest {
            context_type: Kind::new(SEARCH_PANEL_TYPE),
            data: Box::new(RestoreRequest { context_id: ctx.id.clone(), content: content.to_string() }),
        })
    }

    fn apply_cache_update(&self, update: CacheUpdate, ctx: &mut Entry, _state: &mut State) -> bool {
        if let CacheUpdate::Content { content, token_count, .. } = update {
            ctx.cached_content = Some(content.clone());
            ctx.full_token_count = token_count;
            ctx.total_pages = compute_total_pages(token_count);
            ctx.current_page = 0;
            if ctx.total_pages > 1 {
                let page =
                    paginate_content(ctx.cached_content.as_deref().unwrap_or(""), ctx.current_page, ctx.total_pages);
                ctx.token_count = estimate_tokens(&page);
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
        let req = request.data.downcast::<RestoreRequest>().ok()?;
        let token_count = estimate_tokens(&req.content);
        Some(CacheUpdate::Content { context_id: req.context_id.clone(), content: req.content.clone(), token_count })
    }

    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        scroll_key_action(key)
    }

    fn blocks(&self, state: &State) -> Vec<cp_render::Block> {
        use cp_render::{Block, Semantic, Span};

        let ctx = state.context.get(state.selected_context).filter(|c| c.context_type == Kind::new(SEARCH_PANEL_TYPE));

        let Some(ctx) = ctx else {
            return vec![Block::styled_text(" No search result panel".into(), Semantic::Muted)];
        };

        let Some(content) = &ctx.cached_content else {
            return vec![Block::Line(vec![Span::muted(" Loading...".into()).italic()])];
        };

        content.lines().map(|line| Block::text(format!(" {line}"))).collect()
    }

    fn title(&self, state: &State) -> String {
        state.context.get(state.selected_context).map_or_else(|| "Search Results".to_string(), |ctx| ctx.name.clone())
    }

    fn max_freezes(&self) -> u8 {
        0
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        state
            .context
            .iter()
            .filter(|c| c.context_type == Kind::new(SEARCH_PANEL_TYPE))
            .filter_map(|c| {
                let content = c.cached_content.as_ref()?;
                let output = paginate_content(content, c.current_page, c.total_pages);
                Some(ContextItem::new(&c.id, &c.name, output, c.last_refresh_ms))
            })
            .collect()
    }

    fn refresh(&self, _state: &mut State) {}

    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        None
    }

    fn suicide(&self, _ctx: &Entry, _state: &State) -> bool {
        false
    }
}

/// Visualizer for search tool results.
///
/// Highlights file paths, section headers, importance levels, and tags
/// in the conversation view.
pub(crate) fn visualize_search_output(content: &str, width: usize) -> Vec<cp_render::Block> {
    use cp_render::{Block, Semantic, Span};

    content
        .lines()
        .map(|line| {
            if line.is_empty() {
                return Block::empty();
            }

            // Truncate long lines
            let display = if line.len() > width {
                format!("{}...", line.get(..line.floor_char_boundary(width.saturating_sub(3))).unwrap_or(""))
            } else {
                line.to_string()
            };

            let semantic = if line.starts_with("Results for") || line.starts_with("No results") {
                Semantic::Info
            } else if line.starts_with("---") && line.ends_with("---") {
                Semantic::Header
            } else if line.starts_with("Error") || line.contains("[critical]") {
                Semantic::Error
            } else if line.contains("[high]") {
                Semantic::Warning
            } else if line.contains("[low]") {
                Semantic::Muted
            } else if line.starts_with(|c: char| c.is_ascii_digit()) && line.contains(":[") {
                // File result line like "1. src/main.rs:15-42 [function: run]"
                Semantic::Success
            } else {
                Semantic::Default
            };

            Block::Line(vec![Span::styled(display, semantic)])
        })
        .collect()
}

// ─── Result formatting ──────────────────────────────────────────────────────

/// A single entity search hit from the Meilisearch entities index.
pub(crate) struct EntityHit {
    /// Table name (e.g. `"companies"`).
    pub table: String,
    /// YAML-formatted row content from `_all_text`.
    pub all_text: String,
    /// Meilisearch ranking score (0.0–1.0).
    pub score: Option<f64>,
}

/// Bundled search results passed to [`format_results`].
///
/// Groups the three result categories to avoid exceeding the 4-argument limit.
pub(crate) struct SearchOutput<'results> {
    /// File chunk results from the files index.
    pub files: &'results [SearchResult],
    /// Log entry results from the logs index.
    pub logs: &'results [SearchResult],
    /// Entity row results from the entities index.
    pub entities: &'results [EntityHit],
}

/// Format search results as YAML for panel display.
///
/// File results are grouped by path. All metadata is included.
/// Uses `serde_yaml` for consistent formatting matching the brave module style.
pub(crate) fn format_results(query: &str, output: &SearchOutput<'_>, hide_contents: bool) -> String {
    let total = output.files.len().saturating_add(output.logs.len()).saturating_add(output.entities.len());

    let mut root = serde_json::Map::new();
    drop(root.insert("query".into(), serde_json::Value::String(query.to_string())));
    drop(root.insert("total_results".into(), serde_json::json!(total)));

    // -- File results, grouped by path ---------------------------------------

    if !output.files.is_empty() {
        let mut by_path: BTreeMap<String, Vec<&SearchResult>> = BTreeMap::new();
        for r in output.files {
            let path = r.file_path.as_deref().unwrap_or("unknown").to_string();
            by_path.entry(path).or_default().push(r);
        }

        let mut files_arr: Vec<serde_json::Value> = Vec::new();
        for (path, chunks) in &by_path {
            let ext = chunks.first().and_then(|c| c.extension.as_deref()).unwrap_or("");
            let mut file_obj = serde_json::Map::new();
            drop(file_obj.insert("path".into(), serde_json::Value::String(path.clone())));
            drop(file_obj.insert("extension".into(), serde_json::Value::String(ext.to_string())));

            let chunks_arr: Vec<serde_json::Value> =
                chunks.iter().map(|chunk| build_chunk_value(chunk, hide_contents)).collect();

            drop(file_obj.insert("chunks".into(), serde_json::Value::Array(chunks_arr)));
            files_arr.push(serde_json::Value::Object(file_obj));
        }
        drop(root.insert("files".into(), serde_json::Value::Array(files_arr)));
    }

    // -- Log results ---------------------------------------------------------

    if !output.logs.is_empty() {
        let logs_arr: Vec<serde_json::Value> = output.logs.iter().map(|r| build_log_value(r, hide_contents)).collect();
        drop(root.insert("logs".into(), serde_json::Value::Array(logs_arr)));
    }

    // -- Entity results -------------------------------------------------------

    if !output.entities.is_empty() {
        let entities_arr: Vec<serde_json::Value> =
            output.entities.iter().map(|r| build_entity_value(r, hide_contents)).collect();
        drop(root.insert("entities".into(), serde_json::Value::Array(entities_arr)));
    }

    // -- Serialize to YAML ---------------------------------------------------

    serde_yaml::to_string(&serde_json::Value::Object(root)).unwrap_or_else(|_| "# Failed to serialize results\n".into())
}

/// Build a JSON value for a single file chunk.
fn build_chunk_value(chunk: &SearchResult, hide_contents: bool) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    drop(
        obj.insert("type".into(), serde_json::Value::String(chunk.chunk_type.as_deref().unwrap_or("raw").to_string())),
    );
    if let Some(ref name) = chunk.chunk_name
        && !name.is_empty()
    {
        drop(obj.insert("name".into(), serde_json::Value::String(name.clone())));
    }
    if let Some(start) = chunk.line_start {
        drop(obj.insert("line_start".into(), serde_json::json!(start)));
    }
    if let Some(end) = chunk.line_end {
        drop(obj.insert("line_end".into(), serde_json::json!(end)));
    }
    if let Some(score) = chunk.ranking_score {
        drop(obj.insert("relevance".into(), serde_json::json!(format!("{score:.4}"))));
    }
    if !chunk.content.is_empty() && !hide_contents {
        drop(obj.insert("content".into(), serde_json::Value::String(chunk.content.clone())));
    }
    serde_json::Value::Object(obj)
}

/// Build a JSON value for a single log result.
fn build_log_value(r: &SearchResult, hide_contents: bool) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    if let Some(ref id) = r.log_id {
        drop(obj.insert("id".into(), serde_json::Value::String(id.clone())));
    }
    if let Some(ref dt) = r.datetime {
        drop(obj.insert("datetime".into(), serde_json::Value::String(dt.clone())));
    }
    if let Some(ref imp) = r.importance {
        drop(obj.insert("importance".into(), serde_json::Value::String(imp.clone())));
    }
    if let Some(score) = r.ranking_score {
        drop(obj.insert("relevance".into(), serde_json::json!(format!("{score:.4}"))));
    }
    if !r.content.is_empty() && !hide_contents {
        drop(obj.insert("content".into(), serde_json::Value::String(r.content.clone())));
    }
    serde_json::Value::Object(obj)
}

/// Build a JSON value for a single entity search hit.
fn build_entity_value(hit: &EntityHit, hide_contents: bool) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    drop(obj.insert("table".into(), serde_json::Value::String(hit.table.clone())));
    if let Some(score) = hit.score {
        drop(obj.insert("relevance".into(), serde_json::json!(format!("{score:.4}"))));
    }
    if !hit.all_text.is_empty() && !hide_contents {
        drop(obj.insert("content".into(), serde_json::Value::String(hit.all_text.clone())));
    }
    serde_json::Value::Object(obj)
}
