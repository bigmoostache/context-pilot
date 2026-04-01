use crossterm::event::KeyEvent;

use cp_base::panels::{CacheRequest, CacheUpdate};
use cp_base::panels::{ContextItem, Panel, paginate_content};
use cp_base::state::actions::Action;
use cp_base::state::context::{Entry, Kind, compute_total_pages, estimate_tokens};
use cp_base::state::runtime::State;
use cp_base::ui::{find_children_pattern, find_size_pattern};

use crate::types::TreeState;
use cp_base::panels::scroll_key_action;

/// Data payload sent to the background cache worker for tree generation.
pub(crate) struct TreeCacheRequest {
    /// Context element identifier.
    pub context_id: String,
    /// Gitignore-style filter applied to the tree.
    pub tree_filter: String,
    /// Paths of currently expanded folders.
    pub tree_open_folders: Vec<String>,
    /// File/folder description annotations.
    pub tree_descriptions: Vec<crate::types::TreeFileDescription>,
}

/// Panel that renders the directory tree in the sidebar.
pub(crate) struct TreePanel;

impl Panel for TreePanel {
    fn needs_cache(&self) -> bool {
        true
    }

    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        scroll_key_action(key)
    }

    fn blocks(&self, state: &State) -> Vec<cp_render::Block> {
        use cp_render::{Block, Semantic, Span as S};

        let tree_content = state
            .context
            .iter()
            .find(|c| c.context_type.as_str() == Kind::TREE)
            .and_then(|ctx| ctx.cached_content.as_ref())
            .cloned()
            .unwrap_or_else(|| "Loading...".to_string());

        let mut blocks = Vec::new();
        for line in tree_content.lines() {
            // Split off description suffix (" - ...")
            let (main_line, description) = line
                .find(" - ")
                .map_or((line, None), |idx| (line.get(..idx).unwrap_or(""), Some(line.get(idx..).unwrap_or(""))));

            let mut spans = vec![S::new(" ".into())];

            if let Some(size_start) = find_size_pattern(main_line) {
                let (before, size_part) = main_line.split_at(size_start);
                spans.push(S::new(before.to_string()));
                spans.push(S::styled(size_part.to_string(), Semantic::AccentDim));
            } else if let Some((start, end)) = find_children_pattern(main_line) {
                let before = main_line.get(..start).unwrap_or("");
                let children = main_line.get(start..end).unwrap_or("");
                let after = main_line.get(end..).unwrap_or("");
                spans.push(S::new(before.to_string()));
                spans.push(S::accent(children.to_string()));
                if !after.is_empty() {
                    spans.push(S::new(after.to_string()));
                }
            } else {
                spans.push(S::new(main_line.to_string()));
            }

            if let Some(desc) = description {
                spans.push(S::muted(desc.to_string()));
            }

            blocks.push(Block::Line(spans));
        }
        blocks
    }
    fn title(&self, _state: &State) -> String {
        "Directory Tree".to_string()
    }

    fn build_cache_request(&self, ctx: &Entry, state: &State) -> Option<CacheRequest> {
        let ts = TreeState::get(state);
        Some(CacheRequest {
            context_type: Kind::new(Kind::TREE),
            data: Box::new(TreeCacheRequest {
                context_id: ctx.id.clone(),
                tree_filter: ts.filter.clone(),
                tree_open_folders: ts.open_folders.clone(),
                tree_descriptions: ts.descriptions.clone(),
            }),
        })
    }

    fn apply_cache_update(&self, update: CacheUpdate, ctx: &mut Entry, _state: &mut State) -> bool {
        let CacheUpdate::Content { content, token_count, .. } = update else {
            return false;
        };
        ctx.cache_deprecated = false;
        // Check if content actually changed before updating
        if !cp_base::panels::update_if_changed(ctx, &content) && ctx.cached_content.is_some() {
            return false;
        }
        ctx.cached_content = Some(content);
        ctx.token_count = token_count;
        ctx.total_pages = compute_total_pages(token_count);
        ctx.current_page = 0;
        true
    }

    fn refresh(&self, _state: &mut State) {
        // Tree refresh is handled by background cache system via refresh_cache
    }

    fn refresh_cache(&self, request: CacheRequest) -> Option<CacheUpdate> {
        let req = request.data.downcast::<TreeCacheRequest>().ok()?;
        let TreeCacheRequest { context_id, tree_filter, tree_open_folders, tree_descriptions } = *req;
        let content = crate::tools::generate_tree_string(&tree_filter, &tree_open_folders, &tree_descriptions);
        let token_count = estimate_tokens(&content);
        Some(CacheUpdate::Content { context_id, content, token_count })
    }

    fn max_freezes(&self) -> u8 {
        3
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        // Find tree context and use cached content
        for ctx in &state.context {
            if ctx.context_type.as_str() == Kind::TREE {
                if let Some(content) = &ctx.cached_content
                    && !content.is_empty()
                {
                    let output = paginate_content(content, ctx.current_page, ctx.total_pages);
                    return vec![ContextItem::new(&ctx.id, "Directory Tree", output, ctx.last_refresh_ms)];
                }
                break;
            }
        }
        Vec::new()
    }

    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        None
    }
    fn suicide(&self, _ctx: &Entry, _state: &State) -> bool {
        false
    }
}
